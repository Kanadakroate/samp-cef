// Test the production mixer without the GTA entry point / DllMain test build.
#[path = "../src/notification_sound.rs"]
#[allow(dead_code)]
mod notification_sound;
#[path = "../src/ui_sound.rs"]
#[allow(dead_code)]
mod ui_sound;
#[path = "../src/audio.rs"]
#[allow(dead_code)]
mod audio;
mod utils {
    pub fn current_time() -> i128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i128
    }
}
fn main() {}
