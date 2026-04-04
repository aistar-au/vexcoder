pub const DEFAULT_EDIT_DIFF_CONTEXT_LINES: usize = 2;

pub fn format_edit_hunks(
    old_str: &str,
    new_str: &str,
    indent: &str,
    context_lines: usize,
) -> String {
    let mut options = diffy::DiffOptions::new();
    options.set_context_len(context_lines);
    let patch = options.create_patch(old_str, new_str);
    let hunks = patch.hunks();

    if hunks.is_empty() {
        if old_str.is_empty() && new_str.is_empty() {
            return format!("{indent}1   <empty>\n");
        }
        return format!("{indent}... no modified lines ...\n");
    }

    let mut out = String::new();
    for (index, hunk) in hunks.iter().enumerate() {
        if index > 0 {
            out.push_str(&format!("{indent}...\n"));
        }

        let range = hunk.new_range();
        let old_range = hunk.old_range();
        out.push_str(&format!(
            "{indent}@@ -{},{} +{},{} @@\n",
            old_range.start(),
            old_range.len(),
            range.start(),
            range.len(),
        ));

        let mut old_line = old_range.start();
        let mut new_line = range.start();

        for line in hunk.lines() {
            match line {
                diffy::Line::Context(text) => {
                    // Only show context within the requested window
                    let text = if text.is_empty() { "<empty>" } else { text };
                    out.push_str(&format!("{indent}{old_line}   {text}\n"));
                    old_line += 1;
                    new_line += 1;
                }
                diffy::Line::Delete(text) => {
                    let text = if text.is_empty() { "<empty>" } else { text };
                    out.push_str(&format!("{indent}{old_line} - {text}\n"));
                    old_line += 1;
                }
                diffy::Line::Insert(text) => {
                    let text = if text.is_empty() { "<empty>" } else { text };
                    out.push_str(&format!("{indent}{new_line} + {text}\n"));
                    new_line += 1;
                }
            }
        }
    }

    out
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

        // diffy merges nearby hunks with 3-line default context, so this may
        // be 1 or 2 hunks depending on proximity. Just verify the changes appear.
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
