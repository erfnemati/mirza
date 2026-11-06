//! Feeds an audio file to the phone API (`Dictation`) in 50 ms chunks, the way the Android app
//! does, and prints what the listener gets.
//!   cargo run -p mirza-mobile --example dictate -- speech.wav [proxy URL]
//! The Soniox key comes from the usual places (MIRZA_SONIOX_API_KEY, the keys file...).
//! Needs ffmpeg to decode the file.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use mirza_mobile::{Dictation, DictationConfig, DictationListener, sample_rate};

struct Print(mpsc::Sender<()>, AtomicUsize);

impl DictationListener for Print {
    fn on_text(&self, text: String) {
        println!("text: {text:?}");
    }
    fn on_partial(&self, _text: String) {
        self.1.fetch_add(1, Ordering::Relaxed);
    }
    fn on_error(&self, message: String) {
        println!("error: {message}");
        let _ = self.0.send(());
    }
    fn on_finished(&self) {
        println!("finished ({} partial updates)", self.1.load(Ordering::Relaxed));
        let _ = self.0.send(());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(file) = args.first() else {
        return eprintln!("usage: dictate <audio file> [proxy URL]");
    };
    let rate = sample_rate();
    let pcm = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-i", file, "-f", "s16le", "-ac", "1", "-ar", &rate.to_string(), "-"])
        .stderr(Stdio::inherit())
        .output()
        .expect("running ffmpeg")
        .stdout;
    let config = DictationConfig {
        api_key: mirza_core::secrets::get("soniox").map(|(k, _)| k).unwrap_or_default(),
        model: String::new(),
        languages: vec!["fa".into(), "en".into()],
        proxy: args.get(1).cloned().unwrap_or_default(),
    };
    let (done_tx, done_rx) = mpsc::channel();
    let dictation = match Dictation::new(config, Box::new(Print(done_tx, AtomicUsize::new(0)))) {
        Ok(d) => d,
        Err(e) => return eprintln!("{e}"),
    };
    // 50 ms of audio per chunk, as the app's recorder sends it.
    for chunk in pcm.chunks(rate as usize / 20 * 2) {
        if !dictation.push_audio(chunk.to_vec()) {
            println!("a chunk was dropped");
        }
        thread::sleep(Duration::from_millis(50));
    }
    dictation.finish();
    if done_rx.recv_timeout(Duration::from_secs(30)).is_err() {
        println!("nothing after 30 s");
    }
}
