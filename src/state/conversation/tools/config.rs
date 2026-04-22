

const DEFAULT_MAX_COMMAND_OUTPUT_BYTES: usize = 50 * 1024; 

pub(super) fn max_command_output_bytes() -> usize {
    std::env::var("VEX_MAX_COMMAND_OUTPUT_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_COMMAND_OUTPUT_BYTES)
}


pub(super) fn read_file_max_lines() -> usize {
    if let Some(explicit) = std::env::var("VEX_READ_FILE_MAX_LINES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        return explicit;
    }
    
    
    let max_tokens: usize = std::env::var("VEX_MAX_TOKENS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4096);
    let budget_lines = max_tokens / 200; 
    budget_lines.clamp(50, 10_000)
}


pub(super) fn write_file_diff_preferred_above_lines() -> usize {
    std::env::var("VEX_DIFF_PREFERRED_ABOVE_LINES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|v| v.clamp(10, 10_000))
        .unwrap_or(200)
}


pub(super) fn write_file_max_lines() -> usize {
    std::env::var("VEX_WRITE_FILE_MAX_LINES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|v| v.clamp(10, 10_000))
        .unwrap_or(500)
}


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
