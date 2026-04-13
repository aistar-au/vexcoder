/// Truncate `text` to at most `max_bytes` bytes from the **start**,
/// respecting UTF-8 char boundaries. Returns the possibly shortened
/// string and a flag indicating whether any bytes were omitted.
///
/// Used by `ContextAssembler` for file-snapshot content.
pub fn truncate_head_bytes(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    (text[..boundary].to_string(), true)
}

/// Truncate `text` to at most `max_bytes` bytes from the **end**,
/// respecting UTF-8 char boundaries. Returns the possibly shortened
/// string and a flag indicating whether any bytes were omitted.
///
/// Used by `ValidationSuite` for stdout/stderr trailing-output capture.
pub fn truncate_tail_bytes(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let mut start = text.len().saturating_sub(max_bytes);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    (text[start..].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_head_bytes_no_limit_needed() {
        let (result, was_limited) = truncate_head_bytes("hello", 10);
        assert_eq!(result, "hello");
        assert!(!was_limited);
    }

    #[test]
    fn test_truncate_head_bytes_limits_at_boundary() {
        let (result, was_limited) = truncate_head_bytes("hello world", 5);
        assert_eq!(result, "hello");
        assert!(was_limited);
    }

    #[test]
    fn test_truncate_head_bytes_exact_boundary() {
        let (result, was_limited) = truncate_head_bytes("hello", 5);
        assert_eq!(result, "hello");
        assert!(!was_limited);
    }

    #[test]
    fn test_truncate_tail_bytes_no_limit_needed() {
        let (result, was_limited) = truncate_tail_bytes("hello", 10);
        assert_eq!(result, "hello");
        assert!(!was_limited);
    }

    #[test]
    fn test_truncate_tail_bytes_limits_at_boundary() {
        let (result, was_limited) = truncate_tail_bytes("hello world", 5);
        assert_eq!(result, "world");
        assert!(was_limited);
    }

    #[test]
    fn test_truncate_tail_bytes_exact_boundary() {
        let (result, was_limited) = truncate_tail_bytes("hello", 5);
        assert_eq!(result, "hello");
        assert!(!was_limited);
    }
}
