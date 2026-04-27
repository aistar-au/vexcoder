use crate::runtime::UiUpdate;

use super::context::RuntimeContext;
use super::frontend::InputOccurrence;

pub trait RuntimeMode {
    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext);
    fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext);
    fn on_interrupt(&mut self, _ctx: &mut RuntimeContext) {}
    fn on_frontend_signal(&mut self, occurrence: InputOccurrence, ctx: &mut RuntimeContext) {
        match occurrence {
            InputOccurrence::Text(input) => self.on_user_input(input, ctx),
            InputOccurrence::Interrupt => self.on_interrupt(ctx),
            InputOccurrence::Scroll { .. } => {}
        }
    }
    fn is_pulse_in_progress(&self) -> bool;
}
