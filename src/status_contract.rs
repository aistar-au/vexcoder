#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusTone {
    Success,
    Error,
    Progress,
    Attention,
}

pub const MAPPING_ADJACENT_SECTORS: &str = "Mapping adjacent sectors...";

pub const RESPONSE_COMPLETE: &str = "Response complete.";
pub const WAITING_FOR_RESPONSE_LINE: &str = "[thinking] Mapping adjacent sectors...";

pub const fn pending_status_label() -> &'static str {
    MAPPING_ADJACENT_SECTORS
}

pub const fn completed_status_label() -> &'static str {
    RESPONSE_COMPLETE
}

pub const fn waiting_for_response_line() -> &'static str {
    WAITING_FOR_RESPONSE_LINE
}

#[cfg(test)]
const LEGACY_WAITING_FOR_RESPONSE_LINE: &str = "[waiting for response...]";
#[cfg(test)]
const LEGACY_AWAITING_MODEL_RESPONSE_LINE: &str = "[awaiting model response]";

#[cfg(test)]
fn is_waiting_placeholder(line: &str) -> bool {
    if matches!(
        line,
        WAITING_FOR_RESPONSE_LINE
            | LEGACY_WAITING_FOR_RESPONSE_LINE
            | LEGACY_AWAITING_MODEL_RESPONSE_LINE
    ) {
        return true;
    }
    
    
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
    matches!(status, "completed" | RESPONSE_COMPLETE)
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
            "[thinking] Mapping adjacent sectors... 2.5s | ↑:512/2641"
        ));
        assert!(!is_waiting_placeholder("[thinking] trace branch"));
    }

    #[test]
    fn status_tone_accepts_legacy_and_batch_a_status_labels() {
        assert_eq!(status_tone("completed"), Some(StatusTone::Success));
        assert_eq!(status_tone(RESPONSE_COMPLETE), Some(StatusTone::Success));
        assert_eq!(status_tone("running"), Some(StatusTone::Progress));
        assert_eq!(
            status_tone(MAPPING_ADJACENT_SECTORS),
            Some(StatusTone::Progress)
        );
    }

    #[test]
    fn completed_status_label_is_response_complete() {
        assert_eq!(
            completed_status_label(),
            "Response complete.",
            "completed status must say 'Response complete.'"
        );
    }

    #[test]
    fn response_complete_constant_correct() {
        assert_eq!(RESPONSE_COMPLETE, "Response complete.");
        assert!(
            is_completed_status(RESPONSE_COMPLETE),
            "RESPONSE_COMPLETE must be recognised as a completed status"
        );
    }
}
