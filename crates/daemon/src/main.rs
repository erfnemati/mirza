//! Mirza: dictation from a global shortcut into any app.
//!
//! `mirza` runs the tray daemon; other commands talk to the running daemon.

// No console window on Windows; commands run from a terminal attach to it.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod autostart;
mod daemon;
mod desktop;
mod ipc;
mod tasks;

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;

use mirza_core::config;
use mirza_core::ipc::Request;
use mirza_core::secrets;

const USAGE: &str = "Usage: mirza [command]

Without a command, starts Mirza in the tray.

Commands for the running app:
  toggle             start dictating, or stop if already dictating
  toggle-revise      the same, with the text revised by AI before it's typed
  toggle-task        start or stop saying a task for Google Tasks
  add-task <text>    save a task from text, as if it was said
  start | stop       start or stop dictating
  cancel             stop now, without waiting for the rest of the text
  status             show what Mirza is doing
  settings           open the settings
  provider <id>      switch provider: soniox, elevenlabs or openai
  next-provider      switch to the next provider
  press <shortcut>   simulate pressing a shortcut (e.g. dictate-hold)
  release <shortcut> simulate releasing it
  quit               close Mirza

Other commands:
  set-key <service>  save an API key (read from standard input): soniox,
                     elevenlabs, openai, gemini, claude, openrouter or local
  google-login [--client <file>]
                     sign in to Google for voice tasks; --client saves the
                     client file downloaded from Google Cloud first
  google-logout      sign out of Google; waiting tasks stay
  config-path        print where the settings file is
";

fn main() -> ExitCode {
    #[cfg(windows)]
    // SAFETY: attaching to the parent console, if there is one, so output shows.
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("run");
    let arg = args.get(1).cloned();
    let req = match (cmd, arg) {
        ("run" | "daemon", _) => return run_daemon(),
        ("-h" | "--help" | "help", _) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        ("--version" | "version", _) => {
            println!("mirza {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        ("config-path", _) => {
            println!("{}", config::config_path().display());
            return ExitCode::SUCCESS;
        }
        ("set-key", Some(p)) => return set_key(&p),
        ("google-login", _) => return google_login(&args[1..]),
        ("google-logout", _) => return google_logout(),
        ("toggle", _) => Request::Toggle,
        ("toggle-revise", _) => Request::ToggleRevise,
        ("toggle-task", _) => Request::ToggleTask,
        ("add-task", Some(_)) => Request::AddTask { text: args[1..].join(" ") },
        ("start", _) => Request::Start,
        ("stop", _) => Request::Stop,
        ("cancel", _) => Request::Cancel,
        ("status", _) => Request::Status,
        ("settings" | "panel", _) => Request::OpenPanel,
        ("next-provider", _) => Request::NextProvider,
        ("provider", Some(id)) => Request::SetProvider { id },
        ("press", Some(id)) => Request::Shortcut { id, pressed: true },
        ("release", Some(id)) => Request::Shortcut { id, pressed: false },
        ("reload", _) => Request::Reload,
        ("quit", _) => Request::Quit,
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
    match rt.block_on(ipc::request(&req)) {
        Ok(reply) => {
            if req == Request::Status
                && let Some(s) = &reply.status
            {
                println!("state: {:?}\nprovider: {}", s.state, s.provider);
                if !s.last_error.is_empty() {
                    println!("last error: {}", s.last_error);
                }
            }
            if let Some(d) = &reply.data
                && let Some(title) = d["title"].as_str()
            {
                println!("{title}\n{}", d["body"].as_str().unwrap_or_default());
            }
            if reply.ok {
                ExitCode::SUCCESS
            } else {
                eprintln!("mirza: {}", reply.message);
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("mirza: {e}");
            ExitCode::FAILURE
        }
    }
}

fn set_key(provider: &str) -> ExitCode {
    let services = secrets::services();
    if !services.contains(&provider) {
        eprintln!("mirza: unknown service {provider:?} (use {})", services.join(", "));
        return ExitCode::from(2);
    }
    if std::io::stdin().is_terminal() {
        let name = mirza_core::ai::Kind::parse(provider).map_or(config::provider_name(provider), |k| k.name());
        print!("Paste the {name} API key and press Enter: ");
        let _ = std::io::stdout().flush();
    }
    let mut key = String::new();
    if std::io::stdin().lock().read_line(&mut key).is_err() {
        return ExitCode::FAILURE;
    }
    match secrets::set(provider, key.trim()) {
        Ok(()) => {
            println!("Saved to {}", secrets::keys_path().display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("mirza: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Signs in to Google for voice tasks, in this process; the running app
/// picks the sign-in up afterwards.
fn google_login(args: &[String]) -> ExitCode {
    let path = config::config_path();
    if let [flag, file, ..] = args
        && flag == "--client"
    {
        let saved = std::fs::read_to_string(file)
            .map_err(|e| format!("{file}: {e}"))
            .and_then(|text| mirza_core::tasks::settings::read_client(&text))
            .and_then(|(id, secret)| {
                mirza_core::tasks::settings::save_client(&path, &id, secret.as_deref()).map_err(|e| e.to_string())
            });
        if let Err(e) = saved {
            eprintln!("mirza: {e}");
            return ExitCode::FAILURE;
        }
        println!("Saved the Google client.");
    }
    let cfg = match config::Config::load(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("mirza: {e}");
            return ExitCode::FAILURE;
        }
    };
    let rt = runtime();
    let result = rt.block_on(async {
        // An address pasted here stands in for the browser coming back.
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                if tx.blocking_send(line).is_err() {
                    return;
                }
            }
        });
        let show = |url: &str| {
            println!("Sign in to Google in your browser. If it doesn't open, open this address:\n\n{url}\n");
            println!("On a computer without a browser, sign in on another device, then paste here the");
            println!("address the browser ends up at (it starts with http://127.0.0.1) and press Enter.");
            desktop::open_path(url);
        };
        let granted = mirza_core::tasks::sign_in(&cfg, show, rx).await?;
        let _ = ipc::request(&Request::RetryTasks).await;
        Ok::<_, String>(granted)
    });
    match result {
        Ok(g) if g.calendar() => {
            println!("Signed in. Tasks with a time also go to Google Calendar.");
            ExitCode::SUCCESS
        }
        Ok(_) => {
            println!("Signed in, without Calendar: tasks with a time are saved as tasks only.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("mirza: {e}");
            ExitCode::FAILURE
        }
    }
}

fn google_logout() -> ExitCode {
    let cfg = config::Config::load(&config::config_path()).unwrap_or_default();
    let store = mirza_core::tasks::Store::new(mirza_core::tasks::Store::default_dir());
    let rt = runtime();
    match rt.block_on(mirza_core::tasks::sign_out(&cfg, &store)) {
        Ok(()) => {
            let _ = rt.block_on(ipc::request(&Request::RetryTasks));
            println!("Signed out of Google. Tasks said from now on wait until you sign in again.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("mirza: {e}");
            ExitCode::FAILURE
        }
    }
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("MIRZA_LOG")
                .unwrap_or_else(|_| "mirza=info,zbus=error,warn".into()),
        )
        .with_target(false)
        .init();
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio runtime")
}

/// Binds the socket; if Mirza is already running, asks it to show its settings.
async fn claim() -> Result<ipc::Listener, ExitCode> {
    match ipc::listen().await {
        Ok(l) => Ok(l),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Launched again from the app menu: show the running one's settings.
            let _ = ipc::request(&Request::OpenPanel).await;
            Err(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("mirza: {e}");
            Err(ExitCode::FAILURE)
        }
    }
}

/// Quits cleanly on Ctrl+C or SIGTERM.
fn quit_on_signal(tx: tokio::sync::mpsc::UnboundedSender<daemon::Msg>) {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let mut term = signal(SignalKind::terminate()).expect("signal handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
        let (rtx, _rrx) = tokio::sync::oneshot::channel();
        let _ = tx.send(daemon::Msg::Ipc(Request::Quit, rtx));
    });
}

/// Desktops tell apps apart by the service they run in, and name their
/// shortcuts after it. Started from a terminal, Mirza would count as part of
/// the terminal, so it starts itself again as its own user service.
/// MIRZA_FOREGROUND=1 keeps it in the terminal (for debugging).
#[cfg(target_os = "linux")]
fn relaunch_as_own_app() -> bool {
    if std::env::var_os("MIRZA_FOREGROUND").is_some() {
        return false;
    }
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
    if cgroup.contains(&format!("app-{}", daemon::APP_ID)) {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let path = exe.to_string_lossy();
    if path.contains("/target/debug/") || path.contains("/target/release/") {
        return false; // a development build: keep it where it was started
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let mut cmd = std::process::Command::new("systemd-run");
    cmd.args(["--user", "--quiet", "--collect"]).arg(format!("--unit=app-{}@{secs}", daemon::APP_ID));
    // The service doesn't inherit the terminal's environment; pass on what matters.
    for (k, v) in std::env::vars() {
        let proxy = k.to_lowercase().ends_with("_proxy");
        if k.starts_with("MIRZA_") || k.ends_with("_API_KEY") || proxy {
            cmd.arg(format!("--setenv={k}={v}"));
        }
    }
    matches!(cmd.arg(exe).status(), Ok(s) if s.success())
}

#[cfg(target_os = "linux")]
fn run_daemon() -> ExitCode {
    init_logging();
    runtime().block_on(async {
        let listener = match claim().await {
            Ok(l) => l,
            Err(code) => return code,
        };
        if relaunch_as_own_app() {
            let _ = std::fs::remove_file(mirza_core::ipc::socket_path());
            println!("Mirza is running in the tray.");
            return ExitCode::SUCCESS;
        }
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let desktop = match desktop::Desktop::new(tx.clone()).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("mirza: {e}");
                return ExitCode::FAILURE;
            }
        };
        let d = match daemon::Daemon::new(tx.clone(), config::config_path(), desktop).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("mirza: {e}");
                return ExitCode::FAILURE;
            }
        };
        tokio::spawn(ipc::serve(listener, tx.clone()));
        quit_on_signal(tx);
        d.run(rx).await;
        ExitCode::SUCCESS
    })
}

/// Windows and macOS: the tray runs on the main thread, the daemon on another.
#[cfg(not(target_os = "linux"))]
fn run_daemon() -> ExitCode {
    init_logging();
    #[allow(unused_mut)]
    let mut event_loop = tao::event_loop::EventLoopBuilder::<desktop::UiEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory); // no Dock icon
    }
    let proxy = event_loop.create_proxy();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let daemon_tx = tx.clone();
    std::thread::Builder::new()
        .name("mirza-daemon".into())
        .spawn(move || {
            let code = runtime().block_on(async {
                let listener = match claim().await {
                    Ok(l) => l,
                    Err(code) => return code,
                };
                let desktop = match desktop::Desktop::new(daemon_tx.clone(), proxy.clone()).await {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("mirza: {e}");
                        return ExitCode::FAILURE;
                    }
                };
                let d = match daemon::Daemon::new(daemon_tx.clone(), config::config_path(), desktop).await {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("mirza: {e}");
                        return ExitCode::FAILURE;
                    }
                };
                tokio::spawn(ipc::serve(listener, daemon_tx.clone()));
                quit_on_signal(daemon_tx);
                d.run(rx).await;
                ExitCode::SUCCESS
            });
            let _ = proxy.send_event(desktop::UiEvent::Quit);
            // The tray loop ends the process; report failures ourselves.
            if format!("{code:?}") != format!("{:?}", ExitCode::SUCCESS) {
                std::process::exit(1);
            }
        })
        .expect("starting the daemon thread");
    desktop::run_ui(event_loop, tx)
}
