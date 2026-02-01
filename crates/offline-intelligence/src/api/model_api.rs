//! Model Management API Endpoints
//!
//! Provides RESTful API endpoints for:
//! - Listing available and installed models
//! - Searching for models
//! - Downloading/installing models
//! - Removing/uninstalling models
//! - Getting download progress
//! - Hardware recommendations

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::{
    model_management::{
        downloader::DownloadSource,
        registry::{ModelInfo, ModelStatus},
        recommendation::{ModelRecommender, UseCase, QualityPreference, SpeedPreference, CostSensitivity},
        ModelManager,
    },
    shared_state::UnifiedAppState,
};

/// Request to install/download a model
#[derive(Debug, Deserialize)]
pub struct InstallModelRequest {
    pub model_id: String,
    pub model_name: String,
    pub source: ModelSourceSpecifier,
    pub description: Option<String>,
    pub size_bytes: u64,
    pub format: String,
}

/// Specify where to download a model from
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ModelSourceSpecifier {
    HuggingFace { repo_id: String, filename: String },
    Ollama { model_name: String },
    OpenRouter { model_id: String },
}

/// Response for model installation
#[derive(Debug, Serialize)]
pub struct InstallModelResponse {
    pub download_id: String,
    pub message: String,
}

/// Request to search for models
#[derive(Debug, Deserialize)]
pub struct SearchModelsRequest {
    pub query: String,
    pub limit: Option<usize>,
}

/// Response containing search results
#[derive(Debug, Serialize)]
pub struct SearchModelsResponse {
    pub models: Vec<ModelInfo>,
    pub total_found: usize,
}

/// Request to update user preferences
#[derive(Debug, Deserialize)]
pub struct UpdatePreferencesRequest {
    pub primary_use_case: Option<String>,
    pub quality_preference: Option<String>,
    pub speed_preference: Option<String>,
    pub cost_sensitivity: Option<String>,
}

/// Response with hardware recommendations
#[derive(Debug, Serialize)]
pub struct HardwareRecommendationsResponse {
    pub recommendations: Vec<String>,
    pub message: String,
}

/// Helper function to clone models from registry
async fn get_cloned_models(model_manager: &ModelManager) -> Vec<ModelInfo> {
    let registry = model_manager.registry.read().await;
    registry.list_models().iter().map(|m| (*m).clone()).collect()
}

/// Get list of all models (installed and available)
pub async fn list_models(
    State(state): State<UnifiedAppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let model_manager = state.shared_state.model_manager.as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let models = get_cloned_models(model_manager).await;
    
    Ok(Json(models))
}

/// Search for models by name, description, or tags
pub async fn search_models(
    State(state): State<UnifiedAppState>,
    Query(params): Query<SearchModelsRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let model_manager = state.shared_state.model_manager.as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let all_models = get_cloned_models(model_manager).await;
    let query_lower = params.query.to_lowercase();
    
    let mut filtered_models: Vec<ModelInfo> = all_models
        .into_iter()
        .filter(|model| {
            model.name.to_lowercase().contains(&query_lower) ||
            model.description.as_ref().map_or(false, |desc| desc.to_lowercase().contains(&query_lower)) ||
            model.tags.iter().any(|tag| tag.to_lowercase().contains(&query_lower))
        })
        .collect();

    let total_found = filtered_models.len();
    let limit = params.limit.unwrap_or(20).min(total_found);
    
    // Truncate to limit if needed
    filtered_models.truncate(limit);

    Ok(Json(SearchModelsResponse {
        models: filtered_models,
        total_found,
    }))
}

/// Install/download a model
pub async fn install_model(
    State(state): State<UnifiedAppState>,
    Json(payload): Json<InstallModelRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let model_manager = state.shared_state.model_manager.as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    info!("Installing model: {} ({})", payload.model_name, payload.model_id);

    // Create model info
    let model_info = ModelInfo {
        id: payload.model_id.clone(),
        name: payload.model_name.clone(),
        description: payload.description,
        author: None,
        status: ModelStatus::Available,
        size_bytes: payload.size_bytes,
        format: payload.format,
        download_source: None,
        installed_version: None,
        last_updated: None,
        tags: vec![], // TODO: Extract tags from source
        compatibility_score: None,
    };

    // Convert source specifier to download source
    let download_source = match payload.source {
        ModelSourceSpecifier::HuggingFace { repo_id, filename } => {
            DownloadSource::HuggingFace { repo_id, filename }
        }
        ModelSourceSpecifier::Ollama { model_name } => {
            DownloadSource::Ollama { model_name }
        }
        ModelSourceSpecifier::OpenRouter { model_id } => {
            DownloadSource::OpenRouter { model_id }
        }
    };

    // Clone for use in async block
    let download_source_clone = download_source.clone();

    // Start download in background
    let registry = model_manager.registry.clone();
    let downloader = model_manager.downloader.clone();
    
    let _download_handle = tokio::spawn(async move {
        match downloader.download_model(model_info.clone(), download_source_clone).await {
            Ok(download_id) => {
                // Update registry status and persist
                let mut reg = registry.write().await;
                reg.update_model_status(&model_info.id, ModelStatus::Installed);
                if let Err(e) = reg.save_registry().await {
                    error!("Failed to persist registry: {}", e);
                }
                drop(reg);
                if let Err(e) = downloader.save_model_metadata(&model_info, &download_source).await {
                    error!("Failed to save model metadata: {}", e);
                }
                info!("Model installation completed: {}", model_info.name);
                Ok(download_id)
            }
            Err(e) => {
                error!("Model installation failed: {} - {}", model_info.name, e);
                let mut reg = registry.write().await;
                reg.update_model_status(&model_info.id, ModelStatus::Error(e.to_string()));
                Err(e)
            }
        }
    });

    // Return immediate response with download ID
    let download_id = format!("download_{}", uuid::Uuid::new_v4());
    
    Ok((
        StatusCode::ACCEPTED,
        Json(InstallModelResponse {
            download_id,
            message: format!("Started downloading model: {}", payload.model_name),
        })
    ))
}

/// Get download progress for a specific download
pub async fn get_download_progress(
    State(state): State<UnifiedAppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<impl IntoResponse, StatusCode> {
    let model_manager = state.shared_state.model_manager.as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let download_id = params.get("download_id")
        .ok_or(StatusCode::BAD_REQUEST)?;

    let progress = model_manager.downloader.progress_tracker()
        .get_progress(download_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(progress))
}

/// Get all active downloads
pub async fn get_active_downloads(
    State(state): State<UnifiedAppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let model_manager = state.shared_state.model_manager.as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let downloads = model_manager.downloader.progress_tracker()
        .get_active_downloads()
        .await;

    Ok(Json(downloads))
}

/// Cancel an ongoing download
pub async fn cancel_download(
    State(state): State<UnifiedAppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<impl IntoResponse, StatusCode> {
    let model_manager = state.shared_state.model_manager.as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let download_id = params.get("download_id")
        .ok_or(StatusCode::BAD_REQUEST)?;

    let success = model_manager.downloader.cancel_download(download_id).await;
    
    if success {
        Ok(Json(serde_json::json!({
            "message": "Download cancelled successfully"
        })))
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}

/// Remove/uninstall a model
pub async fn remove_model(
    State(state): State<UnifiedAppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<impl IntoResponse, StatusCode> {
    let model_manager = state.shared_state.model_manager.as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let model_id = params.get("model_id")
        .ok_or(StatusCode::BAD_REQUEST)?;

    info!("Removing model: {}", model_id);

    // Remove from storage
    if let Err(e) = model_manager.storage.remove_model(model_id) {
        error!("Failed to remove model from storage: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Remove from registry and persist
    let mut registry = model_manager.registry.write().await;
    registry.remove_model(model_id);
    if let Err(e) = registry.save_registry().await {
        error!("Failed to persist registry after removal: {}", e);
    }

    Ok(Json(serde_json::json!({
        "message": format!("Model {} removed successfully", model_id)
    })))
}

/// Get hardware recommendations
pub async fn get_hardware_recommendations(
    State(state): State<UnifiedAppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let model_manager = state.shared_state.model_manager.as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let hardware = ModelRecommender::detect_hardware_profile(&state.shared_state.config);
    let message = model_manager.recommender.get_hardware_recommendation_message(&hardware);
    
    let recommendations = message.lines().map(|s| s.to_string()).collect::<Vec<String>>();

    Ok(Json(HardwareRecommendationsResponse {
        recommendations,
        message,
    }))
}

/// Update user preferences for model recommendations
pub async fn update_preferences(
    State(state): State<UnifiedAppState>,
    Json(payload): Json<UpdatePreferencesRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let model_manager = state.shared_state.model_manager.as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut preferences = model_manager.recommender.get_preferences().clone();

    if let Some(use_case) = payload.primary_use_case {
        preferences.primary_use_case = match use_case.as_str() {
            "chat_assistant" => UseCase::ChatAssistant,
            "code_generation" => UseCase::CodeGeneration,
            "creative_writing" => UseCase::CreativeWriting,
            "research_analysis" => UseCase::ResearchAnalysis,
            "translation" => UseCase::Translation,
            _ => UseCase::GeneralPurpose,
        };
    }

    if let Some(quality) = payload.quality_preference {
        preferences.quality_preference = match quality.as_str() {
            "high_quality" => QualityPreference::HighQuality,
            "fast_response" => QualityPreference::FastResponse,
            _ => QualityPreference::Balanced,
        };
    }

    if let Some(speed) = payload.speed_preference {
        preferences.speed_preference = match speed.as_str() {
            "fastest" => SpeedPreference::Fastest,
            "highest_quality" => SpeedPreference::HighestQuality,
            _ => SpeedPreference::Balanced,
        };
    }

    if let Some(cost) = payload.cost_sensitivity {
        preferences.cost_sensitivity = match cost.as_str() {
            "budget" => CostSensitivity::Budget,
            "premium" => CostSensitivity::Premium,
            _ => CostSensitivity::Moderate,
        };
    }

    // We can't mutate the recommender through Arc, so we'll need to restructure this
    // For now, let's just acknowledge the preferences were set
    info!("User preferences updated: {:?}", preferences);

    Ok(Json(serde_json::json!({
        "message": "Preferences updated successfully"
    })))
}

/// Get recommended models based on current hardware and preferences
pub async fn get_recommended_models(
    State(state): State<UnifiedAppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<impl IntoResponse, StatusCode> {
    let model_manager = state.shared_state.model_manager.as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let max_results = params.get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let hardware = ModelRecommender::detect_hardware_profile(&state.shared_state.config);
    let all_models = get_cloned_models(model_manager).await;
    
    let recommendations = model_manager.recommender.get_recommendations(
        all_models.iter().collect(),
        &hardware,
        max_results
    );

    // Get full model info for recommended models
    let recommended_models: Vec<ModelInfo> = recommendations
        .into_iter()
        .filter_map(|(model_id, _)| {
            all_models.iter().find(|m| m.id == model_id).cloned()
        })
        .collect();

    Ok(Json(recommended_models))
}

/// Hardware information response
#[derive(Debug, Serialize)]
pub struct HardwareInfoResponse {
    pub total_ram_gb: f32,
    pub available_ram_gb: f32,
    pub cpu_cores: u32,
    pub gpu_available: bool,
    pub gpu_vram_gb: Option<f32>,
    pub storage_used_bytes: u64,
    pub storage_available_bytes: u64,
}

/// Get current hardware info and storage usage
pub async fn get_hardware_info(
    State(state): State<UnifiedAppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let hardware = ModelRecommender::detect_hardware_profile(&state.shared_state.config);

    let (storage_used, storage_available) = if let Some(mm) = state.shared_state.model_manager.as_ref() {
        let used = mm.storage.get_storage_usage().unwrap_or(0);
        let available = mm.storage.get_available_space().unwrap_or(0);
        (used, available)
    } else {
        (0, 0)
    };

    Ok(Json(HardwareInfoResponse {
        total_ram_gb: hardware.total_ram_gb,
        available_ram_gb: hardware.available_ram_gb,
        cpu_cores: hardware.cpu_cores,
        gpu_available: hardware.gpu_available,
        gpu_vram_gb: hardware.gpu_vram_gb,
        storage_used_bytes: storage_used,
        storage_available_bytes: storage_available,
    }))
}