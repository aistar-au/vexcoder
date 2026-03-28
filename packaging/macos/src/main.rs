//! macOS application launcher for vexcoder — ADR-024 Phase H (PH-01/PH-02).
//!
//! This binary is embedded in Vex.app at Contents/MacOS/vex-launcher.  Its
//! sole responsibilities are:
//!
//!   PH-01 — Locate the bundled vex binary and open a system terminal session
//!            with it running.  The launcher exits once the terminal is open;
//!            all agent logic runs inside the spawned vex process.
//!
//!   PH-02 — Read VEX_MODEL_TOKEN from the macOS system keychain before
//!            launching vex, and inject it as an environment variable so the
//!            agent process receives the credential without it ever being
//!            written to disk by this launcher.
//!
//! Phase H boundary: this file must not contain model calls, conversation
//! state, tool dispatch, or any import from the vexcoder library crate.

mod bundle;
mod keychain;

use std::process;

fn main() {
    let vex_binary = match bundle::vex_binary_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[vex-launcher] could not locate vex binary: {e}");
            process::exit(1);
        }
    };

    // Read credential from the system keychain (PH-02).  A missing token is
    // non-fatal: the vex binary produces its own missing-token diagnostic.
    let token = keychain::read_model_token();

    // Build and exec into the terminal command.  On success the exec call
    // replaces this process image; control only returns on failure.
    let mut cmd = terminal_command(&vex_binary);
    if let Some(ref t) = token {
        cmd.env("VEX_MODEL_TOKEN", t);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        eprintln!("[vex-launcher] exec failed: {err}");
        process::exit(1);
    }

    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(s) => process::exit(s.code().unwrap_or(0)),
            Err(e) => {
                eprintln!("[vex-launcher] launch failed: {e}");
                process::exit(1);
            }
        }
    }
}

/// Builds the command that opens a terminal session running the vex binary.
///
/// On macOS the system Terminal.app is launched via `osascript` so that the
/// new window inherits the full GUI session context.  The launcher process
/// exits once the AppleScript hand-off completes.
///
/// On other platforms (development/CI only) the binary is invoked directly.
fn terminal_command(vex_binary: &std::path::Path) -> process::Command {
    #[cfg(target_os = "macos")]
    {
        // Escape the path for embedding in the AppleScript string literal.
        let path_str = vex_binary
            .to_str()
            .unwrap_or("/usr/local/bin/vex")
            .replace('\\', "\\\\")
            .replace('"', "\\\"");

        let script = format!(
            "tell application \"Terminal\"\n\
             \tdo script \"{path_str}\"\n\
             \tactivate\n\
             end tell"
        );
        let mut cmd = process::Command::new("osascript");
        cmd.arg("-e").arg(script);
        cmd
    }

    #[cfg(not(target_os = "macos"))]
    {
        process::Command::new(vex_binary)
    }
}

#[cfg(test)]
mod tests {
    use super::terminal_command;
    use std::path::Path;

    #[test]
    fn terminal_command_contains_vex_binary_path() {
        let cmd = terminal_command(Path::new("/usr/local/bin/vex"));
        // On macOS the first argument to osascript should contain the binary path.
        // On other platforms the program itself should be the binary path.
        #[cfg(target_os = "macos")]
        {
            let args: Vec<_> = cmd
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert!(
                args.iter().any(|a| a.contains("/usr/local/bin/vex")),
                "expected binary path in osascript args: {args:?}"
            );
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(
                cmd.get_program().to_string_lossy(),
                "/usr/local/bin/vex"
            );
        }
    }
}
