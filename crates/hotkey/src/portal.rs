//! Global shortcuts on Linux through the XDG desktop portal
//! (org.freedesktop.portal.GlobalShortcuts). Works on Wayland with KDE Plasma
//! 5.27+, GNOME 48+ and Hyprland; the desktop owns the actual keys, and the
//! user can change them in its shortcut settings.

use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use ashpd::desktop::{CreateSessionOptions, Session};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::{Binding, Combo, Event};

pub struct Registration {
    // Dropping the session would unbind the shortcuts.
    _session: Session<GlobalShortcuts>,
    _portal: GlobalShortcuts,
    _tasks: Vec<tokio::task::JoinHandle<()>>,
    /// What the desktop reports for each shortcut: (id, trigger description).
    pub bound: Vec<(String, String)>,
}

/// Tells the portal which app this is, so its shortcuts show up under the
/// app's name. Needs a matching installed .desktop file; failures are ignored.
pub async fn register_app(app_id: &str) {
    if let Ok(id) = app_id.parse() {
        let _ = ashpd::register_host_app(id).await;
    }
}

/// Binds the shortcuts and sends their press and release events to `tx`.
pub async fn bind(bindings: &[Binding], tx: mpsc::UnboundedSender<Event>) -> ashpd::Result<Registration> {
    let portal = GlobalShortcuts::new().await?;
    let session = portal.create_session(CreateSessionOptions::default()).await?;
    let shortcuts: Vec<NewShortcut> = bindings
        .iter()
        .map(|b| {
            let trigger = Combo::parse(&b.keys).ok().and_then(|c| c.xdg_trigger());
            NewShortcut::new(b.id.clone(), b.description.clone()).preferred_trigger(trigger.as_deref())
        })
        .collect();
    let response = portal.bind_shortcuts(&session, &shortcuts, None, Default::default()).await?.response()?;
    let bound = response.shortcuts().iter().map(|s| (s.id().to_owned(), s.trigger_description().to_owned())).collect();

    let mut activated = portal.receive_activated().await?;
    let mut deactivated = portal.receive_deactivated().await?;
    let mut changed = portal.receive_shortcuts_changed().await?;
    let t1 = tx.clone();
    let a = tokio::spawn(async move {
        while let Some(ev) = activated.next().await {
            let _ = t1.send(Event::Pressed(ev.shortcut_id().to_owned()));
        }
    });
    let t2 = tx.clone();
    let d = tokio::spawn(async move {
        while let Some(ev) = deactivated.next().await {
            let _ = t2.send(Event::Released(ev.shortcut_id().to_owned()));
        }
    });
    let c = tokio::spawn(async move {
        while let Some(ev) = changed.next().await {
            for s in ev.shortcuts() {
                let _ = tx.send(Event::Rebound(s.id().to_owned(), s.trigger_description().to_owned()));
            }
        }
    });
    Ok(Registration { _session: session, _portal: portal, _tasks: vec![a, d, c], bound })
}

/// KDE keeps the keys it first bound for a shortcut ID and ignores new
/// suggestions. Forgetting the ID lets the next bind use the suggested keys.
pub async fn kde_forget(conn: &zbus::Connection, app_id: &str, shortcut_id: &str) -> bool {
    conn.call_method(
        Some("org.kde.kglobalaccel"),
        "/kglobalaccel",
        Some("org.kde.KGlobalAccel"),
        "unregister",
        &(app_id, shortcut_id),
    )
    .await
    .ok()
    .and_then(|m| m.body().deserialize::<bool>().ok())
    .unwrap_or(false)
}

/// KDE names an app's shortcut group after its .desktop file, and falls back
/// to the app ID when it can't find it at that moment. Registering the
/// shortcuts again with the names sets the group's name to `app_name`.
pub async fn kde_set_names(conn: &zbus::Connection, app_id: &str, app_name: &str, bindings: &[Binding]) {
    for b in bindings {
        // A Vec, not an array: D-Bus wants a list of strings ("as").
        let id: Vec<&str> = vec![app_id, &b.id, app_name, &b.description];
        let r = conn
            .call_method(
                Some("org.kde.kglobalaccel"),
                "/kglobalaccel",
                Some("org.kde.KGlobalAccel"),
                "doRegister",
                &(id,),
            )
            .await;
        if let Err(e) = r {
            tracing::warn!("naming the shortcut group in KDE: {e}");
        }
    }
}
