//! Windows: types with SendInput Unicode packets, so any character works
//! regardless of keyboard layout. Windows won't let a normal app type into
//! one running as administrator; then the text goes to the clipboard.

use std::io;
use std::mem::size_of;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows_sys::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_BACK,
    VK_CONTROL, VK_INSERT, VK_MENU, VK_RETURN, VK_SHIFT, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use crate::clipboard::Clipboard;
use crate::{Options, Typed};

pub struct Backend {
    clip: Clipboard,
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn send(inputs: &[INPUT]) -> io::Result<()> {
    // SAFETY: a slice of initialised INPUT structs with the right size.
    let n = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
    if n as usize != inputs.len() {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Presses keys in order, then releases them in reverse order.
fn press(vks: &[VIRTUAL_KEY]) -> io::Result<()> {
    let mut inputs: Vec<INPUT> = vks.iter().map(|&vk| key(vk, 0, 0)).collect();
    inputs.extend(vks.iter().rev().map(|&vk| key(vk, 0, KEYEVENTF_KEYUP)));
    send(&inputs)
}

/// Whether the foreground window belongs to an elevated (administrator)
/// process, which ignores input from normal processes.
fn foreground_elevated() -> bool {
    // SAFETY: plain Win32 queries; handles are closed before returning.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return false;
        }
        let process: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut token: HANDLE = std::ptr::null_mut();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token) != 0;
        CloseHandle(process);
        if !opened {
            return true; // not allowed to look: it runs with higher rights
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        ) != 0;
        CloseHandle(token);
        ok && elevation.TokenIsElevated != 0 && !self_elevated()
    }
}

fn self_elevated() -> bool {
    // SAFETY: querying our own process token.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(windows_sys::Win32::System::Threading::GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        ) != 0;
        CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// The foreground window, as an ID for focus tracking.
pub fn foreground_window() -> String {
    // SAFETY: returns a window handle or null.
    let hwnd = unsafe { GetForegroundWindow() };
    format!("{:x}", hwnd as usize)
}

fn parse_combo(combo: &str) -> Result<Vec<VIRTUAL_KEY>, String> {
    combo
        .to_lowercase()
        .split('+')
        .map(|k| match k.trim() {
            "ctrl" | "control" => Ok(VK_CONTROL),
            "shift" => Ok(VK_SHIFT),
            "alt" => Ok(VK_MENU),
            "insert" => Ok(VK_INSERT),
            k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() => {
                Ok(k.to_ascii_uppercase().as_bytes()[0] as u16)
            }
            k => Err(format!("unknown key {k:?}")),
        })
        .collect()
}

impl Backend {
    pub fn new() -> io::Result<Self> {
        Ok(Self { clip: Clipboard::new() })
    }

    pub fn begin(&mut self) {}

    pub fn type_text(&mut self, text: &str, opts: &Options, ok: &dyn Fn() -> bool) -> io::Result<Typed> {
        if foreground_elevated() {
            return Err(io::Error::other(
                "the active window runs as administrator, so Windows won't let Mirza type into it",
            ));
        }
        let mut n = 0;
        let mut units = [0u16; 2];
        for c in text.chars() {
            if !ok() {
                break;
            }
            let inputs: Vec<INPUT> = match c {
                '\n' => vec![key(VK_RETURN, 0, 0), key(VK_RETURN, 0, KEYEVENTF_KEYUP)],
                '\t' => vec![key(VK_TAB, 0, 0), key(VK_TAB, 0, KEYEVENTF_KEYUP)],
                '\u{8}' => vec![key(VK_BACK, 0, 0), key(VK_BACK, 0, KEYEVENTF_KEYUP)],
                c => c
                    .encode_utf16(&mut units)
                    .iter()
                    .flat_map(|&u| [key(0, u, KEYEVENTF_UNICODE), key(0, u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)])
                    .collect(),
            };
            send(&inputs)?;
            n += 1;
            std::thread::sleep(opts.key_delay);
        }
        Ok(Typed::Chars(n))
    }

    pub fn paste(&mut self, text: &str, opts: &Options) -> io::Result<()> {
        let keys = parse_combo(&opts.paste_key).map_err(io::Error::other)?;
        self.clip.set(text, false)?;
        press(&keys)?;
        std::thread::sleep(Duration::from_millis(150));
        Ok(())
    }

    pub fn copy(&mut self, text: &str) -> io::Result<()> {
        self.clip.set(text, false)
    }

    pub fn restore(&mut self) {}

    pub fn end(&mut self) {}
}
