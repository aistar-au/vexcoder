pub fn extract_notes_from_turn(_input: &str, response: &str, max_notes: usize) -> Vec<String> {
    if max_notes == 0 {
        return Vec::new();
    }
    let mut notes: Vec<String> = Vec::new();
    let mut in_code_block = false;
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        let bullet_content = strip_bullet_prefix(trimmed);
        let is_bullet = bullet_content.is_some();
        let is_numbered = trimmed
            .split_once(". ")
            .map(|(prefix, _)| prefix.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(false);
        let is_key_value = trimmed.contains(": ")
            && !trimmed.starts_with("//")
            && !trimmed.contains("::")
            && !trimmed.contains('{')
            && !trimmed.contains('}')
            && !trimmed.contains('[')
            && !trimmed.contains(']')
            && !trimmed.contains('`')
            && !trimmed.contains(';');
        if is_bullet || is_numbered || is_key_value {
            let content = if is_bullet {
                bullet_content.unwrap_or(trimmed).trim().to_string()
            } else {
                trimmed.to_string()
            };
            let looks_structured = content.contains("```")
                || content.contains('{')
                || content.contains('}')
                || content.contains('[')
                || content.contains(']')
                || content.contains('`')
                || content.starts_with("fn ")
                || content.starts_with("let ")
                || content.starts_with("pub ")
                || content.starts_with("impl ")
                || content.contains("::")
                || content.contains(';');
            if !content.is_empty() && !looks_structured {
                notes.push(content);
            }
        }
        if notes.len() >= max_notes {
            break;
        }
    }
    notes
}

fn strip_bullet_prefix(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix("- ") {
        return Some(rest);
    }
    if let Some(rest) = line.strip_prefix("* ") {
        return Some(rest);
    }
    let mut chars = line.chars();
    let first = chars.next()?;
    let rest = chars.as_str();
    if !first.is_alphanumeric() && !first.is_whitespace() && rest.starts_with(' ') {
        return Some(rest.trim_start());
    }
    None
}

pub fn current_timestamp_seconds() -> u64 {
    chrono::Utc::now().timestamp().try_into().unwrap_or(0)
}

pub fn format_auto_notes(notes: &[String], timestamp_seconds: u64) -> Vec<String> {
    notes
        .iter()
        .map(|note| format!("[{timestamp_seconds}] [auto] {note}"))
        .collect()
}

pub fn is_auto_note_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return false;
    };
    let Some((timestamp, suffix)) = rest.split_once("] ") else {
        return false;
    };
    !timestamp.is_empty()
        && timestamp.chars().all(|c| c.is_ascii_digit())
        && suffix.starts_with("[auto] ")
}

pub fn append_auto_notes(notes: &[String], path: &std::path::Path) -> anyhow::Result<()> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    for note in notes {
        writeln!(file, "{note}")?;
    }
    Ok(())
}

pub fn remove_auto_notes(path: &std::path::Path) -> anyhow::Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let content = std::fs::read_to_string(path)?;
    let (kept, removed): (Vec<&str>, Vec<&str>) =
        content.lines().partition(|line| !is_auto_note_line(line));
    let removed_count = removed.len();
    let new_content = kept.join("\n");
    let new_content = if new_content.is_empty() {
        String::new()
    } else {
        format!("{new_content}\n")
    };
    std::fs::write(path, new_content)?;
    Ok(removed_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn test_extract_notes_empty_response_returns_nothing() {
        let notes = extract_notes_from_turn("input", "", 3);
        assert!(notes.is_empty());
    }

    #[test]
    fn test_extract_notes_respects_max_notes() {
        let response = "- fact one\n- fact two\n- fact three\n- fact four\n";
        let notes = extract_notes_from_turn("q", response, 2);
        assert_eq!(notes.len(), 2);
    }

    #[test]
    fn test_extract_notes_return_plain_text_notes() {
        let response = "- something important\n";
        let notes = extract_notes_from_turn("q", response, 3);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0], "something important");
    }

    #[test]
    fn test_extract_notes_handles_unicode_bullet_prefix() {
        let response = "• unicode bullet note\n";
        let notes = extract_notes_from_turn("q", response, 3);
        assert_eq!(notes, vec!["unicode bullet note".to_string()]);
    }

    #[test]
    fn test_extract_notes_skips_fenced_code_blocks() {
        let response = "```rust\n- not a note\nlet value = 1;\n```\n- real note\n";
        let notes = extract_notes_from_turn("q", response, 3);
        assert_eq!(notes, vec!["real note".to_string()]);
    }

    #[test]
    fn test_format_auto_notes_adds_timestamp_and_tag() {
        let notes = format_auto_notes(&["remember this".to_string()], 42);
        assert_eq!(notes, vec!["[42] [auto] remember this".to_string()]);
    }

    #[test]
    fn test_is_auto_note_line_matches_only_timestamped_auto_entries() {
        assert!(is_auto_note_line("[42] [auto] remember this"));
        assert!(!is_auto_note_line(
            "manual note mentioning [auto] in passing"
        ));
        assert!(!is_auto_note_line("[auto] older-format auto line"));
    }

    #[test]
    fn test_append_and_remove_auto_notes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("notes.md");
        append_auto_notes(&["[42] [auto] note one".to_string()], &path).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"manual note\n")
            .unwrap();
        append_auto_notes(&["[43] [auto] note two".to_string()], &path).unwrap();

        let removed = remove_auto_notes(&path).unwrap();
        assert_eq!(removed, 2, "should remove both [auto] lines");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("manual note"));
        assert!(!content.contains("[auto]"));
    }

    #[test]
    fn test_remove_auto_notes_nonexistent_returns_zero() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("no-such-file.md");
        let removed = remove_auto_notes(&path).unwrap();
        assert_eq!(removed, 0);
    }
}
