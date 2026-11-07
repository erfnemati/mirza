//! The phone app's settings and API keys, kept in the app's private files.
//!
//! Settings are TOML, like the desktop's, but only with what a phone needs; the parts both share
//! (speech providers, AI revision, prepaid credit) use the desktop's types. Kotlin reads them all as JSON
//! and changes one value at a time by its dotted path. Keys live in a separate file and never go
//! back to Kotlin; it only asks whether one is set.

use std::collections::BTreeMap;
use std::path::PathBuf;

use mirza_core::ai::{AiProviders, Kind, Revision};
use mirza_core::config::{Credit, PROVIDERS, Providers, provider_name, set_json, write_private};
use serde::{Deserialize, Serialize};

use crate::MobileError;

/// Unknown keys are an error, so a typo or a newer app's key is caught instead of dropped.
/// Android never installs an older version over a newer one, so a file is never read by an
/// app older than the one that wrote it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MobileConfig {
    /// Which provider dictation uses: "soniox", "elevenlabs" or "openai".
    pub active_provider: String,
    /// Stop after this long without recognized speech (0: never).
    pub silence_stop_sec: u64,
    /// Stop a dictation after this long no matter what (0: never).
    pub max_session_sec: u64,
    /// "" or "none" for a direct connection, or an http:// or socks5:// URL.
    pub proxy: String,
    /// A short sound when dictation starts and stops.
    pub sounds: bool,
    /// 1 to 100.
    pub sound_volume: u8,
    /// Once a day, ask GitHub whether a newer version of Mirza is out.
    pub check_updates: bool,
    /// How many recent texts the app keeps (0: none).
    pub history_size: usize,
    pub typing: Typing,
    pub bubble: Bubble,
    pub gestures: Gestures,
    pub providers: Providers,
    /// AI revision: which provider, how much to change, the rules.
    pub revision: Revision,
    /// AI providers: model, address and thinking effort for each.
    pub ai: AiProviders,
    /// Prepaid credit per provider, so the app can show what is left.
    pub credit: BTreeMap<String, Credit>,
    /// Voice tasks: what you say becomes a Google Tasks entry.
    pub tasks: Tasks,
}

impl Default for MobileConfig {
    fn default() -> Self {
        Self {
            active_provider: "soniox".into(),
            silence_stop_sec: 60,
            max_session_sec: 900,
            proxy: String::new(),
            sounds: true,
            sound_volume: 50,
            check_updates: true,
            history_size: 20,
            typing: Typing::default(),
            bubble: Bubble::default(),
            gestures: Gestures::default(),
            providers: Providers::default(),
            revision: Revision::default(),
            ai: AiProviders::default(),
            credit: BTreeMap::new(),
            tasks: Tasks::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Typing {
    /// Arabic ي and ك become Persian ی and ک.
    pub persian_letters: bool,
    /// Also put each dictation in the clipboard.
    pub keep_on_clipboard: bool,
}

impl Default for Typing {
    fn default() -> Self {
        Self { persian_letters: true, keep_on_clipboard: false }
    }
}

/// The floating button.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bubble {
    /// Show the floating button while typing is on.
    pub show: bool,
    /// Diameter in dp, 40 to 88.
    pub size: u32,
    /// How opaque it is while idle, 30 to 100 percent; it is always solid while recording.
    pub opacity: u8,
    /// Mirza's logo; false for a plain round button.
    pub logo: bool,
    /// Also on the lock screen (where it never types).
    pub lock_screen: bool,
}

impl Default for Bubble {
    fn default() -> Self {
        Self { show: true, size: 56, opacity: 100, logo: true, lock_screen: true }
    }
}

/// The phone's `[tasks]`: the desktop's task settings without the Google client (the phone signs
/// in through Google Play services), plus the lock-screen queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tasks {
    /// "" uses AI revision's provider; otherwise "claude", "gemini", "openai", "openrouter" or
    /// "local". With neither, tasks are saved as said.
    pub provider: String,
    /// How long to wait for the AI before saving the task as said.
    pub timeout_sec: u64,
    /// Google Tasks list id for tasks that fit no list; "" is the account's default list.
    pub default_list: String,
    /// Event length when none was said.
    pub event_minutes: u32,
    /// A task with a clock time also gets that day as its due date, besides the Calendar event.
    pub timed_due_date: bool,
    /// Other names for a list, by Google Tasks list id, e.g. "office, Acme".
    pub list_hints: BTreeMap<String, String>,
    /// Your own rules for the AI that reads tasks, e.g. "Write the title in English."
    pub instructions: String,
    /// How many tasks said on the lock screen may wait for the phone to be unlocked, 1 to 10.
    pub held_limit: u32,
}

impl Default for Tasks {
    fn default() -> Self {
        let t = mirza_core::tasks::TaskSettings::default();
        Self {
            provider: String::new(),
            timeout_sec: t.timeout_sec,
            default_list: String::new(),
            event_minutes: t.event_minutes,
            timed_due_date: t.timed_due_date,
            list_hints: BTreeMap::new(),
            instructions: String::new(),
            held_limit: mirza_core::tasks::HELD_MAX as u32,
        }
    }
}

/// What a gesture on the bubble does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gesture {
    /// Start or stop dictation.
    Dictate,
    /// Dictation revised by AI before it's typed.
    Revise,
    /// Say a task for Google Tasks.
    Task,
    /// Open Mirza's settings.
    OpenApp,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gestures {
    pub tap: Gesture,
    pub double_tap: Gesture,
    pub long_press: Gesture,
    /// Android's accessibility button (in the navigation bar, or floating). It shows up only when
    /// the user turns on the "Mirza button" service, so it dictates by default.
    pub button: Gesture,
    /// Holding volume down while the screen is on. Off by default, like the next one.
    pub volume_long_press: Gesture,
    /// Pressing volume down twice quickly while the screen is on. A single press then changes the
    /// volume a moment later, once it is clear no second press follows.
    pub volume_double_press: Gesture,
}

impl Default for Gestures {
    fn default() -> Self {
        Self {
            tap: Gesture::Dictate,
            double_tap: Gesture::Revise,
            long_press: Gesture::Task,
            button: Gesture::Dictate,
            volume_long_press: Gesture::None,
            volume_double_press: Gesture::None,
        }
    }
}

impl MobileConfig {
    pub fn parse(s: &str) -> Result<Self, String> {
        let cfg: Self = toml::from_str(s).map_err(|e| e.message().to_owned())?;
        cfg.check()?;
        Ok(cfg)
    }

    /// What the file format can't say by itself.
    fn check(&self) -> Result<(), String> {
        if !PROVIDERS.contains(&self.active_provider.as_str()) {
            return Err(format!("unknown speech service {:?}", self.active_provider));
        }
        mirza_core::net::resolve_proxy(&self.proxy).map_err(|e| e.to_string())?;
        let b = &self.bubble;
        if !(40..=88).contains(&b.size) {
            return Err(format!("bubble.size must be 40 to 88, not {}", b.size));
        }
        if !(30..=100).contains(&b.opacity) {
            return Err(format!("bubble.opacity must be 30 to 100, not {}", b.opacity));
        }
        if !(1..=100).contains(&self.sound_volume) {
            return Err(format!("sound_volume must be 1 to 100, not {}", self.sound_volume));
        }
        if self.history_size > 200 {
            return Err("history_size can be at most 200".into());
        }
        if self.gestures.tap == Gesture::None {
            return Err("a tap on the bubble has to do something".into());
        }
        let p = self.revision.provider.trim();
        if !p.is_empty() && Kind::parse(p).is_none() {
            return Err(format!("unknown AI provider {p:?}"));
        }
        if !(1..=120).contains(&self.revision.timeout_sec) {
            return Err("revision.timeout_sec must be 1 to 120".into());
        }
        let t = &self.tasks;
        let p = t.provider.trim();
        if !p.is_empty() && Kind::parse(p).is_none() {
            return Err(format!("unknown AI provider {p:?}"));
        }
        if !(5..=120).contains(&t.timeout_sec) {
            return Err("tasks.timeout_sec must be 5 to 120".into());
        }
        if !(5..=1440).contains(&t.event_minutes) {
            return Err("tasks.event_minutes must be 5 to 1440".into());
        }
        if !(1..=mirza_core::tasks::HELD_MAX as u32).contains(&t.held_limit) {
            return Err(format!("tasks.held_limit must be 1 to {}", mirza_core::tasks::HELD_MAX));
        }
        Ok(())
    }
}

pub(crate) struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn config_path(&self) -> PathBuf {
        self.dir.join("config.toml")
    }

    fn keys_path(&self) -> PathBuf {
        self.dir.join("keys.toml")
    }

    /// The settings, and a note when the file couldn't be read. A broken file is kept next to the
    /// new one as config.toml.broken, so nothing is lost, and the defaults are used.
    pub fn load(&self) -> (MobileConfig, Option<String>) {
        let path = self.config_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (MobileConfig::default(), None),
            Err(e) => {
                return (
                    MobileConfig::default(),
                    Some(format!("Couldn't read the settings ({e}); using the defaults.")),
                );
            }
        };
        match MobileConfig::parse(&text) {
            Ok(cfg) => (cfg, None),
            Err(e) => {
                let _ = std::fs::rename(&path, path.with_extension("toml.broken"));
                (
                    MobileConfig::default(),
                    Some(format!("The settings file was broken ({e}); Mirza started over with the defaults.")),
                )
            }
        }
    }

    /// Sets one value by its dotted path (JSON null: back to the default) and returns the new
    /// settings. Nothing is saved if the result isn't valid.
    pub fn set(&self, path: &str, value: &serde_json::Value) -> Result<MobileConfig, MobileError> {
        let file = self.config_path();
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| invalid(path, e))?;
        set_json(&mut doc, path, value).map_err(|e| invalid(path, e))?;
        let out = doc.to_string();
        let cfg = MobileConfig::parse(&out).map_err(|e| invalid(path, e))?;
        std::fs::create_dir_all(&self.dir).and_then(|()| write_private(&file, &out)).map_err(save_failed)?;
        Ok(cfg)
    }

    fn read_keys(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(self.keys_path()).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn key(&self, service: &str) -> Option<String> {
        self.read_keys().remove(service).map(|k| k.trim().to_owned()).filter(|k| !k.is_empty())
    }

    /// Saves a key; an empty one removes it.
    pub fn set_key(&self, service: &str, key: &str) -> Result<(), MobileError> {
        if !mirza_core::secrets::services().contains(&service) {
            return Err(MobileError::Settings(format!("unknown service {service:?}")));
        }
        let mut keys = self.read_keys();
        match key.trim() {
            "" => keys.remove(service),
            k => keys.insert(service.to_owned(), k.to_owned()),
        };
        let body = toml::to_string(&keys).map_err(|e| save_failed(std::io::Error::other(e)))?;
        std::fs::create_dir_all(&self.dir)
            .and_then(|()| write_private(&self.keys_path(), &format!("# API keys, one per service.\n{body}")))
            .map_err(save_failed)
    }
}

fn invalid(path: &str, e: impl std::fmt::Display) -> MobileError {
    MobileError::Settings(format!("{path}: {e}"))
}

fn save_failed(e: std::io::Error) -> MobileError {
    MobileError::Settings(format!("Couldn't save the settings: {e}"))
}

/// "Enter your Soniox API key first." and friends.
pub(crate) fn missing_key(service: &str) -> MobileError {
    MobileError::Settings(format!("Enter your {} API key first (Speech settings).", provider_name(service)))
}

/// The same for an AI provider.
pub(crate) fn missing_ai_key(kind: Kind) -> String {
    format!("Enter your {} API key first (AI revision settings).", kind.name())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn temp_store() -> (Store, PathBuf) {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mirza-mobile-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (Store::new(&dir), dir)
    }

    #[test]
    fn defaults_without_a_file() {
        let (store, dir) = temp_store();
        let (cfg, note) = store.load();
        assert_eq!(cfg, MobileConfig::default());
        assert!(note.is_none());
        assert_eq!(cfg.providers.soniox.model, "stt-rt-v6");
        assert_eq!(cfg.providers.soniox.languages, ["fa", "en"]);
        assert!(!dir.exists(), "reading creates nothing");
    }

    #[test]
    fn values_are_set_checked_and_reset() {
        let (store, dir) = temp_store();
        let cfg = store.set("providers.soniox.languages", &json!(["fa"])).unwrap();
        assert_eq!(cfg.providers.soniox.languages, ["fa"]);
        assert_eq!(cfg.providers.soniox.model, "stt-rt-v6", "the rest keeps its defaults");
        store.set("bubble.show", &json!(false)).unwrap();
        store.set("proxy", &json!("socks5://127.0.0.1:10808")).unwrap();

        let (cfg, _) = store.load();
        assert_eq!(cfg.providers.soniox.languages, ["fa"]);
        assert!(!cfg.bubble.show);
        assert_eq!(cfg.proxy, "socks5://127.0.0.1:10808");

        let err = store.set("proxy", &json!("ftp://example.com")).unwrap_err().to_string();
        assert!(err.contains("proxy"), "{err}");
        assert!(store.set("active_provider", &json!("whisper")).is_err());
        assert!(store.set("sonds", &json!(true)).is_err(), "typos are refused");
        assert!(store.set("sounds", &json!("yes")).is_err(), "wrong types are refused");
        assert!(store.set("bubble.size", &json!(20)).is_err());
        assert!(store.set("bubble.opacity", &json!(10)).is_err());
        assert!(store.set("gestures.tap", &json!("none")).is_err());
        assert!(store.set("gestures.tap", &json!("fly")).is_err());
        assert!(store.set("revision.provider", &json!("bard")).is_err());
        assert_eq!(MobileConfig::default().gestures.long_press, Gesture::Task);
        let cfg = store.set("gestures.long_press", &json!("revise")).unwrap();
        assert_eq!(cfg.gestures.long_press, Gesture::Revise);
        let g = MobileConfig::default().gestures;
        assert_eq!([g.volume_long_press, g.volume_double_press], [Gesture::None; 2], "off until chosen");
        assert_eq!(g.button, Gesture::Dictate, "the button exists only once turned on in Android");
        let cfg = store.set("gestures.volume_double_press", &json!("task")).unwrap();
        assert_eq!(cfg.gestures.volume_double_press, Gesture::Task);
        assert!(store.set("tasks.held_limit", &json!(11)).is_err());
        assert!(store.set("tasks.held_limit", &json!(0)).is_err());
        assert!(store.set("tasks.provider", &json!("bard")).is_err());
        store.set("tasks.list_hints.abc", &json!("office, Acme")).unwrap();
        let cfg = store.set("tasks.held_limit", &json!(3)).unwrap();
        assert_eq!((cfg.tasks.held_limit, cfg.tasks.list_hints["abc"].as_str()), (3, "office, Acme"));
        let cfg = store.set("revision.provider", &json!("claude")).unwrap();
        assert_eq!(cfg.revision.kind(), Some(Kind::Claude));
        assert_eq!(cfg.ai.claude.model, "claude-haiku-5-5", "the desktop's defaults");
        store.set("credit.soniox.amount", &json!(20.0)).unwrap();
        let cfg = store.set("credit.soniox.since", &json!("2026-11-01")).unwrap();
        assert_eq!((cfg.credit["soniox"].amount, cfg.credit["soniox"].since.as_str()), (20.0, "2026-11-01"));
        assert_eq!(store.load().0.proxy, "socks5://127.0.0.1:10808", "refused changes aren't saved");

        let cfg = store.set("providers.soniox.languages", &serde_json::Value::Null).unwrap();
        assert_eq!(cfg.providers.soniox.languages, ["fa", "en"]);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("config.toml")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_broken_file_is_kept_aside() {
        let (store, dir) = temp_store();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), "sounds = maybe").unwrap();
        let (cfg, note) = store.load();
        assert_eq!(cfg, MobileConfig::default());
        assert!(note.unwrap().contains("broken"));
        assert!(dir.join("config.toml.broken").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keys_are_saved_privately_and_removed() {
        let (store, dir) = temp_store();
        assert_eq!(store.key("soniox"), None);
        store.set_key("soniox", "  abc  ").unwrap();
        store.set_key("claude", "xyz").unwrap();
        assert_eq!(store.key("soniox").as_deref(), Some("abc"));
        assert!(store.set_key("whisper", "k").is_err());
        store.set_key("soniox", "").unwrap();
        assert_eq!(store.key("soniox"), None);
        assert_eq!(store.key("claude").as_deref(), Some("xyz"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("keys.toml")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
