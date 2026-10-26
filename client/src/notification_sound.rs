//! Finite local notification audio. No Chromium capture, HTTP or streaming queue.
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

pub const QUEUED: u8 = 0;
pub const PLAYING: u8 = 1;
pub const ENDED: u8 = 2;
pub const CANCELLED: u8 = 3;
const MAX_BYTES: u64 = 1_048_576;
const MAX_SECONDS: usize = 10;
const ASSET: &str = "lr_progression_toast/achievement-complete.ogg";

pub struct Clip {
    pub rate: u32,
    pub frames: Vec<(f32, f32)>,
}

pub fn decode(bytes: Vec<u8>) -> Result<Arc<Clip>, String> {
    if bytes.len() > MAX_BYTES as usize { return Err("sound too large".into()); }
    let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes))
        .map_err(|_| "invalid OGG header")?;
    let rate = reader.ident_hdr.audio_sample_rate;
    let channels = reader.ident_hdr.audio_channels as usize;
    if !(8_000..=96_000).contains(&rate) || !(1..=2).contains(&channels) {
        return Err("unsupported sound format".into());
    }
    let mut frames = Vec::new();
    while let Some(packet) = reader.read_dec_packet_itl().map_err(|_| "invalid OGG packet")? {
        if reader.ident_hdr.audio_sample_rate != rate || reader.ident_hdr.audio_channels as usize != channels
            || packet.len() % channels != 0 || frames.len() + packet.len()/channels > rate as usize * MAX_SECONDS {
            return Err("sound limit or chained format change".into());
        }
        for sample in packet.chunks_exact(channels) {
            frames.push((sample[0] as f32 / 32768., sample[channels-1] as f32 / 32768.));
        }
    }
    if frames.is_empty() { return Err("empty sound".into()); }
    Ok(Arc::new(Clip { rate, frames }))
}

fn read_clip(root: &Path, bundle: &str) -> Result<Arc<Clip>, String> {
    let root = root.canonicalize().map_err(|_| "missing asset root")?;
    let path = root.join("liberty-revival").join(bundle).join(ASSET).canonicalize().map_err(|_| "missing sound")?;
    if !path.starts_with(&root) { return Err("sound outside asset root".into()); }
    let mut bytes = Vec::new();
    std::fs::File::open(path).map_err(|_| "sound open failed")?
        .take(MAX_BYTES+1).read_to_end(&mut bytes).map_err(|_| "sound read failed")?;
    decode(bytes)
}

#[derive(Default)]
pub struct Cache {
    loading: AtomicBool,
    pub result: OnceLock<Result<Arc<Clip>, String>>,
}
impl Cache {
    pub fn prepare(self: &Arc<Self>, root: std::path::PathBuf, bundle: String) {
        if self.result.get().is_some() || self.loading.swap(true, Ordering::AcqRel) { return; }
        let cache = self.clone();
        if std::thread::Builder::new().name("lr-notification-decode".into()).spawn(move || {
            // Isolate malformed decoder input from the game process's worker lifecycle.
            let result = std::panic::catch_unwind(|| read_clip(&root, &bundle))
                .unwrap_or_else(|_| Err("sound decoder failed".into()));
            let _ = cache.result.set(result);
        }).is_err() { let _ = self.result.set(Err("sound worker unavailable".into())); }
    }
}

#[derive(Default)]
pub struct NotificationSession {
    document: String,
    cache: Arc<crate::notification_sound::Cache>,
    token: String,
    last_sequence: u64,
    state: Option<Arc<AtomicU8>>,
}
impl NotificationSession {
    pub fn cancel(&mut self) {
        if let Some(state) = &self.state { state.store(crate::notification_sound::CANCELLED, Ordering::Release); }
    }
    pub fn reset(&mut self) { self.cancel(); *self = Self::default(); }
}

impl NotificationSession {
    pub fn request(&mut self, action: &str, token: &str, bundle: &str, root: std::path::PathBuf, audible: bool, enqueue: impl FnOnce(Voice) -> bool) -> &'static str {
        let session = self;
        if action == "prepare" {
            if token.contains(':') { return "error"; }
            if !session.document.is_empty() && session.document != token { return "stale"; }
            session.document = token.to_owned();
            session.cache.prepare(root, bundle.to_owned());
            return match session.cache.result.get() {
                None => "loading", Some(Ok(_)) => "ready", Some(Err(_)) => "error",
            };
        }
        if session.document.is_empty() || token.split_once(':').map(|v| v.0) != Some(session.document.as_str()) { return "stale"; }
        if action == "stop" {
            if session.token == token { session.cancel(); }
            return "cancelled";
        }
        if action == "play" && session.token != token {
            let Some(sequence) = token.split_once(':').and_then(|v| v.1.parse::<u64>().ok()) else { return "stale"; };
            if sequence <= session.last_sequence { return "stale"; }
            if !audible { return "muted"; }
            if session.state.as_ref().is_some_and(|s| s.load(Ordering::Acquire) < ENDED) { return "busy"; }
            let Some(Ok(clip)) = session.cache.result.get() else { return "error"; };
            let state = Arc::new(AtomicU8::new(QUEUED));
            if !enqueue(Voice::new(clip.clone(), state.clone())) { return "error"; }
            session.token = token.to_owned(); session.state = Some(state);
            session.last_sequence = sequence;
        }
        if session.token != token { return "stale"; }
        match session.state.as_ref().map(|s| s.load(Ordering::Acquire)) {
            Some(QUEUED) => "queued", Some(PLAYING) => "playing", Some(ENDED) => "ended",
            Some(CANCELLED) => "cancelled", _ => "error",
        }
    }
}

pub struct Voice {
    pub state: Arc<AtomicU8>,
    clip: Arc<Clip>,
    position: f64,
    drain_frames: usize,
}
impl Voice {
    pub fn new(clip: Arc<Clip>, state: Arc<AtomicU8>) -> Self {
        Self { clip, state, position: 0., drain_frames: 0 }
    }
    /// Returns false only after cancellation or the tail has drained. The caller
    /// does not invoke this while paused, preserving the exact sample position.
    pub fn render(&mut self, rate: u32, gain: f32, output: &mut [(f32, f32)]) -> bool {
        if self.state.load(Ordering::Acquire) >= ENDED { return false; }
        let _ = self.state.compare_exchange(QUEUED, PLAYING, Ordering::AcqRel, Ordering::Acquire);
        if self.state.load(Ordering::Acquire) >= ENDED { return false; }
        let step = self.clip.rate as f64 / rate as f64;
        for sample in output {
            if self.position < self.clip.frames.len() as f64 {
                let index = self.position.floor() as usize;
                let a = self.clip.frames[index];
                let b = self.clip.frames[(index+1).min(self.clip.frames.len()-1)];
                let t = (self.position-index as f64) as f32;
                sample.0 += (a.0+(b.0-a.0)*t)*gain*0.35;
                sample.1 += (a.1+(b.1-a.1)*t)*gain*0.35;
                self.position += step;
            } else {
                self.drain_frames += 1;
                // Beyond the existing mixer's four queued 256-frame blocks,
                // including its supported 8 kHz minimum device rate.
                if self.drain_frames >= 2048 {
                    let _ = self.state.compare_exchange(PLAYING, ENDED, Ordering::AcqRel, Ordering::Acquire);
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_rejects_stale_duplicate_busy_hidden_and_failed_enqueue() {
        let mut session = NotificationSession::default();
        assert!(session.cache.result.set(Ok(Arc::new(Clip { rate:48000,frames:vec![(0.5,0.5);100] }))).is_ok());
        let root=std::path::PathBuf::new();
        let request=|s:&mut NotificationSession,a:&str,t:&str,visible:bool| s.request(a,t,"",root.clone(),visible,|_|true);
        assert_eq!(request(&mut session,"play","doc:1",true),"stale");
        assert_eq!(request(&mut session,"prepare","doc",true),"ready");
        assert_eq!(request(&mut session,"prepare","other",true),"stale");
        assert_eq!(request(&mut session,"play","doc:1",false),"muted");
        assert_eq!(session.request("play","doc:1","",root.clone(),true,|_|false),"error");
        assert_eq!(request(&mut session,"play","doc:1",true),"queued");
        assert_eq!(request(&mut session,"play","doc:2",true),"busy");
        assert_eq!(session.request("play","doc:1","",root.clone(),true,|_|panic!("duplicate enqueue")),"queued");
        let old=session.state.as_ref().unwrap().clone();
        assert_eq!(request(&mut session,"stop","doc:1",true),"cancelled");
        assert_eq!(request(&mut session,"play","doc:2",true),"queued");
        assert_eq!(request(&mut session,"play","doc:1",true),"stale");
        assert_eq!(old.load(Ordering::Acquire),CANCELLED);
        let current=session.state.as_ref().unwrap().clone(); session.reset();
        assert_eq!(current.load(Ordering::Acquire),CANCELLED);
        assert_eq!(request(&mut session,"play","doc:3",true),"stale");
    }
    #[test]
    fn first_sample_tail_and_repeated_plays_are_preserved() {
        let frames: Vec<_> = (0..3001).map(|i| (i as f32/4000., 0.75-i as f32/4000.)).collect();
        let clip = Arc::new(Clip { rate: 48000, frames: frames.clone() });
        for _ in 0..4 {
            let state = Arc::new(AtomicU8::new(QUEUED));
            let mut voice = Voice::new(clip.clone(), state.clone());
            let mut samples = Vec::new();
            loop {
                let mut block = [(0.,0.);256];
                let active = voice.render(48000, 0.5, &mut block);
                samples.extend(block);
                if !active { break; }
            }
            for (actual, expected) in samples.iter().zip(&frames) {
                assert_eq!(*actual, (expected.0*0.5*0.35, expected.1*0.5*0.35));
            }
            assert!(samples[frames.len()..].iter().all(|v| *v == (0.,0.)));
            assert_eq!(state.load(Ordering::Acquire), ENDED);
        }
    }
    #[test]
    fn cancelled_voice_never_restarts() {
        let state = Arc::new(AtomicU8::new(CANCELLED));
        let mut voice = Voice::new(Arc::new(Clip {rate:48000,frames:vec![(1.,1.);100]}),state.clone());
        let mut out = [(0.,0.);256];
        assert!(!voice.render(44100,1.,&mut out));
        assert!(out.iter().all(|s| *s==(0.,0.)));
    }
    #[test]
    fn rejects_invalid_or_oversized_input() {
        assert!(decode(vec![0;MAX_BYTES as usize+1]).is_err());
        assert!(decode(b"not an ogg".to_vec()).is_err());
    }
}
