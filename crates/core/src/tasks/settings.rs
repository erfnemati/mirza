//! The `[tasks]` part of the config file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ai::{Kind, Revision};

/// Voice tasks: what you say becomes a Google Tasks entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TaskSettings {
    /// "" uses the AI revision provider; otherwise "openai", "gemini",
    /// "claude", "openrouter" or "local". With neither, tasks are saved as
    /// said.
    pub provider: String,
    /// How long to wait for the AI before saving the task as said.
    pub timeout_sec: u64,
    /// Your own Google OAuth client; "" uses the one built into this copy of
    /// Mirza, if it has one.
    pub google_client_id: String,
    /// Google Tasks list id for tasks that fit no list; "" is your default
    /// list.
    pub default_list: String,
    /// Event length when you don't say one.
    pub event_minutes: u32,
    /// A task with a clock time also gets that day as its due date, besides
    /// the Calendar event.
    pub timed_due_date: bool,
    /// Other names for a list, by Google Tasks list id, e.g. "office, Acme".
    pub list_hints: BTreeMap<String, String>,
}

impl Default for TaskSettings {
    fn default() -> Self {
        Self {
            provider: String::new(),
            timeout_sec: 30,
            google_client_id: String::new(),
            default_list: String::new(),
            event_minutes: 30,
            timed_due_date: true,
            list_hints: BTreeMap::new(),
        }
    }
}

/// The client ID built into this copy, from `MIRZA_GOOGLE_CLIENT_ID` at build
/// time; empty when it was built without one.
pub const BUILT_IN_CLIENT_ID: &str = match option_env!("MIRZA_GOOGLE_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

/// The secret that goes with [`BUILT_IN_CLIENT_ID`].
pub const BUILT_IN_CLIENT_SECRET: &str = match option_env!("MIRZA_GOOGLE_CLIENT_SECRET") {
    Some(s) => s,
    None => "",
};

impl TaskSettings {
    /// The AI to use: this section's own, else AI revision's. None when both
    /// are off.
    pub fn kind(&self, revision: &Revision) -> Option<Kind> {
        match self.provider.trim() {
            "" => revision.kind(),
            p => Kind::parse(p),
        }
    }

    /// The OAuth client to sign in with, and its secret; None when there is
    /// no client.
    pub fn client(&self) -> Option<(String, Option<String>)> {
        match self.google_client_id.trim() {
            "" if BUILT_IN_CLIENT_ID.trim().is_empty() => None,
            "" => {
                let secret = Some(BUILT_IN_CLIENT_SECRET.trim().to_owned()).filter(|s| !s.is_empty());
                Some((BUILT_IN_CLIENT_ID.trim().to_owned(), secret))
            }
            id => Some((id.to_owned(), crate::secrets::get(crate::secrets::GOOGLE_CLIENT_SECRET).map(|(s, _)| s))),
        }
    }

    /// Event length in minutes, between 5 minutes and a day.
    pub fn event_minutes(&self) -> u32 {
        self.event_minutes.clamp(5, 24 * 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_falls_back_to_revision() {
        let mut r = Revision::default();
        let mut t = TaskSettings::default();
        assert_eq!(t.kind(&r), None);
        r.provider = "claude".into();
        assert_eq!(t.kind(&r), Some(Kind::Claude));
        t.provider = "gemini".into();
        assert_eq!(t.kind(&r), Some(Kind::Gemini));
    }

    #[test]
    fn hints_by_list_id() {
        let t: TaskSettings = toml::from_str("event_minutes = 0\n[list_hints]\n\"MDE2\" = \"office, Acme\"").unwrap();
        assert_eq!(t.list_hints["MDE2"], "office, Acme");
        assert_eq!(t.event_minutes(), 5);
        assert!(toml::from_str::<TaskSettings>("lists = 1").is_err());
    }
}
