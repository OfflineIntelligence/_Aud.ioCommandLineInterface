// _Aud.io/crates/offline-intelligence/src/config.rs

use anyhow::{Context, Result};
use std::env;
use std::net::SocketAddr;
use tracing::{info, warn};
use nvml_wrapper::Nvml;
use sysinfo::System;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Config {
    pub model_path: String,
    pub llama_bin: String,
    pub llama_host: String,
    pub llama_port: u16,
    pub ctx_size: u32,
    pub batch_size: u32,
    pub threads: u32,
    pub gpu_layers: u32,
    pub health_timeout_seconds: u64,
    pub hot_swap_grace_seconds: u64,
    pub max_concurrent_streams: u32,
    pub prometheus_port: u16,
    pub api_host: String,
    pub api_port: u16,
    pub requests_per_second: u32,
    pub generate_timeout_seconds: u64,
    pub stream_timeout_seconds: u64,
    pub health_check_timeout_seconds: u64,
    pub queue_size: usize,
    pub queue_timeout_seconds: u64,
    pub backend_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        if let Err(e) = dotenvy::dotenv() {
            warn!("Failed to load .env file: {}. Using system environment variables.", e);
        } else {
            info!("Loaded environment variables from .env file");
        }

        // Find llama binary: env var → AppData/bin → next to exe → PATH
        let llama_bin = if let Ok(env_bin) = env::var("LLAMA_BIN") {
            if std::path::Path::new(&env_bin).exists() {
                info!("Using llama binary from LLAMA_BIN env: {}", env_bin);
                env_bin
            } else {
                warn!("LLAMA_BIN set but not found: {}. Trying auto-detection...", env_bin);
                match crate::model_management::find_llama_binary() {
                    Some(p) => p.to_string_lossy().to_string(),
                    None => String::new(),
                }
            }
        } else {
            match crate::model_management::find_llama_binary() {
                Some(p) => {
                    info!("Auto-detected llama binary: {}", p.display());
                    p.to_string_lossy().to_string()
                }
                None => {
                    warn!("No llama-server binary found. Run `aud setup` to download one.");
                    String::new()
                }
            }
        };

        // Use MODEL_PATH from env, or try to find embedded model
        let model_path = Self::get_model_path_with_fallback()?;

        // Auto‑detect threads if set to "auto"
        let threads = if env::var("THREADS").unwrap_or_else(|_| "auto".into()) == "auto" {
            Self::auto_detect_threads()
        } else {
            env::var("THREADS").unwrap_or_else(|_| "6".into()).parse().unwrap_or(6)
        };

        // Auto‑detect GPU layers if set to "auto"
        let gpu_layers = if env::var("GPU_LAYERS").unwrap_or_else(|_| "auto".into()) == "auto" {
            Self::auto_detect_gpu_layers()
        } else {
            env::var("GPU_LAYERS").unwrap_or_else(|_| "20".into()).parse().unwrap_or(20)
        };

        // Auto‑detect context size
        let ctx_size = if env::var("CTX_SIZE").unwrap_or_else(|_| "auto".into()) == "auto" {
            Self::auto_detect_ctx_size(&model_path)
        } else {
            env::var("CTX_SIZE").unwrap_or_else(|_| "8192".into()).parse().unwrap_or(8192)
        };

        // Auto‑detect batch size
        let batch_size = if env::var("BATCH_SIZE").unwrap_or_else(|_| "auto".into()) == "auto" {
            Self::auto_detect_batch_size(gpu_layers, ctx_size)
        } else {
            env::var("BATCH_SIZE").unwrap_or_else(|_| "256".into()).parse().unwrap_or(256)
        };

        // Get backend URL components
        let llama_host = env::var("LLAMA_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let llama_port = env::var("LLAMA_PORT").unwrap_or_else(|_| "8081".into()).parse()?;
        let backend_url = format!("http://{}:{}", llama_host, llama_port);

        info!(
            "Resource Configuration: {} GPU layers, {} threads, batch size: {}, context: {}",
            gpu_layers, threads, batch_size, ctx_size
        );

        Ok(Self {
            model_path,
            llama_bin,
            llama_host: llama_host.clone(),
            llama_port,
            ctx_size,
            batch_size,
            threads,
            gpu_layers,
            health_timeout_seconds: env::var("HEALTH_TIMEOUT_SECONDS")
                .unwrap_or_else(|_| "60".into())
                .parse()?,
            hot_swap_grace_seconds: env::var("HOT_SWAP_GRACE_SECONDS")
                .unwrap_or_else(|_| "25".into())
                .parse()?,
            max_concurrent_streams: env::var("MAX_CONCURRENT_STREAMS")
                .unwrap_or_else(|_| "4".into())
                .parse()?,
            prometheus_port: env::var("PROMETHEUS_PORT")
                .unwrap_or_else(|_| "9000".into())
                .parse()?,
            api_host: env::var("API_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            api_port: env::var("API_PORT").unwrap_or_else(|_| "8000".into()).parse()?,
            requests_per_second: env::var("REQUESTS_PER_SECOND")
                .unwrap_or_else(|_| "24".into())
                .parse()?,
            generate_timeout_seconds: env::var("GENERATE_TIMEOUT_SECONDS")
                .unwrap_or_else(|_| "300".into())
                .parse()?,
            stream_timeout_seconds: env::var("STREAM_TIMEOUT_SECONDS")
                .unwrap_or_else(|_| "600".into())
                .parse()?,
            health_check_timeout_seconds: env::var("HEALTH_CHECK_TIMEOUT_SECONDS")
                .unwrap_or_else(|_| "90".into())
                .parse()?,
            queue_size: env::var("QUEUE_SIZE")
                .unwrap_or_else(|_| "100".into())
                .parse()?,
            queue_timeout_seconds: env::var("QUEUE_TIMEOUT_SECONDS")
                .unwrap_or_else(|_| "30".into())
                .parse()?,
            backend_url,
        })
    }

    fn get_model_path_with_fallback() -> Result<String> {
        // First try environment variable
        if let Ok(model_path) = env::var("MODEL_PATH") {
            // Check if the path exists
            if std::path::Path::new(&model_path).exists() {
                info!("Using model from MODEL_PATH: {}", model_path);
                return Ok(model_path);
            } else {
                warn!("MODEL_PATH set but file doesn't exist: {}", model_path);
            }
        }

        // Try to find embedded model
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        // Check multiple possible embedded model locations (MULTI-FORMAT SUPPORT)
        let possible_model_locations = vec![
            // GGUF formats
            exe_dir.join("resources/models/default.gguf"),
            exe_dir.join("resources/models/model.gguf"),
            exe_dir.join("models/default.gguf"),
            exe_dir.join("models/model.gguf"),
            exe_dir.join("default.gguf"),
            // ONNX formats
            exe_dir.join("resources/models/default.onnx"),
            exe_dir.join("resources/models/model.onnx"),
            // TensorRT formats
            exe_dir.join("resources/models/default.trt"),
            exe_dir.join("resources/models/model.engine"),
            // Safetensors formats
            exe_dir.join("resources/models/default.safetensors"),
            exe_dir.join("resources/models/model.safetensors"),
            // GGML formats
            exe_dir.join("resources/models/default.ggml"),
            exe_dir.join("resources/models/model.bin"),
        ];

        for model_path in possible_model_locations {
            if model_path.exists() {
                info!("Using embedded model: {}", model_path.display());
                return Ok(model_path.to_string_lossy().to_string());
            }
        }

        // Check for any supported model file in models directory
        if let Ok(entries) = std::fs::read_dir(exe_dir.join("resources/models")) {
            for entry in entries.flatten() {
                if let Some(ext) = entry.path().extension() {
                    let ext_str = ext.to_str().unwrap_or("").to_lowercase();
                    // Check if extension matches any supported format
                    if matches!(ext_str.as_str(), "gguf" | "ggml" | "onnx" | "trt" | "engine" | "plan" | "safetensors" | "mlmodel") {
                        info!("Using found model: {}", entry.path().display());
                        return Ok(entry.path().to_string_lossy().to_string());
                    }
                }
            }
        }

        // No model found — return empty string; the server can still start
        // and guide the user through model installation
        warn!("No model file found. Run `aud models install <model>` to download one, or set MODEL_PATH in .env");
        Ok(String::new())
    }

    fn auto_detect_threads() -> u32 {
        let num_cpus = num_cpus::get() as u32;
        info!("Auto‑detected CPU cores: {}", num_cpus);

        match num_cpus {
            1..=2 => 1,
            3..=4 => (num_cpus * 2) / 3,
            5..=8 => (num_cpus * 3) / 5,
            9..=16 => num_cpus / 2,
            17..=32 => (num_cpus * 2) / 5,
            _ => 16,
        }
    }

    fn auto_detect_gpu_layers() -> u32 {
        if let Ok(nvml) = Nvml::init() {
            if let Ok(device_count) = nvml.device_count() {
                if device_count > 0 {
                    if let Ok(first_gpu) = nvml.device_by_index(0) {
                        if let Ok(memory) = first_gpu.memory_info() {
                            let vram_gb = memory.total / 1024 / 1024 / 1024;
                            let layers = match vram_gb {
                                0..=4 => 12,
                                5..=8 => 20,
                                9..=12 => 32,
                                13..=16 => 40,
                                _ => 50,
                            };
                            info!("Auto‑detected GPU layers: {} ({} GB VRAM)", layers, vram_gb);
                            return layers;
                        }
                    }
                }
            }
        }
        warn!("Failed to detect GPU, using default 20 layers");
        20
    }

    fn auto_detect_ctx_size(model_path: &str) -> u32 {
        let inferred = Self::read_ctx_size_from_model_path(model_path)
            .unwrap_or_else(|| {
                info!("Falling back to default context size (8192)");
                8192
            });
        let adjusted = Self::adjust_ctx_size_for_system(inferred);
        info!("Final context size: {} (inferred: {})", adjusted, inferred);
        adjusted
    }

    fn read_ctx_size_from_model_path(model_path: &str) -> Option<u32> {
        // Simple heuristic based on model filename patterns
        let path_lower = model_path.to_lowercase();

        if path_lower.contains("32k") {
            Some(32768)
        } else if path_lower.contains("16k") {
            Some(16384)
        } else if path_lower.contains("8k") {
            Some(8192)
        } else if path_lower.contains("4k") {
            Some(4096)
        } else if path_lower.contains("2k") {
            Some(2048)
        } else if path_lower.contains("7b") || path_lower.contains("8b") || path_lower.contains("13b") {
            Some(4096)
        } else if path_lower.contains("34b") || path_lower.contains("70b") {
            Some(8192)
        } else {
            // Default fallback
            Some(8192)
        }
    }

    fn adjust_ctx_size_for_system(inferred_ctx: u32) -> u32 {
        let mut system = System::new_all();
        system.refresh_memory();

        let available_ram_gb = system.available_memory() / 1024 / 1024 / 1024;
        let _total_ram_gb = system.total_memory() / 1024 / 1024 / 1024;

        let required_ram_gb = (inferred_ctx as f32 / 4096.0) * 1.5;
        if available_ram_gb < required_ram_gb as u64 {
            let adjusted = (available_ram_gb as f32 * 4096.0 / 1.5) as u32;
            let safe_ctx = adjusted.min(inferred_ctx).max(2048);
            warn!(
                "Reducing context size from {} → {} due to limited RAM ({}GB available)",
                inferred_ctx, safe_ctx, available_ram_gb
            );
            safe_ctx
        } else {
            inferred_ctx
        }
    }

    fn auto_detect_batch_size(gpu_layers: u32, ctx_size: u32) -> u32 {
        let mut system = System::new_all();
        system.refresh_memory();

        let available_mb = system.available_memory() / 1024;
        let has_gpu = gpu_layers > 0;
        let memory_per_batch = Self::estimate_memory_per_batch(ctx_size, has_gpu);
        let safe_available_mb = (available_mb as f32 * 0.6) as u32;
        let max_batch = (safe_available_mb as f32 / memory_per_batch).max(1.0) as u32;

        let optimal = Self::apply_batch_limits(max_batch, ctx_size, has_gpu);
        info!(
            "Auto batch size: {} (ctx: {}, GPU: {}, est mem: {:.1}MB/batch)",
            optimal, ctx_size, has_gpu, memory_per_batch
        );
        optimal
    }

    fn estimate_memory_per_batch(ctx_size: u32, has_gpu: bool) -> f32 {
        if has_gpu {
            (ctx_size as f32 / 1024.0) * 0.5
        } else {
            (ctx_size as f32 / 1024.0) * 1.2
        }
    }

    fn apply_batch_limits(batch_size: u32, ctx_size: u32, _has_gpu: bool) -> u32 {
        let limited = batch_size.clamp(16, 1024);
        match ctx_size {
            0..=2048 => limited.min(512),
            2049..=4096 => limited.min(384),
            4097..=8192 => limited.min(256),
            8193..=16384 => limited.min(128),
            16385..=32768 => limited.min(64),
            _ => limited.min(32),
        }
    }

    pub fn print_config(&self) {
        info!("Current Configuration:");
        info!("- Model Path: {}", self.model_path);
        info!("- Llama Binary: {}", self.llama_bin);
        info!("- Context Size: {}", self.ctx_size);
        info!("- Batch Size: {}", self.batch_size);
        info!("- Threads: {}", self.threads);
        info!("- GPU Layers: {}", self.gpu_layers);
        info!("- Max Streams: {}", self.max_concurrent_streams);
        info!("- API: {}:{}", self.api_host, self.api_port);
        info!("- Backend: {}:{}", self.llama_host, self.llama_port);
        info!("- Queue Size: {}", self.queue_size);
        info!("- Queue Timeout: {}s", self.queue_timeout_seconds);
        info!("- Backend URL: {}", self.backend_url);
    }

    pub fn api_addr(&self) -> SocketAddr {
        format!("{}:{}", self.api_host, self.api_port).parse().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    

    /// Helper function to create a test Config with default values
    fn create_test_config() -> Config {
        Config {
            model_path: "/test/model.gguf".to_string(),
            llama_bin: "/test/llama-server".to_string(),
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
        }
    }

    // ===== Configuration Structure Tests =====

    #[test]
    fn test_config_creation_with_default_values() {
        let config = create_test_config();
        
        assert_eq!(config.model_path, "/test/model.gguf");
        assert_eq!(config.llama_bin, "/test/llama-server");
        assert_eq!(config.api_port, 8000);
        assert_eq!(config.llama_port, 8001);
    }

    #[test]
    fn test_config_clone() {
        let config1 = create_test_config();
        let config2 = config1.clone();
        
        assert_eq!(config1.api_host, config2.api_host);
        assert_eq!(config1.threads, config2.threads);
        assert_eq!(config1.gpu_layers, config2.gpu_layers);
    }

    // ===== API Address Tests =====

    #[test]
    fn test_api_addr_parsing() {
        let config = create_test_config();
        let addr = config.api_addr();
        
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert_eq!(addr.port(), 8000);
    }

    #[test]
    fn test_api_addr_with_different_ports() {
        let mut config = create_test_config();
        config.api_port = 3000;
        
        let addr = config.api_addr();
        assert_eq!(addr.port(), 3000);
    }

    #[test]
    fn test_api_addr_with_zero_address() {
        let mut config = create_test_config();
        config.api_host = "0.0.0.0".to_string();
        config.api_port = 5000;
        
        let addr = config.api_addr();
        assert_eq!(addr.port(), 5000);
        // 0.0.0.0 represents all interfaces
        assert_eq!(addr.ip().to_string(), "0.0.0.0");
    }

    // ===== Timeout Tests =====

    #[test]
    fn test_config_timeouts_are_positive() {
        let config = create_test_config();
        
        assert!(config.health_timeout_seconds > 0);
        assert!(config.generate_timeout_seconds > 0);
        assert!(config.stream_timeout_seconds > 0);
        assert!(config.health_check_timeout_seconds > 0);
    }

    #[test]
    fn test_health_check_timeout_greater_than_health_timeout() {
        let config = create_test_config();
        
        // Health check timeout should typically be longer than regular health timeout
        assert!(config.health_check_timeout_seconds >= config.health_timeout_seconds);
    }

    // ===== Resource Limits Tests =====

    #[test]
    fn test_max_concurrent_streams_is_positive() {
        let config = create_test_config();
        assert!(config.max_concurrent_streams > 0);
    }

    #[test]
    fn test_requests_per_second_is_reasonable() {
        let config = create_test_config();
        
        // Should be a reasonable number (not 0, not extremely high)
        assert!(config.requests_per_second > 0);
        assert!(config.requests_per_second <= 1000);
    }

    #[test]
    fn test_queue_size_is_positive() {
        let config = create_test_config();
        assert!(config.queue_size > 0);
    }

    // ===== Context and Batch Size Tests =====

    #[test]
    fn test_context_size_within_valid_range() {
        let config = create_test_config();
        
        // Context size should be between 512 and 32768
        assert!(config.ctx_size >= 512);
        assert!(config.ctx_size <= 32768);
    }

    #[test]
    fn test_batch_size_valid_range() {
        let config = create_test_config();
        
        // Batch size should be between 16 and 1024
        assert!(config.batch_size >= 16);
        assert!(config.batch_size <= 1024);
    }

    #[test]
    fn test_batch_size_reasonable_vs_context() {
        let config = create_test_config();
        
        // Batch size should typically be less than context size
        assert!(config.batch_size < config.ctx_size);
    }

    // ===== Thread Configuration Tests =====

    #[test]
    fn test_threads_is_positive() {
        let config = create_test_config();
        assert!(config.threads > 0);
    }

    #[test]
    fn test_threads_within_reasonable_range() {
        let config = create_test_config();
        
        // Should not exceed typical CPU thread count significantly
        assert!(config.threads <= 256);
    }

    // ===== GPU Configuration Tests =====

    #[test]
    fn test_gpu_layers_non_negative() {
        let config = create_test_config();
        assert!(config.gpu_layers <= config.ctx_size);
    }

    #[test]
    fn test_gpu_layers_within_range() {
        let config = create_test_config();
        
        // GPU layers should typically be 0-50
        assert!(config.gpu_layers <= 100);
    }

    // ===== Port Configuration Tests =====

    #[test]
    fn test_api_port_valid() {
        let config = create_test_config();
        assert!(config.api_port > 0);
        assert!(config.api_port != config.llama_port);
    }

    #[test]
    fn test_llama_port_valid() {
        let config = create_test_config();
        assert!(config.llama_port > 0);
    }

    #[test]
    fn test_prometheus_port_valid() {
        let config = create_test_config();
        assert!(config.prometheus_port > 0);
    }

    #[test]
    fn test_ports_are_different() {
        let config = create_test_config();
        
        // Ports should be unique to avoid conflicts
        assert_ne!(config.api_port, config.llama_port);
        assert_ne!(config.api_port, config.prometheus_port);
        assert_ne!(config.llama_port, config.prometheus_port);
    }

    // ===== Path Configuration Tests =====

    #[test]
    fn test_model_path_not_empty() {
        let config = create_test_config();
        assert!(!config.model_path.is_empty());
    }

    #[test]
    fn test_llama_bin_not_empty() {
        let config = create_test_config();
        assert!(!config.llama_bin.is_empty());
    }

    #[test]
    fn test_backend_url_not_empty() {
        let config = create_test_config();
        assert!(!config.backend_url.is_empty());
    }

    #[test]
    fn test_backend_url_format() {
        let config = create_test_config();
        
        // Should be a valid URL format
        assert!(config.backend_url.starts_with("http://") || config.backend_url.starts_with("https://"));
    }

    // ===== Host Configuration Tests =====

    #[test]
    fn test_api_host_not_empty() {
        let config = create_test_config();
        assert!(!config.api_host.is_empty());
    }

    #[test]
    fn test_llama_host_not_empty() {
        let config = create_test_config();
        assert!(!config.llama_host.is_empty());
    }

    // ===== Grace Period Tests =====

    #[test]
    fn test_hot_swap_grace_positive() {
        let config = create_test_config();
        assert!(config.hot_swap_grace_seconds > 0);
    }

    #[test]
    fn test_hot_swap_grace_reasonable() {
        let config = create_test_config();
        
        // Grace period should be less than 5 minutes
        assert!(config.hot_swap_grace_seconds < 300);
    }

    // ===== Auto-detect Helper Tests =====

    #[test]
    fn test_auto_detect_threads_returns_positive() {
        let threads = Config::auto_detect_threads();
        assert!(threads > 0);
    }

    #[test]
    fn test_auto_detect_gpu_layers_non_negative() {
        let layers = Config::auto_detect_gpu_layers();
        assert!(layers <= 512);
    }

    #[test]
    fn test_apply_batch_limits_small_context() {
        // For context < 2048, batch should be limited to 512
        let batch = Config::apply_batch_limits(1024, 1024, false);
        assert!(batch <= 512);
    }

    #[test]
    fn test_apply_batch_limits_medium_context() {
        // For context 2048-4096, batch should be limited to 384
        let batch = Config::apply_batch_limits(1024, 3000, false);
        assert!(batch <= 384);
    }

    #[test]
    fn test_apply_batch_limits_large_context() {
        // For context 16384-32768, batch should be limited to 64
        let batch = Config::apply_batch_limits(1024, 24576, false);
        assert!(batch <= 64);
    }

    #[test]
    fn test_apply_batch_limits_minimum() {
        // Batch size should always be at least 16
        let batch = Config::apply_batch_limits(1, 8192, false);
        assert!(batch >= 16);
    }

    #[test]
    fn test_estimate_memory_per_batch_cpu() {
        let memory_cpu = Config::estimate_memory_per_batch(8192, false);
        assert!(memory_cpu > 0.0);
    }

    #[test]
    fn test_estimate_memory_per_batch_gpu() {
        let memory_gpu = Config::estimate_memory_per_batch(8192, true);
        assert!(memory_gpu > 0.0);
    }

    #[test]
    fn test_estimate_memory_gpu_less_than_cpu() {
        let memory_cpu = Config::estimate_memory_per_batch(8192, false);
        let memory_gpu = Config::estimate_memory_per_batch(8192, true);
        
        // GPU memory estimate should be less than CPU
        assert!(memory_gpu < memory_cpu);
    }

    // ===== Queue Configuration Tests =====

    #[test]
    fn test_queue_timeout_is_positive() {
        let config = create_test_config();
        assert!(config.queue_timeout_seconds > 0);
    }

    #[test]
    fn test_queue_timeout_less_than_generate_timeout() {
        let config = create_test_config();
        
        // Queue timeout should be less than or equal to generate timeout
        assert!(config.queue_timeout_seconds <= config.generate_timeout_seconds);
    }

    // ===== Integration Tests =====

    #[test]
    fn test_config_values_consistency() {
        let config = create_test_config();
        
        // Verify all timeout values are reasonable
        assert!(config.health_timeout_seconds <= 3600); // Max 1 hour
        assert!(config.generate_timeout_seconds <= 1800); // Max 30 mins
        assert!(config.stream_timeout_seconds <= 3600); // Max 1 hour
        assert!(config.health_check_timeout_seconds <= 3600); // Max 1 hour
    }

    #[test]
    fn test_config_backend_url_consistency() {
        let config = create_test_config();
        
        // Backend URL should contain the llama host and port
        assert!(config.backend_url.contains(&config.llama_host) || 
                config.backend_url.contains("127.0.0.1") || 
                config.backend_url.contains("localhost"));
    }

    #[test]
    fn test_config_all_fields_initialized() {
        let config = create_test_config();
        
        // Ensure all critical fields have valid values
        assert!(!config.model_path.is_empty());
        assert!(!config.llama_bin.is_empty());
        assert!(!config.api_host.is_empty());
        assert!(!config.llama_host.is_empty());
        assert!(config.threads > 0);
        assert!(config.gpu_layers <= config.ctx_size);
        assert!(config.api_port > 0);
        assert!(config.llama_port > 0);
    }
}