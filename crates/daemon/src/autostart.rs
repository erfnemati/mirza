//! Starting Mirza at login, following the `start_on_login` setting: an
//! autostart entry on Linux, a Run registry value on Windows, a LaunchAgent on
//! macOS.

#[cfg(unix)]
const ID: &str = "io.github.erfnemati.Mirza";

/// Creates or removes the login entry. Development builds (run from a cargo
/// target directory) are left alone so they don't replace the installed app.
pub fn sync(enabled: bool) {
    let Ok(exe) = std::env::current_exe() else { return };
    let path = exe.to_string_lossy().into_owned();
    let dev = ["/target/debug/", "/target/release/", "\\target\\debug\\", "\\target\\release\\"];
    if dev.iter().any(|d| path.contains(d)) {
        return;
    }
    if let Err(e) = platform(enabled, &path) {
        tracing::warn!("can't update start on login: {e}");
    }
}

#[cfg(target_os = "linux")]
fn platform(enabled: bool, exe: &str) -> std::io::Result<()> {
    use std::path::PathBuf;
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("autostart"))
        .ok_or_else(|| std::io::Error::other("no config directory"))?;
    let file = dir.join(format!("{ID}.desktop"));
    if !enabled {
        let _ = std::fs::remove_file(&file);
        return Ok(());
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Mirza\nComment=Dictation in any app\nExec={exe}\nIcon={ID}\n\
         Terminal=false\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n"
    );
    if std::fs::read_to_string(&file).ok().as_deref() != Some(entry.as_str()) {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&file, entry)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn platform(enabled: bool, exe: &str) -> std::io::Result<()> {
    let home = std::env::var_os("HOME").ok_or_else(|| std::io::Error::other("no home directory"))?;
    let dir = std::path::PathBuf::from(home).join("Library/LaunchAgents");
    let file = dir.join(format!("{ID}.plist"));
    if !enabled {
        let _ = std::fs::remove_file(&file);
        return Ok(());
    }
    let exe = exe.replace('&', "&amp;").replace('<', "&lt;");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{ID}</string>
    <key>ProgramArguments</key><array><string>{exe}</string></array>
    <key>RunAtLoad</key><true/>
    <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#
    );
    if std::fs::read_to_string(&file).ok().as_deref() != Some(plist.as_str()) {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&file, plist)?;
    }
    Ok(())
}

#[cfg(windows)]
fn platform(enabled: bool, exe: &str) -> std::io::Result<()> {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW,
    };
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let path = wide(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let name = wide("Mirza");
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: standard registry calls with NUL-terminated wide strings; the key
    // is closed before returning.
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_SET_VALUE, &mut key) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let status = if enabled {
            let value = wide(&format!("\"{exe}\""));
            RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, value.as_ptr().cast(), (value.len() * 2) as u32)
        } else {
            let s = RegDeleteValueW(key, name.as_ptr());
            if s == 2 { 0 } else { s } // ERROR_FILE_NOT_FOUND: already off
        };
        RegCloseKey(key);
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32));
        }
    }
    Ok(())
}
