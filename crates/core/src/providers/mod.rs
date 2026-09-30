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
use url::Url;

use crate::config::Provider;

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
