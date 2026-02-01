//! GGUF Runtime Adapter
//!
//! Wraps the existing llama-server.exe (llama.cpp) for GGUF models.
//! This adapter spawns the llama-server process and proxies requests via HTTP.

use async_trait::async_trait;
use super::runtime_trait::*;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tracing::{info, warn};
use tokio::time::sleep;

pub struct GGUFRuntime {
    config: Option<RuntimeConfig>,
    server_process: Option<Child>,
    http_client: reqwest::Client,
    base_url: String,
}

impl GGUFRuntime {
    pub fn new() -> Self {
        Self {
            config: None,
            server_process: None,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(600))
                .build()
                .unwrap_or_default(),
            base_url: String::new(),
        }
    }

    /// Start llama-server process
    async fn start_server(&mut self, config: &RuntimeConfig) -> anyhow::Result<()> {
        let binary_path = config.runtime_binary.as_ref()
            .ok_or_else(|| anyhow::anyhow!("GGUF runtime requires runtime_binary path"))?;

        if !binary_path.exists() {
            return Err(anyhow::anyhow!(
                "llama-server binary not found at: {}",
                binary_path.display()
            ));
        }

        info!("Starting llama-server for GGUF model: {}", config.model_path.display());
        info!("  Binary: {}", binary_path.display());
        info!("  Port: {}", config.port);
        info!("  Context Size: {}", config.context_size);
        info!("  GPU Layers: {}", config.gpu_layers);

        // Build command arguments
        let mut cmd = Command::new(binary_path);
        cmd.arg("--model").arg(&config.model_path)
            .arg("--host").arg(&config.host)
            .arg("--port").arg(config.port.to_string())
            .arg("--ctx-size").arg(config.context_size.to_string())
            .arg("--batch-size").arg(config.batch_size.to_string())
            .arg("--threads").arg(config.threads.to_string())
            .arg("--n-gpu-layers").arg(config.gpu_layers.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Spawn the process
        let child = cmd.spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn llama-server: {}", e))?;

        self.server_process = Some(child);
        self.base_url = format!("http://{}:{}", config.host, config.port);

        info!("llama-server process started, waiting for health check...");

        // Wait for server to be ready (up to 60 seconds)
        for attempt in 1..=30 {
            sleep(Duration::from_secs(2)).await;
            
            if self.is_ready().await {
                info!("✅ GGUF runtime ready after {} seconds", attempt * 2);
                return Ok(());
            }
            
            if attempt % 5 == 0 {
                info!("Still waiting for llama-server... ({}/60s)", attempt * 2);
            }
        }

        Err(anyhow::anyhow!("llama-server failed to start within 60 seconds"))
    }
}

impl Default for GGUFRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ModelRuntime for GGUFRuntime {
    fn supported_format(&self) -> ModelFormat {
        ModelFormat::GGUF
    }

    async fn initialize(&mut self, config: RuntimeConfig) -> anyhow::Result<()> {
        info!("Initializing GGUF runtime");
        
        // Validate config
        if config.format != ModelFormat::GGUF {
            return Err(anyhow::anyhow!(
                "GGUF runtime received wrong format: {:?}",
                config.format
            ));
        }

        if !config.model_path.exists() {
            return Err(anyhow::anyhow!(
                "Model file not found: {}",
                config.model_path.display()
            ));
        }

        self.config = Some(config.clone());
        self.start_server(&config).await?;
        
        Ok(())
    }

    async fn is_ready(&self) -> bool {
        if self.base_url.is_empty() {
            return false;
        }

        let health_url = format!("{}/health", self.base_url);
        match self.http_client.get(&health_url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    async fn health_check(&self) -> anyhow::Result<String> {
        if self.base_url.is_empty() {
            return Err(anyhow::anyhow!("Runtime not initialized"));
        }

        let health_url = format!("{}/health", self.base_url);
        let resp = self.http_client.get(&health_url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Health check failed: {}", e))?;

        if resp.status().is_success() {
            Ok("healthy".to_string())
        } else {
            Err(anyhow::anyhow!("Health check returned: {}", resp.status()))
        }
    }

    fn base_url(&self) -> String {
        self.base_url.clone()
    }

    async fn generate(
        &self,
        request: InferenceRequest,
    ) -> anyhow::Result<InferenceResponse> {
        let url = self.completions_url();
        
        let payload = serde_json::json!({
            "model": "local-llm",
            "messages": request.messages,
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "stream": false,
        });

        let resp = self.http_client.post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Inference request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Inference failed ({}): {}", status, body));
        }

        let response: serde_json::Value = resp.json().await
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

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
        
        let url = self.completions_url();
        
        let payload = serde_json::json!({
            "model": "local-llm",
            "messages": request.messages,
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "stream": true,
        });

        let resp = self.http_client.post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Stream request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Stream failed ({}): {}", status, body));
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
        info!("Shutting down GGUF runtime");
        
        if let Some(mut child) = self.server_process.take() {
            match child.kill() {
                Ok(_) => {
                    info!("llama-server process killed successfully");
                    let _ = child.wait();
                }
                Err(e) => {
                    warn!("Failed to kill llama-server process: {}", e);
                }
            }
        }

        self.config = None;
        self.base_url.clear();
        Ok(())
    }

    fn metadata(&self) -> RuntimeMetadata {
        RuntimeMetadata {
            format: ModelFormat::GGUF,
            runtime_name: "llama.cpp (llama-server)".to_string(),
            version: "latest".to_string(),
            supports_gpu: true,
            supports_streaming: true,
        }
    }
}

impl Drop for GGUFRuntime {
    fn drop(&mut self) {
        if let Some(mut child) = self.server_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
