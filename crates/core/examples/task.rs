//! Tries voice tasks from a terminal.
//!   cargo run -p mirza-core --example task -- parse claude "call Sara tomorrow at 3"
//!   cargo run -p mirza-core --example task -- parse gemini --lists Personal,Work --pause 4000 < cases.txt
//!   cargo run -p mirza-core --example task -- parse claude --now "2026-11-06T18:40[Europe/Berlin]" "..."
//! `parse` only asks the AI and prints the task; nothing goes to Google.
//! Without text it reads one task per line from stdin (lines starting with #
//! are skipped). Model and address come from the config file unless given;
//! the key comes from the usual places (keys file, MIRZA_<ID>_API_KEY).

use std::io::BufRead;
use std::time::{Duration, Instant};

use mirza_core::ai::{Client, Kind};
use mirza_core::config::{self, Config};
use mirza_core::tasks::{List, parse, when};
use mirza_core::{net, secrets};

const USAGE: &str = "usage: task parse <openai|gemini|claude|openrouter|local> [--model M] [--effort E] [--lists A,B] [--now ZONED] [--pause ms] [text]";

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("parse") => parse_cmd(args).await,
        _ => eprintln!("{USAGE}"),
    }
}

async fn parse_cmd(mut args: impl Iterator<Item = String>) {
    let Some(kind) = args.next().as_deref().and_then(Kind::parse) else { return eprintln!("{USAGE}") };
    let cfg = Config::load(&config::config_path()).unwrap_or_default();
    let mut settings = cfg.ai.get(kind).clone();
    let mut names = "Personal,Learning,Financial,Acme,Hamrah".to_owned();
    let mut now = None;
    let mut pause_ms: u64 = 50;
    let mut text = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => settings.model = args.next().unwrap_or_default(),
            "--effort" => {
                let v = args.next().unwrap_or_default();
                settings.effort = serde_json::from_value(serde_json::Value::String(v)).unwrap_or(settings.effort);
            }
            "--lists" => names = args.next().unwrap_or_default(),
            "--now" => now = args.next().and_then(|v| v.parse::<jiff::Zoned>().ok()),
            "--pause" => pause_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(pause_ms),
            _ => text.push(a),
        }
    }
    let lists: Vec<List> = names
        .split(',')
        .filter(|n| !n.trim().is_empty())
        .enumerate()
        .map(|(i, n)| List { id: format!("list{i}"), title: n.trim().to_owned() })
        .collect();
    let proxy = net::resolve_proxy(&cfg.proxy).expect("proxy setting");
    let key = secrets::get(kind.id()).map(|(k, _)| k);
    let terms = cfg.providers.get(&cfg.active_provider).map(|p| p.terms.clone()).unwrap_or_default();
    let timeout = Duration::from_secs(cfg.tasks.timeout_sec);
    let client = match Client::new(kind, &settings, key, proxy.as_ref(), timeout) {
        Ok(c) => c,
        Err(e) => return eprintln!("{e}"),
    };
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
        let now = now.clone().unwrap_or_else(|| mirza_core::tasks::Said::now().zoned());
        let req = parse::request(&line, &lists, &cfg.tasks.list_hints, &terms, &now);
        let started = Instant::now();
        let r = parse::parse(&client, &req, &lists, "", &now, timeout).await;
        let ms = started.elapsed().as_millis();
        println!("in:    {line}");
        match r {
            Ok(d) => {
                let list = lists.iter().find(|l| l.id == d.list).map(|l| l.title.as_str()).unwrap_or("(default)");
                println!("title: {}", d.title);
                if !d.note.is_empty() {
                    println!("note:  {}", d.note);
                }
                println!("list:  {list}");
                let w = d.when();
                if let Some(date) = w.date {
                    let len = if d.minutes > 0 { format!(", {} min", d.minutes) } else { String::new() };
                    println!("when:  {} ({date}){len}", when::describe(&w, now.date()));
                }
                for g in &d.guesses {
                    println!("guess: {g}");
                }
                println!("       ({ms} ms)\n");
            }
            Err(e) => println!("FAILED: {e} ({ms} ms)\n"),
        }
        tokio::time::sleep(Duration::from_millis(pause_ms)).await;
    }
}
