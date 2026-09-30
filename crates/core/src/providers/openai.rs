//! OpenAI real-time transcription (Realtime API, transcription session):
//! <https://developers.openai.com/api/docs/guides/realtime-transcription>
//!
//! Text only becomes final when the audio buffer is committed, so we commit
//! at each pause in speech (judged from the audio level), every 15 seconds of
//! nonstop speech, and at the end.

use std::collections::HashSet;
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use super::{Joiner, StreamRequest, SttEvent, sample_rate};
use crate::{audio, net};

const URL: &str = "wss://api.openai.com/v1/realtime?intent=transcription";
/// Loudness (see audio::level) below which a chunk counts as a pause.
const QUIET_LEVEL: f32 = 0.3;
/// A pause this long after speech ends a segment.
const PAUSE: Duration = Duration::from_millis(700);
const MAX_SEGMENT: Duration = Duration::from_secs(15);
/// The API rejects commits of less audio than this.
const MIN_COMMIT: Duration = Duration::from_millis(100);
const FINISH_WAIT: Duration = Duration::from_secs(8);

pub fn session_update(req: &StreamRequest) -> Value {
    let s = &req.settings;
    let langs: Vec<&String> = s.languages.iter().filter(|l| !l.trim().is_empty()).collect();
    let terms: Vec<&str> = s.terms.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
    let mut transcription = json!({ "model": s.model });
    if s.model.starts_with("gpt-live") {
        if !langs.is_empty() {
            transcription["languages"] = json!(langs);
        }
        if !terms.is_empty() {
            transcription["keywords"] = json!(terms);
        }
    } else {
        // whisper-1 and gpt-4o-transcribe take one language and a text prompt.
        if langs.len() == 1 {
            transcription["language"] = json!(langs[0]);
        }
        if !terms.is_empty() {
            transcription["prompt"] = json!(terms.join(", "));
        }
    }
    json!({
        "type": "session.update",
        "session": {
            "type": "transcription",
            "audio": {
                "input": {
                    "format": { "type": "audio/pcm", "rate": sample_rate("openai") },
                    "transcription": transcription,
                    "turn_detection": null,
                }
            }
        }
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ServerMsg {
    #[serde(rename = "type")]
    kind: String,
    item_id: String,
    delta: String,
    transcript: String,
    error: Option<Value>,
}

/// Decides when to commit, from the audio sent so far.
#[derive(Default)]
struct Segmenter {
    /// Audio in the uncommitted buffer.
    buffered: Duration,
    /// Quiet audio at the end of the buffer.
    quiet: Duration,
    spoke: bool,
}

impl Segmenter {
    /// Adds a chunk; returns true when the buffer should be committed now.
    fn push(&mut self, chunk_len: Duration, level: f32) -> bool {
        self.buffered += chunk_len;
        if level < QUIET_LEVEL {
            self.quiet += chunk_len;
        } else {
            self.quiet = Duration::ZERO;
            self.spoke = true;
        }
        (self.spoke && self.quiet >= PAUSE) || self.buffered >= MAX_SEGMENT
    }

    fn committed(&mut self) {
        *self = Self::default();
    }
}

pub async fn stream(
    req: &StreamRequest,
    mut audio: mpsc::Receiver<Vec<u8>>,
    events: &mpsc::UnboundedSender<SttEvent>,
) -> Result<(), String> {
    let auth = format!("Bearer {}", req.api_key);
    let mut ws = net::connect_ws(URL, &[("Authorization", &auth)], req.proxy.as_ref())
        .await
        .map_err(|e| format!("connecting to OpenAI: {e}"))?;
    ws.send(Message::Text(session_update(req).to_string().into()))
        .await
        .map_err(|e| format!("starting the OpenAI session: {e}"))?;

    let rate = sample_rate("openai") as u64;
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut seg = Segmenter::default();
    let mut joiner = Joiner::default();
    let mut waiting: HashSet<String> = HashSet::new(); // committed items without a transcript yet
    let mut commits_in_flight = 0usize; // commits not yet acknowledged
    let mut audio_open = true;
    let finish = tokio::time::sleep(Duration::from_secs(3600));
    tokio::pin!(finish);

    loop {
        tokio::select! {
            chunk = audio.recv(), if audio_open => {
                let commit = match chunk {
                    Some(pcm) => {
                        let len = Duration::from_millis(pcm.len() as u64 / 2 * 1000 / rate);
                        let level = audio::level(&pcm);
                        let msg = json!({"type": "input_audio_buffer.append", "audio": b64.encode(&pcm)});
                        ws.send(Message::Text(msg.to_string().into())).await.map_err(|e| format!("sending audio: {e}"))?;
                        seg.push(len, level)
                    }
                    None => {
                        audio_open = false;
                        finish.as_mut().reset(tokio::time::Instant::now() + FINISH_WAIT);
                        seg.spoke
                    }
                };
                if commit && seg.buffered >= MIN_COMMIT && seg.spoke {
                    ws.send(Message::Text(json!({"type": "input_audio_buffer.commit"}).to_string().into()))
                        .await
                        .map_err(|e| format!("sending audio: {e}"))?;
                    commits_in_flight += 1;
                    seg.committed();
                } else if commit {
                    // Only silence so far: drop it rather than transcribe it.
                    ws.send(Message::Text(json!({"type": "input_audio_buffer.clear"}).to_string().into()))
                        .await
                        .map_err(|e| format!("sending audio: {e}"))?;
                    seg.committed();
                }
                if !audio_open && commits_in_flight == 0 && waiting.is_empty() {
                    let _ = ws.close(None).await;
                    return Ok(());
                }
            }
            _ = &mut finish, if !audio_open => {
                let _ = ws.close(None).await;
                return Ok(());
            }
            msg = ws.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t.to_string(),
                    Some(Ok(Message::Close(f))) => {
                        let why = f.map(|f| f.reason.to_string()).unwrap_or_default();
                        return Err(format!("OpenAI closed the connection {why}").trim().to_owned());
                    }
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(format!("connection to OpenAI lost: {e}")),
                    None => return Err("OpenAI closed the connection".into()),
                };
                let m: ServerMsg = serde_json::from_str(&text).map_err(|e| format!("bad message from OpenAI: {e}"))?;
                match m.kind.as_str() {
                    "input_audio_buffer.committed" => {
                        commits_in_flight = commits_in_flight.saturating_sub(1);
                        waiting.insert(m.item_id);
                    }
                    "conversation.item.input_audio_transcription.delta" => {
                        if !m.delta.trim().is_empty() {
                            let _ = events.send(SttEvent::Heard);
                        }
                        let _ = events.send(SttEvent::Partial(m.delta));
                    }
                    "conversation.item.input_audio_transcription.completed" => {
                        waiting.remove(&m.item_id);
                        if let Some(t) = joiner.next(&m.transcript) {
                            let _ = events.send(SttEvent::Heard);
                            let _ = events.send(SttEvent::Final(t));
                        }
                        let _ = events.send(SttEvent::Partial(String::new()));
                    }
                    "conversation.item.input_audio_transcription.failed" => {
                        waiting.remove(&m.item_id);
                        tracing::warn!("OpenAI could not transcribe a segment: {text}");
                    }
                    "error" => {
                        let e = m.error.unwrap_or_default();
                        let code = e["code"].as_str().unwrap_or_default();
                        if code == "input_audio_buffer_commit_empty" {
                            commits_in_flight = commits_in_flight.saturating_sub(1);
                        } else {
                            let msg = e["message"].as_str().unwrap_or("unknown error");
                            return Err(format!("OpenAI: {msg}"));
                        }
                    }
                    _ => {}
                }
                if !audio_open && commits_in_flight == 0 && waiting.is_empty() {
                    let _ = ws.close(None).await;
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Providers;

    #[test]
    fn session_update_for_live_and_older_models() {
        let mut settings = Providers::default().openai;
        settings.terms = vec!["Mirza".into()];
        let req = StreamRequest { provider: "openai".into(), api_key: "k".into(), settings, proxy: None };
        let m = session_update(&req);
        let input = &m["session"]["audio"]["input"];
        assert_eq!(input["format"]["rate"], 24000);
        assert_eq!(input["transcription"]["model"], "gpt-live-transcribe");
        assert_eq!(input["transcription"]["languages"], json!(["fa", "en"]));
        assert_eq!(input["transcription"]["keywords"], json!(["Mirza"]));
        assert!(input["turn_detection"].is_null());

        let mut older = req.settings.clone();
        older.model = "whisper-1".into();
        older.languages = vec!["fa".into()];
        let m = session_update(&StreamRequest { settings: older, ..req });
        let t = &m["session"]["audio"]["input"]["transcription"];
        assert_eq!(t["language"], "fa");
        assert_eq!(t["prompt"], "Mirza");
    }

    #[test]
    fn commits_at_pauses_after_speech() {
        let ms = Duration::from_millis;
        let mut s = Segmenter::default();
        assert!(!s.push(ms(1000), 0.0), "silence alone is not a segment");
        assert!(!s.push(ms(50), 0.8));
        assert!(!s.push(ms(650), 0.1));
        assert!(s.push(ms(50), 0.1), "700 ms pause after speech");
        s.committed();
        let mut long = false;
        for _ in 0..300 {
            long |= s.push(ms(50), 0.9);
        }
        assert!(long, "nonstop speech is cut every 15 s");
    }
}
