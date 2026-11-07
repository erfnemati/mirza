//! Tries voice tasks from a terminal.
//!   cargo run -p mirza-core --example task -- parse claude "call Sara tomorrow at 3"
//!   cargo run -p mirza-core --example task -- parse gemini --lists Personal,Work --pause 4000 < cases.txt
//!   cargo run -p mirza-core --example task -- parse claude --now "2026-11-06T18:40[Europe/Berlin]" "..."
//! `parse` only asks the AI and prints the task; nothing goes to Google.
//! Without text it reads one task per line from stdin (lines starting with #
//! are skipped). Model and address come from the config file unless given;
//! the key comes from the usual places (keys file, MIRZA_<ID>_API_KEY).
//!
//! The rest use Google with the settings and the queue the app uses:
//!   task login [--client client.json] [--wait MINUTES]
//!                                       sign in (paste the final address if no browser)
//!   task lists                          the account's lists
//!   task add "call Sara tomorrow at 3"  save a task, as the shortcut does
//!   task waiting | retry | recent       the queue and the saved tasks
//!   task undo ID                        take a task back
//!   task edit ID [--title T] [--note N] [--list ID] [--date 2026-11-09|-] [--time 09:30|-] [--minutes N]
//!                                       change a task; the rest stays as it is
//!   task logout

use std::io::BufRead;
use std::time::{Duration, Instant};

use mirza_core::ai::{Client, Kind};
use mirza_core::config::{self, Config};
use mirza_core::tasks::{self, List, Store, parse, when};
use mirza_core::{net, secrets};

const USAGE: &str = "usage: task parse <openai|gemini|claude|openrouter|local> [--model M] [--effort E] [--lists A,B] [--now ZONED] [--pause ms] [text]
       task login [--client FILE] [--wait MINUTES] | lists | add TEXT | waiting | retry | recent | undo ID | logout
       task edit ID [--title T] [--note N] [--list ID] [--date YYYY-MM-DD|-] [--time HH:MM|-] [--minutes N]";

#[tokio::main]
async fn main() {
    if std::env::var_os("MIRZA_LOG").is_some() {
        tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_env("MIRZA_LOG")).init();
    }
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    let r = match cmd.as_str() {
        "parse" => {
            parse_cmd(args).await;
            Ok(())
        }
        "login" => login(args).await,
        "logout" => {
            let cfg = Config::load(&config::config_path()).unwrap_or_default();
            tasks::sign_out(&cfg, &Store::new(Store::default_dir())).await.map(|_| println!("Signed out."))
        }
        "lists" | "add" | "waiting" | "retry" | "recent" | "undo" | "edit" => google_cmd(&cmd, args.collect()).await,
        _ => Err(USAGE.to_owned()),
    };
    if let Err(e) = r {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

async fn login(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let path = config::config_path();
    let mut wait = tasks::SIGN_IN_WAIT;
    while let Some(a) = args.next() {
        match (a.as_str(), args.next()) {
            ("--client", Some(file)) => {
                let text = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?;
                let (id, secret) = tasks::settings::read_client(&text)?;
                tasks::settings::save_client(&path, &id, secret.as_deref()).map_err(|e| e.to_string())?;
                println!("Client saved (secret: {}).", if secret.is_some() { "yes" } else { "none" });
            }
            ("--wait", Some(m)) => wait = Duration::from_secs(60 * m.parse::<u64>().map_err(|_| USAGE)?),
            _ => return Err(USAGE.into()),
        }
    }
    let cfg = Config::load(&path).map_err(|e| e.to_string())?;
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.blocking_send(line).is_err() {
                return;
            }
        }
    });
    let show = |url: &str| {
        println!("Open this address and sign in:\n\n{url}\n");
        println!("If the browser is on another device, paste the address it ends up at here.");
    };
    let g = tasks::sign_in_waiting(&cfg, show, rx, wait).await?;
    println!("Signed in. Calendar events: {}.", if g.calendar() { "yes" } else { "no (tasks only)" });
    Ok(())
}

async fn google_cmd(cmd: &str, args: Vec<String>) -> Result<(), String> {
    let cfg = Config::load(&config::config_path()).map_err(|e| e.to_string())?;
    let setup = tasks::Setup::from_config(&cfg);
    let book = tasks::Book::new(Store::new(Store::default_dir()));
    let lists = book.lists(&setup, cmd == "lists").await.unwrap_or_default();
    let today = tasks::Said::now().zoned().date();
    let print = |o: &tasks::Outcome| {
        if let Some((title, body)) = tasks::notification(o, &lists, today) {
            println!("{title}\n  {}", body.replace('\n', "\n  "));
        }
    };
    match cmd {
        "lists" => {
            if lists.is_empty() {
                book.lists(&setup, true).await.map_err(|e| e.to_string())?;
            }
            for l in &lists {
                println!("{}  {}", l.id, l.title);
            }
        }
        "add" => {
            let text = args.join(" ");
            if text.trim().is_empty() {
                return Err(USAGE.into());
            }
            let e = book.capture(&text, tasks::Said::now(), false).map_err(|e| e.to_string())?;
            let o = book.process(&setup, &e.id).await;
            print(&o);
            println!("  (id {})", e.id);
        }
        "waiting" => {
            for e in book.waiting() {
                let next = jiff::Timestamp::from_millisecond(e.next_try_ms).map(|t| t.to_string()).unwrap_or_default();
                println!("{}  tries {}  next {next}  {}", e.id, e.attempts, e.last_error);
            }
        }
        "retry" => {
            book.retry_now();
            for o in book.process_due(&setup).await {
                print(&o);
            }
        }
        "recent" => {
            for s in book.recent() {
                let w = tasks::when::describe(&s.draft.when(), today);
                println!("{}  {}  [{}] {w}", s.id, s.draft.title, tasks::list_name(&lists, &s.draft.list));
            }
        }
        "undo" => {
            let id = args.first().ok_or(USAGE)?;
            book.undo(&setup, id).await?;
            println!("Undone.");
        }
        "edit" => {
            let (id, rest) = args.split_first().ok_or(USAGE)?;
            let d = book
                .waiting()
                .into_iter()
                .find(|e| &e.id == id)
                .and_then(|e| e.draft)
                .or_else(|| book.recent().into_iter().find(|s| &s.id == id).map(|s| s.draft))
                .ok_or("No such task, or the AI hasn't read it yet.")?;
            let mut e = tasks::Edit {
                title: d.title,
                note: d.note,
                list: d.list,
                date: d.date,
                time: d.time,
                minutes: d.minutes,
            };
            let mut rest = rest.iter();
            while let (Some(k), Some(v)) = (rest.next(), rest.next()) {
                match k.as_str() {
                    "--title" => e.title = v.clone(),
                    "--note" => e.note = v.clone(),
                    "--list" => e.list = v.clone(),
                    "--date" if v == "-" => e.date = None,
                    "--date" => e.date = Some(when::parse_date(v).ok_or("bad date")?),
                    "--time" if v == "-" => e.time = None,
                    "--time" => e.time = Some(when::parse_time(v).ok_or("bad time")?),
                    "--minutes" => e.minutes = v.parse().map_err(|_| "bad minutes")?,
                    _ => return Err(USAGE.into()),
                }
            }
            print(&book.edit(&setup, id, e).await?);
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

async fn parse_cmd(mut args: impl Iterator<Item = String>) {
    let Some(kind) = args.next().as_deref().and_then(Kind::parse) else { return eprintln!("{USAGE}") };
    let cfg = Config::load(&config::config_path()).unwrap_or_default();
    let mut settings = cfg.ai.get(kind).clone();
    let mut names = "Personal,Learning,Financial,Acme,Nova".to_owned();
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
