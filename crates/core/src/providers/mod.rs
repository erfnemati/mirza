//! Speech-to-text providers. Each one streams microphone audio over a
//! WebSocket and reports recognized text as [`SttEvent`]s.
//!
//! A stream is a task: it reads raw s16le mono audio from a channel, and when
//! that channel closes it asks the provider for the rest of the transcript
//! and ends with [`SttEvent::Finished`]. Aborting the task cancels the stream.

mod elevenlabs;
mod openai;
mod soniox;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Error as WsError;
use url::Url;

use crate::config::Provider;
use crate::net::NetError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SttEvent {
    /// Newly settled text, to be typed.
    Final(String),
    /// The provider's current guess for the words after the settled text. It
    /// replaces the previous guess.
    Partial(String),
    /// Speech was recognized (settled or not). Resets the silence timer.
    Heard,
    /// The transcript is complete.
    Finished,
    Error(String),
}

pub struct StreamRequest {
    pub provider: String,
    pub api_key: String,
    pub settings: Provider,
    pub proxy: Option<Url>,
}

/// The sample rate a provider wants its audio at.
pub fn sample_rate(provider: &str) -> u32 {
    match provider {
        "openai" => 24_000,
        _ => 16_000,
    }
}

/// Starts streaming `audio` to the provider. Audio sent before the connection
/// is up waits in the channel.
pub fn start(
    req: StreamRequest,
    audio: mpsc::Receiver<Vec<u8>>,
    events: mpsc::UnboundedSender<SttEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let result = match req.provider.as_str() {
            "soniox" => soniox::stream(&req, audio, &events).await,
            "elevenlabs" => elevenlabs::stream(&req, audio, &events).await,
            "openai" => openai::stream(&req, audio, &events).await,
            other => Err(format!("{} is not supported yet", crate::config::provider_name(other))),
        };
        match result {
            Ok(()) => {
                let _ = events.send(SttEvent::Finished);
            }
            Err(e) => {
                let _ = events.send(SttEvent::Error(e));
            }
        }
    })
}

/// Why connecting to `service` failed, in plain words, with what usually helps.
pub(crate) fn connect_error(service: &str, e: &NetError, proxy: Option<&Url>) -> String {
    match e {
        NetError::Ws(WsError::Http(resp)) if matches!(resp.status().as_u16(), 401 | 403) => {
            format!("{service} didn't accept the API key")
        }
        NetError::Ws(WsError::Http(resp)) => format!("{service} answered {}", resp.status()),
        NetError::BadProxy(_) => e.to_string(),
        NetError::Ws(w) => network_error(&format!("couldn't connect to {service}"), &ws_reason(w), service, proxy),
        other => network_error(&format!("couldn't connect to {service}"), &other.to_string(), service, proxy),
    }
}

/// The connection to `service` broke while streaming.
pub(crate) fn lost(service: &str, e: &WsError, proxy: Option<&Url>) -> String {
    network_error(&format!("the connection to {service} was cut"), &ws_reason(e), service, proxy)
}

/// The connection to `service` went quiet: a blocked network often lets the
/// connection open and then stops passing data, rather than refusing it.
pub(crate) fn stalled(service: &str, proxy: Option<&Url>) -> String {
    network_error(&format!("{service} isn't answering"), "the connection is stuck", service, proxy)
}

/// A network failure, and the hint that usually helps: the service is often
/// blocked where there's no proxy, or the proxy isn't running.
fn network_error(what: &str, reason: &str, service: &str, proxy: Option<&Url>) -> String {
    let hint = match proxy {
        Some(_) => "Check that your proxy is working.".to_owned(),
        None => format!("If {service} is blocked where you are, set a proxy in Settings > General."),
    };
    format!("{what}: {}. {hint}", reason.trim_end_matches('.'))
}

/// A WebSocket error in plain words: for network errors, just the system's
/// message.
fn ws_reason(e: &WsError) -> String {
    match e {
        WsError::Io(io) => io.to_string(),
        WsError::ConnectionClosed | WsError::AlreadyClosed => "the connection was closed".into(),
        other => other.to_string(),
    }
}

/// Joins transcript segments that arrive as separate sentences, putting a
/// space between them.
#[derive(Default)]
pub(crate) struct Joiner {
    started: bool,
}

impl Joiner {
    /// The text to type for the next segment, or None if it is empty.
    pub fn next(&mut self, text: &str) -> Option<String> {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        let out = if self.started { format!(" {t}") } else { t.to_owned() };
        self.started = true;
        Some(out)
    }
}
