use client_api::utils::handle_result;
use crossbeam_channel::{Receiver, Sender};
use messages::packets;
use net::{Event as SocketEvent, PeerId, Socket};
use quick_protobuf::deserialize_from_slice;

use crate::app::{Event, ExternalBrowser};

use std::net::SocketAddr;
use std::time::{Duration, Instant};

#[allow(dead_code)]
struct Packet {
    peer: PeerId,
    bytes: Vec<u8>,
}

#[allow(dead_code)]
impl Packet {
    fn new(peer: PeerId, bytes: Vec<u8>) -> Packet {
        Packet { peer, bytes }
    }
}

pub struct NetworkClient {
    event_tx: Sender<Event>,
    incoming_rx: Receiver<Event>,
}

impl NetworkClient {
    pub fn new() -> NetworkClient {
        let (net_tx, incoming_rx) = crossbeam_channel::unbounded();
        let (client_tx, client_rx) = crossbeam_channel::unbounded();

        std::thread::spawn(move || {
            if let Some(network) = Network::new(net_tx.clone(), client_rx) {
                network.run();
            } else {
                tracing::error!("cannot initialize CEF network client");
                std::thread::sleep(Duration::from_secs(2));
                handle_result(net_tx.send(Event::NetworkError));
            }
        });

        NetworkClient {
            event_tx: client_tx,
            incoming_rx,
        }
    }

    pub fn recv(&self) -> Option<Event> { self.incoming_rx.try_recv().ok() }

    pub fn send(&self, message: Event) {
        handle_result(self.event_tx.send(message));
    }
}

impl Drop for NetworkClient {
    fn drop(&mut self) {
        self.send(Event::Terminate);
    }
}

#[derive(Debug, Clone, Copy)]
enum ConnectionState {
    Auth(SocketAddr, Instant, PeerId),
    Connected(SocketAddr, PeerId),
    Disconnected,
}

impl ConnectionState {
    fn peer(&self) -> Option<PeerId> {
        match self {
            ConnectionState::Auth(_, _, peer) => Some(*peer),
            ConnectionState::Connected(_, peer) => Some(*peer),
            _ => None,
        }
    }

    fn is_auth(&self) -> bool {
        matches!(self, ConnectionState::Auth(_, _, _))
    }
}

struct Network {
    socket: Socket,
    connection_state: ConnectionState,

    event_tx: Sender<Event>,
    event_rx: Receiver<Event>,

    timings: Instant,
}

impl Network {
    fn new(event_tx: Sender<Event>, event_rx: Receiver<Event>) -> Option<Network> {
        let addr = "0.0.0.0:0".parse().unwrap();
        let socket = handle_result(Socket::new_client(addr))?;

        Some(Network {
            connection_state: ConnectionState::Disconnected,
            timings: Instant::now(),
            socket,
            event_tx,
            event_rx,
        })
    }

    fn handle_packet(&mut self, packet: packets::Packet) {
        use packets::PacketId::*;

        if !matches!(self.connection_state, ConnectionState::Connected(..))
            && !matches!(packet.packet_id, OPEN_CONNECTION | JOIN_RESPONSE) { return; }
        match packet.packet_id {
            OPEN_CONNECTION => {
                if self.connection_state.is_auth() {
                    if let Ok(open) = deserialize_from_slice::<packets::OpenConnection>(&packet.bytes) {
                        let token = open.bind_token.to_string();
                        if token.len() == 64 && token.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
                            handle_result(self.event_tx.send(Event::BindGameConnection(token)));
                        }
                    }
                }
            }

            JOIN_RESPONSE => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_join_response(packet))
                    .ok();
            }

            CREATE_BROWSER => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_create_browser(packet))
                    .ok();
            }

            DESTROY_BROWSER => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_destroy_browser(packet))
                    .ok();
            }

            HIDE_BROWSER => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_hide_browser(packet))
                    .ok();
            }

            ALWAYS_LISTEN_KEYS => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_always_listen_keys(packet))
                    .ok();
            }

            FOCUS_BROWSER => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_focus_browser(packet))
                    .ok();
            }

            EMIT_EVENT => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_emit_event(packet))
                    .ok();
            }

            CREATE_EXTERNAL_BROWSER => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_create_external_browser(packet))
                    .ok();
            }

            APPEND_TO_OBJECT => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_append_to_object(packet))
                    .ok();
            }

            REMOVE_FROM_OBJECT => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_remove_from_object(packet))
                    .ok();
            }

            TOGGLE_DEV_TOOLS => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_toggle_dev_tools(packet))
                    .ok();
            }

            SET_AUDIO_SETTINGS => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_set_audio_settings(packet))
                    .ok();
            }

            LOAD_URL => {
                deserialize_from_slice(&packet.bytes)
                    .map(|packet| self.handle_load_url(packet))
                    .ok();
            }

            _ => (),
        }
    }

    fn handle_join_response(&mut self, packet: packets::JoinResponse) {
        if let ConnectionState::Auth(addr, _, peer) = self.connection_state {
            if packet.success {
                self.connection_state = ConnectionState::Connected(addr, peer);

                tracing::debug!(server = %addr, ?peer, "CEF server handshake completed");
                handle_result(self.event_tx.send(Event::NetworkJoined));
            } else {
                self.connection_state = ConnectionState::Disconnected;
                tracing::trace!(
                    server_version = packet.current_version,
                    "CEF server rejected client version"
                );
                handle_result(self.event_tx.send(Event::BadVersion));
            }
        } else {
            tracing::trace!(state = ?self.connection_state, "unexpected join response");
        }
    }

    fn handle_create_browser(&mut self, packet: packets::CreateBrowser) {
        let event = Event::CreateBrowser {
            id: packet.browser_id,
            generation: packet.browser_generation,
            url: packet.url.to_string(),
            hidden: packet.hidden,
            focused: packet.focused,
        };

        handle_result(self.event_tx.send(event));
    }

    fn handle_destroy_browser(&mut self, packet: packets::DestroyBrowser) {
        handle_result(self.event_tx.send(Event::DestroyBrowser(packet.browser_id)));
    }

    fn handle_emit_event(&mut self, packet: packets::EmitEvent) {
        let (Some(id), Some(generation)) = (packet.browser_id, packet.browser_generation) else { return; };
        if generation == 0 { return; }
        let list = cef::types::list::List::new();

        for (idx, arg) in packet.arguments.iter().enumerate() {
            if let Some(f) = arg.float_value {
                list.set_double(idx, f as f64);
            }

            if let Some(i) = arg.integer_value {
                list.set_integer(idx, i);
            }

            if let Some(s) = &arg.string_value {
                let cef_str = cef::types::string::CefString::new(s);
                list.set_string(idx, &cef_str);
            }
        }

        handle_result(
            self.event_tx
                .send(Event::EmitEvent(id, generation, packet.event_name.to_string(), list)),
        );
    }

    fn handle_hide_browser(&mut self, packet: packets::HideBrowser) {
        let event = Event::HideBrowser(packet.browser_id, packet.hide);
        handle_result(self.event_tx.send(event));
    }

    fn handle_always_listen_keys(&mut self, packet: packets::AlwaysListenKeys) {
        let event = Event::AlwaysListenKeys(packet.browser_id, packet.listen);
        handle_result(self.event_tx.send(event));
    }

    fn handle_focus_browser(&mut self, packet: packets::FocusBrowser) {
        let event = Event::FocusBrowser(packet.browser_id, packet.focused);
        handle_result(self.event_tx.send(event));
    }

    fn handle_create_external_browser(&mut self, packet: packets::CreateExternalBrowser) {
        let ext = ExternalBrowser {
            id: packet.browser_id,
            texture: packet.texture.to_string(),
            scale: packet.scale,
            url: packet.url.to_string(),
        };

        let event = Event::CreateExternBrowser(ext);
        handle_result(self.event_tx.send(event));
    }

    fn handle_append_to_object(&mut self, packet: packets::AppendToObject) {
        let event = Event::AppendToObject(packet.browser_id, packet.object_id);
        handle_result(self.event_tx.send(event));
    }

    fn handle_remove_from_object(&mut self, packet: packets::RemoveFromObject) {
        let event = Event::RemoveFromObject(packet.browser_id, packet.object_id);
        handle_result(self.event_tx.send(event));
    }

    fn handle_toggle_dev_tools(&mut self, packet: packets::ToggleDevTools) {
        let event = Event::ToggleDevTools(packet.browser_id, packet.enabled);
        handle_result(self.event_tx.send(event));
    }

    fn handle_set_audio_settings(&mut self, packet: packets::SetAudioSettings) {
        let event = Event::SetAudioSettings(
            packet.browser_id,
            crate::audio::BrowserAudioSettings {
                max_distance: packet.max_distance,
                reference_distance: packet.reference_distance,
            },
        );

        handle_result(self.event_tx.send(event));
    }

    fn handle_load_url(&mut self, packet: packets::LoadUrl) {
        let event = Event::LoadUrl(packet.browser_id, packet.url.to_string());
        handle_result(self.event_tx.send(event));
    }

    fn net_open_connection(&mut self, address: SocketAddr) {
        if let Some(peer) = self.connection_state.peer() { self.socket.disconnect(peer); }
        let peer = self.socket.connect(address);
        self.connection_state = ConnectionState::Auth(address, Instant::now(), peer);

        tracing::debug!(server = %address, ?peer, "opening CEF server connection");
        tracing::trace!(
            elapsed_ms = self.timings.elapsed().as_millis(),
            "network startup elapsed"
        );
    }

    fn net_emit_event(&mut self, browser_id: u32, browser_generation: u32, event: String, args: String) {
        if let ConnectionState::Connected(_address, peer) = self.connection_state {
            let emit = packets::EmitEvent {
                browser_id: Some(browser_id),
                browser_generation: Some(browser_generation),
                event_name: event.into(),
                args: Some(args.into()),
                arguments: Vec::new(),
            };

            let Ok(packet) = messages::try_into_packet(emit) else {
                tracing::error!(packet = "emit_event", "cannot serialize network packet");
                return;
            };

            self.socket.send_message(peer, packet);
        }
    }

    fn net_browser_created(&mut self, browser_id: u32, browser_generation: u32, status_code: i32) {
        if let ConnectionState::Connected(_address, peer) = self.connection_state {
            let created = packets::BrowserCreated {
                browser_id,
                browser_generation,
                status_code,
            };

            let Ok(packet) = messages::try_into_packet(created) else {
                tracing::error!(
                    packet = "browser_created",
                    "cannot serialize network packet"
                );
                return;
            };

            self.socket.send_message(peer, packet);
        }
    }

    fn process_network(&mut self) {
        if let Some(server_peer) = self.connection_state.peer() {
            while let Some(event) = self.socket.recv() {
                match event {
                    SocketEvent::Message(peer, packet) => {
                        if peer == server_peer
                            && let Err(e) = deserialize_from_slice(&packet)
                                .map(|packet| self.handle_packet(packet))
                        {
                            tracing::warn!(error = %e, "malformed packet received from CEF server");
                        }
                    }

                    SocketEvent::Disconnect(peer, _) | SocketEvent::ConnectionError(peer) => {
                        if peer == server_peer {
                            self.connection_state = ConnectionState::Disconnected;
                            handle_result(self.event_tx.send(Event::Timeout));
                            break;
                        }
                    }

                    _ => (),
                }
            }
        }

        if let ConnectionState::Auth(addr, time, _) = &self.connection_state
            && time.elapsed() >= Duration::from_secs(15)
        {
            let _ = addr;
            if let Some(peer) = self.connection_state.peer() { self.socket.disconnect(peer); }
            self.connection_state = ConnectionState::Disconnected;
            handle_result(self.event_tx.send(Event::Timeout));
        }
    }

    fn process_event(&mut self, event: Event) {
        match event {
            Event::Connect(addr) => self.net_open_connection(addr),
            Event::EmitEventOnServer(id, generation, event, arguments) => self.net_emit_event(id, generation, event, arguments),
            Event::BrowserCreated(id, generation, code) => self.net_browser_created(id, generation, code),
            _ => (),
        }
    }

    fn run(mut self) {
        'mainloop: loop {
            self.process_network();

            while let Ok(event) = self.event_rx.try_recv() {
                match event {
                    Event::Terminate => {
                        break 'mainloop;
                    }

                    event => self.process_event(event),
                }
            }

            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
