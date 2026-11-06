//! The phone app's settings and API keys, kept in the app's private files.
//!
//! Settings are TOML, like the desktop's, but only with what a phone needs; the parts both share
//! (speech providers, later AI revision) use the desktop's types. Kotlin reads them all as JSON
//! and changes one value at a time by its dotted path. Keys live in a separate file and never go
//! back to Kotlin; it only asks whether one is set.

use std::collections::BTreeMap;
use std::path::PathBuf;

use mirza_core::config::{PROVIDERS, Providers, provider_name, set_json, write_private};
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
    /// Once a day, ask GitHub whether a newer version of Mirza is out.
    pub check_updates: bool,
    pub bubble: Bubble,
    pub providers: Providers,
}

impl Default for MobileConfig {
    fn default() -> Self {
        Self {
            active_provider: "soniox".into(),
            silence_stop_sec: 60,
            max_session_sec: 900,
            proxy: String::new(),
            sounds: true,
            check_updates: true,
            bubble: Bubble::default(),
            providers: Providers::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bubble {
    /// Show the floating button while typing is on.
    pub show: bool,
}

impl Default for Bubble {
    fn default() -> Self {
        Self { show: true }
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
