//! Voice tasks in the tray app: what was said is kept on disk at once, then
//! saved to Google in the background. The daemon's loop only starts the work
//! and shows the results; nothing here blocks it.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mirza_core::config::Config;
use mirza_core::ipc::Reply;
use mirza_core::tasks::{Book, Outcome, Said, Setup, Store};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};

use crate::daemon::Msg;

/// How often the queue is looked at for tasks due again.
const CHECK_EVERY: Duration = Duration::from_secs(15);
/// Notifications whose Undo and Edit buttons still work.
const NOTES_KEPT: usize = 30;

pub struct Tasks {
    pub book: Arc<Book>,
    pub setup: Arc<Setup>,
    /// Notification id and the task it is about, newest last.
    notes: VecDeque<(u32, String)>,
    /// The last task (id, title), for the tray's Undo and Edit.
    pub last: Option<(String, String)>,
    checked: Instant,
    /// A pass over the due tasks is running.
    busy: bool,
}

/// What a background task job reports to the daemon.
pub enum Done {
    /// Tasks were processed. `note` is the "Listening" notification to
    /// replace, `due_pass` whether this was the regular pass over the queue.
    Processed { outcomes: Vec<Outcome>, note: u32, due_pass: bool },
    /// A task was undone (or not); `quiet` when the settings window asked and
    /// shows the result itself.
    Undone { id: String, title: String, result: Result<(), String>, quiet: bool },
    /// A task was changed in the edit form.
    Edited(Box<Outcome>),
}

impl Tasks {
    pub fn new(cfg: &Config) -> Self {
        Self {
            book: Arc::new(Book::new(Store::new(Store::default_dir()))),
            setup: Arc::new(Setup::from_config(cfg)),
            notes: VecDeque::new(),
            last: None,
            checked: Instant::now(),
            busy: false,
        }
    }

    /// Reads the settings and the Google sign-in again.
    pub fn reload(&mut self, cfg: &Config) {
        self.setup = Arc::new(Setup::from_config(cfg));
    }

    /// Keeps a spoken task and starts saving it.
    pub fn capture(&mut self, text: &str, note: u32, tx: &mpsc::UnboundedSender<Msg>) -> Result<String, String> {
        let e = self
            .book
            .capture(text, Said::now(), false)
            .map_err(|e| format!("Mirza can't write the task to disk ({e})."))?;
        let (book, setup, tx, id) = (self.book.clone(), self.setup.clone(), tx.clone(), e.id.clone());
        tokio::spawn(async move {
            let o = book.process(&setup, &id).await;
            let _ = tx.send(Msg::Tasks(Done::Processed { outcomes: vec![o], note, due_pass: false }));
        });
        Ok(e.id)
    }

    /// Starts a pass over the waiting tasks when one is due.
    pub fn tick(&mut self, tx: &mpsc::UnboundedSender<Msg>) {
        if self.busy || self.checked.elapsed() < CHECK_EVERY {
            return;
        }
        self.checked = Instant::now();
        let now = jiff_now_ms();
        if self.book.next_due_ms().is_none_or(|t| t > now) {
            return;
        }
        self.run_due(tx);
    }

    fn run_due(&mut self, tx: &mpsc::UnboundedSender<Msg>) {
        self.busy = true;
        let (book, setup, tx) = (self.book.clone(), self.setup.clone(), tx.clone());
        tokio::spawn(async move {
            let outcomes = book.process_due(&setup).await;
            let _ = tx.send(Msg::Tasks(Done::Processed { outcomes, note: 0, due_pass: true }));
        });
    }

    /// After signing in, or when asked: every waiting task, now.
    pub fn retry(&mut self, cfg: &Config, tx: &mpsc::UnboundedSender<Msg>) {
        self.reload(cfg);
        self.book.retry_now();
        if !self.busy {
            self.run_due(tx);
        }
    }

    pub fn pass_done(&mut self) {
        self.busy = false;
    }

    /// Remembers which task a notification is about.
    pub fn noted(&mut self, note: u32, id: &str) {
        if note == 0 {
            return;
        }
        self.notes.retain(|(n, _)| *n != note);
        self.notes.push_back((note, id.to_owned()));
        while self.notes.len() > NOTES_KEPT {
            self.notes.pop_front();
        }
    }

    pub fn task_of(&self, note: u32) -> Option<String> {
        self.notes.iter().find(|(n, _)| *n == note).map(|(_, id)| id.clone())
    }

    /// The title a task goes by in messages.
    pub fn title_of(&self, id: &str) -> String {
        if let Some(s) = self.book.recent().into_iter().find(|s| s.id == id) {
            return s.draft.title;
        }
        match self.book.store().get(id) {
            Some(e) => e.draft.map(|d| d.title).unwrap_or_else(|| first_words(&e.text)),
            None => String::new(),
        }
    }

    /// Takes a task back in the background.
    pub fn undo(
        &self,
        id: String,
        quiet: bool,
        tx: &mpsc::UnboundedSender<Msg>,
        reply: Option<oneshot::Sender<Reply>>,
    ) {
        let (book, setup, tx) = (self.book.clone(), self.setup.clone(), tx.clone());
        let title = self.title_of(&id);
        tokio::spawn(async move {
            let result = book.undo(&setup, &id).await;
            if let Some(r) = reply {
                let _ = r.send(match &result {
                    Ok(()) => Reply::ok(),
                    Err(e) => Reply::err(e.clone()),
                });
            }
            let _ = tx.send(Msg::Tasks(Done::Undone { id, title, result, quiet }));
        });
    }

    /// Answers the settings window's and the command line's task requests,
    /// in the background when they wait for Google.
    pub fn request(
        &mut self,
        req: mirza_core::ipc::Request,
        reply: oneshot::Sender<Reply>,
        cfg: &Config,
        tx: &mpsc::UnboundedSender<Msg>,
    ) {
        use mirza_core::ipc::Request as R;
        let (book, setup) = (self.book.clone(), self.setup.clone());
        match req {
            R::Tasks => {
                let lists = book.store().lists().map(|(_, l)| l).unwrap_or_default();
                let data = json!({
                    "waiting": book.waiting(),
                    "recent": book.recent(),
                    "lists": lists,
                    "google": setup.google.is_some(),
                    "google_problem": setup.google_problem,
                    "ai": setup.ai.as_ref().map(|c| c.kind().name()),
                    "ai_problem": setup.ai_problem,
                });
                let _ = reply.send(Reply::data(data));
            }
            R::TaskLists { refresh } => {
                tokio::spawn(async move {
                    let r = match book.lists(&setup, refresh).await {
                        Ok(l) => Reply::data(json!(l)),
                        Err(e) => Reply::err(e.to_string()),
                    };
                    let _ = reply.send(r);
                });
            }
            R::AddTask { text } => {
                if text.trim().is_empty() {
                    let _ = reply.send(Reply::err("The task is empty."));
                    return;
                }
                let e = match book.capture(&text, Said::now(), false) {
                    Ok(e) => e,
                    Err(e) => {
                        let _ = reply.send(Reply::err(format!("Mirza can't write the task to disk ({e}).")));
                        return;
                    }
                };
                let tx = tx.clone();
                tokio::spawn(async move {
                    let o = book.process(&setup, &e.id).await;
                    let _ = reply.send(Reply::data(outcome_json(&o, &book)));
                    let _ = tx.send(Msg::Tasks(Done::Processed { outcomes: vec![o], note: 0, due_pass: false }));
                });
            }
            R::UndoTask { id } => self.undo(id, true, tx, Some(reply)),
            R::EditTask { id, edit } => {
                let tx = tx.clone();
                tokio::spawn(async move {
                    match book.edit(&setup, &id, edit).await {
                        Ok(o) => {
                            let _ = reply.send(Reply::data(outcome_json(&o, &book)));
                            let _ = tx.send(Msg::Tasks(Done::Edited(Box::new(o))));
                        }
                        Err(e) => {
                            let _ = reply.send(Reply::err(e));
                        }
                    }
                });
            }
            R::RetryTasks => {
                self.retry(cfg, tx);
                let _ = reply.send(Reply::ok());
            }
            _ => {
                let _ = reply.send(Reply::err("not a task request"));
            }
        }
    }
}

/// Whether a request is one [`Tasks::request`] answers.
pub fn is_task_request(req: &mirza_core::ipc::Request) -> bool {
    use mirza_core::ipc::Request as R;
    matches!(
        req,
        R::Tasks | R::TaskLists { .. } | R::AddTask { .. } | R::UndoTask { .. } | R::EditTask { .. } | R::RetryTasks
    )
}

/// An outcome for the settings window and the command line.
pub fn outcome_json(o: &Outcome, book: &Book) -> serde_json::Value {
    let lists = book.store().lists().map(|(_, l)| l).unwrap_or_default();
    let today = Said::now().zoned().date();
    let (title, body) = mirza_core::tasks::notification(o, &lists, today).unwrap_or_default();
    let (state, id) = match o {
        Outcome::Saved(s) => ("saved", s.id.as_str()),
        Outcome::Waiting { entry, .. } => ("waiting", entry.id.as_str()),
        Outcome::Stuck { entry, .. } => ("stuck", entry.id.as_str()),
        Outcome::Gone(id) => ("gone", id.as_str()),
    };
    json!({"state": state, "id": id, "title": title, "body": body})
}

fn first_words(text: &str) -> String {
    text.split_whitespace().take(5).collect::<Vec<_>>().join(" ")
}

fn jiff_now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or_default()
}
