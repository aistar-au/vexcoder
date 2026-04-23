use std::collections::{HashMap, VecDeque};

pub const FINGERPRINT_VERSION: u16 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharedPrefix {
    pub system_prompt: String,
    pub tools: Vec<ToolDescriptor>,
    pub workspace_context: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: String,
    pub schema: String,
}

impl SharedPrefix {
    pub fn fingerprint(&self) -> String {
        let canonical = self.canonicalise();
        let digest = fnv1a_128(canonical.as_bytes());
        format!("v{}-{:032x}", FINGERPRINT_VERSION, digest)
    }

    pub fn canonicalise(&self) -> String {
        let mut out = String::new();
        out.push_str("system:\n");
        out.push_str(&normalise_prompt(&self.system_prompt));
        out.push_str("\ntools:\n");
        let mut tools = self.tools.clone();
        tools.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.schema.cmp(&b.schema)));
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

#[derive(Debug)]
pub struct MultiplexPrefixManager {
    capacity: usize,
    entries: HashMap<String, Vec<u8>>,
    insertion_order: VecDeque<String>,
}

impl MultiplexPrefixManager {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            insertion_order: VecDeque::new(),
        }
    }

    pub fn register(&mut self, prefix: &SharedPrefix) -> String {
        let fingerprint = prefix.fingerprint();
        if self.entries.contains_key(&fingerprint) {
            return fingerprint;
        }
        let canonical = prefix.canonicalise().into_bytes();
        self.insert(fingerprint.clone(), canonical);
        fingerprint
    }

    pub fn insert(&mut self, fingerprint: String, blob: Vec<u8>) {
        if self.entries.contains_key(&fingerprint) {
            return;
        }
        while self.insertion_order.len() >= self.capacity {
            match self.insertion_order.pop_front() {
                Some(oldest) => {
                    self.entries.remove(&oldest);
                }
                None => break,
            }
        }
        self.insertion_order.push_back(fingerprint.clone());
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
        let version_prefix = format!("v{FINGERPRINT_VERSION}-");
        assert_eq!(first, second);
        assert!(first.starts_with(&version_prefix));
        assert_eq!(first.len(), version_prefix.len() + 32);
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
