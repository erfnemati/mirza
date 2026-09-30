//! Soniox real-time transcription:
//! <https://soniox.com/docs/stt/api-reference/websocket-api>

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use super::{StreamRequest, SttEvent, sample_rate};
use crate::net;

const URL: &str = "wss://stt-rt.soniox.com/transcribe-websocket";

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Response {
    tokens: Vec<Token>,
    finished: bool,
    error_code: Option<serde_json::Value>,
    error_message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Token {
    text: String,
    is_final: bool,
}

pub fn start_message(req: &StreamRequest) -> serde_json::Value {
    let s = &req.settings;
    let mut msg = json!({
        "api_key": req.api_key,
        "model": s.model,
        "audio_format": "pcm_s16le",
        "sample_rate": sample_rate("soniox"),
        "num_channels": 1,
        "enable_endpoint_detection": s.endpoint_detection,
    });
    let langs: Vec<&String> = s.languages.iter().filter(|l| !l.trim().is_empty()).collect();
    if !langs.is_empty() {
        msg["language_hints"] = json!(langs);
        msg["language_hints_strict"] = json!(s.strict_languages);
    }
    let terms: Vec<&String> = s.terms.iter().filter(|t| !t.trim().is_empty()).collect();
    if !terms.is_empty() {
        msg["context"] = json!({ "terms": terms });
    }
    msg
}

pub async fn stream(
    req: &StreamRequest,
    mut audio: mpsc::Receiver<Vec<u8>>,
    events: &mpsc::UnboundedSender<SttEvent>,
) -> Result<(), String> {
    let mut ws =
        net::connect_ws(URL, &[], req.proxy.as_ref()).await.map_err(|e| format!("connecting to Soniox: {e}"))?;
    ws.send(Message::Text(start_message(req).to_string().into()))
        .await
        .map_err(|e| format!("starting the Soniox session: {e}"))?;

    let mut audio_open = true;
    loop {
        tokio::select! {
            chunk = audio.recv(), if audio_open => match chunk {
                Some(pcm) => ws.send(Message::Binary(pcm.into())).await.map_err(|e| format!("sending audio: {e}"))?,
                None => {
                    // An empty text frame ends the stream (an empty binary one is ignored).
                    ws.send(Message::Text("".into())).await.map_err(|e| format!("ending the stream: {e}"))?;
                    audio_open = false;
                }
            },
            msg = ws.next() => {
                let msg = match msg {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => return Err(format!("connection to Soniox lost: {e}")),
                    None => return Err("Soniox closed the connection".into()),
                };
                let text = match msg {
                    Message::Text(t) => t.to_string(),
                    Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                    Message::Close(frame) => {
                        let why = frame.map(|f| format!("{} {}", u16::from(f.code), f.reason)).unwrap_or_default();
                        return Err(format!("Soniox closed the connection {why}").trim().to_owned());
                    }
                    _ => continue,
                };
                if handle(&text, events)? {
                    let _ = ws.close(None).await;
                    return Ok(());
                }
            }
        }
    }
}

/// Turns one response into events. Returns true when the transcript is done.
fn handle(text: &str, events: &mpsc::UnboundedSender<SttEvent>) -> Result<bool, String> {
    let r: Response = serde_json::from_str(text).map_err(|e| format!("bad response from Soniox: {e}"))?;
    let code = r.error_code.as_ref().filter(|c| !c.is_null() && c.as_i64() != Some(0));
    if code.is_some() || r.error_message.as_deref().is_some_and(|m| !m.is_empty()) {
        let code = code.map(|c| c.to_string()).unwrap_or_default();
        return Err(format!("Soniox error {code}: {}", r.error_message.unwrap_or_default()).replace("  ", " "));
    }
    let (mut settled, mut guess, mut heard) = (String::new(), String::new(), false);
    for t in &r.tokens {
        if t.text == "<end>" || t.text == "<fin>" {
            continue;
        }
        heard = true;
        if t.is_final { settled.push_str(&t.text) } else { guess.push_str(&t.text) }
    }
    if heard {
        let _ = events.send(SttEvent::Heard);
    }
    if !settled.is_empty() {
        let _ = events.send(SttEvent::Final(settled));
    }
    if !r.tokens.is_empty() {
        let _ = events.send(SttEvent::Partial(guess));
    }
    Ok(r.finished)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Providers;

    fn drain(rx: &mut mpsc::UnboundedReceiver<SttEvent>) -> Vec<SttEvent> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    #[test]
    fn start_message_has_hints_and_terms() {
        let mut settings = Providers::default().soniox;
        settings.terms = vec!["Mirza".into(), " ".into()];
        let req = StreamRequest { provider: "soniox".into(), api_key: "k".into(), settings, proxy: None };
        let m = start_message(&req);
        assert_eq!(m["model"], "stt-rt-v5");
        assert_eq!(m["sample_rate"], 16000);
        assert_eq!(m["language_hints"], json!(["fa", "en"]));
        assert_eq!(m["language_hints_strict"], false);
        assert_eq!(m["context"], json!({"terms": ["Mirza"]}));
    }

    #[test]
    fn splits_settled_text_from_guesses() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let done = handle(
            r#"{"tokens":[{"text":"سلام","is_final":true},{"text":" دنیا","is_final":false},{"text":"<end>","is_final":true}],"final_audio_proc_ms":10}"#,
            &tx,
        )
        .unwrap();
        assert!(!done);
        assert_eq!(drain(&mut rx), [SttEvent::Heard, SttEvent::Final("سلام".into()), SttEvent::Partial(" دنیا".into())]);
        assert!(handle(r#"{"tokens":[],"finished":true}"#, &tx).unwrap());
        assert_eq!(drain(&mut rx), []);
    }

    #[test]
    fn markers_alone_are_not_speech() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        handle(r#"{"tokens":[{"text":"<end>","is_final":true}]}"#, &tx).unwrap();
        assert_eq!(drain(&mut rx), [SttEvent::Partial(String::new())]);
    }

    #[test]
    fn reports_errors() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let err = handle(r#"{"error_code":401,"error_message":"Invalid API key."}"#, &tx).unwrap_err();
        assert_eq!(err, "Soniox error 401: Invalid API key.");
    }
}
