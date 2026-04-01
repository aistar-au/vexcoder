/// Maximum bytes kept in the accumulated stdout/stderr buffers returned to the
/// model after a `run_command` tool call.  The full output is always streamed to
/// the TUI via `TranscriptLine`, so this cap only limits the in-process buffer
/// that becomes the tool result.  Override with `VEX_MAX_COMMAND_OUTPUT_BYTES`.
const DEFAULT_MAX_COMMAND_OUTPUT_BYTES: usize = 50 * 1024; // 50 KiB

pub(super) fn max_command_output_bytes() -> usize {
    std::env::var("VEX_MAX_COMMAND_OUTPUT_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_COMMAND_OUTPUT_BYTES)
}

/// Maximum lines returned by read_file when no explicit limit is provided.
/// Configurable via `VEX_READ_FILE_MAX_LINES`. When not set, derives from
/// `VEX_MAX_TOKENS` using the heuristic: 1 line ≈ 20 tokens, budget ≈ 10%
/// of max_tokens for a single file read. Defaults to 200 for small contexts.
pub(super) fn read_file_max_lines() -> usize {
    if let Some(explicit) = std::env::var("VEX_READ_FILE_MAX_LINES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        return explicit;
    }
    // Derive from max_tokens: allocate ~10% of context budget per file read,
    // at ~20 tokens per line. Large contexts (128K+) get generous limits.
    let max_tokens: usize = std::env::var("VEX_MAX_TOKENS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4096);
    let budget_lines = max_tokens / 200; // ~10% of context / 20 tok per line
    budget_lines.clamp(50, 10_000)
}

/// Line threshold above which `write_file` warns the model to use
/// `apply_patch` or `edit_file` instead. Default 200. Minimum 10.
pub(super) fn write_file_diff_preferred_above_lines() -> usize {
    std::env::var("VEX_DIFF_PREFERRED_ABOVE_LINES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|v| v.clamp(10, 10_000))
        .unwrap_or(200)
}

/// Hard line limit for `write_file`. Calls on files exceeding this are
/// rejected outright. Default 500. Minimum 10.
pub(super) fn write_file_max_lines() -> usize {
    std::env::var("VEX_WRITE_FILE_MAX_LINES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|v| v.clamp(10, 10_000))
        .unwrap_or(500)
}

/// Append `text` to `buf`, keeping only the tail when the cap is exceeded.
pub(super) fn append_capped(buf: &mut String, text: &str, cap: usize) {
    buf.push_str(text);
    if buf.len() > cap {
        let excess = buf.len() - cap;
        let drain_end = buf
            .char_indices()
            .find(|(i, _)| *i >= excess)
            .map(|(i, _)| i)
            .unwrap_or(excess);
        buf.drain(..drain_end);
    }
}
