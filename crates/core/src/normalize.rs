//! Text clean-up before typing.

/// Writes Arabic letters and digits that providers sometimes return as their
/// Persian forms: ي ى → ی, ك → ک, ٠-٩ → ۰-۹.
pub fn persian_letters(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{064a}' | '\u{0649}' => '\u{06cc}',
            '\u{0643}' => '\u{06a9}',
            '\u{0660}'..='\u{0669}' => char::from_u32(c as u32 - 0x0660 + 0x06f0).unwrap_or(c),
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arabic_forms_become_persian() {
        assert_eq!(persian_letters("كتاب علي ٢٠٢٦"), "کتاب علی ۲۰۲۶");
        assert_eq!(persian_letters("hello سلام"), "hello سلام");
    }
}
