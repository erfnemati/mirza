//! Google Tasks and Google Calendar over REST, through the proxy setting.
//! Errors say in plain words what happened and what usually helps.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use url::Url;

use super::store::TaskRef;
use super::{Draft, List};

/// Where the requests go; tests point these at a mock server.
#[derive(Debug, Clone, PartialEq)]
pub struct Endpoints {
    pub auth: String,
    pub token: String,
    pub revoke: String,
    pub tasks: String,
    pub calendar: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            revoke: "https://oauth2.googleapis.com/revoke".into(),
            tasks: "https://tasks.googleapis.com/tasks/v1".into(),
            calendar: "https://www.googleapis.com/calendar/v3".into(),
        }
    }
}

impl Endpoints {
    /// Every endpoint on one test server.
    pub fn at(base: &str) -> Self {
        let b = base.trim_end_matches('/');
        Self {
            auth: format!("{b}/auth"),
            token: format!("{b}/token"),
            revoke: format!("{b}/revoke"),
            tasks: format!("{b}/tasks/v1"),
            calendar: format!("{b}/calendar/v3"),
        }
    }
}

pub const TASKS_SCOPE: &str = "https://www.googleapis.com/auth/tasks";
pub const CALENDAR_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events.owned";

/// A client ID, its secret (desktop clients need it), and a refresh token.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub refresh_token: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials").field("client_id", &self.client_id).finish_non_exhaustive()
    }
}

/// Gives access tokens from elsewhere, e.g. Android's own Google sign-in.
pub trait TokenProvider: Send + Sync {
    /// A valid access token; `fresh` asks for a new one after Google refused
    /// the last.
    fn access_token(&self, fresh: bool) -> std::result::Result<String, String>;
}

#[derive(Clone)]
pub enum TokenSource {
    Refresh(Credentials),
    External(Arc<dyn TokenProvider>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// No sign-in, or Google ended it.
    SignedOut,
    /// The sign-in doesn't include this (a box left unticked).
    NoAccess,
    /// The API isn't turned on in the Google Cloud project.
    ApiOff,
    RateLimited,
    ServerError,
    Timeout,
    Network,
    NotFound,
    /// Already there (an event id that exists).
    Conflict,
    BadRequest,
    Unreadable,
}

/// Which part of Google a call went to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    SignIn,
    Tasks,
    Calendar,
}

impl Api {
    fn name(self) -> &'static str {
        match self {
            Api::SignIn => "Google sign-in",
            Api::Tasks => "Google Tasks",
            Api::Calendar => "Google Calendar",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GoogleError {
    pub kind: ErrorKind,
    pub api: Api,
    /// Google's own message or a short reason; may be empty.
    pub detail: String,
    pub proxied: bool,
}

impl GoogleError {
    pub fn new(kind: ErrorKind, api: Api, detail: impl Into<String>) -> Self {
        Self { kind, api, detail: detail.into(), proxied: false }
    }

    /// Worth trying again later by itself.
    pub fn retryable(&self) -> bool {
        matches!(self.kind, ErrorKind::RateLimited | ErrorKind::ServerError | ErrorKind::Timeout | ErrorKind::Network)
    }
}

impl std::error::Error for GoogleError {}

impl fmt::Display for GoogleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.api.name();
        let detail = match self.detail.trim().trim_end_matches('.') {
            "" => String::new(),
            d => format!(" ({})", crate::ai::shorten(d, 200)),
        };
        let signin = "Sign in again in Settings > Voice tasks";
        match self.kind {
            ErrorKind::SignedOut => write!(f, "Mirza isn't signed in to Google{detail}. {signin}."),
            ErrorKind::NoAccess => write!(
                f,
                "{name} refused: the sign-in doesn't allow this{detail}. {signin} and tick both boxes (Tasks and Calendar)."
            ),
            ErrorKind::ApiOff => write!(
                f,
                "The {} API is turned off in your Google Cloud project{detail}. Turn it on in the API Library (see the voice tasks guide).",
                name.trim_start_matches("Google ")
            ),
            ErrorKind::RateLimited => write!(f, "{name} is getting too many requests{detail}. Mirza tries again soon."),
            ErrorKind::ServerError => write!(f, "{name} had a problem on its side{detail}. Mirza tries again soon."),
            ErrorKind::Timeout => write!(f, "{name} took too long to answer. Mirza tries again soon."),
            ErrorKind::Network => {
                let hint = if self.proxied {
                    "Check that your proxy is working."
                } else {
                    "If Google is blocked where you are, set a proxy in Settings > General."
                };
                write!(f, "Couldn't reach {name}{detail}. {hint} Mirza tries again soon.")
            }
            ErrorKind::NotFound => write!(f, "{name} doesn't have it any more{detail}."),
            ErrorKind::Conflict => write!(f, "{name} already has it{detail}."),
            ErrorKind::BadRequest => write!(f, "{name} rejected the request{detail}."),
            ErrorKind::Unreadable => write!(f, "{name} sent an answer Mirza couldn't read{detail}."),
        }
    }
}

/// Reads Google's error answer.
pub(crate) fn from_answer(api: Api, status: u16, body: &str) -> GoogleError {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let message = v["error"]["message"]
        .as_str()
        .or_else(|| v["error_description"].as_str())
        .or_else(|| v["error"].as_str())
        .unwrap_or("")
        .to_owned();
    let has = |s: &str| body.contains(s);
    let kind = match status {
        400 if has("invalid_grant") => ErrorKind::SignedOut,
        401 if api == Api::SignIn || has("invalid_client") => ErrorKind::SignedOut,
        401 => ErrorKind::SignedOut,
        403 if has("accessNotConfigured") || has("SERVICE_DISABLED") => ErrorKind::ApiOff,
        403 if has("rateLimitExceeded") || has("RATE_LIMIT_EXCEEDED") || has("quotaExceeded") => ErrorKind::RateLimited,
        403 => ErrorKind::NoAccess,
        404 | 410 => ErrorKind::NotFound,
        409 => ErrorKind::Conflict,
        429 => ErrorKind::RateLimited,
        500..=599 => ErrorKind::ServerError,
        _ => ErrorKind::BadRequest,
    };
    GoogleError::new(kind, api, message)
}

/// Talks to Google Tasks and Calendar for one account.
pub struct Google {
    http: reqwest::Client,
    ep: Endpoints,
    source: TokenSource,
    proxied: bool,
    token: tokio::sync::Mutex<Option<(String, Instant)>>,
    /// Scopes the last refresh said were granted; empty when unknown.
    scopes: std::sync::Mutex<String>,
}

type Result<T> = std::result::Result<T, GoogleError>;

impl Google {
    pub fn new(source: TokenSource, proxy: Option<&Url>, ep: Endpoints, timeout: Duration) -> Result<Self> {
        let http = crate::usage::http_client(proxy, timeout)
            .map_err(|e| GoogleError::new(ErrorKind::Network, Api::SignIn, e))?;
        Ok(Self {
            http,
            ep,
            source,
            proxied: proxy.is_some(),
            token: tokio::sync::Mutex::new(None),
            scopes: std::sync::Mutex::new(String::new()),
        })
    }

    fn err(&self, mut e: GoogleError) -> GoogleError {
        e.proxied = self.proxied;
        e
    }

    fn request_error(&self, api: Api, e: &reqwest::Error) -> GoogleError {
        let kind = if e.is_timeout() { ErrorKind::Timeout } else { ErrorKind::Network };
        self.err(GoogleError::new(kind, api, crate::usage::describe(e)))
    }

    /// Whether the sign-in allows Calendar events. True when unknown (tokens
    /// from elsewhere): a refusal then says so.
    pub fn calendar_allowed(&self) -> bool {
        let s = self.scopes.lock().unwrap_or_else(|e| e.into_inner());
        s.is_empty() || s.split_whitespace().any(|x| x == CALENDAR_SCOPE || x.ends_with("/auth/calendar.events"))
    }

    async fn access_token(&self, fresh: bool) -> Result<String> {
        let mut cached = self.token.lock().await;
        if !fresh
            && let Some((t, until)) = cached.as_ref()
            && Instant::now() < *until
        {
            return Ok(t.clone());
        }
        let creds = match &self.source {
            TokenSource::External(p) => {
                let p = p.clone();
                let t = tokio::task::spawn_blocking(move || p.access_token(fresh))
                    .await
                    .map_err(|e| GoogleError::new(ErrorKind::SignedOut, Api::SignIn, e.to_string()))?
                    .map_err(|e| self.err(GoogleError::new(ErrorKind::SignedOut, Api::SignIn, e)))?;
                return Ok(t);
            }
            TokenSource::Refresh(c) => c,
        };
        let mut form = vec![
            ("client_id", creds.client_id.as_str()),
            ("refresh_token", creds.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];
        if let Some(s) = creds.client_secret.as_deref() {
            form.push(("client_secret", s));
        }
        let (status, body) =
            post_form(&self.http, &self.ep.token, &form).await.map_err(|e| self.request_error(Api::SignIn, &e))?;
        if status != 200 {
            return Err(self.err(from_answer(Api::SignIn, status, &body)));
        }
        let v: Value = serde_json::from_str(&body)
            .map_err(|_| self.err(GoogleError::new(ErrorKind::Unreadable, Api::SignIn, "not JSON")))?;
        let token = v["access_token"]
            .as_str()
            .ok_or_else(|| self.err(GoogleError::new(ErrorKind::Unreadable, Api::SignIn, "no access token")))?
            .to_owned();
        let secs = v["expires_in"].as_u64().unwrap_or(3600);
        if let Some(scope) = v["scope"].as_str() {
            *self.scopes.lock().unwrap_or_else(|e| e.into_inner()) = scope.to_owned();
        }
        *cached = Some((token.clone(), Instant::now() + Duration::from_secs(secs.saturating_sub(120))));
        Ok(token)
    }

    /// One call with the access token; a refused token is renewed once.
    async fn call(&self, api: Api, method: reqwest::Method, url: &str, body: Option<&Value>) -> Result<Value> {
        let mut fresh = false;
        loop {
            let token = self.access_token(fresh).await?;
            let mut rb = self.http.request(method.clone(), url).bearer_auth(&token);
            if let Some(b) = body {
                rb = rb.json(b);
            } else if method == reqwest::Method::POST {
                // Google answers a POST without a length (a move) with 411.
                rb = rb.header(reqwest::header::CONTENT_LENGTH, 0);
            }
            let resp = rb.send().await.map_err(|e| self.request_error(api, &e))?;
            let status = resp.status().as_u16();
            let text = resp.text().await.map_err(|e| self.request_error(api, &e))?;
            if status == 401 && !fresh {
                fresh = true;
                continue;
            }
            if !(200..300).contains(&status) {
                return Err(self.err(from_answer(api, status, &text)));
            }
            if text.trim().is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_str(&text)
                .map_err(|_| self.err(GoogleError::new(ErrorKind::Unreadable, api, "not JSON")));
        }
    }

    // ---- Tasks ----

    fn list_url(&self, list: &str) -> String {
        format!("{}/lists/{}/tasks", self.ep.tasks, seg(if list.is_empty() { "@default" } else { list }))
    }

    /// The account's task lists, in Google's order.
    pub async fn lists(&self) -> Result<Vec<List>> {
        let mut out = Vec::new();
        let mut page = String::new();
        for _ in 0..20 {
            let mut url = format!("{}/users/@me/lists?maxResults=100", self.ep.tasks);
            if !page.is_empty() {
                url.push_str(&format!("&pageToken={}", seg(&page)));
            }
            let v = self.call(Api::Tasks, reqwest::Method::GET, &url, None).await?;
            for item in v["items"].as_array().into_iter().flatten() {
                if let (Some(id), Some(title)) = (item["id"].as_str(), item["title"].as_str()) {
                    out.push(List { id: id.into(), title: title.into() });
                }
            }
            match v["nextPageToken"].as_str() {
                Some(p) if !p.is_empty() => page = p.to_owned(),
                _ => break,
            }
        }
        Ok(out)
    }

    fn task_body(title: &str, note: &str, due: Option<jiff::civil::Date>) -> Value {
        json!({
            "title": title,
            "notes": note,
            "due": due.map(super::when::task_due),
        })
    }

    pub async fn insert_task(&self, list: &str, d: &Draft, due: Option<jiff::civil::Date>) -> Result<TaskRef> {
        let body = Self::task_body(&d.title, &d.note, due);
        let v = self.call(Api::Tasks, reqwest::Method::POST, &self.list_url(list), Some(&body)).await?;
        let id = v["id"]
            .as_str()
            .ok_or_else(|| self.err(GoogleError::new(ErrorKind::Unreadable, Api::Tasks, "no task id")))?;
        Ok(TaskRef { list: list.to_owned(), id: id.to_owned() })
    }

    pub async fn patch_task(&self, t: &TaskRef, d: &Draft, due: Option<jiff::civil::Date>) -> Result<()> {
        let url = format!("{}/{}", self.list_url(&t.list), seg(&t.id));
        self.call(Api::Tasks, reqwest::Method::PATCH, &url, Some(&Self::task_body(&d.title, &d.note, due))).await?;
        Ok(())
    }

    /// Moves a task to another list; its id may change.
    pub async fn move_task(&self, t: &TaskRef, to: &str) -> Result<TaskRef> {
        let to_q = if to.is_empty() { "@default" } else { to };
        let url = format!("{}/{}/move?destinationTasklist={}", self.list_url(&t.list), seg(&t.id), seg(to_q));
        let v = self.call(Api::Tasks, reqwest::Method::POST, &url, None).await?;
        Ok(TaskRef { list: to.to_owned(), id: v["id"].as_str().unwrap_or(&t.id).to_owned() })
    }

    /// Deletes a task; one that's already gone is fine.
    pub async fn delete_task(&self, t: &TaskRef) -> Result<()> {
        let url = format!("{}/{}", self.list_url(&t.list), seg(&t.id));
        match self.call(Api::Tasks, reqwest::Method::DELETE, &url, None).await {
            Err(e) if e.kind == ErrorKind::NotFound => Ok(()),
            r => r.map(|_| ()),
        }
    }

    /// A task with this title and note changed since `since_ms`: one sent
    /// before an unclear failure may have arrived.
    pub async fn find_task(&self, list: &str, d: &Draft, since_ms: i64) -> Result<Option<TaskRef>> {
        let since = jiff::Timestamp::from_millisecond(since_ms - 60_000).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
        let url = format!(
            "{}?updatedMin={}&showHidden=true&showCompleted=true&maxResults=100",
            self.list_url(list),
            seg(&since.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        );
        let v = self.call(Api::Tasks, reqwest::Method::GET, &url, None).await?;
        Ok(v["items"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|i| i["title"].as_str() == Some(&d.title) && i["notes"].as_str().unwrap_or("") == d.note)
            .and_then(|i| i["id"].as_str())
            .map(|id| TaskRef { list: list.to_owned(), id: id.to_owned() }))
    }

    // ---- Calendar ----

    fn event_body(d: &Draft, minutes: u32, tz: &jiff::tz::TimeZone) -> Option<Value> {
        let (date, time) = (d.date?, d.time?);
        let (start, end) = super::when::event_times(date, time, i64::from(minutes), tz)?;
        Some(json!({
            "summary": d.title,
            "description": d.note,
            "start": start,
            "end": end,
            "reminders": {"useDefault": true},
        }))
    }

    /// Adds an event with our own id, so sending it twice can't make two;
    /// an id Calendar already has counts as saved.
    pub async fn insert_event(
        &self,
        id: &str,
        entry_id: &str,
        d: &Draft,
        minutes: u32,
        tz: &jiff::tz::TimeZone,
    ) -> Result<()> {
        let Some(mut body) = Self::event_body(d, minutes, tz) else { return Ok(()) };
        body["id"] = id.into();
        body["extendedProperties"] = json!({"private": {"mirza": entry_id}});
        let url = format!("{}/calendars/primary/events", self.ep.calendar);
        match self.call(Api::Calendar, reqwest::Method::POST, &url, Some(&body)).await {
            Err(e) if e.kind == ErrorKind::Conflict => Ok(()),
            r => r.map(|_| ()),
        }
    }

    pub async fn patch_event(&self, id: &str, d: &Draft, minutes: u32, tz: &jiff::tz::TimeZone) -> Result<()> {
        let Some(body) = Self::event_body(d, minutes, tz) else { return Ok(()) };
        let url = format!("{}/calendars/primary/events/{}", self.ep.calendar, seg(id));
        self.call(Api::Calendar, reqwest::Method::PATCH, &url, Some(&body)).await.map(|_| ())
    }

    /// Deletes an event; one that's already gone is fine.
    pub async fn delete_event(&self, id: &str) -> Result<()> {
        let url = format!("{}/calendars/primary/events/{}?sendUpdates=none", self.ep.calendar, seg(id));
        match self.call(Api::Calendar, reqwest::Method::DELETE, &url, None).await {
            Err(e) if e.kind == ErrorKind::NotFound => Ok(()),
            r => r.map(|_| ()),
        }
    }
}

/// Posts a form (`application/x-www-form-urlencoded`), as Google's token
/// endpoints want, and returns the status and body.
pub(crate) async fn post_form(
    http: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
) -> std::result::Result<(u16, String), reqwest::Error> {
    let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
    let resp = http.post(url).header("content-type", "application/x-www-form-urlencoded").body(body).send().await?;
    let status = resp.status().as_u16();
    Ok((status, resp.text().await?))
}

/// Percent-encodes one path segment or query value. `@` stays, as in
/// Google's own `@default` and `@me`.
fn seg(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>().replace('+', "%20").replace("%40", "@")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::mock::{reply, serve};
    use jiff::civil::{date, time};

    fn creds() -> TokenSource {
        TokenSource::Refresh(Credentials {
            client_id: "cid".into(),
            client_secret: Some("sec".into()),
            refresh_token: "rt".into(),
        })
    }

    fn token(scope: &str) -> crate::ai::mock::Reply {
        reply(
            200,
            json!({"access_token": "at1", "expires_in": 3599, "scope": scope, "token_type": "Bearer"}).to_string(),
        )
    }

    fn draft() -> Draft {
        Draft {
            title: "Call Sara".into(),
            note: "about the contract".into(),
            list: "L1".into(),
            date: Some(date(2026, 11, 7)),
            time: Some(time(15, 0, 0, 0)),
            minutes: 0,
            guesses: vec![],
            raw: false,
        }
    }

    async fn google(replies: Vec<crate::ai::mock::Reply>) -> (Google, crate::ai::mock::Mock) {
        let mock = serve(replies).await;
        let g = Google::new(creds(), None, Endpoints::at(&mock.base), Duration::from_secs(5)).unwrap();
        (g, mock)
    }

    #[tokio::test]
    async fn refreshes_once_and_reads_lists_page_by_page() {
        let (g, mock) = google(vec![
            token(&format!("{TASKS_SCOPE} {CALENDAR_SCOPE}")),
            reply(200, json!({"items": [{"id": "L1", "title": "Personal"}], "nextPageToken": "p 2"}).to_string()),
            reply(200, json!({"items": [{"id": "L2", "title": "Acme"}]}).to_string()),
        ])
        .await;
        let lists = g.lists().await.unwrap();
        assert_eq!(lists.iter().map(|l| l.title.as_str()).collect::<Vec<_>>(), ["Personal", "Acme"]);
        assert!(g.calendar_allowed());
        let seen = mock.seen();
        assert_eq!(seen[0].target, "/token");
        let form = seen[0].body.as_str().unwrap_or_default().to_owned();
        assert!(form.contains("client_secret=sec") && form.contains("grant_type=refresh_token"), "{form}");
        assert_eq!(seen[1].header("authorization"), Some("Bearer at1"));
        assert_eq!(seen[2].target, "/tasks/v1/users/@me/lists?maxResults=100&pageToken=p%202");
    }

    #[tokio::test]
    async fn a_refused_token_is_renewed_once() {
        let (g, mock) = google(vec![
            token(TASKS_SCOPE),
            reply(401, "{}"),
            token(TASKS_SCOPE),
            reply(200, json!({"id": "T1"}).to_string()),
        ])
        .await;
        let t = g.insert_task("L1", &draft(), Some(date(2026, 11, 7))).await.unwrap();
        assert_eq!(t, TaskRef { list: "L1".into(), id: "T1".into() });
        assert!(!g.calendar_allowed());
        let seen = mock.seen();
        assert_eq!(seen.len(), 4);
        assert_eq!(seen[3].target, "/tasks/v1/lists/L1/tasks");
        assert_eq!(seen[3].body["due"], "2026-11-07T00:00:00.000Z");
        assert_eq!(seen[3].body["notes"], "about the contract");
    }

    #[tokio::test]
    async fn sign_in_problems_in_plain_words() {
        let (g, _m) = google(vec![reply(
            400,
            r#"{"error": "invalid_grant", "error_description": "Token has been expired or revoked."}"#,
        )])
        .await;
        let e = g.lists().await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::SignedOut);
        assert!(
            e.to_string().starts_with("Mirza isn't signed in to Google (Token has been expired or revoked)"),
            "{e}"
        );
        assert!(!e.retryable());

        let off = r#"{"error": {"code": 403, "message": "Google Tasks API has not been used in project 1 before or it is disabled.", "errors": [{"reason": "accessNotConfigured"}]}}"#;
        let (g, _m) = google(vec![token(TASKS_SCOPE), reply(403, off)]).await;
        let e = g.lists().await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::ApiOff);
        assert!(e.to_string().starts_with("The Tasks API is turned off"), "{e}");

        let scope = r#"{"error": {"code": 403, "message": "Request had insufficient authentication scopes.", "status": "PERMISSION_DENIED", "details": [{"reason": "ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#;
        let (g, _m) = google(vec![token(TASKS_SCOPE), reply(403, scope)]).await;
        let e = g.delete_event("x").await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::NoAccess);

        let (g, _m) = google(vec![token(TASKS_SCOPE), reply(503, "{}")]).await;
        assert!(g.lists().await.unwrap_err().retryable());
    }

    #[tokio::test]
    async fn events_have_our_id_and_a_repeat_is_fine() {
        let (g, mock) = google(vec![
            token(""),
            reply(409, r#"{"error": {"code": 409, "message": "The requested identifier already exists."}}"#),
            reply(410, "{}"),
        ])
        .await;
        let tz = jiff::tz::TimeZone::get("Europe/Berlin").unwrap();
        g.insert_event("miabcg0", "abc", &draft(), 45, &tz).await.unwrap();
        g.delete_event("miabcg0").await.unwrap();
        let seen = mock.seen();
        let b = &seen[1].body;
        assert_eq!(seen[1].target, "/calendar/v3/calendars/primary/events");
        assert_eq!(b["id"], "miabcg0");
        assert_eq!(b["start"], json!({"dateTime": "2026-11-07T15:00:00+01:00", "timeZone": "Europe/Berlin"}));
        assert_eq!(b["end"]["dateTime"], "2026-11-07T15:45:00+01:00");
        assert_eq!(b["reminders"], json!({"useDefault": true}));
        assert_eq!(b["extendedProperties"]["private"]["mirza"], "abc");
        assert_eq!(seen[2].method, "DELETE");
    }

    #[tokio::test]
    async fn finds_a_task_that_arrived_and_moves_tasks() {
        let (g, mock) = google(vec![
            token(""),
            reply(200, json!({"items": [{"id": "X", "title": "Other"}, {"id": "T9", "title": "Call Sara", "notes": "about the contract"}]}).to_string()),
            reply(200, json!({"id": "T10"}).to_string()),
        ])
        .await;
        let found = g.find_task("L1", &draft(), 1_794_000_000_000).await.unwrap();
        assert_eq!(found, Some(TaskRef { list: "L1".into(), id: "T9".into() }));
        let moved = g.move_task(&found.unwrap(), "L2").await.unwrap();
        assert_eq!(moved, TaskRef { list: "L2".into(), id: "T10".into() });
        let seen = mock.seen();
        assert!(seen[1].target.starts_with("/tasks/v1/lists/L1/tasks?updatedMin=2026-11-06T"), "{}", seen[1].target);
        assert_eq!(seen[2].target, "/tasks/v1/lists/L1/tasks/T9/move?destinationTasklist=L2");
        let length = seen[2].headers.iter().find(|(k, _)| k == "content-length").map(|(_, v)| v.as_str());
        assert_eq!(length, Some("0"), "Google refuses a POST without a length");
    }
}
