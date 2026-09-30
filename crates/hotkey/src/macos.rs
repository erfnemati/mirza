//! Global shortcuts on macOS through a Quartz event tap, which sees every key,
//! so single modifiers (Right Option) work too. It needs the Accessibility
//! permission; until it is granted the tap can't be created, so we ask once
//! and retry every two seconds. Ported from the Swift app's HotkeyMonitor.

use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_foundation::string::CFString;
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType, CallbackResult, EventField,
};
use tokio::sync::mpsc;

use crate::matcher::Matcher;
use crate::{Binding, Event};

struct State {
    matcher: Mutex<Matcher>,
    tx: mpsc::UnboundedSender<Event>,
}

static STATE: OnceLock<State> = OnceLock::new();
/// The tap's mach port, to switch it back on when macOS disables it.
static TAP: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef) -> u8;
    fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
}

/// Asks macOS for the Accessibility permission (shows its prompt once).
/// Returns whether it is granted now.
pub fn request_access() -> bool {
    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
    // SAFETY: a valid options dictionary.
    unsafe { AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef()) != 0 }
}

pub struct Hotkeys;

impl Hotkeys {
    /// Starts the event tap on its own thread (once per process).
    pub fn start(tx: mpsc::UnboundedSender<Event>) -> Result<Self, String> {
        if STATE.set(State { matcher: Mutex::new(Matcher::default()), tx }).is_err() {
            return Ok(Self);
        }
        std::thread::Builder::new().name("mirza-hotkeys".into()).spawn(run).map_err(|e| e.to_string())?;
        Ok(Self)
    }

    pub fn set(&self, bindings: &[Binding], hold: &[String]) {
        if let Some(s) = STATE.get() {
            s.matcher.lock().unwrap().set(bindings, hold);
        }
    }
}

fn run() {
    let mut asked = false;
    loop {
        let tap = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            vec![CGEventType::KeyDown, CGEventType::KeyUp, CGEventType::FlagsChanged],
            callback,
        );
        match tap {
            Ok(tap) => {
                TAP.store(tap.mach_port().as_concrete_TypeRef() as *mut _, Ordering::SeqCst);
                let source = tap.mach_port().create_runloop_source(0).expect("run loop source");
                // SAFETY: adding a valid source to this thread's run loop.
                CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
                tap.enable();
                CFRunLoop::run_current();
                return;
            }
            Err(()) => {
                // No Accessibility permission yet.
                if !asked {
                    request_access();
                    asked = true;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn callback(
    _proxy: core_graphics::event::CGEventTapProxy,
    kind: CGEventType,
    event: &core_graphics::event::CGEvent,
) -> CallbackResult {
    let Some(state) = STATE.get() else { return CallbackResult::Keep };
    let (name, down) = match kind {
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            let tap = TAP.load(Ordering::SeqCst);
            if !tap.is_null() {
                // SAFETY: the tap's own mach port, which lives as long as the thread.
                unsafe { CGEventTapEnable(tap, true) };
            }
            return CallbackResult::Keep;
        }
        CGEventType::KeyDown | CGEventType::KeyUp => {
            // Keys we type ourselves come from another event source state.
            if event.get_integer_value_field(EventField::EVENT_SOURCE_STATE_ID) != 1 {
                return CallbackResult::Keep;
            }
            let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
            let Some(name) = key_name(code) else { return CallbackResult::Keep };
            (name, matches!(kind, CGEventType::KeyDown))
        }
        CGEventType::FlagsChanged => {
            let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
            let Some((name, mask)) = modifier(code) else { return CallbackResult::Keep };
            (name, event.get_flags().bits() & mask != 0)
        }
        _ => return CallbackResult::Keep,
    };
    let out = state.matcher.lock().unwrap().feed(name, down);
    for e in out.events {
        let _ = state.tx.send(e);
    }
    if out.swallow { CallbackResult::Drop } else { CallbackResult::Keep }
}

/// Modifier keys and their device-dependent flag bits, so left and right can
/// be told apart.
fn modifier(code: i64) -> Option<(&'static str, u64)> {
    Some(match code {
        61 => ("rightalt", 0x40),
        58 => ("leftalt", 0x20),
        54 => ("rightmeta", 0x10),
        55 => ("leftmeta", 0x08),
        62 => ("rightctrl", 0x2000),
        59 => ("leftctrl", 0x01),
        60 => ("rightshift", 0x04),
        56 => ("leftshift", 0x02),
        63 => ("fn", 1 << 23),
        _ => return None,
    })
}

/// The Combo key name of a macOS virtual key code (ANSI positions).
fn key_name(code: i64) -> Option<&'static str> {
    Some(match code {
        0 => "a",
        1 => "s",
        2 => "d",
        3 => "f",
        4 => "h",
        5 => "g",
        6 => "z",
        7 => "x",
        8 => "c",
        9 => "v",
        11 => "b",
        12 => "q",
        13 => "w",
        14 => "e",
        15 => "r",
        16 => "y",
        17 => "t",
        18 => "1",
        19 => "2",
        20 => "3",
        21 => "4",
        22 => "6",
        23 => "5",
        24 => "=",
        25 => "9",
        26 => "7",
        27 => "-",
        28 => "8",
        29 => "0",
        30 => "]",
        31 => "o",
        32 => "u",
        33 => "[",
        34 => "i",
        35 => "p",
        36 => "enter",
        37 => "l",
        38 => "j",
        39 => "'",
        40 => "k",
        41 => ";",
        42 => "\\",
        43 => ",",
        44 => "/",
        45 => "n",
        46 => "m",
        47 => ".",
        48 => "tab",
        49 => "space",
        50 => "`",
        51 => "backspace",
        53 => "escape",
        122 => "f1",
        120 => "f2",
        99 => "f3",
        118 => "f4",
        96 => "f5",
        97 => "f6",
        98 => "f7",
        100 => "f8",
        101 => "f9",
        109 => "f10",
        103 => "f11",
        111 => "f12",
        105 => "f13",
        107 => "f14",
        113 => "f15",
        115 => "home",
        116 => "pageup",
        117 => "delete",
        119 => "end",
        121 => "pagedown",
        123 => "left",
        124 => "right",
        125 => "down",
        126 => "up",
        _ => return None,
    })
}
