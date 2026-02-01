//! Online Runtime Adapter
//!
//! Proxies inference requests to online OpenAI-compatible API endpoints:
//! - OpenRouter (https://openrouter.ai)
//! - OpenAI API
//! - Any OpenAI-compatible endpoint
//!
//! This allows users to use cloud models when they don't have a local GPU
//! or want access to larger models (GPT-4, Claude, etc.)

use async_trait::async_trait;
use super::runtime_trait::*;
use std::time::Duration;
use tracing::info;

/// Configuration for online API connection
#[derive(Debug, Clone)]
pub struct OnlineConfig {
    /// API base URL (e.g., "https://openrouter.ai/api/v1")
    pub api_base_url: String,
    /// API key for authentication
    pub api_key: String,
    /// Model identifier (e.g., "meta-llama/llama-3-8b-instruct")
    pub model_id: String,
    /// Provider name for display
    pub provider: String,
}

impl Default for OnlineConfig {
    fn default() -> Self {
        Self {
            api_base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key: String::new(),
            model_id: "meta-llama/llama-3-8b-instruct".to_string(),
            provider: "OpenRouter".to_string(),
        }
    }
}

pub struct OnlineRuntime {
    config: Option<OnlineConfig>,
    http_client: reqwest::Client,
}

impl OnlineRuntime {
    pub fn new() -> Self {
        Self {
            config: None,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Create from explicit online config
    pub fn with_config(online_config: OnlineConfig) -> Self {
        Self {
            config: Some(online_config),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }

    fn get_config(&self) -> anyhow::Result<&OnlineConfig> {
        self.config.as_ref().ok_or_else(|| anyhow::anyhow!("Online runtime not configured"))
    }
}

impl Default for OnlineRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ModelRuntime for OnlineRuntime {
    fn supported_format(&self) -> ModelFormat {
        ModelFormat::Online
    }

    async fn initialize(&mut self, config: RuntimeConfig) -> anyhow::Result<()> {
        info!("Initializing Online runtime");

        // Extract online config from extra_config
        let api_base_url = config.extra_config["api_base_url"]
            .as_str()
            .unwrap_or("https://openrouter.ai/api/v1")
            .to_string();
        let api_key = config.extra_config["api_key"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let model_id = config.extra_config["model_id"]
            .as_str()
            .unwrap_or("meta-llama/llama-3-8b-instruct")
            .to_string();
        let provider = config.extra_config["provider"]
            .as_str()
            .unwrap_or("OpenRouter")
            .to_string();

        if api_key.is_empty() {
            return Err(anyhow::anyhow!(
                "API key is required for online models. Set OPENROUTER_API_KEY or OPENAI_API_KEY environment variable."
            ));
        }

        self.config = Some(OnlineConfig {
            api_base_url,
            api_key,
            model_id,
            provider,
        });

        info!("Online runtime configured for {} ({})",
            self.config.as_ref().unwrap().provider,
            self.config.as_ref().unwrap().model_id
        );

        Ok(())
    }

    async fn is_ready(&self) -> bool {
        self.config.is_some()
    }

    async fn health_check(&self) -> anyhow::Result<String> {
        let cfg = self.get_config()?;

        // Quick connectivity check
        let resp = self.http_client
            .get(format!("{}/models", cfg.api_base_url))
            .header("Authorization", format!("Bearer {}", cfg.api_key))
            .timeout(Duration::from_secs(10))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => Ok("healthy (online)".to_string()),
            Ok(r) => Err(anyhow::anyhow!("API returned: {}", r.status())),
            Err(e) => Err(anyhow::anyhow!("Connection failed: {}", e)),
        }
    }

    fn base_url(&self) -> String {
        self.config.as_ref()
            .map(|c| c.api_base_url.clone())
            .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string())
    }

    fn completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url())
    }

    async fn generate(
        &self,
        request: InferenceRequest,
    ) -> anyhow::Result<InferenceResponse> {
        let cfg = self.get_config()?;

        let payload = serde_json::json!({
            "model": cfg.model_id,
            "messages": request.messages,
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "stream": false,
        });

        let resp = self.http_client.post(&self.completions_url())
            .header("Authorization", format!("Bearer {}", cfg.api_key))
            .header("HTTP-Referer", "https://aud.io")
            .header("X-Title", "Aud.io CLI")
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Online inference request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Online inference failed ({}): {}", status, body));
        }

        let response: serde_json::Value = resp.json().await?;
        let content = response["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let finish_reason = response["choices"][0]["finish_reason"]
            .as_str()
            .map(|s| s.to_string());

        Ok(InferenceResponse {
            content,
            finish_reason,
        })
    }

    async fn generate_stream(
        &self,
        request: InferenceRequest,
    ) -> anyhow::Result<Box<dyn futures_util::Stream<Item = Result<String, anyhow::Error>> + Send + Unpin>> {
        use futures_util::StreamExt;

        let cfg = self.get_config()?;

        let payload = serde_json::json!({
            "model": cfg.model_id,
            "messages": request.messages,
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "stream": true,
        });

        let resp = self.http_client.post(&self.completions_url())
            .header("Authorization", format!("Bearer {}", cfg.api_key))
            .header("HTTP-Referer", "https://aud.io")
            .header("X-Title", "Aud.io CLI")
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Online stream request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Online stream failed ({}): {}", status, body));
        }

        let byte_stream = resp.bytes_stream();

        let sse_stream = async_stream::try_stream! {
            let mut buffer = String::new();
            futures_util::pin_mut!(byte_stream);

            while let Some(chunk_result) = byte_stream.next().await {
                let chunk = chunk_result.map_err(|e| anyhow::anyhow!("Stream read error: {}", e))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.is_empty() || !line.starts_with("data: ") {
                        continue;
                    }

                    let data = &line[6..];
                    if data == "[DONE]" {
                        return;
                    }

                    yield format!("data: {}\n\n", data);
                }
            }
        };

        Ok(Box::new(Box::pin(sse_stream)))
    }

    async fn shutdown(&mut self) -> anyhow::Result<()> {
        info!("Shutting down Online runtime");
        self.config = None;
        Ok(())
    }

    fn metadata(&self) -> RuntimeMetadata {
        let (runtime_name, model) = self.config.as_ref()
            .map(|c| (c.provider.clone(), c.model_id.clone()))
            .unwrap_or_else(|| ("Online API".to_string(), "unknown".to_string()));

        RuntimeMetadata {
            format: ModelFormat::Online,
            runtime_name: format!("{} ({})", runtime_name, model),
            version: "api".to_string(),
            supports_gpu: true, // Cloud-side GPU
            supports_streaming: true,
        }
    }
}
