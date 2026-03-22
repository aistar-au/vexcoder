/// Async embedding provider client for Phase 2 semantic vector search.
///
/// Reads configuration from environment variables:
/// - `VEX_EMBEDDING_PROVIDER` — `"openai"` | `"openai-compat"` | `"ollama"`
/// - `VEX_EMBEDDING_MODEL`    — model name (e.g. `"text-embedding-3-small"`)
/// - `VEX_EMBEDDING_URL`      — optional base URL override
/// - `VEX_EMBEDDING_API_KEY`  — optional API key
///
/// All network calls are async; this module must only be called from async
/// context. It must NOT be used inside `tokio::task::spawn_blocking`.
use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// Identifies the embedding service protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingProvider {
    /// OpenAI public API (`POST /v1/embeddings`, batched).
    OpenAi,
    /// Any OpenAI-compatible local/remote endpoint (same wire format, batched).
    OpenAiCompat,
    /// Ollama local server (`POST /api/embeddings`, one request per text).
    Ollama,
}

/// Embedding provider configuration read from environment variables.
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub provider: EmbeddingProvider,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
}

impl EmbeddingConfig {
    /// Load configuration from environment.  Returns `None` when
    /// `VEX_EMBEDDING_PROVIDER` is absent, empty, or set to `"none"`.
    pub fn from_env() -> Option<Self> {
        let raw = std::env::var("VEX_EMBEDDING_PROVIDER").ok()?;
        let provider_key = raw.trim().to_ascii_lowercase();
        if provider_key.is_empty() || provider_key == "none" {
            return None;
        }

        let (provider, default_url) = match provider_key.as_str() {
            "openai" => (
                EmbeddingProvider::OpenAi,
                "https://api.openai.com".to_string(),
            ),
            "openai-compat" | "openai_compat" => {
                let url = std::env::var("VEX_EMBEDDING_URL")
                    .unwrap_or_else(|_| "http://localhost:8000".to_string());
                (EmbeddingProvider::OpenAiCompat, url)
            }
            "ollama" => {
                let url = std::env::var("VEX_EMBEDDING_URL")
                    .unwrap_or_else(|_| "http://localhost:11434".to_string());
                (EmbeddingProvider::Ollama, url)
            }
            _ => return None,
        };

        let base_url = if provider == EmbeddingProvider::OpenAi {
            std::env::var("VEX_EMBEDDING_URL").unwrap_or(default_url)
        } else {
            default_url
        };

        let model = std::env::var("VEX_EMBEDDING_MODEL").unwrap_or_else(|_| {
            match provider {
                EmbeddingProvider::OpenAi | EmbeddingProvider::OpenAiCompat => {
                    "text-embedding-3-small".to_string()
                }
                EmbeddingProvider::Ollama => "nomic-embed-text".to_string(),
            }
        });

        let api_key = std::env::var("VEX_EMBEDDING_API_KEY").ok();

        Some(EmbeddingConfig {
            provider,
            model,
            base_url,
            api_key,
        })
    }
}

// ── Wire shapes ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OpenAiEmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Vec<OpenAiEmbeddingData>,
}

#[derive(Deserialize)]
struct OllamaEmbeddingResponse {
    embedding: Vec<f32>,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Embed a slice of texts using the configured provider.
///
/// Returns one `Vec<f32>` per input text, in the same order.
/// Must be called from async context only.
pub async fn embed_texts(texts: &[&str], config: &EmbeddingConfig) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client for embeddings")?;

    match config.provider {
        EmbeddingProvider::OpenAi | EmbeddingProvider::OpenAiCompat => {
            embed_openai_batch(&client, texts, config).await
        }
        EmbeddingProvider::Ollama => embed_ollama_sequential(&client, texts, config).await,
    }
}

// ── Provider implementations ─────────────────────────────────────────────────

async fn embed_openai_batch(
    client: &reqwest::Client,
    texts: &[&str],
    config: &EmbeddingConfig,
) -> Result<Vec<Vec<f32>>> {
    let url = format!("{}/v1/embeddings", config.base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": config.model,
        "input": texts,
    });

    let mut req = client.post(&url).json(&body);
    if let Some(ref key) = config.api_key {
        req = req.bearer_auth(key);
    }

    let resp = req.send().await.context("embedding API request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("embedding API returned {status}: {body}");
    }

    let mut parsed: OpenAiEmbeddingResponse =
        resp.json().await.context("embedding response parse error")?;
    // OpenAI guarantees order by index; sort defensively.
    parsed.data.sort_by_key(|d| d.index);
    Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
}

async fn embed_ollama_sequential(
    client: &reqwest::Client,
    texts: &[&str],
    config: &EmbeddingConfig,
) -> Result<Vec<Vec<f32>>> {
    let url = format!("{}/api/embeddings", config.base_url.trim_end_matches('/'));
    let mut results = Vec::with_capacity(texts.len());

    for text in texts {
        let body = serde_json::json!({
            "model": config.model,
            "prompt": text,
        });

        let mut req = client.post(&url).json(&body);
        if let Some(ref key) = config.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req.send().await.context("Ollama embedding request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("Ollama embedding API returned {status}: {body}");
        }

        let parsed: OllamaEmbeddingResponse =
            resp.json().await.context("Ollama embedding response parse error")?;
        results.push(parsed.embedding);
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_env_returns_none_when_unset() {
        // Ensure no leftover env from other tests.
        std::env::remove_var("VEX_EMBEDDING_PROVIDER");
        assert!(EmbeddingConfig::from_env().is_none());
    }

    #[test]
    fn test_from_env_returns_none_for_none_value() {
        std::env::set_var("VEX_EMBEDDING_PROVIDER", "none");
        assert!(EmbeddingConfig::from_env().is_none());
        std::env::remove_var("VEX_EMBEDDING_PROVIDER");
    }

    #[test]
    fn test_from_env_openai_defaults() {
        std::env::set_var("VEX_EMBEDDING_PROVIDER", "openai");
        std::env::remove_var("VEX_EMBEDDING_URL");
        std::env::remove_var("VEX_EMBEDDING_MODEL");
        std::env::remove_var("VEX_EMBEDDING_API_KEY");

        let cfg = EmbeddingConfig::from_env().expect("should be Some");
        assert_eq!(cfg.provider, EmbeddingProvider::OpenAi);
        assert_eq!(cfg.model, "text-embedding-3-small");
        assert!(cfg.base_url.contains("openai.com"));
        assert!(cfg.api_key.is_none());

        std::env::remove_var("VEX_EMBEDDING_PROVIDER");
    }

    #[test]
    fn test_from_env_ollama_defaults() {
        std::env::set_var("VEX_EMBEDDING_PROVIDER", "ollama");
        std::env::remove_var("VEX_EMBEDDING_URL");
        std::env::remove_var("VEX_EMBEDDING_MODEL");

        let cfg = EmbeddingConfig::from_env().expect("should be Some");
        assert_eq!(cfg.provider, EmbeddingProvider::Ollama);
        assert_eq!(cfg.model, "nomic-embed-text");
        assert!(cfg.base_url.contains("11434"));

        std::env::remove_var("VEX_EMBEDDING_PROVIDER");
    }

    #[test]
    fn test_from_env_openai_compat_custom_url() {
        std::env::set_var("VEX_EMBEDDING_PROVIDER", "openai-compat");
        std::env::set_var("VEX_EMBEDDING_URL", "http://my-server:1234");
        std::env::set_var("VEX_EMBEDDING_MODEL", "my-model");

        let cfg = EmbeddingConfig::from_env().expect("should be Some");
        assert_eq!(cfg.provider, EmbeddingProvider::OpenAiCompat);
        assert_eq!(cfg.base_url, "http://my-server:1234");
        assert_eq!(cfg.model, "my-model");

        std::env::remove_var("VEX_EMBEDDING_PROVIDER");
        std::env::remove_var("VEX_EMBEDDING_URL");
        std::env::remove_var("VEX_EMBEDDING_MODEL");
    }
}
