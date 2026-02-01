//! Runtime Manager
//!
//! Orchestrates model runtime selection, initialization, and lifecycle management.
//! Automatically selects the appropriate runtime based on model format.
//! Lock-free implementation using ArcSwap for atomic pointer swapping.

use super::runtime_trait::*;
use super::format_detector::FormatDetector;
use super::*;
use std::sync::Arc;
use arc_swap::ArcSwap;
use tracing::{info, error};

/// Runtime holder for lock-free access
struct RuntimeHolder {
    runtime: Option<Box<dyn ModelRuntime>>,
    config: Option<RuntimeConfig>,
}

/// Runtime Manager - manages active model runtime
pub struct RuntimeManager {
    /// Currently active runtime (lock-free via ArcSwap)
    holder: Arc<ArcSwap<RuntimeHolder>>,
}

impl RuntimeManager {
    pub fn new() -> Self {
        Self {
            holder: Arc::new(ArcSwap::new(Arc::new(RuntimeHolder {
                runtime: None,
                config: None,
            }))),
        }
    }

    /// Initialize runtime with automatic format detection
    pub async fn initialize_auto(&self, config: RuntimeConfig) -> anyhow::Result<String> {
        info!("Auto-detecting model format from: {}", config.model_path.display());
        
        // Detect format from file extension
        let detected_format = FormatDetector::detect_from_path(&config.model_path)
            .ok_or_else(|| anyhow::anyhow!(
                "Could not detect model format from file: {}. Supported formats: {:?}",
                config.model_path.display(),
                FormatDetector::supported_extensions()
            ))?;

        info!("Detected format: {}", detected_format.name());

        // Override config format with detected format
        let mut final_config = config;
        final_config.format = detected_format;

        self.initialize(final_config).await
    }

    /// Initialize runtime with specified configuration
    pub async fn initialize(&self, config: RuntimeConfig) -> anyhow::Result<String> {
        info!("Initializing runtime for format: {}", config.format.name());

        // Shutdown existing runtime if any
        self.shutdown().await?;

        // Create appropriate runtime based on format
        let mut runtime: Box<dyn ModelRuntime> = match config.format {
            ModelFormat::GGUF => Box::new(GGUFRuntime::new()),
            ModelFormat::GGML => Box::new(GGMLRuntime::new()),
            ModelFormat::ONNX => Box::new(ONNXRuntime::new()),
            ModelFormat::TensorRT => Box::new(TensorRTRuntime::new()),
            ModelFormat::Safetensors => Box::new(SafetensorsRuntime::new()),
            ModelFormat::CoreML => Box::new(CoreMLRuntime::new()),
            ModelFormat::Online => Box::new(OnlineRuntime::new()),
        };

        // Initialize the runtime
        runtime.initialize(config.clone()).await
            .map_err(|e| {
                error!("Failed to initialize {} runtime: {}", config.format.name(), e);
                e
            })?;

        let base_url = runtime.base_url();
        let metadata = runtime.metadata();

        info!("✅ Runtime initialized successfully:");
        info!("  Format: {}", metadata.format.name());
        info!("  Runtime: {}", metadata.runtime_name);
        info!("  Base URL: {}", base_url);
        info!("  GPU Support: {}", metadata.supports_gpu);
        info!("  Streaming: {}", metadata.supports_streaming);

        // Atomically store the new runtime
        let new_holder = Arc::new(RuntimeHolder {
            runtime: Some(runtime),
            config: Some(config),
        });
        self.holder.store(new_holder);

        Ok(base_url)
    }

    /// Get the current runtime's base URL (lock-free)
    pub async fn get_base_url(&self) -> Option<String> {
        let holder = self.holder.load();
        holder.runtime.as_ref().map(|r| r.base_url())
    }

    /// Check if runtime is ready (lock-free read)
    pub async fn is_ready(&self) -> bool {
        let holder = self.holder.load();
        match holder.runtime.as_ref() {
            Some(r) => r.is_ready().await,
            None => false,
        }
    }

    /// Perform health check (lock-free read)
    pub async fn health_check(&self) -> anyhow::Result<String> {
        let holder = self.holder.load();
        match holder.runtime.as_ref() {
            Some(r) => r.health_check().await,
            None => Err(anyhow::anyhow!("No runtime initialized")),
        }
    }

    /// Get runtime metadata (lock-free read)
    pub async fn get_metadata(&self) -> Option<RuntimeMetadata> {
        let holder = self.holder.load();
        holder.runtime.as_ref().map(|r| r.metadata())
    }

    /// Shutdown current runtime (atomic replacement)
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        // Atomically replace with empty holder
        let old_holder = self.holder.swap(Arc::new(RuntimeHolder {
            runtime: None,
            config: None,
        }));

        // Shutdown the old runtime outside the critical section
        // Try to get exclusive ownership; if not possible (arc still referenced), skip shutdown
        if let Ok(mut holder) = Arc::try_unwrap(old_holder) {
            if let Some(mut runtime) = holder.runtime.take() {
                info!("Shutting down runtime");
                runtime.shutdown().await?;
            }
        }

        Ok(())
    }

    /// Hot-swap model (shutdown current, initialize new)
    pub async fn hot_swap(&self, new_config: RuntimeConfig) -> anyhow::Result<String> {
        info!("Performing hot-swap to new model: {}", new_config.model_path.display());
        
        self.shutdown().await?;
        self.initialize(new_config).await
    }

    /// Get current configuration (lock-free)
    pub async fn get_current_config(&self) -> Option<RuntimeConfig> {
        let holder = self.holder.load();
        holder.config.clone()
    }

    /// Perform inference (non-streaming, lock-free read)
    pub async fn generate(&self, request: InferenceRequest) -> anyhow::Result<InferenceResponse> {
        let holder = self.holder.load();
        match holder.runtime.as_ref() {
            Some(r) => r.generate(request).await,
            None => Err(anyhow::anyhow!("No runtime initialized")),
        }
    }

    /// Perform streaming inference (lock-free read)
    pub async fn generate_stream(
        &self,
        request: InferenceRequest,
    ) -> anyhow::Result<Box<dyn futures_util::Stream<Item = Result<String, anyhow::Error>> + Send + Unpin>> {
        let holder = self.holder.load();
        match holder.runtime.as_ref() {
            Some(r) => r.generate_stream(request).await,
            None => Err(anyhow::anyhow!("No runtime initialized")),
        }
    }
}

impl Default for RuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RuntimeManager {
    fn drop(&mut self) {
        // Runtime cleanup happens in shutdown()
        // This is just a safety net
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_runtime_manager_creation() {
        let manager = RuntimeManager::new();
        assert!(!manager.is_ready().await);
    }

    #[tokio::test]
    async fn test_format_detection() {
        let manager = RuntimeManager::new();
        
        let config = RuntimeConfig {
            model_path: PathBuf::from("test.gguf"),
            format: ModelFormat::GGUF, // Will be overridden
            ..Default::default()
        };

        // This will fail because the file doesn't exist, but tests the detection logic
        let result = manager.initialize_auto(config).await;
        assert!(result.is_err()); // Expected to fail - file doesn't exist
    }
}
