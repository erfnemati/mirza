//! Whether a newer version is out, from the project's latest GitHub release.
//! The request carries nothing about the user.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

/// Where releases are published.
pub const REPO_URL: &str = env!("CARGO_PKG_REPOSITORY");

/// This build's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// e.g. "0.1.4"
    pub version: String,
    /// The release's download page.
    pub url: String,
}

/// The latest published release (drafts and pre-releases don't count).
pub async fn latest(proxy: Option<&Url>) -> Result<Release, String> {
    let api =
        format!("{}/releases/latest", REPO_URL.replacen("https://github.com/", "https://api.github.com/repos/", 1));
    let http = crate::usage::http_client(proxy, Duration::from_secs(20))?;
    let resp = http
        .get(&api)
        .header("User-Agent", concat!("mirza/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("checking for updates: {}", crate::usage::describe(&e)))?;
    if !resp.status().is_success() {
        return Err(format!("checking for updates: GitHub answered {}", resp.status()));
    }
    let v: serde_json::Value =
        resp.json().await.map_err(|e| format!("checking for updates: {}", crate::usage::describe(&e)))?;
    parse(&v).ok_or_else(|| "checking for updates: unexpected answer from GitHub".into())
}

fn parse(v: &serde_json::Value) -> Option<Release> {
    let version = v["tag_name"].as_str()?.trim().trim_start_matches('v').to_owned();
    // Only ever open a page of this project.
    let url = v["html_url"]
        .as_str()
        .filter(|u| u.starts_with(&format!("{REPO_URL}/")))
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{REPO_URL}/releases/latest"));
    (!version.is_empty()).then_some(Release { version, url })
}

/// Whether version `a` is newer than `b`, e.g. "0.1.10" is newer than "0.1.9".
pub fn is_newer(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> Option<Vec<u64>> {
        let main = v.trim().trim_start_matches('v').split(['-', '+']).next()?;
        main.split('.').map(|p| p.parse().ok()).collect()
    }
    matches!((parts(a), parts(b)), (Some(a), Some(b)) if a > b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.1.4", "0.1.3"));
        assert!(is_newer("v0.1.10", "0.1.9"));
        assert!(is_newer("0.2", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9-rc1"));
        assert!(!is_newer("0.1.3", "0.1.3"));
        assert!(!is_newer("0.1.2", "0.1.3"));
        assert!(!is_newer("nightly", "0.1.3"));
    }

    #[test]
    fn reads_release() {
        let v = serde_json::json!({
            "tag_name": "v0.1.4",
            "html_url": format!("{REPO_URL}/releases/tag/v0.1.4"),
        });
        let r = parse(&v).unwrap();
        assert_eq!(r.version, "0.1.4");
        assert!(r.url.ends_with("/releases/tag/v0.1.4"));
        let elsewhere = serde_json::json!({ "tag_name": "v0.1.4", "html_url": "https://example.com/x" });
        assert_eq!(parse(&elsewhere).unwrap().url, format!("{REPO_URL}/releases/latest"));
        assert!(parse(&serde_json::json!({ "message": "Not Found" })).is_none());
    }
}
