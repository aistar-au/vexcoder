/// Heuristic note extraction for automatic memory.
///
/// Scans the model response for lines that look like actionable facts or
/// decisions (lines starting with a dash/bullet, numbered items, or lines
/// that contain a colon-separated key/value).  Returns at most `max_notes`
/// entries, each tagged with the `[auto]` prefix so they can be selectively
/// removed later.
pub fn extract_notes_from_turn(_input: &str, response: &str, max_notes: usize) -> Vec<String> {
    if max_notes == 0 {
        return Vec::new();
    }
    let mut notes: Vec<String> = Vec::new();
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Accept lines that look like bullet points or fact-like statements.
        let is_bullet =
            trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ");
        let is_numbered = trimmed
            .split_once(". ")
            .map(|(prefix, _)| prefix.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(false);
        let is_key_value = trimmed.contains(": ") && !trimmed.starts_with("//");
        if is_bullet || is_numbered || is_key_value {
            let content = if is_bullet {
                trimmed[2..].trim().to_string()
            } else {
                trimmed.to_string()
            };
            if !content.is_empty() {
                notes.push(format!("[auto] {content}"));
            }
        }
        if notes.len() >= max_notes {
            break;
        }
    }
    notes
}

/// Append `notes` to the notes file at `path`, one entry per line.
///
/// Creates the file if it does not exist.  Returns an error if the write
/// fails.
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

/// Remove all lines that begin with `[auto]` from the notes file at `path`.
///
/// Returns the number of lines removed.  If the file does not exist, returns
/// `Ok(0)`.
pub fn remove_auto_notes(path: &std::path::Path) -> anyhow::Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let content = std::fs::read_to_string(path)?;
    let (kept, removed): (Vec<&str>, Vec<&str>) = content
        .lines()
        .partition(|line| !line.trim_start().starts_with("[auto]"));
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
    fn test_extract_notes_tags_with_auto_prefix() {
        let response = "- something important\n";
        let notes = extract_notes_from_turn("q", response, 3);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].starts_with("[auto]"), "expected [auto] prefix");
    }

    #[test]
    fn test_append_and_remove_auto_notes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("notes.md");
        append_auto_notes(&["[auto] note one".to_string()], &path).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"manual note\n")
            .unwrap();
        append_auto_notes(&["[auto] note two".to_string()], &path).unwrap();

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
