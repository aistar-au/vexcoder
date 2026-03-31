#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusTone {
    Success,
    Error,
    Progress,
    Attention,
}

pub const MAPPING_ADJACENT_SECTORS: &str = "Mapping adjacent sectors...";
pub const STATE_SYNCHRONIZED: &str = "State synchronized.";
pub const WAITING_FOR_RESPONSE_LINE: &str = "[thinking] Mapping adjacent sectors...";

const LEGACY_WAITING_FOR_RESPONSE_LINE: &str = "[waiting for response...]";
const LEGACY_AWAITING_MODEL_RESPONSE_LINE: &str = "[awaiting model response]";

pub const fn pending_status_label() -> &'static str {
    MAPPING_ADJACENT_SECTORS
}

pub const fn completed_status_label() -> &'static str {
    STATE_SYNCHRONIZED
}

pub const fn waiting_for_response_line() -> &'static str {
    WAITING_FOR_RESPONSE_LINE
}

pub fn is_waiting_placeholder(line: &str) -> bool {
    if matches!(
        line,
        WAITING_FOR_RESPONSE_LINE
            | LEGACY_WAITING_FOR_RESPONSE_LINE
            | LEGACY_AWAITING_MODEL_RESPONSE_LINE
    ) {
        return true;
    }
    // Match the formatted waiting status with elapsed time and progress
    // counters appended by format_waiting_status() (e.g.
    // "[thinking] Mapping adjacent sectors... 2.5s | read:512/2641").
    line.starts_with(WAITING_FOR_RESPONSE_LINE)
}

pub fn status_tone(status: &str) -> Option<StatusTone> {
    match status {
        "approved" => Some(StatusTone::Success),
        "failed" | "cancelled" => Some(StatusTone::Error),
        "awaiting approval" | "cancellation requested" => Some(StatusTone::Attention),
        _ if is_completed_status(status) => Some(StatusTone::Success),
        _ if is_progress_status(status) => Some(StatusTone::Progress),
        _ => None,
    }
}

pub fn is_completed_status(status: &str) -> bool {
    matches!(status, "completed" | STATE_SYNCHRONIZED)
}

pub fn is_progress_status(status: &str) -> bool {
    matches!(status, "running" | MAPPING_ADJACENT_SECTORS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_placeholder_accepts_legacy_and_new_strings() {
        assert!(is_waiting_placeholder("[waiting for response...]"));
        assert!(is_waiting_placeholder("[awaiting model response]"));
        assert!(is_waiting_placeholder(WAITING_FOR_RESPONSE_LINE));
        assert!(is_waiting_placeholder(
            "[thinking] Mapping adjacent sectors... 2.5s | read:512/2641"
        ));
        assert!(!is_waiting_placeholder("[thinking] trace branch"));
    }

    #[test]
    fn status_tone_accepts_legacy_and_batch_a_status_labels() {
        assert_eq!(status_tone("completed"), Some(StatusTone::Success));
        assert_eq!(status_tone(STATE_SYNCHRONIZED), Some(StatusTone::Success));
        assert_eq!(status_tone("running"), Some(StatusTone::Progress));
        assert_eq!(
            status_tone(MAPPING_ADJACENT_SECTORS),
            Some(StatusTone::Progress)
        );
    }
}
