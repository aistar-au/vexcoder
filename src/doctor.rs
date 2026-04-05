use crate::config::{doctor_rollup, Config, DoctorConfigRollup, DoctorMcpServer};
use crate::runtime::{
    ApprovalPolicy, FileApprovalPolicy, PassthroughSandbox, SandboxDriver, TaskState,
};
use crate::util::is_local_endpoint_url;
use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DoctorCheck {
    pub check: String,
    pub status: DoctorStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn has_failures(&self) -> bool {
        self.checks
            .iter()
            .any(|check| matches!(check.status, DoctorStatus::Fail))
    }

    pub fn render_text(&self) -> String {
        self.checks
            .iter()
            .map(|check| {
                format!(
                    "[{}] {}: {}",
                    status_label(check.status),
                    check.check,
                    check.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn render_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.checks)?)
    }
}

pub async fn run_doctor(cwd: &Path) -> DoctorReport {
    let mut checks = Vec::new();
    let config_result = Config::load_from_cwd(cwd);
    let snapshot_result = doctor_rollup(cwd);

    let snapshot = snapshot_result
        .ok()
        .unwrap_or_else(|| fallback_rollup(cwd.to_path_buf()));

    match &config_result {
        Ok(_) => checks.push(pass_check(
            "Config load",
            "layered config resolved successfully",
        )),
        Err(error) => checks.push(fail_check("Config load", error.to_string())),
    }

    if let Some(model_url) = snapshot.model_url.as_deref() {
        match reqwest::Url::parse(model_url) {
            Ok(_) => checks.push(pass_check(
                "VEX_MODEL_URL set",
                format!("configured: {model_url}"),
            )),
            Err(error) => checks.push(fail_check(
                "VEX_MODEL_URL set",
                format!("invalid URL '{model_url}': {error}"),
            )),
        }

        match probe_http_endpoint(model_url).await {
            Ok(status) => checks.push(pass_check(
                "Model endpoint reachable",
                format!("received HTTP {}", status.as_u16()),
            )),
            Err(error) => checks.push(warn_check(
                "Model endpoint reachable",
                format!("endpoint probe failed: {error}"),
            )),
        }

        if is_local_endpoint_url(model_url) || snapshot.model_token_present {
            let message = if is_local_endpoint_url(model_url) {
                "local endpoint; token not required".to_string()
            } else {
                "token present for non-local endpoint".to_string()
            };
            checks.push(pass_check("VEX_MODEL_TOKEN present", message));
        } else {
            checks.push(warn_check(
                "VEX_MODEL_TOKEN present",
                "non-local endpoint configured without VEX_MODEL_TOKEN".to_string(),
            ));
        }
    } else {
        checks.push(fail_check(
            "VEX_MODEL_URL set",
            "model_url not explicitly configured".to_string(),
        ));
        checks.push(warn_check(
            "Model endpoint reachable",
            "skipped: no explicit model_url configured".to_string(),
        ));
        checks.push(warn_check(
            "VEX_MODEL_TOKEN present",
            "skipped: no explicit model_url configured".to_string(),
        ));
    }

    match PassthroughSandbox.probe() {
        Ok(()) if snapshot.sandbox_require => checks.push(fail_check(
            "Sandbox probe",
            "sandbox_require=true but only passthrough fallback is available".to_string(),
        )),
        Ok(()) => checks.push(warn_check(
            "Sandbox probe",
            "passthrough sandbox available; sandbox_require=false".to_string(),
        )),
        Err(error) if snapshot.sandbox_require => {
            checks.push(fail_check("Sandbox probe", error.to_string()))
        }
        Err(error) => checks.push(warn_check(
            "Sandbox probe",
            format!("{error}; sandbox_require=false"),
        )),
    }

    if snapshot.mcp_servers.is_empty() {
        checks.push(pass_check(
            "MCP server connectivity",
            "no MCP servers configured".to_string(),
        ));
    } else {
        for server in &snapshot.mcp_servers {
            checks.push(check_mcp_server(server).await);
        }
    }

    checks.push(check_state_dir(&snapshot.working_dir));
    checks.push(check_policy_file(cwd));

    DoctorReport { checks }
}

fn fallback_rollup(cwd: PathBuf) -> DoctorConfigRollup {
    DoctorConfigRollup {
        model_url: None,
        working_dir: cwd,
        model_token_present: false,
        sandbox_require: false,
        mcp_servers: Vec::new(),
    }
}

async fn probe_http_endpoint(url: &str) -> Result<reqwest::StatusCode> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let response = client.get(url).send().await?;
    Ok(response.status())
}

fn check_state_dir(working_dir: &Path) -> DoctorCheck {
    let dir = TaskState::state_dir_from(working_dir);
    if !dir.exists() {
        return warn_check(
            "State directory writable",
            format!("state directory does not exist: {}", dir.display()),
        );
    }

    match std::fs::metadata(&dir) {
        Ok(metadata) if metadata.is_dir() && !metadata.permissions().readonly() => pass_check(
            "State directory writable",
            format!("writable directory: {}", dir.display()),
        ),
        Ok(_) => warn_check(
            "State directory writable",
            format!("directory is not writable: {}", dir.display()),
        ),
        Err(error) => warn_check(
            "State directory writable",
            format!("failed to inspect {}: {error}", dir.display()),
        ),
    }
}

fn check_policy_file(cwd: &Path) -> DoctorCheck {
    let configured =
        std::env::var("VEX_POLICY_FILE").unwrap_or_else(|_| ".vex/policy.toml".to_string());
    let path = if Path::new(&configured).is_absolute() {
        PathBuf::from(configured)
    } else {
        cwd.join(configured)
    };

    if !path.exists() {
        return pass_check(
            "Policy file parseable",
            "policy file not present".to_string(),
        );
    }

    match FileApprovalPolicy::load_from_file(&path) {
        Ok(_) => pass_check(
            "Policy file parseable",
            format!("parsed {}", path.display()),
        ),
        Err(error) => fail_check(
            "Policy file parseable",
            format!("failed to parse {}: {error}", path.display()),
        ),
    }
}

async fn check_mcp_server(server: &DoctorMcpServer) -> DoctorCheck {
    let check_name = format!("MCP server connectivity ({})", server.name);
    match server.transport.trim().to_ascii_lowercase().as_str() {
        "stdio" => match server.command.as_deref().and_then(resolve_binary_on_path) {
            Some(path) => pass_check(
                check_name,
                format!("stdio binary resolved: {}", path.display()),
            ),
            None => warn_check(
                check_name,
                format!(
                    "stdio command not resolvable on PATH: {}",
                    server.command.as_deref().unwrap_or("<missing>")
                ),
            ),
        },
        "http" | "https" => {
            let Some(url) = server.url.as_deref() else {
                return warn_check(check_name, "missing MCP server URL".to_string());
            };
            match probe_http_endpoint(url).await {
                Ok(status) => pass_check(check_name, format!("received HTTP {}", status.as_u16())),
                Err(error) => warn_check(check_name, format!("endpoint probe failed: {error}")),
            }
        }
        other => warn_check(check_name, format!("unsupported MCP transport '{other}'")),
    }
}

fn resolve_binary_on_path(command: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(command);
    if candidate.components().count() > 1 {
        return candidate.exists().then_some(candidate);
    }

    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(command))
        .find(|candidate| candidate.exists())
}

fn pass_check(check: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        check: check.into(),
        status: DoctorStatus::Pass,
        message: message.into(),
    }
}

fn warn_check(check: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        check: check.into(),
        status: DoctorStatus::Warn,
        message: message.into(),
    }
}

fn fail_check(check: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        check: check.into(),
        status: DoctorStatus::Fail,
        message: message.into(),
    }
}

fn status_label(status: DoctorStatus) -> &'static str {
    match status {
        DoctorStatus::Pass => "pass",
        DoctorStatus::Warn => "warn",
        DoctorStatus::Fail => "fail",
    }
}

#[cfg(test)]
mod tests {
    use super::{run_doctor, DoctorStatus};
    use crate::test_support::ENV_LOCK;
    use std::fs;

    struct EnvRestore {
        key: &'static str,
        value: Option<String>,
    }

    impl EnvRestore {
        fn capture(key: &'static str) -> Self {
            Self {
                key,
                value: std::env::var(key).ok(),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match &self.value {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[tokio::test]
    async fn test_vex_doctor_fails_on_missing_model_url() {
        let _env_lock = ENV_LOCK.lock().await;
        let workspace = tempfile::tempdir().expect("tempdir");
        let _home = EnvRestore::capture("HOME");
        let _xdg = EnvRestore::capture("XDG_CONFIG_HOME");
        let home = workspace.path().join("home");
        let xdg = workspace.path().join("xdg");
        fs::create_dir_all(&home).expect("home dir");
        fs::create_dir_all(&xdg).expect("xdg dir");
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", &xdg);
        std::env::remove_var("VEX_MODEL_URL");
        std::env::remove_var("VEX_MODEL_TOKEN");
        std::env::remove_var("VEX_WORKDIR");

        let report = run_doctor(workspace.path()).await;
        let check = report
            .checks
            .iter()
            .find(|check| check.check == "VEX_MODEL_URL set")
            .expect("model url check");
        assert_eq!(check.status, DoctorStatus::Fail);
    }

    #[tokio::test]
    async fn test_vex_doctor_json_output_structure() {
        let _env_lock = ENV_LOCK.lock().await;
        let workspace = tempfile::tempdir().expect("tempdir");
        let _home = EnvRestore::capture("HOME");
        let _xdg = EnvRestore::capture("XDG_CONFIG_HOME");
        let home = workspace.path().join("home");
        let xdg = workspace.path().join("xdg");
        fs::create_dir_all(&home).expect("home dir");
        fs::create_dir_all(&xdg).expect("xdg dir");
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", &xdg);
        std::env::set_var("VEX_MODEL_URL", "http://127.0.0.1:9/v1");

        let report = run_doctor(workspace.path()).await;
        let rendered = report.render_json().expect("json");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid json");
        assert!(parsed.is_array());
        assert!(parsed
            .as_array()
            .expect("array")
            .iter()
            .all(|entry| entry.get("check").is_some() && entry.get("status").is_some()));
    }

    #[tokio::test]
    async fn test_vex_doctor_sandbox_probe_warns_on_fallback() {
        let _env_lock = ENV_LOCK.lock().await;
        let workspace = tempfile::tempdir().expect("tempdir");
        let _home = EnvRestore::capture("HOME");
        let _xdg = EnvRestore::capture("XDG_CONFIG_HOME");
        let home = workspace.path().join("home");
        let xdg = workspace.path().join("xdg");
        fs::create_dir_all(&home).expect("home dir");
        fs::create_dir_all(&xdg).expect("xdg dir");
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", &xdg);
        fs::create_dir_all(workspace.path().join(".vex")).expect("config dir");
        fs::write(
            workspace.path().join(".vex/config.toml"),
            "model_url = \"http://127.0.0.1:9/v1\"\nsandbox_require = false\n",
        )
        .expect("write config");
        std::env::remove_var("VEX_MODEL_URL");

        let old_cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(workspace.path()).expect("set cwd");
        let report = run_doctor(workspace.path()).await;
        std::env::set_current_dir(old_cwd).expect("restore cwd");

        let check = report
            .checks
            .iter()
            .find(|check| check.check == "Sandbox probe")
            .expect("sandbox check");
        assert_eq!(check.status, DoctorStatus::Warn);
    }
}
