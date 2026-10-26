use crossbeam_channel::{Receiver, Sender};
use messages::{packets, try_into_packet};
use network::{CertStrategy, Event as SocketEvent, PeerId, Socket};
use quick_protobuf::deserialize_from_slice;
use std::collections::HashMap;
use std::convert::TryInto;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use crate::{ServerEvent, auth::Auth};

enum Packet { Normal { peer: PeerId, bytes: Vec<u8> }, Disconnect(PeerId) }
struct Browser { generation: u32, ready: bool, failed: bool }
struct Rate { start: Instant, count: u32 }
fn valid_event_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b':' || b == b'.')
}
fn local_url(s: &str) -> bool {
    s.starts_with("sampcef://assets/") && s.len() <= 1024 && !s.contains('\\') && !s.contains("..") && !s.contains(['\r', '\n', '\0'])
}
pub struct Server {
    event_tx: Sender<ServerEvent>, event_rx: Receiver<ServerEvent>, sender: Sender<Packet>,
    auth: Auth<PeerId>, browsers: HashMap<(i32, u32), Browser>, browser_serial: u32,
    rates: HashMap<PeerId, Rate>,
    stopping: Arc<AtomicBool>, worker: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    #[cfg(test)]
    pub(crate) fn test_unbound() -> Self {
        let (sender, _) = crossbeam_channel::bounded(32);
        let (event_tx, event_rx) = crossbeam_channel::bounded(256);
        Self { event_tx, event_rx, sender, auth: Auth::default(), browsers: HashMap::new(), browser_serial: 0, rates: HashMap::new(), stopping: Arc::new(AtomicBool::new(false)), worker: None }
    }
    pub fn new(addr: SocketAddr) -> Result<Arc<Mutex<Self>>, String> {
        let mut socket = Socket::new_server(addr, CertStrategy::FromFiles).map_err(|e| format!("CEF server TLS/bind failed: {e}"))?;
        let (sender, receiver) = crossbeam_channel::bounded(1024);
        let (event_tx, event_rx) = crossbeam_channel::bounded(1024);
        let server = Arc::new(Mutex::new(Self { event_tx, event_rx, sender, auth: Auth::default(), browsers: HashMap::new(), browser_serial: 0, rates: HashMap::new(), stopping: Arc::new(AtomicBool::new(false)), worker: None }));
        let worker = server.clone();
        let stopping = server.lock().unwrap().stopping.clone();
        let handle = std::thread::spawn(move || while !stopping.load(Ordering::Acquire) {
            while let Some(event) = socket.recv() {
                if stopping.load(Ordering::Acquire) { return; }
                let mut server = worker.lock().unwrap();
                match event {
                    SocketEvent::Message(peer, bytes) => {
                        if bytes.len() <= 8192 && server.take_rate(peer) {
                            if let Ok(packet) = deserialize_from_slice::<packets::Packet>(&bytes) { server.handle_client_packet(peer, packet); }
                        }
                    }
                    SocketEvent::Connected(peer, _) => server.handle_new_connection(peer),
                    SocketEvent::Disconnect(peer, _) | SocketEvent::ConnectionError(peer) => server.handle_timeout(peer),
                }
            }
            {
                let mut server = worker.lock().unwrap();
                for peer in server.auth.expire(Instant::now()) {
                    server.rates.remove(&peer);
                    let _ = server.sender.try_send(Packet::Disconnect(peer));
                }
            }
            for packet in receiver.try_iter() {
                match packet { Packet::Normal { peer, bytes } => socket.send_message(peer, bytes), Packet::Disconnect(peer) => socket.disconnect(peer) }
            }
            std::thread::sleep(Duration::from_millis(5));
        });
        server.lock().unwrap().worker = Some(handle);
        Ok(server)
    }
    pub fn shutdown(server: &Arc<Mutex<Self>>) {
        let worker = {
            let mut server = server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            server.stopping.store(true, Ordering::Release);
            server.worker.take()
        };
        // The worker may need this mutex to finish. Never join while holding it.
        if let Some(worker) = worker { let _ = worker.join(); }
    }
    fn take_rate(&mut self, peer: PeerId) -> bool {
        let Some(rate) = self.rates.get_mut(&peer) else { return false; };
        if rate.start.elapsed() >= Duration::from_secs(1) { rate.start = Instant::now(); rate.count = 0; }
        rate.count = rate.count.saturating_add(1); rate.count <= 60
    }
    fn handle_client_packet(&mut self, peer: PeerId, packet: packets::Packet) {
        // REQUEST_JOIN and all other pre-auth messages are never sufficient proof.
        if self.auth.identity(peer).is_none() { return; }
        match packet.packet_id {
            packets::PacketId::EMIT_EVENT => { if let Ok(p) = deserialize_from_slice(&packet.bytes) { self.handle_emit_event(peer, p); } }
            packets::PacketId::BROWSER_CREATED => { if let Ok(p) = deserialize_from_slice(&packet.bytes) { self.handle_browser_created(peer, p); } }
            _ => ()
        }
    }
    fn handle_emit_event(&mut self, peer: PeerId, packet: packets::EmitEvent) {
        let Some((player_id, game_generation)) = self.auth.identity(peer) else { return; };
        let (Some(browser_id), Some(browser_generation), Some(args)) = (packet.browser_id, packet.browser_generation, packet.args) else { return; };
        if !valid_event_name(&packet.event_name) || args.len() > 4096 || args.contains('\0') || !packet.arguments.is_empty() { return; }
        let Some(browser) = self.browsers.get(&(player_id, browser_id)) else { return; };
        if browser.failed || browser.generation != browser_generation { return; }
        let _ = self.event_tx.try_send(ServerEvent::EmitEvent {
            peer, player_id, game_generation, browser_id, browser_generation,
            event: packet.event_name.into_owned(), arguments: args.into_owned(),
        });
    }
    fn handle_browser_created(&mut self, peer: PeerId, packet: packets::BrowserCreated) {
        let Some((player_id, game_generation)) = self.auth.identity(peer) else { return; };
        let Some(browser) = self.browsers.get_mut(&(player_id, packet.browser_id)) else { return; };
        if browser.generation != packet.browser_generation || browser.ready { return; }
        browser.ready = true;
        browser.failed = packet.status_code < 200 || packet.status_code >= 300;
        let _ = self.event_tx.try_send(ServerEvent::BrowserCreated {
            peer, player_id, game_generation, browser_id: packet.browser_id,
            browser_generation: packet.browser_generation, code: packet.status_code,
        });
    }
    fn handle_timeout(&mut self, peer: PeerId) {
        if let Some((player, _)) = self.auth.identity(peer) { self.browsers.retain(|(id, _), _| *id != player); }
        self.auth.disconnect_peer(peer); self.rates.remove(&peer);
    }
    fn handle_new_connection(&mut self, peer: PeerId) {
        let mut bytes = [0u8; 32];
        if getrandom::getrandom(&mut bytes).is_err() { let _ = self.sender.try_send(Packet::Disconnect(peer)); return; }
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        if !self.auth.issue(peer, token.clone(), Instant::now()) { let _ = self.sender.try_send(Packet::Disconnect(peer)); return; }
        self.rates.insert(peer, Rate { start: Instant::now(), count: 0 });
        self.send_peer(peer, packets::OpenConnection { bind_token: token.into() });
    }
    pub fn bind_game_connection(&mut self, player_id: i32, token: &str) -> bool {
        let pending_peer = self.auth.pending_peer(token);
        let Some((peer, game_generation)) = self.auth.bind(player_id, token, Instant::now()) else {
            // A consumed failed proof must not leave an unbound connection occupying
            // the transport limit forever after its pending expiry record is removed.
            if let Some(peer) = pending_peer {
                self.auth.disconnect_peer(peer); self.rates.remove(&peer);
                let _ = self.sender.try_send(Packet::Disconnect(peer));
            }
            return false;
        };
        self.send_peer(peer, packets::JoinResponse { success: true, current_version: Some(1) });
        let _ = self.event_tx.try_send(ServerEvent::PlayerConnected { peer, player_id, game_generation });
        true
    }
    pub fn allow_connection(&mut self, player_id: i32, _addr: IpAddr) {
        self.remove_connection(player_id, None); self.auth.connect_game(player_id);
    }
    pub fn remove_connection(&mut self, player_id: i32, _addr: Option<IpAddr>) {
        if let Some(peer) = self.auth.peer_for(player_id) { let _ = self.sender.try_send(Packet::Disconnect(peer)); self.rates.remove(&peer); }
        self.auth.disconnect_game(player_id); self.browsers.retain(|(id, _), _| *id != player_id);
    }
    pub fn current_event(&self, peer: PeerId, player: i32, generation: u64, browser: Option<(u32, u32)>) -> bool {
        self.auth.is_current(peer, player, generation) && browser.is_none_or(|(id, generation)| self.browsers.get(&(player, id)).is_some_and(|b| b.generation == generation))
    }
    pub fn browser_accepts_events(&self, player: i32, id: u32, generation: u32) -> bool {
        self.browsers.get(&(player, id)).is_some_and(|b| b.generation == generation && !b.failed)
    }
    pub fn game_generation(&self, player: i32) -> Option<u64> { self.auth.game_generation(player) }
    pub fn create_browser(&mut self, player_id: i32, browser_id: i32, url: String, hidden: bool, focused: bool) -> bool {
        if browser_id < 0 || !local_url(&url) || !self.has_plugin(player_id) { return false; }
        let Some(generation) = self.browser_serial.checked_add(1).filter(|v| *v <= i32::MAX as u32) else { return false; };
        self.browser_serial = generation;
        self.browsers.insert((player_id, browser_id as u32), Browser { generation, ready: false, failed: false });
        self.send_packet(player_id, packets::CreateBrowser { browser_id: browser_id as u32, browser_generation: generation, url: url.into(), hidden, focused });
        true
    }
    pub fn destroy_browser(&mut self, player_id: i32, browser_id: i32) {
        self.browsers.remove(&(player_id, browser_id as u32));
        self.send_packet(player_id, packets::DestroyBrowser { browser_id: browser_id as u32 });
    }
    pub fn hide_browser(&self, player_id: i32, browser_id: i32, hide: bool) {
        self.send_packet(
            player_id,
            packets::HideBrowser {
                browser_id: browser_id as u32,
                hide,
            },
        );
    }

    pub fn focus_browser(&self, player_id: i32, browser_id: i32, focused: bool) {
        self.send_packet(
            player_id,
            packets::FocusBrowser {
                browser_id: browser_id as u32,
                focused,
            },
        );
    }

    // Legacy broadcast is deliberately disabled in protocol 1.
    pub fn emit_event(&self, _player_id: i32, _event: &str, _arguments: Vec<packets::EventValue>) {}

    pub fn emit_browser_event(&self, player_id: i32, browser_id: u32, event: &str, arguments: Vec<packets::EventValue>) -> bool {
        if !valid_event_name(event) || arguments.len() > 32 { return false; }
        let Some(browser) = self.browsers.get(&(player_id, browser_id)) else { return false; };
        if browser.failed { return false; }
        let total: usize = arguments.iter().map(|a| a.string_value.as_ref().map_or(4, |s| s.len())).sum();
        if total > 60 * 1024 { return false; }
        self.send_packet(player_id, packets::EmitEvent {
            event_name: event.into(), args: None, arguments,
            browser_id: Some(browser_id), browser_generation: Some(browser.generation),
        });
        true
    }

    pub fn always_listen_keys(&self, player_id: i32, browser_id: i32, listen: bool) {
        self.send_packet(
            player_id,
            packets::AlwaysListenKeys {
                browser_id: browser_id as u32,
                listen,
            },
        );
    }

    pub fn has_plugin(&self, player_id: i32) -> bool { self.auth.peer_for(player_id).is_some() }

    pub fn create_external_browser(&self, _player_id: i32, _browser_id: i32, _texture: String, _url: String, _scale: i32) {}

    pub fn append_to_object(&self, player_id: i32, browser_id: i32, object_id: i32) {
        self.send_packet(
            player_id,
            packets::AppendToObject {
                browser_id: browser_id as u32,
                object_id,
            },
        );
    }

    pub fn remove_from_object(&self, player_id: i32, browser_id: i32, object_id: i32) {
        self.send_packet(
            player_id,
            packets::RemoveFromObject {
                browser_id: browser_id as u32,
                object_id,
            },
        );
    }

    pub fn toggle_dev_tools(&self, player_id: i32, browser_id: i32, enabled: bool) {
        self.send_packet(
            player_id,
            packets::ToggleDevTools {
                browser_id: browser_id as u32,
                enabled,
            },
        );
    }

    pub fn set_audio_settings(
        &self, player_id: i32, browser_id: u32, max_distance: f32, reference_distance: f32,
    ) {
        self.send_packet(
            player_id,
            packets::SetAudioSettings {
                browser_id,
                max_distance,
                reference_distance,
            },
        );
    }

    // Navigations need a fresh generation: destroy/create instead.
    pub fn load_url(&self, _player_id: i32, _browser_id: u32, _url: String) {}

    pub fn receiver(&self) -> Receiver<ServerEvent> {
        self.event_rx.clone()
    }

    fn send_packet<'a, T: TryInto<packets::Packet<'a>, Error = quick_protobuf::Error>>(&self, player_id: i32, packet: T) {
        if let Some(peer) = self.peer_by_id(player_id) { self.send_peer(peer, packet); }
    }
    fn send_peer<'a, T: TryInto<packets::Packet<'a>, Error = quick_protobuf::Error>>(&self, peer: PeerId, packet: T) {
        if let Ok(bytes) = try_into_packet(packet) { let _ = self.sender.try_send(Packet::Normal { peer, bytes }); }
    }
    fn peer_by_id(&self, player_id: i32) -> Option<PeerId> { self.auth.peer_for(player_id) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn server() -> (Server, PeerId, u64) {
        let (sender, _) = crossbeam_channel::bounded(32);
        let (event_tx, event_rx) = crossbeam_channel::bounded(32);
        let mut server = Server { event_tx, event_rx, sender, auth: Auth::default(), browsers: HashMap::new(), browser_serial: 0, rates: HashMap::new(), stopping: Arc::new(AtomicBool::new(false)), worker: None };
        let peer = PeerId::default(); let now = Instant::now(); server.auth.connect_game(7);
        server.auth.issue(peer, "a".repeat(64), now);
        let (_, generation) = server.auth.bind(7, &"a".repeat(64), now).unwrap();
        (server, peer, generation)
    }
    fn emit(id: u32, generation: u32, args: &str) -> packets::EmitEvent<'_> {
        packets::EmitEvent { event_name: "lr:ready".into(), args: Some(args.into()), arguments: Vec::new(), browser_id: Some(id), browser_generation: Some(generation) }
    }
    #[test] fn only_registered_browser_and_generation_can_emit() {
        let (mut s, p, _) = server();
        s.handle_emit_event(p, emit(1, 1, "{\"ready\":true}")); assert!(s.event_rx.try_recv().is_err());
        assert!(s.create_browser(7, 1, "sampcef://assets/house/index.html".into(), false, false));
        s.handle_emit_event(p, emit(1, 1, "{\"ready\":true}"));
        assert!(matches!(s.event_rx.try_recv(), Ok(ServerEvent::EmitEvent { browser_generation: 1, .. })));
        s.handle_emit_event(p, emit(1, 2, "0")); assert!(s.event_rx.try_recv().is_err());
        s.handle_emit_event(p, emit(2, 1, "0")); assert!(s.event_rx.try_recv().is_err());
    }
    #[test] fn browser_reuse_and_game_disconnect_invalidate_queued_events() {
        let (mut s, p, game) = server();
        s.create_browser(7, 1, "sampcef://assets/house/index.html".into(), false, false);
        assert!(s.current_event(p, 7, game, Some((1, 1))));
        s.destroy_browser(7, 1); s.create_browser(7, 1, "sampcef://assets/house/index.html".into(), false, false);
        assert!(!s.current_event(p, 7, game, Some((1, 1))));
        s.remove_connection(7, None); assert!(!s.current_event(p, 7, game, None));
    }
    #[test] fn load_failure_blocks_events_but_preserves_scoped_failure_callback() {
        let (mut s, p, game) = server();
        s.create_browser(7, 1, "sampcef://assets/house/index.html".into(), false, false);
        s.handle_browser_created(p, packets::BrowserCreated { browser_id: 1, browser_generation: 1, status_code: 404 });
        assert!(matches!(s.event_rx.try_recv(), Ok(ServerEvent::BrowserCreated { code: 404, .. })));
        assert!(s.current_event(p, 7, game, Some((1, 1))));
        assert!(!s.browser_accepts_events(7, 1, 1));
        s.handle_emit_event(p, emit(1, 1, "0")); assert!(s.event_rx.try_recv().is_err());
    }
    #[test] fn shutdown_joins_worker_and_releases_its_server_reference() {
        let server = Arc::new(Mutex::new(Server::test_unbound()));
        let held = server.clone();
        let stopping = server.lock().unwrap().stopping.clone();
        let handle = std::thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) { std::thread::yield_now(); }
            // Proves shutdown released the mutex before waiting for this thread.
            let _guard = held.lock().unwrap();
        });
        server.lock().unwrap().worker = Some(handle);
        Server::shutdown(&server);
        assert_eq!(Arc::strong_count(&server), 1);
        assert!(server.lock().unwrap().worker.is_none());
        Server::shutdown(&server);
    }
    #[test] fn rejected_consumed_proof_disconnects_pending_peer() {
        let (mut s, p, _) = server();
        s.auth.disconnect_peer(p);
        let (tx, rx) = crossbeam_channel::bounded(32); s.sender = tx;
        assert!(s.auth.issue(p, "b".repeat(64), Instant::now()));
        s.rates.insert(p, Rate { start: Instant::now(), count: 0 });
        assert!(!s.bind_game_connection(999, &"b".repeat(64)));
        assert!(matches!(rx.try_recv(), Ok(Packet::Disconnect(peer)) if peer == p));
        assert!(s.auth.pending_peer(&"b".repeat(64)).is_none());
        assert!(!s.rates.contains_key(&p));
    }
    #[test] fn limits_and_local_urls_fail_closed() {
        let (mut s, p, _) = server();
        assert!(!s.create_browser(7, 1, "https://example.com".into(), false, false));
        assert!(!s.create_browser(7, 1, "sampcef://assets/../private".into(), false, false));
        s.create_browser(7, 1, "sampcef://assets/house/index.html".into(), false, false);
        s.handle_emit_event(p, emit(1, 1, &"x".repeat(4097))); assert!(s.event_rx.try_recv().is_err());
        s.rates.insert(p, Rate { start: Instant::now(), count: 0 });
        for _ in 0..60 { assert!(s.take_rate(p)); } assert!(!s.take_rate(p));
        s.auth.disconnect_peer(p); s.handle_emit_event(p, emit(1, 1, "0")); assert!(s.event_rx.try_recv().is_err());
    }
}
