//! Shared helpers used by multiple modules. Kept narrow on purpose — only
//! genuinely cross-cutting utilities live here. Module-specific helpers stay
//! next to their use sites.

/// Trim `s` to at most `max_bytes`, backing up to the nearest UTF-8 char
/// boundary. Never panics on multi-byte chars (em-dashes, smart quotes from
/// LLM output are common at trim points). Returns the original slice when
/// already under the limit.
pub fn truncate_to_byte_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_to_byte_boundary_returns_input_when_under_limit() {
        assert_eq!(truncate_to_byte_boundary("hello", 10), "hello");
        assert_eq!(truncate_to_byte_boundary("hello", 5), "hello");
    }

    #[test]
    fn truncate_to_byte_boundary_trims_to_exact_boundary() {
        assert_eq!(truncate_to_byte_boundary("abcdef", 3), "abc");
    }

    #[test]
    fn truncate_to_byte_boundary_backs_up_to_char_boundary() {
        // "—" is 3 bytes in UTF-8 (E2 80 94). Asking for max_bytes=2 in the
        // middle of that sequence must back up to before the em-dash, not
        // panic.
        let s = "a—b";
        // 'a' (1) + '—' (3) + 'b' (1) = 5 bytes
        // max_bytes=2 lands inside the em-dash → back up to 1 (after 'a').
        let out = truncate_to_byte_boundary(s, 2);
        assert_eq!(out, "a");
    }

    #[test]
    fn truncate_to_byte_boundary_handles_empty_string() {
        assert_eq!(truncate_to_byte_boundary("", 10), "");
        assert_eq!(truncate_to_byte_boundary("", 0), "");
    }

    #[test]
    fn truncate_to_byte_boundary_handles_zero_budget() {
        assert_eq!(truncate_to_byte_boundary("hello", 0), "");
    }
}
