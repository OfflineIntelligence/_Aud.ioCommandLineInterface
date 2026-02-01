//! Model Management System
//!
//! Provides comprehensive model lifecycle management including:
//! - Model registry and metadata storage
//! - Download from multiple sources (HuggingFace, Ollama, OpenRouter)
//! - Local storage management in AppData
//! - Hardware-aware model recommendations
//! - Progress tracking for downloads

pub mod registry;
pub mod downloader;
pub mod storage;
pub mod recommendation;
pub mod progress;
pub mod llama_binary;

pub use registry::{ModelRegistry, ModelInfo, ModelStatus};
pub use downloader::{ModelDownloader, DownloadSource};
pub use storage::{ModelStorage, StorageLocation};
pub use recommendation::ModelRecommender;
pub use progress::{DownloadProgress, ProgressTracker};
pub use llama_binary::{find_llama_binary, find_model_file, detect_platform, download_llama_binary, get_hardware_summary, get_binary_dir};

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Main model management service
pub struct ModelManager {
    pub registry: Arc<RwLock<ModelRegistry>>,
    pub downloader: Arc<ModelDownloader>,
    pub storage: Arc<ModelStorage>,
    pub recommender: Arc<ModelRecommender>,
}

impl ModelManager {
    pub fn new() -> Result<Self> {
        let storage = Arc::new(ModelStorage::new()?);
        let registry = Arc::new(RwLock::new(ModelRegistry::new(storage.clone())?));
        let downloader = Arc::new(ModelDownloader::new(storage.clone()));
        let recommender = Arc::new(ModelRecommender::new());

        Ok(Self {
            registry,
            downloader,
            storage,
            recommender,
        })
    }

    /// Initialize the model manager and scan for existing models
    pub async fn initialize(&self) -> Result<()> {
        // Scan storage for existing models and populate registry
        self.registry.write().await.scan_storage().await?;
        Ok(())
    }
}