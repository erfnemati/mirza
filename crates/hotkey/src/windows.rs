//! Global shortcuts on Windows through a low-level keyboard hook, which sees
//! every key, so single modifiers (RightCtrl) work too. The hook runs on its
//! own thread with a message loop and must answer quickly, or Windows drops it.

use std::sync::{Mutex, OnceLock};

use tokio::sync::mpsc;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, SetWindowsHookExW,
    TranslateMessage, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
};

use crate::matcher::Matcher;
use crate::{Binding, Event};

struct State {
    matcher: Mutex<Matcher>,
    tx: mpsc::UnboundedSender<Event>,
}

static STATE: OnceLock<State> = OnceLock::new();

pub struct Hotkeys;

impl Hotkeys {
    /// Installs the hook (once per process). Events go to `tx`.
    pub fn start(tx: mpsc::UnboundedSender<Event>) -> Result<Self, String> {
        if STATE.set(State { matcher: Mutex::new(Matcher::default()), tx }).is_err() {
            return Ok(Self); // already running
        }
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("mirza-hotkeys".into())
            .spawn(move || {
                // SAFETY: installing a hook with a valid procedure, then pumping
                // this thread's messages so Windows can call it.
                unsafe {
                    let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), GetModuleHandleW(std::ptr::null()), 0);
                    let _ = ready_tx.send(!hook.is_null());
                    if hook.is_null() {
                        return;
                    }
                    let mut msg: MSG = std::mem::zeroed();
                    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv() {
            Ok(true) => Ok(Self),
            _ => Err("could not install the keyboard hook".into()),
        }
    }

    /// Replaces the shortcuts; `hold` lists IDs of hold-to-talk ones.
    pub fn set(&self, bindings: &[Binding], hold: &[String]) {
        if let Some(s) = STATE.get() {
            s.matcher.lock().unwrap().set(bindings, hold);
        }
    }
}

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && let Some(state) = STATE.get()
    {
        // SAFETY: for WH_KEYBOARD_LL, lparam points to a KBDLLHOOKSTRUCT.
        let kb = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        // Keys we type ourselves must not trigger shortcuts.
        if kb.flags & LLKHF_INJECTED == 0
            && let Some(name) = vk_name(kb.vkCode)
        {
            let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let out = state.matcher.lock().unwrap().feed(name, down);
            for e in out.events {
                let _ = state.tx.send(e);
            }
            if out.swallow {
                return 1;
            }
        }
    }
    // SAFETY: passing the event on, as the hook contract requires.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// The Combo key name of a virtual-key code.
fn vk_name(vk: u32) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v",
        "w", "x", "y", "z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const FKEYS: [&str; 24] = [
        "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12", "f13", "f14", "f15", "f16", "f17",
        "f18", "f19", "f20", "f21", "f22", "f23", "f24",
    ];
    Some(match vk {
        0x41..=0x5A => LETTERS[(vk - 0x41) as usize],
        0x30..=0x39 => DIGITS[(vk - 0x30) as usize],
        0x70..=0x87 => FKEYS[(vk - 0x70) as usize],
        0xA2 => "leftctrl",
        0xA3 => "rightctrl",
        0xA4 => "leftalt",
        0xA5 => "rightalt",
        0xA0 => "leftshift",
        0xA1 => "rightshift",
        0x5B => "leftmeta",
        0x5C => "rightmeta",
        0x20 => "space",
        0x0D => "enter",
        0x1B => "escape",
        0x09 => "tab",
        0x08 => "backspace",
        0x2D => "insert",
        0x2E => "delete",
        0x24 => "home",
        0x23 => "end",
        0x21 => "pageup",
        0x22 => "pagedown",
        0x25 => "left",
        0x26 => "up",
        0x27 => "right",
        0x28 => "down",
        0x13 => "pause",
        0x91 => "scrolllock",
        0x14 => "capslock",
        0xC0 => "`",
        0xBD => "-",
        0xBB => "=",
        0xDB => "[",
        0xDD => "]",
        0xDC => "\\",
        0xBA => ";",
        0xDE => "'",
        0xBC => ",",
        0xBE => ".",
        0xBF => "/",
        _ => return None,
    })
}
