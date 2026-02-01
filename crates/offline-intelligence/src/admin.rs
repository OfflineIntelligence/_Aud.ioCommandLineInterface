// Server/src/admin.rs
// Simplified for 1-hop architecture - removed external process dependencies

use axum::extract::{State, Json};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use crate::config::Config;
use crate::metrics;
use crate::shared_state::SharedState;
use serde::{Deserialize, Serialize};
use tracing::info;
use std::sync::Arc;
use sysinfo::System;


#[allow(dead_code)]
#[derive(Clone)]
pub struct AdminState {
    pub cfg: Config,
    pub shared_state: Arc<SharedState>,
}

#[derive(Deserialize)]
pub struct LoadModelRequest {
    pub model_path: String,
    pub ctx_size: Option<u32>,
    pub gpu_layers: Option<u32>,
    pub batch_size: Option<u32>,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub current_model: Option<String>,
    pub current_port: Option<u16>,
    pub gpu_layers: Option<u32>,
    pub ctx_size: Option<u32>,        // Add context size
    pub batch_size: Option<u32>,      // Add batch size
    pub is_healthy: bool,
    pub uptime_seconds: Option<u64>,
    pub memory_usage: Option<String>, // Add memory info
}

pub async fn get_status(
    State(state): State<AdminState>,
) -> impl IntoResponse {
    // Simplified status for 1-hop architecture
    let is_healthy = true; // Always healthy in direct memory access
    
    // Memory info
    let memory_usage = {
        let mut sys = System::new_all();
        sys.refresh_memory();
        let used = sys.used_memory();
        let total = sys.total_memory();
        Some(format!("{}/{} MB", used / 1024 / 1024, total / 1024 / 1024))
    };

    let response = StatusResponse {
        current_model: Some("direct-llm".to_string()),
        current_port: None,
        gpu_layers: Some(0),
        ctx_size: Some(state.cfg.ctx_size),
        batch_size: Some(state.cfg.batch_size),
        is_healthy,
        uptime_seconds: Some(0),
        memory_usage,
    };
    
    metrics::inc_request("admin_status", "ok");
    (StatusCode::OK, Json(response))
}

pub async fn load_model(
    State(_state): State<AdminState>,
    Json(req): Json<LoadModelRequest>,
) -> impl IntoResponse {
    info!("Received load model request for: {} with ctx_size: {:?}, gpu_layers: {:?}", 
          req.model_path, req.ctx_size, req.gpu_layers);
    
    // In 1-hop architecture, model loading happens directly through shared state
    // This is a placeholder implementation
    metrics::inc_request("admin_load", "ok");
    (StatusCode::OK, format!("Model loading initiated: {}", req.model_path))
}

pub async fn stop_backend(
    State(_state): State<AdminState>,
) -> impl IntoResponse {
    info!("Received stop backend request");
    
    // In 1-hop architecture, there's no separate backend to stop
    // This is a placeholder implementation
    metrics::inc_request("admin_stop", "ok");
    (StatusCode::OK, "System shutdown initiated".to_string())
}

