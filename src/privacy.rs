use serde::Serialize;
use std::fmt::Write as _;

pub const PRIVACY_VERSION: u16 = 1;
pub const PRIVACY_DOC_PATH: &str = "docs/src/privacy.md";
pub const LOCAL_API_PRIVACY_PATH: &str = "/v1/privacy";

const CLIENT_LOCAL_STORAGE: &[&str] = &[
    "Saved task state, projections, and peer-message files are written under `.vex/state/` or `VEX_STATE_DIR`.",
    "Search indexes are written under `.vex/index/` when structural or semantic indexing is enabled.",
    "Notes are read from and written to the operator-selected `notes_path`, which is allowed only in the user config layer.",
    "Exported task artifacts are written only when `vex export` is invoked.",
    "Rolling file logs are written only when `RUST_LOG` is set.",
];

const CLIENT_REMOTE_TRANSFER: &[&str] = &[
    "Prompts, selected repository context, tool results, and model outputs are sent to the configured model endpoint.",
    "Requests to configured MCP servers or HTTP hooks leave the machine only when those integrations are enabled.",
    "Provider-returned usage metadata may appear in transcript rows, saved task state, or exports because it arrives inside the model response.",
];

const CLIENT_PROTECTIONS: &[&str] = &[
    "Model credentials are read from `VEX_MODEL_TOKEN` or the OS credential store; `vex credentials set` refuses argv secrets.",
    "Repo-local config rejects `notes_path` and API secrets so repository state does not become the persistence layer for sensitive settings.",
    "The current build does not include a repository-managed analytics uploader; task telemetry shown in the surface is local runtime state.",
];

const SERVER_LOCAL_STORAGE: &[&str] = &[
    "Active turn state, approval state, and streamed envelope buffers are held in memory while a task is running.",
    "Persistent task state still uses the same `.vex/state/` files as the CLI surface; the server does not create a second hosted history store.",
    "Unix-socket mode uses a filesystem path under `${XDG_RUNTIME_DIR}` or `/tmp` on supported platforms.",
];

const SERVER_REMOTE_TRANSFER: &[&str] = &[
    "Authorized LocalApiServer clients receive prompts, transcript rows, tool results, and turn metadata over `POST /v1/turns` and the related control routes.",
    "If the server is bound beyond loopback, those payloads travel over the configured TLS surface.",
    "Metadata endpoints such as `/v1/health`, `/v1/schema`, and `/v1/privacy` expose service or policy data rather than runtime envelopes.",
];

const SERVER_PROTECTIONS: &[&str] = &[
    "HTTP mode requires a bearer token, and repo-local config may not provide `api.key`.",
    "Non-loopback HTTP requires TLS 1.2 or newer, with TLS 1.3 preferred, and HTTPS responses emit HSTS.",
    "Streaming responses set no-cache headers and disable proxy buffering on the LocalApiServer surface.",
];

const CREDENTIAL_ITEMS: &[&str] = &[
    "Model credentials are stored in the OS keyring or supplied through environment variables.",
    "LocalApiServer bearer tokens are user-config or environment only; repo-local config rejects them.",
    "`VEX_KEYRING_DISABLED` disables credential-store fallback when environment-only token handling is required.",
];

const TELEMETRY_ITEMS: &[&str] = &[
    "Interactive turn telemetry is displayed in the CLI transcript and status surface as local state rather than as a separate analytics export.",
    "When the configured model endpoint returns usage metadata, VexCoder may persist or render that metadata with the turn because it is part of the model response.",
    "Crash or debug logs are opt-in via `RUST_LOG`; without it, the runtime does not create rolling log files.",
];

const RETENTION_ITEMS: &[&str] = &[
    "Saved task state, peer-message files, indexes, and exports remain on disk until the operator removes them or changes the configured storage path.",
    "Notes remain at the configured `notes_path` until they are edited or deleted.",
    "Active LocalApiServer task buffers are retained in memory for the life of the active turn and then released.",
    "Remote retention for model endpoints, MCP servers, and other external services is controlled by those services rather than by VexCoder.",
];

const CONTROL_ITEMS: &[&str] = &[
    "Run against a same-machine local model endpoint when prompts and repository context must remain on the machine.",
    "Prefer Unix-socket mode or loopback HTTP for LocalApiServer clients.",
    "Keep `api.host` on loopback unless TLS and the bearer-token boundary are intentionally configured.",
    "Use `vex credentials` instead of literal secrets in shell history or repo-local config.",
    "Review `docs/src/privacy.md` before enabling remote hooks, MCP servers, or non-loopback LocalApiServer binds.",
];

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct PrivacySurface {
    pub summary: &'static str,
    pub local_storage: &'static [&'static str],
    pub remote_transfer: &'static [&'static str],
    pub protections: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct PrivacySection {
    pub summary: &'static str,
    pub items: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct PrivacyReferences {
    pub docs_path: &'static str,
    pub local_api_path: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct PrivacyStatement {
    pub version: u16,
    pub summary: &'static str,
    pub client: PrivacySurface,
    pub server: PrivacySurface,
    pub credentials: PrivacySection,
    pub telemetry: PrivacySection,
    pub retention: PrivacySection,
    pub controls: &'static [&'static str],
    pub references: PrivacyReferences,
}

pub fn privacy_statement() -> PrivacyStatement {
    PrivacyStatement {
        version: PRIVACY_VERSION,
        summary: "VexCoder keeps operator state local by default, sends prompts and repository context only to explicitly configured external services, and treats the LocalApiServer transport as a separate privacy boundary.",
        client: PrivacySurface {
            summary: "The CLI surface stores operator-selected state on disk and sends task content only to configured model or integration endpoints.",
            local_storage: CLIENT_LOCAL_STORAGE,
            remote_transfer: CLIENT_REMOTE_TRANSFER,
            protections: CLIENT_PROTECTIONS,
        },
        server: PrivacySurface {
            summary: "LocalApiServer exposes the shared runtime to local clients through loopback HTTP or Unix sockets and applies a dedicated auth and transport boundary.",
            local_storage: SERVER_LOCAL_STORAGE,
            remote_transfer: SERVER_REMOTE_TRANSFER,
            protections: SERVER_PROTECTIONS,
        },
        credentials: PrivacySection {
            summary: "Credential handling is explicit and keeps long-lived secrets out of repo-local config and argv surfaces.",
            items: CREDENTIAL_ITEMS,
        },
        telemetry: PrivacySection {
            summary: "Runtime telemetry is part of the local operator surface; external telemetry paths are opt-in or provider-defined rather than repository-managed.",
            items: TELEMETRY_ITEMS,
        },
        retention: PrivacySection {
            summary: "Retention is primarily operator-controlled for local files and service-controlled for any configured external endpoint.",
            items: RETENTION_ITEMS,
        },
        controls: CONTROL_ITEMS,
        references: PrivacyReferences {
            docs_path: PRIVACY_DOC_PATH,
            local_api_path: LOCAL_API_PRIVACY_PATH,
        },
    }
}

pub fn render_privacy_markdown() -> String {
    let privacy = privacy_statement();
    let mut out = String::new();
    writeln!(&mut out, "# Privacy\n").expect("write privacy title");
    writeln!(&mut out, "{}\n", privacy.summary).expect("write privacy summary");
    writeln!(
        &mut out,
        "This summary covers the interactive CLI and the LocalApiServer surface implemented in this tree. It does not replace the policies of the configured model endpoint, MCP servers, or any other external service.\n"
    )
    .expect("write privacy scope");

    write_surface_section(&mut out, "CLI surface", privacy.client);
    write_surface_section(&mut out, "Local API surface", privacy.server);
    write_section(&mut out, "Credentials", privacy.credentials);
    write_section(&mut out, "Telemetry and logs", privacy.telemetry);
    write_section(&mut out, "Retention", privacy.retention);

    writeln!(&mut out, "## Operator controls").expect("write controls heading");
    write_bullets(&mut out, privacy.controls);

    writeln!(&mut out, "## References").expect("write references heading");
    writeln!(&mut out, "- CLI guide: `{}`", privacy.references.docs_path)
        .expect("write docs reference");
    writeln!(
        &mut out,
        "- LocalApiServer metadata endpoint: `{}`",
        privacy.references.local_api_path
    )
    .expect("write api reference");

    out
}

fn write_surface_section(out: &mut String, title: &str, section: PrivacySurface) {
    writeln!(out, "## {title}").expect("write surface heading");
    writeln!(out, "{}", section.summary).expect("write surface summary");
    writeln!(out).expect("write surface spacing");
    writeln!(out, "### What stays on this machine or local surface")
        .expect("write local storage heading");
    write_bullets(out, section.local_storage);
    writeln!(out, "### What may cross a configured boundary")
        .expect("write remote transfer heading");
    write_bullets(out, section.remote_transfer);
    writeln!(out, "### Protections and defaults").expect("write protections heading");
    write_bullets(out, section.protections);
}

fn write_section(out: &mut String, title: &str, section: PrivacySection) {
    writeln!(out, "## {title}").expect("write section heading");
    writeln!(out, "{}", section.summary).expect("write section summary");
    writeln!(out).expect("write section spacing");
    write_bullets(out, section.items);
}

fn write_bullets(out: &mut String, items: &[&str]) {
    for item in items {
        writeln!(out, "- {item}").expect("write privacy bullet");
    }
    writeln!(out).expect("write privacy spacing");
}

#[cfg(test)]
mod tests {
    use super::{
        LOCAL_API_PRIVACY_PATH, PRIVACY_DOC_PATH, PRIVACY_VERSION, privacy_statement,
        render_privacy_markdown,
    };

    #[test]
    fn privacy_statement_exposes_expected_references() {
        let statement = privacy_statement();
        assert_eq!(statement.version, PRIVACY_VERSION);
        assert_eq!(statement.references.docs_path, PRIVACY_DOC_PATH);
        assert_eq!(statement.references.local_api_path, LOCAL_API_PRIVACY_PATH);
    }

    #[test]
    fn rendered_privacy_markdown_mentions_cli_and_server_surfaces() {
        let markdown = render_privacy_markdown();
        assert!(markdown.contains("## CLI surface"));
        assert!(markdown.contains("## Local API surface"));
        assert!(markdown.contains(LOCAL_API_PRIVACY_PATH));
        assert!(markdown.contains("VEX_MODEL_TOKEN"));
    }
}
