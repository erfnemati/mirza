//! Tray pictures as raw RGBA, drawn by scripts/icons.py. They are sent to the
//! tray as pixels rather than by icon name, so desktops can't show a stale
//! cached copy.

/// The picture for a state (see TrayView::icon) at one of the sizes this
/// platform uses.
#[cfg(target_os = "linux")]
pub fn rgba(state: &str, size: u32) -> &'static [u8] {
    match (state, size) {
        ("idle", 22) => include_bytes!("../../../../assets/icons/tray/idle-22.rgba"),
        ("busy", 22) => include_bytes!("../../../../assets/icons/tray/busy-22.rgba"),
        ("error", 22) => include_bytes!("../../../../assets/icons/tray/error-22.rgba"),
        ("rec0", 22) => include_bytes!("../../../../assets/icons/tray/rec0-22.rgba"),
        ("rec1", 22) => include_bytes!("../../../../assets/icons/tray/rec1-22.rgba"),
        ("rec2", 22) => include_bytes!("../../../../assets/icons/tray/rec2-22.rgba"),
        ("rec3", 22) => include_bytes!("../../../../assets/icons/tray/rec3-22.rgba"),
        ("rec4", 22) => include_bytes!("../../../../assets/icons/tray/rec4-22.rgba"),
        ("rec5", 22) => include_bytes!("../../../../assets/icons/tray/rec5-22.rgba"),
        ("idle", 44) => include_bytes!("../../../../assets/icons/tray/idle-44.rgba"),
        ("busy", 44) => include_bytes!("../../../../assets/icons/tray/busy-44.rgba"),
        ("error", 44) => include_bytes!("../../../../assets/icons/tray/error-44.rgba"),
        ("rec0", 44) => include_bytes!("../../../../assets/icons/tray/rec0-44.rgba"),
        ("rec1", 44) => include_bytes!("../../../../assets/icons/tray/rec1-44.rgba"),
        ("rec2", 44) => include_bytes!("../../../../assets/icons/tray/rec2-44.rgba"),
        ("rec3", 44) => include_bytes!("../../../../assets/icons/tray/rec3-44.rgba"),
        ("rec4", 44) => include_bytes!("../../../../assets/icons/tray/rec4-44.rgba"),
        ("rec5", 44) => include_bytes!("../../../../assets/icons/tray/rec5-44.rgba"),
        _ => include_bytes!("../../../../assets/icons/tray/idle-22.rgba"),
    }
}

#[cfg(windows)]
pub fn rgba(state: &str, size: u32) -> &'static [u8] {
    match (state, size) {
        ("idle", 32) => include_bytes!("../../../../assets/icons/tray/idle-32.rgba"),
        ("busy", 32) => include_bytes!("../../../../assets/icons/tray/busy-32.rgba"),
        ("error", 32) => include_bytes!("../../../../assets/icons/tray/error-32.rgba"),
        ("rec0", 32) => include_bytes!("../../../../assets/icons/tray/rec0-32.rgba"),
        ("rec1", 32) => include_bytes!("../../../../assets/icons/tray/rec1-32.rgba"),
        ("rec2", 32) => include_bytes!("../../../../assets/icons/tray/rec2-32.rgba"),
        ("rec3", 32) => include_bytes!("../../../../assets/icons/tray/rec3-32.rgba"),
        ("rec4", 32) => include_bytes!("../../../../assets/icons/tray/rec4-32.rgba"),
        ("rec5", 32) => include_bytes!("../../../../assets/icons/tray/rec5-32.rgba"),
        _ => include_bytes!("../../../../assets/icons/tray/idle-32.rgba"),
    }
}

/// macOS menu bar template images (black; the system recolours them).
#[cfg(target_os = "macos")]
pub fn rgba(state: &str, size: u32) -> &'static [u8] {
    match (state, size) {
        ("idle", 36) => include_bytes!("../../../../assets/icons/tray/mac-idle-36.rgba"),
        ("busy", 36) => include_bytes!("../../../../assets/icons/tray/mac-busy-36.rgba"),
        ("error", 36) => include_bytes!("../../../../assets/icons/tray/mac-error-36.rgba"),
        ("rec0", 36) => include_bytes!("../../../../assets/icons/tray/mac-rec0-36.rgba"),
        ("rec1", 36) => include_bytes!("../../../../assets/icons/tray/mac-rec1-36.rgba"),
        ("rec2", 36) => include_bytes!("../../../../assets/icons/tray/mac-rec2-36.rgba"),
        ("rec3", 36) => include_bytes!("../../../../assets/icons/tray/mac-rec3-36.rgba"),
        ("rec4", 36) => include_bytes!("../../../../assets/icons/tray/mac-rec4-36.rgba"),
        ("rec5", 36) => include_bytes!("../../../../assets/icons/tray/mac-rec5-36.rgba"),
        _ => include_bytes!("../../../../assets/icons/tray/mac-idle-36.rgba"),
    }
}
