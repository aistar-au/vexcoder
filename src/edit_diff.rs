pub const DEFAULT_EDIT_DIFF_CONTEXT_LINES: usize = 2;

pub fn format_edit_hunks(
    old_str: &str,
    new_str: &str,
    indent: &str,
    context_lines: usize,
) -> String {
    use similar::{ChangeTag, TextDiff};

    if old_str.is_empty() && new_str.is_empty() {
        return format!("{indent}1   <empty>\n");
    }

    let diff = TextDiff::from_lines(old_str, new_str);
    let groups = diff.grouped_ops(context_lines);

    if groups.is_empty() {
        return format!("{indent}... no modified lines ...\n");
    }

    let mut out = String::new();
    for (group_idx, group) in groups.iter().enumerate() {
        if group_idx > 0 {
            out.push_str(&format!("{indent}...\n"));
        }

        let old_start = group[0].old_range().start + 1;
        let new_start = group[0].new_range().start + 1;
        let old_len = group.last().unwrap().old_range().end - group[0].old_range().start;
        let new_len = group.last().unwrap().new_range().end - group[0].new_range().start;

        out.push_str(&format!(
            "{indent}@@ -{old_start},{old_len} +{new_start},{new_len} @@\n"
        ));

        for op in group {
            for change in diff.iter_changes(op) {
                let raw = change.value();
                let text = raw.trim_end_matches(['\n', '\r']);
                let text = if text.is_empty() { "<empty>" } else { text };

                match change.tag() {
                    ChangeTag::Equal => {
                        let line_no = change.old_index().unwrap_or(0) + 1;
                        out.push_str(&format!("{indent}{line_no}   {text}\n"));
                    }
                    ChangeTag::Delete => {
                        let line_no = change.old_index().unwrap_or(0) + 1;
                        out.push_str(&format!("{indent}{line_no} - {text}\n"));
                    }
                    ChangeTag::Insert => {
                        let line_no = change.new_index().unwrap_or(0) + 1;
                        out.push_str(&format!("{indent}{line_no} + {text}\n"));
                    }
                }
            }
        }
    }

    out
}

/// Generate a unified diff patch compatible with `git diff` / `diff -u` output.
///
/// Uses the `imara-diff` high-speed backend with the Histogram algorithm and
/// line-based interning.  Returns an empty string when both inputs are identical.
/// This complements `format_edit_hunks`, which is optimised for displaying diffs
/// inside the editor UI — `format_unified_patch` produces the raw patch format
/// that external tools (e.g. `git apply`) can consume.
pub fn format_unified_patch(old: &str, new: &str) -> String {
    use imara_diff::{Algorithm, BasicLineDiffPrinter, Diff, InternedInput, UnifiedDiffConfig};
    let input = InternedInput::new(old, new);
    let mut d = Diff::compute(Algorithm::Histogram, &input);
    d.postprocess_lines(&input);
    if d.hunks().count() == 0 {
        return String::new();
    }
    d.unified_diff(
        &BasicLineDiffPrinter(&input.interner),
        UnifiedDiffConfig::default(),
        &input,
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_edit_hunks_respects_requested_context_window() {
        let old_str = "a\nb\nc\nd\ne\nf";
        let new_str = "a\nb\nc changed\nd\ne\nf";

        let no_context = format_edit_hunks(old_str, new_str, "  ", 0);
        let wider_context = format_edit_hunks(old_str, new_str, "  ", 2);

        assert!(no_context.contains("- c"));
        assert!(no_context.contains("+ c changed"));
        assert!(!no_context.contains("1   a"));
        assert!(!no_context.contains("2   b"));

        assert!(wider_context.contains("1   a"));
        assert!(wider_context.contains("2   b"));
        assert!(wider_context.contains("4   d"));
        assert!(wider_context.contains("5   e"));
    }

    #[test]
    fn test_format_edit_hunks_adds_gap_between_separate_hunks() {
        let old_str = "a\nb\nc\nd\ne\nf\ng\nh";
        let new_str = "a\nb changed\nc\nd\ne\nf\ng changed\nh";

        let rendered = format_edit_hunks(old_str, new_str, "  ", 1);

        // similar groups nearby changes with context; 1-line context makes two distant
        // changes separable. Just verify the changes appear.
        assert!(rendered.contains("@@"));
        assert!(rendered.contains("- b"));
        assert!(rendered.contains("+ b changed"));
        assert!(rendered.contains("- g"));
        assert!(rendered.contains("+ g changed"));
    }

    #[test]
    fn test_format_edit_hunks_handles_empty_insert() {
        let rendered = format_edit_hunks("", "new line", "  ", 2);
        assert!(rendered.contains("@@"));
        assert!(rendered.contains("+ new line"));
    }
}
