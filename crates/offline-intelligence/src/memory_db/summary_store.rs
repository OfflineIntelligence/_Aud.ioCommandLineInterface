// "D:\_ProjectWorks\AUDIO_Interface\Server\src\memory_db\summary_store.rs"
//! Summary storage and retrieval operations

use crate::memory_db::schema::*;
use rusqlite::{params, Result, Row};
use chrono::{DateTime, Utc};
use tracing::{debug, info};
use std::sync::Arc;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

/// Manages summary storage and retrieval
pub struct SummaryStore {
    pool: Arc<Pool<SqliteConnectionManager>>,
}

impl SummaryStore {
    /// Create a new summary store
    pub fn new(pool: Arc<Pool<SqliteConnectionManager>>) -> Self {
        Self { pool }
    }

    /// Internal helper to get a connection
    fn get_conn(&self) -> anyhow::Result<r2d2::PooledConnection<SqliteConnectionManager>> {
        self.pool.get()
            .map_err(|e| anyhow::anyhow!("Failed to get connection from pool: {}", e))
    }

    /// Store a summary
    pub fn store_summary(&self, summary: &Summary) -> anyhow::Result<()> {
        let conn = self.get_conn()?;
        
        debug!(
            "Storing summary for session {} (messages {} to {})",
            summary.session_id,
            summary.message_range_start,
            summary.message_range_end
        );
        
        conn.execute(
            "INSERT INTO summaries 
             (session_id, message_range_start, message_range_end, summary_text, 
              compression_ratio, key_topics, generated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &summary.session_id,
                summary.message_range_start,
                summary.message_range_end,
                &summary.summary_text,
                summary.compression_ratio,
                serde_json::to_string(&summary.key_topics)?,
                summary.generated_at.to_rfc3339(),
            ],
        )?;
        
        Ok(())
    }

    /// Get summaries for a session
    pub fn get_session_summaries(&self, session_id: &str) -> anyhow::Result<Vec<Summary>> {
        let conn = self.get_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, message_range_start, message_range_end, summary_text,
             compression_ratio, key_topics, generated_at
             FROM summaries WHERE session_id = ?1 ORDER BY generated_at DESC"
        )?;
        
        let mut rows = stmt.query([session_id])?;
        let mut summaries = Vec::new();
        
        while let Some(row) = rows.next()? {
            summaries.push(self.row_to_summary(row)?);
        }
        
        Ok(summaries)
    }

    /// Get summary for a specific message range
    pub fn get_summary_for_range(&self, session_id: &str, start: i32, end: i32) -> anyhow::Result<Option<Summary>> {
        let conn = self.get_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, message_range_start, message_range_end, summary_text,
             compression_ratio, key_topics, generated_at
             FROM summaries WHERE session_id = ?1 AND message_range_start = ?2 AND message_range_end = ?3"
        )?;
        
        let mut rows = stmt.query(params![session_id, start, end])?;
        
        if let Some(row) = rows.next()? {
            Ok(Some(self.row_to_summary(row)?))
        } else {
            Ok(None)
        }
    }

    /// Update an existing summary
    pub fn update_summary(&self, summary: &Summary) -> anyhow::Result<()> {
        let conn = self.get_conn()?;
        
        debug!("Updating summary for session {}", summary.session_id);
        
        conn.execute(
            "UPDATE summaries SET
             summary_text = ?2,
             compression_ratio = ?3,
             key_topics = ?4,
             generated_at = ?5
             WHERE id = ?1",
            params![
                summary.id,
                &summary.summary_text,
                summary.compression_ratio,
                serde_json::to_string(&summary.key_topics)?,
                summary.generated_at.to_rfc3339(),
            ],
        )?;
        
        Ok(())
    }

    /// Delete summaries for a session
    pub fn delete_session_summaries(&self, session_id: &str) -> anyhow::Result<usize> {
        let conn = self.get_conn()?;
        let deleted = conn.execute(
            "DELETE FROM summaries WHERE session_id = ?1",
            [session_id],
        )?;
        
        info!("Deleted {} summaries for session {}", deleted, session_id);
        Ok(deleted)
    }

    /// Clean up old summaries for a session
    pub fn cleanup_old_summaries(&self, session_id: &str, keep_latest: usize) -> anyhow::Result<usize> {
        let conn = self.get_conn()?;
        
        // Get IDs of summaries to delete - FIXED: use rusqlite::params for mixed types
        let mut stmt = conn.prepare(
            "SELECT id FROM summaries 
             WHERE session_id = ?1 
             ORDER BY generated_at DESC
             LIMIT -1 OFFSET ?2"
        )?;
        
        // Use params! macro for mixed types
        let ids_to_delete: Vec<i64> = stmt
            .query_map(params![session_id, keep_latest as i64], |row| row.get(0))?
            .collect::<Result<Vec<_>>>()?;
        
        if ids_to_delete.is_empty() {
            return Ok(0);
        }
        
        // Create placeholders for the IN clause
        let placeholders: Vec<String> = ids_to_delete.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            "DELETE FROM summaries WHERE id IN ({})",
            placeholders.join(",")
        );
        
        let mut stmt = conn.prepare(&query)?;
        let deleted = stmt.execute(rusqlite::params_from_iter(ids_to_delete))?;
        
        debug!("Cleaned up {} old summaries for session {}", deleted, session_id);
        Ok(deleted)
    }

    /// Convert a database row to a Summary struct
    fn row_to_summary(&self, row: &Row) -> anyhow::Result<Summary> {
        let key_topics_json: String = row.get(6)?;
        let key_topics: Vec<String> = serde_json::from_str(&key_topics_json)
            .map_err(|e| anyhow::anyhow!("Failed to parse key_topics: {}", e))?;
        
        let generated_at_str: String = row.get(7)?;
        let generated_at = DateTime::parse_from_rfc3339(&generated_at_str)?
            .with_timezone(&Utc);
        
        Ok(Summary {
            id: row.get(0)?,
            session_id: row.get(1)?,
            message_range_start: row.get(2)?,
            message_range_end: row.get(3)?,
            summary_text: row.get(4)?,
            compression_ratio: row.get(5)?,
            key_topics,
            generated_at,
        })
    }
}