//! Anthropic's Messages API.

use serde_json::{Value, json};

use super::error::{ErrorKind, from_answer};
use super::{Effort, Format, Http, Request, budget, shorten};

/// The API version these requests and answers follow.
const VERSION: &str = "2026-10-21";

pub(super) fn build(base: &str, key: Option<&str>, model: &str, effort: Effort, req: &Request) -> Http {
    let mut headers = vec![("anthropic-version", VERSION.to_owned())];
    if let Some(k) = key {
        headers.push(("x-api-key", k.to_owned()));
    }
    // No temperature: newer models reject sampling settings.
    let mut body = json!({
        "model": model,
        "max_tokens": budget(req.max_tokens, effort),
        "system": req.system,
        "messages": [{"role": "user", "content": req.user}],
    });
    let mut output = serde_json::Map::new();
    match effort {
        Effort::Default => {}
        // Thinking is on by default; turning it off is allowed at effort
        // high or lower. Claude's lowest effort is "low".
        Effort::Off | Effort::Minimal => {
            body["thinking"] = json!({"type": "disabled"});
            output.insert("effort".into(), json!("low"));
        }
        e => {
            output.insert("effort".into(), json!(e.as_str()));
        }
    }
    if let Format::JsonSchema { schema, .. } = &req.format {
        output.insert("format".into(), json!({"type": "json_schema", "schema": schema}));
    }
    if !output.is_empty() {
        body["output_config"] = Value::Object(output);
    }
    Http { url: format!("{base}/v1/messages"), headers, body }
}

pub(super) fn parse(status: u16, body: &str) -> Result<String, (ErrorKind, String)> {
    if !(200..300).contains(&status) {
        return Err(from_answer(status, body));
    }
    let v: Value = serde_json::from_str(body).map_err(|_| (ErrorKind::Unreadable, "not JSON".to_owned()))?;
    match v["stop_reason"].as_str() {
        Some("refusal") => return Err((ErrorKind::Refused, String::new())),
        Some("max_tokens" | "model_context_window_exceeded") => return Err((ErrorKind::CutOff, String::new())),
        _ => {}
    }
    // Read blocks by type: thinking blocks may come first.
    let text: String = v["content"]
        .as_array()
        .map(|blocks| blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect())
        .unwrap_or_default();
    if text.trim().is_empty() {
        let why = v["stop_reason"].as_str().map(|r| format!("no text, stop reason {r}"));
        return Err((ErrorKind::Unreadable, shorten(&why.unwrap_or_else(|| "the answer was empty".into()), 120)));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_shape() {
        let req = Request { system: "sys".into(), user: "hi".into(), max_tokens: 300, format: Format::Text };
        let h = build("https://api.anthropic.com", Some("sk-ant"), "claude-haiku-5-5", Effort::Off, &req);
        assert_eq!(h.url, "https://api.anthropic.com/v1/messages");
        assert_eq!(h.headers, vec![("anthropic-version", "2026-10-21".to_owned()), ("x-api-key", "sk-ant".to_owned())]);
        assert_eq!(
            h.body,
            json!({
                "model": "claude-haiku-5-5",
                "max_tokens": 300,
                "system": "sys",
                "messages": [{"role": "user", "content": "hi"}],
                "thinking": {"type": "disabled"},
                "output_config": {"effort": "low"},
            })
        );
        assert!(h.body.get("temperature").is_none());

        let schema = json!({"type": "object"});
        let req = Request { format: Format::JsonSchema { name: "t".into(), schema: schema.clone() }, ..req };
        let h = build("https://api.anthropic.com", Some("k"), "m", Effort::Medium, &req);
        assert!(h.body.get("thinking").is_none());
        assert_eq!(
            h.body["output_config"],
            json!({"effort": "medium", "format": {"type": "json_schema", "schema": schema}})
        );
        assert_eq!(h.body["max_tokens"], json!(300 + 8192));
        let h = build(
            "https://api.anthropic.com",
            Some("k"),
            "m",
            Effort::Default,
            &Request { format: Format::Text, ..req },
        );
        assert!(h.body.get("output_config").is_none());
    }

    #[test]
    fn answers_and_errors() {
        let ok =
            r#"{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"Hi."}],"stop_reason":"end_turn"}"#;
        assert_eq!(parse(200, ok).unwrap(), "Hi.");
        assert_eq!(parse(200, r#"{"content":[],"stop_reason":"refusal"}"#).unwrap_err().0, ErrorKind::Refused);
        let cut = r#"{"content":[{"type":"text","text":"Hi"}],"stop_reason":"max_tokens"}"#;
        assert_eq!(parse(200, cut).unwrap_err().0, ErrorKind::CutOff);
        let err = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        assert_eq!(parse(401, err).unwrap_err(), (ErrorKind::KeyRejected, "invalid x-api-key".into()));
        let over = r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        assert_eq!(parse(529, over).unwrap_err().0, ErrorKind::ServerError);
        let model = r#"{"type":"error","error":{"type":"not_found_error","message":"model: claude-x"}}"#;
        assert_eq!(parse(404, model).unwrap_err().0, ErrorKind::ModelNotFound);
    }
}
