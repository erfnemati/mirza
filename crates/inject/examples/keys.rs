//! Test helper: a virtual keyboard that holds Ctrl+Alt+Shift+F9 on command.
//! Reads lines from stdin: "hold <ms>" or "quit".

#[cfg(target_os = "linux")]
use std::io::BufRead;
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use mirza_inject::linux::VirtualKeyboard;

#[cfg(target_os = "linux")]
fn main() {
    let mut kb = VirtualKeyboard::new("Mirza test keyboard").expect("uinput");
    let codes = [29u16, 56, 42, 67]; // Ctrl, Alt, Shift, F9
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap_or_default();
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next().and_then(|v| v.parse::<u64>().ok())) {
            (Some("hold"), Some(ms)) => {
                codes.iter().for_each(|&c| kb.set_key(c, true).unwrap());
                std::thread::sleep(Duration::from_millis(ms));
                codes.iter().rev().for_each(|&c| kb.set_key(c, false).unwrap());
            }
            (Some("quit"), _) => break,
            _ => {}
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("this example is for Linux");
}
