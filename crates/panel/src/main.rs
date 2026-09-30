//! Mirza's settings window. It runs only while open: it reads and writes the
//! config file and the keys directly, and asks the running daemon for live
//! state (status, spend, history) over its socket.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;

use mirza_core::config::{self, Config, PROVIDERS, Shortcut};
use mirza_core::ipc::{self, Reply, Request, Snapshot};
use mirza_core::models;
use mirza_core::secrets;
use serde::Serialize;
use tauri::Manager;

#[derive(Serialize)]
struct KeyInfo {
    present: bool,
    /// The key with all but its last four characters hidden.
    masked: String,
    /// "env" (from an environment variable) or "file".
    source: String,
}

#[derive(Serialize)]
struct Models {
    models: Vec<models::ModelInfo>,
    /// Why the provider's own list couldn't be fetched, if it couldn't.
    error: String,
}

#[derive(Serialize)]
struct PanelState {
    config: Config,
    config_path: String,
    /// Set when the config file has an error; `config` is then the defaults.
    config_error: String,
    keys: BTreeMap<String, KeyInfo>,
    platform: &'static str,
    /// The Linux desktop, e.g. "KDE" or "GNOME".
    desktop: String,
    wayland: bool,
}

#[derive(Serialize)]
struct Mic {
    id: String,
    name: String,
}

fn key_info(provider: &str) -> KeyInfo {
    match secrets::get(provider) {
        Some((key, src)) => {
            let chars: Vec<char> = key.chars().collect();
            let tail: String = chars[chars.len().saturating_sub(4)..].iter().collect();
            KeyInfo {
                present: true,
                masked: format!("{}{tail}", "•".repeat(12)),
                source: format!("{src:?}").to_lowercase(),
            }
        }
        None => KeyInfo { present: false, masked: String::new(), source: String::new() },
    }
}

/// The provider's real-time models; the known ones if it can't be asked.
#[tauri::command]
async fn list_models(provider: String) -> Models {
    let fallback = |error: String| Models { models: models::known(&provider), error };
    let Some((key, _)) = secrets::get(&provider) else { return fallback(String::new()) };
    let proxy_setting = Config::load(&config::config_path()).map(|c| c.proxy).unwrap_or_default();
    let proxy = match mirza_core::net::resolve_proxy(&proxy_setting) {
        Ok(p) => p,
        Err(e) => return fallback(e.to_string()),
    };
    match models::list(&provider, &key, proxy.as_ref()).await {
        Ok(list) if !list.is_empty() => Models { models: list, error: String::new() },
        Ok(_) => fallback(String::new()),
        Err(e) => fallback(e),
    }
}

#[tauri::command]
fn languages() -> Vec<models::Language> {
    models::default_languages()
}

#[tauri::command]
fn load() -> PanelState {
    let path = config::config_path();
    let (config, config_error) = match Config::load(&path) {
        Ok(c) => (c, String::new()),
        Err(e) => (Config::default(), e.to_string()),
    };
    PanelState {
        config,
        config_path: path.to_string_lossy().into_owned(),
        config_error,
        keys: PROVIDERS.iter().map(|p| (p.to_string(), key_info(p))).collect(),
        platform: std::env::consts::OS,
        desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        wayland: std::env::var_os("WAYLAND_DISPLAY").is_some(),
    }
}

/// Live state from the daemon; None when it isn't running.
#[tauri::command]
async fn snapshot() -> Option<Snapshot> {
    ipc::request(&Request::Snapshot).await.ok().and_then(|r| r.snapshot)
}

#[tauri::command]
async fn send(request: Request) -> Result<Reply, String> {
    ipc::request(&request).await.map_err(|e| e.to_string())
}

async fn edited(result: Result<(), String>) -> Result<(), String> {
    result?;
    let _ = ipc::request(&Request::Reload).await; // apply now rather than at the next check
    Ok(())
}

#[tauri::command]
async fn set_setting(path: String, value: serde_json::Value) -> Result<(), String> {
    let mut result = Ok(());
    let saved = config::update_file(&config::config_path(), |doc| result = config::set_json(doc, &path, &value));
    edited(result.and(saved.map_err(|e| e.to_string()))).await
}

#[tauri::command]
async fn set_shortcuts(list: Vec<Shortcut>) -> Result<(), String> {
    let saved = config::update_file(&config::config_path(), |doc| config::set_shortcuts(doc, &list));
    edited(saved.map_err(|e| e.to_string())).await
}

#[tauri::command]
fn set_key(provider: String, key: String) -> Result<KeyInfo, String> {
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(format!("unknown provider {provider:?}"));
    }
    secrets::set(&provider, &key).map_err(|e| e.to_string())?;
    Ok(key_info(&provider))
}

fn open(target: &str) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    cmd.arg(target).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_config() -> Result<(), String> {
    let path = config::config_path();
    if !path.exists() {
        let _ = Config::load(&path); // writes the commented template
    }
    open(&path.to_string_lossy())
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("only https links".into());
    }
    open(&url)
}

/// Opens the desktop's own shortcut settings (Linux, where the desktop owns
/// global shortcuts).
#[tauri::command]
fn open_shortcut_settings() -> Result<(), String> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_uppercase();
    let (prog, args): (&str, &[&str]) = if desktop.contains("KDE") {
        ("systemsettings", &["kcm_keys"])
    } else if desktop.contains("GNOME") {
        ("gnome-control-center", &["keyboard"])
    } else {
        return Err("Open your desktop's keyboard shortcut settings".into());
    };
    std::process::Command::new(prog).args(args).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Starts the Mirza daemon (installed next to this program, or on PATH).
#[tauri::command]
fn start_daemon() -> Result<(), String> {
    let name = if cfg!(windows) { "mirza.exe" } else { "mirza" };
    let beside = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(name)));
    let prog = beside.filter(|p| p.is_file()).unwrap_or_else(|| name.into());
    std::process::Command::new(prog).spawn().map(|_| ()).map_err(|e| format!("starting Mirza: {e}"))
}

/// Lets the page write to the panel's log (run it from a terminal to see).
#[tauri::command]
fn log(msg: String) {
    eprintln!("[settings] {msg}");
}

/// MIRZA_PANEL_DEBUG=1 makes the page report its layout to the log.
#[tauri::command]
fn debug_enabled() -> bool {
    std::env::var_os("MIRZA_PANEL_DEBUG").is_some()
}

/// Microphones the user can pick (Linux: PipeWire/PulseAudio sources).
#[tauri::command]
async fn list_mics() -> Vec<Mic> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    let out = tokio::process::Command::new("pactl").args(["-f", "json", "list", "sources"]).output().await;
    let Ok(out) = out else { return Vec::new() };
    let list: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    list.as_array()
        .map(|a| {
            a.iter()
                .filter(|s| s["monitor_source"].as_str().unwrap_or_default().is_empty())
                .filter_map(|s| {
                    let id = s["name"].as_str()?.to_owned();
                    let name = s["description"].as_str().unwrap_or(&id).to_owned();
                    Some(Mic { id, name })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn g_set_prgname(prgname: *const std::ffi::c_char);
}

fn main() {
    #[cfg(target_os = "linux")]
    {
        // Desktops find a window's icon through its app ID. GTK may take it
        // from the program name; KDE uses the executable name, which
        // mirza-panel.desktop covers.
        // SAFETY: called before GTK starts, with a static NUL-terminated string.
        unsafe { g_set_prgname(c"io.github.erfnemati.Mirza".as_ptr()) };
        // WebKitGTK uses about 35 MB less without its DMA-BUF renderer, and
        // some NVIDIA setups show a blank window with it.
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            // SAFETY: set before any other thread exists.
            unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .invoke_handler(tauri::generate_handler![
            load,
            snapshot,
            send,
            set_setting,
            set_shortcuts,
            set_key,
            open_config,
            open_url,
            open_shortcut_settings,
            start_daemon,
            list_mics,
            list_models,
            languages,
            log,
            debug_enabled
        ])
        .run(tauri::generate_context!())
        .expect("running the settings window");
}
