//! Offline sample audit using the same decoder and finite voice as the plugin.
//! No GTA, OS output, microphone or CEF capture; only the supplied local OGG.
#[path = "../src/notification_sound.rs"]
#[allow(dead_code)]
mod notification_sound;
use notification_sound::*;
use std::sync::{Arc, atomic::{AtomicU8, Ordering}};
use std::io::Write;
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    assert_eq!(args.len(), 3, "probe <input.ogg> <new-output-directory>");
    let out = std::path::PathBuf::from(&args[2]);
    std::fs::create_dir(&out).expect("output must be new");
    let clip = decode(std::fs::read(&args[1]).unwrap()).unwrap();
    let mut reference = std::fs::File::create(out.join("decoded.f32")).unwrap();
    for &(l,r) in &clip.frames { reference.write_all(&l.to_le_bytes()).unwrap(); reference.write_all(&r.to_le_bytes()).unwrap(); }
    for rate in [clip.rate, 44100, 48000] {
        for index in 0..4 {
            let state = Arc::new(AtomicU8::new(QUEUED));
            let mut voice = Voice::new(clip.clone(), state.clone());
            let mut samples = Vec::new();
            loop {
                let mut block = [(0.,0.);256];
                let active = voice.render(rate,1.,&mut block);
                samples.extend(block);
                if !active { break; }
                assert!(samples.len() < rate as usize*12);
            }
            assert_eq!(state.load(Ordering::Acquire), ENDED);
            if rate == clip.rate {
                for (actual, expected) in samples.iter().zip(&clip.frames) {
                    assert_eq!(*actual,(expected.0*0.35,expected.1*0.35));
                }
                assert!(samples[clip.frames.len()..].iter().all(|s| *s==(0.,0.)));
            }
            let mut file=std::fs::File::create(out.join(format!("play-{rate}-{index}.f32"))).unwrap();
            for (l,r) in &samples { file.write_all(&l.to_le_bytes()).unwrap(); file.write_all(&r.to_le_bytes()).unwrap(); }
            println!("rate={rate} play={} decoded={} rendered={} complete",index+1,clip.frames.len(),samples.len());
        }
    }
}
