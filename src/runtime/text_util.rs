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
