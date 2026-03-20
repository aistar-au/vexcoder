use crate::app::StepLifecycle;
use std::io::Write;

// ── ANSI escape helpers ─────────────────────────────────────────────

pub(super) const CSI: &str = "\x1b[";
pub(super) const RESET: &str = "\x1b[0m";

pub(super) fn move_to(w: &mut dyn Write, row: u16, col: u16) {
    let _ = write!(w, "{CSI}{};{}H", row + 1, col + 1);
}

pub(super) fn clear_line(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}2K");
}

pub(super) fn clear_to_end(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}0J");
}

pub(super) fn set_fg(w: &mut dyn Write, code: u8) {
    let _ = write!(w, "{CSI}38;5;{code}m");
}

pub(super) fn set_bold(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}1m");
}

pub(super) fn set_dim(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}2m");
}

pub(super) fn set_italic(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}3m");
}

pub(super) fn reset_style(w: &mut dyn Write) {
    let _ = write!(w, "{RESET}");
}

pub(super) fn hide_cursor(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}?25l");
}

pub(super) fn show_cursor(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}?25h");
}

// ── Spinner frames (braille animation for running steps) ────────────

pub(super) const SPINNER_FRAMES: &[&str] = &[
    "\u{2846}", // ⡆
    "\u{2807}", // ⠇
    "\u{2803}", // ⠃
    "\u{2819}", // ⠙
    "\u{2838}", // ⠸
    "\u{2830}", // ⠰
    "\u{2860}", // ⡠
    "\u{2844}", // ⡄
];

// ── Color palette ───────────────────────────────────────────────────

pub(super) const GREEN: u8 = 2;
pub(super) const RED: u8 = 1;
pub(super) const CYAN: u8 = 6;
pub(super) const YELLOW: u8 = 3;
pub(super) const MAGENTA: u8 = 5;
pub(super) const GRAY: u8 = 245;
pub(super) const DIM_GRAY: u8 = 240;
pub(super) const WHITE: u8 = 15;
pub(super) const BLUE: u8 = 4;

// ── Progress bar frames (animated fill for running state) ───────────

pub(super) const PROGRESS_FRAMES: &[&str] = &[
    "\u{2591}\u{2591}\u{2593}\u{2588}\u{2588}\u{2593}\u{2591}\u{2591}", // ░░▓██▓░░
    "\u{2591}\u{2593}\u{2588}\u{2588}\u{2593}\u{2591}\u{2591}\u{2591}", // ░▓██▓░░░
    "\u{2593}\u{2588}\u{2588}\u{2593}\u{2591}\u{2591}\u{2591}\u{2591}", // ▓██▓░░░░
    "\u{2588}\u{2588}\u{2593}\u{2591}\u{2591}\u{2591}\u{2591}\u{2593}", // ██▓░░░░▓
    "\u{2588}\u{2593}\u{2591}\u{2591}\u{2591}\u{2591}\u{2593}\u{2588}", // █▓░░░░▓█
    "\u{2593}\u{2591}\u{2591}\u{2591}\u{2591}\u{2593}\u{2588}\u{2588}", // ▓░░░░▓██
    "\u{2591}\u{2591}\u{2591}\u{2591}\u{2593}\u{2588}\u{2588}\u{2593}", // ░░░░▓██▓
    "\u{2591}\u{2591}\u{2591}\u{2593}\u{2588}\u{2588}\u{2593}\u{2591}", // ░░░▓██▓░
];

pub(super) fn lifecycle_color(lifecycle: &StepLifecycle) -> u8 {
    match lifecycle {
        StepLifecycle::Queued => YELLOW,
        StepLifecycle::Completed => GREEN,
        StepLifecycle::Failed => RED,
        StepLifecycle::Running => CYAN,
        StepLifecycle::AwaitingApproval => YELLOW,
        StepLifecycle::Approved => GREEN,
        StepLifecycle::UserInput => DIM_GRAY,
        StepLifecycle::CommandSession => MAGENTA,
    }
}

pub(super) fn lifecycle_prefix(lifecycle: &StepLifecycle) -> &'static str {
    match lifecycle {
        StepLifecycle::Queued => "\u{2736}",           // ✶
        StepLifecycle::Completed => "\u{2605}",        // ★
        StepLifecycle::Failed => "\u{2716}",           // ✖
        StepLifecycle::Running => "\u{2726}",          // ✦
        StepLifecycle::AwaitingApproval => "\u{2606}", // ☆
        StepLifecycle::Approved => "\u{2713}",         // ✓
        StepLifecycle::UserInput => "\u{203a}",        // ›
        StepLifecycle::CommandSession => "\u{25c6}",   // ◆
    }
}
