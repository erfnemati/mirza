//! Desktop notifications over D-Bus (org.freedesktop.Notifications).

use std::collections::HashMap;

use zbus::Connection;
use zbus::zvariant::Value;

pub struct Notifier {
    conn: Connection,
    icon: String,
}

impl Notifier {
    /// `icon_dir` holds the app icon (see tray::install_icons).
    pub fn new(conn: Connection, icon_dir: &str) -> Self {
        let file = std::path::Path::new(icon_dir).join("io.github.erfnemati.Mirza.svg");
        let icon = if file.is_file() { file.to_string_lossy().into_owned() } else { "audio-input-microphone".into() };
        Self { conn, icon }
    }

    /// Shows a notification, replacing `replaces` if non-zero, and returns its ID.
    pub async fn show(&self, summary: &str, body: &str, urgent: bool, replaces: u32) -> u32 {
        let mut hints: HashMap<&str, Value> = HashMap::new();
        if urgent {
            hints.insert("urgency", Value::U8(2));
        } else {
            hints.insert("transient", Value::Bool(true));
        }
        let timeout: i32 = if urgent { -1 } else { 2500 };
        self.send(summary, body, hints, timeout, replaces).await
    }

    /// An ordinary notification that stays in the history.
    pub async fn announce(&self, summary: &str, body: &str) {
        self.send(summary, body, HashMap::new(), -1, 0).await;
    }

    async fn send(
        &self,
        summary: &str,
        body: &str,
        mut hints: HashMap<&str, Value<'_>>,
        timeout: i32,
        replaces: u32,
    ) -> u32 {
        hints.insert("desktop-entry", Value::from("io.github.erfnemati.Mirza"));
        let reply = self
            .conn
            .call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &("Mirza", replaces, self.icon.as_str(), summary, body, Vec::<&str>::new(), hints, timeout),
            )
            .await;
        match reply {
            Ok(m) => m.body().deserialize().unwrap_or(0),
            Err(e) => {
                tracing::warn!("notification failed: {e}");
                0
            }
        }
    }

    pub async fn close(&self, id: u32) {
        if id == 0 {
            return;
        }
        let _ = self
            .conn
            .call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "CloseNotification",
                &(id,),
            )
            .await;
    }
}
