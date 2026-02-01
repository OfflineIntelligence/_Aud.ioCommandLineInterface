//! LLM worker thread implementation
//!
//! Handles LLM inference by proxying requests to the local llama-server process.
//! This is the 1-hop architecture: shared memory state → HTTP to localhost llama-server.

use futures_util::StreamExt;
use tracing::{info, debug, warn};
use serde::{Deserialize, Serialize};

use crate::memory::Message;

/// Chat completion request sent to llama-server (OpenAI-compatible format)
#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: f32,
    stream: bool,
}

/// Embedding request sent to llama-server (OpenAI-compatible format)
#[derive(Debug, Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
}

/// Embedding response from llama-server
#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

/// Non-streaming response from llama-server
#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: Option<ChatMessage>,
}

/// Streaming delta chunk from llama-server
#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Option<ChatDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct ChatDelta {
    content: Option<String>,
}

pub struct LLMWorker {
    backend_url: String,
    http_client: reqwest::Client,
}

impl LLMWorker {
    /// Create with shared state (legacy constructor)
    pub fn new(shared_state: std::sync::Arc<crate::shared_state::SharedState>) -> Self {
        let backend_url = shared_state.config.backend_url.clone();
        Self {
            backend_url,
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Create with explicit backend URL
    pub fn new_with_backend(backend_url: String) -> Self {
        info!("LLM worker initialized with backend: {}", backend_url);
        Self {
            backend_url,
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Get the chat completions endpoint URL
    fn completions_url(&self) -> String {
        format!("{}/v1/chat/completions", self.backend_url)
    }

    /// Get the embeddings endpoint URL
    fn embeddings_url(&self) -> String {
        format!("{}/v1/embeddings", self.backend_url)
    }

    /// Convert internal Message format to OpenAI-compatible ChatMessage
    fn to_chat_messages(messages: &[Message]) -> Vec<ChatMessage> {
        messages.iter().map(|m| ChatMessage {
            role: m.role.clone(),
            content: m.content.clone(),
        }).collect()
    }

    /// Generate a complete (non-streaming) response from the LLM.
    pub async fn generate_response(
        &self,
        _session_id: String,
        context: Vec<Message>,
    ) -> anyhow::Result<String> {
        debug!("LLM worker generating response (non-streaming)");

        let request = ChatCompletionRequest {
            model: "local-llm".to_string(),
            messages: Self::to_chat_messages(&context),
            max_tokens: 2000,
            temperature: 0.7,
            stream: false,
        };

        let response = self.http_client
            .post(&self.completions_url())
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("LLM backend request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("LLM backend returned {}: {}", status, body));
        }

        let completion: ChatCompletionResponse = response.json().await
            .map_err(|e| anyhow::anyhow!("Failed to parse LLM response: {}", e))?;

        let content = completion.choices
            .first()
            .and_then(|c| c.message.as_ref())
            .map(|m| m.content.clone())
            .unwrap_or_default();

        Ok(content)
    }

    /// Stream response tokens from the LLM as Server-Sent Events.
    /// Returns a stream of SSE-formatted strings ready to send to the client.
    pub async fn stream_response(
        &self,
        messages: Vec<Message>,
        max_tokens: u32,
        temperature: f32,
    ) -> anyhow::Result<impl futures_util::Stream<Item = Result<String, anyhow::Error>>> {
        debug!("LLM worker starting streaming response");

        let request = ChatCompletionRequest {
            model: "local-llm".to_string(),
            messages: Self::to_chat_messages(&messages),
            max_tokens,
            temperature,
            stream: true,
        };

        let response = self.http_client
            .post(&self.completions_url())
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("LLM backend request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("LLM backend returned {}: {}", status, body));
        }

        let byte_stream = response.bytes_stream();

        let sse_stream = async_stream::try_stream! {
            let mut buffer = String::new();

            futures_util::pin_mut!(byte_stream);

            while let Some(chunk_result) = byte_stream.next().await {
                let chunk = chunk_result
                    .map_err(|e| anyhow::anyhow!("Stream read error: {}", e))?;

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    if line.starts_with("data: ") {
                        let data = &line[6..];

                        if data == "[DONE]" {
                            yield "data: [DONE]\n\n".to_string();
                            return;
                        }

                        match serde_json::from_str::<StreamChunk>(data) {
                            Ok(chunk) => {
                                let finished = chunk.choices.iter()
                                    .any(|c| c.finish_reason.is_some());

                                yield format!("data: {}\n\n", data);

                                if finished {
                                    yield "data: [DONE]\n\n".to_string();
                                    return;
                                }
                            }
                            Err(_) => {
                                yield format!("data: {}\n\n", data);
                            }
                        }
                    }
                }
            }

            yield "data: [DONE]\n\n".to_string();
        };

        Ok(sse_stream)
    }

    /// Batch process multiple prompts (non-streaming)
    pub async fn batch_process(
        &self,
        prompts: Vec<(String, Vec<Message>)>,
    ) -> anyhow::Result<Vec<String>> {
        debug!("LLM worker batch processing {} prompts", prompts.len());

        let mut responses = Vec::new();
        for (session_id, messages) in prompts {
            match self.generate_response(session_id.clone(), messages).await {
                Ok(response) => responses.push(response),
                Err(e) => {
                    warn!("Batch item {} failed: {}", session_id, e);
                    responses.push(format!("Error: {}", e));
                }
            }
        }

        info!("Batch processed {} prompts", responses.len());
        Ok(responses)
    }

    /// Initialize LLM model (no-op for HTTP proxy mode)
    pub async fn initialize_model(&self, model_path: &str) -> anyhow::Result<()> {
        debug!("LLM worker model init (HTTP proxy mode): {}", model_path);
        Ok(())
    }

    /// Generate embeddings for one or more text inputs via llama-server's /v1/embeddings endpoint.
    /// This reuses the vectors llama.cpp already computes during inference — no separate model needed.
    /// Returns a Vec of embedding vectors (one per input string).
    pub async fn generate_embeddings(
        &self,
        texts: Vec<String>,
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        debug!("Generating embeddings for {} text(s) via llama-server", texts.len());

        let request = EmbeddingRequest {
            model: "local-llm".to_string(),
            input: texts,
        };

        let response = self.http_client
            .post(&self.embeddings_url())
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Embedding request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Embedding endpoint returned {}: {}", status, body));
        }

        let embedding_response: EmbeddingResponse = response.json().await
            .map_err(|e| anyhow::anyhow!("Failed to parse embedding response: {}", e))?;

        let embeddings: Vec<Vec<f32>> = embedding_response.data
            .into_iter()
            .map(|d| d.embedding)
            .collect();

        debug!("Generated {} embeddings (dim={})",
            embeddings.len(),
            embeddings.first().map(|e| e.len()).unwrap_or(0));

        Ok(embeddings)
    }

    /// Generate title for a chat using the LLM
    pub async fn generate_title(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> anyhow::Result<String> {
        debug!("LLM worker generating title for prompt ({} chars)", prompt.len());

        let messages = vec![Message {
            role: "user".to_string(),
            content: prompt.to_string(),
        }];

        let request = ChatCompletionRequest {
            model: "local-llm".to_string(),
            messages: Self::to_chat_messages(&messages),
            max_tokens: max_tokens.min(20),
            temperature: 0.3,
            stream: false,
        };

        let response = self.http_client
            .post(&self.completions_url())
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Title generation request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Title generation failed ({}): {}", status, body));
        }

        let completion: ChatCompletionResponse = response.json().await
            .map_err(|e| anyhow::anyhow!("Failed to parse title response: {}", e))?;

        let title = completion.choices
            .first()
            .and_then(|c| c.message.as_ref())
            .map(|m| m.content.trim().to_string())
            .unwrap_or_else(|| "New Chat".to_string());

        let title = title.trim_matches('"').trim_matches('\'').to_string();

        info!("Generated title: '{}'", title);
        Ok(title)
    }
}
