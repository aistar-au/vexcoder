use super::*;

pub fn build_file_overlay(
    prefix: &str,
    matches: &[String],
    selected: usize,
) -> Vec<PickerOverlayLine> {
    if matches.is_empty() {
        let text = if prefix.is_empty() {
            "[file] type to search files".to_string()
        } else {
            format!("[file] no matches for {prefix}")
        };
        return vec![PickerOverlayLine {
            text,
            selected: false,
        }];
    }

    let mut lines = Vec::new();
    let selected = selected.min(matches.len().saturating_sub(1));
    let window = MAX_PICKER_OVERLAY_VISIBLE.min(matches.len());
    let start = selected
        .saturating_sub(window / 2)
        .min(matches.len().saturating_sub(window));
    let end = (start + window).min(matches.len());

    lines.push(PickerOverlayLine {
        text: format!(
            "[file] {} match(es) — Up/Down to navigate, Enter to select",
            matches.len()
        ),
        selected: false,
    });

    if start > 0 {
        lines.push(PickerOverlayLine {
            text: format!("  [{start} earlier]"),
            selected: false,
        });
    }

    for (offset, path) in matches[start..end].iter().enumerate() {
        let index = start + offset;
        let is_selected = index == selected;
        let marker = if is_selected { ">" } else { " " };
        lines.push(PickerOverlayLine {
            text: format!("{marker} {path}"),
            selected: is_selected,
        });
    }

    if end < matches.len() {
        lines.push(PickerOverlayLine {
            text: format!("  [{} more]", matches.len() - end),
            selected: false,
        });
    }

    lines
}

pub fn build_slash_overlay(
    matches: &[SlashPickerMatch],
    selected: usize,
) -> Vec<PickerOverlayLine> {
    if matches.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let selected = selected.min(matches.len().saturating_sub(1));
    let window = MAX_PICKER_OVERLAY_VISIBLE.min(matches.len());
    let start = selected
        .saturating_sub(window / 2)
        .min(matches.len().saturating_sub(window));
    let end = (start + window).min(matches.len());

    lines.push(PickerOverlayLine {
        text: format!("/ {} command(s)", matches.len()),
        selected: false,
    });

    for (offset, entry) in matches[start..end].iter().enumerate() {
        let index = start + offset;
        let is_selected = index == selected;
        let marker = if is_selected { ">" } else { " " };
        lines.push(PickerOverlayLine {
            text: format!("{marker} {}", entry.label),
            selected: is_selected,
        });
    }

    lines
}

pub fn active_file_picker(
    mode: &TuiMode,
    buffer: &str,
    cursor: usize,
) -> Option<FileMentionPickerState> {
    let range = file_mention_range(buffer, cursor)?;
    let token = &buffer[range.clone()];
    let prefix = token.strip_prefix('@')?.to_string();
    Some(FileMentionPickerState {
        range,
        prefix: prefix.clone(),
        matches: mode.file_prompt_matches(&prefix),
    })
}

pub fn file_picker_is_dismissed(
    dismissed: Option<&(String, Range<usize>)>,
    input: &str,
    cursor: usize,
) -> bool {
    dismissed.is_some_and(|(dismissed_input, dismissed_range)| {
        dismissed_input == input
            && file_mention_range(input, cursor)
                .as_ref()
                .is_some_and(|current_range| current_range == dismissed_range)
    })
}

pub fn render_file_picker_hint(prefix: &str, matches: &[String], selected: usize) -> String {
    let mut lines = vec!["Prompt".to_string(), "mode: file mention".to_string()];
    if matches.is_empty() {
        if prefix.is_empty() {
            lines.push("[file] no files available".to_string());
        } else {
            lines.push(format!("[file] no matches for {prefix}"));
        }
        return lines.join("\n");
    }

    lines.push(format!("[file] {} match(es)", matches.len()));
    let selected = selected.min(matches.len().saturating_sub(1));
    let window = 12.min(matches.len());
    let start = selected
        .saturating_sub(window / 2)
        .min(matches.len() - window);
    let end = (start + window).min(matches.len());

    if start > 0 {
        lines.push(format!("[file] {start} earlier match(es)"));
    }

    for (offset, path) in matches[start..end].iter().enumerate() {
        let index = start + offset;
        let marker = if index == selected { '>' } else { ' ' };
        lines.push(format!("{marker} [file] {path}"));
    }

    if end < matches.len() {
        lines.push(format!("[file] {} more match(es)", matches.len() - end));
    }
    lines.join("\n")
}

/// Extract the slash prefix token from input (e.g. "/ed" from "/ed something").
/// Returns `None` if the trimmed input does not start with `/`.
pub fn slash_prefix_token(input: &str) -> Option<&str> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('/') {
        return None;
    }
    Some(trimmed.split_whitespace().next().unwrap_or(trimmed))
}

pub fn active_slash_picker(mode: &TuiMode, buffer: &str) -> Option<SlashPickerState> {
    let token = slash_prefix_token(buffer)?;
    let matches = mode.slash_picker_matches(token);
    if matches.is_empty() {
        return None;
    }
    Some(SlashPickerState {
        prefix: token.to_string(),
        matches,
    })
}

pub fn render_slash_picker_hint(matches: &[SlashPickerMatch], selected: usize) -> String {
    let mut lines = vec!["Prompt".to_string(), "mode: slash".to_string()];
    if matches.is_empty() {
        return lines.join("\n");
    }

    let selected = selected.min(matches.len().saturating_sub(1));
    let window = 12.min(matches.len());
    let start = selected
        .saturating_sub(window / 2)
        .min(matches.len() - window);
    let end = (start + window).min(matches.len());

    if start > 0 {
        lines.push(format!("{start} earlier command(s)"));
    }

    for (offset, entry) in matches[start..end].iter().enumerate() {
        let index = start + offset;
        let marker = if index == selected { '>' } else { ' ' };
        lines.push(format!("{marker} {}", entry.label));
    }

    if end < matches.len() {
        lines.push(format!("{} more command(s)", matches.len() - end));
    }
    lines.join("\n")
}

pub fn apply_slash_picker_selection(editor: &mut InputEditor, command: &str) {
    editor.replace_range(0, editor.buffer().len(), command);
}

pub fn apply_file_picker_selection(editor: &mut InputEditor, range: &Range<usize>, path: &str) {
    let range =
        file_mention_range(editor.buffer(), editor.cursor()).unwrap_or_else(|| range.clone());
    // Directories (trailing /) stay open for drill-down — no trailing space.
    let is_directory = path.ends_with('/');
    let suffix_needs_space = !is_directory
        && editor
            .buffer()
            .get(range.end..)
            .map(|rest| rest.is_empty() || !rest.starts_with(char::is_whitespace))
            .unwrap_or(true);
    let replacement = if suffix_needs_space {
        format!("@{path} ")
    } else {
        format!("@{path}")
    };
    editor.replace_range(range.start, range.end, &replacement);
}

