use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnTokens {
    pub input: u64,
    pub output: u64,
    #[serde(default)]
    pub estimated: bool,
}

impl TurnTokens {
    pub fn is_zero(self) -> bool {
        self.input == 0 && self.output == 0
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTokens {
    pub input: u64,
    pub output: u64,
    pub last_input: u64,
    pub last_output: u64,
    #[serde(default)]
    pub estimated: bool,
}

impl SessionTokens {
    pub fn record_turn(&mut self, turn: TurnTokens) {
        self.input = self.input.saturating_add(turn.input);
        self.output = self.output.saturating_add(turn.output);
        self.last_input = turn.input;
        self.last_output = turn.output;
        self.estimated = self.estimated || turn.estimated;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn has_completed_turns(&self) -> bool {
        self.input > 0 || self.output > 0 || self.last_input > 0 || self.last_output > 0
    }

    pub fn last_turn(&self) -> TurnTokens {
        TurnTokens {
            input: self.last_input,
            output: self.last_output,
            estimated: self.estimated,
        }
    }
}

pub fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    if chars == 0 {
        0
    } else {
        chars.div_ceil(4)
    }
}

#[cfg(test)]
mod tests {
    use super::{estimate_tokens, SessionTokens, TurnTokens};

    #[test]
    fn estimate_tokens_uses_char_quarters() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn session_tokens_accumulate_turns() {
        let mut session = SessionTokens::default();
        session.record_turn(TurnTokens {
            input: 10,
            output: 5,
            estimated: false,
        });
        session.record_turn(TurnTokens {
            input: 4,
            output: 3,
            estimated: true,
        });

        assert_eq!(session.input, 14);
        assert_eq!(session.output, 8);
        assert_eq!(session.last_input, 4);
        assert_eq!(session.last_output, 3);
        assert!(session.estimated);
    }
}
