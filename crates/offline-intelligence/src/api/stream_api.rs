//! Streaming chat endpoint — the core 1-hop architecture handler.
//!
//! Flow: Client POST → SharedState (session + cache lookup) → LLM Worker (HTTP to llama-server) → SSE stream back
//! All state access is in-process via Arc/shared memory. The only network hop is to localhost llama-server.

use axum::{
    extract::State,
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
    http::StatusCode,
    Json,
};
use futures_util::StreamExt;
use serde::Deserialize;
use std::convert::Infallible;
use tracing::{info, error, debug};

use crate::memory::Message;
use crate::memory_db::schema::Embedding;
use crate::shared_state::UnifiedAppState;

/// Request body matching what the frontend sends
#[derive(Debug, Deserialize)]
pub struct StreamChatRequest {
    pub model: Option<String>,
    pub messages: Vec<Message>,
    pub session_id: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_stream")]
    pub stream: bool,
}

fn default_max_tokens() -> u32 { 2000 }
fn default_temperature() -> f32 { 0.7 }
fn default_stream() -> bool { true }

/// POST /generate/stream — Main streaming chat endpoint
///
/// 1. Validates request and gets/creates session in shared memory
/// 2. Persists user message to database
/// 3. Streams LLM response back via SSE
/// 4. Persists assistant response to database after completion
pub async fn generate_stream(
    State(state): State<UnifiedAppState>,
    Json(req): Json<StreamChatRequest>,
) -> Response {
    let request_num = state.shared_state.counters.inc_total_requests();
    info!("Stream request #{} for session: {}", request_num, req.session_id);

    if req.messages.is_empty() {
        return (StatusCode::BAD_REQUEST, "Messages array cannot be empty").into_response();
    }

    let session_id = req.session_id.clone();

    // 1. Get or create session in shared memory (zero-cost Arc lookup)
    let session = state.shared_state.get_or_create_session(&session_id).await;

    // 2. Update in-memory session with the incoming messages
    {
        if let Ok(mut session_data) = session.write() {
            session_data.last_accessed = std::time::Instant::now();
            session_data.messages = req.messages.clone();
        }
    }

    // 3. Ensure session exists in database and persist user message
    //    Also capture the user message content for embedding generation later
    let user_msg_content = req.messages.iter().rev().find(|m| m.role == "user").map(|m| m.content.clone());
    if let Some(ref content) = user_msg_content {
        let db = state.shared_state.database_pool.clone();
        let sid = session_id.clone();
        let content = content.clone();
        tokio::spawn(async move {
            // Ensure session exists in DB (ignore error if already exists)
            let _ = db.conversations.create_session_with_id(&sid, None);
            // Get next available message index
            let next_index = db.conversations.get_session_message_count(&sid)
                .unwrap_or(0) as i32;
            // Persist user message via batch API
            if let Err(e) = db.conversations.store_messages_batch(
                &sid,
                &[("user".to_string(), content, next_index, 0, 0.5)],
            ) {
                error!("Failed to persist user message: {}", e);
            }
        });
    }

    // 4. Context Engine: Retrieve past context via semantic search when KV cache misses.
    //    Always let the retrieval planner decide — even a brand-new session can trigger
    //    cross-session search if the user asks "what did we discuss yesterday?".
    //    The planner + orchestrator handle the "nothing to search" case internally
    //    (checks has_embeddings > 0 before hitting llama-server, returns early if no past refs).
    let context_messages = {
        let orchestrator_guard = state.context_orchestrator.read().await;
        if let Some(ref orchestrator) = *orchestrator_guard {
            let user_query = user_msg_content.as_deref();
            match orchestrator.process_conversation(&session_id, &req.messages, user_query).await {
                Ok(optimized) => {
                    if optimized.len() != req.messages.len() {
                        info!("Context engine optimized: {} → {} messages (retrieved past context)",
                            req.messages.len(), optimized.len());
                    }
                    optimized
                }
                Err(e) => {
                    error!("Context engine error (falling back to raw messages): {}", e);
                    req.messages.clone()
                }
            }
        } else {
            debug!("Context orchestrator not initialized, using raw messages");
            req.messages.clone()
        }
    };

    // 5. Stream from LLM worker (single network hop to localhost llama-server)
    let llm_worker = state.llm_worker.clone();
    let max_tokens = req.max_tokens;
    let temperature = req.temperature;
    let db_for_persist = state.shared_state.database_pool.clone();
    let session_id_for_persist = session_id.clone();

    // Clones for background embedding generation after stream completes
    let llm_worker_for_embed = state.llm_worker.clone();
    let db_for_embed_persist = state.shared_state.database_pool.clone();
    let session_id_for_embed = session_id.clone();
    let user_msg_for_embed = user_msg_content.clone();

    match llm_worker.stream_response(context_messages, max_tokens, temperature).await {
        Ok(llm_stream) => {
            // Wrap the LLM stream to collect the full response for DB persistence
            let output_stream = async_stream::stream! {
                let mut full_response = String::new();

                futures_util::pin_mut!(llm_stream);

                while let Some(item) = llm_stream.next().await {
                    match item {
                        Ok(sse_line) => {
                            // Extract content from SSE data for persistence
                            if sse_line.starts_with("data: ") && !sse_line.contains("[DONE]") {
                                if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(&sse_line[6..].trim()) {
                                    if let Some(content) = chunk
                                        .get("choices")
                                        .and_then(|c| c.get(0))
                                        .and_then(|c| c.get("delta"))
                                        .and_then(|d| d.get("content"))
                                        .and_then(|c| c.as_str())
                                    {
                                        full_response.push_str(content);
                                    }
                                }
                            }

                            // Yield SSE event to client
                            let data = sse_line.trim_start_matches("data: ").trim_end().to_string();
                            yield Ok::<_, Infallible>(Event::default().data(data));
                        }
                        Err(e) => {
                            error!("Stream error: {}", e);
                            yield Ok(Event::default().data(
                                format!("{{\"error\": \"{}\"}}", e)
                            ));
                            break;
                        }
                    }
                }

                // Persist assistant response to database after stream completes
                if !full_response.is_empty() {
                    let assistant_index = db_for_persist.conversations
                        .get_session_message_count(&session_id_for_persist)
                        .unwrap_or(0) as i32;
                    match db_for_persist.conversations.store_messages_batch(
                        &session_id_for_persist,
                        &[("assistant".to_string(), full_response.clone(), assistant_index, 0, 0.5)],
                    ) {
                        Ok(stored_msgs) => {
                            debug!("Persisted assistant response ({} chars) for session {}",
                                full_response.len(), session_id_for_persist);

                            // Background: Generate and store embeddings for the new messages
                            // This captures the vectors llama.cpp computes via /v1/embeddings
                            // enabling semantic search for future KV cache misses.
                            let llm_for_embed = llm_worker_for_embed.clone();
                            let db_for_embed = db_for_embed_persist.clone();
                            let assistant_content = full_response.clone();
                            let user_content_for_embed = user_msg_for_embed.clone();
                            let stored = stored_msgs;

                            tokio::spawn(async move {
                                // Collect texts + their message IDs for embedding
                                let mut texts = Vec::new();
                                let mut message_ids = Vec::new();

                                // User message embedding (get ID from DB)
                                if let Some(ref user_text) = user_content_for_embed {
                                    // The user message was stored one index before the assistant
                                    // We need its DB ID — query by session + content
                                    if let Ok(msgs) = db_for_embed.search_messages_by_keywords(
                                        &session_id_for_embed,
                                        &[user_text.clone()],
                                        1,
                                    ).await {
                                        if let Some(user_stored) = msgs.first() {
                                            texts.push(user_text.clone());
                                            message_ids.push(user_stored.id);
                                        }
                                    }
                                }

                                // Assistant message embedding
                                if let Some(assistant_stored) = stored.first() {
                                    texts.push(assistant_content);
                                    message_ids.push(assistant_stored.id);
                                }

                                if texts.is_empty() {
                                    return;
                                }

                                // Call llama-server /v1/embeddings
                                match llm_for_embed.generate_embeddings(texts).await {
                                    Ok(embeddings) => {
                                        let now = chrono::Utc::now();
                                        for (embedding_vec, msg_id) in embeddings.into_iter().zip(message_ids.iter()) {
                                            let emb = Embedding {
                                                id: 0, // auto-assigned by DB
                                                message_id: *msg_id,
                                                embedding: embedding_vec,
                                                embedding_model: "llama-server".to_string(),
                                                generated_at: now,
                                            };
                                            if let Err(e) = db_for_embed.embeddings.store_embedding(&emb) {
                                                debug!("Failed to store embedding for msg {}: {}", msg_id, e);
                                            }
                                        }
                                        // Mark messages as having embeddings
                                        for msg_id in &message_ids {
                                            let _ = db_for_embed.conversations.mark_embedding_generated(*msg_id);
                                        }
                                        debug!("Stored {} embeddings for session {}", message_ids.len(), session_id_for_embed);
                                    }
                                    Err(e) => {
                                        debug!("Embedding generation skipped (llama-server may not support /v1/embeddings): {}", e);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            error!("Failed to persist assistant message: {}", e);
                        }
                    }
                }
            };

            Sse::new(output_stream)
                .keep_alive(
                    axum::response::sse::KeepAlive::new()
                        .interval(std::time::Duration::from_secs(15))
                )
                .into_response()
        }
        Err(e) => {
            error!("Failed to start LLM stream: {}", e);
            (StatusCode::BAD_GATEWAY, format!("LLM backend error: {}", e)).into_response()
        }
    }
}
