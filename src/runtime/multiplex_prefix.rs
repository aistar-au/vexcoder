use serde_json::{Map, Value};
use std::collections::{HashMap, VecDeque};
use xxhash_rust::xxh3::xxh3_128;

pub const FINGERPRINT_VERSION: u16 = 2;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharedPrefix {
    pub system_prompt: String,
    pub tools: Vec<ToolDescriptor>,
    pub workspace_context: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: String,
    pub schema: Value,
}

impl SharedPrefix {
    pub fn fingerprint(&self) -> String {
        let implemented = self.implemented_json();
        let digest = xxh3_128(implemented.as_bytes());
        format!("v{}-{:032x}", FINGERPRINT_VERSION, digest)
    }

    pub fn implemented_json(&self) -> String {
        thin_json_string(&self.implemented_value())
    }

    fn implemented_value(&self) -> Value {
        struct ImplementedTool {
            name: String,
            schema: Value,
            schema_json: String,
        }

        let mut tools = self
            .tools
            .iter()
            .map(|tool| {
                let schema = thin_json_value(&tool.schema);
                let schema_json = thin_json_string(&schema);
                ImplementedTool {
                    name: tool.name.clone(),
                    schema,
                    schema_json,
                }
            })
            .collect::<Vec<_>>();
        tools.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| a.schema_json.cmp(&b.schema_json))
        });

        let mut implemented = Map::new();
        implemented.insert(
            "system_prompt".to_string(),
            Value::String(normalise_prompt(&self.system_prompt)),
        );
        implemented.insert(
            "tools".to_string(),
            Value::Array(
                tools
                    .into_iter()
                    .map(|tool| {
                        let mut entry = Map::new();
                        entry.insert("name".to_string(), Value::String(tool.name));
                        entry.insert("schema".to_string(), tool.schema);
                        Value::Object(entry)
                    })
                    .collect(),
            ),
        );
        implemented.insert(
            "workspace_context".to_string(),
            Value::String(self.workspace_context.trim_end().to_string()),
        );
        Value::Object(implemented)
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

fn thin_json_string(value: &Value) -> String {
    serde_json::to_string(&thin_json_value(value)).unwrap_or_else(|_| "null".to_string())
}

fn thin_json_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(thin_json_value).collect()),
        Value::Object(map) => {
            let mut sorted = Map::new();
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            for key in keys {
                if let Some(entry) = map.get(&key) {
                    sorted.insert(key, thin_json_value(entry));
                }
            }
            Value::Object(sorted)
        }
        _ => value.clone(),
    }
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
        let implemented = prefix.implemented_json().into_bytes();
        self.insert(fingerprint.clone(), implemented);
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
    use serde_json::json;

    fn sample_tool(name: &str, schema: Value) -> ToolDescriptor {
        ToolDescriptor {
            name: name.into(),
            schema,
        }
    }

    fn sample_prefix() -> SharedPrefix {
        SharedPrefix {
            system_prompt: "You are a helpful assistant.\n".into(),
            tools: vec![
                sample_tool("read_file", json!({"type": "object"})),
                sample_tool("write_file", json!({"type": "object"})),
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
        mutated.tools[0].schema = json!({"additional": true, "type": "object"});
        assert_ne!(base.fingerprint(), mutated.fingerprint());
    }

    #[test]
    fn fingerprint_ignores_schema_key_order() {
        let a = SharedPrefix {
            tools: vec![sample_tool(
                "read_file",
                json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "offset": {"type": "integer"}
                    },
                    "required": ["path"]
                }),
            )],
            ..SharedPrefix::default()
        };
        let b = SharedPrefix {
            tools: vec![sample_tool(
                "read_file",
                json!({
                    "required": ["path"],
                    "properties": {
                        "offset": {"type": "integer"},
                        "path": {"type": "string"}
                    },
                    "type": "object"
                }),
            )],
            ..SharedPrefix::default()
        };
        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn fingerprint_distinguishes_structured_prefixes_that_tab_joining_can_alias() {
        let left = SharedPrefix {
            tools: vec![sample_tool(
                "tool",
                json!({
                    "type": "object",
                    "properties": {
                        "payload": {"type": "string"},
                        "workspace": {"type": "string", "default": "A"}
                    }
                }),
            )],
            workspace_context: "B".into(),
            ..SharedPrefix::default()
        };
        let right = SharedPrefix {
            tools: vec![sample_tool(
                "tool",
                json!({
                    "type": "object",
                    "properties": {
                        "payload": {"type": "string"}
                    }
                }),
            )],
            workspace_context: "A\nworkspace:\nB".into(),
            ..SharedPrefix::default()
        };

        assert_ne!(left.implemented_json(), right.implemented_json());
        assert_ne!(left.fingerprint(), right.fingerprint());
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
    fn implemented_json_is_structured_and_parseable() {
        let prefix = sample_prefix();
        let implemented = prefix.implemented_json();
        let parsed: Value = serde_json::from_str(&implemented).expect("implemented JSON parses");
        assert_eq!(
            parsed["system_prompt"],
            Value::String("You are a helpful assistant.".into())
        );
        assert!(parsed["tools"].is_array());
        assert_eq!(
            parsed["workspace_context"],
            Value::String("cwd: /srv/repo".into())
        );
    }
}
