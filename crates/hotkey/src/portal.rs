//! Global shortcuts on Linux through the XDG desktop portal
//! (org.freedesktop.portal.GlobalShortcuts). Works on Wayland with KDE Plasma
//! 5.27+, GNOME 48+ and Hyprland; the desktop owns the actual keys, and the
//! user can change them in its shortcut settings.

use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut, Shortcut};
use ashpd::desktop::{CreateSessionOptions, Session};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::{Binding, Combo, Event};

pub struct Registration {
    session: Option<Session<GlobalShortcuts>>,
    _portal: GlobalShortcuts,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    /// What the desktop reports for each shortcut: (id, trigger description).
    pub bound: Vec<(String, String)>,
}

impl Drop for Registration {
    /// Stops listening and closes the portal session. Without this, every
    /// re-binding left another listener behind, and each key press arrived
    /// once per listener.
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
        if let Some(session) = self.session.take() {
            tokio::spawn(async move {
                let _ = session.close().await;
            });
        }
    }
}

/// Tells the portal which app this is, so its shortcuts show up under the
/// app's name. Needs a matching installed .desktop file; failures are ignored.
pub async fn register_app(app_id: &str) {
    if let Ok(id) = app_id.parse() {
        let _ = ashpd::register_host_app(id).await;
    }
}

/// Binds the shortcuts and sends their press and release events to `tx`.
///
/// On KDE, `kde` gives the D-Bus connection and the app ID. Shortcuts KDE
/// already has for the app are then used as they are when their keys match:
/// KDE turns them on again when the session starts, and binding makes Plasma
/// 6.3 open its shortcut settings every time. Shortcuts whose keys changed
/// are forgotten before binding, because KDE keeps the keys it first bound and
/// ignores new suggestions.
pub async fn bind(
    bindings: &[Binding],
    tx: mpsc::UnboundedSender<Event>,
    kde: Option<(&zbus::Connection, &str)>,
) -> ashpd::Result<Registration> {
    let portal = GlobalShortcuts::new().await?;
    let mut session = portal.create_session(CreateSessionOptions::default()).await?;
    let mut bound = None;
    if let Some((conn, app_id)) = kde {
        let listed = portal.list_shortcuts(&session, Default::default()).await?.response()?;
        let existing = described(listed.shortcuts());
        if already_bound(bindings, &existing) {
            bound = Some(existing);
        } else {
            let stale = stale(bindings, &existing);
            if !stale.is_empty() {
                // With no session open, so the next one starts without them.
                let _ = session.close().await;
                for id in &stale {
                    kde_forget(conn, app_id, id).await;
                }
                session = portal.create_session(CreateSessionOptions::default()).await?;
            }
        }
    }
    let bound = match bound {
        Some(b) => b,
        None => {
            let shortcuts: Vec<NewShortcut> = bindings
                .iter()
                .map(|b| NewShortcut::new(b.id.clone(), b.description.clone()).preferred_trigger(trigger(b).as_deref()))
                .collect();
            let response = portal.bind_shortcuts(&session, &shortcuts, None, Default::default()).await?.response()?;
            described(response.shortcuts())
        }
    };

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
    Ok(Registration { session: Some(session), _portal: portal, tasks: vec![a, d, c], bound })
}

/// The keys to suggest to the desktop, in the portal's format.
fn trigger(b: &Binding) -> Option<String> {
    Combo::parse(&b.keys).ok().and_then(|c| c.xdg_trigger())
}

/// (id, trigger description) for each shortcut.
fn described(shortcuts: &[Shortcut]) -> Vec<(String, String)> {
    shortcuts.iter().map(|s| (s.id().to_owned(), s.trigger_description().to_owned())).collect()
}

/// Whether the desktop already has exactly these shortcuts, with these keys.
/// Keys the portal can't express (a lone modifier) count as matching.
fn already_bound(bindings: &[Binding], existing: &[(String, String)]) -> bool {
    bindings.len() == existing.len()
        && bindings.iter().all(|b| {
            existing.iter().any(|(id, keys)| *id == b.id && (trigger(b).is_none() || Combo::same_keys(keys, &b.keys)))
        })
}

/// Shortcuts the desktop has with other keys than the config asks for.
fn stale(bindings: &[Binding], existing: &[(String, String)]) -> Vec<String> {
    bindings
        .iter()
        .filter(|b| trigger(b).is_some())
        .filter(|b| existing.iter().any(|(id, keys)| *id == b.id && !Combo::same_keys(keys, &b.keys)))
        .map(|b| b.id.clone())
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(id: &str, keys: &str) -> Binding {
        Binding { id: id.into(), description: String::new(), keys: keys.into() }
    }

    #[test]
    fn reuses_matching_shortcuts() {
        let want = [binding("dictate-toggle", "Ctrl+Alt+H"), binding("dictate-hold", "Ctrl+Down")];
        let have = |a: &str, b: &str| {
            vec![("dictate-hold".to_string(), b.to_string()), ("dictate-toggle".to_string(), a.to_string())]
        };
        assert!(already_bound(&want, &have("Ctrl+Alt+H", "Ctrl+Down")));
        assert!(!already_bound(&want, &have("Ctrl+Alt+H", "Ctrl+Up")), "other keys");
        assert!(!already_bound(&want, &have("Ctrl+Alt+H", "")), "no keys");
        assert!(!already_bound(&want[..1], &have("Ctrl+Alt+H", "Ctrl+Down")), "one too many");
        assert!(!already_bound(&want, &[]), "nothing bound yet");
        assert_eq!(stale(&want, &have("Ctrl+Alt+H", "Ctrl+Up")), ["dictate-hold"]);
        assert!(stale(&want, &[]).is_empty());
    }
}
