//! Text models: one call in, text (or JSON text) out. Used to revise
//! dictation now, and later to turn speech into typed tasks.
//!
//! Three wire formats cover the five providers: OpenAI's Chat Completions
//! (OpenAI, OpenRouter and local servers such as vLLM), Anthropic's Messages
//! API (Claude) and Google's generateContent (Gemini). Each one is a pair of
//! pure functions that build the request and read the answer, so they can be
//! tested without a network.

mod claude;
mod error;
mod gemini;
mod openai_chat;
pub mod revise;
mod settings;

#[cfg(test)]
pub(crate) mod mock;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;
use url::Url;

pub use error::{AiError, ErrorKind};
pub use settings::{AiProvider, AiProviders, Effort, Preset, Revision, Style};

/// The AI providers Mirza can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    OpenAi,
    Gemini,
    Claude,
    OpenRouter,
    Local,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::OpenAi, Kind::Gemini, Kind::Claude, Kind::OpenRouter, Kind::Local];

    /// The ID used in the config file and for the API key.
    pub fn id(self) -> &'static str {
        match self {
            Kind::OpenAi => "openai",
            Kind::Gemini => "gemini",
            Kind::Claude => "claude",
            Kind::OpenRouter => "openrouter",
            Kind::Local => "local",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id.trim())
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::OpenAi => "OpenAI",
            Kind::Gemini => "Gemini",
            Kind::Claude => "Claude",
            Kind::OpenRouter => "OpenRouter",
            Kind::Local => "your local AI server",
        }
    }

    /// Where requests go when the settings name no base URL.
    pub fn default_base_url(self) -> &'static str {
        match self {
            Kind::OpenAi => "https://api.openai.com/v1",
            Kind::Gemini => "https://generativelanguage.googleapis.com",
            Kind::Claude => "https://api.anthropic.com",
            Kind::OpenRouter => "https://openrouter.ai/api/v1",
            Kind::Local => "http://localhost:8000/v1",
        }
    }

    /// A local server usually runs without a key.
    pub fn needs_key(self) -> bool {
        self != Kind::Local
    }
}

/// What the answer should look like.
#[derive(Debug, Clone, PartialEq)]
pub enum Format {
    Text,
    /// JSON matching this schema. The answer is the JSON text; the caller
    /// parses and checks it.
    JsonSchema {
        name: String,
        schema: Value,
    },
}

/// One request: instructions, the user's text, and how long the answer may be.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub system: String,
    pub user: String,
    pub max_tokens: u32,
    pub format: Format,
}

/// A request ready to send: where, with which headers, and the JSON body.
#[derive(Debug, Clone, PartialEq)]
struct Http {
    url: String,
    headers: Vec<(&'static str, String)>,
    body: Value,
}

/// A configured provider, ready to answer requests.
#[derive(Debug, Clone)]
pub struct Client {
    kind: Kind,
    model: String,
    base_url: String,
    effort: Effort,
    api_key: Option<String>,
    http: reqwest::Client,
    proxied: bool,
}

impl Client {
    /// `proxy` is the resolved proxy setting; servers on this computer or the
    /// local network are reached directly (see [`direct_host`]). `timeout`
    /// covers the whole request.
    pub fn new(
        kind: Kind,
        settings: &AiProvider,
        api_key: Option<String>,
        proxy: Option<&Url>,
        timeout: Duration,
    ) -> Result<Self, AiError> {
        let err = |k, d: String| AiError::new(k, kind, d, proxy.is_some());
        let base = match settings.base_url.trim() {
            "" => kind.default_base_url(),
            b => b,
        }
        .trim_end_matches('/')
        .to_owned();
        let url = Url::parse(&base)
            .map_err(|e| err(ErrorKind::NotSetUp, format!("the address {base:?} is not valid ({e})")))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(err(ErrorKind::NotSetUp, format!("the address {base:?} must start with http:// or https://")));
        }
        let api_key = api_key.filter(|k| !k.trim().is_empty());
        if kind.needs_key() && api_key.is_none() {
            return Err(err(ErrorKind::NoKey, String::new()));
        }
        let proxy = if direct_host(&url) { None } else { proxy };
        let http = crate::usage::http_client(proxy, timeout).map_err(|e| err(ErrorKind::NotSetUp, e))?;
        Ok(Self {
            kind,
            model: settings.model.trim().to_owned(),
            base_url: base,
            effort: settings.effort,
            api_key,
            http,
            proxied: proxy.is_some(),
        })
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The model asked for; empty for a local server that picks its own.
    pub fn model(&self) -> &str {
        &self.model
    }

    fn error(&self, kind: ErrorKind, detail: impl Into<String>) -> AiError {
        AiError::new(kind, self.kind, detail.into(), self.proxied)
    }

    /// Sends the request and returns the answer's text.
    pub async fn complete(&self, req: &Request) -> Result<String, AiError> {
        let model = match (self.kind, self.model.is_empty()) {
            (Kind::Local, true) => self.local_model().await?,
            (_, true) => return Err(self.error(ErrorKind::NotSetUp, "no model is set")),
            (_, false) => self.model.clone(),
        };
        let key = self.api_key.as_deref();
        let http = match self.kind {
            Kind::Claude => claude::build(&self.base_url, key, &model, self.effort, req),
            Kind::Gemini => gemini::build(&self.base_url, key, &model, self.effort, req),
            _ => openai_chat::build(self.kind, &self.base_url, key, &model, self.effort, req),
        };
        let (status, body) = self.send(self.http.post(&http.url).json(&http.body), &http.headers).await?;
        let answer = match self.kind {
            Kind::Claude => claude::parse(status, &body),
            Kind::Gemini => gemini::parse(status, &body),
            _ => openai_chat::parse(self.kind, status, &body),
        };
        answer.map_err(|(kind, detail)| {
            if self.kind == Kind::Local && self.model.is_empty() {
                // The server may have switched models; ask again next time.
                local_models().remove(&self.base_url);
            }
            self.error(kind, detail)
        })
    }

    async fn send(&self, rb: reqwest::RequestBuilder, headers: &[(&str, String)]) -> Result<(u16, String), AiError> {
        let mut rb = rb;
        for (k, v) in headers {
            rb = rb.header(*k, v);
        }
        let resp = rb.send().await.map_err(|e| self.request_error(&e))?;
        let status = resp.status().as_u16();
        let body = resp.text().await.map_err(|e| self.request_error(&e))?;
        Ok((status, body))
    }

    fn request_error(&self, e: &reqwest::Error) -> AiError {
        if e.is_timeout() {
            self.error(ErrorKind::Timeout, String::new())
        } else {
            self.error(ErrorKind::Network, crate::usage::describe(e))
        }
    }

    /// The first model a local server lists, for when none is set. Asked once
    /// per address, not on every dictation.
    async fn local_model(&self) -> Result<String, AiError> {
        if let Some(m) = local_models().get(&self.base_url) {
            return Ok(m.clone());
        }
        let model =
            self.models().await?.into_iter().next().ok_or_else(|| {
                self.error(ErrorKind::NotSetUp, "the server lists no models; set one in the settings")
            })?;
        local_models().insert(self.base_url.clone(), model.clone());
        Ok(model)
    }

    /// The models the provider offers, by ID, in its own order; for Gemini
    /// only the ones that suit revision, newest first.
    pub async fn models(&self) -> Result<Vec<String>, AiError> {
        let key = self.api_key.clone().unwrap_or_default();
        let (url, headers): (String, Vec<(&str, String)>) = match self.kind {
            Kind::Claude => (
                format!("{}/v1/models?limit=100", self.base_url),
                vec![("x-api-key", key), ("anthropic-version", claude::VERSION.to_owned())],
            ),
            Kind::Gemini => (format!("{}/v1beta/models?pageSize=200", self.base_url), vec![("x-goog-api-key", key)]),
            _ if self.api_key.is_some() => {
                (format!("{}/models", self.base_url), vec![("authorization", format!("Bearer {key}"))])
            }
            _ => (format!("{}/models", self.base_url), Vec::new()),
        };
        let (status, body) = self.send(self.http.get(url), &headers).await?;
        if !(200..300).contains(&status) {
            let (kind, detail) = openai_chat::error_kind(self.kind, status, &body);
            return Err(self.error(kind, detail));
        }
        let v: Value = serde_json::from_str(&body).unwrap_or_default();
        let ids = match self.kind {
            Kind::Gemini => gemini::pick_models(
                v["models"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|m| {
                        m["supportedGenerationMethods"]
                            .as_array()
                            .is_none_or(|a| a.iter().any(|x| x == "generateContent"))
                    })
                    .filter_map(|m| m["name"].as_str())
                    .map(|n| n.trim_start_matches("models/").to_owned())
                    .collect(),
            ),
            _ => {
                v["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str()).map(str::to_owned).collect()
            }
        };
        Ok(ids)
    }
}

fn local_models() -> std::sync::MutexGuard<'static, HashMap<String, String>> {
    static MODELS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    MODELS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// Room for the answer: thinking counts against the limit too.
fn budget(max_tokens: u32, effort: Effort) -> u32 {
    if effort.thinks() { max_tokens.saturating_add(8192) } else { max_tokens }
}

/// Whether `url` points at this computer or the local network, which is
/// reached directly rather than through the proxy: a proxy can't reach a
/// server on your LAN, and the text has no reason to leave it.
pub fn direct_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified(),
        Some(url::Host::Ipv6(ip)) => {
            let first = ip.segments()[0];
            ip.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            !d.contains('.')
                || [".localhost", ".local", ".lan", ".home.arpa", ".internal"].iter().any(|s| d.ends_with(s))
        }
        None => false,
    }
}

/// The message out of an error answer like `{"error": {"message": ...}}`,
/// shortened so a long answer doesn't fill a notification.
fn error_message(body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or_default();
    let e = &v["error"];
    let msg = e["message"].as_str().or_else(|| e.as_str()).map(str::to_owned).unwrap_or_else(|| {
        // Not JSON: a proxy's or gateway's own page. Its first line will do.
        body.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_owned()
    });
    shorten(msg.trim(), 200)
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// Words that mark "not available in your country" errors.
fn sounds_like_region(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    ["location is not supported", "country", "region", "territory"].iter().any(|w| m.contains(w))
}

/// Words that mark "out of money or quota" errors.
fn sounds_like_credit(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    ["quota", "credit", "billing", "balance", "insufficient"].iter().any(|w| m.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn local_addresses_skip_the_proxy() {
        for u in [
            "http://localhost:8000/v1",
            "http://127.0.0.1:11434",
            "http://192.168.1.20:8000/v1",
            "http://192.168.1.5/v1",
            "http://172.20.0.3",
            "http://[::1]:8000",
            "http://[fd12::1]/v1",
            "http://gpu-box:8000/v1",
            "http://gpu.lan/v1",
            "http://llm.home.arpa",
            "http://mac-mini.local:1234/v1",
        ] {
            assert!(direct_host(&url(u)), "{u} is local");
        }
        for u in ["https://api.openai.com/v1", "http://8.8.8.8", "https://172.32.0.1", "https://example.com"] {
            assert!(!direct_host(&url(u)), "{u} is not local");
        }
    }

    #[test]
    fn kinds_round_trip() {
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k.id()), Some(k));
        }
        assert_eq!(Kind::parse("soniox"), None);
    }

    #[test]
    fn error_messages_are_short_and_readable() {
        assert_eq!(error_message(r#"{"error":{"message":"Bad key"}}"#), "Bad key");
        assert_eq!(error_message(r#"{"error":"plain"}"#), "plain");
        assert_eq!(error_message("<html>\n<h1>502 Bad Gateway</h1>"), "<html>");
        assert_eq!(error_message(&format!(r#"{{"error":{{"message":"{}"}}}}"#, "x".repeat(300))).chars().count(), 201);
    }

    #[test]
    fn needs_a_key_except_local() {
        let s = AiProvider::default();
        let e = Client::new(Kind::OpenAi, &s, None, None, Duration::from_secs(5)).unwrap_err();
        assert_eq!(e.kind, ErrorKind::NoKey);
        assert!(Client::new(Kind::Local, &s, None, None, Duration::from_secs(5)).is_ok());
        let bad = AiProvider { base_url: "ftp://x".into(), ..s };
        let e = Client::new(Kind::Local, &bad, None, None, Duration::from_secs(5)).unwrap_err();
        assert_eq!(e.kind, ErrorKind::NotSetUp);
    }
}
