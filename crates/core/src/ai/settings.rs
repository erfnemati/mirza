//! The `[revision]` and `[ai.*]` parts of the config file.

use serde::{Deserialize, Serialize};

use super::Kind;

/// How much the model may think before it answers. Revising dictation needs
/// none, and thinking makes it slow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    /// Send nothing; the model's own default.
    #[default]
    Default,
    /// No thinking at all, where the model allows it.
    #[serde(rename = "none")]
    Off,
    Minimal,
    Low,
    Medium,
    High,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Default => "default",
            Effort::Off => "none",
            Effort::Minimal => "minimal",
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
        }
    }

    /// Whether the model may spend real time thinking, so its answer needs
    /// room for that.
    pub(crate) fn thinks(self) -> bool {
        matches!(self, Effort::Low | Effort::Medium | Effort::High)
    }
}

/// One AI provider's settings.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct AiProvider {
    pub model: String,
    /// "" for the provider's own address. A local server needs one.
    pub base_url: String,
    pub effort: Effort,
}

impl AiProvider {
    fn new(model: &str, effort: Effort) -> Self {
        Self { model: model.into(), base_url: String::new(), effort }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "AiProvidersFile")]
pub struct AiProviders {
    pub openai: AiProvider,
    pub gemini: AiProvider,
    pub claude: AiProvider,
    pub openrouter: AiProvider,
    pub local: AiProvider,
}

/// Fast, cheap models that clean up text well. Checked on 2026-11-06.
impl Default for AiProviders {
    fn default() -> Self {
        Self {
            openai: AiProvider::new("gpt-6-luna", Effort::Off),
            // Flash-Lite already thinks at its lowest level by default, and some
            // other Gemini models refuse "minimal".
            gemini: AiProvider::new("gemini-3.5-flash-lite", Effort::Default),
            claude: AiProvider::new("claude-haiku-5-5", Effort::Off),
            openrouter: AiProvider::new("openai/gpt-6-luna", Effort::Off),
            local: AiProvider {
                model: String::new(),
                base_url: Kind::Local.default_base_url().into(),
                effort: Effort::Default,
            },
        }
    }
}

impl AiProviders {
    pub fn get(&self, kind: Kind) -> &AiProvider {
        match kind {
            Kind::OpenAi => &self.openai,
            Kind::Gemini => &self.gemini,
            Kind::Claude => &self.claude,
            Kind::OpenRouter => &self.openrouter,
            Kind::Local => &self.local,
        }
    }
}

/// What the file says; anything missing keeps that provider's own default.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AiProvidersFile {
    openai: AiPatch,
    gemini: AiPatch,
    claude: AiPatch,
    openrouter: AiPatch,
    local: AiPatch,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AiPatch {
    model: Option<String>,
    base_url: Option<String>,
    effort: Option<Effort>,
}

impl AiPatch {
    fn apply(self, mut p: AiProvider) -> AiProvider {
        p.model = self.model.map(|m| m.trim().to_owned()).unwrap_or(p.model);
        p.base_url = self.base_url.map(|b| b.trim().to_owned()).unwrap_or(p.base_url);
        p.effort = self.effort.unwrap_or(p.effort);
        p
    }
}

impl From<AiProvidersFile> for AiProviders {
    fn from(f: AiProvidersFile) -> Self {
        let d = AiProviders::default();
        Self {
            openai: f.openai.apply(d.openai),
            gemini: f.gemini.apply(d.gemini),
            claude: f.claude.apply(d.claude),
            openrouter: f.openrouter.apply(d.openrouter),
            local: f.local.apply(d.local),
        }
    }
}

/// Revising dictation with AI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Revision {
    /// "" (off), "openai", "gemini", "claude", "openrouter" or "local".
    pub provider: String,
    /// Give up and type the original text after this many seconds. Long
    /// dictations get a little more.
    pub timeout_sec: u64,
    /// Your own rules, added to Mirza's, e.g. "Write numbers as digits".
    pub instructions: String,
}

impl Default for Revision {
    fn default() -> Self {
        Self { provider: String::new(), timeout_sec: 10, instructions: String::new() }
    }
}

impl Revision {
    /// The chosen provider, or None when revision is off or the name is wrong.
    pub fn kind(&self) -> Option<Kind> {
        Kind::parse(&self.provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_keys_keep_each_providers_default() {
        let p: AiProviders = toml::from_str("[claude]\neffort = \"low\"\n[local]\nmodel = \"gemma\"").unwrap();
        assert_eq!(p.claude.model, "claude-haiku-5-5");
        assert_eq!(p.claude.effort, Effort::Low);
        assert_eq!(p.local.model, "gemma");
        assert_eq!(p.local.base_url, "http://localhost:8000/v1");
        assert_eq!(p.gemini, AiProviders::default().gemini);
        assert!(toml::from_str::<AiProviders>("[claude]\nmodle = \"x\"").is_err());
        assert!(toml::from_str::<AiProviders>("[claude]\neffort = \"max\"").is_err());
    }
}
