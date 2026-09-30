//! Typing recognized text into whichever window has focus.
//!
//! A [`TypingSession`] types text as it arrives, on a thread of its own. While
//! a window other than the one dictation started in is active, typing pauses,
//! and it resumes when that window is back. Text no keyboard layout can type is
//! pasted. At the end the untyped rest, if any, is left in the clipboard.

pub mod clipboard;
pub mod keymap;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

#[cfg(target_os = "linux")]
use linux::Backend;
#[cfg(target_os = "macos")]
use macos::Backend;
#[cfg(windows)]
use windows::Backend;

/// The active window's ID as reported by the desktop ("" when none is
/// active). `None` until the first report.
pub type SharedActive = Arc<Mutex<Option<String>>>;

#[derive(Clone, Debug)]
pub struct Options {
    /// Pause after each typed character.
    pub key_delay: Duration,
    /// Key combination that pastes, e.g. "shift+insert" or "ctrl+v".
    pub paste_key: String,
    /// Paste instead of typing when a piece of text is longer than this
    /// (0 means always type).
    pub paste_over_chars: usize,
    /// Leave the whole transcript in the clipboard at the end, even when all of
    /// it was typed.
    pub keep_on_clipboard: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            key_delay: Duration::from_millis(2),
            paste_key: "shift+insert".into(),
            paste_over_chars: 0,
            keep_on_clipboard: false,
        }
    }
}

pub enum Typed {
    Chars(usize),
    Unmappable,
}

/// What a finished session did with the transcript.
#[derive(Debug, Default, Clone)]
pub struct Outcome {
    pub text: String,
    /// Part of the text wasn't typed (another window was active); it is in the
    /// clipboard.
    pub held: bool,
    /// The whole transcript was put in the clipboard.
    pub copied: bool,
}

/// Something the user may want to hear about while a session runs.
#[derive(Debug, Clone)]
pub enum Event {
    Paused,
    Error(String),
}

/// Which window dictation started in, compared with the active one.
#[derive(Clone)]
pub struct Focus {
    active: Option<Arc<Mutex<Option<String>>>>, // None: the active window can't be watched
    origin: Option<String>,
}

impl Focus {
    pub fn unknown() -> Self {
        Self { active: None, origin: None }
    }

    /// Follows a window ID that something else keeps up to date (a KWin
    /// script on Linux, a foreground-window poll on Windows).
    pub fn watched(active: &SharedActive) -> Self {
        let origin = active.lock().unwrap().clone();
        Self { active: Some(active.clone()), origin }
    }

    #[cfg(test)]
    fn set_current(&self, id: Option<String>) {
        if let Some(a) = &self.active {
            *a.lock().unwrap() = id;
        }
    }

    /// Whether typing may go on: the original window is active, or the active
    /// window isn't known.
    pub fn ok(&self) -> bool {
        let Some(active) = &self.active else { return true };
        match &self.origin {
            None => true, // no report had arrived when dictation started
            Some(origin) => !origin.is_empty() && active.lock().unwrap().as_deref() == Some(origin.as_str()),
        }
    }
}

enum Msg {
    Text(String),
    FocusChanged,
    Hold(bool),
}

/// Keeps the platform backend alive between sessions.
pub struct Injector {
    backend: Arc<Mutex<Backend>>,
}

impl Injector {
    #[cfg(target_os = "linux")]
    pub fn new(rt: tokio::runtime::Handle, conn: zbus::Connection) -> io::Result<Self> {
        Ok(Self { backend: Arc::new(Mutex::new(Backend::new(rt, conn)?)) })
    }

    #[cfg(not(target_os = "linux"))]
    pub fn new() -> io::Result<Self> {
        Ok(Self { backend: Arc::new(Mutex::new(Backend::new()?)) })
    }

    /// Starts typing into the window that has focus now.
    pub fn begin(&self, opts: Options, focus: Focus, on_event: impl Fn(Event) + Send + 'static) -> TypingSession {
        let (tx, rx) = mpsc::channel();
        let backend = self.backend.clone();
        let handle = std::thread::Builder::new()
            .name("mirza-typer".into())
            .spawn(move || run(backend, opts, focus, rx, on_event))
            .expect("spawning the typing thread");
        TypingSession { tx, handle }
    }

    /// Puts text in the clipboard.
    pub fn copy(&self, text: &str) -> io::Result<()> {
        self.backend.lock().unwrap().copy(text)
    }

    /// Direct access to the virtual keyboard (Linux), e.g. for self-tests.
    #[cfg(target_os = "linux")]
    pub fn with_keyboard<R>(&self, f: impl FnOnce(&mut linux::VirtualKeyboard) -> R) -> R {
        f(self.backend.lock().unwrap().raw_keyboard())
    }
}

pub struct TypingSession {
    tx: Sender<Msg>,
    handle: JoinHandle<Outcome>,
}

impl TypingSession {
    pub fn push(&self, text: impl Into<String>) {
        let _ = self.tx.send(Msg::Text(text.into()));
    }

    /// Holds typing back (e.g. while the user holds a shortcut, whose
    /// modifiers would turn typed characters into shortcuts) or lets it go on.
    pub fn hold(&self, on: bool) {
        let _ = self.tx.send(Msg::Hold(on));
    }

    /// Tells the session the active window changed.
    pub fn focus_changed(&self) {
        let _ = self.tx.send(Msg::FocusChanged);
    }

    /// A handle that can report focus changes from another thread.
    pub fn focus_notifier(&self) -> impl Fn() + Send + Sync + 'static {
        let tx = Mutex::new(self.tx.clone());
        move || {
            let _ = tx.lock().unwrap().send(Msg::FocusChanged);
        }
    }

    /// Waits for pending typing, then leaves the untyped rest, or the whole
    /// transcript if asked, in the clipboard. Blocks.
    pub fn finish(self) -> Outcome {
        drop(self.tx);
        self.handle.join().unwrap_or_default()
    }
}

fn run(
    backend: Arc<Mutex<Backend>>,
    opts: Options,
    focus: Focus,
    rx: Receiver<Msg>,
    on_event: impl Fn(Event),
) -> Outcome {
    let mut b = backend.lock().unwrap();
    b.begin();
    let mut text = String::new();
    let mut pending = String::new();
    let mut paused = false;
    let mut held = false;
    let mut report = |e: io::Error| on_event(Event::Error(e.to_string()));

    let flush = |b: &mut Backend, pending: &mut String, held: bool, report: &mut dyn FnMut(io::Error)| {
        if pending.is_empty() || held || !focus.ok() {
            return;
        }
        let paste = opts.paste_over_chars > 0 && pending.chars().count() > opts.paste_over_chars;
        let typed = if paste { Ok(Typed::Unmappable) } else { b.type_text(pending, &opts, &|| focus.ok()) };
        match typed {
            Ok(Typed::Chars(n)) => {
                let cut = pending.char_indices().nth(n).map_or(pending.len(), |(i, _)| i);
                pending.drain(..cut);
            }
            Ok(Typed::Unmappable) => match b.paste(pending, &opts) {
                Ok(()) => pending.clear(),
                Err(e) => report(e),
            },
            Err(e) => report(e),
        }
    };

    for msg in rx.iter() {
        match msg {
            Msg::Text(mut s) => {
                if text.is_empty() {
                    s = s.trim_start_matches(' ').to_owned();
                    if s.is_empty() {
                        continue;
                    }
                }
                text.push_str(&s);
                pending.push_str(&s);
            }
            Msg::FocusChanged => {
                if !focus.ok() {
                    b.restore(); // the other window gets the user's own layout back
                }
            }
            Msg::Hold(on) => held = on,
        }
        flush(&mut b, &mut pending, held, &mut report);
        let left = !pending.is_empty() && !focus.ok();
        if left && !paused {
            on_event(Event::Paused);
        }
        paused = left;
    }

    flush(&mut b, &mut pending, false, &mut report); // the original window may be active again
    b.end();
    let held = !pending.is_empty();
    let clip = if held {
        Some(&pending)
    } else if opts.keep_on_clipboard && !text.is_empty() {
        Some(&text)
    } else {
        None
    };
    let mut copied = false;
    if let Some(c) = clip {
        match b.copy(c) {
            Ok(()) => copied = !held,
            Err(e) => report(e),
        }
    }
    Outcome { text, held, copied }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watched(origin: Option<&str>) -> Focus {
        let origin = origin.map(str::to_owned);
        Focus { active: Some(Arc::new(Mutex::new(origin.clone()))), origin }
    }

    #[test]
    fn focus_follows_the_original_window() {
        assert!(Focus::unknown().ok(), "typing goes on while the active window is unknown");
        assert!(watched(None).ok(), "no report yet when dictation started");
        let f = watched(Some("A"));
        for (active, ok) in [("A", true), ("B", false), ("A", true)] {
            f.set_current(Some(active.into()));
            assert_eq!(f.ok(), ok, "active {active:?}");
        }
        let g = watched(Some(""));
        g.set_current(Some("C".into()));
        assert!(!g.ok(), "with no window at the start, text must only go to the clipboard");
    }
}
