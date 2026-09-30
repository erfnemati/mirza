//! The daemon's main loop: shortcuts, the tray, the socket and each dictation
//! session all send it messages, and it drives recording, the provider stream
//! and typing.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use mirza_core::audio::Recorder;
use mirza_core::config::{self, Action as ShortcutAction, Config, Mode, provider_name};
use mirza_core::ipc::{Reply, Request, Snapshot, State, Status, UsageInfo};
use mirza_core::providers::{self, StreamRequest, SttEvent};
use mirza_core::usage::{self, Date, Usage};
use mirza_core::{net, normalize, secrets};
use mirza_hotkey::Binding;
use mirza_inject::{Outcome, TypingSession};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::desktop::{self, Action as TrayAction, Desktop, TrayView};

/// The app's ID: its .desktop file name, and how the shortcuts portal knows it.
#[cfg(target_os = "linux")]
pub const APP_ID: &str = "io.github.erfnemati.Mirza";

/// Keep recording briefly after stop so the last word isn't clipped.
const STOP_TAIL: Duration = Duration::from_millis(250);
/// How long to wait for the rest of the transcript after the audio ends.
const FINISH_TIMEOUT: Duration = Duration::from_secs(10);
/// After the user lets go of a shortcut, wait this long before typing so no
/// modifier is still down.
const KEYS_UP_DELAY: Duration = Duration::from_millis(150);

pub enum Msg {
    Ipc(Request, oneshot::Sender<Reply>),
    Hotkey(mirza_hotkey::Event),
    Tray(TrayAction),
    Stt(u64, SttEvent),
    Typing(u64, mirza_inject::Event),
    HoldConnect(u64),
    StopTail(u64),
    ReleaseTyping(u64),
    Done(Done),
    #[cfg_attr(target_os = "macos", allow(dead_code))] // no focus tracking on macOS yet
    FocusChanged,
    RefreshUsage,
    Usage(String, Result<Usage, String>),
    /// Microphone loudness, 0 to 1, about 20 times a second while recording.
    Level(f32),
}

pub struct Done {
    id: u64,
    outcome: Outcome,
    error: Option<String>,
    reason: Reason,
    note: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reason {
    User,
    Silence,
    MaxLength,
    Cancelled,
    TooShort,
}

struct Session {
    id: u64,
    recorder: Option<Recorder>,
    /// Audio waiting for the connection; taken when connecting.
    audio_rx: Option<mpsc::Receiver<Vec<u8>>>,
    request: Option<StreamRequest>,
    stream: Option<JoinHandle<()>>,
    typing: Option<TypingSession>,
    started: Instant,
    last_heard: Instant,
    stopping: Option<Instant>,
    /// The shortcut held for a hold-to-talk session.
    hold: Option<String>,
    reason: Reason,
    note: u32,
    paused_noted: bool,
}

pub struct Daemon {
    tx: mpsc::UnboundedSender<Msg>,
    cfg: Config,
    cfg_path: PathBuf,
    cfg_mtime: Option<SystemTime>,
    desktop: Desktop,
    held: HashMap<String, Instant>,
    session: Option<Session>,
    next_id: u64,
    status: Status,
    history: VecDeque<String>,
    usage: Option<(String, Usage)>,
    usage_at: Option<Instant>,
    /// Set while the settings window records keys (see suspend_shortcuts).
    shortcuts_suspended: Option<Instant>,
    /// Loudest recent microphone level, for the recording animation.
    level_peak: f32,
    /// The recording dot's current size (see TrayView::frame).
    frame: u8,
    started_anim: Instant,
}

impl Daemon {
    pub async fn new(tx: mpsc::UnboundedSender<Msg>, cfg_path: PathBuf, desktop: Desktop) -> Result<Self, String> {
        let cfg = Config::load(&cfg_path).map_err(|e| e.to_string())?;
        let status = Status { provider: cfg.active_provider.clone(), ..Default::default() };
        Ok(Self {
            tx,
            cfg_mtime: mtime(&cfg_path),
            cfg,
            cfg_path,
            desktop,
            held: HashMap::new(),
            session: None,
            next_id: 1,
            status,
            history: VecDeque::new(),
            usage: None,
            usage_at: None,
            shortcuts_suspended: None,
            level_peak: 0.0,
            frame: 0,
            started_anim: Instant::now(),
        })
    }

    pub async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Msg>) {
        if let Err(e) = self.desktop.injector() {
            self.desktop.notify("Mirza can't type", &e, true, 0).await;
        }
        crate::autostart::sync(self.cfg.start_on_login);
        self.desktop.start(self.tray_view()).await;
        self.bind_shortcuts().await;
        self.refresh_usage(true);

        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut anim = tokio::time::interval(Duration::from_millis(125));
        anim.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    let Some(msg) = msg else { break };
                    if !self.handle(msg).await {
                        break;
                    }
                }
                _ = tick.tick() => self.tick().await,
                _ = anim.tick(), if self.status.state == State::Listening || self.frame != 0 => self.animate().await,
            }
        }
        self.cancel(Reason::Cancelled).await;
        self.desktop.shutdown().await;
        #[cfg(unix)]
        let _ = std::fs::remove_file(mirza_core::ipc::socket_path());
    }

    /// Handles one message; returns false to quit.
    async fn handle(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Ipc(req, reply) => {
                let quit = req == Request::Quit;
                let r = self.request(req).await;
                let _ = reply.send(r);
                return !quit;
            }
            Msg::Hotkey(ev) => self.hotkey(ev).await,
            Msg::Tray(a) => return self.tray_action(a).await,
            Msg::Stt(id, ev) => self.stt(id, ev).await,
            Msg::Typing(id, ev) => self.typing_event(id, ev).await,
            Msg::HoldConnect(id) => {
                if self.session.as_ref().is_some_and(|s| s.id == id && s.stopping.is_none()) {
                    self.connect();
                }
            }
            Msg::ReleaseTyping(id) => {
                if self.held.is_empty()
                    && let Some(t) = self.session.as_ref().filter(|s| s.id == id).and_then(|s| s.typing.as_ref())
                {
                    t.hold(false);
                }
            }
            Msg::StopTail(id) => {
                if let Some(s) = self.session.as_mut().filter(|s| s.id == id)
                    && let Some(r) = &s.recorder
                {
                    r.stop();
                }
            }
            Msg::Done(d) => self.done(d).await,
            Msg::FocusChanged => {
                if let Some(t) = self.session.as_ref().and_then(|s| s.typing.as_ref()) {
                    t.focus_changed();
                }
            }
            Msg::RefreshUsage => self.refresh_usage(false),
            Msg::Level(l) => self.level_peak = self.level_peak.max(l),
            Msg::Usage(provider, result) => match result {
                Ok(u) => {
                    self.usage = Some((provider, u));
                    self.update_tray().await;
                }
                Err(e) => tracing::info!("{e}"),
            },
        }
        true
    }

    async fn request(&mut self, req: Request) -> Reply {
        let result = match req {
            Request::Toggle => self.toggle().await,
            Request::Start => {
                if self.session.is_none() {
                    self.start(None).await
                } else {
                    Ok(())
                }
            }
            Request::Stop => {
                self.stop(Reason::User);
                Ok(())
            }
            Request::Cancel => {
                self.cancel(Reason::Cancelled).await;
                Ok(())
            }
            Request::Status => Ok(()),
            Request::OpenPanel => self.open_settings(),
            Request::NextProvider => self.next_provider().await,
            Request::SetProvider { id } => self.set_provider(&id).await,
            Request::Reload => {
                self.reload().await;
                Ok(())
            }
            Request::Quit => Ok(()),
            Request::Shortcut { id, pressed } => {
                let ev = if pressed { mirza_hotkey::Event::Pressed(id) } else { mirza_hotkey::Event::Released(id) };
                self.hotkey(ev).await;
                Ok(())
            }
            Request::Snapshot => return Reply { snapshot: Some(self.snapshot()), ..Reply::ok() },
            Request::SuspendShortcuts { suspend } => {
                self.suspend_shortcuts(suspend).await;
                Ok(())
            }
            Request::RefreshUsage => {
                self.refresh_usage(true);
                Ok(())
            }
        };
        match result {
            Ok(()) => Reply { status: Some(self.status.clone()), ..Reply::ok() },
            Err(e) => Reply { status: Some(self.status.clone()), ..Reply::err(e) },
        }
    }

    async fn toggle(&mut self) -> Result<(), String> {
        if self.session.is_some() {
            self.stop(Reason::User);
            Ok(())
        } else {
            self.start(None).await
        }
    }

    /// Starts a session. For hold-to-talk, the connection waits until the key
    /// has been held for the minimum time, so a tap costs nothing.
    async fn start(&mut self, hold: Option<String>) -> Result<(), String> {
        let result = self.try_start(hold).await;
        if let Err(e) = &result {
            self.status.last_error = e.clone();
            self.desktop.notify("Dictation failed", e, true, 0).await;
            self.update_tray().await;
        }
        result
    }

    async fn try_start(&mut self, hold: Option<String>) -> Result<(), String> {
        if self.session.is_some() {
            return Err("already dictating".into());
        }
        let cfg = &self.cfg;
        let provider = cfg.active_provider.clone();
        let name = provider_name(&provider).to_owned();
        let settings = cfg.providers.get(&provider).cloned().ok_or_else(|| format!("unknown provider {provider:?}"))?;
        let (api_key, _) = secrets::get(&provider)
            .ok_or_else(|| format!("No API key for {name}. Run `mirza set-key {provider}` or add it in Settings."))?;
        let proxy = net::resolve_proxy(&cfg.proxy).map_err(|e| e.to_string())?;
        let injector = self.desktop.injector()?;

        let rate = providers::sample_rate(&provider);
        let (atx, arx) = mpsc::channel(1200); // a minute of 50 ms chunks while connecting
        let level_tx = self.tx.clone();
        let recorder = Recorder::start(&cfg.record_cmd, &cfg.mic_device, rate, atx, move |l| {
            let _ = level_tx.send(Msg::Level(l));
        })
        .map_err(|e| format!("microphone: {e}"))?;
        self.started_anim = Instant::now();
        let focus = self.desktop.focus(cfg.typing.follow_focus);
        let id = self.next_id;
        self.next_id += 1;
        let tx = self.tx.clone();
        let opts = mirza_inject::Options {
            key_delay: Duration::from_millis(cfg.typing.key_delay_ms),
            paste_key: cfg.typing.paste_key.clone(),
            paste_over_chars: cfg.typing.paste_over_chars,
            keep_on_clipboard: cfg.typing.keep_on_clipboard,
        };
        let typing = injector.begin(opts, focus, move |e| {
            let _ = tx.send(Msg::Typing(id, e));
        });
        if !self.held.is_empty() {
            typing.hold(true); // the shortcut that started this is still down
        }
        let now = Instant::now();
        let is_hold = hold.is_some();
        self.session = Some(Session {
            id,
            recorder: Some(recorder),
            audio_rx: Some(arx),
            request: Some(StreamRequest { provider, api_key, settings, proxy }),
            stream: None,
            typing: Some(typing),
            started: now,
            last_heard: now,
            stopping: None,
            hold,
            reason: Reason::User,
            note: 0,
            paused_noted: false,
        });
        if is_hold {
            let tx = self.tx.clone();
            let wait = Duration::from_millis(self.cfg.min_hold_ms);
            tokio::spawn(async move {
                tokio::time::sleep(wait).await;
                let _ = tx.send(Msg::HoldConnect(id));
            });
        } else {
            self.connect();
        }
        self.status.state = State::Listening;
        self.status.last_error.clear();
        if self.cfg.notifications {
            let note = self.desktop.notify("Listening…", &name, false, 0).await;
            if let Some(s) = self.session.as_mut() {
                s.note = note;
            }
        }
        self.update_tray().await;
        Ok(())
    }

    /// Opens the provider stream with the audio recorded so far.
    fn connect(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        let (Some(req), Some(audio)) = (s.request.take(), s.audio_rx.take()) else { return };
        let (etx, mut erx) = mpsc::unbounded_channel();
        let tx = self.tx.clone();
        let id = s.id;
        tokio::spawn(async move {
            while let Some(ev) = erx.recv().await {
                if tx.send(Msg::Stt(id, ev)).is_err() {
                    return;
                }
            }
            // The stream always ends with Finished or Error; if it died without
            // one, end the session anyway (ignored if it already ended).
            let _ = tx.send(Msg::Stt(id, SttEvent::Error("the connection to the provider ended unexpectedly".into())));
        });
        s.stream = Some(providers::start(req, audio, etx));
    }

    /// Stops recording; the session ends when the provider has sent the rest.
    fn stop(&mut self, reason: Reason) {
        if self.session.as_ref().is_none_or(|s| s.stopping.is_some()) {
            return;
        }
        self.connect(); // a hold released right at the minimum may not be connected yet
        let Some(s) = self.session.as_mut() else { return };
        s.stopping = Some(Instant::now());
        s.reason = reason;
        let (tx, id) = (self.tx.clone(), s.id);
        tokio::spawn(async move {
            tokio::time::sleep(STOP_TAIL).await;
            let _ = tx.send(Msg::StopTail(id));
        });
        self.status.state = State::Finishing;
    }

    /// Ends the session now, keeping what was typed.
    async fn cancel(&mut self, reason: Reason) {
        if self.session.is_some() {
            self.end(None, Some(reason)).await;
        }
    }

    /// Tears the session down and finishes typing in the background.
    async fn end(&mut self, error: Option<String>, reason: Option<Reason>) {
        let Some(mut s) = self.session.take() else { return };
        if let Some(h) = s.stream.take() {
            h.abort();
        }
        drop(s.recorder.take());
        let reason = reason.unwrap_or(s.reason);
        let (id, note) = (s.id, s.note);
        let typing = s.typing.take();
        let tx = self.tx.clone();
        self.status.state = State::Finishing;
        tokio::task::spawn_blocking(move || {
            let outcome = typing.map(TypingSession::finish).unwrap_or_default();
            let _ = tx.send(Msg::Done(Done { id, outcome, error, reason, note }));
        });
        self.update_tray().await;
    }

    async fn done(&mut self, d: Done) {
        tracing::debug!(
            "session done: {} chars, held back {}, copied {}, error {:?}",
            d.outcome.text.chars().count(),
            d.outcome.held,
            d.outcome.copied,
            d.error
        );
        self.status.state = if self.session.is_some() { State::Listening } else { State::Idle };
        let text = d.outcome.text.trim().to_owned();
        if !text.is_empty() {
            self.status.last_text = text.clone();
            if self.cfg.history_size > 0 {
                self.history.push_back(text.clone());
                while self.history.len() > self.cfg.history_size {
                    self.history.pop_front();
                }
            }
        }
        if let Some(e) = d.error {
            self.status.last_error = e.clone();
            let body = if d.outcome.held || (!text.is_empty() && d.outcome.copied) {
                format!("{e}\nThe text so far is in your clipboard.")
            } else {
                e
            };
            self.desktop.notify("Dictation failed", &body, true, d.note).await;
        } else if d.reason == Reason::TooShort {
            self.desktop.close_notification(d.note).await;
        } else if self.cfg.notifications {
            let (summary, body) = match (d.reason, text.is_empty(), d.outcome.held) {
                (Reason::Cancelled, _, _) => ("Cancelled", String::new()),
                (_, true, _) => ("Stopped", "No speech recognized".into()),
                (_, false, true) => (
                    "Stopped",
                    "Part of the text wasn't typed because another window was active. It's in your clipboard.".into(),
                ),
                (Reason::Silence, false, false) => ("Stopped after silence", String::new()),
                (Reason::MaxLength, false, false) => ("Stopped: time limit reached", String::new()),
                _ => (
                    "Stopped",
                    if d.outcome.copied { "The text is also in your clipboard".into() } else { String::new() },
                ),
            };
            self.desktop.notify(summary, &body, false, d.note).await;
        } else if d.outcome.held {
            self.desktop.notify("Part of the text wasn't typed", "It's in your clipboard.", false, d.note).await;
        }
        let _ = d.id;
        self.update_tray().await;
        // The provider needs a moment to count the session.
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let _ = tx.send(Msg::RefreshUsage);
        });
        self.usage_at = None;
    }

    async fn stt(&mut self, id: u64, ev: SttEvent) {
        let persian = self.cfg.typing.persian_letters;
        let Some(s) = self.session.as_mut().filter(|s| s.id == id) else { return };
        match ev {
            SttEvent::Final(text) => {
                let text = if persian { normalize::persian_letters(&text) } else { text };
                tracing::debug!("final: {text:?}");
                if let Some(t) = &s.typing {
                    t.push(text);
                }
            }
            SttEvent::Partial(_) => {}
            SttEvent::Heard => s.last_heard = Instant::now(),
            SttEvent::Finished => {
                let error = s.stopping.is_none().then(|| "the microphone recording stopped unexpectedly".to_owned());
                self.end(error, None).await;
            }
            SttEvent::Error(e) => self.end(Some(e), None).await,
        }
    }

    async fn typing_event(&mut self, id: u64, ev: mirza_inject::Event) {
        let Some(s) = self.session.as_mut().filter(|s| s.id == id) else { return };
        match ev {
            mirza_inject::Event::Paused if !s.paused_noted => {
                s.paused_noted = true;
                tracing::info!("typing paused: another window is active");
                if self.cfg.notifications {
                    self.desktop
                        .notify(
                            "Typing paused",
                            "You left the window you started in. Go back to continue; if you stop first, the text will be in your clipboard.",
                            false,
                            0,
                        )
                        .await;
                }
            }
            mirza_inject::Event::Paused => {}
            mirza_inject::Event::Error(e) => tracing::warn!("typing: {e}"),
        }
    }

    async fn tick(&mut self) {
        if mtime(&self.cfg_path) != self.cfg_mtime {
            self.reload().await;
        }
        if self.shortcuts_suspended.is_some_and(|t| t.elapsed() > Duration::from_secs(30)) {
            self.suspend_shortcuts(false).await; // the settings window never said it was done
        }
        if self.usage_at.is_some_and(|t| t.elapsed() > Duration::from_secs(3600)) {
            self.refresh_usage(false);
        }
        let Some(s) = self.session.as_ref() else { return };
        match s.stopping {
            Some(t) if t.elapsed() > STOP_TAIL + FINISH_TIMEOUT => {
                self.end(Some("timed out waiting for the final transcript".into()), None).await;
            }
            Some(_) => {}
            None if s.stream.is_some() => {
                let silence = self.cfg.silence_stop_sec;
                let max = self.cfg.max_session_sec;
                if silence > 0 && s.last_heard.elapsed() >= Duration::from_secs(silence) {
                    self.stop(Reason::Silence);
                } else if max > 0 && s.started.elapsed() >= Duration::from_secs(max) {
                    self.stop(Reason::MaxLength);
                }
            }
            None => {}
        }
    }

    async fn hotkey(&mut self, ev: mirza_hotkey::Event) {
        tracing::debug!("shortcut event: {ev:?}");
        match ev {
            mirza_hotkey::Event::Pressed(id) => {
                if self.held.contains_key(&id) {
                    return; // key repeat
                }
                self.held.insert(id.clone(), Instant::now());
                if let Some(t) = self.session.as_ref().and_then(|s| s.typing.as_ref()) {
                    t.hold(true);
                }
                let Some(sc) = self.cfg.shortcuts.iter().find(|s| s.id() == id).cloned() else { return };
                let _ = match (sc.action, sc.mode) {
                    (ShortcutAction::Dictate, Mode::Toggle) => self.toggle().await,
                    (ShortcutAction::Dictate, Mode::Hold) => {
                        if self.session.is_none() {
                            self.start(Some(id)).await
                        } else {
                            self.stop(Reason::User);
                            Ok(())
                        }
                    }
                    (ShortcutAction::Cancel, _) => {
                        self.cancel(Reason::Cancelled).await;
                        Ok(())
                    }
                    (ShortcutAction::Panel, _) => self.open_settings(),
                    (ShortcutAction::NextProvider, _) => self.next_provider().await,
                };
            }
            mirza_hotkey::Event::Rebound(id, keys) => {
                // Changed in the desktop's settings: remember it in ours.
                let mut list = self.cfg.shortcuts.clone();
                let mut changed = false;
                for sc in list.iter_mut().filter(|s| s.id() == id) {
                    if !mirza_hotkey::Combo::same_keys(&sc.keys, &keys) {
                        sc.keys = keys.clone();
                        changed = true;
                    }
                }
                if changed {
                    match config::update_file(&self.cfg_path, |doc| config::set_shortcuts(doc, &list)) {
                        Ok(()) => {
                            self.cfg.shortcuts = list;
                            self.cfg_mtime = mtime(&self.cfg_path);
                        }
                        Err(e) => tracing::warn!("saving shortcut keys: {e}"),
                    }
                }
            }
            mirza_hotkey::Event::Cancelled(id) => {
                // The key was used for an ordinary shortcut (RightCtrl+C).
                self.held.remove(&id);
                if self.session.as_ref().is_some_and(|s| s.hold.as_deref() == Some(id.as_str()) && s.stream.is_none()) {
                    self.cancel(Reason::TooShort).await;
                }
            }
            mirza_hotkey::Event::Released(id) => {
                let pressed_at = self.held.remove(&id);
                let Some(s) = self.session.as_ref() else { return };
                if self.held.is_empty() {
                    let (tx, sid) = (self.tx.clone(), s.id);
                    tokio::spawn(async move {
                        tokio::time::sleep(KEYS_UP_DELAY).await;
                        let _ = tx.send(Msg::ReleaseTyping(sid));
                    });
                }
                if s.hold.as_deref() != Some(id.as_str()) || s.stopping.is_some() {
                    return;
                }
                let held_for = pressed_at.map(|t| t.elapsed()).unwrap_or_default();
                if held_for < Duration::from_millis(self.cfg.min_hold_ms) && s.stream.is_none() {
                    self.cancel(Reason::TooShort).await;
                } else {
                    self.stop(Reason::User);
                }
            }
        }
    }

    async fn tray_action(&mut self, a: TrayAction) -> bool {
        let result = match a {
            TrayAction::Toggle => self.toggle().await,
            TrayAction::Cancel => {
                self.cancel(Reason::Cancelled).await;
                Ok(())
            }
            TrayAction::SetProvider(id) => self.set_provider(&id).await,
            TrayAction::CopyLast => match (self.history.back(), self.desktop.injector()) {
                (Some(t), Ok(inj)) => inj.copy(t).map_err(|e| e.to_string()),
                _ => Ok(()),
            },
            TrayAction::OpenSettings => self.open_settings(),
            TrayAction::OpenConfigFile => {
                desktop::open_path(&self.cfg_path);
                Ok(())
            }
            TrayAction::Quit => return false,
        };
        if let Err(e) = result {
            self.desktop.notify("Mirza", &e, true, 0).await;
        }
        true
    }

    fn open_settings(&mut self) -> Result<(), String> {
        let name = if cfg!(windows) { "mirza-panel.exe" } else { "mirza-panel" };
        let panel = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(name)));
        match panel.filter(|p| p.is_file()) {
            Some(p) => {
                let mut child = std::process::Command::new(p).spawn().map_err(|e| e.to_string())?;
                // Collect its exit status when the window closes, so it doesn't linger.
                std::thread::spawn(move || child.wait());
                Ok(())
            }
            None => {
                desktop::open_path(&self.cfg_path);
                Ok(())
            }
        }
    }

    async fn next_provider(&mut self) -> Result<(), String> {
        let all = config::PROVIDERS;
        let i = all.iter().position(|p| *p == self.cfg.active_provider).unwrap_or(0);
        let next = all[(i + 1) % all.len()];
        self.set_provider(next).await?;
        self.desktop.notify("Provider", provider_name(next), false, 0).await;
        Ok(())
    }

    async fn set_provider(&mut self, id: &str) -> Result<(), String> {
        if !config::PROVIDERS.contains(&id) {
            return Err(format!("unknown provider {id:?}"));
        }
        config::update_file(&self.cfg_path, |doc| doc["active_provider"] = toml_edit::value(id))
            .map_err(|e| format!("saving the config: {e}"))?;
        self.cfg.active_provider = id.to_owned();
        self.cfg_mtime = mtime(&self.cfg_path);
        self.status.provider = id.to_owned();
        self.update_tray().await;
        Ok(())
    }

    async fn reload(&mut self) {
        self.cfg_mtime = mtime(&self.cfg_path);
        match Config::load(&self.cfg_path) {
            Ok(cfg) => {
                let rebind = cfg.shortcuts != self.cfg.shortcuts;
                if cfg.start_on_login != self.cfg.start_on_login {
                    crate::autostart::sync(cfg.start_on_login);
                }
                self.cfg = cfg;
                self.status.provider = self.cfg.active_provider.clone();
                if rebind {
                    self.bind_shortcuts().await;
                }
                self.update_tray().await;
            }
            Err(e) => {
                self.desktop.notify("Mirza settings not applied", &e.to_string(), true, 0).await;
            }
        }
    }

    async fn suspend_shortcuts(&mut self, suspend: bool) {
        if suspend {
            self.shortcuts_suspended = Some(Instant::now());
            self.held.clear();
            let _ = self.desktop.bind_shortcuts(Vec::new()).await;
        } else if self.shortcuts_suspended.take().is_some() {
            self.bind_shortcuts().await;
        }
    }

    async fn bind_shortcuts(&mut self) {
        if self.shortcuts_suspended.is_some() {
            return; // the settings window is recording keys; bound again when it's done
        }
        self.held.clear();
        let mut bindings: Vec<Binding> = Vec::new();
        for sc in &self.cfg.shortcuts {
            let id = sc.id();
            let safe = mirza_hotkey::Combo::parse(&sc.keys).is_ok_and(|c| c.is_safe_global());
            if !safe && !sc.keys.trim().is_empty() {
                tracing::warn!("not binding {:?}: a global shortcut needs Ctrl, Alt or Meta", sc.keys);
            }
            if safe && !bindings.iter().any(|b| b.id == id) {
                bindings.push(Binding { id, description: sc.description().into(), keys: sc.keys.clone() });
            }
        }
        if let Err(e) = self.desktop.bind_shortcuts(bindings).await {
            self.desktop.notify("Global shortcuts unavailable", &e, true, 0).await;
        }
    }

    /// Fetches the account's spend in the background, at most once a minute
    /// unless forced.
    fn refresh_usage(&mut self, force: bool) {
        if !force && self.usage_at.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
            return;
        }
        let provider = self.cfg.active_provider.clone();
        if provider != "soniox" {
            return; // the others have no usage API that works with a normal key
        }
        let Some((key, _)) = secrets::get(&provider) else { return };
        let Ok(proxy) = net::resolve_proxy(&self.cfg.proxy) else { return };
        let since = self.cfg.credit.get(&provider).and_then(|c| Date::parse(&c.since));
        self.usage_at = Some(Instant::now());
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let r = usage::soniox(&key, proxy.as_ref(), since).await;
            let _ = tx.send(Msg::Usage(provider, r));
        });
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            status: self.status.clone(),
            history: self.history.iter().cloned().collect(),
            usage: self.usage.as_ref().map(|(provider, u)| UsageInfo {
                provider: provider.clone(),
                today: u.today,
                month: u.month,
                month_minutes: u.month_minutes,
                requests: u.requests,
                since_credit: u.since_credit,
            }),
            shortcuts: self.desktop.bound_shortcuts(),
            shortcut_backend: self.desktop.shortcut_backend().into(),
            typing_problem: self.desktop.typing_problem(),
            focus_tracking: self.desktop.focus_tracking(),
        }
    }

    fn spend_line(&self) -> String {
        let Some((provider, u)) = &self.usage else { return String::new() };
        if *provider != self.cfg.active_provider {
            return String::new();
        }
        let mut s = format!("{} this month · {} today", usage::money(u.month), usage::money(u.today));
        if let (Some(c), Some(spent)) = (self.cfg.credit.get(provider), u.since_credit)
            && c.amount > 0.0
        {
            s.push_str(&format!(" · {} left", usage::money(c.amount - spent)));
        }
        s
    }

    /// Sizes the tray's red recording dot: it follows your voice, and breathes
    /// slowly while you're quiet so it's clear Mirza is still listening.
    async fn animate(&mut self) {
        let frame = if self.status.state == State::Listening {
            // audio::level maps -50..0 dBFS to 0..1; speech sits around 0.4 to 0.8.
            let voice = ((self.level_peak - 0.3) / 0.45).clamp(0.0, 1.0);
            let t = self.started_anim.elapsed().as_secs_f32();
            let breath = 0.2 * (1.0 - (t * std::f32::consts::TAU / 1.6).cos());
            self.level_peak *= 0.55; // fall back smoothly between words
            (voice.max(breath) * 5.0).round() as u8
        } else {
            0
        };
        if frame != self.frame {
            tracing::trace!("recording dot size {frame}");
            self.frame = frame;
            self.update_tray().await;
        }
    }

    fn tray_view(&self) -> TrayView {
        TrayView {
            frame: self.frame,
            state: self.status.state,
            provider: self.cfg.active_provider.clone(),
            error: self.status.last_error.clone(),
            has_last: !self.history.is_empty(),
            spend: self.spend_line(),
        }
    }

    async fn update_tray(&mut self) {
        self.desktop.show(self.tray_view()).await;
    }
}

fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}
