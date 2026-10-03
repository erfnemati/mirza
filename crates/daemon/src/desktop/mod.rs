//! What differs between desktops: the tray, notifications, global shortcuts,
//! typing and focus tracking. Each OS module has a `Desktop` with the same
//! methods.

use mirza_core::ipc::State;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod linux_notify;
#[cfg(target_os = "linux")]
mod linux_tray;
#[cfg(target_os = "linux")]
pub use linux::Desktop;

mod images;
#[cfg(not(target_os = "linux"))]
mod native;
#[cfg(not(target_os = "linux"))]
pub use native::{Desktop, UiEvent, run_ui};

/// Something chosen in the tray menu.
#[derive(Debug, Clone)]
pub enum Action {
    Toggle,
    Cancel,
    SetProvider(String),
    CopyLast,
    OpenSettings,
    OpenConfigFile,
    /// Open the download page of a newer release.
    OpenUpdate,
    Quit,
}

/// What the tray shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrayView {
    pub state: State,
    pub provider: String,
    pub error: String,
    pub has_last: bool,
    /// e.g. "$0.17 this month · $0.01 today"; empty when unknown.
    pub spend: String,
    /// While recording: how big the red dot is (0 to 5), following your voice.
    pub frame: u8,
    /// The version of a newer release; empty when there is none.
    pub update: String,
}

impl TrayView {
    pub fn status_line(&self) -> String {
        let name = mirza_core::config::provider_name(&self.provider);
        match self.state {
            State::Idle if !self.error.is_empty() => format!("Error: {}", self.error),
            State::Idle => format!("Ready · {name}"),
            State::Connecting => format!("Connecting to {name}…"),
            State::Listening => format!("Listening · {name}"),
            State::Finishing => "Finishing…".into(),
        }
    }

    /// Which tray picture to show: idle, busy, error, or rec0 to rec5.
    pub fn icon(&self) -> &'static str {
        const REC: [&str; 6] = ["rec0", "rec1", "rec2", "rec3", "rec4", "rec5"];
        match self.state {
            State::Idle if !self.error.is_empty() => "error",
            State::Idle => "idle",
            State::Connecting | State::Listening => REC[(self.frame as usize).min(5)],
            State::Finishing => "busy",
        }
    }
}

/// Opens a file or URL with the system's default app.
pub fn open_path(p: impl AsRef<std::ffi::OsStr>) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(p).spawn();
}
