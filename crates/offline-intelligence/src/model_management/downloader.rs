//! Model Downloader
//!
//! Downloads models from various sources:
//! - Hugging Face Hub
//! - Ollama Library
//! - OpenRouter (API-based models)

use super::{
    progress::{DownloadStatus, ProgressTracker},
    registry::ModelInfo,
    storage::{ModelStorage, ModelMetadata, HardwareRequirements},
};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::{fs::File, io::AsyncWriteExt};
use tracing::{error, info};

/// Source from which to download a model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DownloadSource {
    HuggingFace { repo_id: String, filename: String },
    Ollama { model_name: String },
    OpenRouter { model_id: String },
}

/// Model downloader service
pub struct ModelDownloader {
    storage: Arc<ModelStorage>,
    progress_tracker: Arc<ProgressTracker>,
    http_client: Client,
}

impl ModelDownloader {
    pub fn new(storage: Arc<ModelStorage>) -> Self {
        let client = Client::builder()
            .user_agent("Aud.io/0.1.1")
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            storage,
            progress_tracker: Arc::new(ProgressTracker::new()),
            http_client: client,
        }
    }

    /// Download a model from the specified source
    pub async fn download_model(
        &self,
        model_info: ModelInfo,
        source: DownloadSource,
    ) -> Result<String> {
        info!("Starting download of model: {} from {:?}", model_info.name, source);

        // Start progress tracking
        let download_id = self.progress_tracker
            .start_download(
                model_info.id.clone(),
                model_info.name.clone(),
                Some(model_info.size_bytes),
            )
            .await;

        // Update status to starting
        self.progress_tracker
            .update_progress(&download_id, 0, DownloadStatus::Starting, None)
            .await;

        // Create model directory
        let model_dir = self.storage.create_model_directory(&model_info.id)
            .context("Failed to create model directory")?;

        let result = match source {
            DownloadSource::HuggingFace { repo_id, filename } => {
                self.download_from_huggingface(&download_id, &repo_id, &filename, &model_dir, &model_info).await
            }
            DownloadSource::Ollama { model_name } => {
                self.download_from_ollama(&download_id, &model_name, &model_dir, &model_info).await
            }
            DownloadSource::OpenRouter { model_id } => {
                self.download_from_openrouter(&download_id, &model_id, &model_dir, &model_info).await
            }
        };

        match result {
            Ok(_) => {
                info!("Successfully downloaded model: {}", model_info.name);
                self.progress_tracker
                    .update_progress(&download_id, model_info.size_bytes, DownloadStatus::Completed, None)
                    .await;
                Ok(download_id)
            }
            Err(e) => {
                error!("Failed to download model {}: {}", model_info.name, e);
                self.progress_tracker
                    .update_progress(&download_id, 0, DownloadStatus::Failed, Some(e.to_string()))
                    .await;
                Err(e)
            }
        }
    }

    /// Download from Hugging Face Hub
    async fn download_from_huggingface(
        &self,
        download_id: &str,
        repo_id: &str,
        filename: &str,
        model_dir: &std::path::Path,
        model_info: &ModelInfo,
    ) -> Result<()> {
        let url = format!("https://huggingface.co/{}/resolve/main/{}", repo_id, filename);
        info!("Downloading from HuggingFace: {}", url);

        self.download_file_with_progress(download_id, &url, model_dir, filename, model_info).await
    }

    /// Download from Ollama (using Ollama's API)
    async fn download_from_ollama(
        &self,
        download_id: &str,
        model_name: &str,
        model_dir: &std::path::Path,
        model_info: &ModelInfo,
    ) -> Result<()> {
        // First, try to get model info from Ollama API
        let ollama_api_url = "http://localhost:11434/api/show";
        let request_body = serde_json::json!({
            "name": model_name
        });

        let response = self.http_client
            .post(ollama_api_url)
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Ollama API request failed: {}", response.status()));
        }

        let model_details: serde_json::Value = response.json().await?;
        
        // Extract download URL if available, otherwise use Ollama pull mechanism
        if let Some(url) = model_details.get("url").and_then(|v| v.as_str()) {
            info!("Downloading from Ollama URL: {}", url);
            self.download_file_with_progress(
                download_id,
                url,
                model_dir,
                &format!("{}.gguf", model_name.replace(":", "-")),
                model_info,
            ).await
        } else {
            // Fallback to Ollama pull command
            info!("Pulling model {} via Ollama CLI", model_name);
            self.pull_via_ollama_cli(download_id, model_name, model_dir, model_info).await
        }
    }

    /// Download from OpenRouter (API-based)
    async fn download_from_openrouter(
        &self,
        _download_id: &str,
        model_id: &str,
        model_dir: &std::path::Path,
        model_info: &ModelInfo,
    ) -> Result<()> {
        // OpenRouter provides API access rather than direct downloads
        // We'll create a configuration file for API usage
        let config_content = serde_json::json!({
            "model_id": model_id,
            "provider": "openrouter",
            "api_key_required": true,
            "downloaded_at": chrono::Utc::now().to_rfc3339(),
            "size_bytes": model_info.size_bytes
        });

        let config_path = model_dir.join("openrouter_config.json");
        tokio::fs::write(&config_path, serde_json::to_string_pretty(&config_content)?).await?;

        info!("Created OpenRouter configuration for model: {}", model_id);
        Ok(())
    }

    /// Download a file with progress tracking using chunked streaming
    async fn download_file_with_progress(
        &self,
        download_id: &str,
        url: &str,
        model_dir: &std::path::Path,
        filename: &str,
        model_info: &ModelInfo,
    ) -> Result<()> {
        use futures_util::StreamExt;

        let response = self.http_client
            .get(url)
            .send()
            .await
            .context("Failed to start download")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Download failed with HTTP status: {}", response.status()));
        }

        let _total_size = response.content_length().unwrap_or(model_info.size_bytes);
        let mut file = File::create(model_dir.join(filename)).await?;
        let mut downloaded: u64 = 0;
        let start_time = std::time::Instant::now();

        self.progress_tracker
            .update_progress(download_id, 0, DownloadStatus::Downloading, None)
            .await;

        // Stream the response in chunks for real-time progress
        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            // Check for cancellation
            if let Some(progress) = self.progress_tracker.get_progress(download_id).await {
                if progress.status == DownloadStatus::Cancelled {
                    // Clean up partial file
                    let _ = tokio::fs::remove_file(model_dir.join(filename)).await;
                    return Err(anyhow::anyhow!("Download cancelled by user"));
                }
            }

            let chunk = chunk_result.context("Error reading download stream")?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;

            // Update elapsed time and progress
            let elapsed = start_time.elapsed();
            self.progress_tracker.update_elapsed_time(download_id, elapsed).await;
            self.progress_tracker
                .update_progress(download_id, downloaded, DownloadStatus::Downloading, None)
                .await;
        }

        file.flush().await?;
        Ok(())
    }

    /// Pull model using Ollama CLI
    async fn pull_via_ollama_cli(
        &self,
        download_id: &str,
        model_name: &str,
        model_dir: &std::path::Path,
        _model_info: &ModelInfo,
    ) -> Result<()> {
        use tokio::process::Command;

        // Update progress to indicate CLI operation
        self.progress_tracker
            .update_progress(download_id, 0, DownloadStatus::Starting, None)
            .await;

        let output = Command::new("ollama")
            .arg("pull")
            .arg(model_name)
            .output()
            .await
            .context("Failed to execute ollama pull command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Ollama pull failed: {}", stderr));
        }

        // Try to locate the downloaded model file
        let ollama_models_path = self.get_ollama_models_path()?;
        let model_file = ollama_models_path.join(format!("{}.gguf", model_name.replace(":", "-")));
        
        if model_file.exists() {
            // Copy to our storage location
            tokio::fs::copy(&model_file, model_dir.join(model_file.file_name().unwrap())).await?;
        }

        Ok(())
    }

    /// Get Ollama models storage path
    fn get_ollama_models_path(&self) -> Result<std::path::PathBuf> {
        let ollama_home = std::env::var("OLLAMA_MODELS")
            .unwrap_or_else(|_| {
                if cfg!(target_os = "windows") {
                    dirs::data_dir()
                        .map(|d| d.join("Ollama").join("models"))
                        .unwrap_or_else(|| std::path::PathBuf::from("C:\\Users\\Public\\Ollama\\models"))
                        .to_string_lossy()
                        .to_string()
                } else {
                    dirs::data_dir()
                        .map(|d| d.join("ollama").join("models"))
                        .unwrap_or_else(|| std::path::PathBuf::from("/usr/share/ollama/.ollama/models"))
                        .to_string_lossy()
                        .to_string()
                }
            });
        
        Ok(std::path::PathBuf::from(ollama_home))
    }

    /// Save model metadata after successful download
    pub async fn save_model_metadata(
        &self,
        model_info: &ModelInfo,
        source: &DownloadSource,
    ) -> Result<()> {
        let metadata = ModelMetadata {
            id: model_info.id.clone(),
            name: model_info.name.clone(),
            description: model_info.description.clone(),
            author: model_info.author.clone(),
            size_bytes: model_info.size_bytes,
            format: model_info.format.clone(),
            download_source: match source {
                DownloadSource::HuggingFace { .. } => "huggingface".to_string(),
                DownloadSource::Ollama { .. } => "ollama".to_string(),
                DownloadSource::OpenRouter { .. } => "openrouter".to_string(),
            },
            download_date: chrono::Utc::now(),
            last_used: None,
            tags: model_info.tags.clone(),
            hardware_requirements: HardwareRequirements::default(),
            compatibility_notes: None,
        };

        let metadata_path = self.storage.metadata_path(&model_info.id);
        let metadata_json = serde_json::to_string_pretty(&metadata)?;
        tokio::fs::write(&metadata_path, metadata_json).await?;

        Ok(())
    }

    /// Get reference to progress tracker
    pub fn progress_tracker(&self) -> &Arc<ProgressTracker> {
        &self.progress_tracker
    }

    /// Cancel an ongoing download
    pub async fn cancel_download(&self, download_id: &str) -> bool {
        self.progress_tracker.cancel_download(download_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_downloader_creation() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let storage = Arc::new(ModelStorage {
            location: super::super::storage::StorageLocation {
                app_data_dir: temp_dir.path().to_path_buf(),
                models_dir: temp_dir.path().join("models"),
                registry_dir: temp_dir.path().join("registry"),
            },
        });

        let downloader = ModelDownloader::new(storage);
        assert!(downloader.progress_tracker().get_all_downloads().await.is_empty());
        
        Ok(())
    }
}