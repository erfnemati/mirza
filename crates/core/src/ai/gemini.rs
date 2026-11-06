//! Google's generateContent API (Gemini's own format; its OpenAI-compatible
//! endpoint is still in beta).

use serde_json::{Value, json};

use super::error::{ErrorKind, from_answer};
use super::{Effort, Format, Http, Request, budget, shorten};

pub(super) fn build(base: &str, key: Option<&str>, model: &str, effort: Effort, req: &Request) -> Http {
    let mut headers = Vec::new();
    if let Some(k) = key {
        headers.push(("x-goog-api-key", k.to_owned()));
    }
    let model = model.trim_start_matches("models/");
    let mut config = json!({"maxOutputTokens": budget(req.max_tokens, effort)});
    let level = match effort {
        Effort::Default => None,
        // Gemini's lowest level; thinking can't be turned fully off.
        Effort::Off | Effort::Minimal => Some("minimal"),
        e => Some(e.as_str()),
    };
    if let Some(l) = level {
        config["thinkingConfig"] = json!({"thinkingLevel": l});
    }
    if let Format::JsonSchema { schema, .. } = &req.format {
        config["responseMimeType"] = json!("application/json");
        config["responseJsonSchema"] = schema.clone();
    }
    let body = json!({
        "systemInstruction": {"parts": [{"text": req.system}]},
        "contents": [{"role": "user", "parts": [{"text": req.user}]}],
        "generationConfig": config,
    });
    Http { url: format!("{base}/v1beta/models/{model}:generateContent"), headers, body }
}

pub(super) fn parse(status: u16, body: &str) -> Result<String, (ErrorKind, String)> {
    if !(200..300).contains(&status) {
        return Err(quota_error(body).unwrap_or_else(|| from_answer(status, body)));
    }
    let v: Value = serde_json::from_str(body).map_err(|_| (ErrorKind::Unreadable, "not JSON".to_owned()))?;
    if let Some(r) = v["promptFeedback"]["blockReason"].as_str() {
        return Err((ErrorKind::Refused, r.to_lowercase()));
    }
    let cand = &v["candidates"][0];
    match cand["finishReason"].as_str() {
        Some("MAX_TOKENS") => return Err((ErrorKind::CutOff, String::new())),
        Some(r @ ("SAFETY" | "RECITATION" | "PROHIBITED_CONTENT" | "BLOCKLIST" | "SPII")) => {
            return Err((ErrorKind::Refused, r.to_lowercase()));
        }
        _ => {}
    }
    // Thought summaries, when present, are parts marked "thought".
    let text: String = cand["content"]["parts"]
        .as_array()
        .map(|parts| parts.iter().filter(|p| p["thought"] != true).filter_map(|p| p["text"].as_str()).collect())
        .unwrap_or_default();
    if text.trim().is_empty() {
        let why = cand["finishReason"].as_str().map(|r| format!("no text, finish reason {r}"));
        return Err((ErrorKind::Unreadable, shorten(&why.unwrap_or_else(|| "the answer was empty".into()), 120)));
    }
    Ok(text)
}

/// Google's quota errors all start "You exceeded your current quota, please
/// check your plan and billing details", even when the free tier's limit per
/// minute was hit; the details say which limit it was.
fn quota_error(body: &str) -> Option<(ErrorKind, String)> {
    let v: Value = serde_json::from_str(body).ok()?;
    let failure = v["error"]["details"]
        .as_array()?
        .iter()
        .find(|d| d["@type"].as_str().is_some_and(|t| t.ends_with("QuotaFailure")))?;
    let violation = &failure["violations"][0];
    let id = violation["quotaId"].as_str()?;
    let (kind, per) = if id.contains("PerMinute") || id.contains("PerSecond") {
        (ErrorKind::RateLimited, if id.contains("PerSecond") { "a second" } else { "a minute" })
    } else if id.contains("PerDay") {
        (ErrorKind::DailyLimit, "a day")
    } else {
        return None;
    };
    let tier = if id.contains("FreeTier") { "the free tier" } else { "your plan" };
    let what =
        if violation["quotaMetric"].as_str().unwrap_or_default().contains("token") { "tokens" } else { "requests" };
    let detail = match violation["quotaValue"].as_str() {
        Some(n) => format!("{tier} allows {n} {what} {per}"),
        None => format!("{tier}'s limit of {what} {per}"),
    };
    Some((kind, detail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_shape() {
        let req = Request { system: "sys".into(), user: "hi".into(), max_tokens: 200, format: Format::Text };
        let h =
            build("https://generativelanguage.googleapis.com", Some("g"), "gemini-3.5-flash-lite", Effort::Off, &req);
        assert_eq!(
            h.url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.5-flash-lite:generateContent"
        );
        assert_eq!(h.headers, vec![("x-goog-api-key", "g".to_owned())]);
        assert_eq!(
            h.body,
            json!({
                "systemInstruction": {"parts": [{"text": "sys"}]},
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
                "generationConfig": {"maxOutputTokens": 200, "thinkingConfig": {"thinkingLevel": "minimal"}},
            })
        );
        let schema = json!({"type": "object"});
        let req = Request { format: Format::JsonSchema { name: "t".into(), schema: schema.clone() }, ..req };
        let h = build("https://x", None, "models/gemini-3.8-flash", Effort::Default, &req);
        assert_eq!(h.url, "https://x/v1beta/models/gemini-3.8-flash:generateContent");
        let c = &h.body["generationConfig"];
        assert!(c.get("thinkingConfig").is_none());
        assert_eq!(c["responseMimeType"], json!("application/json"));
        assert_eq!(c["responseJsonSchema"], schema);
    }

    #[test]
    fn answers_and_errors() {
        let ok = r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"plan","thought":true},{"text":"Hi."}]},"finishReason":"STOP"}]}"#;
        assert_eq!(parse(200, ok).unwrap(), "Hi.");
        let cut = r#"{"candidates":[{"content":{"parts":[{"text":"H"}]},"finishReason":"MAX_TOKENS"}]}"#;
        assert_eq!(parse(200, cut).unwrap_err().0, ErrorKind::CutOff);
        let blocked = r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#;
        assert_eq!(parse(200, blocked).unwrap_err(), (ErrorKind::Refused, "safety".into()));
        let key = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#;
        assert_eq!(parse(400, key).unwrap_err().0, ErrorKind::KeyRejected);
        let region = r#"{"error":{"code":400,"message":"User location is not supported for the API use.","status":"FAILED_PRECONDITION"}}"#;
        assert_eq!(parse(400, region).unwrap_err().0, ErrorKind::Region);
        let model = r#"{"error":{"code":404,"message":"models/gemini-x is not found for API version v1beta","status":"NOT_FOUND"}}"#;
        assert_eq!(parse(404, model).unwrap_err().0, ErrorKind::ModelNotFound);
        let quota =
            r#"{"error":{"code":429,"message":"You exceeded your current quota","status":"RESOURCE_EXHAUSTED"}}"#;
        assert_eq!(parse(429, quota).unwrap_err().0, ErrorKind::Credit);
    }

    /// The shape of a real answer from November 2026 (message shortened).
    fn quota_answer(id: &str, metric: &str) -> String {
        json!({"error": {
            "code": 429,
            "message": "You exceeded your current quota, please check your plan and billing details.",
            "status": "RESOURCE_EXHAUSTED",
            "details": [
                {"@type": "type.googleapis.com/google.rpc.Help", "links": []},
                {"@type": "type.googleapis.com/google.rpc.QuotaFailure", "violations": [{
                    "quotaMetric": metric,
                    "quotaId": id,
                    "quotaDimensions": {"model": "gemini-3.5-flash-lite", "location": "global"},
                    "quotaValue": "15",
                }]},
                {"@type": "type.googleapis.com/google.rpc.RetryInfo", "retryDelay": "21s"},
            ],
        }})
        .to_string()
    }

    #[test]
    fn free_tier_limits_are_not_billing_problems() {
        let requests = "generativelanguage.googleapis.com/generate_content_free_tier_requests";
        let minute = quota_answer("GenerateRequestsPerMinutePerProjectPerModel-FreeTier", requests);
        assert_eq!(
            parse(429, &minute).unwrap_err(),
            (ErrorKind::RateLimited, "the free tier allows 15 requests a minute".into())
        );
        let day = quota_answer("GenerateRequestsPerDayPerProjectPerModel-FreeTier", requests);
        assert_eq!(
            parse(429, &day).unwrap_err(),
            (ErrorKind::DailyLimit, "the free tier allows 15 requests a day".into())
        );
        let tokens = quota_answer(
            "GenerateContentInputTokensPerModelPerMinute",
            "generativelanguage.googleapis.com/generate_content_paid_tier_input_token_count",
        );
        assert_eq!(
            parse(429, &tokens).unwrap_err(),
            (ErrorKind::RateLimited, "your plan allows 15 tokens a minute".into())
        );
        let other = quota_answer("SomethingElse", requests);
        assert_eq!(parse(429, &other).unwrap_err().0, ErrorKind::Credit);
    }
}
