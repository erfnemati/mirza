//! Windows and macOS: the tray icon lives on the main thread (the OS requires
//! it) in a tao event loop, while the daemon runs on a tokio thread and sends
//! it updates. Shortcuts come from a keyboard hook (Windows) or an event tap
//! (macOS); notifications go through the system's notification center.

#[cfg(windows)]
use std::time::Duration;

use mirza_core::config::{PROVIDERS, provider_name};
use mirza_core::ipc::State;
use mirza_hotkey::Binding;
use mirza_inject::{Focus, Injector, SharedActive};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopProxy};
use tokio::sync::mpsc;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[cfg(target_os = "macos")]
use mirza_hotkey::macos::Hotkeys;
#[cfg(windows)]
use mirza_hotkey::windows::Hotkeys;

use super::{Action, TrayView};
use crate::daemon::Msg;

pub enum UiEvent {
    Show(TrayView),
    Menu(MenuId),
    TrayClick,
    Quit,
}

pub struct Desktop {
    #[cfg_attr(not(windows), allow(dead_code))] // the Windows focus watcher uses it
    tx: mpsc::UnboundedSender<Msg>,
    proxy: EventLoopProxy<UiEvent>,
    injector: Result<Injector, String>,
    hotkeys: Result<Hotkeys, String>,
    bound: Vec<(String, String)>,
    active: Option<SharedActive>,
}

impl Desktop {
    pub async fn new(tx: mpsc::UnboundedSender<Msg>, proxy: EventLoopProxy<UiEvent>) -> Result<Self, String> {
        let (htx, mut hrx) = mpsc::unbounded_channel();
        let fwd = tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = hrx.recv().await {
                if fwd.send(Msg::Hotkey(ev)).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            injector: Injector::new().map_err(|e| e.to_string()),
            hotkeys: Hotkeys::start(htx),
            tx,
            proxy,
            bound: Vec::new(),
            active: None,
        })
    }

    pub async fn start(&mut self, view: TrayView) {
        #[cfg(windows)]
        {
            // Follow the foreground window so typing pauses while another is active.
            let active: SharedActive =
                std::sync::Arc::new(std::sync::Mutex::new(Some(mirza_inject::windows::foreground_window())));
            let (shared, tx) = (active.clone(), self.tx.clone());
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(150));
                    let now = mirza_inject::windows::foreground_window();
                    let changed = {
                        let mut a = shared.lock().unwrap();
                        let changed = a.as_deref() != Some(now.as_str());
                        *a = Some(now);
                        changed
                    };
                    if changed && tx.send(Msg::FocusChanged).is_err() {
                        break;
                    }
                }
            });
            self.active = Some(active);
        }
        self.show(view).await;
    }

    pub async fn shutdown(&mut self) {
        let _ = self.proxy.send_event(UiEvent::Quit);
    }

    pub fn injector(&self) -> Result<&Injector, String> {
        self.injector.as_ref().map_err(Clone::clone)
    }

    pub fn typing_problem(&self) -> String {
        if let Err(e) = &self.injector {
            return e.clone();
        }
        #[cfg(target_os = "macos")]
        if !mirza_inject::macos::trusted() {
            return "Mirza needs the Accessibility permission to type and to see its shortcuts \
                    (System Settings → Privacy & Security → Accessibility)"
                .into();
        }
        String::new()
    }

    pub fn focus_tracking(&self) -> bool {
        self.active.is_some()
    }

    pub fn focus(&self, follow: bool) -> Focus {
        match (&self.active, follow) {
            (Some(a), true) => Focus::watched(a),
            _ => Focus::unknown(),
        }
    }

    pub async fn notify(&self, summary: &str, body: &str, _urgent: bool, _replaces: u32) -> u32 {
        let (summary, body) = (summary.to_owned(), body.to_owned());
        tokio::task::spawn_blocking(move || {
            #[cfg(windows)]
            let r = notify_rust::Notification::new().appname("Mirza").summary(&summary).body(&body).show().map(|_| ());
            #[cfg(target_os = "macos")]
            let r = {
                // AppleScript string literals: escape backslashes and quotes.
                let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
                let script =
                    format!("display notification \"{}\" with title \"Mirza\" subtitle \"{}\"", q(&body), q(&summary));
                std::process::Command::new("osascript").args(["-e", &script]).status().map(|_| ())
            };
            if let Err(e) = r {
                tracing::warn!("notification failed: {e}");
            }
        });
        0
    }

    pub async fn announce(&self, summary: &str, body: &str) {
        self.notify(summary, body, false, 0).await;
    }

    pub async fn close_notification(&self, _id: u32) {}

    pub async fn show(&self, view: TrayView) {
        let _ = self.proxy.send_event(UiEvent::Show(view));
    }

    pub async fn bind_shortcuts(&mut self, bindings: Vec<Binding>) -> Result<(), String> {
        let hotkeys = self.hotkeys.as_ref().map_err(Clone::clone)?;
        let hold: Vec<String> = bindings.iter().filter(|b| b.id.ends_with("-hold")).map(|b| b.id.clone()).collect();
        hotkeys.set(&bindings, &hold);
        self.bound = bindings.into_iter().map(|b| (b.id, b.keys)).collect();
        Ok(())
    }

    pub fn bound_shortcuts(&self) -> Vec<(String, String)> {
        self.bound.clone()
    }

    pub fn shortcut_backend(&self) -> &'static str {
        if self.hotkeys.is_err() {
            ""
        } else if cfg!(windows) {
            "hook"
        } else {
            "tap"
        }
    }
}

/// Runs the tray on the main thread until the daemon quits.
pub fn run_ui(event_loop: EventLoop<UiEvent>, tx: mpsc::UnboundedSender<Msg>) -> ! {
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = proxy.send_event(UiEvent::Menu(e.id));
    }));
    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            let _ = proxy.send_event(UiEvent::TrayClick);
        }
    }));
    let mut ui: Option<TrayUi> = None;
    let mut pending: Option<TrayView> = None;
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => match TrayUi::new() {
                Ok(mut u) => {
                    if let Some(v) = pending.take() {
                        u.show(&v);
                    }
                    ui = Some(u);
                }
                Err(e) => tracing::warn!("no tray icon: {e}"),
            },
            Event::UserEvent(UiEvent::Show(v)) => match ui.as_mut() {
                Some(u) => u.show(&v),
                None => pending = Some(v),
            },
            Event::UserEvent(UiEvent::Menu(id)) => {
                if let Some(a) = ui.as_ref().and_then(|u| u.action(&id)) {
                    let _ = tx.send(Msg::Tray(a));
                }
            }
            Event::UserEvent(UiEvent::TrayClick) => {
                // On Windows a left click opens the settings (the menu is on the
                // right button); on macOS every click shows the menu.
                if cfg!(windows) {
                    let _ = tx.send(Msg::Tray(Action::OpenSettings));
                }
            }
            Event::UserEvent(UiEvent::Quit) => *flow = ControlFlow::Exit,
            _ => {}
        }
    })
}

struct TrayUi {
    tray: TrayIcon,
    menu: Menu,
    icon: &'static str,
    last: Option<TrayView>,
    status: MenuItem,
    spend: MenuItem,
    /// "Download Mirza x.y.z", in the menu only while there is a newer release.
    update: MenuItem,
    update_shown: bool,
    toggle: MenuItem,
    cancel: MenuItem,
    providers: Vec<(&'static str, CheckMenuItem)>,
    copy_last: MenuItem,
    settings: MenuItem,
    config: MenuItem,
    quit: MenuItem,
}

impl TrayUi {
    fn new() -> Result<Self, String> {
        let status = MenuItem::new("Mirza", false, None);
        let spend = MenuItem::new("", false, None);
        let update = MenuItem::new("", true, None);
        let toggle = MenuItem::new("Start dictation", true, None);
        let cancel = MenuItem::new("Cancel", false, None);
        let providers: Vec<(&'static str, CheckMenuItem)> =
            PROVIDERS.iter().map(|p| (*p, CheckMenuItem::new(provider_name(p), true, false, None))).collect();
        let copy_last = MenuItem::new("Copy last transcript", false, None);
        let settings = MenuItem::new("Settings…", true, None);
        let config = MenuItem::new("Edit config file", true, None);
        let quit = MenuItem::new("Quit Mirza", true, None);
        let menu = Menu::new();
        let sep = PredefinedMenuItem::separator;
        let err = |e: tray_icon::menu::Error| e.to_string();
        menu.append_items(&[&status, &spend, &sep(), &toggle, &cancel, &sep()]).map_err(err)?;
        for (_, item) in &providers {
            menu.append(item).map_err(err)?;
        }
        menu.append_items(&[&sep(), &copy_last, &settings, &config, &sep(), &quit]).map_err(err)?;
        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_tooltip("Mirza")
            .with_menu_on_left_click(cfg!(target_os = "macos"));
        // macOS draws a template icon in the menu bar's own colour.
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(icon("idle"));
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(icon("idle"));
        let tray = builder.build().map_err(|e| e.to_string())?;
        Ok(Self {
            tray,
            menu,
            icon: "idle",
            last: None,
            status,
            spend,
            update,
            update_shown: false,
            toggle,
            cancel,
            providers,
            copy_last,
            settings,
            config,
            quit,
        })
    }

    fn show(&mut self, v: &TrayView) {
        let wanted = v.icon();
        if wanted != self.icon {
            #[cfg(target_os = "macos")]
            let _ = self.tray.set_icon_templated(Some(icon(wanted)));
            #[cfg(not(target_os = "macos"))]
            let _ = self.tray.set_icon(Some(icon(wanted)));
            self.icon = wanted;
        }
        // While recording only the picture changes, several times a second.
        let same_text = self.last.as_ref().is_some_and(|l| TrayView { frame: v.frame, ..l.clone() } == *v);
        self.last = Some(v.clone());
        if same_text {
            return;
        }
        self.status.set_text(v.status_line());
        self.spend.set_text(if v.spend.is_empty() { "Usage: open the settings" } else { &v.spend });
        if !v.update.is_empty() {
            self.update.set_text(format!("Download Mirza {}", v.update));
        }
        if v.update.is_empty() == self.update_shown {
            // Right under the status and spend lines.
            let r = if self.update_shown { self.menu.remove(&self.update) } else { self.menu.insert(&self.update, 2) };
            match r {
                Ok(()) => self.update_shown = !self.update_shown,
                Err(e) => tracing::warn!("tray menu: {e}"),
            }
        }
        let idle = v.state == State::Idle;
        self.toggle.set_text(if idle { "Start dictation" } else { "Stop dictation" });
        self.cancel.set_enabled(!idle);
        for (id, item) in &self.providers {
            item.set_checked(*id == v.provider);
        }
        self.copy_last.set_enabled(v.has_last);
        let _ = self.tray.set_tooltip(Some(format!("Mirza — {}", v.status_line())));
    }

    fn action(&self, id: &MenuId) -> Option<Action> {
        if id == self.toggle.id() {
            Some(Action::Toggle)
        } else if id == self.cancel.id() {
            Some(Action::Cancel)
        } else if id == self.copy_last.id() {
            Some(Action::CopyLast)
        } else if id == self.settings.id() {
            Some(Action::OpenSettings)
        } else if id == self.config.id() {
            Some(Action::OpenConfigFile)
        } else if id == self.update.id() {
            Some(Action::OpenUpdate)
        } else if id == self.quit.id() {
            Some(Action::Quit)
        } else {
            self.providers.iter().find(|(_, item)| item.id() == id).map(|(p, _)| Action::SetProvider((*p).into()))
        }
    }
}

/// The tray picture for a state (see TrayView::icon).
fn icon(state: &str) -> Icon {
    let size = if cfg!(target_os = "macos") { 36 } else { 32 };
    Icon::from_rgba(super::images::rgba(state, size).to_vec(), size, size).expect("valid icon data")
}
