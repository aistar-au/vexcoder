use anyhow::Result;
use std::path::Path;

pub const INIT_CONFIG_TEMPLATE: &str = concat!(
    "# vex workspace config\n",
    "# uncomment only the keys you need for this workspace\n",
    "# model_name = \"local/default\"\n",
    "# model_url = \"https://example.invalid/v1\"\n",
    "# working_dir = \".\"\n",
    "# model_backend = \"local-runtime\"\n",
    "# model_protocol = \"messages-v1\"\n",
    "# tool_call_mode = \"structured\"\n",
    "# model_profile = \"models/local-balanced.toml\"\n",
    "# max_project_instructions_tokens = 4096\n",
    "# max_memory_tokens = 2048\n",
    "# notes_path = \"~/.config/vex/memory.md\"\n",
    "# sandbox = \"passthrough\"\n",
    "# sandbox_profile = \"\"\n",
    "# sandbox_require = false\n",
    "# model_headers = '{\"X-Client-Id\":\"vexcoder\"}'\n",
    "\n",
    "# [api]\n",
    "# transport = \"http\"\n",
    "# host = \"127.0.0.1\"\n",
    "# port = 6274\n",
    "# socket = \"\"\n",
    "# key = \"${VEX_API_KEY}\"\n",
    "# tls_cert = \"\"\n",
    "# tls_key = \"\"\n",
    "# tls_ca_cert = \"\"\n",
    "# tls_skip_verify = false\n",
    "# vpn_trust = false\n",
    "\n",
    "# user config only:\n",
    "# [[hooks]]\n",
    "# event = \"post_tool\"\n",
    "# tool = \"apply_patch\"\n",
    "# command = \"cargo\"\n",
    "# args = [\"fmt\"]\n",
    "# on_fail = \"warn\"\n",
    "\n",
    "# user config only:\n",
    "# [[mcp_servers]]\n",
    "# name = \"filesystem\"\n",
    "# transport = \"stdio\"\n",
    "# command = \"npx\"\n",
    "# args = [\"-y\", \"@modelcontextprotocol/server-filesystem\", \"/tmp\"]\n",
    "# url = \"http://localhost:3000/mcp\"\n",
    "\n",
    "# [mcp_servers.headers]\n",
    "# Authorization = \"${MCP_PRIVATE_SEARCH_TOKEN}\"\n",
);

pub const INIT_AGENTS_TEMPLATE: &str = concat!(
    "# Project Agents\n",
    "\n",
    "Fill in project-specific guidance for coding agents working in this repository.\n",
);

pub const INIT_VALIDATE_TEMPLATE: &str = concat!(
    "# validation commands applied by `vex validate`\n",
    "# [[commands]]\n",
    "# name = \"example\"\n",
    "# command = \"cargo test --all-targets\"\n",
);

pub const INIT_CONFIG_NORMATIVE_KEYS: &[&str] = &[
    "model_name",
    "model_url",
    "working_dir",
    "model_backend",
    "model_protocol",
    "tool_call_mode",
    "model_profile",
    "max_project_instructions_tokens",
    "max_memory_tokens",
    "notes_path",
    "sandbox",
    "sandbox_profile",
    "sandbox_require",
    "model_headers",
    "api",
    "api.transport",
    "api.host",
    "api.port",
    "api.socket",
    "api.key",
    "api.tls_cert",
    "api.tls_key",
    "api.tls_ca_cert",
    "api.tls_skip_verify",
    "api.vpn_trust",
    "hooks",
    "hooks.event",
    "hooks.tool",
    "hooks.command",
    "hooks.args",
    "hooks.on_fail",
    "mcp_servers",
    "mcp_servers.name",
    "mcp_servers.transport",
    "mcp_servers.command",
    "mcp_servers.args",
    "mcp_servers.url",
    "mcp_servers.headers",
    "mcp_servers.headers.Authorization",
];

pub fn scaffold_workspace(cwd: &Path) -> Result<Vec<String>> {
    let vex_dir = cwd.join(".vex");
    std::fs::create_dir_all(&vex_dir)?;

    let files: &[(&str, &str)] = &[
        (".vex/config.toml", INIT_CONFIG_TEMPLATE),
        ("AGENTS.md", INIT_AGENTS_TEMPLATE),
        (".vex/validate.toml", INIT_VALIDATE_TEMPLATE),
    ];
    let mut summary = Vec::new();

    for (rel_path, content) in files {
        let full = cwd.join(rel_path);
        if full.exists() {
            summary.push(format!("[init] skip (exists): {rel_path}"));
        } else {
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&full, content)?;
            summary.push(format!("[init] created: {rel_path}"));
        }
    }

    summary.push("[init] done".to_string());
    Ok(summary)
}

pub fn run_init(cwd: &Path) -> Result<Vec<String>> {
    scaffold_workspace(cwd)
}

pub fn extract_init_template_keys(content: &str) -> std::collections::BTreeSet<String> {
    let mut section: Option<&str> = None;
    let mut keys = std::collections::BTreeSet::new();

    for raw_line in content.lines() {
        let Some(line) = raw_line.trim().strip_prefix('#') else {
            continue;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match line {
            "[api]" => {
                section = Some("api");
                keys.insert("api".to_string());
                continue;
            }
            "[[hooks]]" => {
                section = Some("hooks");
                keys.insert("hooks".to_string());
                continue;
            }
            "[[mcp_servers]]" => {
                section = Some("mcp_servers");
                keys.insert("mcp_servers".to_string());
                continue;
            }
            "[mcp_servers.headers]" => {
                section = Some("mcp_servers.headers");
                keys.insert("mcp_servers.headers".to_string());
                continue;
            }
            _ => {}
        }

        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let full_key = match section {
            Some(prefix) => format!("{prefix}.{key}"),
            None => key.to_string(),
        };
        keys.insert(full_key);
    }

    keys
}
