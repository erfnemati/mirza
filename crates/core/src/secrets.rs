//! API keys. They are looked up in this order: an environment variable
//! (MIRZA_SONIOX_API_KEY, or SONIOX_API_KEY and friends), then the keys file,
//! which only the user can read.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::config::{config_dir, write_private};

pub fn keys_path() -> PathBuf {
    config_dir().join("keys.toml")
}

/// The Google sign-in for voice tasks, kept like a key. Not "google": that
/// name would also read GOOGLE_API_KEY, which is a Gemini key.
pub const GOOGLE_REFRESH_TOKEN: &str = "google_refresh_token";
/// The secret of your own Google OAuth client.
pub const GOOGLE_CLIENT_SECRET: &str = "google_client_secret";

fn env_names(provider: &str) -> Vec<String> {
    if provider.starts_with("google_") {
        return vec![format!("MIRZA_{}", provider.to_uppercase())];
    }
    let up = provider.to_uppercase();
    let mut names = vec![format!("MIRZA_{up}_API_KEY")];
    match provider {
        // LOCAL_API_KEY is too generic to be meant for Mirza.
        "local" => {}
        "claude" => names.push("ANTHROPIC_API_KEY".into()),
        "gemini" => names.extend(["GEMINI_API_KEY".into(), "GOOGLE_API_KEY".into()]),
        _ => names.push(format!("{up}_API_KEY")),
    }
    names
}

fn read_file() -> BTreeMap<String, String> {
    std::fs::read_to_string(keys_path()).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
}

/// Where a key came from, for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Env,
    File,
}

pub fn get(provider: &str) -> Option<(String, Source)> {
    for name in env_names(provider) {
        if let Ok(v) = std::env::var(&name)
            && !v.trim().is_empty()
        {
            return Some((v.trim().to_owned(), Source::Env));
        }
    }
    read_file().get(provider).filter(|v| !v.trim().is_empty()).map(|v| (v.trim().to_owned(), Source::File))
}

pub fn set(provider: &str, key: &str) -> std::io::Result<()> {
    let mut keys = read_file();
    if key.trim().is_empty() {
        keys.remove(provider);
    } else {
        keys.insert(provider.to_owned(), key.trim().to_owned());
    }
    std::fs::create_dir_all(config_dir())?;
    let body = toml::to_string(&keys).map_err(std::io::Error::other)?;
    write_private(&keys_path(), &format!("# API keys, one per provider. Keep this file private.\n{body}"))
}

/// Every service that can have a key: the speech providers, then the AI
/// providers (OpenAI's key serves both).
pub fn services() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = crate::config::PROVIDERS.to_vec();
    for k in crate::ai::Kind::ALL {
        if !ids.contains(&k.id()) {
            ids.push(k.id());
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_names_per_service() {
        assert_eq!(env_names("soniox"), ["MIRZA_SONIOX_API_KEY", "SONIOX_API_KEY"]);
        assert_eq!(env_names("claude"), ["MIRZA_CLAUDE_API_KEY", "ANTHROPIC_API_KEY"]);
        assert_eq!(env_names("gemini"), ["MIRZA_GEMINI_API_KEY", "GEMINI_API_KEY", "GOOGLE_API_KEY"]);
        assert_eq!(env_names("local"), ["MIRZA_LOCAL_API_KEY"]);
        assert_eq!(env_names(GOOGLE_REFRESH_TOKEN), ["MIRZA_GOOGLE_REFRESH_TOKEN"]);
        assert_eq!(services(), ["soniox", "elevenlabs", "openai", "gemini", "claude", "openrouter", "local"]);
    }
}
