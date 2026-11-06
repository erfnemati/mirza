//! Voice tasks for the phone: the same queue, AI reading and Google calls as
//! on the desktop. The app signs in with Android's own Google sign-in and
//! hands access tokens in through [`GoogleTokens`].
//!
//! Every method that talks to Google or the AI blocks until it's done, so
//! call them from a background thread (Kotlin: `Dispatchers.IO`).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use mirza_core::ai::{AiProviders, Client, Kind};
use mirza_core::tasks::google::{Endpoints, TokenProvider, TokenSource};
use mirza_core::tasks::{self, Book, Draft, Google, Outcome, Said, Setup, Store, TaskSettings, when};

use super::{MobileError, runtime};

/// What voice tasks need to know. Rebuilt with [`TaskBook::update`] when the
/// settings change.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TaskConfig {
    /// Where the queue is kept: a folder in the app's own files.
    pub data_dir: String,
    /// "claude", "gemini", "openai", "openrouter", or "" to save tasks as said.
    pub ai_provider: String,
    /// Empty for the provider's default model.
    pub ai_model: String,
    pub ai_key: String,
    /// "" or "none" for a direct connection, or an http:// or socks5:// URL.
    pub proxy: String,
    /// Google Tasks list id for tasks that fit no list; "" for the account's
    /// default list.
    pub default_list: String,
    /// Other names for a list, by list id, e.g. "office, Acme".
    pub list_hints: HashMap<String, String>,
    /// Event length when none was said.
    pub event_minutes: u32,
    /// Whether a task with a clock time keeps its day in Google Tasks.
    pub timed_due_date: bool,
    /// Names spelled the way the user wants.
    pub terms: Vec<String>,
    /// The phone's time zone, e.g. "Asia/Tehran"; "" for the system's.
    pub time_zone: String,
}

/// Gives Google access tokens from the app's own sign-in. Called from a
/// background thread.
#[uniffi::export(callback_interface)]
pub trait GoogleTokens: Send + Sync {
    /// A valid access token for the Tasks (and Calendar) scopes; `fresh`
    /// means Google refused the last one. An error says why in plain words
    /// (for example that the user must sign in).
    fn access_token(&self, fresh: bool) -> Result<String, MobileError>;
}

struct Tokens(Box<dyn GoogleTokens>);

impl TokenProvider for Tokens {
    fn access_token(&self, fresh: bool) -> Result<String, String> {
        self.0.access_token(fresh).map_err(|e| e.to_string())
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
    /// "2026-11-07", or "" for none.
    pub date: String,
    /// "15:00", or "" for none.
    pub time: String,
    /// Event length if one was said; 0 for the default.
    pub minutes: u32,
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
}

/// A task Google has.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SavedTask {
    pub id: String,
    pub text: String,
    pub draft: TaskDraft,
    pub list_name: String,
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
    /// It was saved or undone already.
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

fn draft_out(d: &Draft) -> TaskDraft {
    TaskDraft {
        title: d.title.clone(),
        note: d.note.clone(),
        list: d.list.clone(),
        date: d.date.map(|x| x.to_string()).unwrap_or_default(),
        time: d.time.map(|t| t.strftime("%H:%M").to_string()).unwrap_or_default(),
        minutes: d.minutes,
        guesses: d.guesses.clone(),
        raw: d.raw,
    }
}

fn setup(c: &TaskConfig, tokens: Arc<dyn TokenProvider>) -> Result<Setup, MobileError> {
    let bad = |e: String| MobileError::Settings(e);
    let proxy = mirza_core::net::resolve_proxy(if c.proxy.trim().is_empty() { "none" } else { &c.proxy })
        .map_err(|e| bad(e.to_string()))?;
    let settings = TaskSettings {
        provider: c.ai_provider.trim().to_owned(),
        default_list: c.default_list.trim().to_owned(),
        event_minutes: c.event_minutes,
        timed_due_date: c.timed_due_date,
        list_hints: c.list_hints.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<BTreeMap<_, _>>(),
        ..TaskSettings::default()
    };
    let timeout = Duration::from_secs(settings.timeout_sec);
    let (ai, ai_problem) = match c.ai_provider.trim() {
        "" => (None, "no AI is chosen for voice tasks".to_owned()),
        p => {
            let kind = Kind::parse(p).ok_or_else(|| bad(format!("Unknown AI provider \"{p}\".")))?;
            let mut s = AiProviders::default().get(kind).clone();
            if !c.ai_model.trim().is_empty() {
                s.model = c.ai_model.trim().to_owned();
            }
            let key = Some(c.ai_key.trim().to_owned()).filter(|k| !k.is_empty());
            let client = Client::new(kind, &s, key, proxy.as_ref(), timeout).map_err(|e| bad(e.to_string()))?;
            (Some(client), String::new())
        }
    };
    let google =
        Google::new(TokenSource::External(tokens), proxy.as_ref(), Endpoints::default(), Duration::from_secs(20))
            .map_err(|e| bad(e.to_string()))?;
    Ok(Setup { settings, ai, ai_problem, terms: c.terms.clone(), google: Some(google), google_problem: String::new() })
}

/// The phone's voice tasks: the waiting ones, the saved ones, and what
/// changes them.
#[derive(uniffi::Object)]
pub struct TaskBook {
    book: Book,
    tokens: Arc<dyn TokenProvider>,
    setup: RwLock<Arc<Setup>>,
    time_zone: RwLock<String>,
}

impl TaskBook {
    fn setup(&self) -> Arc<Setup> {
        self.setup.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn lists_now(&self) -> Vec<tasks::List> {
        self.book.store().lists().map(|(_, l)| l).unwrap_or_default()
    }

    fn outcome(&self, o: &Outcome) -> TaskOutcome {
        let (id, state) = match o {
            Outcome::Saved(s) => (s.id.clone(), TaskState::Saved),
            Outcome::Waiting { entry, .. } => (entry.id.clone(), TaskState::Waiting),
            Outcome::Stuck { entry, .. } => (entry.id.clone(), TaskState::Stuck),
            Outcome::Gone(id) => (id.clone(), TaskState::Gone),
        };
        let today = Said::at(jiff_now_ms(), &self.time_zone.read().unwrap_or_else(|e| e.into_inner())).zoned().date();
        let (title, body) = tasks::notification(o, &self.lists_now(), today).unwrap_or_default();
        TaskOutcome { id, state, title, body }
    }
}

fn jiff_now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[uniffi::export]
impl TaskBook {
    #[uniffi::constructor]
    pub fn new(config: TaskConfig, tokens: Box<dyn GoogleTokens>) -> Result<Arc<Self>, MobileError> {
        if config.data_dir.trim().is_empty() {
            return Err(MobileError::Settings("No folder for the task queue.".into()));
        }
        let tokens: Arc<dyn TokenProvider> = Arc::new(Tokens(tokens));
        let setup = setup(&config, tokens.clone())?;
        Ok(Arc::new(Self {
            book: Book::new(Store::new(config.data_dir.trim())),
            tokens,
            setup: RwLock::new(Arc::new(setup)),
            time_zone: RwLock::new(config.time_zone),
        }))
    }

    /// New settings; the queue stays.
    pub fn update(&self, config: TaskConfig) -> Result<(), MobileError> {
        let s = setup(&config, self.tokens.clone())?;
        *self.setup.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(s);
        *self.time_zone.write().unwrap_or_else(|e| e.into_inner()) = config.time_zone;
        Ok(())
    }

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
        Ok(waiting_out(&e))
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
        self.book.waiting().iter().map(waiting_out).collect()
    }

    /// Saved tasks, newest first (the last 20).
    pub fn recent(&self) -> Vec<SavedTask> {
        let lists = self.lists_now();
        self.book
            .recent()
            .iter()
            .map(|s| SavedTask {
                id: s.id.clone(),
                text: s.text.clone(),
                draft: draft_out(&s.draft),
                list_name: tasks::list_name(&lists, &s.draft.list).to_owned(),
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

fn waiting_out(e: &tasks::Entry) -> WaitingTask {
    WaitingTask {
        id: e.id.clone(),
        text: e.text.clone(),
        said_ms: e.said.at_ms,
        attempts: e.attempts,
        next_try_ms: e.next_try_ms,
        last_error: e.last_error.clone(),
        draft: e.draft.as_ref().map(draft_out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoSignIn;

    impl GoogleTokens for NoSignIn {
        fn access_token(&self, _fresh: bool) -> Result<String, MobileError> {
            Err(MobileError::Settings("Sign in to Google in Mirza first.".into()))
        }
    }

    fn config(dir: &std::path::Path) -> TaskConfig {
        TaskConfig {
            data_dir: dir.to_string_lossy().into_owned(),
            ai_provider: String::new(),
            ai_model: String::new(),
            ai_key: String::new(),
            proxy: String::new(),
            default_list: String::new(),
            list_hints: HashMap::new(),
            event_minutes: 30,
            timed_due_date: true,
            terms: vec![],
            time_zone: "Asia/Tehran".into(),
        }
    }

    #[test]
    fn signed_out_tasks_stay_on_the_phone() {
        let dir = std::env::temp_dir().join(format!("mirza-mobile-tasks-{}", jiff_now_ms()));
        let book = TaskBook::new(config(&dir), Box::new(NoSignIn)).unwrap();
        let w = book.capture("buy milk".into()).unwrap();
        let o = book.process(w.id.clone());
        assert_eq!(o.state, TaskState::Stuck);
        assert!(o.title.contains("buy milk"), "{o:?}");
        assert!(o.body.contains("Sign in to Google in Mirza first"), "{o:?}");
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
        assert!(book.update(TaskConfig { ai_provider: "nope".into(), ..config(&dir) }).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
