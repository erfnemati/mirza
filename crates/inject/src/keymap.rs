//! Which key, on which XKB layout, types each character, and how to split text
//! into runs that are each typeable on one layout.

use std::collections::HashMap;
use std::sync::OnceLock;

pub const KEY_ESC: u16 = 1;
pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_TAB: u16 = 15;
pub const KEY_ENTER: u16 = 28;
pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_RIGHTALT: u16 = 100; // AltGr on layouts that use it
pub const KEY_INSERT: u16 = 110;
pub const KEY_LEFTMETA: u16 = 125;

/// A key plus the XKB level that produces a character:
/// 0 plain, 1 Shift, 2 AltGr, 3 Shift+AltGr.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyStroke {
    pub code: u16,
    pub level: u8,
}

/// Each row assigns each level's characters to consecutive key codes starting
/// at the given one. NUL marks an empty slot; the first mapping of a character
/// wins.
const ROWS: &[(&str, u16, &[&str])] = &[
    // US QWERTY.
    ("us", 2, &["1234567890-=", "!@#$%^&*()_+"]),
    ("us", 16, &["qwertyuiop[]", "QWERTYUIOP{}"]),
    ("us", 30, &["asdfghjkl;'`", "ASDFGHJKL:\"~"]),
    ("us", 43, &["\\zxcvbnm,./", "|ZXCVBNM<>?"]),
    ("us", 57, &[" "]),
    // Persian standard layout (ISIRI 9147), from /usr/share/X11/xkb/symbols/ir.
    (
        "ir",
        2,
        &[
            "\u{06f1}\u{06f2}\u{06f3}\u{06f4}\u{06f5}\u{06f6}\u{06f7}\u{06f8}\u{06f9}\u{06f0}-=",
            "!\u{066c}\u{066b}\u{fdfc}\u{066a}\u{00d7}\u{060c}*)(\u{0640}+",
            "`@#$%^&\u{2022}\u{0000}\u{0000}_\u{2212}",
            "1234567890",
        ],
    ),
    (
        "ir",
        16,
        &[
            "\u{0636}\u{0635}\u{062b}\u{0642}\u{0641}\u{063a}\u{0639}\u{0647}\u{062e}\u{062d}\u{062c}\u{0686}",
            "\u{0652}\u{064c}\u{064d}\u{064b}\u{064f}\u{0650}\u{064e}\u{0651}][}{",
            "\u{00b0}\u{0000}\u{20ac}",
        ],
    ),
    (
        "ir",
        30,
        &[
            "\u{0634}\u{0633}\u{06cc}\u{0628}\u{0644}\u{0627}\u{062a}\u{0646}\u{0645}\u{06a9}\u{06af}",
            "\u{0624}\u{0626}\u{064a}\u{0625}\u{0623}\u{0622}\u{0629}\u{00bb}\u{00ab}:\u{061b}",
            "\u{0000}\u{0000}\u{0649}\u{0000}\u{06c0}\u{0671}\u{0000}\u{fd3e}\u{fd3f};\"",
        ],
    ),
    ("ir", 41, &["\u{200d}", "\u{00f7}", "~"]),
    ("ir", 43, &["\\", "|", "\u{2010}"]),
    (
        "ir",
        44,
        &[
            "\u{0638}\u{0637}\u{0632}\u{0631}\u{0630}\u{062f}\u{067e}\u{0648}./",
            "\u{0643}\u{0653}\u{0698}\u{0670}\u{200c}\u{0654}\u{0621}><\u{061f}",
            "\u{0000}\u{0000}\u{0000}\u{0656}\u{200d}\u{0655}\u{2026},'?",
        ],
    ),
    ("ir", 57, &[" ", "\u{200c}", "\u{00a0}", "\u{202f}"]), // Shift+Space: ZWNJ (half-space)
];

type Keymap = HashMap<char, KeyStroke>;

fn keymaps() -> &'static HashMap<&'static str, Keymap> {
    static MAPS: OnceLock<HashMap<&'static str, Keymap>> = OnceLock::new();
    MAPS.get_or_init(|| {
        let mut maps: HashMap<&'static str, Keymap> = HashMap::new();
        for (layout, first, levels) in ROWS {
            let m = maps.entry(layout).or_default();
            for (level, chars) in levels.iter().enumerate() {
                for (i, c) in chars.chars().enumerate() {
                    if c != '\0' {
                        m.entry(c).or_insert(KeyStroke { code: first + i as u16, level: level as u8 });
                    }
                }
            }
        }
        for m in maps.values_mut() {
            m.insert('\t', KeyStroke { code: KEY_TAB, level: 0 });
            m.insert('\n', KeyStroke { code: KEY_ENTER, level: 0 });
        }
        maps
    })
}

/// The keystroke for `c` on `layout`, if the layout is known and has it.
pub fn lookup(layout: &str, c: char) -> Option<KeyStroke> {
    keymaps().get(layout)?.get(&c).copied()
}

/// Text typed on one layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub layout: usize,
    pub keys: Vec<KeyStroke>,
}

/// Splits `text` into runs that are each typeable on one of `layouts`, staying
/// on the current layout as long as possible. Returns None if some character
/// has no key on any layout, so the caller should paste instead.
pub fn plan(layouts: &[String], current: usize, text: &str) -> Option<Vec<Run>> {
    let mut runs: Vec<Run> = Vec::new();
    let mut cur = current;
    for c in text.chars() {
        let ks = match layouts.get(cur).and_then(|l| lookup(l, c)) {
            Some(ks) => ks,
            None => {
                let (i, ks) = layouts.iter().enumerate().find_map(|(i, l)| lookup(l, c).map(|ks| (i, ks)))?;
                cur = i;
                ks
            }
        };
        match runs.last_mut() {
            Some(r) if r.layout == cur => r.keys.push(ks),
            _ => runs.push(Run { layout: cur, keys: vec![ks] }),
        }
    }
    Some(runs)
}

/// Parses a key combination such as "ctrl+shift+v" or "shift+insert" into
/// Linux key codes, pressed in order.
pub fn parse_combo(combo: &str) -> Result<Vec<u16>, String> {
    let mut keys = Vec::new();
    for name in combo.to_lowercase().split('+') {
        let name = name.trim();
        let code = match name {
            "ctrl" | "control" => KEY_LEFTCTRL,
            "shift" => KEY_LEFTSHIFT,
            "alt" => KEY_LEFTALT,
            "meta" | "super" | "win" => KEY_LEFTMETA,
            "insert" => KEY_INSERT,
            "enter" | "return" => KEY_ENTER,
            "tab" => KEY_TAB,
            "esc" | "escape" => KEY_ESC,
            _ => {
                let mut chars = name.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => match lookup("us", c) {
                        Some(ks) if ks.level == 0 => ks.code,
                        _ => return Err(format!("unknown key {name:?}")),
                    },
                    _ => return Err(format!("unknown key {name:?}")),
                }
            }
        };
        keys.push(code);
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layouts() -> Vec<String> {
        vec!["us".into(), "ir".into()]
    }

    #[test]
    fn plan_matches_the_go_tool() {
        let (us, ir) = (0, 1);
        let cases: &[(&str, usize, Option<&[usize]>, usize)] = &[
            ("Hello, world!", us, Some(&[us]), 13),
            ("Hello, world!", ir, Some(&[us]), 13), // switches away from Persian
            ("سلام، می‌خواهم ۲ فایل را باز کنم.", ir, Some(&[ir]), 33),
            ("سلام، می‌خواهم ۲ فایل را باز کنم.", us, Some(&[ir]), 33),
            ("این فایل را در VS Code باز کن.", ir, Some(&[ir, us, ir]), 30),
            ("سال 2026 است", ir, Some(&[ir]), 12), // ASCII digits exist on the Persian layout too
            ("café", us, None, 0),
        ];
        for &(text, start, want, strokes) in cases {
            let runs = plan(&layouts(), start, text);
            match (runs, want) {
                (None, None) => {}
                (Some(runs), Some(want)) => {
                    let got: Vec<usize> = runs.iter().map(|r| r.layout).collect();
                    let n: usize = runs.iter().map(|r| r.keys.len()).sum();
                    assert_eq!((got.as_slice(), n), (want, strokes), "plan({text:?}) from {start}");
                }
                (got, want) => panic!("plan({text:?}) from {start} = {got:?}, want {want:?}"),
            }
        }
    }

    #[test]
    fn zwnj_is_shift_b_on_persian() {
        // The first mapping wins, so ZWNJ comes from Shift+B rather than Shift+Space.
        assert_eq!(lookup("ir", '\u{200c}'), Some(KeyStroke { code: 48, level: 1 }));
    }

    #[test]
    fn parses_paste_keys() {
        assert_eq!(parse_combo("shift+insert").unwrap(), vec![KEY_LEFTSHIFT, KEY_INSERT]);
        assert_eq!(parse_combo("Ctrl+Shift+V").unwrap(), vec![KEY_LEFTCTRL, KEY_LEFTSHIFT, 47]);
        assert!(parse_combo("ctrl+é").is_err());
    }
}
