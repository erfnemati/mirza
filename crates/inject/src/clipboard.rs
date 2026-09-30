//! Clipboard writes, marked so clipboard managers don't keep dictated text in
//! their history. On Linux, wl-copy is used when arboard can't reach the
//! Wayland clipboard (e.g. compositors without the data-control protocol).

use std::io;

pub struct Clipboard {
    inner: Option<arboard::Clipboard>,
}

impl Clipboard {
    pub fn new() -> Self {
        Self { inner: arboard::Clipboard::new().ok() }
    }

    pub fn set(&mut self, text: &str, also_primary: bool) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            use arboard::{LinuxClipboardKind, SetExtLinux};
            if let Some(c) = self.inner.as_mut() {
                let mut ok = c.set().exclude_from_history().clipboard(LinuxClipboardKind::Clipboard).text(text).is_ok();
                if ok && also_primary {
                    ok = c.set().exclude_from_history().clipboard(LinuxClipboardKind::Primary).text(text).is_ok();
                }
                if ok {
                    return Ok(());
                }
            }
            wl_copy(text, false)?;
            if also_primary {
                wl_copy(text, true)?;
            }
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            #[cfg(target_os = "macos")]
            use arboard::SetExtApple as _;
            #[cfg(windows)]
            use arboard::SetExtWindows as _;
            let _ = also_primary;
            let c = self.inner.as_mut().ok_or_else(|| io::Error::other("no clipboard"))?;
            c.set().exclude_from_history().text(text).map_err(io::Error::other)
        }
    }
}

impl Default for Clipboard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
fn wl_copy(text: &str, primary: bool) -> io::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("wl-copy");
    if primary {
        cmd.arg("--primary");
    }
    let mut child = cmd.stdin(Stdio::piped()).spawn()?;
    child.stdin.take().expect("piped stdin").write_all(text.as_bytes())?;
    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other(format!("wl-copy failed: {status}")));
    }
    Ok(())
}
