//! Context worker thread implementation
//!
//! Handles conversation context optimization and management in a dedicated thread.

use std::sync::Arc;
use tracing::{info, debug, warn};

use crate::{
    shared_state::SharedState,
    memory::Message,
};

pub struct ContextWorker {
    shared_state: Arc<SharedState>,
}

impl ContextWorker {
    pub fn new(shared_state: Arc<SharedState>) -> Self {
        Self { shared_state }
    }

    /// Process conversation context optimization
    pub async fn process_conversation(
        &self,
        session_id: String,
        messages: Vec<Message>,
        user_query: Option<&str>,
    ) -> anyhow::Result<Vec<Message>> {
        debug!("Context worker processing conversation for session: {}", session_id);

        // Access context orchestrator through shared state (tokio RwLock)
        let orchestrator_guard = self.shared_state.context_orchestrator.read().await;

        if let Some(ref orchestrator) = *orchestrator_guard {
            match orchestrator.process_conversation(&session_id, &messages, user_query).await {
                Ok(optimized) => {
                    debug!("Context optimized: {} -> {} messages", messages.len(), optimized.len());
                    Ok(optimized)
                }
                Err(e) => {
                    warn!("Context optimization failed: {}, using original", e);
                    Ok(messages)
                }
            }
        } else {
            warn!("Context orchestrator not available, using original messages");
            Ok(messages)
        }
    }

    /// Save assistant response to database
    pub async fn save_assistant_response(
        &self,
        session_id: &str,
        assistant_content: &str,
    ) -> anyhow::Result<()> {
        debug!("Saving assistant response for session: {}", session_id);

        let orchestrator_guard = self.shared_state.context_orchestrator.read().await;
        if let Some(ref orchestrator) = *orchestrator_guard {
            orchestrator.save_assistant_response(session_id, assistant_content).await?;
            info!("Assistant response saved for session: {}", session_id);
        }

        Ok(())
    }

    /// Ensure session exists in database
    pub async fn ensure_session_exists(
        &self,
        session_id: &str,
        title: Option<&str>,
    ) -> anyhow::Result<()> {
        let orchestrator_guard = self.shared_state.context_orchestrator.read().await;
        if let Some(ref orchestrator) = *orchestrator_guard {
            let tier_manager_guard = orchestrator.tier_manager().read().await;
            if let Err(e) = tier_manager_guard.ensure_session_exists(session_id, title.map(|s| s.to_string())).await {
                return Err(anyhow::anyhow!("Failed to ensure session exists: {}", e));
            }
            info!("Ensured session {} exists", session_id);
        }

        Ok(())
    }
}
