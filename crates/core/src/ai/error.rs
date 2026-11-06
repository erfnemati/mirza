//! What went wrong with an AI request, in plain words, with what usually
//! helps.

use std::fmt;

use super::{Kind, shorten, sounds_like_credit, sounds_like_region};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NoKey,
    /// A setting is missing or wrong (address, model).
    NotSetUp,
    KeyRejected,
    /// 403: a wrong key, or the service isn't offered in the user's country.
    Forbidden,
    Credit,
    RateLimited,
    Region,
    ModelNotFound,
    BadRequest,
    ServerError,
    Timeout,
    Network,
    /// The answer hit the length limit.
    CutOff,
    Refused,
    Unreadable,
    /// The answer doesn't look like a revision of the text.
    Suspicious,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiError {
    pub kind: ErrorKind,
    pub provider: Kind,
    /// The provider's own message or a short reason; may be empty.
    pub detail: String,
    /// Whether the request went through the proxy.
    pub proxied: bool,
}

impl AiError {
    pub fn new(kind: ErrorKind, provider: Kind, detail: String, proxied: bool) -> Self {
        Self { kind, provider, detail, proxied }
    }
}

impl std::error::Error for AiError {}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.provider.name();
        let local = self.provider == Kind::Local;
        let detail = match self.detail.trim().trim_end_matches('.') {
            "" => String::new(),
            d => format!(" ({d})"),
        };
        let settings = "Settings > AI revision";
        let s = match self.kind {
            ErrorKind::NoKey => {
                format!("No API key for {name}. Add it in {settings}, or run `mirza set-key {}`.", self.provider.id())
            }
            ErrorKind::NotSetUp => format!("{name} isn't set up: {}. Check {settings}.", self.detail),
            ErrorKind::KeyRejected => format!("{name} didn't accept the API key{detail}. Check it in {settings}."),
            ErrorKind::Forbidden => format!(
                "{name} refused access{detail}. Check the API key; if {name} isn't offered in your country, {}.",
                if self.proxied { "try a proxy in another country" } else { "set a proxy in Settings > General" }
            ),
            ErrorKind::Credit => {
                format!("{name} says the account is out of credit or quota{detail}. Check billing on its website.")
            }
            ErrorKind::RateLimited => {
                format!("{name} is getting too many requests{detail}. Wait a moment and try again.")
            }
            ErrorKind::Region if self.proxied => {
                format!("{name} isn't offered where your proxy connects from{detail}. Try a proxy in another country.")
            }
            ErrorKind::Region => {
                format!("{name} isn't offered in your country{detail}. Set a proxy in Settings > General.")
            }
            ErrorKind::ModelNotFound => {
                format!("{name} doesn't have the chosen model{detail}. Pick another one in {settings}.")
            }
            ErrorKind::BadRequest => {
                format!("{name} rejected the request{detail}. Check the model and settings in {settings}.")
            }
            ErrorKind::ServerError => format!("{name} had a problem on its side{detail}. Try again in a moment."),
            ErrorKind::Timeout if local => {
                format!(
                    "{name} took too long to answer. Check that it isn't busy, or raise the time limit in {settings}."
                )
            }
            ErrorKind::Timeout => {
                format!("{name} took too long to answer. Try again, or raise the time limit in {settings}.")
            }
            ErrorKind::Network => {
                let hint = if local {
                    "Check that the server is running and the address is right.".to_owned()
                } else if self.proxied {
                    "Check that your proxy is working.".to_owned()
                } else {
                    format!("If {name} is blocked where you are, set a proxy in Settings > General.")
                };
                format!("Couldn't reach {name}{detail}. {hint}")
            }
            ErrorKind::CutOff => format!("{name}'s answer was cut off before the end."),
            ErrorKind::Refused => format!("{name} refused to work on this text{detail}."),
            ErrorKind::Unreadable => format!("{name} sent an answer Mirza couldn't read{detail}."),
            ErrorKind::Suspicious => format!("{name}'s answer didn't look like your text{detail}."),
        };
        // "your local AI server ..." at the start of a sentence.
        let mut chars = s.chars();
        match chars.next() {
            Some(c) => write!(f, "{}{}", c.to_uppercase(), chars.as_str()),
            None => Ok(()),
        }
    }
}

/// What an HTTP error status (or an error code inside a 200 answer) means.
pub(super) fn from_status(status: u16, msg: &str) -> ErrorKind {
    let lower = msg.to_ascii_lowercase();
    let model_missing = lower.contains("model")
        && [
            "not found",
            "does not exist",
            "not exist",
            "unknown model",
            "no such",
            "invalid model",
            "not a valid model",
        ]
        .iter()
        .any(|w| lower.contains(w));
    let bad_key = ["api key not valid", "invalid api key", "invalid x-api-key", "incorrect api key", "api_key_invalid"]
        .iter()
        .any(|w| lower.contains(w));
    match status {
        401 => ErrorKind::KeyRejected,
        402 => ErrorKind::Credit,
        403 if sounds_like_region(msg) => ErrorKind::Region,
        403 => ErrorKind::Forbidden,
        404 => ErrorKind::ModelNotFound,
        408 | 504 => ErrorKind::Timeout,
        429 if sounds_like_credit(msg) => ErrorKind::Credit,
        429 => ErrorKind::RateLimited,
        _ if sounds_like_region(msg) && !model_missing => ErrorKind::Region,
        _ if bad_key => ErrorKind::KeyRejected,
        _ if model_missing => ErrorKind::ModelNotFound,
        400..=499 if sounds_like_credit(msg) => ErrorKind::Credit,
        500..=599 => ErrorKind::ServerError,
        _ => ErrorKind::BadRequest,
    }
}

/// An HTTP error answer: its kind and the provider's message.
pub(super) fn from_answer(status: u16, body: &str) -> (ErrorKind, String) {
    let msg = super::error_message(body);
    let kind = from_status(status, &msg);
    let detail = if msg.is_empty() { format!("HTTP {status}") } else { shorten(&msg, 200) };
    (kind, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_plain_kinds() {
        assert_eq!(from_status(401, ""), ErrorKind::KeyRejected);
        assert_eq!(from_status(403, "Request not allowed"), ErrorKind::Forbidden);
        assert_eq!(from_status(403, "Country, region, or territory not supported"), ErrorKind::Region);
        assert_eq!(from_status(400, "User location is not supported for the API use."), ErrorKind::Region);
        assert_eq!(from_status(400, "API key not valid. Please pass a valid API key."), ErrorKind::KeyRejected);
        assert_eq!(from_status(400, "Your credit balance is too low to access the Anthropic API."), ErrorKind::Credit);
        assert_eq!(from_status(429, "You exceeded your current quota"), ErrorKind::Credit);
        assert_eq!(from_status(429, "Rate limit reached"), ErrorKind::RateLimited);
        assert_eq!(from_status(400, "The model `gpt-x` does not exist"), ErrorKind::ModelNotFound);
        assert_eq!(from_status(404, ""), ErrorKind::ModelNotFound);
        assert_eq!(from_status(529, "Overloaded"), ErrorKind::ServerError);
        assert_eq!(from_status(400, "max_tokens: too large"), ErrorKind::BadRequest);
    }

    #[test]
    fn messages_say_what_helps() {
        let e = |kind, provider, proxied| AiError::new(kind, provider, String::new(), proxied).to_string();
        assert_eq!(
            e(ErrorKind::Network, Kind::Claude, false),
            "Couldn't reach Claude. If Claude is blocked where you are, set a proxy in Settings > General."
        );
        assert!(e(ErrorKind::Network, Kind::Claude, true).ends_with("Check that your proxy is working."));
        assert!(e(ErrorKind::Network, Kind::Local, false).starts_with("Couldn't reach your local AI server."));
        assert!(e(ErrorKind::Timeout, Kind::Local, false).starts_with("Your local AI server took too long"));
        assert!(e(ErrorKind::NoKey, Kind::Gemini, false).contains("mirza set-key gemini"));
        let d = AiError::new(ErrorKind::KeyRejected, Kind::OpenAi, "Incorrect API key".into(), false).to_string();
        assert_eq!(d, "OpenAI didn't accept the API key (Incorrect API key). Check it in Settings > AI revision.");
    }
}
