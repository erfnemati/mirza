//! Voice tasks: a spoken task becomes a Google Tasks entry, and also an event
//! in Google Calendar when it has a clock time.
//!
//! [`Book::capture`] writes what was said to the queue at once.
//! [`Book::process`] reads it with AI (or keeps it as said), saves it to
//! Google Tasks, adds the Calendar event, and only then removes it from the
//! queue. Whatever changes Google runs one at a time, so a retry, an Undo and
//! an Edit never cross, and a task is never sent twice at once.

pub mod google;
pub mod oauth;
pub mod parse;
pub mod settings;
pub mod store;
pub mod when;

use std::time::Duration;

use jiff::civil::{Date, Time};
use serde::{Deserialize, Serialize};

pub use google::{Google, GoogleError};
pub use parse::{Draft, List};
pub use settings::TaskSettings;
pub use store::{Entry, Saved, Store};
pub use when::Said;

use google::{Credentials, Endpoints, ErrorKind, TokenSource};
use store::{TaskRef, event_id};

/// Everything a task needs that comes from the settings: rebuilt when they
/// change.
pub struct Setup {
    pub settings: TaskSettings,
    pub ai: Option<crate::ai::Client>,
    /// Why there's no AI, when there isn't.
    pub ai_problem: String,
    /// Names spelled the way the user wants (from the speech settings).
    pub terms: Vec<String>,
    pub google: Option<Google>,
    /// Why there's no Google, when there isn't.
    pub google_problem: String,
}

pub const NOT_SIGNED_IN: &str = "Mirza isn't signed in to Google. Sign in in Settings > Voice tasks.";
pub const NO_CLIENT: &str =
    "No Google client is set up for voice tasks. Paste the client file from Google in Settings > Voice tasks.";

impl Setup {
    /// From the config file and the stored keys.
    pub fn from_config(cfg: &crate::config::Config) -> Setup {
        let t = &cfg.tasks;
        let proxy = crate::net::resolve_proxy(&cfg.proxy);
        let timeout = Duration::from_secs(t.timeout_sec.max(5));
        let (ai, ai_problem) = match (t.kind(&cfg.revision), &proxy) {
            (None, _) => (None, "no AI is chosen for voice tasks".to_owned()),
            (Some(_), Err(e)) => (None, e.to_string()),
            (Some(kind), Ok(proxy)) => {
                let key = crate::secrets::get(kind.id()).map(|(k, _)| k);
                match crate::ai::Client::new(kind, cfg.ai.get(kind), key, proxy.as_ref(), timeout) {
                    Ok(c) => (Some(c), String::new()),
                    Err(e) => (None, e.to_string()),
                }
            }
        };
        let terms = cfg.providers.get(&cfg.active_provider).map(|p| p.terms.clone()).unwrap_or_default();
        let token = crate::secrets::get(crate::secrets::GOOGLE_REFRESH_TOKEN).map(|(k, _)| k);
        let (google, google_problem) = match (t.client(), token, &proxy) {
            (None, _, _) => (None, NO_CLIENT.to_owned()),
            (_, None, _) => (None, NOT_SIGNED_IN.to_owned()),
            (_, _, Err(e)) => (None, e.to_string()),
            (Some((client_id, client_secret)), Some(refresh_token), Ok(proxy)) => {
                let source = TokenSource::Refresh(Credentials { client_id, client_secret, refresh_token });
                match Google::new(source, proxy.as_ref(), Endpoints::default(), Duration::from_secs(20)) {
                    Ok(g) => (Some(g), String::new()),
                    Err(e) => (None, e.to_string()),
                }
            }
        };
        Setup { settings: t.clone(), ai, ai_problem, terms, google, google_problem }
    }

    fn default_list(&self) -> &str {
        self.settings.default_list.trim()
    }

    /// The due date to send: the task's day, unless the task has a clock
    /// time, gets an event, and the settings say a timed task has no date.
    fn due(&self, d: &Draft, event: bool) -> Option<Date> {
        if d.time.is_some() && event && !self.settings.timed_due_date { None } else { d.date }
    }

    fn minutes(&self, d: &Draft) -> u32 {
        if d.minutes > 0 { d.minutes } else { self.settings.event_minutes() }
    }
}

/// What happened to a task.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Google has it.
    Saved(Saved),
    /// Kept; Mirza tries again by itself.
    Waiting { entry: Entry, reason: String },
    /// Kept, but it needs you first (sign in, turn an API on).
    Stuck { entry: Entry, reason: String },
    /// Nothing to do: it was saved or undone already.
    Gone(String),
}

/// A change made in the edit form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edit {
    pub title: String,
    #[serde(default)]
    pub note: String,
    /// List id; "" for the default list.
    #[serde(default)]
    pub list: String,
    #[serde(default)]
    pub date: Option<Date>,
    #[serde(default)]
    pub time: Option<Time>,
    #[serde(default)]
    pub minutes: u32,
}

/// Lists read from Google are used for this long before being read again.
const LISTS_FRESH_MS: i64 = 10 * 60 * 1000;

/// Waits between tries: 30 s, 1, 2, 5, 10, then every 15 minutes.
fn backoff_ms(attempts: u32) -> i64 {
    const STEPS: [i64; 6] = [30, 60, 120, 300, 600, 900];
    STEPS[(attempts.max(1) as usize - 1).min(STEPS.len() - 1)] * 1000
}

/// How long a task that needs you waits before Mirza looks again.
const STUCK_MS: i64 = 15 * 60 * 1000;

/// At most this many tasks wait for the phone to be unlocked.
pub const HELD_MAX: usize = 10;

fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

/// The first sentence of an error, for a short note.
fn first_sentence(s: &str) -> String {
    match s.find(". ") {
        Some(i) => s[..i].to_owned(),
        None => s.trim_end_matches('.').to_owned(),
    }
}

/// The waiting tasks and the saved ones, and what changes them.
pub struct Book {
    store: Store,
    op: tokio::sync::Mutex<()>,
}

impl Book {
    pub fn new(store: Store) -> Book {
        Book { store, op: tokio::sync::Mutex::new(()) }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Keeps what was said, before anything else happens.
    pub fn capture(&self, text: &str, said: Said, held: bool) -> std::io::Result<Entry> {
        let e = Entry {
            id: store::new_id(said.at_ms),
            text: text.trim().to_owned(),
            said,
            held,
            attempts: 0,
            next_try_ms: 0,
            last_error: String::new(),
            draft: None,
            progress: Default::default(),
        };
        self.store.put(&e)?;
        tracing::info!("kept a spoken task ({} words) as {}", e.text.split_whitespace().count(), e.id);
        tracing::debug!("task text: {:?}", e.text);
        Ok(e)
    }

    /// Keeps a task said on the lock screen. Nothing goes to the AI or to
    /// Google until it's confirmed with [`Book::release`] (or edited); Undo
    /// drops it. At most `limit` (1 to [`HELD_MAX`]) wait at once.
    pub fn hold(&self, text: &str, said: Said, limit: usize) -> Result<Entry, String> {
        let limit = limit.clamp(1, HELD_MAX);
        if self.store.queue().iter().filter(|e| e.held).count() >= limit {
            return Err(format!(
                "{limit} tasks are already waiting for the phone to be unlocked. Unlock it to confirm or drop them."
            ));
        }
        self.capture(text, said, true).map_err(|e| format!("Can't keep the task: {e}"))
    }

    /// Confirms a task kept on the lock screen, so it's saved like any other.
    /// False when there's no such task waiting to be confirmed.
    pub fn release(&self, id: &str) -> bool {
        let Some(mut e) = self.store.get(id).filter(|e| e.held) else { return false };
        e.held = false;
        e.next_try_ms = 0;
        self.store.put(&e).is_ok()
    }

    /// Tasks waiting to be saved, oldest first.
    pub fn waiting(&self) -> Vec<Entry> {
        self.store.queue()
    }

    /// Saved tasks, newest first.
    pub fn recent(&self) -> Vec<Saved> {
        self.store.recent()
    }

    /// Makes every waiting task due now (after signing in, or by hand).
    pub fn retry_now(&self) {
        for mut e in self.store.queue() {
            if e.next_try_ms != 0 {
                e.next_try_ms = 0;
                let _ = self.store.put(&e);
            }
        }
    }

    /// When the next waiting task is due (ms), if any.
    pub fn next_due_ms(&self) -> Option<i64> {
        self.store.queue().iter().filter(|e| !e.held).map(|e| e.next_try_ms).min()
    }

    /// The account's lists: from the last read if it's recent and `refresh`
    /// isn't asked for, else from Google (or the last read if Google can't
    /// be reached).
    pub async fn lists(&self, setup: &Setup, refresh: bool) -> Result<Vec<List>, GoogleError> {
        let cached = self.store.lists();
        if !refresh
            && let Some((at, lists)) = &cached
            && now_ms() - at < LISTS_FRESH_MS
        {
            return Ok(lists.clone());
        }
        let Some(google) = &setup.google else {
            return match cached {
                Some((_, l)) if !refresh => Ok(l),
                _ => Err(GoogleError::new(ErrorKind::SignedOut, google::Api::SignIn, "")),
            };
        };
        match google.lists().await {
            Ok(lists) => {
                let _ = self.store.put_lists(now_ms(), &lists);
                Ok(lists)
            }
            Err(e) if e.retryable() && !refresh => cached.map(|(_, l)| l).ok_or(e),
            Err(e) => Err(e),
        }
    }

    /// Reads `text` the way a spoken task is read, without keeping or saving
    /// anything: for trying the AI in the settings window.
    pub async fn preview(&self, setup: &Setup, text: &str) -> Result<Draft, String> {
        let ai = setup.ai.as_ref().ok_or_else(|| setup.ai_problem.clone())?;
        let lists = self.lists(setup, false).await.unwrap_or_default();
        let said = Said::now().zoned();
        let req = parse::request(text, &lists, &setup.settings.list_hints, &setup.terms, &said);
        let timeout = Duration::from_secs(setup.settings.timeout_sec.max(5));
        parse::parse(ai, &req, &lists, setup.default_list(), &said, timeout).await.map_err(|e| e.to_string())
    }

    /// Saves one waiting task, or says why it's still waiting.
    pub async fn process(&self, setup: &Setup, id: &str) -> Outcome {
        let _op = self.op.lock().await;
        self.process_locked(setup, id).await
    }

    /// Saves every waiting task that is due.
    pub async fn process_due(&self, setup: &Setup) -> Vec<Outcome> {
        let now = now_ms();
        let mut out = Vec::new();
        for e in self.store.queue() {
            if e.held || e.next_try_ms > now {
                continue;
            }
            match self.process(setup, &e.id).await {
                Outcome::Gone(_) => {}
                o => out.push(o),
            }
        }
        out
    }

    fn failed(&self, mut e: Entry, err: GoogleError, now: i64) -> Outcome {
        e.attempts += 1;
        e.last_error = err.to_string();
        tracing::warn!("task {} not saved (try {}): {err}", e.id, e.attempts);
        let retry = err.retryable();
        e.next_try_ms = now + if retry { backoff_ms(e.attempts) } else { STUCK_MS };
        let _ = self.store.put(&e);
        let reason = e.last_error.clone();
        if retry { Outcome::Waiting { entry: e, reason } } else { Outcome::Stuck { entry: e, reason } }
    }

    async fn process_locked(&self, setup: &Setup, id: &str) -> Outcome {
        let Some(mut e) = self.store.get(id) else { return Outcome::Gone(id.to_owned()) };
        if e.held {
            return Outcome::Gone(id.to_owned());
        }
        let now = now_ms();
        let Some(g) = &setup.google else {
            e.attempts += 1;
            e.last_error = setup.google_problem.clone();
            e.next_try_ms = now + STUCK_MS;
            let _ = self.store.put(&e);
            let reason = e.last_error.clone();
            return Outcome::Stuck { entry: e, reason };
        };
        let lists = match self.lists(setup, false).await {
            Ok(l) => l,
            Err(err) => return self.failed(e, err, now),
        };
        let default = setup.default_list();

        let mut draft = match e.draft.clone() {
            Some(d) => d,
            None => match &setup.ai {
                Some(ai) => {
                    let said = e.said.zoned();
                    let req = parse::request(&e.text, &lists, &setup.settings.list_hints, &setup.terms, &said);
                    let timeout = Duration::from_secs(setup.settings.timeout_sec.max(5));
                    match parse::parse(ai, &req, &lists, default, &said, timeout).await {
                        Ok(d) => {
                            e.draft = Some(d.clone());
                            let _ = self.store.put(&e);
                            d
                        }
                        Err(err) => parse::raw(&e.text, default, first_sentence(&err.to_string())),
                    }
                }
                None => parse::raw(&e.text, default, setup.ai_problem.clone()),
            },
        };
        // A list removed in Google since: the default list instead.
        if !draft.list.is_empty() && !lists.iter().any(|l| l.id == draft.list) && !lists.is_empty() {
            draft.list = default.to_owned();
            draft.guesses.push("its list is gone, so it's in the default list".into());
        }
        let timed = draft.date.is_some() && draft.time.is_some();
        let event_wanted = timed && g.calendar_allowed();

        // The task.
        if e.progress.task.is_none() && e.progress.task_sent_ms > 0 {
            // Sent before, and the answer never came: it may be there.
            match g.find_task(&draft.list, &draft, e.progress.task_sent_ms).await {
                Ok(found) => e.progress.task = found,
                Err(err) => return self.failed(e, err, now),
            }
        }
        if e.progress.task.is_none() {
            e.draft = Some(draft.clone());
            e.progress.task_sent_ms = now;
            if let Err(err) = self.store.put(&e) {
                tracing::warn!("can't write task {}: {err}", e.id);
            }
            match g.insert_task(&draft.list, &draft, setup.due(&draft, event_wanted)).await {
                Ok(t) => e.progress.task = Some(t),
                Err(err) => {
                    if draft.raw && err.kind != ErrorKind::Timeout {
                        // Nothing was made: ask the AI again next time.
                        e.draft = None;
                        e.progress.task_sent_ms = 0;
                    }
                    return self.failed(e, err, now);
                }
            }
            let _ = self.store.put(&e);
        }
        let Some(task) = e.progress.task.clone() else { return Outcome::Gone(id.to_owned()) };

        // The event.
        if timed && e.progress.event.is_none() {
            // Known for sure by now: saving the task renewed the sign-in.
            let allowed = g.calendar_allowed();
            let mut no_calendar = !allowed;
            if allowed {
                let ev = event_id(&e.id, e.progress.event_gen);
                match g.insert_event(&ev, &e.id, &draft, setup.minutes(&draft), &e.said.time_zone()).await {
                    Ok(()) => {
                        e.progress.event = Some(ev);
                        let _ = self.store.put(&e);
                    }
                    Err(err) if err.kind == ErrorKind::NoAccess => no_calendar = true,
                    Err(err) => return self.failed(e, err, now),
                }
            }
            if no_calendar {
                draft.guesses.push("no Calendar event: the Google sign-in doesn't include Calendar".into());
                if setup.due(&draft, event_wanted).is_none() {
                    // Sent without a date because an event was expected.
                    let _ = g.patch_task(&task, &draft, draft.date).await;
                }
            }
        }

        let saved = Saved {
            id: e.id.clone(),
            text: e.text.clone(),
            said: e.said.clone(),
            draft,
            task,
            event: e.progress.event.clone(),
            event_gen: e.progress.event_gen,
            saved_ms: now,
        };
        if let Err(err) = self.store.put_recent(&saved) {
            tracing::warn!("can't write the recent tasks: {err}");
        }
        let _ = self.store.remove(&e.id);
        tracing::info!("saved task {} (event: {})", e.id, saved.event.is_some());
        Outcome::Saved(saved)
    }

    /// Takes a task back: a waiting one is forgotten, a saved one is deleted
    /// from Google Tasks and Calendar.
    pub async fn undo(&self, setup: &Setup, id: &str) -> Result<(), String> {
        let _op = self.op.lock().await;
        if let Some(e) = self.store.get(id) {
            if e.progress.task.is_some() || e.progress.event.is_some() {
                let g = setup.google.as_ref().ok_or_else(|| setup.google_problem.clone())?;
                if let Some(ev) = &e.progress.event {
                    g.delete_event(ev).await.map_err(|e| e.to_string())?;
                }
                if let Some(t) = &e.progress.task {
                    g.delete_task(t).await.map_err(|e| e.to_string())?;
                }
            }
            self.store.remove(id).map_err(|e| e.to_string())?;
            tracing::info!("undid waiting task {id}");
            return Ok(());
        }
        let Some(s) = self.store.recent().into_iter().find(|s| s.id == id) else {
            return Err("That task isn't among Mirza's recent tasks any more; change it in Google Tasks.".into());
        };
        let g = setup.google.as_ref().ok_or_else(|| setup.google_problem.clone())?;
        if let Some(ev) = &s.event {
            g.delete_event(ev).await.map_err(|e| e.to_string())?;
        }
        g.delete_task(&s.task).await.map_err(|e| e.to_string())?;
        self.store.remove_recent(id).map_err(|e| e.to_string())?;
        tracing::info!("undid task {id}");
        Ok(())
    }

    /// Changes a task: a waiting one before it's saved, a saved one in Google.
    pub async fn edit(&self, setup: &Setup, id: &str, edit: Edit) -> Result<Outcome, String> {
        let title = edit.title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            return Err("A task needs a title.".into());
        }
        if edit.time.is_some() && edit.date.is_none() {
            return Err("A time needs a day.".into());
        }
        let apply = |d: &mut Draft| {
            d.title = title.clone();
            d.note = edit.note.split_whitespace().collect::<Vec<_>>().join(" ");
            d.list = edit.list.trim().to_owned();
            d.date = edit.date;
            d.time = edit.time;
            d.minutes = edit.minutes.min(24 * 60);
            d.guesses.clear();
            d.raw = false;
        };
        let op = self.op.lock().await;
        if let Some(mut e) = self.store.get(id) {
            let mut d = e.draft.clone().unwrap_or_else(|| parse::raw(&e.text, setup.default_list(), String::new()));
            let old = d.clone();
            apply(&mut d);
            if let Some(mut task) = e.progress.task.clone() {
                let g = setup.google.as_ref().ok_or_else(|| setup.google_problem.clone())?;
                let mut event = e.progress.event.clone();
                let mut generation = e.progress.event_gen;
                change(setup, g, &e, &old, &d, &mut task, &mut event, &mut generation).await?;
                e.progress.task = Some(task);
                e.progress.event = event;
                e.progress.event_gen = generation;
            }
            e.draft = Some(d);
            e.next_try_ms = 0;
            e.held = false;
            self.store.put(&e).map_err(|e| e.to_string())?;
            drop(op);
            return Ok(self.process(setup, id).await);
        }
        let Some(mut s) = self.store.recent().into_iter().find(|s| s.id == id) else {
            return Err("That task isn't among Mirza's recent tasks any more; change it in Google Tasks.".into());
        };
        let g = setup.google.as_ref().ok_or_else(|| setup.google_problem.clone())?;
        let old = s.draft.clone();
        apply(&mut s.draft);
        let entry = Entry {
            id: s.id.clone(),
            text: s.text.clone(),
            said: s.said.clone(),
            held: false,
            attempts: 0,
            next_try_ms: 0,
            last_error: String::new(),
            draft: None,
            progress: Default::default(),
        };
        let new = s.draft.clone();
        change(setup, g, &entry, &old, &new, &mut s.task, &mut s.event, &mut s.event_gen).await?;
        self.store.put_recent(&s).map_err(|e| e.to_string())?;
        tracing::info!("changed task {id}");
        Ok(Outcome::Saved(s))
    }
}

/// Brings Google in line with an edited draft: moves and patches the task,
/// and patches, adds or deletes the event.
#[allow(clippy::too_many_arguments)]
async fn change(
    setup: &Setup,
    g: &Google,
    e: &Entry,
    old: &Draft,
    new: &Draft,
    task: &mut TaskRef,
    event: &mut Option<String>,
    generation: &mut u32,
) -> Result<(), String> {
    let s = |err: GoogleError| err.to_string();
    let timed = new.date.is_some() && new.time.is_some();
    let event_wanted = timed && g.calendar_allowed();
    if new.list != task.list && !(new.list.is_empty() && task.list == "@default") {
        *task = g.move_task(task, &new.list).await.map_err(s)?;
    }
    if new != old {
        g.patch_task(task, new, setup.due(new, event_wanted)).await.map_err(s)?;
    }
    let tz = e.said.time_zone();
    match (event.clone(), event_wanted) {
        (Some(ev), true) => g.patch_event(&ev, new, setup.minutes(new), &tz).await.map_err(s)?,
        (Some(ev), false) => {
            g.delete_event(&ev).await.map_err(s)?;
            *event = None;
            *generation += 1;
        }
        (None, true) => {
            let ev = event_id(&e.id, *generation);
            g.insert_event(&ev, &e.id, new, setup.minutes(new), &tz).await.map_err(s)?;
            *event = Some(ev);
        }
        (None, false) => {}
    }
    Ok(())
}

/// The list's name for people; "" is the account's default list, which
/// Google lists first.
pub fn list_name<'a>(lists: &'a [List], id: &str) -> &'a str {
    match lists.iter().find(|l| l.id == id) {
        Some(l) => &l.title,
        None if id.is_empty() || id == "@default" => {
            lists.first().map(|l| l.title.as_str()).unwrap_or("your default list")
        }
        None => "a list",
    }
}

/// A notification for an outcome: (title, body).
pub fn notification(o: &Outcome, lists: &[List], today: Date) -> Option<(String, String)> {
    match o {
        Outcome::Saved(s) => {
            let d = &s.draft;
            let mut line = list_name(lists, &d.list).to_owned();
            let when = when::describe(&d.when(), today);
            if !when.is_empty() {
                line.push_str(&format!(" · {when}"));
            }
            if s.event.is_some() {
                line.push_str(" · in Calendar too");
            }
            let mut body = vec![line];
            body.extend(d.guesses.iter().cloned());
            Some((d.title.clone(), body.join("\n")))
        }
        Outcome::Waiting { entry, reason } | Outcome::Stuck { entry, reason } => {
            let words = entry.draft.as_ref().map(|d| d.title.clone()).unwrap_or_else(|| {
                let w: Vec<&str> = entry.text.split_whitespace().take(5).collect();
                w.join(" ")
            });
            let title = format!("Task kept, not saved yet: {words}");
            let tail = match o {
                // Most reasons already say so.
                Outcome::Waiting { .. } if reason.contains("tries again") => "",
                Outcome::Waiting { .. } => "Mirza tries again by itself.",
                _ => "It's kept until then.",
            };
            Some((title, format!("{reason} {tail}").trim_end().to_owned()))
        }
        Outcome::Gone(_) => None,
    }
}

/// Signs in to Google with the client in the settings and keeps the refresh
/// token like an API key. `show` gets the address to open (it should open
/// the browser); `pasted` may bring the address the browser ended at.
pub async fn sign_in(
    cfg: &crate::config::Config,
    show: impl FnOnce(&str),
    pasted: tokio::sync::mpsc::Receiver<String>,
) -> Result<oauth::Granted, String> {
    let (id, secret) = cfg.tasks.client().ok_or(NO_CLIENT)?;
    let ep = Endpoints::default();
    let attempt = oauth::start(&ep, &id).await.map_err(|e| format!("Can't start the sign-in: {e}"))?;
    show(&attempt.url);
    let code = attempt.code(pasted, Duration::from_secs(300)).await?;
    let proxy = crate::net::resolve_proxy(&cfg.proxy).map_err(|e| e.to_string())?;
    let http = crate::usage::http_client(proxy.as_ref(), Duration::from_secs(30))?;
    let granted = attempt.exchange(&http, &ep, &id, secret.as_deref(), &code).await.map_err(|e| e.to_string())?;
    if !granted.tasks() {
        return Err("Google didn't allow Mirza to use Tasks. Sign in again and tick the Tasks box.".into());
    }
    crate::secrets::set(crate::secrets::GOOGLE_REFRESH_TOKEN, &granted.refresh_token)
        .map_err(|e| format!("Signed in, but can't save the sign-in: {e}"))?;
    tracing::info!("signed in to Google (calendar: {})", granted.calendar());
    Ok(granted)
}

/// Signs out: asks Google to end the sign-in and forgets it here. Waiting
/// tasks stay.
pub async fn sign_out(cfg: &crate::config::Config, store: &Store) -> Result<(), String> {
    if let Some((token, _)) = crate::secrets::get(crate::secrets::GOOGLE_REFRESH_TOKEN) {
        let proxy = crate::net::resolve_proxy(&cfg.proxy).ok().flatten();
        if let Ok(http) = crate::usage::http_client(proxy.as_ref(), Duration::from_secs(15))
            && let Err(e) = oauth::revoke(&http, &Endpoints::default(), &token).await
        {
            tracing::warn!("Google didn't confirm the sign-out: {e}");
        }
    }
    crate::secrets::set(crate::secrets::GOOGLE_REFRESH_TOKEN, "").map_err(|e| e.to_string())?;
    store.forget_account();
    Ok(())
}

#[cfg(test)]
mod tests;
