//! Keyboard layouts through KDE Plasma's D-Bus API, and the active window
//! through a small KWin script that calls back over D-Bus. Elsewhere a single
//! US layout is assumed and the active window is unknown.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::runtime::Handle;
use zbus::Connection;

/// The user's layouts and which one is active.
pub struct Layouts {
    pub names: Vec<String>, // XKB layouts, e.g. "us", "ir"; non-default variants as "us(dvorak)"
    pub cur: usize,
    orig: usize,         // active when dictation started
    last: Option<usize>, // last layout we switched to
    pub typed: bool,     // keys sent since the last switch
    kde: bool,
}

impl Layouts {
    fn us_only() -> Self {
        Self { names: vec!["us".into()], cur: 0, orig: 0, last: None, typed: false, kde: false }
    }

    pub fn current(rt: &Handle, conn: &Connection) -> Self {
        let Ok(cur) = rt.block_on(get_layout(conn)) else {
            return Self::us_only();
        };
        let Ok(list) = rt.block_on(call::<Vec<(String, String, String)>>(conn, "getLayoutsList", &())) else {
            return Self::us_only();
        };
        let mut names: Vec<String> = list.into_iter().map(|(name, _, _)| name).collect();
        for (i, v) in layout_variants().into_iter().enumerate() {
            if i < names.len() && !v.is_empty() {
                names[i] = format!("{}({v})", names[i]);
            }
        }
        if cur >= names.len() {
            return Self::us_only();
        }
        Self { names, cur, orig: cur, last: None, typed: false, kde: true }
    }

    /// Re-reads the active layout; the user may have switched meanwhile.
    pub fn refresh(&mut self, rt: &Handle, conn: &Connection) {
        if !self.kde {
            return;
        }
        if let Ok(cur) = rt.block_on(get_layout(conn))
            && cur < self.names.len()
        {
            self.cur = cur;
        }
    }

    /// Makes layout `i` active.
    pub fn use_layout(&mut self, rt: &Handle, conn: &Connection, i: usize) -> zbus::Result<()> {
        if i == self.cur {
            return Ok(());
        }
        if self.typed {
            // Let the compositor handle keys sent on the old layout.
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        rt.block_on(call::<bool>(conn, "setLayout", &(i as u32)))?;
        self.cur = i;
        self.last = Some(i);
        self.typed = false;
        Ok(())
    }

    /// Switches back to the layout active when dictation started, unless the
    /// user has changed the layout themselves.
    pub fn restore(&mut self, rt: &Handle, conn: &Connection) {
        self.refresh(rt, conn);
        if self.last == Some(self.cur) && self.cur != self.orig {
            let _ = self.use_layout(rt, conn, self.orig);
        }
    }
}

async fn call<R>(
    conn: &Connection,
    method: &str,
    body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
) -> zbus::Result<R>
where
    R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
{
    let reply =
        conn.call_method(Some("org.kde.keyboard"), "/Layouts", Some("org.kde.KeyboardLayouts"), method, body).await?;
    reply.body().deserialize()
}

async fn get_layout(conn: &Connection) -> zbus::Result<usize> {
    Ok(call::<u32>(conn, "getLayout", &()).await? as usize)
}

/// The variant of each layout from KDE's settings (empty for the default).
fn layout_variants() -> Vec<String> {
    let Some(dir) = dirs_config() else { return Vec::new() };
    let data = std::fs::read_to_string(dir.join("kxkbrc")).unwrap_or_default();
    data.lines()
        .find_map(|l| l.trim().strip_prefix("VariantList="))
        .map(|v| v.split(',').map(str::to_owned).collect())
        .unwrap_or_default()
}

fn dirs_config() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

pub use crate::SharedActive;

struct FocusIface {
    active: SharedActive,
    changed: Arc<dyn Fn() + Send + Sync>,
}

#[zbus::interface(name = "io.github.erfnemati.Mirza.Focus")]
impl FocusIface {
    fn active(&self, id: String) {
        tracing::debug!("active window: {id:?}");
        if id.is_empty() {
            return; // not a window: keep the last real one
        }
        *self.active.lock().unwrap() = Some(id);
        (self.changed)();
    }
}

const SCRIPT_NAME: &str = "mirza-focus";

/// Loads a KWin script that reports the active window to us whenever it
/// changes. Works on KDE Plasma 6 only; errors elsewhere.
pub async fn watch_active_window(
    conn: &Connection,
    changed: Arc<dyn Fn() + Send + Sync>,
) -> zbus::Result<SharedActive> {
    let active: SharedActive = Arc::default();
    conn.object_server().at("/focus", FocusIface { active: active.clone(), changed }).await?;
    let me = conn.unique_name().map(|n| n.to_string()).unwrap_or_default();
    // Only real windows are reported. On some systems KWin also emits an
    // activation with no window right after the real one, and taking that as
    // "no window is active" made typing wait for good.
    let js = format!(
        r#"function report(w) {{
    if (w) callDBus("{me}", "/focus", "io.github.erfnemati.Mirza.Focus", "Active", w.internalId.toString());
}}
workspace.windowActivated.connect(report);
report(workspace.activeWindow);
"#
    );
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("{SCRIPT_NAME}.js"));
    std::fs::write(&path, js).map_err(|e| zbus::Error::Failure(e.to_string()))?;

    let scripting = |method: &'static str| {
        let conn = conn.clone();
        async move {
            conn.call_method(
                Some("org.kde.KWin"),
                "/Scripting",
                Some("org.kde.kwin.Scripting"),
                method,
                &(SCRIPT_NAME,),
            )
            .await
        }
    };
    let _ = scripting("unloadScript").await; // left over from an earlier run
    let reply = conn
        .call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "loadScript",
            &(path.to_string_lossy().as_ref(), SCRIPT_NAME),
        )
        .await?;
    let id: i32 = reply.body().deserialize()?;
    if id < 0 {
        return Err(zbus::Error::Failure("KWin refused the script".into()));
    }
    conn.call_method(
        Some("org.kde.KWin"),
        format!("/Scripting/Script{id}").as_str(),
        Some("org.kde.kwin.Script"),
        "run",
        &(),
    )
    .await?;
    Ok(active)
}

/// Unloads the KWin script; call on shutdown.
pub async fn stop_watching(conn: &Connection) {
    let _ = conn
        .call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "unloadScript",
            &(SCRIPT_NAME,),
        )
        .await;
}
