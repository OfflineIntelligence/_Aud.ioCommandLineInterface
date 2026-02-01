//! Model Storage Management
//!
//! Handles local storage of models in platform-appropriate locations:
//! - Windows: %APPDATA%/Aud.io/models
//! - Linux: ~/.local/share/aud.io/models
//! - macOS: ~/Library/Application Support/Aud.io/models

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, info};

/// Platform-specific storage location
#[derive(Debug, Clone)]
pub struct StorageLocation {
    /// Base directory for all Aud.io data
    pub app_data_dir: PathBuf,
    /// Directory for storing downloaded models
    pub models_dir: PathBuf,
    /// Directory for model metadata and registry
    pub registry_dir: PathBuf,
}

/// Model storage manager
pub struct ModelStorage {
    pub location: StorageLocation,
}

impl ModelStorage {
    /// Create new storage manager with platform-appropriate paths
    pub fn new() -> Result<Self> {
        let location = Self::get_platform_storage_location()?;
        
        // Ensure directories exist
        Self::ensure_directories(&location)?;
        
        info!("Model storage initialized at: {}", location.models_dir.display());
        
        Ok(Self { location })
    }

    /// Get platform-appropriate storage location
    fn get_platform_storage_location() -> Result<StorageLocation> {
        let app_data_dir = if cfg!(target_os = "windows") {
            // Windows: %APPDATA%\Aud.io
            dirs::data_dir()
                .context("Failed to get APPDATA directory")?
                .join("Aud.io")
        } else if cfg!(target_os = "macos") {
            // macOS: ~/Library/Application Support/Aud.io
            dirs::data_dir()
                .context("Failed to get Library directory")?
                .join("Aud.io")
        } else {
            // Linux: ~/.local/share/aud.io
            dirs::data_dir()
                .context("Failed to get .local/share directory")?
                .join("aud.io")
        };

        let models_dir = app_data_dir.join("models");
        let registry_dir = app_data_dir.join("registry");

        Ok(StorageLocation {
            app_data_dir,
            models_dir,
            registry_dir,
        })
    }

    /// Ensure all required directories exist
    fn ensure_directories(location: &StorageLocation) -> Result<()> {
        std::fs::create_dir_all(&location.app_data_dir)
            .context("Failed to create app data directory")?;
        std::fs::create_dir_all(&location.models_dir)
            .context("Failed to create models directory")?;
        std::fs::create_dir_all(&location.registry_dir)
            .context("Failed to create registry directory")?;
        
        debug!("Created storage directories successfully");
        Ok(())
    }

    /// Get the full path for a model file
    pub fn model_path(&self, model_id: &str, filename: &str) -> PathBuf {
        self.location.models_dir.join(model_id).join(filename)
    }

    /// Get the path for model metadata
    pub fn metadata_path(&self, model_id: &str) -> PathBuf {
        self.location.registry_dir.join(format!("{}.json", model_id))
    }

    /// List all available models in storage
    pub fn list_models(&self) -> Result<Vec<String>> {
        let mut models = Vec::new();
        
        if self.location.models_dir.exists() {
            for entry in std::fs::read_dir(&self.location.models_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    if let Some(model_name) = entry.file_name().to_str() {
                        models.push(model_name.to_string());
                    }
                }
            }
        }
        
        Ok(models)
    }

    /// Check if a model exists locally
    pub fn model_exists(&self, model_id: &str) -> bool {
        self.location.models_dir.join(model_id).exists()
    }

    /// Get total storage usage
    pub fn get_storage_usage(&self) -> Result<u64> {
        if !self.location.models_dir.exists() {
            return Ok(0);
        }

        let mut total_size = 0u64;
        
        for entry in walkdir::WalkDir::new(&self.location.models_dir) {
            let entry = entry?;
            if entry.file_type().is_file() {
                total_size += entry.metadata()?.len();
            }
        }
        
        Ok(total_size)
    }

    /// Remove a model from storage
    pub fn remove_model(&self, model_id: &str) -> Result<()> {
        let model_path = self.location.models_dir.join(model_id);
        let metadata_path = self.metadata_path(model_id);
        
        if model_path.exists() {
            std::fs::remove_dir_all(model_path)?;
        }
        
        if metadata_path.exists() {
            std::fs::remove_file(metadata_path)?;
        }
        
        info!("Removed model: {}", model_id);
        Ok(())
    }

    /// Create model directory structure
    pub fn create_model_directory(&self, model_id: &str) -> Result<PathBuf> {
        let model_dir = self.location.models_dir.join(model_id);
        std::fs::create_dir_all(&model_dir)?;
        Ok(model_dir)
    }

    /// Get available disk space (cross-platform)
    pub fn get_available_space(&self) -> Result<u64> {
        let target_path = if self.location.models_dir.exists() {
            self.location.models_dir.clone()
        } else {
            self.location.app_data_dir.clone()
        };
        let space = fs2::available_space(&target_path)?;
        Ok(space)
    }
}

/// Model metadata structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub size_bytes: u64,
    pub format: String,
    pub download_source: String,
    pub download_date: chrono::DateTime<chrono::Utc>,
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
    pub tags: Vec<String>,
    pub hardware_requirements: HardwareRequirements,
    pub compatibility_notes: Option<String>,
}

/// Hardware requirements for a model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareRequirements {
    pub min_ram_gb: f32,
    pub min_vram_gb: Option<f32>,
    pub recommended_ram_gb: f32,
    pub recommended_vram_gb: Option<f32>,
    pub cpu_cores: Option<u32>,
    pub gpu_required: bool,
}

impl Default for HardwareRequirements {
    fn default() -> Self {
        Self {
            min_ram_gb: 4.0,
            min_vram_gb: None,
            recommended_ram_gb: 8.0,
            recommended_vram_gb: None,
            cpu_cores: None,
            gpu_required: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_storage_creation() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let storage = ModelStorage {
            location: StorageLocation {
                app_data_dir: temp_dir.path().to_path_buf(),
                models_dir: temp_dir.path().join("models"),
                registry_dir: temp_dir.path().join("registry"),
            },
        };
        
        // Test directory creation
        assert!(storage.location.app_data_dir.exists());
        assert!(storage.location.models_dir.exists());
        assert!(storage.location.registry_dir.exists());
        
        Ok(())
    }

    #[test]
    fn test_model_path_generation() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let storage = ModelStorage {
            location: StorageLocation {
                app_data_dir: temp_dir.path().to_path_buf(),
                models_dir: temp_dir.path().join("models"),
                registry_dir: temp_dir.path().join("registry"),
            },
        };
        
        let path = storage.model_path("test-model", "model.gguf");
        assert_eq!(path, temp_dir.path().join("models").join("test-model").join("model.gguf"));
        
        Ok(())
    }
}