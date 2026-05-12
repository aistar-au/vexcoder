use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;

use crate::edit_diff::format_edit_hunks;
use crate::runtime::context_cache::find_pulse_read;

use super::{PendingPatch, ToolOperator, WriteFileOutcome};

const MAX_EDIT_SNIPPET_CHARS: usize = 2_000;
const MAX_EDIT_SNIPPET_LINES: usize = 80;
const EDIT_ANCHOR_LINE_PAD: usize = 3;

impl ToolOperator {
    pub fn read_file(&self, path: &str) -> Result<String> {
        let resolved = self.resolve_path(path)?;
        if resolved.is_dir() {
            bail!("read_file expected a file path, got a directory: {path}");
        }
        fs::read_to_string(resolved).context("Failed to read file")
    }

    pub fn read_file_range(
        &self,
        path: &str,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<String> {
        let resolved = self.resolve_path(path)?;
        if resolved.is_dir() {
            bail!("read_file expected a file path, got a directory: {path}");
        }
        let content = fs::read_to_string(&resolved).context("Failed to read file")?;
        let user_offset = offset.unwrap_or(1);
        let start = user_offset.saturating_sub(1);
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        if start >= total {
            return Ok(format!(
                "(file has {total} lines, offset {user_offset} is past end)"
            ));
        }
        let end = limit
            .map(|value| (start + value).min(total))
            .unwrap_or(total);
        let selected: Vec<String> = lines[start..end]
            .iter()
            .enumerate()
            .map(|(index, line)| format!("{:>5}\t{}", start + index + 1, line))
            .collect();
        let header = if start > 0 || end < total {
            format!("(showing lines {}-{} of {total})\n", start + 1, end)
        } else {
            String::new()
        };
        Ok(format!("{}{}", header, selected.join("\n")))
    }

    pub fn read_file_if_exists(&self, path: &str) -> Result<Option<String>> {
        let resolved = self.resolve_path(path)?;
        if resolved.is_dir() {
            bail!("read_file expected a file path, got a directory: {path}");
        }
        match fs::read_to_string(resolved) {
            Ok(content) => Ok(Some(content)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).context("Failed to read file"),
        }
    }

    pub fn existing_path(&self, path: &str) -> Result<Option<std::path::PathBuf>> {
        let resolved = self.resolve_path(path)?;
        if resolved.exists() {
            Ok(Some(resolved))
        } else {
            Ok(None)
        }
    }

    pub fn write_file(&self, path: &str, content: &str) -> Result<WriteFileOutcome> {
        let resolved = self.resolve_path(path)?;
        if resolved.is_dir() {
            bail!("write_file expected a file path, got a directory: {path}");
        }

        if resolved.exists() {
            let old_content = fs::read_to_string(&resolved).unwrap_or_default();
            return self
                .propose_patch(path, &old_content, content)
                .map(WriteFileOutcome::Pending);
        }

        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(resolved, content).context("Failed to write file")?;
        Ok(WriteFileOutcome::Written)
    }

    pub fn propose_patch(
        &self,
        path: &str,
        old_content: &str,
        new_content: &str,
    ) -> Result<PendingPatch> {
        let resolved = self.resolve_path(path)?;
        if resolved.is_dir() {
            bail!("propose_patch expected a file path, got a directory: {path}");
        }

        let diff = format_edit_hunks(old_content, new_content, "", 2);

        Ok(PendingPatch {
            diff,
            new_content: new_content.to_string(),
            path: resolved,
        })
    }

    pub fn apply_patch(&self, pending: PendingPatch) -> Result<()> {
        self.ensure_path_is_within_workspace(&pending.path)?;

        let temp_path = pending.path.with_extension("tmp");
        fs::write(&temp_path, &pending.new_content)
            .with_context(|| format!("Failed to write temp file: {}", temp_path.display()))?;

        fs::rename(&temp_path, &pending.path)
            .with_context(|| format!("Failed to apply patch to: {}", pending.path.display()))?;

        Ok(())
    }

    pub fn edit_file(&self, path: &str, old_str: &str, new_str: &str) -> Result<()> {
        let resolved = self.resolve_path(path)?;
        if resolved.is_dir() {
            bail!("edit_file expected a file path, got a directory: {path}");
        }
        let content = fs::read_to_string(&resolved).context("Failed to read file for edit")?;

        if old_str.trim().is_empty() {
            bail!("edit_file requires a non-empty old_str");
        }
        if old_str.chars().count() > MAX_EDIT_SNIPPET_CHARS
            || new_str.chars().count() > MAX_EDIT_SNIPPET_CHARS
            || old_str.lines().count() > MAX_EDIT_SNIPPET_LINES
            || new_str.lines().count() > MAX_EDIT_SNIPPET_LINES
        {
            bail!(
                "edit_file requires focused snippets; old_str/new_str are too large (max {} chars or {} lines each)",
                MAX_EDIT_SNIPPET_CHARS,
                MAX_EDIT_SNIPPET_LINES
            );
        }
        if old_str == content {
            bail!(
                "edit_file refuses full-file replacement; provide a focused old_str snippet instead"
            );
        }

        let occurrences = content.matches(old_str).count();
        if occurrences == 0 {
            bail!("String '{}' not found in file", old_str);
        }
        if occurrences == 1 {
            let new_content = content.replacen(old_str, new_str, 1);
            return fs::write(resolved, new_content).context("Failed to edit file");
        }

        if let Some(anchored) =
            anchored_unique_replacement(&resolved, &content, old_str, new_str)?
        {
            return fs::write(resolved, anchored).context("Failed to edit file");
        }

        bail!(
            "String '{}' appears {} times; must be unique",
            old_str,
            occurrences
        );
    }

    pub fn rename_file(&self, old_path: &str, new_path: &str) -> Result<String> {
        let from = self.resolve_path(old_path)?;
        let to = self.resolve_path(new_path)?;

        if !from.exists() {
            bail!(
                "Failed to rename file: source '{}' does not exist",
                old_path
            );
        }
        if from == to {
            return Ok(format!("Source and target are the same: {old_path}"));
        }

        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).context("Failed to create destination directory")?;
        }
        fs::rename(&from, &to).context("Failed to rename file")?;
        Ok(format!("Renamed {} -> {}", old_path, new_path))
    }

    pub fn list_files(&self, path: Option<&str>, max_entries: usize) -> Result<String> {
        let root = self.resolve_optional_path(path)?;
        let limit = max_entries.clamp(1, 2000);
        let mut entries = Vec::new();

        if root.is_file() {
            entries.push(self.to_workspace_relative_display(&root));
        } else {
            let mut entries_in_dir: Vec<_> = fs::read_dir(&root)
                .with_context(|| format!("Failed to read directory {}", root.display()))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .with_context(|| format!("Failed to list entries in {}", root.display()))?;
            entries_in_dir.sort_by_key(|entry| entry.path());

            for entry in entries_in_dir {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if should_skip_list_entry(root.as_path(), self.working_dir.as_path(), &name) {
                    continue;
                }

                let path = entry.path();
                let is_dir = entry
                    .file_type()
                    .with_context(|| format!("Failed to inspect {}", path.display()))?
                    .is_dir();
                let mut display = self.to_workspace_relative_display(&path);
                if is_dir {
                    display.push('/');
                }
                entries.push(display);
                if entries.len() >= limit {
                    break;
                }
            }
        }

        if entries.is_empty() {
            Ok("(no files found)".to_string())
        } else {
            Ok(entries.join("\n"))
        }
    }
}

fn anchored_unique_replacement(
    resolved: &Path,
    content: &str,
    old_str: &str,
    new_str: &str,
) -> Result<Option<String>> {
    let Some(anchor) = find_pulse_read(resolved) else {
        return Ok(None);
    };

    let window_start = anchor.start_line.saturating_sub(EDIT_ANCHOR_LINE_PAD);
    let window_end = anchor.end_line.saturating_add(EDIT_ANCHOR_LINE_PAD);

    let mut candidate_offsets = Vec::new();
    for (byte_offset, _) in content.match_indices(old_str) {
        let line = line_of_byte_offset(content, byte_offset);
        if line >= window_start && line <= window_end {
            candidate_offsets.push(byte_offset);
            if candidate_offsets.len() > 1 {
                return Ok(None);
            }
        }
    }

    let Some(offset) = candidate_offsets.first().copied() else {
        return Ok(None);
    };

    let mut new_content = String::with_capacity(content.len() + new_str.len());
    new_content.push_str(&content[..offset]);
    new_content.push_str(new_str);
    new_content.push_str(&content[offset + old_str.len()..]);
    Ok(Some(new_content))
}

fn line_of_byte_offset(content: &str, byte_offset: usize) -> usize {
    if byte_offset == 0 {
        return 1;
    }
    let bounded = byte_offset.min(content.len());
    content[..bounded].bytes().filter(|byte| *byte == b'\n').count() + 1
}

fn should_skip_list_entry(root: &Path, working_dir: &Path, name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }

    if root != working_dir {
        return false;
    }

    matches!(
        name,
        "target" | "node_modules" | "__pycache__" | ".venv" | "venv" | "build" | "dist"
    )
}
