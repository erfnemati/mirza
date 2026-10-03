//! Linux: tray over StatusNotifierItem, notifications and shortcuts over
//! D-Bus (the shortcuts portal), typing through uinput.

use std::sync::Arc;

use ksni::TrayMethods;
use mirza_hotkey::{Binding, portal};
use mirza_inject::linux::SharedActive;
use mirza_inject::{Focus, Injector};
use tokio::sync::mpsc;

use super::TrayView;
use super::linux_notify::Notifier;
use super::linux_tray::{self, MirzaTray};
use crate::daemon::{APP_ID, Msg};

pub struct Desktop {
    tx: mpsc::UnboundedSender<Msg>,
    conn: zbus::Connection,
    notifier: Notifier,
    injector: Result<Injector, String>,
    kwin: Option<SharedActive>,
    tray: Option<ksni::Handle<MirzaTray>>,
    shortcuts: Option<portal::Registration>,
}

impl Desktop {
    pub async fn new(tx: mpsc::UnboundedSender<Msg>) -> Result<Self, String> {
        let conn = zbus::Connection::session().await.map_err(|e| format!("connecting to D-Bus: {e}"))?;
        let rt = tokio::runtime::Handle::current();
        let injector = Injector::new(rt, conn.clone()).map_err(|e| e.to_string());
        let icon_dir = linux_tray::install_icons();
        let notifier = Notifier::new(conn.clone(), &icon_dir);
        Ok(Self { tx, conn, notifier, injector, kwin: None, tray: None, shortcuts: None })
    }

    /// Starts watching the active window and shows the tray icon.
    pub async fn start(&mut self, view: TrayView) {
        let tx = self.tx.clone();
        let changed = Arc::new(move || {
            let _ = tx.send(Msg::FocusChanged);
        });
        match mirza_inject::linux::watch_active_window(&self.conn, changed).await {
            Ok(active) => self.kwin = Some(active),
            Err(e) => tracing::info!("not watching the active window: {e}"),
        }
        let t = MirzaTray { tx: self.tx.clone(), view };
        match t.spawn().await {
            Ok(h) => self.tray = Some(h),
            Err(e) => tracing::warn!("no tray icon: {e}"),
        }
        portal::register_app(APP_ID).await;
    }

    pub async fn shutdown(&mut self) {
        mirza_inject::linux::stop_watching(&self.conn).await;
        if let Some(t) = self.tray.take() {
            t.shutdown().await;
        }
    }

    pub fn injector(&self) -> Result<&Injector, String> {
        self.injector.as_ref().map_err(Clone::clone)
    }

    pub fn typing_problem(&self) -> String {
        self.injector.as_ref().err().cloned().unwrap_or_default()
    }

    pub fn focus_tracking(&self) -> bool {
        self.kwin.is_some()
    }

    /// The window dictation starts in, to pause typing while another is active.
    pub fn focus(&self, follow: bool) -> Focus {
        match (&self.kwin, follow) {
            (Some(active), true) => Focus::watched(active),
            _ => Focus::unknown(),
        }
    }

    pub async fn notify(&self, summary: &str, body: &str, urgent: bool, replaces: u32) -> u32 {
        self.notifier.show(summary, body, urgent, replaces).await
    }

    /// A message worth keeping, like a new release: it stays in the
    /// notification history.
    pub async fn announce(&self, summary: &str, body: &str) {
        self.notifier.announce(summary, body).await;
    }

    pub async fn close_notification(&self, id: u32) {
        self.notifier.close(id).await;
    }

    pub async fn show(&self, view: TrayView) {
        if let Some(h) = &self.tray {
            h.update(move |t| t.view = view).await;
        }
    }

    /// Binds global shortcuts through the portal; their events go to the
    /// daemon as Msg::Hotkey.
    pub async fn bind_shortcuts(&mut self, bindings: Vec<Binding>) -> Result<(), String> {
        self.shortcuts = None; // unbinds the previous set
        if bindings.is_empty() {
            return Ok(());
        }
        let (htx, mut hrx) = mpsc::unbounded_channel();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = hrx.recv().await {
                if tx.send(Msg::Hotkey(ev)).is_err() {
                    break;
                }
            }
        });
        let kde = is_kde().then_some((&self.conn, APP_ID));
        let result = portal::bind(&bindings, htx, kde).await;
        let reg = result.map_err(|e| {
            format!("{e}\nBind the command `mirza toggle` in your desktop's shortcut settings instead.")
        })?;
        for (id, trigger) in &reg.bound {
            tracing::info!("shortcut {id}: {trigger}");
        }
        if is_kde() {
            portal::kde_set_names(&self.conn, APP_ID, "Mirza", &bindings).await;
        }
        self.shortcuts = Some(reg);
        Ok(())
    }

    /// The shortcuts as the desktop bound them: (id, keys).
    pub fn bound_shortcuts(&self) -> Vec<(String, String)> {
        self.shortcuts.as_ref().map(|r| r.bound.clone()).unwrap_or_default()
    }

    pub fn shortcut_backend(&self) -> &'static str {
        if self.shortcuts.is_some() { "portal" } else { "" }
    }
}

fn is_kde() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_uppercase().contains("KDE")
}
