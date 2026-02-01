//! Core trait and types for model runtime abstraction

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Supported model formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelFormat {
    /// GGUF format (llama.cpp quantized)
    GGUF,
    /// GGML format (llama.cpp legacy)
    GGML,
    /// ONNX format (Open Neural Network Exchange)
    ONNX,
    /// TensorRT optimized format (NVIDIA)
    TensorRT,
    /// Safetensors format (Hugging Face)
    Safetensors,
    /// CoreML format (Apple)
    CoreML,
    /// Online API model (OpenRouter, OpenAI-compatible endpoints — requires internet)
    Online,
}

impl ModelFormat {
    /// Get file extensions for this format
    pub fn extensions(&self) -> &[&str] {
        match self {
            ModelFormat::GGUF => &["gguf"],
            ModelFormat::GGML => &["ggml", "bin"],
            ModelFormat::ONNX => &["onnx"],
            ModelFormat::TensorRT => &["trt", "engine", "plan"],
            ModelFormat::Safetensors => &["safetensors"],
            ModelFormat::CoreML => &["mlmodel", "mlpackage"],
            ModelFormat::Online => &[], // No file extension — API-based
        }
    }

    /// Get human-readable name
    pub fn name(&self) -> &str {
        match self {
            ModelFormat::GGUF => "GGUF (llama.cpp)",
            ModelFormat::GGML => "GGML (llama.cpp legacy)",
            ModelFormat::ONNX => "ONNX Runtime",
            ModelFormat::TensorRT => "TensorRT",
            ModelFormat::Safetensors => "Safetensors",
            ModelFormat::CoreML => "CoreML",
            ModelFormat::Online => "Online API",
        }
    }

    /// Whether this format requires a local file
    pub fn is_local(&self) -> bool {
        !matches!(self, ModelFormat::Online)
    }
}

/// Runtime configuration for model initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Path to model file
    pub model_path: PathBuf,
    /// Model format
    pub format: ModelFormat,
    /// Host for runtime server (e.g., "127.0.0.1")
    pub host: String,
    /// Port for runtime server (e.g., 8001)
    pub port: u16,
    /// Context size
    pub context_size: u32,
    /// Batch size
    pub batch_size: u32,
    /// Number of CPU threads
    pub threads: u32,
    /// GPU layers to offload (0 = CPU only)
    pub gpu_layers: u32,
    /// Path to runtime binary (e.g., llama-server.exe)
    pub runtime_binary: Option<PathBuf>,
    /// Additional runtime-specific configuration
    pub extra_config: serde_json::Value,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::new(),
            format: ModelFormat::GGUF,
            host: "127.0.0.1".to_string(),
            port: 8001,
            context_size: 8192,
            batch_size: 128,
            threads: 6,
            gpu_layers: 0,
            runtime_binary: None,
            extra_config: serde_json::json!({}),
        }
    }
}

/// Inference request (OpenAI-compatible format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_stream")]
    pub stream: bool,
}

fn default_max_tokens() -> u32 { 2000 }
fn default_temperature() -> f32 { 0.7 }
fn default_stream() -> bool { false }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Inference response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResponse {
    pub content: String,
    pub finish_reason: Option<String>,
}

/// Model runtime trait - all runtime adapters must implement this
#[async_trait]
pub trait ModelRuntime: Send + Sync {
    /// Get the format this runtime supports
    fn supported_format(&self) -> ModelFormat;

    /// Initialize the runtime (start server process, load model, etc.)
    async fn initialize(&mut self, config: RuntimeConfig) -> anyhow::Result<()>;

    /// Check if runtime is ready for inference
    async fn is_ready(&self) -> bool;

    /// Get health status
    async fn health_check(&self) -> anyhow::Result<String>;

    /// Get the base URL for inference API (e.g., "http://127.0.0.1:8001")
    fn base_url(&self) -> String;

    /// Get the OpenAI-compatible chat completions endpoint
    fn completions_url(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url())
    }

    /// Perform inference (non-streaming)
    async fn generate(
        &self,
        request: InferenceRequest,
    ) -> anyhow::Result<InferenceResponse>;

    /// Perform streaming inference
    async fn generate_stream(
        &self,
        request: InferenceRequest,
    ) -> anyhow::Result<Box<dyn futures_util::Stream<Item = Result<String, anyhow::Error>> + Send + Unpin>>;

    /// Shutdown the runtime (stop server, cleanup resources)
    async fn shutdown(&mut self) -> anyhow::Result<()>;

    /// Get runtime metadata
    fn metadata(&self) -> RuntimeMetadata;
}

/// Runtime metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeMetadata {
    pub format: ModelFormat,
    pub runtime_name: String,
    pub version: String,
    pub supports_gpu: bool,
    pub supports_streaming: bool,
}
