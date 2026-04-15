use anyhow::{Context, Result};

// Keep clipboard crate churn out of slash-command handling.
pub fn copy_text(text: &str) -> Result<()> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .context("clipboard unavailable")
}
