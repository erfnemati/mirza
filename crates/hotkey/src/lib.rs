//! Global shortcuts. Each binding has an ID and a key description such as
//! "Meta+H" or "Ctrl+Alt+Space"; pressing and releasing it produces events
//! with that ID, so the daemon can tell a tap from a hold.

#[cfg(target_os = "macos")]
pub mod macos;
pub mod matcher;
#[cfg(target_os = "linux")]
pub mod portal;
#[cfg(windows)]
pub mod windows;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Pressed(String),
    Released(String),
    /// The user gave a shortcut other keys in the desktop's settings:
    /// (id, new key description).
    Rebound(String, String),
    /// A single-modifier hold turned out to be part of an ordinary shortcut
    /// (e.g. RightCtrl+C): whatever it started should stop.
    Cancelled(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub id: String,
    pub description: String,
    /// e.g. "Meta+H"; may be empty when the desktop decides the key.
    pub keys: String,
}

/// A parsed key combination.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    /// The main key, lowercased ("h", "space", "f5"), or a lone modifier
    /// ("rightctrl", "rightalt", "leftmeta"...) when there are no others.
    pub key: String,
}

impl Combo {
    pub fn parse(s: &str) -> Result<Self, String> {
        let mut c = Combo::default();
        let parts: Vec<String> = s.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            return Err("no key given".into());
        }
        let (last, mods) = parts.split_last().expect("non-empty");
        for m in mods {
            match m.as_str() {
                "ctrl" | "control" => c.ctrl = true,
                "alt" | "option" | "opt" => c.alt = true,
                "shift" => c.shift = true,
                "meta" | "super" | "win" | "cmd" | "command" | "logo" => c.meta = true,
                other => return Err(format!("{other:?} is not a modifier")),
            }
        }
        c.key = normalize_key(last);
        Ok(c)
    }

    /// Whether two descriptions mean the same keys ("Meta+H" = "meta + h").
    pub fn same_keys(a: &str, b: &str) -> bool {
        matches!((Combo::parse(a), Combo::parse(b)), (Ok(x), Ok(y)) if x == y)
    }

    /// Whether this is a single modifier key like "RightCtrl".
    pub fn is_lone_modifier(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.meta && lone_modifier(&self.key)
    }

    /// The trigger in the XDG "shortcuts" format used by the portal, e.g.
    /// "LOGO+h". None for keys the portal can't express (lone modifiers).
    pub fn xdg_trigger(&self) -> Option<String> {
        if self.is_lone_modifier() {
            return None;
        }
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "CTRL"), (self.alt, "ALT"), (self.shift, "SHIFT"), (self.meta, "LOGO")] {
            if on {
                s.push_str(name);
                s.push('+');
            }
        }
        let key = match self.key.as_str() {
            "space" => "space".to_owned(),
            "enter" => "Return".to_owned(),
            "escape" => "Escape".to_owned(),
            "tab" => "Tab".to_owned(),
            k if k.len() > 1 && k.starts_with('f') && k[1..].parse::<u8>().is_ok() => k.to_uppercase(),
            k => k.to_owned(),
        };
        s.push_str(&key);
        Some(s)
    }
}

/// One name per key: "RightOption" and "rightalt" are the same key, as are
/// "LeftCmd" and "leftmeta".
pub fn normalize_key(k: &str) -> String {
    let k = k.trim().to_lowercase();
    match k.as_str() {
        "esc" => "escape".into(),
        "return" => "enter".into(),
        " " => "space".into(),
        "rightoption" => "rightalt".into(),
        "leftoption" => "leftalt".into(),
        "rightcmd" | "rightcommand" | "rightsuper" | "rightwin" => "rightmeta".into(),
        "leftcmd" | "leftcommand" | "leftsuper" | "leftwin" => "leftmeta".into(),
        "rightcontrol" => "rightctrl".into(),
        "leftcontrol" => "leftctrl".into(),
        _ => k,
    }
}

fn lone_modifier(k: &str) -> bool {
    matches!(
        k,
        "leftctrl"
            | "rightctrl"
            | "leftalt"
            | "rightalt"
            | "leftshift"
            | "rightshift"
            | "leftmeta"
            | "rightmeta"
            | "fn"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combos() {
        let c = Combo::parse("Meta+H").unwrap();
        assert!(c.meta && !c.ctrl);
        assert_eq!(c.xdg_trigger().as_deref(), Some("LOGO+h"));
        assert_eq!(Combo::parse("ctrl + alt + Space").unwrap().xdg_trigger().as_deref(), Some("CTRL+ALT+space"));
        assert_eq!(Combo::parse("Shift+F5").unwrap().xdg_trigger().as_deref(), Some("SHIFT+F5"));
        let lone = Combo::parse("RightCtrl").unwrap();
        assert!(lone.is_lone_modifier());
        assert_eq!(lone.xdg_trigger(), None);
        assert!(Combo::parse("h+Meta").is_err());
        assert!(Combo::same_keys("Meta+H", "meta + h"));
        assert!(!Combo::same_keys("Meta+H", "Meta+J"));
        assert!(Combo::parse("").is_err());
    }
}
