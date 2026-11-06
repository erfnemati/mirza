//! Revises text with an AI provider and prints what comes back.
//!   cargo run -p mirza-core --example revise -- claude "um so I I want to go"
//!   cargo run -p mirza-core --example revise -- local --base-url http://10.0.0.5:8000/v1 < cases.txt
//!   cargo run -p mirza-core --example revise -- local --style full --presets plain_words,digits < cases.txt
//! Without text it reads one dictation per line from stdin (lines starting
//! with # are skipped). Model and address come from the config file unless
//! given, and so do style and presets; the key comes from the usual places
//! (keys file, MIRZA_<ID>_API_KEY).

use std::io::BufRead;
use std::time::{Duration, Instant};

use mirza_core::ai::revise::{self, Hints};
use mirza_core::ai::{Client, Kind, Style};
use mirza_core::config::{self, Config};
use mirza_core::{net, secrets};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let Some(kind) = args.next().as_deref().and_then(Kind::parse) else {
        return eprintln!(
            "usage: revise <openai|gemini|claude|openrouter|local> [--base-url U] [--model M] [--style clean|light|full] [--presets a,b] [text]"
        );
    };
    let mut cfg = Config::load(&config::config_path()).unwrap_or_default();
    let mut settings = cfg.ai.get(kind).clone();
    let mut text = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--base-url" => settings.base_url = args.next().unwrap_or_default(),
            "--model" => settings.model = args.next().unwrap_or_default(),
            "--style" => {
                cfg.revision.style = match args.next().as_deref() {
                    Some("light") => Style::Light,
                    Some("full") => Style::Full,
                    _ => Style::Clean,
                }
            }
            "--presets" => {
                cfg.revision.presets = args.next().unwrap_or_default().split(',').map(str::to_owned).collect()
            }
            _ => text.push(a),
        }
    }
    let proxy = net::resolve_proxy(&cfg.proxy).expect("proxy setting");
    let key = secrets::get(kind.id()).map(|(k, _)| k);
    let stt = cfg.providers.get(&cfg.active_provider).cloned().unwrap_or_else(|| cfg.providers.soniox.clone());
    let hints = Hints::new(&cfg.revision, stt.languages, stt.terms);
    let lines: Vec<String> = if text.is_empty() {
        std::io::stdin()
            .lock()
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .collect()
    } else {
        vec![text.join(" ")]
    };
    for line in lines {
        let timeout = revise::timeout_for(&line, cfg.revision.timeout_sec);
        let client = match Client::new(kind, &settings, key.clone(), proxy.as_ref(), timeout) {
            Ok(c) => c,
            Err(e) => return eprintln!("{e}"),
        };
        let started = Instant::now();
        let r = revise::revise_or_raw(&client, &line, &hints, timeout).await;
        let ms = started.elapsed().as_millis();
        println!("in:  {}", r.raw);
        match r.note {
            None => println!("out: {}\n     ({ms} ms)\n", r.text),
            Some(why) => println!("RAW: {why} ({ms} ms)\n"),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
