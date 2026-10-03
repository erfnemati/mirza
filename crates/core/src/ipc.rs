//! Messages between the daemon and its clients (the command line and the
//! settings window), one JSON object per line over a local socket.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Toggle,
    Start,
    Stop,
    Cancel,
    Status,
    OpenPanel,
    NextProvider,
    SetProvider {
        id: String,
    },
    Reload,
    Quit,
    /// Simulates a shortcut press or release (used by tests and scripts).
    Shortcut {
        id: String,
        pressed: bool,
    },
    /// Turns global shortcuts off while the settings window records new
    /// keys, so the desktop doesn't act on them; they come back on after
    /// `false` or after 30 seconds.
    SuspendShortcuts {
        suspend: bool,
    },
    /// Everything the settings window shows that isn't in the config file.
    Snapshot,
    /// Fetches usage now; the snapshot shows when it's done.
    RefreshUsage,
    /// Plays the start sound, to try the volume.
    TestSound,
    /// Forgets the recent transcripts.
    ClearHistory,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Snapshot>,
}

impl Reply {
    pub fn ok() -> Self {
        Self { ok: true, message: String::new(), status: None, snapshot: None }
    }
    pub fn err(message: impl Into<String>) -> Self {
        Self { ok: false, message: message.into(), status: None, snapshot: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Idle,
    Connecting,
    Listening,
    Finishing,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Status {
    pub state: State,
    pub provider: String,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub last_text: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub status: Status,
    /// Recent transcripts, newest last.
    pub history: Vec<String>,
    pub usage: Option<UsageInfo>,
    /// Whether usage is being fetched right now.
    #[serde(default)]
    pub usage_loading: bool,
    /// Why the last usage fetch failed; empty when it worked.
    #[serde(default)]
    pub usage_error: String,
    /// When usage was last fetched, in Unix seconds.
    #[serde(default)]
    pub usage_updated: Option<u64>,
    /// Shortcuts as the desktop bound them: (id, key description).
    pub shortcuts: Vec<(String, String)>,
    /// How shortcuts are delivered: "portal", or "" when none are active.
    pub shortcut_backend: String,
    /// Why typing can't work, if it can't (e.g. no access to /dev/uinput).
    pub typing_problem: String,
    /// Whether Mirza can tell which window is active, to pause typing when
    /// you switch away (Windows, and KDE Plasma on Linux).
    #[serde(default)]
    pub focus_tracking: bool,
    /// The running Mirza's version.
    #[serde(default)]
    pub version: String,
    /// A newer release, when there is one.
    #[serde(default)]
    pub update: Option<crate::update::Release>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct UsageInfo {
    pub provider: String,
    pub today: f64,
    pub month: f64,
    pub month_minutes: f64,
    pub requests: u64,
    pub since_credit: Option<f64>,
}

/// Sends one request to the running daemon.
#[cfg(unix)]
pub async fn request(req: &Request) -> std::io::Result<Reply> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let stream = tokio::net::UnixStream::connect(socket_path()).await.map_err(|e| {
        std::io::Error::new(e.kind(), "Mirza is not running (start it with `mirza` or from the app menu)")
    })?;
    let (r, mut w) = stream.into_split();
    let mut msg = serde_json::to_string(req).map_err(std::io::Error::other)?;
    msg.push('\n');
    w.write_all(msg.as_bytes()).await?;
    let mut line = String::new();
    BufReader::new(r).read_line(&mut line).await?;
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

/// Sends one request to the running daemon.
#[cfg(windows)]
pub async fn request(req: &Request) -> std::io::Result<Reply> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ClientOptions;
    const PIPE_BUSY: i32 = 231;
    let name = socket_path();
    let mut tries = 0;
    let pipe = loop {
        match ClientOptions::new().open(&name) {
            Ok(p) => break p,
            Err(e) if e.raw_os_error() == Some(PIPE_BUSY) && tries < 40 => {
                tries += 1;
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            Err(e) => {
                return Err(std::io::Error::new(e.kind(), "Mirza is not running (start it from the Start menu)"));
            }
        }
    };
    let (r, mut w) = tokio::io::split(pipe);
    let mut msg = serde_json::to_string(req).map_err(std::io::Error::other)?;
    msg.push('\n');
    w.write_all(msg.as_bytes()).await?;
    let mut line = String::new();
    BufReader::new(r).read_line(&mut line).await?;
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

/// Where the daemon listens.
pub fn socket_path() -> std::path::PathBuf {
    #[cfg(unix)]
    {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        dir.join("mirza.sock")
    }
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").unwrap_or_default();
        std::path::PathBuf::from(format!(r"\\.\pipe\mirza-{user}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format() {
        assert_eq!(serde_json::to_string(&Request::Toggle).unwrap(), r#"{"cmd":"toggle"}"#);
        let r: Request = serde_json::from_str(r#"{"cmd":"set_provider","id":"openai"}"#).unwrap();
        assert_eq!(r, Request::SetProvider { id: "openai".into() });
        assert_eq!(serde_json::to_string(&Reply::ok()).unwrap(), r#"{"ok":true}"#);
    }
}
