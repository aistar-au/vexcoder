use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

const DEFAULT_EMBED_BATCH_SIZE: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingProvider {
    Compat,
    Native,
}

impl EmbeddingProvider {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "compat" => Some(Self::Compat),
            "native" => Some(Self::Native),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Compat => "compat",
            Self::Native => "native",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingConfig {
    pub provider: EmbeddingProvider,
    pub model: String,
    pub url: String,
    pub api_key: Option<String>,
    pub batch_size: usize,
}

impl EmbeddingConfig {
    pub fn from_env() -> Result<Option<Self>> {
        let provider_raw = match std::env::var("VEX_EMBEDDING_PROVIDER") {
            Ok(value) if !value.trim().is_empty() => value,
            _ => return Ok(None),
        };

        let provider = EmbeddingProvider::parse(&provider_raw).ok_or_else(|| {
            anyhow!(
                "unsupported VEX_EMBEDDING_PROVIDER '{}'; expected compat or native",
                provider_raw
            )
        })?;

        let model = required_env("VEX_EMBEDDING_MODEL")?;
        let url = required_env("VEX_EMBEDDING_URL")?;
        let api_key = std::env::var("VEX_EMBEDDING_API_KEY")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let batch_size = std::env::var("VEX_EMBEDDING_BATCH_SIZE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_EMBED_BATCH_SIZE);

        Ok(Some(Self {
            provider,
            model,
            url: url.trim_end_matches('/').to_string(),
            api_key,
            batch_size,
        }))
    }
}

pub async fn embed_texts(config: &EmbeddingConfig, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let client = reqwest::Client::new();
    match config.provider {
        EmbeddingProvider::Compat => embed_compat_batches(&client, config, texts).await,
        EmbeddingProvider::Native => embed_native_batches(&client, config, texts).await,
    }
}

async fn embed_compat_batches(
    client: &reqwest::Client,
    config: &EmbeddingConfig,
    texts: &[String],
) -> Result<Vec<Vec<f32>>> {
    let endpoint = format!("{}/embeddings", config.url);
    let mut embeddings = Vec::with_capacity(texts.len());

    for chunk in texts.chunks(config.batch_size) {
        let mut request = client.post(&endpoint).json(&serde_json::json!({
            "model": config.model,
            "input": chunk,
        }));
        if let Some(api_key) = &config.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("failed to call embedding endpoint {endpoint}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("embedding request failed with {status}: {body}");
        }

        let mut payload: CompatEmbeddingResponse = response
            .json()
            .await
            .context("failed to decode embedding response")?;
        payload.data.sort_by_key(|item| item.index);
        embeddings.extend(payload.data.into_iter().map(|item| item.embedding));
    }

    Ok(embeddings)
}

async fn embed_native_batches(
    client: &reqwest::Client,
    config: &EmbeddingConfig,
    texts: &[String],
) -> Result<Vec<Vec<f32>>> {
    let endpoint = format!("{}/api/embeddings", config.url);
    let mut embeddings = Vec::with_capacity(texts.len());

    for text in texts {
        let response = client
            .post(&endpoint)
            .json(&serde_json::json!({
                "model": config.model,
                "prompt": text,
            }))
            .send()
            .await
            .with_context(|| format!("failed to call embedding endpoint {endpoint}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("embedding request failed with {status}: {body}");
        }

        let payload: NativeEmbeddingResponse = response
            .json()
            .await
            .context("failed to decode native embedding response")?;
        embeddings.push(payload.embedding);
    }

    Ok(embeddings)
}

fn required_env(name: &str) -> Result<String> {
    let value = std::env::var(name)
        .with_context(|| format!("{name} must be set when embeddings are enabled"))?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{name} must not be empty when embeddings are enabled");
    }
    Ok(trimmed.to_string())
}

#[derive(Debug, Deserialize)]
struct CompatEmbeddingResponse {
    data: Vec<CompatEmbeddingItem>,
}

#[derive(Debug, Deserialize)]
struct CompatEmbeddingItem {
    embedding: Vec<f32>,
    index: usize,
}

#[derive(Debug, Deserialize)]
struct NativeEmbeddingResponse {
    embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_config_absent_without_provider() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::remove_var("VEX_EMBEDDING_PROVIDER");
        std::env::remove_var("VEX_EMBEDDING_MODEL");
        std::env::remove_var("VEX_EMBEDDING_URL");
        std::env::remove_var("VEX_EMBEDDING_API_KEY");

        let config = EmbeddingConfig::from_env().unwrap();
        assert!(config.is_none());
    }

    #[test]
    fn test_embedding_config_parses_compat_provider() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_EMBEDDING_PROVIDER", "compat");
        std::env::set_var("VEX_EMBEDDING_MODEL", "text-embedding-3-small");
        std::env::set_var("VEX_EMBEDDING_URL", "https://example.invalid/v1/");
        std::env::set_var("VEX_EMBEDDING_API_KEY", "secret");

        let config = EmbeddingConfig::from_env()
            .unwrap()
            .expect("embedding config should parse");

        assert_eq!(config.provider, EmbeddingProvider::Compat);
        assert_eq!(config.model, "text-embedding-3-small");
        assert_eq!(config.url, "https://example.invalid/v1");
        assert_eq!(config.api_key.as_deref(), Some("secret"));

        std::env::remove_var("VEX_EMBEDDING_PROVIDER");
        std::env::remove_var("VEX_EMBEDDING_MODEL");
        std::env::remove_var("VEX_EMBEDDING_URL");
        std::env::remove_var("VEX_EMBEDDING_API_KEY");
    }
}
