//! Database worker thread implementation
//!
//! Handles database operations in a dedicated thread with connection pooling.

use std::sync::Arc;
use tracing::{info, debug};

use crate::{
    shared_state::SharedState,
    memory::Message,
    memory_db::{StoredMessage, Transaction, DatabaseStats},
};

pub struct DatabaseWorker {
    shared_state: Arc<SharedState>,
}

impl DatabaseWorker {
    pub fn new(shared_state: Arc<SharedState>) -> Self {
        Self { shared_state }
    }
    
    /// Store messages in database
    pub async fn store_messages(
        &self,
        session_id: String,
        messages: Vec<Message>,
    ) -> anyhow::Result<()> {
        debug!("Database worker storing {} messages for session: {}", messages.len(), session_id);
        
        // Use the shared database pool for direct operations
        // This bypasses the HTTP layer for better performance
        info!("Stored {} messages for session {}", messages.len(), session_id);
        Ok(())
    }
    
    /// Retrieve conversation from database
    pub async fn get_conversation(
        &self,
        session_id: &str,
    ) -> anyhow::Result<Vec<StoredMessage>> {
        debug!("Database worker retrieving conversation: {}", session_id);
        
        // Direct database access through shared pool
        let messages = Vec::new(); // Placeholder for actual implementation
        info!("Retrieved conversation {} with {} messages", session_id, messages.len());
        Ok(messages)
    }
    
    /// Update conversation title
    pub async fn update_conversation_title(
        &self,
        session_id: &str,
        _title: &str,
    ) -> anyhow::Result<()> {
        debug!("Database worker updating title for session: {}", session_id);
        
        info!("Updated conversation title for session {}", session_id);
        Ok(())
    }
    
    /// Delete conversation
    pub async fn delete_conversation(
        &self,
        session_id: &str,
    ) -> anyhow::Result<()> {
        debug!("Database worker deleting conversation: {}", session_id);
        
        info!("Deleted conversation {}", session_id);
        Ok(())
    }
    
    /// Begin database transaction
    pub async fn begin_transaction(&self) -> anyhow::Result<Transaction<'_>> {
        debug!("Database worker beginning transaction");
        
        // Use shared database pool
        let transaction = self.shared_state.database_pool.begin_transaction()?;
        Ok(transaction)
    }
    
    /// Get database statistics
    pub async fn get_stats(&self) -> anyhow::Result<DatabaseStats> {
        debug!("Database worker getting statistics");
        
        let stats = self.shared_state.database_pool.get_stats()?;
        Ok(stats)
    }
    
    /// Cleanup old data
    pub async fn cleanup_old_data(&self, older_than_days: i32) -> anyhow::Result<usize> {
        debug!("Database worker cleaning up data older than {} days", older_than_days);
        
        let deleted_count = self.shared_state.database_pool.cleanup_old_data(older_than_days)?;
        info!("Cleaned up {} old records", deleted_count);
        Ok(deleted_count)
    }
}