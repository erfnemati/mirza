//! Settings, stored as TOML in the OS config directory. Every value has a
//! default, so a missing file or key is fine; unknown keys are an error so
//! typos don't go unnoticed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const APP: &str = "mirza";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Which provider dictation uses: "soniox", "elevenlabs" or "openai".
    pub active_provider: String,
    /// Global shortcuts. Empty until the user picks one.
    pub shortcuts: Vec<Shortcut>,
    /// A hold-to-talk press shorter than this is ignored.
    pub min_hold_ms: u64,
    /// Stop after this long without recognized speech (0: never).
    pub silence_stop_sec: u64,
    /// Stop a session after this long no matter what (0: never).
    pub max_session_sec: u64,
    /// Microphone to record from; empty for the system default.
    pub mic_device: String,
    /// Linux: a custom command that writes raw s16le mono audio to stdout at
    /// the rate given by {rate}, e.g. "pw-record --rate {rate} --channels 1 --format s16 -".
    pub record_cmd: String,
    /// "" uses HTTPS_PROXY/ALL_PROXY from the environment if set, "none" connects
    /// directly, anything else is a proxy URL (http:// or socks5://).
    pub proxy: String,
    pub notifications: bool,
    pub sounds: bool,
    /// Start Mirza when you log in.
    pub start_on_login: bool,
    /// How many recent transcripts to keep for recovery (0: none).
    pub history_size: usize,
    pub typing: Typing,
    pub providers: Providers,
    /// Prepaid credit per provider, so the app can show what is left: amount
    /// minus spend since the date.
    pub credit: BTreeMap<String, Credit>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            active_provider: "soniox".into(),
            shortcuts: Vec::new(),
            min_hold_ms: 150,
            silence_stop_sec: 60,
            max_session_sec: 900,
            mic_device: String::new(),
            record_cmd: String::new(),
            proxy: String::new(),
            notifications: true,
            sounds: false,
            start_on_login: true,
            history_size: 20,
            typing: Typing::default(),
            providers: Providers::default(),
            credit: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Dictate,
    Cancel,
    Panel,
    NextProvider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Press once to start, again to stop.
    #[default]
    Toggle,
    /// Talk while the key is held.
    Hold,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shortcut {
    pub action: Action,
    /// e.g. "Meta+H", "Ctrl+Alt+Space", "RightCtrl".
    pub keys: String,
    #[serde(default)]
    pub mode: Mode,
}

impl Shortcut {
    /// A stable ID for the shortcut, used where the desktop remembers bindings
    /// by name (the Linux shortcuts portal).
    pub fn id(&self) -> String {
        match self.action {
            Action::Dictate => format!("dictate-{}", if self.mode == Mode::Hold { "hold" } else { "toggle" }),
            Action::Cancel => "cancel".into(),
            Action::Panel => "panel".into(),
            Action::NextProvider => "next-provider".into(),
        }
    }

    pub fn description(&self) -> &'static str {
        match (self.action, self.mode) {
            (Action::Dictate, Mode::Toggle) => "Start or stop dictation",
            (Action::Dictate, Mode::Hold) => "Dictate while held",
            (Action::Cancel, _) => "Cancel dictation",
            (Action::Panel, _) => "Open Mirza settings",
            (Action::NextProvider, _) => "Switch to the next speech-to-text provider",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypingMode {
    /// Type text once the provider has settled on it.
    #[default]
    Final,
    /// Type the provider's guesses as they come and correct them.
    Live,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Typing {
    pub mode: TypingMode,
    pub key_delay_ms: u64,
    /// Key combination that pastes in the focused app.
    pub paste_key: String,
    /// Paste instead of typing text longer than this (0: always type).
    pub paste_over_chars: usize,
    /// Also leave the whole transcript in the clipboard at the end.
    pub keep_on_clipboard: bool,
    /// Write Arabic ي and ك as Persian ی and ک.
    pub persian_letters: bool,
    /// Pause typing while another window than the one dictation started in is
    /// active (KDE Plasma).
    pub follow_focus: bool,
}

impl Default for Typing {
    fn default() -> Self {
        Self {
            mode: TypingMode::Final,
            key_delay_ms: 2,
            paste_key: if cfg!(target_os = "macos") {
                "cmd+v"
            } else if cfg!(target_os = "linux") {
                "shift+insert"
            } else {
                "ctrl+v"
            }
            .into(),
            paste_over_chars: 0,
            keep_on_clipboard: false,
            persian_letters: true,
            follow_focus: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Provider {
    pub model: String,
    /// Language hints such as ["fa", "en"].
    pub languages: Vec<String>,
    /// Only recognize the hinted languages (where the provider supports it).
    pub strict_languages: bool,
    /// Words and names the provider should expect.
    pub terms: Vec<String>,
    /// Price per minute of audio in USD, for the local usage estimate.
    pub price_per_min: f64,
    /// Let the provider mark pauses between utterances (Soniox).
    pub endpoint_detection: bool,
}

impl Provider {
    fn new(model: &str, price_per_min: f64) -> Self {
        Self {
            model: model.into(),
            languages: vec!["fa".into(), "en".into()],
            strict_languages: false,
            terms: Vec::new(),
            price_per_min,
            endpoint_detection: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "ProvidersFile")]
pub struct Providers {
    pub soniox: Provider,
    pub elevenlabs: Provider,
    pub openai: Provider,
}

impl Default for Providers {
    fn default() -> Self {
        Self {
            soniox: Provider::new("stt-rt-v5", 0.002),
            elevenlabs: Provider::new("scribe_v2_realtime", 0.0065),
            openai: Provider::new("gpt-live-transcribe", 0.006),
        }
    }
}

/// What the file says about each provider; anything missing keeps that
/// provider's own default (each has a different model and price).
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ProvidersFile {
    soniox: ProviderPatch,
    elevenlabs: ProviderPatch,
    openai: ProviderPatch,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ProviderPatch {
    model: Option<String>,
    languages: Option<Vec<String>>,
    strict_languages: Option<bool>,
    terms: Option<Vec<String>>,
    price_per_min: Option<f64>,
    endpoint_detection: Option<bool>,
}

impl ProviderPatch {
    fn apply(self, mut p: Provider) -> Provider {
        if let Some(v) = self.model.filter(|m| !m.trim().is_empty()) {
            p.model = v;
        }
        p.languages = self.languages.unwrap_or(p.languages);
        p.strict_languages = self.strict_languages.unwrap_or(p.strict_languages);
        p.terms = self.terms.unwrap_or(p.terms);
        p.price_per_min = self.price_per_min.unwrap_or(p.price_per_min);
        p.endpoint_detection = self.endpoint_detection.unwrap_or(p.endpoint_detection);
        p
    }
}

impl From<ProvidersFile> for Providers {
    fn from(f: ProvidersFile) -> Self {
        let d = Providers::default();
        Self {
            soniox: f.soniox.apply(d.soniox),
            elevenlabs: f.elevenlabs.apply(d.elevenlabs),
            openai: f.openai.apply(d.openai),
        }
    }
}

impl Providers {
    pub fn get(&self, id: &str) -> Option<&Provider> {
        match id {
            "soniox" => Some(&self.soniox),
            "elevenlabs" => Some(&self.elevenlabs),
            "openai" => Some(&self.openai),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Credit {
    pub amount: f64,
    /// YYYY-MM-DD
    pub since: String,
}

pub const PROVIDERS: [&str; 3] = ["soniox", "elevenlabs", "openai"];

pub fn provider_name(id: &str) -> &str {
    match id {
        "soniox" => "Soniox",
        "elevenlabs" => "ElevenLabs",
        "openai" => "OpenAI",
        other => other,
    }
}

pub fn config_dir() -> PathBuf {
    directories::BaseDirs::new().map(|d| d.config_dir().join(APP)).unwrap_or_else(|| PathBuf::from(".").join(APP))
}

pub fn config_path() -> PathBuf {
    std::env::var_os("MIRZA_CONFIG").map(PathBuf::from).unwrap_or_else(|| config_dir().join("config.toml"))
}

pub fn data_dir() -> PathBuf {
    directories::BaseDirs::new().map(|d| d.data_local_dir().join(APP)).unwrap_or_else(|| PathBuf::from(".").join(APP))
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("reading {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("{0}: {1}")]
    Parse(PathBuf, toml::de::Error),
}

impl Config {
    /// Reads the config file, creating it with commented defaults if missing.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(s) => Self::parse(&s).map_err(|e| ConfigError::Parse(path.to_owned(), e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = write_private(path, TEMPLATE);
                Ok(Self::default())
            }
            Err(e) => Err(ConfigError::Io(path.to_owned(), e)),
        }
    }

    pub fn parse(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    pub fn provider(&self) -> &Provider {
        self.providers.get(&self.active_provider).unwrap_or(&self.providers.soniox)
    }
}

/// Edits the config file in place, keeping comments and layout, and checks
/// that the result is still a valid config before saving it.
pub fn update_file(path: &Path, f: impl FnOnce(&mut toml_edit::DocumentMut)) -> std::io::Result<()> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(std::io::Error::other)?;
    f(&mut doc);
    let out = doc.to_string();
    Config::parse(&out).map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    write_private(path, &out)
}

/// Sets a value by dotted path, e.g. "typing.paste_key" or
/// "providers.soniox.languages". JSON null removes the key (back to the
/// default).
pub fn set_json(doc: &mut toml_edit::DocumentMut, path: &str, v: &serde_json::Value) -> Result<(), String> {
    let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();
    let Some((last, tables)) = parts.split_last() else { return Err("empty setting name".into()) };
    let mut item = doc.as_item_mut();
    for t in tables {
        if item.get(t).is_none() {
            let mut table = toml_edit::Table::new();
            table.set_implicit(true);
            item.as_table_like_mut()
                .ok_or_else(|| format!("{path}: not a table"))?
                .insert(t, toml_edit::Item::Table(table));
        }
        item = &mut item[*t];
    }
    let table = item.as_table_like_mut().ok_or_else(|| format!("{path}: not a table"))?;
    if v.is_null() {
        table.remove(last);
    } else {
        table.insert(last, toml_edit::Item::Value(json_to_toml(v)?));
    }
    Ok(())
}

fn json_to_toml(v: &serde_json::Value) -> Result<toml_edit::Value, String> {
    use serde_json::Value as J;
    Ok(match v {
        J::Bool(b) => (*b).into(),
        J::Number(n) if n.is_i64() => n.as_i64().unwrap_or_default().into(),
        J::Number(n) => n.as_f64().unwrap_or_default().into(),
        J::String(s) => s.as_str().into(),
        J::Array(items) => {
            let mut arr = toml_edit::Array::new();
            for i in items {
                arr.push(json_to_toml(i)?);
            }
            toml_edit::Value::Array(arr)
        }
        J::Null | J::Object(_) => return Err("unsupported value".into()),
    })
}

/// Replaces the [[shortcuts]] list.
pub fn set_shortcuts(doc: &mut toml_edit::DocumentMut, list: &[Shortcut]) {
    if list.is_empty() {
        doc.remove("shortcuts");
        return;
    }
    let mut arr = toml_edit::ArrayOfTables::new();
    for sc in list {
        let mut t = toml_edit::Table::new();
        let action =
            serde_json::to_value(sc.action).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        t["action"] = toml_edit::value(action);
        t["keys"] = toml_edit::value(sc.keys.as_str());
        if sc.action == Action::Dictate {
            t["mode"] = toml_edit::value(if sc.mode == Mode::Hold { "hold" } else { "toggle" });
        }
        arr.push(t);
    }
    doc.insert("shortcuts", toml_edit::Item::ArrayOfTables(arr));
}

/// Writes a file readable only by the user.
pub fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        let mut f = opts.open(&tmp)?;
        std::io::Write::write_all(&mut f, contents.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// The file written on first run. Everything here can also be changed in the
/// settings window.
pub const TEMPLATE: &str = r#"# Mirza settings. Every key is optional; the values shown are the defaults.
# Changes apply as soon as the file is saved. API keys are kept in the
# system keyring, not here (see "mirza set-key").

# active_provider = "soniox"      # soniox, elevenlabs or openai

# Global shortcuts. Add one or more; `mode` is "toggle" or "hold".
# On Linux (Wayland) the desktop owns the actual key: the value here is only
# suggested the first time, and you change it in the system shortcut settings.
# [[shortcuts]]
# action = "dictate"              # dictate, cancel, panel or next_provider
# keys = "Meta+H"
# mode = "toggle"

# min_hold_ms = 150               # hold mode: shorter presses are ignored
# silence_stop_sec = 60           # stop after this long without speech (0: never)
# max_session_sec = 900           # hard limit per session (0: never)
# mic_device = ""                 # empty: system default
# proxy = ""                      # "", "none", or e.g. "http://user:pass@host:port"
# notifications = true
# start_on_login = true
# history_size = 20

# [typing]
# mode = "final"                  # final, or live (types guesses and corrects them)
# paste_key = "shift+insert"
# paste_over_chars = 0            # paste text longer than this instead of typing (0: never)
# keep_on_clipboard = false
# persian_letters = true          # write Arabic ي/ك as Persian ی/ک
# follow_focus = true             # pause while another window is active (KDE)

# [providers.soniox]
# model = "stt-rt-v5"
# languages = ["fa", "en"]
# strict_languages = false
# terms = []                      # names and words to expect
# price_per_min = 0.002

# [credit.soniox]                 # prepaid credit, to show what is left
# amount = 20.0
# since = "2026-09-01"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_parses_to_defaults() {
        assert_eq!(Config::parse(TEMPLATE).unwrap(), Config::default());
    }

    #[test]
    fn reads_shortcuts_and_credit() {
        let cfg = Config::parse(
            r#"
            active_provider = "openai"
            [[shortcuts]]
            action = "dictate"
            keys = "RightCtrl"
            mode = "hold"
            [[shortcuts]]
            action = "cancel"
            keys = "Meta+Escape"
            [providers.soniox]
            languages = ["fa"]
            [credit.soniox]
            amount = 20.0
            since = "2026-09-01"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.shortcuts.len(), 2);
        assert_eq!(cfg.shortcuts[0].id(), "dictate-hold");
        assert_eq!(cfg.shortcuts[1].mode, Mode::Toggle);
        assert_eq!(cfg.providers.soniox.languages, ["fa"]);
        assert_eq!(cfg.providers.soniox.model, "stt-rt-v5");
        assert_eq!(cfg.credit["soniox"].amount, 20.0);
        assert_eq!(cfg.provider().model, "gpt-live-transcribe");
    }

    #[test]
    fn edits_keep_comments_and_parse() {
        let mut doc: toml_edit::DocumentMut = TEMPLATE.parse().unwrap();
        set_json(&mut doc, "typing.paste_key", &serde_json::json!("ctrl+v")).unwrap();
        set_json(&mut doc, "providers.soniox.languages", &serde_json::json!(["fa"])).unwrap();
        set_json(&mut doc, "credit.soniox.amount", &serde_json::json!(20.5)).unwrap();
        set_json(&mut doc, "silence_stop_sec", &serde_json::json!(30)).unwrap();
        set_shortcuts(&mut doc, &[Shortcut { action: Action::Dictate, keys: "Meta+H".into(), mode: Mode::Hold }]);
        let out = doc.to_string();
        assert!(out.contains("# Mirza settings."), "comments survive");
        let cfg = Config::parse(&out).unwrap();
        assert_eq!(cfg.typing.paste_key, "ctrl+v");
        assert_eq!(cfg.providers.soniox.languages, ["fa"]);
        assert_eq!(cfg.providers.soniox.model, "stt-rt-v5");
        assert_eq!(cfg.credit["soniox"].amount, 20.5);
        assert_eq!(cfg.silence_stop_sec, 30);
        assert_eq!(cfg.shortcuts[0].mode, Mode::Hold);
        set_json(&mut doc, "silence_stop_sec", &serde_json::Value::Null).unwrap();
        assert_eq!(Config::parse(&doc.to_string()).unwrap().silence_stop_sec, 60);
    }

    #[test]
    fn rejects_typos() {
        assert!(Config::parse("silense_stop_sec = 3").is_err());
    }
}
