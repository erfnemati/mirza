//! Sends an audio file to a provider in real time and prints what comes back.
//!   cargo run -p mirza-core --example transcribe -- soniox speech.wav
//! The key comes from the usual places (MIRZA_<PROVIDER>_API_KEY, the keys file...).
//! Needs ffmpeg to decode the file.

use std::process::{Command, Stdio};
use std::time::Duration;

use mirza_core::config::Config;
use mirza_core::providers::{self, StreamRequest, SttEvent};
use mirza_core::{audio, net, secrets};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(provider), Some(file)) = (args.first(), args.get(1)) else {
        return eprintln!("usage: transcribe <soniox|elevenlabs|openai> <audio file>");
    };
    let cfg = Config::default();
    let settings = cfg.providers.get(provider).expect("unknown provider").clone();
    let api_key = secrets::get(provider).map(|(k, _)| k).unwrap_or_else(|| "missing-key".into());
    let rate = providers::sample_rate(provider);
    let pcm = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-i", file, "-f", "s16le", "-ac", "1", "-ar", &rate.to_string(), "-"])
        .stderr(Stdio::inherit())
        .output()
        .expect("running ffmpeg")
        .stdout;
    let proxy = net::resolve_proxy("").expect("proxy");
    let (atx, arx) = tokio::sync::mpsc::channel(4096);
    let (etx, mut erx) = tokio::sync::mpsc::unbounded_channel();
    let req = StreamRequest { provider: provider.clone(), api_key, settings, proxy };
    let task = providers::start(req, arx, etx);
    tokio::spawn(async move {
        for chunk in pcm.chunks(audio::chunk_bytes(rate)) {
            if atx.send(chunk.to_vec()).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });
    while let Some(ev) = erx.recv().await {
        match ev {
            SttEvent::Final(t) => println!("final: {t:?}"),
            SttEvent::Finished => {
                println!("finished");
                break;
            }
            SttEvent::Error(e) => {
                println!("error: {e}");
                break;
            }
            _ => {}
        }
    }
    let _ = task.await;
}
