//! Spoken tasks on disk until Google has them, the recently saved ones (for
//! Undo and Edit, also after a restart), and the last list of Google Tasks
//! lists. One file per waiting task, written before any network call, so
//! nothing said is lost when the computer is offline or Mirza stops.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::{Draft, List, Said};
use crate::config::write_private;

/// A task in Google Tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRef {
    pub list: String,
    pub id: String,
}

/// How far saving a waiting task got.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    pub task: Option<TaskRef>,
    /// When the task was last sent to Google (ms), so a retry after an
    /// unclear failure looks for it before sending it again.
    pub task_sent_ms: i64,
    /// The Calendar event's id, once Google has it.
    pub event: Option<String>,
    /// Counts fresh events for this task: Calendar never reuses the id of a
    /// deleted event.
    pub event_gen: u32,
}

/// A spoken task waiting to be saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// What was said.
    pub text: String,
    pub said: Said,
    /// Kept until confirmed (the phone's lock-screen queue).
    #[serde(default)]
    pub held: bool,
    #[serde(default)]
    pub attempts: u32,
    /// Don't try again before this (ms).
    #[serde(default)]
    pub next_try_ms: i64,
    #[serde(default)]
    pub last_error: String,
    /// The AI's reading, once there is one, so a retry doesn't ask again.
    #[serde(default)]
    pub draft: Option<Draft>,
    #[serde(default)]
    pub progress: Progress,
}

/// A task Google has.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub id: String,
    pub text: String,
    pub said: Said,
    pub draft: Draft,
    pub task: TaskRef,
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub event_gen: u32,
    pub saved_ms: i64,
}

/// How many saved tasks are kept for Undo and Edit.
pub const RECENT: usize = 20;

#[derive(Debug, Default, Serialize, Deserialize)]
struct ListCache {
    fetched_ms: i64,
    lists: Vec<List>,
}

pub struct Store {
    dir: PathBuf,
    /// Every change goes through here, so two writers never race on a file.
    lock: Mutex<()>,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Store {
        Store { dir: dir.into(), lock: Mutex::new(()) }
    }

    /// `<data dir>/tasks`.
    pub fn default_dir() -> PathBuf {
        crate::config::data_dir().join("tasks")
    }

    fn queue_dir(&self) -> PathBuf {
        self.dir.join("queue")
    }

    fn entry_path(&self, id: &str) -> PathBuf {
        self.queue_dir().join(format!("{id}.json"))
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Writes a new or changed waiting task.
    pub fn put(&self, e: &Entry) -> std::io::Result<()> {
        let _g = self.guard();
        std::fs::create_dir_all(self.queue_dir())?;
        write_json(&self.entry_path(&e.id), e)
    }

    pub fn get(&self, id: &str) -> Option<Entry> {
        read_json(&self.entry_path(id))
    }

    pub fn remove(&self, id: &str) -> std::io::Result<()> {
        let _g = self.guard();
        match std::fs::remove_file(self.entry_path(id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Every waiting task, oldest first.
    pub fn queue(&self) -> Vec<Entry> {
        let Ok(dir) = std::fs::read_dir(self.queue_dir()) else { return Vec::new() };
        let mut out: Vec<Entry> = dir
            .filter_map(Result::ok)
            .map(|d| d.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| read_json(&p))
            .collect();
        out.sort_by(|a, b| a.said.at_ms.cmp(&b.said.at_ms).then(a.id.cmp(&b.id)));
        out
    }

    /// Saved tasks, newest first.
    pub fn recent(&self) -> Vec<Saved> {
        read_json(&self.dir.join("recent.json")).unwrap_or_default()
    }

    fn write_recent(&self, list: &[Saved]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        write_json(&self.dir.join("recent.json"), &list)
    }

    /// Adds or replaces a saved task, newest first.
    pub fn put_recent(&self, s: &Saved) -> std::io::Result<()> {
        let _g = self.guard();
        let mut list = self.recent();
        match list.iter_mut().find(|r| r.id == s.id) {
            Some(r) => *r = s.clone(),
            None => list.insert(0, s.clone()),
        }
        list.truncate(RECENT);
        self.write_recent(&list)
    }

    pub fn remove_recent(&self, id: &str) -> std::io::Result<()> {
        let _g = self.guard();
        let mut list = self.recent();
        list.retain(|r| r.id != id);
        self.write_recent(&list)
    }

    /// The lists last read from Google, and when (ms).
    pub fn lists(&self) -> Option<(i64, Vec<List>)> {
        read_json::<ListCache>(&self.dir.join("lists.json")).map(|c| (c.fetched_ms, c.lists))
    }

    pub fn put_lists(&self, fetched_ms: i64, lists: &[List]) -> std::io::Result<()> {
        let _g = self.guard();
        std::fs::create_dir_all(&self.dir)?;
        write_json(&self.dir.join("lists.json"), &ListCache { fetched_ms, lists: lists.to_vec() })
    }

    /// Forgets the saved tasks and the lists (on sign-out). Waiting tasks
    /// stay; they're saved after the next sign-in.
    pub fn forget_account(&self) {
        let _g = self.guard();
        let _ = std::fs::remove_file(self.dir.join("recent.json"));
        let _ = std::fs::remove_file(self.dir.join("lists.json"));
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let s = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&s) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!("can't read {}: {e}", path.display());
            None
        }
    }
}

fn write_json<T: Serialize + ?Sized>(path: &Path, v: &T) -> std::io::Result<()> {
    let s = serde_json::to_string_pretty(v).map_err(std::io::Error::other)?;
    write_private(path, &s)
}

/// Random bytes from the system.
pub fn random<const N: usize>() -> [u8; N] {
    use ring::rand::SecureRandom;
    let mut b = [0u8; N];
    // The system's random source doesn't fail on any platform Mirza runs on.
    ring::rand::SystemRandom::new().fill(&mut b).expect("system random");
    b
}

/// A new id for something said at `at_ms`: sortable by time, unique per
/// device, and only `0-9a-v-` so it can also become a Calendar event id.
pub fn new_id(at_ms: i64) -> String {
    let r = random::<3>();
    format!("{}-{:02x}{:02x}{:02x}", radix32(at_ms.max(0) as u64), r[0], r[1], r[2])
}

fn radix32(mut n: u64) -> String {
    const DIGITS: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 32) as usize]);
        n /= 32;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The Calendar event id for a task: letters a-v and digits only. A new
/// `generation` gives a new id, since Calendar keeps deleted events' ids.
pub fn event_id(entry_id: &str, generation: u32) -> String {
    let base: String = entry_id.chars().filter(|c| matches!(c, '0'..='9' | 'a'..='v')).collect();
    format!("mi{base}g{generation}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!("mirza-tasks-test-{}", new_id(0)));
        (Store::new(&dir), dir)
    }

    fn entry(id: &str, at_ms: i64) -> Entry {
        Entry {
            id: id.into(),
            text: "call Sara".into(),
            said: Said { at_ms, tz: "Europe/Berlin".into() },
            held: false,
            attempts: 0,
            next_try_ms: 0,
            last_error: String::new(),
            draft: None,
            progress: Progress::default(),
        }
    }

    #[test]
    fn queue_survives_and_keeps_order() {
        let (s, dir) = store();
        assert!(s.queue().is_empty());
        s.put(&entry("b", 2)).unwrap();
        s.put(&entry("a", 1)).unwrap();
        let mut e = entry("c", 3);
        e.progress.task = Some(TaskRef { list: "@default".into(), id: "t1".into() });
        s.put(&e).unwrap();
        let again = Store::new(&dir);
        assert_eq!(again.queue().iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(again.get("c").unwrap(), e);
        again.remove("a").unwrap();
        again.remove("a").unwrap();
        assert_eq!(again.queue().len(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("queue/b.json")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recent_is_capped_and_newest_first() {
        let (s, dir) = store();
        let draft = super::super::parse::raw("x", "", "test".into());
        for i in 0..25 {
            let saved = Saved {
                id: format!("s{i}"),
                text: "x".into(),
                said: Said { at_ms: i, tz: String::new() },
                draft: draft.clone(),
                task: TaskRef { list: "l".into(), id: format!("t{i}") },
                event: None,
                event_gen: 0,
                saved_ms: i,
            };
            s.put_recent(&saved).unwrap();
        }
        let r = s.recent();
        assert_eq!(r.len(), RECENT);
        assert_eq!(r[0].id, "s24");
        s.remove_recent("s24").unwrap();
        assert_eq!(s.recent()[0].id, "s23");
        s.put_lists(5, &[List { id: "l".into(), title: "Personal".into() }]).unwrap();
        assert_eq!(s.lists().unwrap().0, 5);
        s.forget_account();
        assert!(s.recent().is_empty() && s.lists().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ids_fit_calendar() {
        let id = new_id(1_794_000_000_000);
        assert!(id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='v' | '-')), "{id}");
        assert_ne!(id, new_id(1_794_000_000_000));
        let ev = event_id(&id, 2);
        assert!(ev.len() >= 5 && ev.chars().all(|c| matches!(c, '0'..='9' | 'a'..='v')), "{ev}");
        assert_ne!(event_id(&id, 0), event_id(&id, 1));
    }
}
