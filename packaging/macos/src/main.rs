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

    let token = keychain::read_model_token();

    let mut cmd = terminal_command(&vex_binary, token.as_deref());

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

fn terminal_command(vex_binary: &std::path::Path, token: Option<&str>) -> process::Command {
    #[cfg(target_os = "macos")]
    {
        let shell_command = terminal_shell_command(vex_binary, token);

        let script = format!(
            "tell application \"Terminal\"\n\
            		do script \"{}\"\n\
             \tactivate\n\
             end tell",
            apple_script_escape(&shell_command)
        );
        let mut cmd = process::Command::new("osascript");
        cmd.arg("-e").arg(script);
        cmd
    }

    #[cfg(not(target_os = "macos"))]
    {
        let mut cmd = process::Command::new(vex_binary);
        if let Some(token) = token {
            cmd.env("VEX_MODEL_TOKEN", token);
        }
        cmd
    }
}

fn terminal_shell_command(vex_binary: &std::path::Path, token: Option<&str>) -> String {
    let vex_binary = shell_quote(vex_binary.to_str().unwrap_or("/usr/local/bin/vex"));

    match token {
        Some(token) => {
            let token = shell_quote(token);
            format!("export VEX_MODEL_TOKEN={token}; exec {vex_binary}")
        }
        None => format!("exec {vex_binary}"),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn apple_script_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{apple_script_escape, shell_quote, terminal_command, terminal_shell_command};
    use std::path::Path;

    #[test]
    fn terminal_command_contains_vex_binary_path() {
        let cmd = terminal_command(Path::new("/usr/local/bin/vex"), None);
        
        #[cfg(target_os = "macos")]
        {
            let args: Vec<_> = cmd
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert!(
                args.iter().any(|a| a.contains("exec '/usr/local/bin/vex'")),
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

    #[test]
    fn terminal_command_quotes_binary_paths_with_spaces() {
        let cmd = terminal_command(Path::new("/Applications/Vex App/vex"), None);

        #[cfg(target_os = "macos")]
        {
            let args: Vec<_> = cmd
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert!(
                args.iter().any(|a| a.contains("exec '/Applications/Vex App/vex'")),
                "expected quoted binary path in osascript args: {args:?}"
            );
        }

        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(
                cmd.get_program().to_string_lossy(),
                "/Applications/Vex App/vex"
            );
        }
    }

    #[test]
    fn terminal_shell_command_includes_token_export_when_present() {
        let command = terminal_shell_command(Path::new("/usr/local/bin/vex"), Some("token value"));

        assert_eq!(
            command,
            "export VEX_MODEL_TOKEN='token value'; exec '/usr/local/bin/vex'"
        );
    }

    #[test]
    fn terminal_shell_command_escapes_single_quotes() {
        let command = terminal_shell_command(Path::new("/Applications/Vex's App/vex"), Some("tok'en"));

        assert_eq!(
            command,
            "export VEX_MODEL_TOKEN='tok'\"'\"'en'; exec '/Applications/Vex'\"'\"'s App/vex'"
        );
    }

    #[test]
    fn shell_quote_handles_empty_and_single_quote_values() {
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a'b"), "'a'\"'\"'b'");
    }

    #[test]
    fn apple_script_escape_preserves_shell_quotes() {
        assert_eq!(
            apple_script_escape("exec '/Applications/Vex App/vex'"),
            "exec '/Applications/Vex App/vex'"
        );
        assert_eq!(
            apple_script_escape("say \"hello\" \\\\ world"),
            "say \\\"hello\\\" \\\\\\\\ world"
        );
    }
}
