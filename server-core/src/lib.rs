use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::collections::HashMap;
use crossbeam_channel::Receiver;
use network::PeerId;
mod auth;
mod server;
use crate::server::Server;
pub use messages::packets::EventValue;
const EVENT_SCAN_BUDGET: usize = 128;
const INIT_TIMEOUT: Duration = Duration::from_secs(20);
#[derive(Debug, Clone)]
pub enum CoreEvent {
    EmitEvent { player_id: i32, browser_id: u32, browser_generation: u32, event: String, arguments: String },
    PlayerInitialized { player_id: i32, success: bool },
    BrowserCreated { player_id: i32, browser_id: u32, browser_generation: u32, code: i32 },
}
#[derive(Debug)]
pub(crate) enum ServerEvent {
    EmitEvent { peer: PeerId, player_id: i32, game_generation: u64, browser_id: u32, browser_generation: u32, event: String, arguments: String },
    PlayerConnected { peer: PeerId, player_id: i32, game_generation: u64 },
    BrowserCreated { peer: PeerId, player_id: i32, game_generation: u64, browser_id: u32, browser_generation: u32, code: i32 },
}
pub struct ServerCore {
    server: Arc<Mutex<Server>>, event_rx: Receiver<ServerEvent>,
    await_connect: HashMap<i32, (u64, Instant, bool)>,
}
impl Drop for ServerCore {
    fn drop(&mut self) { Server::shutdown(&self.server); }
}

impl ServerCore {
    pub fn new(addr: SocketAddr) -> Result<Self, String> {
        let server = Server::new(addr)?;
        let event_rx = server.lock().unwrap().receiver();
        Ok(Self { server, event_rx, await_connect: HashMap::new() })
    }
    pub fn allow_connection(&mut self, player_id: i32, addr: IpAddr) {
        let mut server = self.server.lock().unwrap();
        server.allow_connection(player_id, addr);
        if let Some(generation) = server.game_generation(player_id) { self.await_connect.insert(player_id, (generation, Instant::now(), false)); }
    }
    pub fn remove_connection(&mut self, player_id: i32) {
        self.server.lock().unwrap().remove_connection(player_id, None);
        self.await_connect.remove(&player_id);
    }
    pub fn bind_game_connection(&self, player_id: i32, token: &str) -> bool { self.server.lock().unwrap().bind_game_connection(player_id, token) }
    pub fn create_browser(&mut self, player_id: i32, browser_id: i32, url: String, hidden: bool, focused: bool) -> bool {
        self.server.lock().unwrap().create_browser(player_id, browser_id, url, hidden, focused)
    }
    pub fn emit_browser_event(&self, player_id: i32, browser_id: u32, event: &str, arguments: Vec<EventValue<'static>>) -> bool {
        self.server.lock().unwrap().emit_browser_event(player_id, browser_id, event, arguments)
    }
    pub fn destroy_browser(&mut self, player_id: i32, browser_id: i32) {
        let mut server = self.server.lock().unwrap();
        server.destroy_browser(player_id, browser_id);
    }

    pub fn hide_browser(&self, player_id: i32, browser_id: i32, hide: bool) {
        let server = self.server.lock().unwrap();
        server.hide_browser(player_id, browser_id, hide);
    }

    pub fn focus_browser(&self, player_id: i32, browser_id: i32, focused: bool) {
        let server = self.server.lock().unwrap();
        server.focus_browser(player_id, browser_id, focused);
    }

    pub fn emit_event(&self, player_id: i32, event: &str, arguments: Vec<EventValue<'static>>) {
        let server = self.server.lock().unwrap();
        server.emit_event(player_id, event, arguments);
    }

    pub fn always_listen_keys(&self, player_id: i32, browser_id: i32, listen: bool) {
        let server = self.server.lock().unwrap();
        server.always_listen_keys(player_id, browser_id, listen);
    }

    pub fn has_plugin(&self, player_id: i32) -> bool {
        let server = self.server.lock().unwrap();
        server.has_plugin(player_id)
    }

    pub fn create_external_browser(
        &self, player_id: i32, browser_id: i32, texture: String, url: String, scale: i32,
    ) {
        let server = self.server.lock().unwrap();
        server.create_external_browser(player_id, browser_id, texture, url, scale);
    }

    pub fn append_to_object(&self, player_id: i32, browser_id: i32, object_id: i32) {
        let server = self.server.lock().unwrap();
        server.append_to_object(player_id, browser_id, object_id);
    }

    pub fn remove_from_object(&self, player_id: i32, browser_id: i32, object_id: i32) {
        let server = self.server.lock().unwrap();
        server.remove_from_object(player_id, browser_id, object_id);
    }

    pub fn toggle_dev_tools(&self, player_id: i32, browser_id: i32, enabled: bool) {
        let server = self.server.lock().unwrap();
        server.toggle_dev_tools(player_id, browser_id, enabled);
    }

    pub fn set_audio_settings(
        &self, player_id: i32, browser_id: u32, max_distance: f32, reference_distance: f32,
    ) {
        let server = self.server.lock().unwrap();
        server.set_audio_settings(player_id, browser_id, max_distance, reference_distance);
    }

    pub fn load_url(&self, player_id: i32, browser_id: u32, url: String) {
        let server = self.server.lock().unwrap();
        server.load_url(player_id, browser_id, url);
    }

    pub fn poll_event(&mut self) -> Option<CoreEvent> {
        // A producer or a stale-event flood must never hold the game tick here.
        for _ in 0..EVENT_SCAN_BUDGET {
            let Ok(event) = self.event_rx.try_recv() else { break; };
            let server = self.server.lock().unwrap();
            match event {
                ServerEvent::EmitEvent { peer, player_id, game_generation, browser_id, browser_generation, event, arguments } => {
                    if server.current_event(peer, player_id, game_generation, Some((browser_id, browser_generation))) && server.browser_accepts_events(player_id, browser_id, browser_generation) {
                        return Some(CoreEvent::EmitEvent { player_id, browser_id, browser_generation, event, arguments });
                    }
                }
                ServerEvent::PlayerConnected { peer, player_id, game_generation } => {
                    if server.current_event(peer, player_id, game_generation, None) {
                        self.await_connect.remove(&player_id);
                        return Some(CoreEvent::PlayerInitialized { player_id, success: true });
                    }
                }
                ServerEvent::BrowserCreated { peer, player_id, game_generation, browser_id, browser_generation, code } => {
                    if server.current_event(peer, player_id, game_generation, Some((browser_id, browser_generation))) {
                        return Some(CoreEvent::BrowserCreated { player_id, browser_id, browser_generation, code });
                    }
                }
            }
        }
        for (&player_id, (generation, start, notified)) in &mut self.await_connect {
            if !*notified && start.elapsed() >= INIT_TIMEOUT {
                *notified = true;
                if self.server.lock().unwrap().game_generation(player_id) == Some(*generation) {
                    return Some(CoreEvent::PlayerInitialized { player_id, success: false });
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod poll_budget_tests {
    use super::*;
    #[test] fn stale_events_are_consumed_in_bounded_batches() {
        let (tx, rx) = crossbeam_channel::bounded(256);
        let mut core = ServerCore { server: Arc::new(Mutex::new(Server::test_unbound())), event_rx: rx, await_connect: HashMap::new() };
        for _ in 0..129 { tx.send(ServerEvent::PlayerConnected { peer: PeerId::default(), player_id: 7, game_generation: 1 }).unwrap(); }
        assert!(core.poll_event().is_none());
        assert_eq!(core.event_rx.len(), 1, "One tick must scan at most 128 stale events");
        assert!(core.poll_event().is_none()); assert!(core.event_rx.is_empty());
    }
}
