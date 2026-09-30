//! ElevenLabs Scribe real-time transcription:
//! <https://elevenlabs.io/docs/api-reference/speech-to-text/v-1-speech-to-text-realtime>
//!
//! Audio goes up as base64 chunks; the service commits a segment at each pause
//! (VAD) and sends it as a committed transcript. At the end we commit the rest
//! by hand and wait for its transcript.

use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use super::{Joiner, StreamRequest, SttEvent, sample_rate};
use crate::net;

const URL: &str = "wss://api.elevenlabs.io/v1/speech-to-text/realtime";
/// After the final commit, stop waiting once nothing has arrived for this long.
const QUIET_AFTER_COMMIT: Duration = Duration::from_secs(4);

pub fn url(req: &StreamRequest) -> String {
    let s = &req.settings;
    let rate = sample_rate("elevenlabs");
    let mut url = url::Url::parse(URL).expect("static URL");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("model_id", &s.model);
        q.append_pair("audio_format", &format!("pcm_{rate}"));
        q.append_pair("commit_strategy", "vad");
        let langs: Vec<&String> = s.languages.iter().filter(|l| !l.trim().is_empty()).collect();
        // One language pins recognition to it; with several, let it detect.
        if langs.len() == 1 || (s.strict_languages && !langs.is_empty()) {
            q.append_pair("language_code", langs[0]);
        }
        for t in s.terms.iter().filter(|t| !t.trim().is_empty()) {
            q.append_pair("keyterms", t.trim());
        }
    }
    url.into()
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ServerMsg {
    message_type: String,
    text: String,
    error: Option<String>,
    message: Option<String>,
}

pub async fn stream(
    req: &StreamRequest,
    mut audio: mpsc::Receiver<Vec<u8>>,
    events: &mpsc::UnboundedSender<SttEvent>,
) -> Result<(), String> {
    let mut ws = net::connect_ws(&url(req), &[("xi-api-key", &req.api_key)], req.proxy.as_ref())
        .await
        .map_err(|e| format!("connecting to ElevenLabs: {e}"))?;
    let rate = sample_rate("elevenlabs");
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut joiner = Joiner::default();
    let mut audio_open = true;
    let quiet = tokio::time::sleep(Duration::from_secs(3600));
    tokio::pin!(quiet);

    loop {
        tokio::select! {
            chunk = audio.recv(), if audio_open => {
                let (data, commit) = match chunk {
                    Some(pcm) => (b64.encode(pcm), false),
                    None => {
                        audio_open = false;
                        quiet.as_mut().reset(tokio::time::Instant::now() + QUIET_AFTER_COMMIT);
                        (String::new(), true)
                    }
                };
                let msg = json!({"message_type": "input_audio_chunk", "audio_base_64": data, "commit": commit, "sample_rate": rate});
                ws.send(Message::Text(msg.to_string().into())).await.map_err(|e| format!("sending audio: {e}"))?;
            }
            _ = &mut quiet, if !audio_open => {
                let _ = ws.close(None).await;
                return Ok(());
            }
            msg = ws.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t.to_string(),
                    Some(Ok(Message::Close(f))) => {
                        if !audio_open {
                            return Ok(()); // closed after our final commit
                        }
                        let why = f.map(|f| f.reason.to_string()).unwrap_or_default();
                        return Err(format!("ElevenLabs closed the connection {why}").trim().to_owned());
                    }
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(format!("connection to ElevenLabs lost: {e}")),
                    None if !audio_open => return Ok(()),
                    None => return Err("ElevenLabs closed the connection".into()),
                };
                let committed = handle(&text, &mut joiner, events)?;
                if !audio_open {
                    if committed {
                        // The rest arrived; give a moment for anything after it.
                        quiet.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(500));
                    } else {
                        quiet.as_mut().reset(tokio::time::Instant::now() + QUIET_AFTER_COMMIT);
                    }
                }
            }
        }
    }
}

/// Turns one server message into events. Returns true for a committed segment.
fn handle(text: &str, joiner: &mut Joiner, events: &mpsc::UnboundedSender<SttEvent>) -> Result<bool, String> {
    let m: ServerMsg = serde_json::from_str(text).map_err(|e| format!("bad message from ElevenLabs: {e}"))?;
    match m.message_type.as_str() {
        "partial_transcript" => {
            if !m.text.trim().is_empty() {
                let _ = events.send(SttEvent::Heard);
            }
            let _ = events.send(SttEvent::Partial(m.text));
            Ok(false)
        }
        "committed_transcript" | "committed_transcript_with_timestamps" => {
            if let Some(t) = joiner.next(&m.text) {
                let _ = events.send(SttEvent::Heard);
                let _ = events.send(SttEvent::Final(t));
            }
            let _ = events.send(SttEvent::Partial(String::new()));
            Ok(true)
        }
        "session_started" | "warning" | "committed_transcript_entities" | "edited_transcript" => Ok(false),
        // Nothing to commit: fine at the end of a silent session.
        "insufficient_audio_activity" | "commit_throttled" => Ok(false),
        t if t.contains("error")
            || matches!(
                t,
                "quota_exceeded"
                    | "unaccepted_terms"
                    | "rate_limited"
                    | "queue_overflow"
                    | "resource_exhausted"
                    | "session_time_limit_exceeded"
                    | "invalid_request"
                    | "chunk_size_exceeded"
            ) =>
        {
            let detail = m.error.or(m.message).unwrap_or_default();
            Err(format!("ElevenLabs: {t} {detail}").trim().to_owned())
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Providers;

    fn drain(rx: &mut mpsc::UnboundedReceiver<SttEvent>) -> Vec<SttEvent> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    #[test]
    fn url_has_model_format_and_terms() {
        let mut settings = Providers::default().elevenlabs;
        settings.terms = vec!["Mirza".into(), "Soniox".into()];
        let req = StreamRequest { provider: "elevenlabs".into(), api_key: "k".into(), settings, proxy: None };
        let u = url(&req);
        assert!(u.contains("model_id=scribe_v2_realtime"), "{u}");
        assert!(u.contains("audio_format=pcm_16000"));
        assert!(u.contains("keyterms=Mirza&keyterms=Soniox"));
        assert!(!u.contains("language_code"), "fa and en: let it detect");
        let mut one = req.settings.clone();
        one.languages = vec!["fa".into()];
        let u = url(&StreamRequest { settings: one, ..req });
        assert!(u.contains("language_code=fa"));
    }

    #[test]
    fn committed_segments_are_joined_with_spaces() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut j = Joiner::default();
        handle(r#"{"message_type":"partial_transcript","text":"سلام"}"#, &mut j, &tx).unwrap();
        assert!(handle(r#"{"message_type":"committed_transcript","text":"سلام دنیا."}"#, &mut j, &tx).unwrap());
        handle(r#"{"message_type":"committed_transcript","text":"Hello."}"#, &mut j, &tx).unwrap();
        let finals: Vec<String> = drain(&mut rx)
            .into_iter()
            .filter_map(|e| if let SttEvent::Final(t) = e { Some(t) } else { None })
            .collect();
        assert_eq!(finals, ["سلام دنیا.", " Hello."]);
    }

    #[test]
    fn errors_end_the_stream() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let err = handle(r#"{"message_type":"auth_error","error":"invalid key"}"#, &mut Joiner::default(), &tx);
        assert_eq!(err.unwrap_err(), "ElevenLabs: auth_error invalid key");
        assert!(handle(r#"{"message_type":"insufficient_audio_activity"}"#, &mut Joiner::default(), &tx).is_ok());
    }
}
