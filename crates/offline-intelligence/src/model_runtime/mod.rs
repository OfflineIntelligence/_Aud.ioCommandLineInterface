//! Model Runtime Abstraction Layer
//!
//! Provides a unified interface for hosting multiple model formats (GGUF, ONNX, 
//! TensorRT, Safetensors, GGML, CoreML) through a trait-based runtime system.
//!
//! Architecture:
//! - Each model format has its own runtime adapter
//! - All runtimes expose OpenAI-compatible HTTP API
//! - Maintains 1-hop architecture: Rust → HTTP → Runtime Server
//! - Automatic format detection from file extension

pub mod runtime_trait;
pub mod gguf_runtime;
pub mod onnx_runtime;
pub mod tensorrt_runtime;
pub mod safetensors_runtime;
pub mod ggml_runtime;
pub mod coreml_runtime;
pub mod online_runtime;
pub mod format_detector;
pub mod runtime_manager;

pub use runtime_trait::{ModelRuntime, ModelFormat, RuntimeConfig, InferenceRequest, InferenceResponse};
pub use gguf_runtime::GGUFRuntime;
pub use onnx_runtime::ONNXRuntime;
pub use tensorrt_runtime::TensorRTRuntime;
pub use safetensors_runtime::SafetensorsRuntime;
pub use ggml_runtime::GGMLRuntime;
pub use coreml_runtime::CoreMLRuntime;
pub use online_runtime::OnlineRuntime;
pub use format_detector::FormatDetector;
pub use runtime_manager::RuntimeManager;
