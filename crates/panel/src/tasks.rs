//! The Voice tasks page: the Google client and sign-in, the lists, trying the
//! AI, and the tasks kept on disk. Saving, Undo and Edit go through the
//! running app, which owns the queue; everything here only reads it.

use std::sync::Mutex;

use mirza_core::config::{self, Config};
use mirza_core::ipc::{self, Request};
use mirza_core::secrets;
use mirza_core::tasks::{self, Book, Draft, List, Said, Setup, Store, settings, when};
use serde::Serialize;
use tauri::Emitter;
use tokio::sync::{mpsc, oneshot};

/// The `--edit-task` id the window was opened with, until the page asks.
pub static EDIT_ON_START: Mutex<Option<String>> = Mutex::new(None);

/// The id after `--edit-task` in a command line.
pub fn edit_arg(args: &[String]) -> Option<String> {
    let at = args.iter().position(|a| a == "--edit-task")?;
    args.get(at + 1).filter(|id| !id.is_empty()).cloned()
}

#[tauri::command]
pub fn edit_task_on_start() -> Option<String> {
    EDIT_ON_START.lock().unwrap_or_else(|e| e.into_inner()).take()
}

fn book() -> Book {
    Book::new(Store::new(Store::default_dir()))
}

fn load_config() -> Result<Config, String> {
    Config::load(&config::config_path()).map_err(|e| e.to_string())
}

/// A task as the page lists it.
#[derive(Serialize)]
pub struct TaskItem {
    id: String,
    title: String,
    /// The list's name, and the day and time in words ("tomorrow 15:00").
    list: String,
    when: String,
    /// What went wrong last, for a waiting task.
    error: String,
    /// The task as it stands, for the edit form; None until the AI read it.
    draft: Option<Draft>,
    /// What was said.
    text: String,
}

/// A task's list name and its day and time in words.
fn place(d: &Draft, lists: &[List]) -> (String, String) {
    (tasks::list_name(lists, &d.list).to_owned(), when::describe(&d.when(), Said::now().zoned().date()))
}

#[derive(Serialize)]
pub struct TasksState {
    /// The Google client in use; "" when there is none.
    client_id: String,
    /// The client came with this copy of Mirza.
    built_in: bool,
    signed_in: bool,
    /// Why tasks can't reach Google, if they can't.
    google_problem: String,
    /// The AI that reads tasks, "" when there's none, and why.
    ai: String,
    ai_problem: String,
    waiting: Vec<TaskItem>,
    recent: Vec<TaskItem>,
    /// The lists as last read from Google.
    lists: Vec<List>,
}

#[tauri::command]
pub fn tasks_state() -> TasksState {
    let cfg = Config::load(&config::config_path()).unwrap_or_default();
    let setup = Setup::from_config(&cfg);
    let book = book();
    let lists = book.store().lists().map(|(_, l)| l).unwrap_or_default();
    let client = cfg.tasks.client();
    let waiting = book
        .waiting()
        .into_iter()
        .map(|e| {
            let (list, when) = e.draft.as_ref().map(|d| place(d, &lists)).unwrap_or_default();
            TaskItem {
                title: e.draft.as_ref().map(|d| d.title.clone()).unwrap_or_else(|| e.text.clone()),
                list,
                when,
                error: e.last_error,
                draft: e.draft,
                text: e.text,
                id: e.id,
            }
        })
        .collect();
    let recent = book
        .recent()
        .into_iter()
        .map(|s| {
            let (list, when) = place(&s.draft, &lists);
            TaskItem {
                title: s.draft.title.clone(),
                list,
                when,
                error: String::new(),
                draft: Some(s.draft),
                text: s.text,
                id: s.id,
            }
        })
        .collect();
    TasksState {
        built_in: client.is_some() && cfg.tasks.google_client_id.trim().is_empty(),
        client_id: client.map(|(id, _)| id).unwrap_or_default(),
        signed_in: secrets::get(secrets::GOOGLE_REFRESH_TOKEN).is_some(),
        google_problem: setup.google_problem,
        ai: setup.ai.as_ref().map(|c| c.kind().id().to_owned()).unwrap_or_default(),
        ai_problem: setup.ai_problem,
        waiting,
        recent,
        lists,
    }
}

/// The lists in Google Tasks; read again from Google with `refresh`.
#[tauri::command]
pub async fn task_lists(refresh: bool) -> Result<Vec<List>, String> {
    let setup = Setup::from_config(&load_config()?);
    book().lists(&setup, refresh).await.map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct TriedTask {
    title: String,
    note: String,
    list: String,
    when: String,
    guesses: Vec<String>,
    ms: u64,
}

/// Reads `text` as a spoken task would be read, without saving it.
#[tauri::command]
pub async fn try_task(text: String) -> Result<TriedTask, String> {
    let setup = Setup::from_config(&load_config()?);
    let book = book();
    let started = std::time::Instant::now();
    let d = book.preview(&setup, &text).await?;
    let lists = book.store().lists().map(|(_, l)| l).unwrap_or_default();
    let (list, when) = place(&d, &lists);
    Ok(TriedTask {
        list,
        when,
        title: d.title,
        note: d.note,
        guesses: d.guesses,
        ms: started.elapsed().as_millis() as u64,
    })
}

/// Tells the running app to read the settings and the sign-in again and to
/// try the waiting tasks now. Nothing to do when it isn't running.
async fn wake_app() {
    let _ = ipc::request(&Request::RetryTasks).await;
}

/// Keeps the Google client pasted on the page: the JSON file Google gives,
/// or a client ID. A sign-in with another client ends first, since its
/// token only works with that client.
#[tauri::command]
pub async fn google_client(pasted: String) -> Result<String, String> {
    let (id, secret) = settings::read_client(&pasted)?;
    let cfg = load_config()?;
    if cfg.tasks.client().is_some_and(|(old, _)| old != id) {
        tasks::sign_out(&cfg, &Store::new(Store::default_dir())).await?;
    }
    settings::save_client(&config::config_path(), &id, secret.as_deref()).map_err(|e| e.to_string())?;
    wake_app().await;
    Ok(id)
}

/// Forgets the pasted client, and the sign-in made with it.
#[tauri::command]
pub async fn google_client_remove() -> Result<(), String> {
    let cfg = load_config()?;
    tasks::sign_out(&cfg, &Store::new(Store::default_dir())).await?;
    settings::save_client(&config::config_path(), "", None).map_err(|e| e.to_string())?;
    wake_app().await;
    Ok(())
}

/// A sign-in waiting for the browser: where a pasted address goes, and how
/// to stop it.
struct Waiting {
    paste: mpsc::Sender<String>,
    cancel: oneshot::Sender<()>,
}

static SIGN_IN: Mutex<Option<Waiting>> = Mutex::new(None);

const CANCELLED: &str = "Sign-in cancelled.";

/// Signs in to Google in the browser. The page gets the address as a
/// "google-sign-in" event, in case the browser doesn't open. Returns whether
/// Calendar was allowed too.
#[tauri::command]
pub async fn google_sign_in(app: tauri::AppHandle) -> Result<bool, String> {
    let cfg = load_config()?;
    let (paste, pasted) = mpsc::channel(1);
    let (cancel, cancelled) = oneshot::channel();
    let ours = paste.clone();
    // Starting again ends an earlier attempt: its cancel sender is dropped.
    *SIGN_IN.lock().unwrap_or_else(|e| e.into_inner()) = Some(Waiting { paste, cancel });
    let show = |url: &str| {
        let _ = app.emit("google-sign-in", url);
        if let Err(e) = crate::open(url) {
            eprintln!("[settings] can't open the browser: {e}");
        }
    };
    let result = tokio::select! {
        r = tasks::sign_in(&cfg, show, pasted) => r,
        _ = cancelled => Err(CANCELLED.to_owned()),
    };
    {
        let mut s = SIGN_IN.lock().unwrap_or_else(|e| e.into_inner());
        if s.as_ref().is_some_and(|w| w.paste.same_channel(&ours)) {
            *s = None;
        }
    }
    let granted = result?;
    wake_app().await;
    Ok(granted.calendar())
}

/// The address the browser ended at, from another device.
#[tauri::command]
pub fn google_sign_in_paste(address: String) -> Result<(), String> {
    let s = SIGN_IN.lock().unwrap_or_else(|e| e.into_inner());
    let w = s.as_ref().ok_or("No sign-in is waiting. Click Sign in first.")?;
    w.paste.try_send(address.trim().to_owned()).map_err(|_| "The sign-in is busy; wait a moment.".to_owned())
}

#[tauri::command]
pub fn google_sign_in_cancel() {
    if let Some(w) = SIGN_IN.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = w.cancel.send(());
    }
}

#[tauri::command]
pub async fn google_sign_out() -> Result<(), String> {
    let cfg = Config::load(&config::config_path()).unwrap_or_default();
    tasks::sign_out(&cfg, &Store::new(Store::default_dir())).await?;
    wake_app().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_argument() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(edit_arg(&args(&["mirza-panel", "--edit-task", "t1"])), Some("t1".into()));
        assert_eq!(edit_arg(&args(&["mirza-panel", "--edit-task"])), None);
        assert_eq!(edit_arg(&args(&["mirza-panel"])), None);
    }
}
