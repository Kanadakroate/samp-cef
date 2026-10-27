//! Fixed local click voice, independent of achievement notification identity.
use crate::notification_sound::{Cache, Voice, QUEUED, CANCELLED};
use std::sync::{Arc, atomic::{AtomicU8, Ordering}};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct UiSession {
    cache: Arc<Cache>,
    last_click: Option<Instant>,
    state: Option<Arc<AtomicU8>>,
}
impl UiSession {
    pub fn prepare(&self, root: std::path::PathBuf, bundle: &str) {
        self.cache.prepare_click(root, bundle.to_owned());
    }
    pub fn cancel(&mut self) {
        if let Some(state) = self.state.take() { state.store(CANCELLED, Ordering::Release); }
    }
    pub fn reset(&mut self) { self.cancel(); *self = Self::default(); }
    pub fn click(&mut self, audible: bool, enqueue: impl FnOnce(Voice) -> bool) -> bool {
        if !audible || self.last_click.is_some_and(|t| t.elapsed() < Duration::from_millis(45)) { return false; }
        let Some(Ok(clip)) = self.cache.result.get() else { return false; };
        let clip = clip.clone();
        let state = Arc::new(AtomicU8::new(QUEUED));
        if !enqueue(Voice::new(clip, state.clone())) { return false; }
        self.cancel(); self.state = Some(state); self.last_click = Some(Instant::now()); true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification_sound::Clip;
    #[test]
    fn clicks_are_bounded_mutable_and_cancelled_on_teardown() {
        let mut ui = UiSession::default();
        assert!(!ui.click(true, |_| panic!("unprepared")));
        assert!(ui.cache.result.set(Ok(Arc::new(Clip{rate:48000,frames:vec![(1.,1.);128]}))).is_ok());
        assert!(!ui.click(false, |_| panic!("hidden")));
        assert!(!ui.click(true, |_| false));
        assert!(ui.click(true, |_| true));
        assert!(!ui.click(true, |_| panic!("rate limit")));
        let state = ui.state.as_ref().unwrap().clone(); ui.reset();
        assert_eq!(state.load(Ordering::Acquire), CANCELLED);
    }
}
