//! A tiny HTTP server for tests: it answers each connection with the next
//! canned reply and records what it was sent.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    /// The request target: a path, or a full URL when sent to a proxy.
    pub target: String,
    /// Header names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

pub struct Reply {
    pub status: u16,
    pub body: String,
    pub delay: Duration,
}

pub fn reply(status: u16, body: impl Into<String>) -> Reply {
    Reply { status, body: body.into(), delay: Duration::ZERO }
}

pub struct Mock {
    pub base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Mock {
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

pub async fn serve(replies: Vec<Reply>) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        let mut replies = replies.into_iter();
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            // Read the head, then as much body as Content-Length says.
            let head_end = loop {
                let n = sock.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    break None;
                }
                buf.extend_from_slice(&chunk[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(i + 4);
                }
            };
            let Some(head_end) = head_end else { continue };
            let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
            let mut lines = head.lines();
            let mut first = lines.next().unwrap_or_default().split(' ');
            let method = first.next().unwrap_or_default().to_owned();
            let target = first.next().unwrap_or_default().to_owned();
            let headers: Vec<(String, String)> = lines
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
                .collect();
            let len: usize =
                headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
            while buf.len() < head_end + len {
                let n = sock.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let body = serde_json::from_slice(&buf[head_end..]).unwrap_or(Value::Null);
            log.lock().unwrap().push(Seen { method, target, headers, body });
            let r = replies.next().unwrap_or_else(|| reply(500, r#"{"error":{"message":"no more replies"}}"#));
            tokio::time::sleep(r.delay).await;
            let resp = format!(
                "HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                r.status,
                r.body.len(),
                r.body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
        }
    });
    Mock { base, seen }
}

fn text_request() -> Request {
    Request { system: "Clean it.".into(), user: "um hello".into(), max_tokens: 64, format: Format::Text }
}

fn client(kind: Kind, base: &str, key: Option<&str>, model: &str) -> Client {
    let settings = AiProvider { model: model.into(), base_url: base.into(), effort: Effort::Off };
    // A proxy that doesn't exist: requests to this computer must skip it.
    let proxy = Url::parse("http://127.0.0.1:9").unwrap();
    Client::new(kind, &settings, key.map(str::to_owned), Some(&proxy), Duration::from_secs(5)).unwrap()
}

const CHAT_OK: &str = r#"{"choices":[{"message":{"role":"assistant","content":"Hello."},"finish_reason":"stop"}]}"#;

#[tokio::test]
async fn openai_over_the_wire() {
    let m = serve(vec![reply(200, CHAT_OK)]).await;
    let c = client(Kind::OpenAi, &format!("{}/v1", m.base), Some("sk-test"), "gpt-6-luna");
    assert_eq!(c.complete(&text_request()).await.unwrap(), "Hello.");
    let s = &m.seen()[0];
    assert_eq!((s.method.as_str(), s.target.as_str()), ("POST", "/v1/chat/completions"));
    assert_eq!(s.header("authorization"), Some("Bearer sk-test"));
    assert_eq!(s.header("content-type"), Some("application/json"));
    assert_eq!(s.body["model"], json!("gpt-6-luna"));
    assert_eq!(s.body["messages"][1], json!({"role": "user", "content": "um hello"}));
    assert_eq!(s.body["reasoning_effort"], json!("none"));
}

#[tokio::test]
async fn openrouter_over_the_wire() {
    let m =
        serve(vec![reply(200, CHAT_OK), reply(200, r#"{"error":{"code":429,"message":"Rate limit exceeded"}}"#)]).await;
    let c = client(Kind::OpenRouter, &m.base, Some("or-key"), "openai/gpt-6-luna");
    assert_eq!(c.complete(&text_request()).await.unwrap(), "Hello.");
    let s = &m.seen()[0];
    assert_eq!(s.target, "/chat/completions");
    assert_eq!(s.header("authorization"), Some("Bearer or-key"));
    assert_eq!(s.header("x-title"), Some("Mirza"));
    assert_eq!(s.body["reasoning"], json!({"effort": "none"}));
    let e = c.complete(&text_request()).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::RateLimited);
    assert!(e.to_string().starts_with("OpenRouter is getting too many requests"), "{e}");
}

#[tokio::test]
async fn claude_over_the_wire() {
    let ok = r#"{"content":[{"type":"text","text":"Hello."}],"stop_reason":"end_turn"}"#;
    let bad = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
    let m = serve(vec![reply(200, ok), reply(401, bad)]).await;
    let c = client(Kind::Claude, &m.base, Some("sk-ant-test"), "claude-haiku-5-5");
    assert_eq!(c.complete(&text_request()).await.unwrap(), "Hello.");
    let s = &m.seen()[0];
    assert_eq!(s.target, "/v1/messages");
    assert_eq!(s.header("x-api-key"), Some("sk-ant-test"));
    assert_eq!(s.header("anthropic-version"), Some("2026-10-21"));
    assert!(s.header("authorization").is_none());
    assert_eq!(s.body["system"], json!("Clean it."));
    assert_eq!(s.body["thinking"], json!({"type": "disabled"}));
    let e = c.complete(&text_request()).await.unwrap_err();
    assert_eq!(
        e.to_string(),
        "Claude didn't accept the API key (invalid x-api-key). Check it in Settings > AI revision."
    );
}

#[tokio::test]
async fn gemini_over_the_wire() {
    let ok = r#"{"candidates":[{"content":{"parts":[{"text":"Hello."}]},"finishReason":"STOP"}]}"#;
    let region = r#"{"error":{"code":400,"message":"User location is not supported for the API use.","status":"FAILED_PRECONDITION"}}"#;
    let m = serve(vec![reply(200, ok), reply(400, region)]).await;
    let c = client(Kind::Gemini, &m.base, Some("g-key"), "gemini-3.5-flash-lite");
    assert_eq!(c.complete(&text_request()).await.unwrap(), "Hello.");
    let s = &m.seen()[0];
    assert_eq!(s.target, "/v1beta/models/gemini-3.5-flash-lite:generateContent");
    assert_eq!(s.header("x-goog-api-key"), Some("g-key"));
    assert_eq!(s.body["contents"][0]["parts"][0]["text"], json!("um hello"));
    let e = c.complete(&text_request()).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Region);
    // The proxy is set but skipped for this address, so it wasn't to blame.
    assert_eq!(
        e.to_string(),
        "Gemini isn't offered in your country (User location is not supported for the API use). Set a proxy in Settings > General."
    );
}

#[tokio::test]
async fn local_server_picks_its_model_once() {
    let models = r#"{"object":"list","data":[{"id":"gemma-qat","object":"model"}]}"#;
    let m = serve(vec![reply(200, models), reply(200, CHAT_OK), reply(200, CHAT_OK)]).await;
    let c = client(Kind::Local, &format!("{}/v1", m.base), None, "");
    assert_eq!(c.complete(&text_request()).await.unwrap(), "Hello.");
    assert_eq!(c.complete(&text_request()).await.unwrap(), "Hello.");
    let seen = m.seen();
    assert_eq!(seen.len(), 3, "the model list is asked for once");
    assert_eq!((seen[0].method.as_str(), seen[0].target.as_str()), ("GET", "/v1/models"));
    assert!(seen[1].header("authorization").is_none());
    assert_eq!(seen[1].body["model"], json!("gemma-qat"));
    assert!(seen[1].body.get("reasoning_effort").is_some(), "this test sets effort none");
}

#[tokio::test]
async fn slow_answers_time_out() {
    let m = serve(vec![Reply { status: 200, body: CHAT_OK.into(), delay: Duration::from_secs(3) }]).await;
    let settings = AiProvider { model: "m".into(), base_url: m.base.clone(), effort: Effort::Default };
    let c = Client::new(Kind::Local, &settings, None, None, Duration::from_millis(300)).unwrap();
    let e = c.complete(&text_request()).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Timeout);
}

#[tokio::test]
async fn unreachable_server_is_a_network_error() {
    // Bind and drop a listener to get a port nothing listens on.
    let port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let c = client(Kind::Local, &format!("http://127.0.0.1:{port}/v1"), None, "m");
    let e = c.complete(&text_request()).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Network);
    assert!(e.to_string().contains("Check that the server is running"), "{e}");
}

#[tokio::test]
async fn public_hosts_go_through_the_proxy() {
    // The mock plays an HTTP proxy: the request arrives with the full URL.
    let m = serve(vec![reply(200, CHAT_OK)]).await;
    let proxy = Url::parse(&m.base).unwrap();
    let settings =
        AiProvider { model: "m".into(), base_url: "http://api.example.com/v1".into(), effort: Effort::Default };
    let c = Client::new(Kind::OpenAi, &settings, Some("k".into()), Some(&proxy), Duration::from_secs(5)).unwrap();
    assert_eq!(c.complete(&text_request()).await.unwrap(), "Hello.");
    assert_eq!(m.seen()[0].target, "http://api.example.com/v1/chat/completions");
}

#[tokio::test]
async fn model_lists() {
    let gemini = r#"{"models":[{"name":"models/gemini-3.5-flash-lite","supportedGenerationMethods":["generateContent"]},{"name":"models/embedding-1","supportedGenerationMethods":["embedContent"]}]}"#;
    let claude = r#"{"data":[{"id":"claude-haiku-5-5","type":"model"}],"has_more":false}"#;
    let m = serve(vec![reply(200, gemini), reply(200, claude), reply(401, r#"{"error":{"message":"bad key"}}"#)]).await;
    let g = client(Kind::Gemini, &m.base, Some("g"), "x");
    assert_eq!(g.models().await.unwrap(), ["gemini-3.5-flash-lite"]);
    let c = client(Kind::Claude, &m.base, Some("c"), "x");
    assert_eq!(c.models().await.unwrap(), ["claude-haiku-5-5"]);
    let o = client(Kind::OpenRouter, &m.base, Some("o"), "x");
    assert_eq!(o.models().await.unwrap_err().kind, ErrorKind::KeyRejected);
    let seen = m.seen();
    assert_eq!(seen[0].target, "/v1beta/models?pageSize=200");
    assert_eq!(seen[1].header("anthropic-version"), Some("2026-10-21"));
    assert_eq!(seen[2].header("authorization"), Some("Bearer o"));
}
