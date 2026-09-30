//! macOS: types by posting keyboard events that carry the text itself (up to
//! 20 UTF-16 units each), so the keyboard layout doesn't matter. Needs the
//! Accessibility permission.

use std::io;
use std::time::Duration;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

use crate::clipboard::Clipboard;
use crate::{Options, Typed};

const KEY_RETURN: CGKeyCode = 36;
const KEY_TAB: CGKeyCode = 48;
const KEY_DELETE: CGKeyCode = 51;
const KEY_V: CGKeyCode = 9; // on the ANSI layout
const MAX_UNITS: usize = 20;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
}

/// Whether macOS lets this app send keystrokes (Accessibility permission).
pub fn trusted() -> bool {
    // SAFETY: no arguments; returns a Boolean.
    unsafe { AXIsProcessTrusted() != 0 }
}

pub struct Backend {
    clip: Clipboard,
}

fn source() -> io::Result<CGEventSource> {
    CGEventSource::new(CGEventSourceStateID::CombinedSessionState).map_err(|_| io::Error::other("no event source"))
}

fn post_key(code: CGKeyCode, flags: CGEventFlags) -> io::Result<()> {
    for down in [true, false] {
        let e = CGEvent::new_keyboard_event(source()?, code, down).map_err(|_| io::Error::other("key event"))?;
        e.set_flags(flags);
        e.post(CGEventTapLocation::HID);
    }
    Ok(())
}

fn post_text(units: &[u16]) -> io::Result<()> {
    for down in [true, false] {
        let e = CGEvent::new_keyboard_event(source()?, 0, down).map_err(|_| io::Error::other("key event"))?;
        e.set_flags(CGEventFlags::empty()); // held modifiers must not turn text into shortcuts
        e.set_string_from_utf16_unchecked(units);
        e.post(CGEventTapLocation::HID);
    }
    Ok(())
}

impl Backend {
    pub fn new() -> io::Result<Self> {
        Ok(Self { clip: Clipboard::new() })
    }

    pub fn begin(&mut self) {}

    pub fn type_text(&mut self, text: &str, opts: &Options, ok: &dyn Fn() -> bool) -> io::Result<Typed> {
        if !trusted() {
            return Err(io::Error::other(
                "Mirza needs the Accessibility permission to type (System Settings → Privacy & Security → Accessibility)",
            ));
        }
        // SAFETY: no arguments; returns a Boolean.
        if unsafe { IsSecureEventInputEnabled() } != 0 {
            return Err(io::Error::other("a password field has secure input on, so macOS blocks typing"));
        }
        let mut n = 0;
        let mut batch: Vec<u16> = Vec::with_capacity(MAX_UNITS + 2);
        let mut batch_chars = 0;
        let flush = |batch: &mut Vec<u16>| -> io::Result<()> {
            if !batch.is_empty() {
                post_text(batch)?;
                batch.clear();
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(())
        };
        for c in text.chars() {
            if !ok() {
                break;
            }
            match c {
                '\n' | '\t' | '\u{8}' => {
                    flush(&mut batch)?;
                    n += batch_chars;
                    batch_chars = 0;
                    let code = match c {
                        '\n' => KEY_RETURN,
                        '\t' => KEY_TAB,
                        _ => KEY_DELETE,
                    };
                    post_key(code, CGEventFlags::empty())?;
                    n += 1;
                    std::thread::sleep(opts.key_delay);
                }
                c => {
                    let mut buf = [0u16; 2];
                    let units = c.encode_utf16(&mut buf);
                    if batch.len() + units.len() > MAX_UNITS {
                        flush(&mut batch)?;
                        n += batch_chars;
                        batch_chars = 0;
                    }
                    batch.extend_from_slice(units);
                    batch_chars += 1;
                }
            }
        }
        flush(&mut batch)?;
        n += batch_chars;
        Ok(Typed::Chars(n))
    }

    pub fn paste(&mut self, text: &str, opts: &Options) -> io::Result<()> {
        self.clip.set(text, false)?;
        let combo = opts.paste_key.to_lowercase();
        let flags =
            if combo.contains("ctrl") { CGEventFlags::CGEventFlagControl } else { CGEventFlags::CGEventFlagCommand };
        post_key(KEY_V, flags)?;
        std::thread::sleep(Duration::from_millis(150));
        Ok(())
    }

    pub fn copy(&mut self, text: &str) -> io::Result<()> {
        self.clip.set(text, false)
    }

    pub fn restore(&mut self) {}

    pub fn end(&mut self) {}
}
