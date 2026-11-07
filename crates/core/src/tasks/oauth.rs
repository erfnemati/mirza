//! Signing in to Google. The browser opens Google's page; Google sends the
//! browser back to a small server on this computer (127.0.0.1, a random
//! port) with a code, and Mirza trades the code for a refresh token. PKCE
//! ties the code to this attempt, so a code caught by anyone else is useless.
//! Where the browser can't reach this computer (a server over SSH), the
//! address the browser ended at can be pasted instead.

use std::fmt;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

use super::google::{self, Api, CALENDAR_SCOPE, Endpoints, ErrorKind, GoogleError, TASKS_SCOPE};
use super::store::random;

/// A started sign-in: open `url` in a browser, then [`Attempt::finish`].
pub struct Attempt {
    pub url: String,
    redirect: String,
    state: String,
    verifier: String,
    listener: TcpListener,
}

/// What a finished sign-in gives.
#[derive(Clone, PartialEq, Eq)]
pub struct Granted {
    pub refresh_token: String,
    /// The scopes allowed, separated by spaces.
    pub scopes: String,
}

impl fmt::Debug for Granted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Granted").field("scopes", &self.scopes).finish_non_exhaustive()
    }
}

impl Granted {
    pub fn tasks(&self) -> bool {
        self.scopes.split_whitespace().any(|s| s == TASKS_SCOPE)
    }

    pub fn calendar(&self) -> bool {
        self.scopes.split_whitespace().any(|s| s == CALENDAR_SCOPE || s.ends_with("/auth/calendar.events"))
    }
}

/// The PKCE pair: a random verifier and its SHA-256, both base64url.
fn pkce() -> (String, String) {
    let verifier = URL_SAFE_NO_PAD.encode(random::<32>());
    let digest = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    (verifier.clone(), URL_SAFE_NO_PAD.encode(digest.as_ref()))
}

/// Google's sign-in page for these settings.
fn auth_url(ep: &Endpoints, client_id: &str, redirect: &str, state: &str, challenge: &str) -> String {
    let mut u =
        Url::parse(&ep.auth).unwrap_or_else(|_| Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap());
    u.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", &format!("{TASKS_SCOPE} {CALENDAR_SCOPE}"))
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        // A refresh token every time, also when signing in again.
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("include_granted_scopes", "true");
    u.into()
}

/// Starts a sign-in: listens on a free port of 127.0.0.1.
pub async fn start(ep: &Endpoints, client_id: &str) -> std::io::Result<Attempt> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let state = URL_SAFE_NO_PAD.encode(random::<16>());
    let (verifier, challenge) = pkce();
    let url = auth_url(ep, client_id, &redirect, &state, &challenge);
    Ok(Attempt { url, redirect, state, verifier, listener })
}

/// Reads the code from the address Google sent the browser to. Ok(None) for
/// anything else the browser asks for (like the page icon).
pub fn code_from(address: &str, state: &str) -> Result<Option<String>, String> {
    let address = address.trim();
    let full = if address.starts_with('/') { format!("http://127.0.0.1{address}") } else { address.to_owned() };
    let Ok(u) = Url::parse(&full) else { return Err("that isn't the address the browser ended at".into()) };
    let get = |k: &str| u.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned());
    if let Some(e) = get("error") {
        return Err(match e.as_str() {
            "access_denied" => "the sign-in was cancelled in the browser".into(),
            e => format!("Google said {e}"),
        });
    }
    let Some(code) = get("code") else { return Ok(None) };
    if get("state").as_deref() != Some(state) {
        return Err("the answer belongs to another sign-in attempt; try again".into());
    }
    Ok(Some(code))
}

const PAGE_OK: &str = "Mirza is signed in to Google. You can close this tab.";
const PAGE_OK_FA: &str = "میرزا به حساب گوگل وصل شد. می‌توانید این صفحه را ببندید.";

fn page(ok: bool, message: &str) -> String {
    let (title, fa) = if ok {
        (PAGE_OK, PAGE_OK_FA)
    } else {
        ("Mirza couldn't sign in to Google.", "میرزا نتوانست به حساب گوگل وصل شود.")
    };
    let detail = if ok { String::new() } else { format!("<p>{}</p>", html_escape(message)) };
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Mirza</title>\
         <body style=\"font:16px system-ui,sans-serif;max-width:32em;margin:4em auto;padding:0 1em\">\
         <p>{title}</p>{detail}<p dir=rtl lang=fa>{fa}</p></body>"
    );
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Answers one browser request. Some(result) once the sign-in is decided.
async fn answer(mut sock: TcpStream, state: &str) -> Option<Result<String, String>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
        match tokio::time::timeout(Duration::from_secs(5), sock.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&chunk[..n]),
            _ => break,
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let target = head.lines().next().and_then(|l| l.split(' ').nth(1)).unwrap_or("/");
    let result = code_from(target, state);
    let reply = match &result {
        Ok(Some(_)) => page(true, ""),
        Ok(None) => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned(),
        Err(e) => page(false, e),
    };
    let _ = sock.write_all(reply.as_bytes()).await;
    let _ = sock.shutdown().await;
    result.transpose()
}

impl Attempt {
    /// Waits for the browser to come back, or for `pasted` (the address the
    /// browser ended at, from wherever the user pasted it), up to `timeout`.
    pub async fn code(
        &self,
        mut pasted: tokio::sync::mpsc::Receiver<String>,
        timeout: Duration,
    ) -> Result<String, String> {
        let wait = async {
            let mut paste_open = true;
            loop {
                tokio::select! {
                    conn = self.listener.accept() => {
                        let Ok((sock, _)) = conn else { continue };
                        if let Some(r) = answer(sock, &self.state).await {
                            return r;
                        }
                    }
                    line = pasted.recv(), if paste_open => match line {
                        Some(l) => match code_from(&l, &self.state) {
                            Ok(Some(code)) => return Ok(code),
                            Ok(None) => return Err("that address has no sign-in code in it".into()),
                            Err(e) => return Err(e),
                        },
                        None => paste_open = false,
                    },
                }
            }
        };
        tokio::time::timeout(timeout, wait).await.unwrap_or_else(|_| Err("the sign-in took too long; try again".into()))
    }

    /// Trades the code for a refresh token.
    pub async fn exchange(
        &self,
        http: &reqwest::Client,
        ep: &Endpoints,
        client_id: &str,
        client_secret: Option<&str>,
        code: &str,
    ) -> Result<Granted, GoogleError> {
        let mut form = vec![
            ("client_id", client_id),
            ("code", code),
            ("code_verifier", self.verifier.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", self.redirect.as_str()),
        ];
        if let Some(s) = client_secret {
            form.push(("client_secret", s));
        }
        let (status, body) = google::post_form(http, &ep.token, &form).await.map_err(|e| {
            let kind = if e.is_timeout() { ErrorKind::Timeout } else { ErrorKind::Network };
            GoogleError::new(kind, Api::SignIn, crate::usage::describe(&e))
        })?;
        if status != 200 {
            let mut e = google::from_answer(Api::SignIn, status, &body);
            if body.contains("client_secret is missing") || body.contains("invalid_client") {
                e.detail = "the client ID or secret is wrong or missing; paste the JSON file from Google again".into();
            }
            return Err(e);
        }
        let v: serde_json::Value = serde_json::from_str(&body)
            .map_err(|_| GoogleError::new(ErrorKind::Unreadable, Api::SignIn, "not JSON"))?;
        let Some(rt) = v["refresh_token"].as_str() else {
            return Err(GoogleError::new(ErrorKind::Unreadable, Api::SignIn, "Google sent no refresh token"));
        };
        Ok(Granted { refresh_token: rt.to_owned(), scopes: v["scope"].as_str().unwrap_or_default().to_owned() })
    }
}

/// Tells Google to end a sign-in. Best effort: the token is forgotten here
/// either way.
pub async fn revoke(http: &reqwest::Client, ep: &Endpoints, token: &str) -> Result<(), GoogleError> {
    let (status, body) = google::post_form(http, &ep.revoke, &[("token", token)])
        .await
        .map_err(|e| GoogleError::new(ErrorKind::Network, Api::SignIn, crate::usage::describe(&e)))?;
    match status {
        200 => Ok(()),
        // Already ended.
        400 if body.contains("invalid_token") => Ok(()),
        _ => Err(google::from_answer(Api::SignIn, status, &body)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::mock::{reply, serve};

    #[test]
    fn auth_url_asks_for_a_refresh_token_and_both_scopes() {
        let ep = Endpoints::default();
        let u = Url::parse(&auth_url(&ep, "cid", "http://127.0.0.1:5555", "st", "ch")).unwrap();
        let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(u.host_str(), Some("accounts.google.com"));
        assert_eq!(q["scope"], format!("{TASKS_SCOPE} {CALENDAR_SCOPE}"));
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["prompt"], "consent");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:5555");
    }

    #[test]
    fn pkce_challenge_is_the_verifiers_hash() {
        let (v, c) = pkce();
        assert_eq!(v.len(), 43);
        let d = ring::digest::digest(&ring::digest::SHA256, v.as_bytes());
        assert_eq!(c, URL_SAFE_NO_PAD.encode(d.as_ref()));
        assert_ne!(pkce().0, v);
    }

    #[test]
    fn reads_the_code_or_says_why_not() {
        assert_eq!(code_from("/?state=s&code=4/abc&scope=x", "s"), Ok(Some("4/abc".into())));
        assert_eq!(code_from("http://127.0.0.1:4000/?code=c%2F1&state=s", "s"), Ok(Some("c/1".into())));
        assert_eq!(code_from("/favicon.ico", "s"), Ok(None));
        assert!(code_from("/?code=c&state=other", "s").is_err());
        assert_eq!(
            code_from("/?error=access_denied&state=s", "s"),
            Err("the sign-in was cancelled in the browser".into())
        );
        assert!(code_from("hello", "s").is_err());
    }

    #[tokio::test]
    async fn browser_comes_back_and_the_code_is_traded() {
        let mock = serve(vec![reply(
            200,
            serde_json::json!({"access_token": "at", "refresh_token": "rt", "scope": format!("{TASKS_SCOPE} {CALENDAR_SCOPE}")})
                .to_string(),
        )])
        .await;
        let ep = Endpoints::at(&mock.base);
        let a = start(&ep, "cid").await.unwrap();
        let redirect = a.redirect.clone();
        let state = a.state.clone();
        let browser = tokio::spawn(async move {
            let http = reqwest::Client::new();
            // The page icon first, as browsers do.
            let _ = http.get(format!("{redirect}/favicon.ico")).send().await;
            let page =
                http.get(format!("{redirect}/?state={state}&code=xyz")).send().await.unwrap().text().await.unwrap();
            assert!(page.contains(PAGE_OK), "{page}");
        });
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        let code = a.code(rx, Duration::from_secs(10)).await.unwrap();
        browser.await.unwrap();
        assert_eq!(code, "xyz");
        let http = crate::usage::http_client(None, Duration::from_secs(5)).unwrap();
        let g = a.exchange(&http, &ep, "cid", Some("sec"), &code).await.unwrap();
        assert!(g.tasks() && g.calendar());
        assert_eq!(g.refresh_token, "rt");
        let form = mock.seen()[0].body.as_str().unwrap().to_owned();
        assert!(form.contains(&format!("code_verifier={}", a.verifier)), "{form}");
        assert!(form.contains("client_secret=sec") && form.contains("grant_type=authorization_code"));
        assert!(!format!("{g:?}").contains("rt"), "the token never shows in logs");
    }

    #[tokio::test]
    async fn a_pasted_address_works_too() {
        let a = start(&Endpoints::default(), "cid").await.unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tx.send(format!("{}/?code=pasted&state={}", a.redirect, a.state)).await.unwrap();
        assert_eq!(a.code(rx, Duration::from_secs(5)).await.unwrap(), "pasted");
    }
}
