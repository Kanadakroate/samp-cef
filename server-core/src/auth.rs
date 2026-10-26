//! The game callback is the sole authority for player identity. No IP address participates.
use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

pub const TOKEN_TTL: Duration = Duration::from_secs(15);
const MAX_PENDING: usize = 128;
struct Pending<P> { peer: P, until: Instant, eligible: HashMap<i32, u64> }

pub struct Auth<P> {
    serial: u64,
    games: HashMap<i32, u64>,
    pending: HashMap<String, Pending<P>>,
    bound: HashMap<P, (i32, u64)>,
}

impl<P: Copy + Eq + Hash> Default for Auth<P> {
    fn default() -> Self {
        Self { serial: 0, games: HashMap::new(), pending: HashMap::new(), bound: HashMap::new() }
    }
}

pub fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl<P: Copy + Eq + Hash> Auth<P> {
    pub fn connect_game(&mut self, player: i32) {
        self.disconnect_game(player);
        self.serial = self.serial.checked_add(1).expect("game generation exhausted");
        self.games.insert(player, self.serial);
    }
    pub fn disconnect_game(&mut self, player: i32) {
        self.games.remove(&player);
        self.bound.retain(|_, (id, _)| *id != player);
        for pending in self.pending.values_mut() { pending.eligible.remove(&player); }
    }
    pub fn issue(&mut self, peer: P, token: String, now: Instant) -> bool {
        if !valid_token(&token) || self.pending.len() >= MAX_PENDING || self.pending.contains_key(&token)
            || self.bound.contains_key(&peer) || self.pending.values().any(|p| p.peer == peer) {
            return false;
        }
        self.pending.insert(token, Pending { peer, until: now + TOKEN_TTL, eligible: self.games.clone() });
        true
    }
    pub fn pending_peer(&self, token: &str) -> Option<P> { self.pending.get(token).map(|pending| pending.peer) }
    pub fn bind(&mut self, player: i32, token: &str, now: Instant) -> Option<(P, u64)> {
        if !valid_token(token) { return None; }
        // Consume even a rejected proof, preventing repeated identity guesses.
        let pending = self.pending.remove(token)?;
        let generation = *self.games.get(&player)?;
        if now >= pending.until || pending.eligible.get(&player) != Some(&generation)
            || self.peer_for(player).is_some() || self.bound.contains_key(&pending.peer) { return None; }
        self.bound.insert(pending.peer, (player, generation));
        Some((pending.peer, generation))
    }
    pub fn identity(&self, peer: P) -> Option<(i32, u64)> { self.bound.get(&peer).copied() }
    pub fn is_current(&self, peer: P, player: i32, generation: u64) -> bool {
        self.identity(peer) == Some((player, generation)) && self.games.get(&player) == Some(&generation)
    }
    pub fn peer_for(&self, player: i32) -> Option<P> {
        self.bound.iter().find_map(|(p, (id, _))| (*id == player).then_some(*p))
    }
    pub fn game_generation(&self, player: i32) -> Option<u64> { self.games.get(&player).copied() }
    pub fn disconnect_peer(&mut self, peer: P) {
        self.bound.remove(&peer);
        self.pending.retain(|_, pending| pending.peer != peer);
    }
    pub fn expire(&mut self, now: Instant) -> Vec<P> {
        let peers = self.pending.values().filter(|p| now >= p.until).map(|p| p.peer).collect();
        self.pending.retain(|_, p| now < p.until);
        peers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn token(n: u32) -> String { format!("{n:064x}") }
    #[test] fn independent_peers_without_ip_identity() {
        let mut a = Auth::default(); let now = Instant::now();
        a.connect_game(7); a.connect_game(8);
        assert!(a.issue(10, token(1), now)); assert!(a.issue(11, token(2), now));
        assert_eq!(a.identity(10), None);
        assert_eq!(a.bind(8, &token(2), now).unwrap().0, 11);
        assert_eq!(a.bind(7, &token(1), now).unwrap().0, 10);
    }
    #[test] fn replay_expiry_and_duplicate_binding_fail() {
        let mut a = Auth::default(); let now = Instant::now(); a.connect_game(7);
        a.issue(10, token(1), now); assert!(a.bind(7, &token(1), now).is_some());
        assert!(a.bind(7, &token(1), now).is_none());
        a.issue(11, token(2), now); assert!(a.bind(7, &token(2), now).is_none());
        a.disconnect_game(7); a.connect_game(7);
        a.issue(12, token(3), now); assert!(a.bind(7, &token(3), now + TOKEN_TTL).is_none());
    }
    #[test] fn reused_slot_rejects_pending_tokens_and_queued_callbacks() {
        let mut a = Auth::default(); let now = Instant::now(); a.connect_game(7);
        a.issue(10, token(1), now); let (_, old) = a.bind(7, &token(1), now).unwrap();
        a.issue(11, token(2), now); a.disconnect_game(7); a.connect_game(7);
        assert!(!a.is_current(10, 7, old)); assert!(a.bind(7, &token(2), now).is_none());
        a.issue(12, token(3), now); let (_, fresh) = a.bind(7, &token(3), now).unwrap();
        assert_ne!(old, fresh); assert!(a.is_current(12, 7, fresh));
        a.disconnect_peer(12); assert!(!a.is_current(12, 7, fresh));
    }
    #[test] fn expiry_capacity_and_format_are_bounded() {
        let mut a = Auth::default(); let now = Instant::now(); a.connect_game(0);
        assert!(!a.issue(1, "A".repeat(64), now)); assert!(!valid_token(&"1".repeat(63)));
        for n in 0..MAX_PENDING as u32 { assert!(a.issue(n, token(n), now)); }
        assert!(!a.issue(999, token(999), now));
        assert_eq!(a.expire(now + TOKEN_TTL).len(), MAX_PENDING);
        assert!(a.issue(999, token(999), now));
    }
    #[test] fn token_issued_before_game_is_not_eligible_for_future_slot() {
        let mut a = Auth::default(); let now = Instant::now();
        a.issue(10, token(1), now); a.connect_game(7);
        assert!(a.bind(7, &token(1), now).is_none());
    }
}
