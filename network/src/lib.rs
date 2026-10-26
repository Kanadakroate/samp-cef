use quinn::{Connecting, Connection, Endpoint};

use slotmap::{SecondaryMap, SlotMap, new_key_type};
use std::net::SocketAddr;
use tokio::{
    runtime::Runtime,
    sync::{
        mpsc::{self, UnboundedReceiver as Recv, UnboundedSender as Sender},
        oneshot,
    },
};

mod client;
mod server;

new_key_type! {
    pub struct PeerId;
}

const INCOMING_PACKET_SIZE: usize = 64 * 1024; // bounded native wire envelope

pub enum CertStrategy {
    FromFiles,
}

pub enum Event {
    Connected(PeerId, SocketAddr),
    Message(PeerId, Vec<u8>),
    Disconnect(PeerId, SocketAddr),
    ConnectionError(PeerId),
}

#[derive(Debug)]
enum WorkerEvent {
    Connected(
        Connection,
        oneshot::Sender<PeerId>,
        mpsc::Sender<Vec<u8>>,
        Option<PeerId>,
    ),
    Message(PeerId, Vec<u8>),
    Disconnect(PeerId),
    ConnectionError(PeerId),
}

#[derive(Debug)]
enum Command {
    Connect(SocketAddr, PeerId),
    Close,
}

struct ActiveConnection {
    connection: Connection,
    msg_tx: mpsc::Sender<Vec<u8>>,
}

pub struct Socket {
    _runtime: Runtime,
    cmd_tx: Sender<Command>,
    event_rx: crossbeam_channel::Receiver<WorkerEvent>,
    peers_id: SlotMap<PeerId, ()>,
    peers: SecondaryMap<PeerId, ActiveConnection>,
}

impl Socket {
    pub fn new_client(addr: SocketAddr) -> anyhow::Result<Self> {
        let runtime = Runtime::new()?;
        let _guard = runtime.enter();

        let endpoint = client::make_pinned_client(addr)?;

        Ok(Self::setup(runtime, endpoint, false))
    }

    pub fn new_server(addr: SocketAddr, _cert: CertStrategy) -> anyhow::Result<Self> {
        let runtime = Runtime::new()?;
        let _guard = runtime.enter();

        let endpoint = server::make_from_files(addr)?;

        Ok(Self::setup(runtime, endpoint, true))
    }

    fn setup(runtime: Runtime, endpoint: Endpoint, is_listening: bool) -> Self {
        let (event_tx, event_rx) = crossbeam_channel::bounded(1024);
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        runtime.spawn(worker_task(endpoint, cmd_rx, event_tx, is_listening));

        Self {
            _runtime: runtime,
            cmd_tx,
            event_rx,
            peers_id: SlotMap::with_key(),
            peers: SecondaryMap::new(),
        }
    }

    pub fn connect(&mut self, addr: SocketAddr) -> PeerId {
        let peer_id = self.peers_id.insert(());

        let _ = self.cmd_tx.send(Command::Connect(addr, peer_id));

        peer_id
    }

    pub fn disconnect(&self, peer_id: PeerId) {
        if let Some(peer) = self.peers.get(peer_id) {
            peer.connection.close(0u32.into(), &[]);
        }
    }

    pub fn send_message(&self, peer_id: PeerId, message: Vec<u8>) {
        if let Some(peer) = self.peers.get(peer_id) {
            if peer.msg_tx.try_send(message).is_err() {
                peer.connection.close(0u32.into(), b"outbound queue full");
            }
        }
    }

    pub fn recv(&mut self) -> Option<Event> {
        let event = self.event_rx.try_recv().ok()?;

        match event {
            WorkerEvent::Connected(connection, tx, msg_tx, peer_id) => {
                let addr = connection.remote_address();
                let peer_id = peer_id.unwrap_or_else(|| self.peers_id.insert(()));

                self.peers
                    .insert(peer_id, ActiveConnection { connection, msg_tx });

                let _ = tx.send(peer_id);

                return Some(Event::Connected(peer_id, addr));
            }

            WorkerEvent::Message(peer_id, bytes) => {
                return Some(Event::Message(peer_id, bytes));
            }

            WorkerEvent::Disconnect(peer_id) => {
                self.peers_id.remove(peer_id);

                if let Some(peer) = self.peers.remove(peer_id) {
                    return Some(Event::Disconnect(peer_id, peer.connection.remote_address()));
                }
            }

            WorkerEvent::ConnectionError(peer_id) => {
                self.peers_id.remove(peer_id);

                return Some(Event::ConnectionError(peer_id));
            }
        }

        None
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Command::Close);
    }
}

async fn worker_task(
    endpoint: Endpoint, mut cmd_rx: Recv<Command>,
    event_tx: crossbeam_channel::Sender<WorkerEvent>, is_listening: bool,
) {
    if is_listening {
        tokio::spawn(accept_connections(endpoint.clone(), event_tx.clone()));
    }

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            Command::Connect(addr, peer_id) => match endpoint.connect(addr, "samp.cef") {
                Ok(connecting) => {
                    tokio::spawn(process_connection(
                        connecting,
                        event_tx.clone(),
                        Some(peer_id),
                    ));
                }
                Err(_) => {
                    let _ = event_tx.try_send(WorkerEvent::ConnectionError(peer_id));
                }
            },

            Command::Close => {
                endpoint.close(0u32.into(), &[]);
                break;
            }
        }
    }
}

async fn accept_connections(endpoint: Endpoint, event_tx: crossbeam_channel::Sender<WorkerEvent>) {
    let limit = std::sync::Arc::new(tokio::sync::Semaphore::new(128));
    while let Some(incoming) = endpoint.accept().await {
        let Ok(permit) = limit.clone().try_acquire_owned() else { incoming.refuse(); continue; };
        let event_tx = event_tx.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if let Ok(connecting) = incoming.accept() {
                let _ = process_connection(connecting, event_tx, None).await;
            }
        });
    }
}

async fn process_connection(
    connecting: Connecting, event_tx: crossbeam_channel::Sender<WorkerEvent>,
    peer_id: Option<PeerId>,
) -> anyhow::Result<()> {
    let connection = match connecting.await {
        Ok(conn) => conn,
        Err(_) => {
            if let Some(peer_id) = peer_id {
                let _ = event_tx.try_send(WorkerEvent::ConnectionError(peer_id));
            }

            return Ok(());
        }
    };

    let (msg_tx, mut msg_rx) = mpsc::channel(128);

    let peer_id =
        notify_about_incoming(event_tx.clone(), connection.clone(), msg_tx, peer_id).await?;

    tokio::spawn(listen_to_streams(connection.clone(), peer_id, event_tx));

    while let Some(bytes) = msg_rx.recv().await {
        let mut stream = connection.open_uni().await?;

        stream.write_all(&bytes).await?;
        let _ = stream.finish();
    }

    Ok(())
}

async fn notify_about_incoming(
    event_tx: crossbeam_channel::Sender<WorkerEvent>, connection: Connection,
    msg_tx: mpsc::Sender<Vec<u8>>, peer_id: Option<PeerId>,
) -> anyhow::Result<PeerId> {
    let (tx, rx) = oneshot::channel();
    event_tx.try_send(WorkerEvent::Connected(connection, tx, msg_tx, peer_id))?;

    Ok(rx.await?)
}

async fn listen_to_streams(
    connection: Connection, peer_id: PeerId, event_tx: crossbeam_channel::Sender<WorkerEvent>,
) {
    while let Ok(mut stream) = connection.accept_uni().await {
        if let Ok(bytes) = stream.read_to_end(INCOMING_PACKET_SIZE).await
            && event_tx.try_send(WorkerEvent::Message(peer_id, bytes)).is_err()
        {
            connection.close(0u32.into(), b"event queue full");
            break;
        }
    }

    // Do not lose lifecycle cleanup when the bounded data queue is saturated.
    // Yield between retries so runtime shutdown can cancel this task cleanly.
    let mut event = WorkerEvent::Disconnect(peer_id);
    loop {
        match event_tx.try_send(event) {
            Ok(()) | Err(crossbeam_channel::TrySendError::Disconnected(_)) => break,
            Err(crossbeam_channel::TrySendError::Full(returned)) => { event = returned; }
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}
