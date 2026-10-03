//! The tray icon (StatusNotifierItem on Linux, through ksni).

use ksni::menu::{RadioGroup, RadioItem, StandardItem};
use ksni::{MenuItem, ToolTip, Tray};
use mirza_core::config::{PROVIDERS, provider_name};
use mirza_core::ipc::State;
use tokio::sync::mpsc;

use super::{Action, TrayView};
use crate::daemon::Msg;

/// Writes the app icon to a private directory for notifications to use, and
/// returns the directory ("" on failure).
pub fn install_icons() -> String {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join("mirza").join("icons");
    let svg = include_str!("../../../../assets/icons/io.github.erfnemati.Mirza.svg");
    match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join("io.github.erfnemati.Mirza.svg"), svg)) {
        Ok(()) => dir.to_string_lossy().into_owned(),
        Err(_) => String::new(),
    }
}

/// RGBA to the ARGB (big-endian) the StatusNotifierItem protocol wants.
fn pixmap(state: &str, size: u32) -> ksni::Icon {
    let rgba = super::images::rgba(state, size);
    let data = rgba.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, a]| [a, r, g, b]).collect();
    ksni::Icon { width: size as i32, height: size as i32, data }
}

pub struct MirzaTray {
    pub tx: mpsc::UnboundedSender<Msg>,
    pub view: TrayView,
}

impl MirzaTray {
    fn send(&self, a: Action) {
        let _ = self.tx.send(Msg::Tray(a));
    }
}

impl Tray for MirzaTray {
    fn id(&self) -> String {
        "mirza".into()
    }

    fn title(&self) -> String {
        "Mirza".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let state = self.view.icon();
        vec![pixmap(state, 22), pixmap(state, 44)]
    }

    fn status(&self) -> ksni::Status {
        // The icon's colour shows the state; NeedsAttention would make KDE
        // animate it.
        ksni::Status::Active
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip { title: "Mirza".into(), description: self.view.status_line(), ..Default::default() }
    }

    fn menu_about_to_show(&mut self) {
        let _ = self.tx.send(Msg::RefreshUsage);
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Action::OpenSettings);
    }

    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        self.send(Action::Toggle);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let v = &self.view;
        let idle = v.state == State::Idle;
        let mut items: Vec<MenuItem<Self>> =
            vec![StandardItem { label: v.status_line(), enabled: false, ..Default::default() }.into()];
        if !v.spend.is_empty() {
            items.push(StandardItem { label: v.spend.clone(), enabled: false, ..Default::default() }.into());
        }
        if !v.update.is_empty() {
            items.push(
                StandardItem {
                    label: format!("Download Mirza {}", v.update),
                    icon_name: "software-update-available".into(),
                    activate: Box::new(|t: &mut Self| t.send(Action::OpenUpdate)),
                    ..Default::default()
                }
                .into(),
            );
        }
        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: if idle { "Start dictation".into() } else { "Stop dictation".into() },
                icon_name: if idle { "media-record".into() } else { "media-playback-stop".into() },
                activate: Box::new(|t: &mut Self| t.send(Action::Toggle)),
                ..Default::default()
            }
            .into(),
        );
        if !idle {
            items.push(
                StandardItem {
                    label: "Cancel".into(),
                    icon_name: "process-stop".into(),
                    activate: Box::new(|t: &mut Self| t.send(Action::Cancel)),
                    ..Default::default()
                }
                .into(),
            );
        }
        items.push(MenuItem::Separator);
        items.push(StandardItem { label: "Provider".into(), enabled: false, ..Default::default() }.into());
        items.push(
            RadioGroup {
                selected: PROVIDERS.iter().position(|p| *p == v.provider).unwrap_or(0),
                select: Box::new(|t: &mut Self, i| t.send(Action::SetProvider(PROVIDERS[i].into()))),
                options: PROVIDERS
                    .iter()
                    .map(|p| RadioItem { label: provider_name(p).into(), ..Default::default() })
                    .collect(),
            }
            .into(),
        );
        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Copy last transcript".into(),
                icon_name: "edit-copy".into(),
                enabled: v.has_last,
                activate: Box::new(|t: &mut Self| t.send(Action::CopyLast)),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Settings…".into(),
                icon_name: "configure".into(),
                activate: Box::new(|t: &mut Self| t.send(Action::OpenSettings)),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Edit config file".into(),
                icon_name: "document-edit".into(),
                activate: Box::new(|t: &mut Self| t.send(Action::OpenConfigFile)),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| t.send(Action::Quit)),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}
