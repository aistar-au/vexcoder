

pub fn bounded_incremental_suffix(existing: &str, incoming: &str) -> String {
    if incoming.is_empty() {
        return String::new();
    }

    
    let existing_len = existing.len();
    if incoming.len() > existing_len && incoming.as_bytes()[..existing_len] == *existing.as_bytes()
    {
        return incoming[existing_len..].to_string();
    }

    
    if existing_len >= incoming.len()
        && existing.as_bytes()[..incoming.len()] == *incoming.as_bytes()
    {
        return String::new();
    }

    
    incoming.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_suffix_extracts_new_text() {
        assert_eq!(bounded_incremental_suffix("hello", "hello world"), " world");
    }

    #[test]
    fn bounded_suffix_empty_incoming() {
        assert_eq!(bounded_incremental_suffix("hello", ""), "");
    }

    #[test]
    fn bounded_suffix_redundant_retransmission() {
        assert_eq!(bounded_incremental_suffix("hello world", "hello"), "");
    }

    #[test]
    fn bounded_suffix_no_overlap() {
        assert_eq!(bounded_incremental_suffix("aaa", "bbb"), "bbb");
    }
}
