//! Search API endpoints — hybrid semantic + keyword search across conversations.
//!
//! When a query comes in:
//! 1. Generate an embedding for the query via llama-server /v1/embeddings
//! 2. Search HNSW index for semantically similar messages (cosine similarity)
//! 3. Fall back to keyword search if embeddings are unavailable
//! 4. Merge and rank results by combined relevance score

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tracing::{info, debug};

use crate::shared_state::SharedState;

/// Search request payload
#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub session_id: Option<String>,
    pub limit: Option<i32>,
    /// Minimum similarity threshold for semantic results (0.0 - 1.0, default 0.3)
    pub similarity_threshold: Option<f32>,
}

/// Search response
#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub total: usize,
    pub search_type: String, // "semantic", "keyword", or "hybrid"
}

/// Individual search result
#[derive(Debug, Serialize, Clone)]
pub struct SearchResult {
    pub session_id: String,
    pub message_id: i64,
    pub content: String,
    pub role: String,
    pub relevance_score: f32,
    pub search_source: String, // "semantic" or "keyword"
}

/// Search endpoint handler — hybrid semantic + keyword search
pub async fn search(
    State(shared_state): State<Arc<SharedState>>,
    Json(payload): Json<SearchRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    info!("Search request: query='{}', session={:?}, limit={:?}",
          payload.query, payload.session_id, payload.limit);

    // Validate input
    if payload.query.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Query cannot be empty".to_string()));
    }

    let limit = payload.limit.unwrap_or(10).clamp(1, 100) as usize;
    let similarity_threshold = payload.similarity_threshold.unwrap_or(0.3);

    let mut all_results: Vec<SearchResult> = Vec::new();
    let mut search_type = String::from("keyword"); // default

    // ── Phase 1: Semantic search via embeddings ──
    let llm_worker = &shared_state.llm_worker;
    let db = &shared_state.database_pool;

    // Try to generate query embedding
    match llm_worker.generate_embeddings(vec![payload.query.clone()]).await {
        Ok(query_embeddings) if !query_embeddings.is_empty() => {
            let query_vec = &query_embeddings[0];

            // Search HNSW index (or linear fallback) for similar message embeddings
            match db.embeddings.find_similar_embeddings(
                query_vec,
                "llama-server",
                (limit * 2) as i32, // fetch extra, we'll filter
                similarity_threshold,
            ) {
                Ok(similar_ids) if !similar_ids.is_empty() => {
                    search_type = "semantic".to_string();
                    debug!("Semantic search found {} candidates", similar_ids.len());

                    // Fetch the actual messages for each matching embedding
                    for (message_id, similarity) in &similar_ids {
                        // Get the message content from DB
                        if let Ok(Some(session_id_filter)) = get_message_session_id(db, *message_id) {
                            // If session filter is set, skip messages from other sessions
                            if let Some(ref filter_sid) = payload.session_id {
                                if &session_id_filter != filter_sid {
                                    continue;
                                }
                            }
                            if let Ok(msg) = get_message_by_id(db, *message_id) {
                                all_results.push(SearchResult {
                                    session_id: session_id_filter,
                                    message_id: *message_id,
                                    content: msg.content,
                                    role: msg.role,
                                    relevance_score: *similarity,
                                    search_source: "semantic".to_string(),
                                });
                            }
                        }
                    }
                }
                Ok(_) => {
                    debug!("Semantic search returned no results above threshold {}", similarity_threshold);
                }
                Err(e) => {
                    debug!("Semantic search failed (falling back to keyword): {}", e);
                }
            }
        }
        Ok(_) => {
            debug!("Empty embedding response, falling back to keyword search");
        }
        Err(e) => {
            debug!("Embedding generation unavailable ({}), using keyword search only", e);
        }
    }

    // ── Phase 2: Keyword search (always runs as fallback/supplement) ──
    let keywords: Vec<String> = payload.query
        .split_whitespace()
        .filter(|word| word.len() > 2)
        .map(|s| s.to_lowercase())
        .collect();

    if !keywords.is_empty() {
        let orchestrator_guard = shared_state.context_orchestrator.read().await;
        if let Some(orchestrator) = &*orchestrator_guard {
            if let Ok(stored_messages) = orchestrator.search_messages(
                payload.session_id.as_deref(),
                &keywords,
                limit,
            ).await {
                let semantic_ids: std::collections::HashSet<i64> = all_results.iter()
                    .map(|r| r.message_id)
                    .collect();

                for msg in stored_messages {
                    // Skip duplicates already found by semantic search
                    if semantic_ids.contains(&msg.id) {
                        continue;
                    }

                    let keyword_score = calculate_relevance(&msg.content, &keywords);
                    all_results.push(SearchResult {
                        session_id: msg.session_id,
                        message_id: msg.id,
                        content: msg.content,
                        role: msg.role,
                        relevance_score: keyword_score,
                        search_source: "keyword".to_string(),
                    });
                }

                if search_type == "semantic" && all_results.iter().any(|r| r.search_source == "keyword") {
                    search_type = "hybrid".to_string();
                }
            }
        }
    }

    // ── Phase 3: Sort by relevance and truncate ──
    all_results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap_or(std::cmp::Ordering::Equal));
    all_results.truncate(limit);

    let total = all_results.len();
    info!("Search completed: {} results ({})", total, search_type);

    Ok(Json(SearchResponse {
        results: all_results,
        total,
        search_type,
    }))
}

/// Calculate keyword relevance score
fn calculate_relevance(content: &str, keywords: &[String]) -> f32 {
    let content_lower = content.to_lowercase();
    let mut score = 0.0;

    for keyword in keywords {
        let matches = content_lower.matches(keyword).count();
        if matches > 0 {
            score += matches as f32 * (keyword.len() as f32 / content.len().max(1) as f32);
        }
    }

    score.min(1.0)
}

/// Helper: get the session_id for a message by its ID
fn get_message_session_id(
    db: &crate::memory_db::MemoryDatabase,
    message_id: i64,
) -> anyhow::Result<Option<String>> {
    let pool_conn = db.conversations.get_conn_public()?;
    let mut stmt = pool_conn.prepare(
        "SELECT session_id FROM messages WHERE id = ?1"
    )?;
    let mut rows = stmt.query([message_id])?;
    if let Some(row) = rows.next()? {
        let sid: String = row.get(0)?;
        Ok(Some(sid))
    } else {
        Ok(None)
    }
}

/// Helper: minimal message data by ID
struct MinimalMessage {
    content: String,
    role: String,
}

fn get_message_by_id(
    db: &crate::memory_db::MemoryDatabase,
    message_id: i64,
) -> anyhow::Result<MinimalMessage> {
    let conn = db.conversations.get_conn_public()?;
    let mut stmt = conn.prepare(
        "SELECT content, role FROM messages WHERE id = ?1"
    )?;
    let mut rows = stmt.query([message_id])?;
    if let Some(row) = rows.next()? {
        Ok(MinimalMessage {
            content: row.get(0)?,
            role: row.get(1)?,
        })
    } else {
        Err(anyhow::anyhow!("Message {} not found", message_id))
    }
}
