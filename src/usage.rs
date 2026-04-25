use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PulseTokens {
    pub input: u64,
    pub output: u64,
    #[serde(default)]
    pub estimated: bool,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

impl PulseTokens {
    pub fn is_zero(self) -> bool {
        self.input == 0
            && self.output == 0
            && self.cache_creation_input_tokens == 0
            && self.cache_read_input_tokens == 0
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTokens {
    pub input: u64,
    pub output: u64,
    pub last_input: u64,
    pub last_output: u64,
    #[serde(default)]
    pub last_estimated: bool,
    #[serde(default)]
    pub estimated: bool,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub last_cache_creation_input_tokens: u64,
    #[serde(default)]
    pub last_cache_read_input_tokens: u64,
}

impl SessionTokens {
    pub fn record_pulse(&mut self, pulse: PulseTokens) {
        self.input = self.input.saturating_add(pulse.input);
        self.output = self.output.saturating_add(pulse.output);
        self.last_input = pulse.input;
        self.last_output = pulse.output;
        self.last_estimated = pulse.estimated;
        self.estimated = self.estimated || pulse.estimated;
        self.cache_creation_input_tokens = self
            .cache_creation_input_tokens
            .saturating_add(pulse.cache_creation_input_tokens);
        self.cache_read_input_tokens = self
            .cache_read_input_tokens
            .saturating_add(pulse.cache_read_input_tokens);
        self.last_cache_creation_input_tokens = pulse.cache_creation_input_tokens;
        self.last_cache_read_input_tokens = pulse.cache_read_input_tokens;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn has_completed_turns(&self) -> bool {
        self.input > 0 || self.output > 0 || self.last_input > 0 || self.last_output > 0
    }

    pub fn last_pulse(&self) -> PulseTokens {
        PulseTokens {
            input: self.last_input,
            output: self.last_output,
            estimated: self.last_estimated,
            cache_creation_input_tokens: self.last_cache_creation_input_tokens,
            cache_read_input_tokens: self.last_cache_read_input_tokens,
        }
    }
}

pub fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    if chars == 0 { 0 } else { chars.div_ceil(4) }
}

#[cfg(test)]
mod tests {
    use super::{PulseTokens, SessionTokens, estimate_tokens};

    #[test]
    fn estimate_tokens_uses_char_quarters() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn session_tokens_accumulate_turns() {
        let mut session = SessionTokens::default();
        session.record_pulse(PulseTokens {
            input: 10,
            output: 5,
            estimated: false,
            ..Default::default()
        });
        session.record_pulse(PulseTokens {
            input: 4,
            output: 3,
            estimated: true,
            ..Default::default()
        });

        assert_eq!(session.input, 14);
        assert_eq!(session.output, 8);
        assert_eq!(session.last_input, 4);
        assert_eq!(session.last_output, 3);
        assert!(session.last_estimated);
        assert!(session.estimated);
    }

    #[test]
    fn session_tokens_last_turn_preserves_per_turn_estimate_flag() {
        let mut session = SessionTokens::default();
        session.record_pulse(PulseTokens {
            input: 10,
            output: 5,
            estimated: true,
            ..Default::default()
        });
        session.record_pulse(PulseTokens {
            input: 4,
            output: 3,
            estimated: false,
            ..Default::default()
        });

        assert!(
            session.estimated,
            "session should remember any estimated pulse"
        );
        assert!(
            !session.last_pulse().estimated,
            "last_pulse should reflect only the latest pulse"
        );
    }
}
