/// Truncate a string to at most `max_chars` characters, respecting UTF-8
/// char boundaries. Returns a slice of the original string.
pub fn truncate_safe(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_within_limit() {
        assert_eq!(truncate_safe("hello", 10), "hello");
    }

    #[test]
    fn ascii_at_limit() {
        assert_eq!(truncate_safe("hello", 5), "hello");
    }

    #[test]
    fn ascii_truncated() {
        assert_eq!(truncate_safe("hello world", 5), "hello");
    }

    #[test]
    fn empty_string() {
        assert_eq!(truncate_safe("", 10), "");
    }

    #[test]
    fn zero_max() {
        assert_eq!(truncate_safe("hello", 0), "");
    }

    #[test]
    fn multibyte_em_dash_at_boundary() {
        // em-dash is U+2014, 3 bytes in UTF-8
        let s = "hello\u{2014}world";
        // Truncate at 6 chars: "hello" (5) + em-dash (1) = 6 chars
        assert_eq!(truncate_safe(s, 6), "hello\u{2014}");
        // Truncate at 5 chars: just "hello" — does NOT split the em-dash
        assert_eq!(truncate_safe(s, 5), "hello");
    }

    #[test]
    fn multibyte_smart_quotes() {
        // left double quote U+201C, right double quote U+201D — each 3 bytes
        let s = "\u{201C}quoted\u{201D} text";
        assert_eq!(truncate_safe(s, 1), "\u{201C}");
        assert_eq!(truncate_safe(s, 8), "\u{201C}quoted\u{201D}");
    }

    #[test]
    fn multibyte_cjk() {
        // CJK characters are 3 bytes each in UTF-8
        let s = "\u{4F60}\u{597D}\u{4E16}\u{754C}"; // 你好世界
        assert_eq!(truncate_safe(s, 2), "\u{4F60}\u{597D}");
        assert_eq!(truncate_safe(s, 4), s);
        assert_eq!(truncate_safe(s, 100), s);
    }

    #[test]
    fn multibyte_emoji_4byte() {
        // Emoji U+1F600 is 4 bytes in UTF-8
        let s = "ab\u{1F600}cd";
        assert_eq!(truncate_safe(s, 3), "ab\u{1F600}");
        assert_eq!(truncate_safe(s, 2), "ab");
    }

    #[test]
    fn realistic_finding_text() {
        // Simulates real finding text with em-dashes that triggered the panic
        let s = "Build 4321 performance — 97.2 tok/s — exceeds previous baseline by 1.9% due to delta-net contiguity optimizations";
        let result = truncate_safe(s, 80);
        assert_eq!(result.chars().count(), 80);
        // Must not panic, and result must be valid UTF-8 (it is, since it's a &str)
        assert!(result.is_char_boundary(result.len()));
    }
}
