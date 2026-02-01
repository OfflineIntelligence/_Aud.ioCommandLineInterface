//! Model Registry
//!
//! Manages model metadata, tracks installed models, and provides
//! querying capabilities for available models.

use super::storage::{ModelStorage, ModelMetadata};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Status of a model in the registry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ModelStatus {
    /// Model is available locally
    Installed,
    /// Model is being downloaded
    Downloading,
    /// Model is available for download
    Available,
    /// Model had an error during download/installation
    Error(String),
}

/// Information about a model in the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub status: ModelStatus,
    pub size_bytes: u64,
    pub format: String,
    pub download_source: Option<String>,
    pub installed_version: Option<String>,
    pub last_updated: Option<chrono::DateTime<chrono::Utc>>,
    pub tags: Vec<String>,
    pub compatibility_score: Option<f32>, // 0.0 to 1.0 based on hardware match
}

/// Model registry manager
pub struct ModelRegistry {
    storage: Arc<ModelStorage>,
    models: HashMap<String, ModelInfo>,
    known_sources: Vec<ModelSource>,
}

/// Source where models can be found
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSource {
    pub name: String,
    pub url: String,
    pub api_type: SourceType,
}

/// Type of model source API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SourceType {
    HuggingFace,
    Ollama,
    OpenRouter,
}

impl ModelRegistry {
    pub fn new(storage: Arc<ModelStorage>) -> Result<Self> {
        let mut registry = Self {
            storage,
            models: HashMap::new(),
            known_sources: vec![
                ModelSource {
                    name: "Hugging Face".to_string(),
                    url: "https://huggingface.co".to_string(),
                    api_type: SourceType::HuggingFace,
                },
                ModelSource {
                    name: "Ollama".to_string(),
                    url: "https://ollama.ai".to_string(),
                    api_type: SourceType::Ollama,
                },
                ModelSource {
                    name: "OpenRouter".to_string(),
                    url: "https://openrouter.ai".to_string(),
                    api_type: SourceType::OpenRouter,
                },
            ],
        };

        // Load existing registry data
        registry.load_registry()?;

        // Populate default catalog (only adds models not already present)
        registry.populate_default_catalog();

        Ok(registry)
    }

    /// Load registry data from persistent storage
    fn load_registry(&mut self) -> Result<()> {
        let registry_path = self.storage.location.registry_dir.join("registry.json");
        if registry_path.exists() {
            match std::fs::read_to_string(&registry_path) {
                Ok(content) if !content.trim().is_empty() => {
                    match serde_json::from_str::<HashMap<String, ModelInfo>>(&content) {
                        Ok(saved_models) => {
                            self.models = saved_models;
                            info!("Loaded {} models from registry", self.models.len());
                        }
                        Err(e) => {
                            warn!("Registry file corrupted, starting fresh: {}", e);
                        }
                    }
                }
                Ok(_) => {
                    debug!("Registry file is empty, starting fresh");
                }
                Err(e) => {
                    warn!("Failed to read registry file: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Scan local storage for existing models and populate registry
    pub async fn scan_storage(&mut self) -> Result<()> {
        let model_ids = self.storage.list_models()?;
        
        for model_id in model_ids {
            if let Some(metadata) = self.load_model_metadata(&model_id).await? {
                let model_info = ModelInfo {
                    id: model_id.clone(),
                    name: metadata.name,
                    description: metadata.description,
                    author: metadata.author,
                    status: ModelStatus::Installed,
                    size_bytes: metadata.size_bytes,
                    format: metadata.format,
                    download_source: Some(metadata.download_source),
                    installed_version: None, // TODO: Extract from metadata
                    last_updated: Some(metadata.download_date),
                    tags: metadata.tags,
                    compatibility_score: None, // Will be calculated on demand
                };
                
                self.models.insert(model_id, model_info);
            }
        }
        
        info!("Scanned storage and found {} models", self.models.len());
        Ok(())
    }

    /// Load metadata for a specific model
    async fn load_model_metadata(&self, model_id: &str) -> Result<Option<ModelMetadata>> {
        let metadata_path = self.storage.metadata_path(model_id);
        
        if metadata_path.exists() {
            let content = tokio::fs::read_to_string(&metadata_path).await?;
            let metadata: ModelMetadata = serde_json::from_str(&content)?;
            Ok(Some(metadata))
        } else {
            Ok(None)
        }
    }

    /// Add a model to the registry
    pub fn add_model(&mut self, model_info: ModelInfo) {
        self.models.insert(model_info.id.clone(), model_info);
    }

    /// Get model information by ID
    pub fn get_model(&self, model_id: &str) -> Option<&ModelInfo> {
        self.models.get(model_id)
    }

    /// Get mutable reference to model information
    pub fn get_model_mut(&mut self, model_id: &str) -> Option<&mut ModelInfo> {
        self.models.get_mut(model_id)
    }

    /// List all models in registry
    pub fn list_models(&self) -> Vec<&ModelInfo> {
        self.models.values().collect()
    }

    /// List models by status
    pub fn list_models_by_status(&self, status: ModelStatus) -> Vec<&ModelInfo> {
        self.models.values()
            .filter(|model| model.status == status)
            .collect()
    }

    /// Search models by name or tags
    pub fn search_models(&self, query: &str) -> Vec<&ModelInfo> {
        let query_lower = query.to_lowercase();
        self.models.values()
            .filter(|model| {
                model.name.to_lowercase().contains(&query_lower) ||
                model.description.as_ref().map_or(false, |desc| desc.to_lowercase().contains(&query_lower)) ||
                model.tags.iter().any(|tag| tag.to_lowercase().contains(&query_lower))
            })
            .collect()
    }

    /// Get models sorted by compatibility score for current hardware
    pub fn get_recommended_models(&self, max_results: usize) -> Vec<&ModelInfo> {
        let mut models: Vec<_> = self.models.values().collect();
        models.sort_by(|a, b| {
            b.compatibility_score.unwrap_or(0.0)
                .partial_cmp(&a.compatibility_score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        models.truncate(max_results);
        models
    }

    /// Update model status
    pub fn update_model_status(&mut self, model_id: &str, status: ModelStatus) {
        if let Some(model) = self.models.get_mut(model_id) {
            model.status = status;
        }
    }

    /// Remove a model from registry
    pub fn remove_model(&mut self, model_id: &str) -> bool {
        self.models.remove(model_id).is_some()
    }

    /// Get registry statistics
    pub fn get_statistics(&self) -> RegistryStats {
        let mut stats = RegistryStats::default();
        
        for model in self.models.values() {
            match model.status {
                ModelStatus::Installed => stats.installed_count += 1,
                ModelStatus::Downloading => stats.downloading_count += 1,
                ModelStatus::Available => stats.available_count += 1,
                ModelStatus::Error(_) => stats.error_count += 1,
            }
            stats.total_size_bytes += model.size_bytes;
        }
        
        stats
    }

    /// Save registry to persistent storage
    pub async fn save_registry(&self) -> Result<()> {
        let registry_path = self.storage.location.registry_dir.join("registry.json");
        let content = serde_json::to_string_pretty(&self.models)
            .context("Failed to serialize registry")?;
        tokio::fs::write(&registry_path, content).await
            .context("Failed to write registry file")?;
        debug!("Saved {} models to registry", self.models.len());
        Ok(())
    }

    /// Populate the registry with well-known models from all sources.
    /// Only adds models that are not already in the registry.
    pub fn populate_default_catalog(&mut self) {
        let catalog = Self::get_default_catalog();
        let mut added = 0;
        for model in catalog {
            if !self.models.contains_key(&model.id) {
                self.models.insert(model.id.clone(), model);
                added += 1;
            }
        }
        if added > 0 {
            info!("Populated catalog with {} new available models", added);
        }
    }

    /// Returns the built-in catalog of well-known models
    fn get_default_catalog() -> Vec<ModelInfo> {
        vec![
            // ── HuggingFace GGUF Models ──
            ModelInfo {
                id: "TheBloke/Llama-2-7B-Chat-GGUF".into(),
                name: "Llama 2 7B Chat".into(),
                description: Some("Meta's Llama 2 7B parameter chat model in GGUF format. Good general-purpose assistant.".into()),
                author: Some("Meta / TheBloke".into()),
                status: ModelStatus::Available,
                size_bytes: 3_830_000_000,
                format: "gguf".into(),
                download_source: Some("huggingface".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["chat".into(), "instruction".into(), "general".into(), "7b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "TheBloke/Mistral-7B-Instruct-v0.2-GGUF".into(),
                name: "Mistral 7B Instruct v0.2".into(),
                description: Some("Mistral AI's 7B instruction-tuned model. Excellent performance for its size.".into()),
                author: Some("Mistral AI / TheBloke".into()),
                status: ModelStatus::Available,
                size_bytes: 4_100_000_000,
                format: "gguf".into(),
                download_source: Some("huggingface".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["chat".into(), "instruction".into(), "general".into(), "7b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "microsoft/Phi-3-mini-4k-instruct-gguf".into(),
                name: "Phi-3 Mini 4K Instruct".into(),
                description: Some("Microsoft's Phi-3 Mini with 3.8B parameters. Compact yet capable model.".into()),
                author: Some("Microsoft".into()),
                status: ModelStatus::Available,
                size_bytes: 2_200_000_000,
                format: "gguf".into(),
                download_source: Some("huggingface".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["chat".into(), "instruction".into(), "code".into(), "small".into(), "3.8b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "TheBloke/Llama-2-13B-Chat-GGUF".into(),
                name: "Llama 2 13B Chat".into(),
                description: Some("Meta's Llama 2 13B parameter chat model. Higher quality than 7B variant.".into()),
                author: Some("Meta / TheBloke".into()),
                status: ModelStatus::Available,
                size_bytes: 7_370_000_000,
                format: "gguf".into(),
                download_source: Some("huggingface".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["chat".into(), "instruction".into(), "general".into(), "13b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "Qwen/Qwen2-7B-Instruct-GGUF".into(),
                name: "Qwen2 7B Instruct".into(),
                description: Some("Alibaba's Qwen2 7B instruction model. Strong multilingual and coding capabilities.".into()),
                author: Some("Alibaba / Qwen".into()),
                status: ModelStatus::Available,
                size_bytes: 4_200_000_000,
                format: "gguf".into(),
                download_source: Some("huggingface".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["chat".into(), "multilingual".into(), "code".into(), "7b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "TheBloke/CodeLlama-7B-Instruct-GGUF".into(),
                name: "Code Llama 7B Instruct".into(),
                description: Some("Meta's Code Llama 7B, specialized for code generation and understanding.".into()),
                author: Some("Meta / TheBloke".into()),
                status: ModelStatus::Available,
                size_bytes: 3_830_000_000,
                format: "gguf".into(),
                download_source: Some("huggingface".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["code".into(), "programming".into(), "instruction".into(), "7b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "NousResearch/Hermes-2-Pro-Mistral-7B-GGUF".into(),
                name: "Hermes 2 Pro Mistral 7B".into(),
                description: Some("Nous Research's Hermes 2 Pro. Excellent function calling and structured output.".into()),
                author: Some("Nous Research".into()),
                status: ModelStatus::Available,
                size_bytes: 4_100_000_000,
                format: "gguf".into(),
                download_source: Some("huggingface".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["chat".into(), "instruction".into(), "function-calling".into(), "7b".into()],
                compatibility_score: None,
            },

            // ── Ollama Models ──
            ModelInfo {
                id: "ollama:llama3".into(),
                name: "Llama 3 (Ollama)".into(),
                description: Some("Meta's latest Llama 3 model via Ollama. State-of-the-art open model.".into()),
                author: Some("Meta".into()),
                status: ModelStatus::Available,
                size_bytes: 4_700_000_000,
                format: "gguf".into(),
                download_source: Some("ollama".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["chat".into(), "general".into(), "instruction".into(), "8b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "ollama:mistral".into(),
                name: "Mistral (Ollama)".into(),
                description: Some("Mistral 7B via Ollama. Fast and capable general-purpose model.".into()),
                author: Some("Mistral AI".into()),
                status: ModelStatus::Available,
                size_bytes: 4_100_000_000,
                format: "gguf".into(),
                download_source: Some("ollama".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["chat".into(), "general".into(), "7b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "ollama:phi3".into(),
                name: "Phi-3 (Ollama)".into(),
                description: Some("Microsoft Phi-3 via Ollama. Small but capable model for constrained hardware.".into()),
                author: Some("Microsoft".into()),
                status: ModelStatus::Available,
                size_bytes: 2_200_000_000,
                format: "gguf".into(),
                download_source: Some("ollama".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["chat".into(), "small".into(), "code".into(), "3.8b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "ollama:gemma2".into(),
                name: "Gemma 2 (Ollama)".into(),
                description: Some("Google's Gemma 2 model via Ollama. Efficient and performant.".into()),
                author: Some("Google".into()),
                status: ModelStatus::Available,
                size_bytes: 5_400_000_000,
                format: "gguf".into(),
                download_source: Some("ollama".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["chat".into(), "general".into(), "9b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "ollama:codellama".into(),
                name: "Code Llama (Ollama)".into(),
                description: Some("Meta's Code Llama via Ollama. Specialized for code tasks.".into()),
                author: Some("Meta".into()),
                status: ModelStatus::Available,
                size_bytes: 3_800_000_000,
                format: "gguf".into(),
                download_source: Some("ollama".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["code".into(), "programming".into(), "7b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "ollama:llama3:70b".into(),
                name: "Llama 3 70B (Ollama)".into(),
                description: Some("Meta's Llama 3 70B via Ollama. Highest quality, requires significant resources.".into()),
                author: Some("Meta".into()),
                status: ModelStatus::Available,
                size_bytes: 39_000_000_000,
                format: "gguf".into(),
                download_source: Some("ollama".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["chat".into(), "general".into(), "instruction".into(), "70b".into(), "gpu".into()],
                compatibility_score: None,
            },

            // ── OpenRouter API Models ──
            ModelInfo {
                id: "openrouter:meta-llama/llama-3-8b-instruct".into(),
                name: "Llama 3 8B (OpenRouter)".into(),
                description: Some("Meta Llama 3 8B via OpenRouter API. No local resources needed, requires API key.".into()),
                author: Some("Meta / OpenRouter".into()),
                status: ModelStatus::Available,
                size_bytes: 0,
                format: "api".into(),
                download_source: Some("openrouter".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["api".into(), "chat".into(), "cloud".into(), "8b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "openrouter:mistralai/mistral-7b-instruct".into(),
                name: "Mistral 7B (OpenRouter)".into(),
                description: Some("Mistral 7B via OpenRouter API. Cloud-based, no local resources needed.".into()),
                author: Some("Mistral AI / OpenRouter".into()),
                status: ModelStatus::Available,
                size_bytes: 0,
                format: "api".into(),
                download_source: Some("openrouter".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["api".into(), "chat".into(), "cloud".into(), "7b".into()],
                compatibility_score: None,
            },
            ModelInfo {
                id: "openrouter:google/gemma-2-9b-it".into(),
                name: "Gemma 2 9B (OpenRouter)".into(),
                description: Some("Google Gemma 2 via OpenRouter API. Cloud-based inference.".into()),
                author: Some("Google / OpenRouter".into()),
                status: ModelStatus::Available,
                size_bytes: 0,
                format: "api".into(),
                download_source: Some("openrouter".into()),
                installed_version: None,
                last_updated: None,
                tags: vec!["api".into(), "chat".into(), "cloud".into(), "9b".into()],
                compatibility_score: None,
            },
        ]
    }
}

/// Registry statistics
#[derive(Debug, Default)]
pub struct RegistryStats {
    pub installed_count: usize,
    pub downloading_count: usize,
    pub available_count: usize,
    pub error_count: usize,
    pub total_size_bytes: u64,
}

impl RegistryStats {
    pub fn total_models(&self) -> usize {
        self.installed_count + self.downloading_count + self.available_count + self.error_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_registry_creation() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let storage = Arc::new(ModelStorage {
            location: super::super::storage::StorageLocation {
                app_data_dir: temp_dir.path().to_path_buf(),
                models_dir: temp_dir.path().join("models"),
                registry_dir: temp_dir.path().join("registry"),
            },
        });
        
        let registry = ModelRegistry::new(storage)?;
        assert_eq!(registry.models.len(), 0);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_model_addition_and_lookup() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let storage = Arc::new(ModelStorage {
            location: super::super::storage::StorageLocation {
                app_data_dir: temp_dir.path().to_path_buf(),
                models_dir: temp_dir.path().join("models"),
                registry_dir: temp_dir.path().join("registry"),
            },
        });
        
        let mut registry = ModelRegistry::new(storage)?;
        
        let model_info = ModelInfo {
            id: "test-model".to_string(),
            name: "Test Model".to_string(),
            description: Some("A test model".to_string()),
            author: Some("Test Author".to_string()),
            status: ModelStatus::Available,
            size_bytes: 1024,
            format: "gguf".to_string(),
            download_source: Some("huggingface".to_string()),
            installed_version: None,
            last_updated: None,
            tags: vec!["test".to_string()],
            compatibility_score: Some(0.8),
        };
        
        registry.add_model(model_info);
        assert_eq!(registry.models.len(), 1);
        
        let retrieved = registry.get_model("test-model");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Test Model");
        
        Ok(())
    }
}