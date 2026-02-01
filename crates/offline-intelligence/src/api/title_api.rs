// Title generation API: summarize first prompt into 1-5 word chat title via model inference
// Uses direct LLM integration with shared state for 1-hop architecture
use axum::{
    extract::{State, Json},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::shared_state::UnifiedAppState;

#[derive(Debug, Deserialize)]
pub struct GenerateTitleRequest {
    pub prompt: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_max_tokens() -> u32 {
    20
}

#[derive(Debug, Serialize)]
pub struct GenerateTitleResponse {
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Generate a concise chat title (1-5 words) from a user prompt using the LLM.
/// Uses temperature 0.3 for consistent results, caps output at 20 tokens
pub async fn generate_title(
    State(state): State<UnifiedAppState>,
    Json(req): Json<GenerateTitleRequest>,
) -> Result<Json<GenerateTitleResponse>, (StatusCode, Json<ErrorResponse>)> {
    info!("Generating title for prompt (length: {} chars)", req.prompt.len());

    if req.prompt.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Prompt cannot be empty".to_string(),
            }),
        ));
    }

    let llm_worker = state.llm_worker.clone();

    // Improved instruction: prioritize clarity and let LLM naturally generate 1-5 word titles
    let title_instruction = format!(
        "User prompt: {}\n\n\
         Create a short, meaningful chat title using 1-5 words maximum that captures the essence of this prompt.",
        req.prompt
    );

    // Generate title using LLM worker directly (1-hop architecture)
    match llm_worker.generate_title(&title_instruction, req.max_tokens.min(20)).await {
        Ok(title) => {
            let word_count = title.split_whitespace().count();
            info!("Generated title: '{}' ({} words)", title, word_count);
            Ok(Json(GenerateTitleResponse { title }))
        }
        Err(e) => {
            info!("Title generation failed: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Title generation failed: {}", e),
                }),
            ))
        }
    }
}
