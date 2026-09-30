mod kde;
mod uinput;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use tokio::runtime::Handle;
use zbus::Connection;

pub use kde::{SharedActive, stop_watching, watch_active_window};
pub use uinput::VirtualKeyboard;

use crate::clipboard::Clipboard;
use crate::keymap::{self, Run};
use crate::{Options, Typed};

/// Types on Linux through a uinput keyboard, switching KDE layouts so each
/// character is typed on a layout that has it.
pub struct Backend {
    kb: VirtualKeyboard,
    rt: Handle,
    conn: Connection,
    clip: Clipboard,
    layouts: Option<kde::Layouts>,
}

impl Backend {
    pub fn new(rt: Handle, conn: Connection) -> io::Result<Self> {
        let kb = VirtualKeyboard::new("Mirza virtual keyboard").map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("cannot create the virtual keyboard: {e} (is the uinput udev rule installed?)"),
            )
        })?;
        Ok(Self { kb, rt, conn, clip: Clipboard::new(), layouts: None })
    }

    pub fn begin(&mut self) {
        self.layouts = None; // read lazily, just before the first keystroke
    }

    /// Types as much of `text` as it can while `ok()` holds. Returns how many
    /// characters were typed, or `Typed::Unmappable` if some character has no
    /// key on any layout.
    pub fn type_text(&mut self, text: &str, opts: &Options, ok: &dyn Fn() -> bool) -> io::Result<Typed> {
        let layouts = self.layouts.get_or_insert_with(|| kde::Layouts::current(&self.rt, &self.conn));
        layouts.refresh(&self.rt, &self.conn);
        let Some(runs) = keymap::plan(&layouts.names, layouts.cur, text) else {
            return Ok(Typed::Unmappable);
        };
        let mut n = 0;
        for Run { layout, keys } in runs {
            layouts
                .use_layout(&self.rt, &self.conn, layout)
                .map_err(|e| io::Error::other(format!("switching keyboard layout: {e}")))?;
            for ks in keys {
                if !ok() {
                    return Ok(Typed::Chars(n));
                }
                self.kb.stroke(ks, opts.key_delay)?;
                layouts.typed = true;
                n += 1;
            }
        }
        Ok(Typed::Chars(n))
    }

    pub fn paste(&mut self, text: &str, opts: &Options) -> io::Result<()> {
        let keys = keymap::parse_combo(&opts.paste_key).map_err(io::Error::other)?;
        // Shift+Insert pastes the primary selection in some apps.
        let primary = keys == [keymap::KEY_LEFTSHIFT, keymap::KEY_INSERT];
        self.clip.set(text, primary)?;
        self.kb.press(&keys)?;
        std::thread::sleep(Duration::from_millis(150)); // the app reads the clipboard asynchronously
        Ok(())
    }

    pub fn copy(&mut self, text: &str) -> io::Result<()> {
        self.clip.set(text, false)
    }

    /// Gives the user's own layout back, e.g. when another window is active.
    pub fn restore(&mut self) {
        if let Some(l) = self.layouts.as_mut() {
            l.restore(&self.rt, &self.conn);
        }
    }

    pub fn end(&mut self) {
        if self.layouts.as_ref().is_some_and(|l| l.typed) {
            std::thread::sleep(Duration::from_millis(200)); // let the compositor process the last keys
        }
        self.restore();
        self.layouts = None;
    }

    pub fn raw_keyboard(&mut self) -> &mut VirtualKeyboard {
        &mut self.kb
    }
}

pub type FocusChanged = Arc<dyn Fn() + Send + Sync>;
