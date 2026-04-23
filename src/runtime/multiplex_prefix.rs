//! Shared-prefix fingerprinting for prompt multiplexing.
//!
//! A subtask fan-out typically produces N requests that share an
//! identical system prompt, tool schema, and workspace preamble, and
//! differ only in a short task-specific tail. When the upstream
//! transport exposes a prompt-cache key surface, issuing those N
//! requests with a common stable identifier is sufficient for the
//! provider to amortise prefix re-processing across the fan-out.
//!
//! This module produces such an identifier. It is an application-layer
//! fingerprint; it is not cryptographically secure, and it does not
//! attempt to detect adversarial collisions. Its sole property of
//! interest is determinism: two `SharedPrefix` values that are
//! semantically equal in the sense defined by [`SharedPrefix::canonicalise`]
//! produce the same fingerprint across processes and Rust toolchain
//! versions, subject to the `FINGERPRINT_VERSION` constant.
//!
//! # Canonicalisation
//!
//! The canonical form rejects three common sources of spurious
//! divergence:
//!
//! 1. tool ordering — tools are sorted lexicographically by name;
//! 2. trailing whitespace — each line of the system prompt is
//!    right-trimmed and the prompt is terminated by a single newline;
//! 3. empty workspace context — an absent preamble is treated as the
//!    empty string.
//!
//! Additional volatile fields (wall-clock timestamps, git HEAD, process
//! identifiers) are excluded by construction: they are not inputs to
//! the fingerprint.

use std::collections::HashMap;

/// Fingerprint-format version. Bump when the canonicalisation rules
/// change in a way that would invalidate previously cached entries.
pub const FINGERPRINT_VERSION: u16 = 1;

/// Canonical representation of a shared prompt prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharedPrefix {
    pub system_prompt: String,
    pub tools: Vec<ToolDescriptor>,
    pub workspace_context: String,
}

/// Minimal tool description used by the fingerprinter.
///
/// The `schema` field is the tool's JSON schema serialised as a
/// canonical string; callers are responsible for emitting it with
/// sorted keys. For schemas produced by `serde_json::to_string` on a
/// `BTreeMap`-backed value this is automatic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: String,
    pub schema: String,
}

impl SharedPrefix {
    /// Return a deterministic, lowercase hex fingerprint of the
    /// canonical representation.
    ///
    /// The fingerprint width is 32 hexadecimal characters (128 bits).
    /// It is prefixed with `v{FINGERPRINT_VERSION}-` so that consumers
    /// can route across version migrations without collision.
    pub fn fingerprint(&self) -> String {
        let canonical = self.canonicalise();
        let digest = fnv1a_128(canonical.as_bytes());
        format!("v{}-{:032x}", FINGERPRINT_VERSION, digest)
    }

    /// Return the canonical byte sequence used to compute the
    /// fingerprint. Exposed for test inspection and for callers that
    /// wish to feed the canonical form into a different hash.
    pub fn canonicalise(&self) -> String {
        let mut out = String::new();
        out.push_str("system:\n");
        out.push_str(&normalise_prompt(&self.system_prompt));
        out.push_str("\ntools:\n");
        let mut tools = self.tools.clone();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        for tool in &tools {
            out.push_str(&tool.name);
            out.push('\t');
            out.push_str(&tool.schema);
            out.push('\n');
        }
        out.push_str("workspace:\n");
        out.push_str(self.workspace_context.trim_end());
        out.push('\n');
        out
    }
}

fn normalise_prompt(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for (index, line) in input.lines().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end());
    }
    out
}

/// FNV-1a 128-bit fingerprint. Constants are from the published FNV
/// reference <http://www.isthe.com/chongo/tech/comp/fnv/index.html>;
/// no third-party attribution is required.
fn fnv1a_128(bytes: &[u8]) -> u128 {
    const OFFSET: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000000001000000000000000000013b;
    let mut hash = OFFSET;
    for &byte in bytes {
        hash ^= byte as u128;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Bounded in-memory registry that associates prefix fingerprints with
/// arbitrary opaque blobs.
///
/// The registry uses a first-in, first-out eviction discipline when the
/// `capacity` threshold is reached. FIFO is chosen in preference to a
/// recency-based policy because the expected workload is a short burst
/// of sibling subtasks sharing one or two prefixes, for which any
/// reasonable policy produces equivalent cache residency.
#[derive(Debug)]
pub struct MultiplexPrefixManager {
    capacity: usize,
    entries: HashMap<String, Vec<u8>>,
    insertion_order: Vec<String>,
}

impl MultiplexPrefixManager {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            insertion_order: Vec::new(),
        }
    }

    /// Register `prefix` and return its fingerprint. If the fingerprint
    /// is already known, no re-insertion occurs.
    pub fn register(&mut self, prefix: &SharedPrefix) -> String {
        let fingerprint = prefix.fingerprint();
        if self.entries.contains_key(&fingerprint) {
            return fingerprint;
        }
        let canonical = prefix.canonicalise().into_bytes();
        self.insert(fingerprint.clone(), canonical);
        fingerprint
    }

    /// Register pre-serialised blob under `fingerprint`. Intended for
    /// callers that already hold the canonical form and wish to avoid
    /// re-materialising it.
    pub fn insert(&mut self, fingerprint: String, blob: Vec<u8>) {
        if self.entries.contains_key(&fingerprint) {
            return;
        }
        while self.insertion_order.len() >= self.capacity {
            if let Some(oldest) = self.insertion_order.first().cloned() {
                self.insertion_order.remove(0);
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
        self.insertion_order.push(fingerprint.clone());
        self.entries.insert(fingerprint, blob);
    }

    pub fn get(&self, fingerprint: &str) -> Option<&[u8]> {
        self.entries.get(fingerprint).map(Vec::as_slice)
    }

    pub fn contains(&self, fingerprint: &str) -> bool {
        self.entries.contains_key(fingerprint)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tool(name: &str, schema: &str) -> ToolDescriptor {
        ToolDescriptor {
            name: name.into(),
            schema: schema.into(),
        }
    }

    fn sample_prefix() -> SharedPrefix {
        SharedPrefix {
            system_prompt: "You are a helpful assistant.\n".into(),
            tools: vec![
                sample_tool("read_file", r#"{"type":"object"}"#),
                sample_tool("write_file", r#"{"type":"object"}"#),
            ],
            workspace_context: "cwd: /srv/repo".into(),
        }
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let prefix = sample_prefix();
        let first = prefix.fingerprint();
        let second = prefix.fingerprint();
        assert_eq!(first, second);
        assert!(first.starts_with(&format!("v{FINGERPRINT_VERSION}-")));
        assert_eq!(first.len(), "v1-".len() + 32);
    }

    #[test]
    fn fingerprint_is_tool_order_independent() {
        let mut a = sample_prefix();
        let mut b = sample_prefix();
        a.tools.sort_by(|x, y| x.name.cmp(&y.name));
        b.tools.sort_by(|x, y| y.name.cmp(&x.name));
        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn fingerprint_ignores_trailing_whitespace_on_prompt_lines() {
        let plain = SharedPrefix {
            system_prompt: "line one\nline two".into(),
            ..SharedPrefix::default()
        };
        let padded = SharedPrefix {
            system_prompt: "line one   \nline two\t".into(),
            ..SharedPrefix::default()
        };
        assert_eq!(plain.fingerprint(), padded.fingerprint());
    }

    #[test]
    fn fingerprint_changes_when_system_prompt_differs() {
        let a = SharedPrefix {
            system_prompt: "instruction A".into(),
            ..SharedPrefix::default()
        };
        let b = SharedPrefix {
            system_prompt: "instruction B".into(),
            ..SharedPrefix::default()
        };
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn fingerprint_changes_when_tool_schema_differs() {
        let base = sample_prefix();
        let mut mutated = base.clone();
        mutated.tools[0].schema = r#"{"type":"object","additional":true}"#.into();
        assert_ne!(base.fingerprint(), mutated.fingerprint());
    }

    #[test]
    fn manager_register_returns_stable_fingerprint_and_caches_blob() {
        let mut manager = MultiplexPrefixManager::new(4);
        let prefix = sample_prefix();
        let first = manager.register(&prefix);
        let second = manager.register(&prefix);
        assert_eq!(first, second);
        assert_eq!(manager.len(), 1);
        let blob = manager.get(&first).expect("blob retained");
        assert!(!blob.is_empty());
    }

    #[test]
    fn manager_evicts_oldest_when_capacity_exceeded() {
        let mut manager = MultiplexPrefixManager::new(2);
        let variant_for = |suffix: &str| SharedPrefix {
            system_prompt: format!("prompt {suffix}"),
            ..SharedPrefix::default()
        };
        let a = manager.register(&variant_for("a"));
        let b = manager.register(&variant_for("b"));
        let c = manager.register(&variant_for("c"));
        assert_eq!(manager.len(), 2);
        assert!(!manager.contains(&a), "oldest entry must be evicted");
        assert!(manager.contains(&b));
        assert!(manager.contains(&c));
    }

    #[test]
    fn manager_capacity_zero_is_normalised_to_one() {
        let mut manager = MultiplexPrefixManager::new(0);
        let prefix = sample_prefix();
        let fp = manager.register(&prefix);
        assert!(manager.contains(&fp));
        assert_eq!(manager.len(), 1);
    }

    #[test]
    fn canonical_form_begins_with_sections_in_declared_order() {
        let prefix = sample_prefix();
        let canonical = prefix.canonicalise();
        let system_at = canonical.find("system:").expect("system section present");
        let tools_at = canonical.find("tools:").expect("tools section present");
        let ws_at = canonical
            .find("workspace:")
            .expect("workspace section present");
        assert!(system_at < tools_at);
        assert!(tools_at < ws_at);
    }
}
