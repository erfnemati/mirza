//! A virtual keyboard on /dev/uinput. The compositor treats it like a real
//! keyboard, so it works on Wayland and X11 alike. It sends key positions, which
//! the compositor turns into characters with the active layout.
//!
//! The device is created once when the daemon starts: a compositor takes a
//! moment to pick up a new input device, and the first keys would be lost.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::time::Duration;

use crate::keymap::{KEY_LEFTSHIFT, KEY_RIGHTALT, KeyStroke};

const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
const UI_DEV_SETUP: libc::c_ulong = 0x405c5503; // _IOW('U', 3, struct uinput_setup)
const UI_SET_EVBIT: libc::c_ulong = 0x40045564;
const UI_SET_KEYBIT: libc::c_ulong = 0x40045565;
const EV_KEY: u16 = 0x01;
const MAX_KEY: libc::c_ulong = 248; // KEY_MICMUTE

pub struct VirtualKeyboard {
    file: File,
}

impl VirtualKeyboard {
    pub fn new(name: &str) -> io::Result<Self> {
        let file = OpenOptions::new().write(true).open("/dev/uinput")?;
        let fd = file.as_raw_fd();
        let ioctl = |req: libc::c_ulong, arg: libc::c_ulong| -> io::Result<()> {
            // SAFETY: plain uinput ioctls on a descriptor we own; arg is an integer.
            if unsafe { libc::ioctl(fd, req as _, arg) } < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        };
        ioctl(UI_SET_EVBIT, EV_KEY as libc::c_ulong)?;
        for code in 1..=MAX_KEY {
            ioctl(UI_SET_KEYBIT, code)?;
        }
        // struct uinput_setup { struct input_id id; char name[80]; __u32 ff_effects_max; }
        let mut setup = [0u8; 92];
        setup[0..2].copy_from_slice(&0x06u16.to_le_bytes()); // BUS_VIRTUAL
        let name = name.as_bytes();
        let n = name.len().min(79);
        setup[8..8 + n].copy_from_slice(&name[..n]);
        // SAFETY: setup is a correctly sized uinput_setup that outlives the call.
        if unsafe { libc::ioctl(fd, UI_DEV_SETUP as _, setup.as_ptr()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        ioctl(UI_DEV_CREATE, 0)?;
        Ok(Self { file })
    }

    /// Writes one key event followed by a SYN_REPORT.
    fn send(&mut self, code: u16, down: bool) -> io::Result<()> {
        // Two struct input_event (64-bit timeval); the second stays zero (EV_SYN).
        let mut ev = [0u8; 48];
        ev[16..18].copy_from_slice(&EV_KEY.to_le_bytes());
        ev[18..20].copy_from_slice(&code.to_le_bytes());
        if down {
            ev[20..24].copy_from_slice(&1i32.to_le_bytes());
        }
        self.file.write_all(&ev)
    }

    /// Presses keys in order, then releases them in reverse order.
    pub fn press(&mut self, keys: &[u16]) -> io::Result<()> {
        for &k in keys {
            self.send(k, true)?;
        }
        for &k in keys.iter().rev() {
            self.send(k, false)?;
        }
        Ok(())
    }

    /// Holds or releases one key (used by tests that simulate a held hotkey).
    pub fn set_key(&mut self, code: u16, down: bool) -> io::Result<()> {
        self.send(code, down)
    }

    /// Types one character given as a key and level.
    pub fn stroke(&mut self, ks: KeyStroke, delay: Duration) -> io::Result<()> {
        let mut keys = [0u16; 3];
        let mut n = 0;
        if ks.level & 1 != 0 {
            keys[n] = KEY_LEFTSHIFT;
            n += 1;
        }
        if ks.level & 2 != 0 {
            keys[n] = KEY_RIGHTALT;
            n += 1;
        }
        keys[n] = ks.code;
        self.press(&keys[..=n])?;
        std::thread::sleep(delay);
        Ok(())
    }
}

impl Drop for VirtualKeyboard {
    fn drop(&mut self) {
        // SAFETY: destroying the device we created on our own descriptor.
        unsafe { libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY as _, 0) };
    }
}
