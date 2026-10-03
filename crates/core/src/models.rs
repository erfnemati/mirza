//! The models a provider offers for real-time transcription, for the model
//! picker in the settings window. Soniox lists its models with the languages
//! each supports; OpenAI lists model IDs; ElevenLabs has one real-time model.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Language {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    /// The model this ID is another name for, if any.
    pub alias_of: Option<String>,
    pub languages: Vec<Language>,
}

/// Languages to offer when the provider doesn't list its own (the set
/// Soniox supports, which covers the common ones).
pub fn default_languages() -> Vec<Language> {
    serde_json::from_str(include_str!("languages.json")).expect("valid built-in language list")
}

/// Models known without asking the provider, used when there is no key yet
/// or the request fails.
pub fn known(provider: &str) -> Vec<ModelInfo> {
    let m = |id: &str, name: &str| ModelInfo {
        id: id.into(),
        name: name.into(),
        alias_of: None,
        languages: default_languages(),
    };
    match provider {
        "soniox" => vec![m("stt-rt-v5", "Speech-to-Text Real-time v5")],
        "elevenlabs" => vec![m("scribe_v2_realtime", "Scribe v2 Realtime")],
        "openai" => vec![
            m("gpt-live-transcribe", "GPT Live Transcribe"),
            m("gpt-transcribe", "GPT Transcribe"),
            m("gpt-4o-transcribe", "GPT-4o Transcribe"),
            m("gpt-4o-mini-transcribe", "GPT-4o mini Transcribe"),
            m("whisper-1", "Whisper"),
        ],
        _ => Vec::new(),
    }
}

/// Asks the provider for its real-time models.
pub async fn list(provider: &str, key: &str, proxy: Option<&Url>) -> Result<Vec<ModelInfo>, String> {
    let http = crate::usage::http_client(proxy, Duration::from_secs(15))?;
    match provider {
        "soniox" => {
            let v: serde_json::Value = get(&http, "https://api.soniox.com/v1/models", key).await?;
            Ok(soniox_models(&v))
        }
        "openai" => {
            let v: serde_json::Value = get(&http, "https://api.openai.com/v1/models", key).await?;
            let mut ids: Vec<String> = v["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| m["id"].as_str())
                .filter(|id| id.contains("transcribe") || id.starts_with("whisper"))
                .map(str::to_owned)
                .collect();
            ids.sort();
            let known = known("openai");
            Ok(ids
                .into_iter()
                .map(|id| {
                    let name = known.iter().find(|k| k.id == id).map(|k| k.name.clone()).unwrap_or_else(|| id.clone());
                    ModelInfo { id, name, alias_of: None, languages: default_languages() }
                })
                .collect())
        }
        _ => Ok(known(provider)),
    }
}

async fn get(http: &reqwest::Client, url: &str, key: &str) -> Result<serde_json::Value, String> {
    let resp = http.get(url).bearer_auth(key).send().await.map_err(|e| crate::usage::describe(&e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 | 403 => "the API key was not accepted".into(),
            code => format!("the provider answered {code}"),
        });
    }
    resp.json().await.map_err(|e| e.to_string())
}

/// Real-time models from Soniox's list, main ones first, aliases after.
fn soniox_models(v: &serde_json::Value) -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["transcription_mode"].as_str() == Some("real_time"))
        .filter_map(|m| {
            Some(ModelInfo {
                id: m["id"].as_str()?.to_owned(),
                name: m["name"].as_str().unwrap_or_default().to_owned(),
                alias_of: m["aliased_model_id"].as_str().map(str::to_owned),
                languages: serde_json::from_value(m["languages"].clone()).unwrap_or_default(),
            })
        })
        .collect();
    out.sort_by_key(|m| (m.alias_of.is_some(), m.id.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_languages_include_persian_and_english() {
        let langs = default_languages();
        assert!(langs.len() >= 50);
        assert!(langs.iter().any(|l| l.code == "fa" && l.name == "Persian"));
        assert!(langs.iter().any(|l| l.code == "en"));
    }

    #[test]
    fn soniox_list_keeps_real_time_models_main_first() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"models":[
                {"id":"stt-rt-v4","name":"RT v4","aliased_model_id":"stt-rt-v5","transcription_mode":"real_time","languages":[]},
                {"id":"stt-async-v5","name":"Async","aliased_model_id":null,"transcription_mode":"async","languages":[]},
                {"id":"stt-rt-v5","name":"RT v5","aliased_model_id":null,"transcription_mode":"real_time",
                 "languages":[{"code":"fa","name":"Persian"}]}]}"#,
        )
        .unwrap();
        let m = soniox_models(&v);
        let ids: Vec<&str> = m.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["stt-rt-v5", "stt-rt-v4"]);
        assert_eq!(m[0].languages[0].code, "fa");
        assert_eq!(m[1].alias_of.as_deref(), Some("stt-rt-v5"));
    }
}
