//! Turns raw key events into shortcut events, for platforms where Mirza sees
//! every key (Windows' keyboard hook, macOS' event tap).
//!
//! A combination (Ctrl+Alt+Space) is pressed when its key goes down with
//! exactly its modifiers held, and released when that key or one of the
//! modifiers comes up. A single modifier (RightCtrl) works differently, so that
//! using it for ordinary shortcuts (RightCtrl+C) never starts dictation:
//! - hold-to-talk: pressed when it goes down, cancelled if another key goes
//!   down while it is held, released when it comes up;
//! - toggle: pressed (and released) when it comes up, if no other key went
//!   down in between.

use crate::{Binding, Combo, Event};

/// Key names as Combo uses them: "h", "space", "f5", "leftctrl", "rightalt"…
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Default)]
struct Mods {
    ctrl: [bool; 2],
    alt: [bool; 2],
    shift: [bool; 2],
    meta: [bool; 2],
}

impl Mods {
    fn slot(&mut self, key: &str) -> Option<&mut bool> {
        let (side, name) = match key.strip_prefix("left") {
            Some(n) => (0, n),
            None => (1, key.strip_prefix("right")?),
        };
        Some(match name {
            "ctrl" => &mut self.ctrl[side],
            "alt" => &mut self.alt[side],
            "shift" => &mut self.shift[side],
            "meta" => &mut self.meta[side],
            _ => return None,
        })
    }

    fn matches(&self, c: &Combo) -> bool {
        let any = |m: &[bool; 2]| m[0] || m[1];
        any(&self.ctrl) == c.ctrl && any(&self.alt) == c.alt && any(&self.shift) == c.shift && any(&self.meta) == c.meta
    }

    fn none(&self) -> bool {
        [self.ctrl, self.alt, self.shift, self.meta].iter().all(|m| !m[0] && !m[1])
    }
}

struct Entry {
    id: String,
    combo: Combo,
    hold: bool,
    active: bool,
    /// Lone modifier: another key went down while it was held.
    dirty: bool,
}

#[derive(Default)]
pub struct Matcher {
    entries: Vec<Entry>,
    mods: Mods,
    /// Non-modifier keys currently down, to tell repeats apart.
    down: Vec<String>,
}

pub struct Outcome {
    pub events: Vec<Event>,
    /// Keep the key from reaching the focused app.
    pub swallow: bool,
}

impl Matcher {
    /// Sets the shortcuts; `hold` lists IDs of hold-to-talk shortcuts.
    pub fn set(&mut self, bindings: &[Binding], hold: &[String]) {
        self.entries = bindings
            .iter()
            .filter_map(|b| {
                let combo = Combo::parse(&b.keys).ok().filter(Combo::is_safe_global)?;
                Some(Entry { id: b.id.clone(), combo, hold: hold.contains(&b.id), active: false, dirty: false })
            })
            .collect();
    }

    /// Feeds one key event. `key` is a Combo key name.
    pub fn feed(&mut self, key: &str, down: bool) -> Outcome {
        let key = crate::normalize_key(key);
        let mut out = Outcome { events: Vec::new(), swallow: false };
        let is_mod = self.mods.slot(&key).is_some();

        if is_mod {
            let was_down = *self.mods.slot(&key).expect("modifier");
            if down && was_down {
                return out; // repeat
            }
            let others_idle = self.down.is_empty() && self.mods.none();
            *self.mods.slot(&key).expect("modifier") = down;
            for e in self.entries.iter_mut() {
                if e.combo.is_lone_modifier() && e.combo.key == key {
                    if down && others_idle {
                        e.active = true;
                        e.dirty = false;
                        if e.hold {
                            out.events.push(Event::Pressed(e.id.clone()));
                        }
                    } else if !down && e.active {
                        e.active = false;
                        if e.hold && !e.dirty {
                            out.events.push(Event::Released(e.id.clone()));
                        } else if !e.hold && !e.dirty {
                            out.events.push(Event::Pressed(e.id.clone()));
                            out.events.push(Event::Released(e.id.clone()));
                        }
                    }
                } else if e.combo.is_lone_modifier() && e.active && down {
                    // Another modifier joined: this is an ordinary shortcut.
                    Self::interrupt(e, &mut out);
                } else if !e.combo.is_lone_modifier() && e.active && !down && !self.mods.matches(&e.combo) {
                    e.active = false;
                    out.events.push(Event::Released(e.id.clone()));
                }
            }
            return out;
        }

        if down {
            if self.down.contains(&key) {
                // Repeat: keep swallowing the keys of an active combination.
                out.swallow =
                    self.entries.iter().any(|e| e.active && !e.combo.is_lone_modifier() && e.combo.key == key);
                return out;
            }
            self.down.push(key.clone());
            for e in self.entries.iter_mut() {
                if e.combo.is_lone_modifier() {
                    if e.active {
                        Self::interrupt(e, &mut out);
                    }
                } else if e.combo.key == key && self.mods.matches(&e.combo) {
                    e.active = true;
                    out.swallow = true;
                    out.events.push(Event::Pressed(e.id.clone()));
                }
            }
        } else {
            self.down.retain(|k| *k != key);
            for e in self.entries.iter_mut() {
                if !e.combo.is_lone_modifier() && e.active && e.combo.key == key {
                    e.active = false;
                    out.swallow = true;
                    out.events.push(Event::Released(e.id.clone()));
                }
            }
        }
        out
    }

    fn interrupt(e: &mut Entry, out: &mut Outcome) {
        if !e.dirty {
            e.dirty = true;
            if e.hold {
                out.events.push(Event::Cancelled(e.id.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher(list: &[(&str, &str, bool)]) -> Matcher {
        let bindings: Vec<Binding> = list
            .iter()
            .map(|(id, keys, _)| Binding { id: (*id).into(), description: String::new(), keys: (*keys).into() })
            .collect();
        let hold: Vec<String> = list.iter().filter(|(_, _, h)| *h).map(|(id, _, _)| (*id).into()).collect();
        let mut m = Matcher::default();
        m.set(&bindings, &hold);
        m
    }

    fn run(m: &mut Matcher, keys: &[(&str, bool)]) -> Vec<Event> {
        keys.iter().flat_map(|(k, d)| m.feed(k, *d).events).collect()
    }

    fn p(id: &str) -> Event {
        Event::Pressed(id.into())
    }
    fn r(id: &str) -> Event {
        Event::Released(id.into())
    }

    #[test]
    fn combination_press_and_release() {
        let mut m = matcher(&[("d", "Ctrl+Alt+Space", true)]);
        let ev =
            run(&mut m, &[("leftctrl", true), ("leftalt", true), ("space", true), ("space", true), ("space", false)]);
        assert_eq!(ev, [p("d"), r("d")]);
        // Letting go of a modifier first also releases it.
        let ev = run(&mut m, &[("space", true), ("leftalt", false), ("space", false), ("leftctrl", false)]);
        assert_eq!(ev, [p("d"), r("d")]);
    }

    #[test]
    fn extra_modifiers_do_not_match() {
        let mut m = matcher(&[("d", "Meta+H", false)]);
        assert!(run(&mut m, &[("leftmeta", true), ("leftshift", true), ("h", true), ("h", false)]).is_empty());
    }

    #[test]
    fn combination_keys_are_swallowed() {
        let mut m = matcher(&[("d", "Meta+H", false)]);
        m.feed("leftmeta", true);
        assert!(m.feed("h", true).swallow);
        assert!(!m.feed("j", true).swallow);
    }

    #[test]
    fn lone_modifier_hold() {
        let mut m = matcher(&[("d", "RightCtrl", true)]);
        assert_eq!(run(&mut m, &[("rightctrl", true), ("rightctrl", true), ("rightctrl", false)]), [p("d"), r("d")]);
        // RightCtrl+C is an ordinary copy: cancelled, and no release follows.
        let ev = run(&mut m, &[("rightctrl", true), ("c", true), ("c", false), ("rightctrl", false)]);
        assert_eq!(ev, [p("d"), Event::Cancelled("d".into())]);
        // The left key is a different key.
        assert!(run(&mut m, &[("leftctrl", true), ("leftctrl", false)]).is_empty());
    }

    #[test]
    fn lone_modifier_toggle_fires_on_a_clean_tap() {
        let mut m = matcher(&[("t", "RightAlt", false)]);
        assert_eq!(run(&mut m, &[("rightalt", true), ("rightalt", false)]), [p("t"), r("t")]);
        assert!(run(&mut m, &[("rightalt", true), ("x", true), ("x", false), ("rightalt", false)]).is_empty());
        // Pressed while other keys are down: not a tap on its own.
        assert!(
            run(&mut m, &[("leftshift", true), ("rightalt", true), ("rightalt", false), ("leftshift", false)])
                .is_empty()
        );
    }

    #[test]
    fn mac_names_match() {
        let mut m = matcher(&[("d", "RightOption", true)]);
        assert_eq!(run(&mut m, &[("rightalt", true), ("rightalt", false)]), [p("d"), r("d")]);
        let mut m = matcher(&[("c", "Cmd+Shift+D", false)]);
        assert_eq!(
            run(&mut m, &[("leftmeta", true), ("rightshift", true), ("d", true), ("d", false)]),
            [p("c"), r("c")]
        );
    }
}
