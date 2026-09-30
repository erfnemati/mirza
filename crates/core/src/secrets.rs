//! API keys. They are looked up in this order: an environment variable
//! (MIRZA_SONIOX_API_KEY, or SONIOX_API_KEY and friends), then the keys file,
//! which only the user can read.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::config::{config_dir, write_private};

pub fn keys_path() -> PathBuf {
    config_dir().join("keys.toml")
}

fn env_names(provider: &str) -> Vec<String> {
    let up = provider.to_uppercase();
    vec![format!("MIRZA_{up}_API_KEY"), format!("{up}_API_KEY")]
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
