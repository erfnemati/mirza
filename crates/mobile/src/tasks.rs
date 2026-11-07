//! Voice tasks for the phone: the same queue, AI reading and Google calls as
//! on the desktop. The app signs in with Android's own Google sign-in and
//! hands access tokens in through [`GoogleTokens`]; everything else comes from
//! the phone's settings and keys ([`crate::MirzaCore::task_book`]).
//!
//! Every method that talks to Google or the AI blocks until it's done, so
//! call them from a background thread (Kotlin: `Dispatchers.IO`).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use mirza_core::ai::Client;
use mirza_core::tasks::google::{Endpoints, TokenError, TokenProvider, TokenSource};
use mirza_core::tasks::{self, Book, Draft, Google, Outcome, Said, Setup, Store, TaskSettings, when};

use super::{MobileError, runtime};
use crate::settings::{self, MobileConfig};

/// Gives Google access tokens from the app's own sign-in. Called from a
/// background thread.
#[uniffi::export(callback_interface)]
pub trait GoogleTokens: Send + Sync {
    /// A valid access token for the Tasks (and Calendar) scopes; `fresh`
    /// means Google refused the last one.
    fn access_token(&self, fresh: bool) -> GoogleToken;
}

/// The app's answer to [`GoogleTokens::access_token`]. An enum rather than an
/// error, since UniFFI can't carry a flat error from Kotlin back to Rust.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum GoogleToken {
    Token {
        token: String,
    },
    /// The user has to sign in (again). `detail` is a short reason; the core
    /// writes the sentence around it.
    SignedOut {
        detail: String,
    },
    /// Google couldn't be reached now, so the task waits and tries again.
    Offline {
        detail: String,
    },
}

struct Tokens(Box<dyn GoogleTokens>);

impl TokenProvider for Tokens {
    fn access_token(&self, fresh: bool) -> Result<String, TokenError> {
        match self.0.access_token(fresh) {
            GoogleToken::Token { token } => Ok(token),
            GoogleToken::SignedOut { detail } => Err(TokenError::SignedOut(detail)),
            GoogleToken::Offline { detail } => Err(TokenError::Network(detail)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TaskList {
    pub id: String,
    pub title: String,
}

/// A task as read from what was said.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TaskDraft {
    pub title: String,
    pub note: String,
    /// List id; "" for the default list.
    pub list: String,
    /// The list's name, as the app shows it.
    pub list_name: String,
    /// "2026-11-07", or "" for none.
    pub date: String,
    /// "15:00", or "" for none.
    pub time: String,
    /// Event length if one was said; 0 for the default.
    pub minutes: u32,
    /// The day and time in words, like "tomorrow 15:00"; "" for none.
    pub when_text: String,
    /// What was unclear or assumed.
    pub guesses: Vec<String>,
    /// Saved as said, without AI.
    pub raw: bool,
}

/// A task waiting to be saved.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct WaitingTask {
    pub id: String,
    pub text: String,
    pub said_ms: i64,
    pub attempts: u32,
    pub next_try_ms: i64,
    pub last_error: String,
    pub draft: Option<TaskDraft>,
    /// Said on the lock screen and not confirmed yet: see
    /// [`TaskBook::capture_held`].
    pub held: bool,
}

/// A task Google has.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SavedTask {
    pub id: String,
    pub text: String,
    pub draft: TaskDraft,
    pub has_event: bool,
    pub saved_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TaskState {
    /// Google has it.
    Saved,
    /// Kept; Mirza tries again by itself (see [`TaskBook::next_due_ms`]).
    Waiting,
    /// Kept, but the user must act first (sign in, turn on an API).
    Stuck,
    /// It was saved or undone already, or it waits for confirmation.
    Gone,
}

/// What happened, with the notification to show.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TaskOutcome {
    pub id: String,
    pub state: TaskState,
    /// Notification title and body; empty for [`TaskState::Gone`].
    pub title: String,
    pub body: String,
}

/// A change made in the edit form.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TaskEdit {
    pub title: String,
    pub note: String,
    pub list: String,
    /// "2026-11-07" or "".
    pub date: String,
    /// "15:00" or "".
    pub time: String,
    pub minutes: u32,
}

/// What the app needs to show tasks: the list names and today's date.
struct View {
    lists: Vec<tasks::List>,
    today: jiff::civil::Date,
}

impl View {
    fn draft(&self, d: &Draft) -> TaskDraft {
        TaskDraft {
            title: d.title.clone(),
            note: d.note.clone(),
            list: d.list.clone(),
            list_name: tasks::list_name(&self.lists, &d.list).to_owned(),
            date: d.date.map(|x| x.to_string()).unwrap_or_default(),
            time: d.time.map(|t| t.strftime("%H:%M").to_string()).unwrap_or_default(),
            minutes: d.minutes,
            when_text: when::describe(&d.when(), self.today),
            guesses: d.guesses.clone(),
            raw: d.raw,
        }
    }

    fn waiting(&self, e: &tasks::Entry) -> WaitingTask {
        WaitingTask {
            id: e.id.clone(),
            text: e.text.clone(),
            said_ms: e.said.at_ms,
            attempts: e.attempts,
            next_try_ms: e.next_try_ms,
            last_error: e.last_error.clone(),
            draft: e.draft.as_ref().map(|d| self.draft(d)),
            held: e.held,
        }
    }
}

/// What [`Book`] needs, from the phone's settings and keys.
fn setup(cfg: &MobileConfig, store: &settings::Store, tokens: Arc<dyn TokenProvider>) -> Result<Setup, MobileError> {
    let bad = |e: String| MobileError::Settings(e);
    let proxy = mirza_core::net::resolve_proxy(&cfg.proxy).map_err(|e| bad(e.to_string()))?;
    let t = &cfg.tasks;
    let settings = TaskSettings {
        provider: t.provider.trim().to_owned(),
        timeout_sec: t.timeout_sec,
        default_list: t.default_list.trim().to_owned(),
        event_minutes: t.event_minutes,
        timed_due_date: t.timed_due_date,
        list_hints: t.list_hints.clone(),
        instructions: t.instructions.clone(),
        ..TaskSettings::default()
    };
    let timeout = Duration::from_secs(t.timeout_sec.max(5));
    let (ai, ai_problem) = match settings.kind(&cfg.revision) {
        None => (None, "no AI is chosen for voice tasks".to_owned()),
        Some(kind) => {
            let key = store.key(kind.id());
            if kind.needs_key() && key.is_none() {
                (None, format!("there is no {} API key yet", kind.name()))
            } else {
                match Client::new(kind, cfg.ai.get(kind), key, proxy.as_ref(), timeout) {
                    Ok(c) => (Some(c), String::new()),
                    Err(e) => (None, e.to_string()),
                }
            }
        }
    };
    let terms = cfg.providers.get(&cfg.active_provider).map(|p| p.terms.clone()).unwrap_or_default();
    let google =
        Google::new(TokenSource::External(tokens), proxy.as_ref(), Endpoints::default(), Duration::from_secs(20))
            .map_err(|e| bad(e.to_string()))?;
    Ok(Setup { settings, ai, ai_problem, terms, google: Some(google), google_problem: String::new() })
}

/// The phone's voice tasks: the waiting ones, the saved ones, and what
/// changes them.
#[derive(uniffi::Object)]
pub struct TaskBook {
    book: Book,
    tokens: Arc<dyn TokenProvider>,
    setup: RwLock<Arc<Setup>>,
    time_zone: RwLock<String>,
    held_limit: AtomicU32,
}

impl TaskBook {
    /// The queue in `data_dir`, with the phone's settings.
    pub(crate) fn create(
        cfg: &MobileConfig,
        store: &settings::Store,
        data_dir: &str,
        time_zone: String,
        tokens: Box<dyn GoogleTokens>,
    ) -> Result<Arc<Self>, MobileError> {
        if data_dir.trim().is_empty() {
            return Err(MobileError::Settings("No folder for the task queue.".into()));
        }
        let tokens: Arc<dyn TokenProvider> = Arc::new(Tokens(tokens));
        let setup = setup(cfg, store, tokens.clone())?;
        Ok(Arc::new(Self {
            book: Book::new(Store::new(data_dir.trim())),
            tokens,
            setup: RwLock::new(Arc::new(setup)),
            time_zone: RwLock::new(time_zone),
            held_limit: AtomicU32::new(cfg.tasks.held_limit),
        }))
    }

    /// New settings or keys; the queue stays.
    pub(crate) fn reload(
        &self,
        cfg: &MobileConfig,
        store: &settings::Store,
        time_zone: String,
    ) -> Result<(), MobileError> {
        let s = setup(cfg, store, self.tokens.clone())?;
        *self.setup.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(s);
        *self.time_zone.write().unwrap_or_else(|e| e.into_inner()) = time_zone;
        self.held_limit.store(cfg.tasks.held_limit, Ordering::Relaxed);
        Ok(())
    }

    fn setup(&self) -> Arc<Setup> {
        self.setup.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn view(&self) -> View {
        let tz = self.time_zone.read().unwrap_or_else(|e| e.into_inner()).clone();
        View {
            lists: self.book.store().lists().map(|(_, l)| l).unwrap_or_default(),
            today: Said::at(jiff_now_ms(), &tz).zoned().date(),
        }
    }

    fn outcome(&self, o: &Outcome) -> TaskOutcome {
        let (id, state) = match o {
            Outcome::Saved(s) => (s.id.clone(), TaskState::Saved),
            Outcome::Waiting { entry, .. } => (entry.id.clone(), TaskState::Waiting),
            Outcome::Stuck { entry, .. } => (entry.id.clone(), TaskState::Stuck),
            Outcome::Gone(id) => (id.clone(), TaskState::Gone),
        };
        let v = self.view();
        let (title, body) = tasks::notification(o, &v.lists, v.today).unwrap_or_default();
        TaskOutcome { id, state, title, body }
    }
}

fn jiff_now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[uniffi::export]
impl TaskBook {
    /// Keeps what was said, on disk, before anything else. Fast; then call
    /// [`TaskBook::process`] with the id.
    pub fn capture(&self, text: String) -> Result<WaitingTask, MobileError> {
        if text.trim().is_empty() {
            return Err(MobileError::Settings("Nothing was heard.".into()));
        }
        let tz = self.time_zone.read().unwrap_or_else(|e| e.into_inner()).clone();
        let e = self
            .book
            .capture(&text, Said::at(jiff_now_ms(), &tz), false)
            .map_err(|e| MobileError::Settings(format!("Can't keep the task on the phone: {e}")))?;
        Ok(self.view().waiting(&e))
    }

    /// Keeps a task said on the lock screen, on disk only: nothing goes to
    /// the AI or to Google, and no retry touches it, until [`TaskBook::confirm`]
    /// (or [`TaskBook::edit`]). [`TaskBook::undo`] drops it. Fails when
    /// `held_limit` tasks are already waiting. Fast.
    pub fn capture_held(&self, text: String) -> Result<WaitingTask, MobileError> {
        if text.trim().is_empty() {
            return Err(MobileError::Settings("Nothing was heard.".into()));
        }
        let tz = self.time_zone.read().unwrap_or_else(|e| e.into_inner()).clone();
        let limit = match self.held_limit.load(Ordering::Relaxed) {
            0 => tasks::HELD_MAX,
            n => n as usize,
        };
        let e = self.book.hold(&text, Said::at(jiff_now_ms(), &tz), limit).map_err(MobileError::Settings)?;
        Ok(self.view().waiting(&e))
    }

    /// Confirms a task kept on the lock screen; then save it with
    /// [`TaskBook::process`] (or [`TaskBook::process_due`] for all of them).
    /// False when it isn't waiting for confirmation. Fast.
    pub fn confirm(&self, id: String) -> bool {
        self.book.release(&id)
    }

    /// Reads `text` the way a spoken task is read, without keeping or saving
    /// anything ("Try it" on the Voice tasks screen). Blocks.
    pub fn preview(&self, text: String) -> Result<TaskDraft, MobileError> {
        let setup = self.setup();
        runtime()
            .block_on(self.book.preview(&setup, &text))
            .map(|d| self.view().draft(&d))
            .map_err(MobileError::Settings)
    }

    /// Why tasks are saved word for word right now ("" when an AI reads them).
    pub fn ai_problem(&self) -> String {
        let setup = self.setup();
        if setup.ai.is_some() { String::new() } else { setup.ai_problem.clone() }
    }

    /// Saves one task: AI reading, Google Tasks, Calendar. Blocks.
    pub fn process(&self, id: String) -> TaskOutcome {
        let setup = self.setup();
        let o = runtime().block_on(self.book.process(&setup, &id));
        self.outcome(&o)
    }

    /// Saves every waiting task that is due. Blocks.
    pub fn process_due(&self) -> Vec<TaskOutcome> {
        let setup = self.setup();
        let out = runtime().block_on(self.book.process_due(&setup));
        out.iter().map(|o| self.outcome(o)).collect()
    }

    /// Makes every waiting task due now, e.g. after signing in.
    pub fn retry_now(&self) {
        self.book.retry_now();
    }

    /// When the next waiting task is due (ms since 1970), if any: schedule a
    /// background job for then.
    pub fn next_due_ms(&self) -> Option<i64> {
        self.book.next_due_ms()
    }

    pub fn waiting(&self) -> Vec<WaitingTask> {
        let v = self.view();
        self.book.waiting().iter().map(|e| v.waiting(e)).collect()
    }

    /// Saved tasks, newest first (the last 20).
    pub fn recent(&self) -> Vec<SavedTask> {
        let v = self.view();
        self.book
            .recent()
            .iter()
            .map(|s| SavedTask {
                id: s.id.clone(),
                text: s.text.clone(),
                draft: v.draft(&s.draft),
                has_event: s.event.is_some(),
                saved_ms: s.saved_ms,
            })
            .collect()
    }

    /// The account's lists; `refresh` reads them from Google even if the
    /// last read is recent. Blocks.
    pub fn lists(&self, refresh: bool) -> Result<Vec<TaskList>, MobileError> {
        let setup = self.setup();
        let lists =
            runtime().block_on(self.book.lists(&setup, refresh)).map_err(|e| MobileError::Settings(e.to_string()))?;
        Ok(lists.into_iter().map(|l| TaskList { id: l.id, title: l.title }).collect())
    }

    /// Takes a task back. Blocks.
    pub fn undo(&self, id: String) -> Result<(), MobileError> {
        let setup = self.setup();
        runtime().block_on(self.book.undo(&setup, &id)).map_err(MobileError::Settings)
    }

    /// Changes a task, waiting or saved. Blocks.
    pub fn edit(&self, id: String, edit: TaskEdit) -> Result<TaskOutcome, MobileError> {
        let bad = |s: &str| MobileError::Settings(s.to_owned());
        let date = match edit.date.trim() {
            "" => None,
            d => Some(when::parse_date(d).ok_or_else(|| bad("That date isn't valid."))?),
        };
        let time = match edit.time.trim() {
            "" => None,
            t => Some(when::parse_time(t).ok_or_else(|| bad("That time isn't valid."))?),
        };
        let e = tasks::Edit { title: edit.title, note: edit.note, list: edit.list, date, time, minutes: edit.minutes };
        let setup = self.setup();
        let o = runtime().block_on(self.book.edit(&setup, &id, e)).map_err(MobileError::Settings)?;
        Ok(self.outcome(&o))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoSignIn;

    impl GoogleTokens for NoSignIn {
        fn access_token(&self, _fresh: bool) -> GoogleToken {
            GoogleToken::SignedOut { detail: String::new() }
        }
    }

    /// A book in a fresh folder, with the settings `change` makes.
    fn book(name: &str, change: impl FnOnce(&mut MobileConfig)) -> (Arc<TaskBook>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("mirza-mobile-{name}-{}-{}", std::process::id(), jiff_now_ms()));
        let mut cfg = MobileConfig::default();
        change(&mut cfg);
        let store = settings::Store::new(&dir);
        let data = dir.join("tasks").to_string_lossy().into_owned();
        let book = TaskBook::create(&cfg, &store, &data, "Asia/Tehran".into(), Box::new(NoSignIn)).unwrap();
        (book, dir)
    }

    struct Offline;

    impl GoogleTokens for Offline {
        fn access_token(&self, _fresh: bool) -> GoogleToken {
            GoogleToken::Offline { detail: "no connection".into() }
        }
    }

    #[test]
    fn signed_out_tasks_stay_on_the_phone() {
        let (book, dir) = book("tasks", |_| {});
        assert_eq!(book.ai_problem(), "no AI is chosen for voice tasks");
        let w = book.capture("buy milk".into()).unwrap();
        let o = book.process(w.id.clone());
        assert_eq!(o.state, TaskState::Stuck);
        assert!(o.title.contains("buy milk"), "{o:?}");
        assert!(o.body.starts_with("Mirza isn't signed in to Google. Sign in"), "{o:?}");
        assert_eq!(book.waiting().len(), 1);
        assert!(book.next_due_ms().is_some());

        let edit = TaskEdit {
            title: "Buy milk".into(),
            note: String::new(),
            list: String::new(),
            date: "2026-11-09".into(),
            time: "25:00".into(),
            minutes: 0,
        };
        assert!(book.edit(w.id.clone(), edit.clone()).is_err());
        let o = book.edit(w.id.clone(), TaskEdit { time: "9:30".into(), ..edit }).unwrap();
        assert_eq!(o.state, TaskState::Stuck);
        let d = book.waiting()[0].draft.clone().unwrap();
        assert_eq!((d.title.as_str(), d.date.as_str(), d.time.as_str()), ("Buy milk", "2026-11-09", "09:30"));

        book.undo(w.id).unwrap();
        assert!(book.waiting().is_empty());
        let cfg = MobileConfig { proxy: "ftp://nowhere".into(), ..MobileConfig::default() };
        assert!(book.reload(&cfg, &settings::Store::new(&dir), String::new()).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lock_screen_tasks_wait_until_confirmed() {
        let (book, dir) = book("held", |c| c.tasks.held_limit = 1);
        let w = book.capture_held("buy milk".into()).unwrap();
        assert!(w.held);
        assert!(book.capture_held("call Sara".into()).is_err(), "the limit is 1");
        assert!(book.process_due().is_empty());
        assert_eq!(book.process(w.id.clone()).state, TaskState::Gone);
        assert_eq!(book.next_due_ms(), None);

        assert!(book.confirm(w.id.clone()));
        assert!(!book.waiting()[0].held);
        assert_eq!(book.process(w.id.clone()).state, TaskState::Stuck, "not signed in");
        book.reload(&MobileConfig::default(), &settings::Store::new(&dir), "Asia/Tehran".into()).unwrap();
        book.undo(w.id).unwrap();
        for _ in 0..tasks::HELD_MAX {
            book.capture_held("x".into()).unwrap();
        }
        assert!(book.capture_held("x".into()).is_err(), "the default is 10");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn offline_tasks_wait_and_try_again() {
        let dir = std::env::temp_dir().join(format!("mirza-mobile-offline-{}-{}", std::process::id(), jiff_now_ms()));
        let store = settings::Store::new(&dir);
        let data = dir.join("tasks").to_string_lossy().into_owned();
        let book = TaskBook::create(&MobileConfig::default(), &store, &data, String::new(), Box::new(Offline)).unwrap();
        let w = book.capture("buy milk".into()).unwrap();
        let o = book.process(w.id);
        assert_eq!(o.state, TaskState::Waiting, "{o:?}");
        assert!(o.body.contains("Couldn't reach Google sign-in"), "{o:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_ai_needs_its_key() {
        let (book, dir) = book("nokey", |c| c.tasks.provider = "claude".into());
        assert_eq!(book.ai_problem(), "there is no Claude API key yet");
        assert!(book.preview("buy milk".into()).is_err());
        let _ = std::fs::remove_dir_all(dir); // nothing was written
    }
}
