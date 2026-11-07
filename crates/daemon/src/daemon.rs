//! The daemon's main loop: shortcuts, the tray, the socket and each dictation
//! session all send it messages, and it drives recording, the provider stream
//! and typing.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use mirza_core::ai::revise::{self, Hints, Revised};
use mirza_core::audio::Recorder;
use mirza_core::audio::sound::{self, Sound};
use mirza_core::config::{self, Action as ShortcutAction, Config, Mode, provider_name};
use mirza_core::ipc::{Reply, Request, Snapshot, State, Status, UsageInfo};
use mirza_core::providers::{self, StreamRequest, SttEvent};
use mirza_core::update::{self, Release};
use mirza_core::usage::{self, Date, Usage};
use mirza_core::{net, normalize, secrets};
use mirza_hotkey::Binding;
use mirza_inject::{Outcome, TypingSession};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::desktop::{self, Action as TrayAction, Desktop, TrayView};
use crate::tasks::{self, Tasks};

/// The app's ID: on Linux its .desktop file name, and how the shortcuts portal
/// knows it; on Windows what its notifications are filed under.
#[cfg(any(target_os = "linux", windows))]
pub const APP_ID: &str = "io.github.erfnemati.Mirza";

/// Keep recording briefly after stop so the last word isn't clipped.
const STOP_TAIL: Duration = Duration::from_millis(250);
/// How long to wait for the rest of the transcript after the audio ends.
const FINISH_TIMEOUT: Duration = Duration::from_secs(10);
/// After the user lets go of a shortcut, wait this long before typing so no
/// modifier is still down.
const KEYS_UP_DELAY: Duration = Duration::from_millis(150);
/// The first update check waits a little after start, for the network to be up.
const UPDATE_FIRST: Duration = Duration::from_secs(20);
const UPDATE_INTERVAL: Duration = Duration::from_secs(24 * 3600);
/// After a failed check (offline, say), try again this much later.
const UPDATE_RETRY: Duration = Duration::from_secs(3600);

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
    /// The AI's revision of a session's text (or the original, if it failed).
    Revised(u64, Revised),
    #[cfg_attr(target_os = "macos", allow(dead_code))] // no focus tracking on macOS yet
    FocusChanged,
    RefreshUsage,
    Usage(String, Result<Usage, String>),
    Update(Result<Release, String>),
    /// Microphone loudness, 0 to 1, about 20 times a second while recording.
    Level(f32),
    /// Voice tasks: saving, undoing or changing one finished.
    Tasks(tasks::Done),
    /// A button on a notification was clicked: (notification id, action).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    NoteAction(u32, String),
    /// A button on a task's notification was clicked: (task id, action).
    #[cfg_attr(not(windows), allow(dead_code))]
    TaskAction(String, String),
}

pub struct Done {
    id: u64,
    outcome: Outcome,
    error: Option<String>,
    reason: Reason,
    note: u32,
    /// A revise session: what the speech provider heard, kept for Recent
    /// text even when nothing was typed.
    raw: Option<String>,
    /// Why the text was typed without AI revision.
    unrevised: Option<String>,
    /// A task session: what was said.
    task: Option<String>,
}

/// What a session is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Dictate,
    /// Dictate, revised with AI before typing.
    Revise,
    /// Say a task for Google Tasks; nothing is typed.
    Task,
}

/// The AI part of a "dictate and revise" session.
struct Revision {
    client: mirza_core::ai::Client,
    hints: Hints,
    timeout: u64,
    /// The final transcript so far; nothing is typed until the AI is done.
    transcript: String,
    /// While the AI works: when it started, and its task.
    running: Option<(Instant, JoinHandle<()>)>,
    /// Whether text has been handed to typing.
    typed: bool,
    unrevised: Option<String>,
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
    /// Whether the start and stop sounds have played.
    started_sound: bool,
    stopped_sound: bool,
    /// The provider sent its last words; later events are ignored.
    finished: bool,
    revision: Option<Revision>,
    /// A task session: the transcript so far.
    task: Option<String>,
}

/// One entry of Recent text.
struct HistoryItem {
    text: String,
    /// What the speech provider heard, when AI revised it.
    raw: String,
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
    history: VecDeque<HistoryItem>,
    usage: Option<(String, Usage)>,
    /// When usage was last asked for.
    usage_at: Option<Instant>,
    usage_loading: bool,
    usage_error: String,
    /// When usage last arrived, in Unix seconds.
    usage_updated: Option<u64>,
    /// Set while the settings window records keys (see suspend_shortcuts).
    shortcuts_suspended: Option<Instant>,
    /// Loudest recent microphone level, for the recording animation.
    level_peak: f32,
    /// The recording dot's current size (see TrayView::frame).
    frame: u8,
    started_anim: Instant,
    /// A newer release, when there is one.
    update: Option<Release>,
    /// When to check for updates next; None while a check is running.
    update_due: Option<Instant>,
    tasks: Tasks,
}

impl Daemon {
    pub async fn new(tx: mpsc::UnboundedSender<Msg>, cfg_path: PathBuf, desktop: Desktop) -> Result<Self, String> {
        let cfg = Config::load(&cfg_path).map_err(|e| e.to_string())?;
        let status = Status { provider: cfg.active_provider.clone(), ..Default::default() };
        let tasks = Tasks::new(&cfg);
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
            usage_loading: false,
            usage_error: String::new(),
            usage_updated: None,
            shortcuts_suspended: None,
            level_peak: 0.0,
            frame: 0,
            started_anim: Instant::now(),
            update: None,
            update_due: Some(Instant::now() + UPDATE_FIRST),
            tasks,
        })
    }

    pub async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Msg>) {
        if let Err(e) = self.desktop.injector() {
            self.desktop.notify("Mirza can't type", &e, true, 0).await;
        }
        crate::autostart::sync(self.cfg.start_on_login);
        self.desktop.start(self.tray_view()).await;
        self.bind_shortcuts().await;
        let _ = self.refresh_usage(true);

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
            Msg::Ipc(req, reply) if tasks::is_task_request(&req) => {
                self.tasks.request(req, reply, &self.cfg, &self.tx);
            }
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
                    self.session_sound(Sound::Start);
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
                if let Some(s) = self.session.as_mut().filter(|s| s.id == id) {
                    if let Some(r) = &s.recorder {
                        r.stop();
                    }
                    // Played now that recording has ended, so it isn't recorded.
                    self.session_sound(Sound::Stop);
                }
            }
            Msg::Done(d) => self.done(d).await,
            Msg::Revised(id, r) => self.revised(id, r).await,
            Msg::FocusChanged => {
                if let Some(t) = self.session.as_ref().and_then(|s| s.typing.as_ref()) {
                    t.focus_changed();
                }
            }
            Msg::RefreshUsage => {
                let _ = self.refresh_usage(false);
            }
            Msg::Level(l) => self.level_peak = self.level_peak.max(l),
            Msg::Usage(provider, result) => {
                self.usage_loading = false;
                match result {
                    Ok(u) => {
                        self.usage = Some((provider, u));
                        self.usage_error.clear();
                        self.usage_updated =
                            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).ok().map(|d| d.as_secs());
                        self.update_tray().await;
                    }
                    Err(e) => {
                        tracing::info!("usage: {e}");
                        self.usage_error = e;
                    }
                }
            }
            Msg::Update(result) => self.got_update(result).await,
            Msg::Tasks(done) => self.tasks_done(done).await,
            Msg::NoteAction(note, action) => {
                if let Some(id) = self.tasks.task_of(note) {
                    self.task_action(id, &action).await;
                }
            }
            Msg::TaskAction(id, action) => self.task_action(id, &action).await,
        }
        true
    }

    async fn request(&mut self, req: Request) -> Reply {
        let result = match req {
            Request::Toggle => self.toggle().await,
            Request::ToggleRevise => self.toggle_kind(Kind::Revise).await,
            Request::ToggleTask => self.toggle_kind(Kind::Task).await,
            Request::Start => {
                if self.session.is_none() {
                    self.start(None, Kind::Dictate).await
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
            Request::RefreshUsage => self.refresh_usage(true),
            Request::ClearHistory => {
                self.history.clear();
                self.status.last_text.clear();
                self.update_tray().await;
                Ok(())
            }
            Request::TestSound => {
                sound::play(Sound::Start, self.cfg.sound_volume);
                Ok(())
            }
            // Answered in `handle`, by `Tasks::request`.
            Request::Tasks
            | Request::TaskLists { .. }
            | Request::AddTask { .. }
            | Request::UndoTask { .. }
            | Request::EditTask { .. }
            | Request::RetryTasks => Ok(()),
        };
        match result {
            Ok(()) => Reply { status: Some(self.status.clone()), ..Reply::ok() },
            Err(e) => Reply { status: Some(self.status.clone()), ..Reply::err(e) },
        }
    }

    async fn toggle(&mut self) -> Result<(), String> {
        self.toggle_kind(Kind::Dictate).await
    }

    /// Starts a session of this kind, or acts on the one running.
    async fn toggle_kind(&mut self, kind: Kind) -> Result<(), String> {
        if self.session.is_some() {
            self.press_during_session().await;
            Ok(())
        } else {
            self.start(None, kind).await
        }
    }

    /// A dictation shortcut pressed during a session: stop recording, or,
    /// while the AI is revising, stop waiting and type the original text.
    async fn press_during_session(&mut self) {
        let revising = self.session.as_ref().and_then(|s| s.revision.as_ref()).is_some_and(|r| r.running.is_some());
        if revising {
            tracing::info!("revision skipped: typing the original text");
            self.end(None, None).await;
        } else {
            self.stop(Reason::User);
        }
    }

    /// Starts a session. For hold-to-talk, the connection waits until the key
    /// has been held for the minimum time, so a tap costs nothing.
    async fn start(&mut self, hold: Option<String>, kind: Kind) -> Result<(), String> {
        let result = self.try_start(hold, kind).await;
        if let Err(e) = &result {
            self.play(Sound::Error);
            self.status.last_error = e.clone();
            let title = if kind == Kind::Task { "Can't take a task" } else { "Dictation failed" };
            self.desktop.notify(title, e, true, 0).await;
            self.update_tray().await;
        }
        result
    }

    async fn try_start(&mut self, hold: Option<String>, kind: Kind) -> Result<(), String> {
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
        let revision = if kind == Kind::Revise {
            // Checked now, so a missing key doesn't cost a whole dictation.
            let kind = match cfg.revision.provider.trim() {
                "" => return Err("AI revision is off. Choose an AI provider in Settings > AI revision.".into()),
                p => mirza_core::ai::Kind::parse(p)
                    .ok_or_else(|| format!("Unknown AI provider {p:?} in the settings."))?,
            };
            let key = secrets::get(kind.id()).map(|(k, _)| k);
            // The time limit is applied per dictation, by its length.
            let client =
                mirza_core::ai::Client::new(kind, cfg.ai.get(kind), key, proxy.as_ref(), Duration::from_secs(600))
                    .map_err(|e| e.to_string())?;
            let hints = Hints::new(&cfg.revision, settings.languages.clone(), settings.terms.clone());
            Some(Revision {
                client,
                hints,
                timeout: cfg.revision.timeout_sec,
                transcript: String::new(),
                running: None,
                typed: false,
                unrevised: None,
            })
        } else {
            None
        };
        // A task is saved, not typed: it works without typing.
        let injector = if kind == Kind::Task { None } else { Some(self.desktop.injector()?) };

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
        let typing = injector.map(|injector| {
            let typing = injector.begin(opts, focus, move |e| {
                let _ = tx.send(Msg::Typing(id, e));
            });
            if !self.held.is_empty() {
                typing.hold(true); // the shortcut that started this is still down
            }
            typing
        });
        let now = Instant::now();
        let is_hold = hold.is_some();
        self.session = Some(Session {
            id,
            recorder: Some(recorder),
            audio_rx: Some(arx),
            request: Some(StreamRequest { provider, api_key, settings, proxy }),
            stream: None,
            typing,
            started: now,
            last_heard: now,
            stopping: None,
            hold,
            reason: Reason::User,
            note: 0,
            paused_noted: false,
            started_sound: false,
            stopped_sound: false,
            finished: false,
            revision,
            task: (kind == Kind::Task).then(String::new),
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
            self.session_sound(Sound::Start);
        }
        self.status.state = State::Listening;
        self.status.last_error.clear();
        if self.cfg.notifications {
            let ai = self.session.as_ref().and_then(|s| s.revision.as_ref()).map(|r| r.client.kind().name());
            let (title, body) = match (kind, ai) {
                (Kind::Task, _) => ("Say a task…", format!("{name}, then saved to Google Tasks")),
                (_, Some(ai)) => ("Listening…", format!("{name}, then revised by {ai}")),
                _ => ("Listening…", name),
            };
            let note = self.desktop.notify(title, &body, false, 0).await;
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
        let cancelled = matches!(reason, Some(Reason::Cancelled | Reason::TooShort));
        if let Some(r) = s.revision.as_mut() {
            if let Some((_, task)) = r.running.take() {
                task.abort();
            }
            // Not revised (an error, or the user didn't want to wait): type
            // what was heard, so nothing is lost. Cancelling types nothing;
            // the text stays in Recent text.
            if !r.typed && !cancelled && !r.transcript.trim().is_empty() {
                if let Some(t) = &s.typing {
                    t.push(r.transcript.trim().to_owned());
                }
                r.typed = true;
            }
        }
        if error.is_some() {
            self.play(Sound::Error);
        } else if s.started_sound && !s.stopped_sound {
            self.play(Sound::Stop); // cancelled, or the recording ended on its own
        }
        let reason = reason.unwrap_or(s.reason);
        let (id, note) = (s.id, s.note);
        let raw = s.revision.as_ref().map(|r| r.transcript.trim().to_owned());
        let unrevised = s.revision.as_mut().and_then(|r| r.unrevised.take());
        let task = s.task.take();
        let typing = s.typing.take();
        let tx = self.tx.clone();
        self.status.state = State::Finishing;
        tokio::task::spawn_blocking(move || {
            let outcome = typing.map(TypingSession::finish).unwrap_or_default();
            let _ = tx.send(Msg::Done(Done { id, outcome, error, reason, note, raw, unrevised, task }));
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
        if let Some(said) = d.task.clone() {
            self.task_session_done(d, said).await;
            return;
        }
        let text = d.outcome.text.trim().to_owned();
        let raw = d.raw.clone().unwrap_or_default();
        // A cancelled revise session typed nothing, but what was said is kept.
        let kept = if text.is_empty() { raw.clone() } else { text.clone() };
        if !kept.is_empty() {
            self.status.last_text = kept.clone();
            if self.cfg.history_size > 0 {
                let raw = if raw != kept { raw.clone() } else { String::new() };
                self.history.push_back(HistoryItem { text: kept.clone(), raw });
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
        } else if let Some(why) = d.unrevised {
            // Shown even with notifications off: the text isn't what was asked for.
            self.status.last_error = format!("Typed without AI revision. {why}");
            self.desktop.notify("Typed without AI revision", &why, false, d.note).await;
        } else if d.reason == Reason::TooShort {
            self.desktop.close_notification(d.note).await;
        } else if self.cfg.notifications {
            let (summary, body) = match (d.reason, text.is_empty(), d.outcome.held) {
                (Reason::Cancelled, _, _) if text.is_empty() && !raw.is_empty() => {
                    ("Cancelled", "What you said is in Recent text in Settings.".into())
                }
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
        // After Finished, a revise session waits for the AI; the provider
        // task's closing Error must not end it.
        let Some(s) = self.session.as_mut().filter(|s| s.id == id && !s.finished) else { return };
        match ev {
            SttEvent::Final(text) => {
                let text = if persian { normalize::persian_letters(&text) } else { text };
                tracing::debug!("final: {text:?}");
                if let Some(t) = s.task.as_mut() {
                    t.push_str(&text);
                } else if let Some(r) = s.revision.as_mut() {
                    r.transcript.push_str(&text);
                } else if let Some(t) = &s.typing {
                    t.push(text);
                }
            }
            SttEvent::Partial(_) => {}
            SttEvent::Heard => s.last_heard = Instant::now(),
            SttEvent::Finished => {
                s.finished = true;
                if s.stopping.is_none() {
                    self.end(Some("the microphone recording stopped unexpectedly".into()), None).await;
                } else if s.revision.is_some() {
                    self.start_revision().await;
                } else {
                    self.end(None, None).await;
                }
            }
            SttEvent::Error(e) => {
                s.finished = true;
                self.end(Some(e), None).await;
            }
        }
    }

    /// Sends the whole transcript to the AI; typing waits for its answer.
    async fn start_revision(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        let Some(r) = s.revision.as_mut() else { return };
        let text = r.transcript.trim().to_owned();
        if text.is_empty() {
            self.end(None, None).await;
            return;
        }
        let (client, hints) = (r.client.clone(), r.hints.clone());
        let timeout = revise::timeout_for(&text, r.timeout);
        let (tx, id) = (self.tx.clone(), s.id);
        let task = tokio::spawn(async move {
            let result = revise::revise_or_raw(&client, &text, &hints, timeout).await;
            let _ = tx.send(Msg::Revised(id, result));
        });
        r.running = Some((Instant::now(), task));
        let name = r.client.kind().name();
        if self.cfg.notifications {
            s.note = self.desktop.notify("Revising…", name, false, s.note).await;
        }
        self.status.state = State::Revising;
        self.update_tray().await;
    }

    async fn revised(&mut self, id: u64, result: Revised) {
        let persian = self.cfg.typing.persian_letters;
        let Some(s) = self.session.as_mut().filter(|s| s.id == id) else { return };
        let Some(r) = s.revision.as_mut().filter(|r| r.running.is_some()) else { return };
        if let Some((started, _)) = r.running.take() {
            tracing::debug!("revision came back after {} ms", started.elapsed().as_millis());
        }
        let text = if persian { normalize::persian_letters(&result.text) } else { result.text };
        if let Some(t) = &s.typing {
            t.push(text);
        }
        r.typed = true;
        r.unrevised = result.note;
        self.end(None, None).await;
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
        if self.usage_loading && self.usage_at.is_some_and(|t| t.elapsed() > Duration::from_secs(60)) {
            self.usage_loading = false; // the request never came back
        }
        if self.usage_at.is_some_and(|t| t.elapsed() > Duration::from_secs(3600)) {
            let _ = self.refresh_usage(false);
        }
        if self.cfg.check_updates && self.update_due.is_some_and(|t| Instant::now() >= t) {
            self.check_update();
        }
        self.tasks.tick(&self.tx);
        let Some(s) = self.session.as_ref() else { return };
        match s.stopping {
            // Once the transcript is complete, a revise session waits for the
            // AI, which has its own time limit.
            Some(t) if t.elapsed() > STOP_TAIL + FINISH_TIMEOUT && !s.finished => {
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
                let kind = match sc.action {
                    ShortcutAction::Revise => Kind::Revise,
                    ShortcutAction::Task => Kind::Task,
                    _ => Kind::Dictate,
                };
                let _ = match (sc.action, sc.mode) {
                    (ShortcutAction::Dictate | ShortcutAction::Revise | ShortcutAction::Task, Mode::Toggle) => {
                        self.toggle_kind(kind).await
                    }
                    (ShortcutAction::Dictate | ShortcutAction::Revise | ShortcutAction::Task, Mode::Hold) => {
                        if self.session.is_none() {
                            self.start(Some(id), kind).await
                        } else {
                            self.press_during_session().await;
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
            // The clipboard is busy with typing until the session ends.
            TrayAction::CopyLast if self.session.is_some() => {
                Err("Mirza is still dictating. Copy the last text once it's typed.".into())
            }
            TrayAction::CopyLast => match (self.history.back(), self.desktop.injector()) {
                (Some(h), Ok(inj)) => inj.copy(&h.text).map_err(|e| e.to_string()),
                _ => Ok(()),
            },
            TrayAction::OpenSettings => self.open_settings(),
            TrayAction::UndoTask => match self.tasks.last.clone() {
                Some((id, _)) => {
                    self.task_action(id, "undo").await;
                    Ok(())
                }
                None => Ok(()),
            },
            TrayAction::EditTask => match self.tasks.last.clone() {
                Some((id, _)) => {
                    self.task_action(id, "edit").await;
                    Ok(())
                }
                None => Ok(()),
            },
            TrayAction::OpenConfigFile => {
                desktop::open_path(&self.cfg_path);
                Ok(())
            }
            TrayAction::OpenUpdate => {
                if let Some(r) = &self.update {
                    desktop::open_path(&r.url);
                }
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
        self.open_panel(&[])
    }

    /// Opens the settings window with these arguments (e.g. a task to edit).
    fn open_panel(&mut self, args: &[&str]) -> Result<(), String> {
        let name = if cfg!(windows) { "mirza-panel.exe" } else { "mirza-panel" };
        let panel = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(name)));
        match panel.filter(|p| p.is_file()) {
            Some(p) => {
                let mut child = std::process::Command::new(p).args(args).spawn().map_err(|e| e.to_string())?;
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
                if !cfg.check_updates {
                    self.update = None;
                } else if !self.cfg.check_updates {
                    self.update_due = Some(Instant::now()); // just turned on
                }
                self.cfg = cfg;
                self.tasks.reload(&self.cfg);
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
    /// unless forced. An error says why it can't be fetched at all.
    fn refresh_usage(&mut self, force: bool) -> Result<(), String> {
        if self.usage_loading || (!force && self.usage_at.is_some_and(|t| t.elapsed() < Duration::from_secs(60))) {
            return Ok(());
        }
        let provider = self.cfg.active_provider.clone();
        let name = provider_name(&provider);
        if provider != "soniox" {
            // The others have no usage API that works with a normal key.
            return Err(format!("{name} doesn't share usage with apps"));
        }
        let (key, _) = secrets::get(&provider).ok_or_else(|| format!("No API key for {name}"))?;
        let proxy = net::resolve_proxy(&self.cfg.proxy).map_err(|e| e.to_string())?;
        let since = self.cfg.credit.get(&provider).and_then(|c| Date::parse(&c.since));
        self.usage_at = Some(Instant::now());
        self.usage_loading = true;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let r = usage::soniox(&key, proxy.as_ref(), since).await;
            let _ = tx.send(Msg::Usage(provider, r));
        });
        Ok(())
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            status: self.status.clone(),
            history: self.history.iter().map(|h| h.text.clone()).collect(),
            history_raw: self.history.iter().map(|h| h.raw.clone()).collect(),
            usage: self.usage.as_ref().map(|(provider, u)| UsageInfo {
                provider: provider.clone(),
                today: u.today,
                month: u.month,
                month_minutes: u.month_minutes,
                requests: u.requests,
                since_credit: u.since_credit,
            }),
            usage_loading: self.usage_loading,
            usage_error: self.usage_error.clone(),
            usage_updated: self.usage_updated,
            shortcuts: self.desktop.bound_shortcuts(),
            shortcut_backend: self.desktop.shortcut_backend().into(),
            typing_problem: self.desktop.typing_problem(),
            focus_tracking: self.desktop.focus_tracking(),
            version: update::VERSION.into(),
            update: self.update.clone(),
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
            update: self.update.as_ref().map(|r| r.version.clone()).unwrap_or_default(),
            last_task: self.tasks.last.as_ref().map(|(_, t)| t.clone()).unwrap_or_default(),
        }
    }

    async fn update_tray(&mut self) {
        self.desktop.show(self.tray_view()).await;
    }

    fn play(&self, sound: Sound) {
        if self.cfg.sounds {
            sound::play(sound, self.cfg.sound_volume);
        }
    }

    /// Plays the session's start or stop sound, each once, and the stop sound
    /// only after the start sound (a hold too short to count makes none).
    fn session_sound(&mut self, sound: Sound) {
        let Some(s) = self.session.as_mut() else { return };
        let play = match sound {
            Sound::Start => !std::mem::replace(&mut s.started_sound, true),
            _ => s.started_sound && !std::mem::replace(&mut s.stopped_sound, true),
        };
        if play {
            self.play(sound);
        }
    }

    /// Asks GitHub for the latest release in the background.
    fn check_update(&mut self) {
        let Ok(proxy) = net::resolve_proxy(&self.cfg.proxy) else {
            self.update_due = Some(Instant::now() + UPDATE_RETRY);
            return;
        };
        self.update_due = None;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let r = update::latest(proxy.as_ref()).await;
            let _ = tx.send(Msg::Update(r));
        });
    }

    async fn got_update(&mut self, result: Result<Release, String>) {
        let release = match result {
            Ok(r) => r,
            Err(e) => {
                tracing::info!("{e}");
                self.update_due = Some(Instant::now() + UPDATE_RETRY);
                return;
            }
        };
        self.update_due = Some(Instant::now() + UPDATE_INTERVAL);
        let newer = self.cfg.check_updates && update::is_newer(&release.version, update::VERSION);
        let release = newer.then_some(release);
        if release == self.update {
            return;
        }
        if let Some(r) = &release {
            tracing::info!("Mirza {} is available: {}", r.version, r.url);
            // Each release is announced once; the menu keeps showing it.
            let seen = config::data_dir().join("update-announced");
            if std::fs::read_to_string(&seen).unwrap_or_default().trim() != r.version {
                let summary = format!("Mirza {} is available", r.version);
                let body = format!("You have {}. Download it from the Mirza menu or settings.", update::VERSION);
                self.desktop.announce(&summary, &body).await;
                let _ = std::fs::create_dir_all(config::data_dir()).and_then(|_| std::fs::write(&seen, &r.version));
            }
        }
        self.update = release;
        self.update_tray().await;
    }
}

impl Daemon {
    /// A task session ended: keep what was said and start saving it.
    async fn task_session_done(&mut self, d: Done, said: String) {
        let said = said.trim().to_owned();
        let keep = |me: &mut Self, text: &str| {
            if !text.is_empty() && me.cfg.history_size > 0 {
                me.status.last_text = text.to_owned();
                me.history.push_back(HistoryItem { text: text.to_owned(), raw: String::new() });
                while me.history.len() > me.cfg.history_size {
                    me.history.pop_front();
                }
            }
        };
        if let Some(e) = d.error {
            keep(self, &said);
            self.status.last_error = e.clone();
            let body = if said.is_empty() { e } else { format!("{e}\nWhat you said is in Recent text in Settings.") };
            self.desktop.notify("Task not saved", &body, true, d.note).await;
        } else if d.reason == Reason::TooShort {
            self.desktop.close_notification(d.note).await;
        } else if d.reason == Reason::Cancelled {
            if self.cfg.notifications {
                self.desktop.notify("Cancelled", "No task was saved.", false, d.note).await;
            }
        } else if said.is_empty() {
            self.desktop.notify("No task heard", "Nothing was saved.", false, d.note).await;
        } else {
            match self.tasks.capture(&said, d.note, &self.tx) {
                Ok(_) => {
                    if self.cfg.notifications {
                        self.desktop.notify("Saving the task…", &said, false, d.note).await;
                    }
                }
                Err(e) => {
                    keep(self, &said);
                    let body = format!("{e} What you said is in Recent text in Settings.");
                    self.desktop.notify("Task not saved", &body, true, d.note).await;
                }
            }
        }
        self.update_tray().await;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let _ = tx.send(Msg::RefreshUsage);
        });
        self.usage_at = None;
    }

    async fn tasks_done(&mut self, done: tasks::Done) {
        match done {
            tasks::Done::Processed { outcomes, note, due_pass } => {
                if due_pass {
                    self.tasks.pass_done();
                }
                let lists = self.tasks.book.store().lists().map(|(_, l)| l).unwrap_or_default();
                let today = mirza_core::tasks::Said::now().zoned().date();
                for o in &outcomes {
                    // A pass over the queue only reports what got saved; the
                    // tasks still waiting were reported when they were said.
                    if due_pass && !matches!(o, mirza_core::tasks::Outcome::Saved(_)) {
                        continue;
                    }
                    let Some((title, body)) = mirza_core::tasks::notification(o, &lists, today) else { continue };
                    let id = match o {
                        mirza_core::tasks::Outcome::Saved(s) => s.id.clone(),
                        mirza_core::tasks::Outcome::Waiting { entry, .. }
                        | mirza_core::tasks::Outcome::Stuck { entry, .. } => entry.id.clone(),
                        mirza_core::tasks::Outcome::Gone(_) => continue,
                    };
                    let short = match o {
                        mirza_core::tasks::Outcome::Saved(s) => s.draft.title.clone(),
                        _ => self.tasks.title_of(&id),
                    };
                    let shown = self.desktop.notify_task(&title, &body, &id, note).await;
                    self.tasks.noted(shown, &id);
                    self.tasks.last = Some((id, short));
                }
            }
            tasks::Done::Undone { id, title, result, quiet } => match result {
                Ok(()) => {
                    if self.tasks.last.as_ref().is_some_and(|(l, _)| *l == id) {
                        self.tasks.last = None;
                    }
                    if !quiet {
                        self.desktop.notify("Task undone", &title, false, 0).await;
                    }
                }
                Err(e) if !quiet => {
                    self.desktop.notify("Task not undone", &e, true, 0).await;
                }
                Err(_) => {}
            },
            tasks::Done::Edited(o) => {
                if let mirza_core::tasks::Outcome::Saved(s) = &*o
                    && self.tasks.last.as_ref().is_some_and(|(l, _)| *l == s.id)
                {
                    self.tasks.last = Some((s.id.clone(), s.draft.title.clone()));
                }
            }
        }
        self.update_tray().await;
    }

    /// Undo or Edit, from a notification or the tray.
    async fn task_action(&mut self, id: String, action: &str) {
        tracing::debug!("task {id}: {action}");
        match action {
            "undo" => self.tasks.undo(id, false, &self.tx, None),
            "edit" => {
                if let Err(e) = self.open_panel(&["--edit-task", &id]) {
                    self.desktop.notify("Mirza", &e, true, 0).await;
                }
            }
            _ => {}
        }
    }
}

fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}
