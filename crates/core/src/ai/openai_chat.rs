//! OpenAI's Chat Completions format, which OpenAI, OpenRouter and local
//! servers (vLLM, llama.cpp, Ollama, LM Studio) all speak.

use serde_json::{Value, json};

use super::error::{ErrorKind, from_answer, from_status};
use super::{Effort, Format, Http, Kind, Request, budget, shorten};

pub(super) fn build(kind: Kind, base: &str, key: Option<&str>, model: &str, effort: Effort, req: &Request) -> Http {
    let mut headers = Vec::new();
    if let Some(k) = key {
        headers.push(("authorization", format!("Bearer {k}")));
    }
    if kind == Kind::OpenRouter {
        // Shows Mirza as the app on the user's OpenRouter activity page.
        headers.push(("http-referer", "https://github.com/erfnemati/mirza".into()));
        headers.push(("x-title", "Mirza".into()));
    }
    let mut body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": req.system},
            {"role": "user", "content": req.user},
        ],
    });
    let max = budget(req.max_tokens, effort);
    match kind {
        // OpenAI's reasoning models refuse the older max_tokens.
        Kind::OpenAi => body["max_completion_tokens"] = json!(max),
        _ => body["max_tokens"] = json!(max),
    }
    if effort != Effort::Default {
        match kind {
            Kind::OpenRouter => body["reasoning"] = json!({"effort": effort.as_str()}),
            _ => body["reasoning_effort"] = json!(effort.as_str()),
        }
    }
    if kind == Kind::Local {
        // Cloud reasoning models reject a temperature; local models follow
        // instructions more closely with a low one.
        body["temperature"] = json!(0.2);
    }
    if let Format::JsonSchema { name, schema } = &req.format {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": {"name": name, "strict": true, "schema": schema},
        });
        if kind == Kind::OpenRouter {
            // Only route to providers that can follow the schema.
            body["provider"] = json!({"require_parameters": true});
        }
    }
    Http { url: format!("{base}/chat/completions"), headers, body }
}

pub(super) fn parse(_kind: Kind, status: u16, body: &str) -> Result<String, (ErrorKind, String)> {
    if !(200..300).contains(&status) {
        return Err(from_answer(status, body));
    }
    let v: Value = serde_json::from_str(body).map_err(|_| unreadable(body))?;
    // OpenRouter reports some failures inside a 200 answer.
    if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
        let msg = shorten(e["message"].as_str().unwrap_or("unknown error"), 200);
        let code = e["code"].as_u64().and_then(|c| u16::try_from(c).ok()).unwrap_or(500);
        return Err((from_status(code, &msg), msg));
    }
    let choice = &v["choices"][0];
    if choice.is_null() {
        return Err(unreadable(body));
    }
    let msg = &choice["message"];
    if let Some(r) = msg["refusal"].as_str().filter(|r| !r.trim().is_empty()) {
        return Err((ErrorKind::Refused, shorten(r, 200)));
    }
    match choice["finish_reason"].as_str() {
        Some("length") => return Err((ErrorKind::CutOff, String::new())),
        Some("content_filter") => return Err((ErrorKind::Refused, "content filter".into())),
        _ => {}
    }
    let text = match &msg["content"] {
        Value::String(s) => s.clone(),
        // A few servers send a list of parts.
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect(),
        _ => String::new(),
    };
    if text.trim().is_empty() {
        return Err((ErrorKind::Unreadable, "the answer was empty".into()));
    }
    Ok(text)
}

/// An error from a non-chat call (listing models).
pub(super) fn error_kind(_kind: Kind, status: u16, body: &str) -> (ErrorKind, String) {
    let (kind, detail) = from_answer(status, body);
    // A 404 from /models means a wrong address, not a missing model.
    if status == 404 {
        (ErrorKind::NotSetUp, format!("nothing answers at this address ({detail})"))
    } else {
        (kind, detail)
    }
}

fn unreadable(body: &str) -> (ErrorKind, String) {
    let first = body.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("empty");
    (ErrorKind::Unreadable, shorten(first, 120))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(format: Format) -> Request {
        Request { system: "sys".into(), user: "hi".into(), max_tokens: 100, format }
    }

    #[test]
    fn openai_request_shape() {
        let h = build(
            Kind::OpenAi,
            "https://api.openai.com/v1",
            Some("sk-1"),
            "gpt-6-luna",
            Effort::Off,
            &req(Format::Text),
        );
        assert_eq!(h.url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(h.headers, vec![("authorization", "Bearer sk-1".to_owned())]);
        assert_eq!(
            h.body,
            json!({
                "model": "gpt-6-luna",
                "messages": [{"role": "system", "content": "sys"}, {"role": "user", "content": "hi"}],
                "max_completion_tokens": 100,
                "reasoning_effort": "none",
            })
        );
    }

    #[test]
    fn openrouter_and_local_differences() {
        let schema = json!({"type": "object"});
        let f = Format::JsonSchema { name: "task".into(), schema: schema.clone() };
        let h = build(Kind::OpenRouter, "https://openrouter.ai/api/v1", Some("k"), "m", Effort::Low, &req(f.clone()));
        assert!(h.headers.iter().any(|(k, v)| *k == "x-title" && v == "Mirza"));
        assert_eq!(h.body["reasoning"], json!({"effort": "low"}));
        assert_eq!(h.body["max_tokens"], json!(100 + 8192));
        assert_eq!(h.body["response_format"]["json_schema"]["schema"], schema);
        assert_eq!(h.body["provider"]["require_parameters"], json!(true));
        assert!(h.body.get("reasoning_effort").is_none());

        let h = build(Kind::Local, "http://10.0.0.2:8000/v1", None, "gemma", Effort::Default, &req(f));
        assert!(h.headers.is_empty());
        assert!(h.body.get("reasoning_effort").is_none());
        assert!(h.body.get("provider").is_none());
        assert_eq!(h.body["max_tokens"], json!(100));
        assert_eq!(h.body["response_format"]["type"], json!("json_schema"));
    }

    #[test]
    fn answers_and_errors() {
        let ok = r#"{"choices":[{"message":{"role":"assistant","content":"Hello."},"finish_reason":"stop"}]}"#;
        assert_eq!(parse(Kind::OpenAi, 200, ok).unwrap(), "Hello.");
        let cut = r#"{"choices":[{"message":{"content":"Hel"},"finish_reason":"length"}]}"#;
        assert_eq!(parse(Kind::OpenAi, 200, cut).unwrap_err().0, ErrorKind::CutOff);
        let refused = r#"{"choices":[{"message":{"content":null,"refusal":"I can't help"},"finish_reason":"stop"}]}"#;
        assert_eq!(parse(Kind::OpenAi, 200, refused).unwrap_err().0, ErrorKind::Refused);
        let empty = r#"{"choices":[{"message":{"content":""},"finish_reason":"stop"}]}"#;
        assert_eq!(parse(Kind::Local, 200, empty).unwrap_err().0, ErrorKind::Unreadable);
        let in_200 = r#"{"error":{"code":402,"message":"Insufficient credits"}}"#;
        assert_eq!(
            parse(Kind::OpenRouter, 200, in_200).unwrap_err(),
            (ErrorKind::Credit, "Insufficient credits".into())
        );
        let bad = r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error"}}"#;
        assert_eq!(
            parse(Kind::OpenAi, 401, bad).unwrap_err(),
            (ErrorKind::KeyRejected, "Incorrect API key provided".into())
        );
        assert_eq!(parse(Kind::OpenAi, 502, "<html>bad gateway</html>").unwrap_err().0, ErrorKind::ServerError);
        assert_eq!(parse(Kind::Local, 200, "not json").unwrap_err().0, ErrorKind::Unreadable);
        assert_eq!(error_kind(Kind::Local, 404, "").0, ErrorKind::NotSetUp);
    }
}
