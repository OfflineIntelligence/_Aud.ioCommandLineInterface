//! Model Recommendation System
//!
//! Provides hardware-aware model recommendations based on:
//! - Available RAM and VRAM
//! - CPU/GPU capabilities
//! - Model size requirements
//! - User preferences and use cases

use super::registry::ModelInfo;
use crate::config::Config;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// User preferences for model recommendations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    pub primary_use_case: UseCase,
    pub quality_preference: QualityPreference,
    pub speed_preference: SpeedPreference,
    pub cost_sensitivity: CostSensitivity,
    pub preferred_formats: Vec<String>,
}

/// Primary use case for the model
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum UseCase {
    ChatAssistant,
    CodeGeneration,
    CreativeWriting,
    ResearchAnalysis,
    Translation,
    GeneralPurpose,
}

/// Quality preference setting
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum QualityPreference {
    HighQuality,      // Prefer larger, more capable models
    Balanced,         // Balance quality and performance
    FastResponse,     // Prioritize speed over quality
}

/// Speed preference setting
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SpeedPreference {
    Fastest,          // Prioritize inference speed
    Balanced,         // Balance speed and quality
    HighestQuality,   // Prioritize quality over speed
}

/// Cost sensitivity (relevant for cloud/API models)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CostSensitivity {
    Budget,           // Prefer smaller, free models
    Moderate,         // Balanced approach
    Premium,          // Willing to use larger/expensive models
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            primary_use_case: UseCase::GeneralPurpose,
            quality_preference: QualityPreference::Balanced,
            speed_preference: SpeedPreference::Balanced,
            cost_sensitivity: CostSensitivity::Moderate,
            preferred_formats: vec!["gguf".to_string()], // Default to GGUF for local inference
        }
    }
}

/// Hardware profile detected from system
#[derive(Debug, Clone)]
pub struct HardwareProfile {
    pub total_ram_gb: f32,
    pub available_ram_gb: f32,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub gpu_available: bool,
    pub gpu_vram_gb: Option<f32>,
    pub gpu_compute_capability: Option<f32>,
    pub system_architecture: String,
}

/// Model recommender service
pub struct ModelRecommender {
    user_preferences: UserPreferences,
}

impl ModelRecommender {
    pub fn new() -> Self {
        Self {
            user_preferences: UserPreferences::default(),
        }
    }

    /// Update user preferences
    pub fn set_preferences(&mut self, preferences: UserPreferences) {
        self.user_preferences = preferences;
        info!("Updated user preferences: {:?}", self.user_preferences);
    }

    /// Get current user preferences
    pub fn get_preferences(&self) -> &UserPreferences {
        &self.user_preferences
    }

    /// Detect hardware profile from system configuration
    pub fn detect_hardware_profile(config: &Config) -> HardwareProfile {
        let mut system = sysinfo::System::new_all();
        system.refresh_memory();
        system.refresh_cpu();

        let total_ram_gb = system.total_memory() as f32 / (1024.0 * 1024.0 * 1024.0);
        let available_ram_gb = system.available_memory() as f32 / (1024.0 * 1024.0 * 1024.0);
        let cpu_cores = num_cpus::get() as u32;
        let cpu_threads = config.threads;

        // GPU detection
        let (gpu_available, gpu_vram_gb, gpu_compute_capability) = Self::detect_gpu();

        HardwareProfile {
            total_ram_gb,
            available_ram_gb,
            cpu_cores,
            cpu_threads,
            gpu_available,
            gpu_vram_gb,
            gpu_compute_capability,
            system_architecture: std::env::consts::ARCH.to_string(),
        }
    }

    /// Detect GPU capabilities
    fn detect_gpu() -> (bool, Option<f32>, Option<f32>) {
        match nvml_wrapper::Nvml::init() {
            Ok(nvml) => {
                match nvml.device_count() {
                    Ok(count) if count > 0 => {
                        match nvml.device_by_index(0) {
                            Ok(device) => {
                                let vram_bytes = device.memory_info().map(|mem| mem.total).unwrap_or(0);
                                let vram_gb = if vram_bytes > 0 {
                                    Some(vram_bytes as f32 / (1024.0 * 1024.0 * 1024.0))
                                } else {
                                    None
                                };
                                
                                // Simplified compute capability detection
                                let compute_capability = Some(7.5); // Default assumption
                                
                                (true, vram_gb, compute_capability)
                            }
                            Err(_) => (false, None, None),
                        }
                    }
                    _ => (false, None, None),
                }
            }
            Err(_) => (false, None, None),
        }
    }

    /// Score a model's compatibility with current hardware
    pub fn score_model_compatibility(
        &self,
        model: &ModelInfo,
        hardware: &HardwareProfile,
    ) -> f32 {
        let mut score = 1.0f32;

        // RAM requirements check
        let model_ram_gb = (model.size_bytes as f32) / (1024.0 * 1024.0 * 1024.0) * 1.5; // Estimate with buffer
        
        if model_ram_gb > hardware.available_ram_gb {
            score *= 0.3; // Heavy penalty for insufficient RAM
        } else if model_ram_gb > hardware.total_ram_gb * 0.8 {
            score *= 0.7; // Moderate penalty for tight RAM
        }

        // GPU requirements check
        let requires_gpu = model.tags.contains(&"gpu".to_string()) || 
                          model.format.eq_ignore_ascii_case("tensorrt");
        
        if requires_gpu && !hardware.gpu_available {
            score *= 0.2; // Heavy penalty for GPU requirement without GPU
        } else if requires_gpu && hardware.gpu_available {
            if let Some(vram_gb) = hardware.gpu_vram_gb {
                let required_vram = match model.size_bytes {
                    s if s < 5 * 1024 * 1024 * 1024 => 6.0, // 5GB model needs ~6GB VRAM
                    s if s < 10 * 1024 * 1024 * 1024 => 12.0,
                    _ => 24.0,
                };
                
                if vram_gb < required_vram * 0.8 {
                    score *= 0.5; // Penalty for tight VRAM
                }
            }
        }

        // Format preference bonus
        if self.user_preferences.preferred_formats.iter().any(|f| 
            model.format.eq_ignore_ascii_case(f)
        ) {
            score *= 1.2; // Bonus for preferred format
        }

        // Use case alignment
        score *= self.score_use_case_alignment(model);

        // Quality/speed preference adjustment
        score *= self.score_quality_speed_preference(model);

        score.clamp(0.0, 1.0)
    }

    /// Score how well a model aligns with the user's use case
    fn score_use_case_alignment(&self, model: &ModelInfo) -> f32 {
        let use_case_tags: Vec<&str> = model.tags.iter()
            .map(|s| s.as_str())
            .collect();

        match self.user_preferences.primary_use_case {
            UseCase::ChatAssistant => {
                if use_case_tags.contains(&"chat") || use_case_tags.contains(&"instruction") {
                    1.3
                } else if use_case_tags.contains(&"general") {
                    1.1
                } else {
                    1.0
                }
            }
            UseCase::CodeGeneration => {
                if use_case_tags.contains(&"code") || use_case_tags.contains(&"programming") {
                    1.4
                } else {
                    1.0
                }
            }
            UseCase::CreativeWriting => {
                if use_case_tags.contains(&"creative") || use_case_tags.contains(&"story") {
                    1.3
                } else if use_case_tags.contains(&"text-generation") {
                    1.1
                } else {
                    1.0
                }
            }
            UseCase::ResearchAnalysis => {
                if use_case_tags.contains(&"research") || use_case_tags.contains(&"analysis") {
                    1.3
                } else {
                    1.0
                }
            }
            UseCase::Translation => {
                if use_case_tags.contains(&"translation") || use_case_tags.contains(&"multilingual") {
                    1.4
                } else {
                    1.0
                }
            }
            UseCase::GeneralPurpose => 1.0,
        }
    }

    /// Adjust score based on quality/speed preferences
    fn score_quality_speed_preference(&self, model: &ModelInfo) -> f32 {
        let model_size_category = if model.size_bytes < 3 * 1024 * 1024 * 1024 {
            "small" // < 3GB
        } else if model.size_bytes < 10 * 1024 * 1024 * 1024 {
            "medium" // 3-10GB
        } else {
            "large" // > 10GB
        };

        match (&self.user_preferences.quality_preference, &self.user_preferences.speed_preference) {
            (QualityPreference::HighQuality, SpeedPreference::HighestQuality) => {
                match model_size_category {
                    "large" => 1.3,
                    "medium" => 1.1,
                    "small" => 0.8,
                    _ => 1.0,
                }
            }
            (QualityPreference::FastResponse, SpeedPreference::Fastest) => {
                match model_size_category {
                    "small" => 1.3,
                    "medium" => 1.0,
                    "large" => 0.6,
                    _ => 1.0,
                }
            }
            _ => 1.0, // Balanced preferences
        }
    }

    /// Get top recommended models for current hardware and preferences
    pub fn get_recommendations(
        &self,
        models: Vec<&ModelInfo>,
        hardware: &HardwareProfile,
        max_results: usize,
    ) -> Vec<(String, f32)> {
        let mut scored_models: Vec<(String, f32)> = models
            .iter()
            .map(|model| {
                let score = self.score_model_compatibility(model, hardware);
                (model.id.clone(), score)
            })
            .collect();

        // Sort by compatibility score (descending)
        scored_models.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        // Take top results
        scored_models.truncate(max_results);
        
        debug!("Generated {} model recommendations", scored_models.len());
        scored_models
    }

    /// Get hardware recommendations message
    pub fn get_hardware_recommendation_message(&self, hardware: &HardwareProfile) -> String {
        let mut recommendations = Vec::new();

        if hardware.total_ram_gb < 8.0 {
            recommendations.push("Consider upgrading RAM to 8GB+ for better model performance".to_string());
        }

        if !hardware.gpu_available && hardware.total_ram_gb >= 16.0 {
            recommendations.push("A GPU would significantly accelerate inference for larger models".to_string());
        }

        if hardware.gpu_available {
            if let Some(vram) = hardware.gpu_vram_gb {
                if vram < 8.0 {
                    recommendations.push(format!("With {:.1}GB VRAM, you can run medium-sized models efficiently", vram));
                } else if vram >= 16.0 {
                    recommendations.push(format!("With {:.1}GB VRAM, you can run large models with full GPU acceleration", vram));
                }
            }
        }

        if recommendations.is_empty() {
            "Your system configuration is well-suited for running various model sizes".to_string()
        } else {
            recommendations.join("\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_hardware_detection() {
        let config = Config {
            model_path: "".to_string(),
            llama_bin: "".to_string(),
            llama_host: "127.0.0.1".to_string(),
            llama_port: 8001,
            ctx_size: 8192,
            batch_size: 128,
            threads: 6,
            gpu_layers: 20,
            health_timeout_seconds: 600,
            hot_swap_grace_seconds: 25,
            max_concurrent_streams: 2,
            prometheus_port: 9000,
            api_host: "127.0.0.1".to_string(),
            api_port: 8000,
            requests_per_second: 24,
            generate_timeout_seconds: 300,
            stream_timeout_seconds: 600,
            health_check_timeout_seconds: 900,
            queue_size: 1000,
            queue_timeout_seconds: 300,
            backend_url: "http://127.0.0.1:8001".to_string(),
        };

        let hardware = ModelRecommender::detect_hardware_profile(&config);
        assert!(hardware.total_ram_gb > 0.0);
        assert!(hardware.cpu_cores > 0);
    }
}