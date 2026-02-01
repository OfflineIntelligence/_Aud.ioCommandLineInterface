// "D:\_ProjectWorks\AUDIO_Interface\Server\src\memory_db\mod.rs"
//! Memory database module - SQLite-based storage for conversations, summaries, and embeddings

pub mod schema;
pub mod migration;
pub mod conversation_store;
pub mod summary_store;
pub mod embedding_store;

// Re-export commonly used types
pub use schema::*;
pub use migration::MigrationManager;
pub use conversation_store::ConversationStore;
pub use summary_store::SummaryStore;
pub use embedding_store::{EmbeddingStore, EmbeddingStats};

use std::path::Path;
use std::sync::Arc;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use tracing::info;
use crate::cache_management::cache_extractor::KVEntry;
use crate::cache_management::cache_manager::SessionCacheState;

/// Main database manager that coordinates all stores
pub struct MemoryDatabase {
    pub conversations: ConversationStore,
    pub summaries: SummaryStore,
    pub embeddings: EmbeddingStore,
    pool: Arc<Pool<SqliteConnectionManager>>,
}

/// Transaction manager for atomic operations across stores
pub struct Transaction<'a> {
    conn: r2d2::PooledConnection<SqliteConnectionManager>,
    _marker: std::marker::PhantomData<&'a MemoryDatabase>,
}

impl<'a> Transaction<'a> {
    /// Commit the transaction
    pub fn commit(self) -> anyhow::Result<()> {
        // Changes are automatically committed when the connection is dropped
        Ok(())
    }

    /// Rollback the transaction
    pub fn rollback(self) -> anyhow::Result<()> {
        // SQLite auto-rolls back on DROP if not committed
        Ok(())
    }

    /// Get raw connection for store operations
    pub fn connection(&mut self) -> &mut rusqlite::Connection {
        &mut self.conn
    }
}

impl MemoryDatabase {
    /// Create a new memory database at the specified path
    pub fn new(db_path: &Path) -> anyhow::Result<Self> {
        info!("Opening memory database at: {}", db_path.display());

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let manager = SqliteConnectionManager::file(db_path)
            .with_flags(
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
                | rusqlite::OpenFlags::SQLITE_OPEN_FULL_MUTEX,
            );

        let pool = Pool::builder()
            .max_size(10)
            .build(manager)
            .map_err(|e| anyhow::anyhow!("Failed to create connection pool: {}", e))?;

        // Initialize DB and pragmas - FIXED: Use mutable connection
        {
            let mut conn = pool.get()?;
            let mut migrator = migration::MigrationManager::new(&mut conn);
            migrator.initialize_database()?;

            conn.execute_batch(
                "PRAGMA foreign_keys = ON;
                 PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA busy_timeout = 5000;",
            )?;
        }

        let pool = Arc::new(pool);

        info!("Memory database initialized successfully");

        Ok(Self {
            conversations: ConversationStore::new(Arc::clone(&pool)),
            summaries: SummaryStore::new(Arc::clone(&pool)),
            embeddings: EmbeddingStore::new(Arc::clone(&pool)),
            pool,
        })
    }

    /// Create an in-memory database (useful for testing)
    pub fn new_in_memory() -> anyhow::Result<Self> {
        let manager = SqliteConnectionManager::memory();
        let pool = Pool::builder()
            .max_size(5)
            .build(manager)?;

        {
            let conn = pool.get()?;
            conn.execute_batch(schema::SCHEMA_SQL)?;
        }

        let pool = Arc::new(pool);

        Ok(Self {
            conversations: ConversationStore::new(Arc::clone(&pool)),
            summaries: SummaryStore::new(Arc::clone(&pool)),
            embeddings: EmbeddingStore::new(Arc::clone(&pool)),
            pool,
        })
    }

    /// Begin a transaction for atomic operations
    pub fn begin_transaction(&self) -> anyhow::Result<Transaction<'_>> {
        let conn = self.pool.get()?;
        conn.execute_batch("BEGIN IMMEDIATE TRANSACTION;")?;
        Ok(Transaction {
            conn,
            _marker: std::marker::PhantomData,
        })
    }

    /// Execute operations in a transaction
    pub fn with_transaction<T, F>(&self, f: F) -> anyhow::Result<T>
    where
        F: FnOnce(&mut Transaction<'_>) -> anyhow::Result<T>,
    {
        let mut tx = self.begin_transaction()?;
        match f(&mut tx) {
            Ok(result) => {
                tx.commit()?;
                Ok(result)
            }
            Err(e) => {
                tx.rollback()?;
                Err(e)
            }
        }
    }

    /// Get database statistics
    pub fn get_stats(&self) -> anyhow::Result<DatabaseStats> {
        let conn = self.pool.get()?;
        Ok(migration::get_database_stats(&conn)?)
    }

    /// Cleanup old data (older than specified days)
    pub fn cleanup_old_data(&self, older_than_days: i32) -> anyhow::Result<usize> {
        let mut conn = self.pool.get()?;
        let mut migrator = migration::MigrationManager::new(&mut conn);
        Ok(migrator.cleanup_old_data(older_than_days)?)
    }

    /// Create a KV snapshot
    pub async fn create_kv_snapshot(
        &self,
        session_id: &str,
        entries: &[KVEntry],
    ) -> anyhow::Result<i64> {
        use blake3;

        let mut conn = self.pool.get()?;  // FIXED: Added mut
        let tx = conn.transaction()?;
        
        // Calculate total size
        let total_size_bytes: usize = entries.iter()
            .map(|entry| entry.value_data.len())
            .sum();
        
        // Serialize entries to BLOB
        let kv_state = bincode::serialize(entries)?;
        let kv_state_hash = blake3::hash(&kv_state).to_string();
        
        // Get the latest message ID for this session
        let message_id: i64 = tx.query_row(
            "SELECT COALESCE(MAX(id), 0) FROM messages WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )?;
        
        // Insert snapshot
        tx.execute(
            "INSERT INTO kv_snapshots 
             (session_id, message_id, kv_state, kv_state_hash, size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![session_id, message_id, kv_state, kv_state_hash, total_size_bytes as i64],
        )?;
        
        let snapshot_id = tx.last_insert_rowid();
        
        // Insert individual cache entries
        for entry in entries {
            tx.execute(
                "INSERT INTO kv_cache_entries 
                 (snapshot_id, key_hash, key_data, value_data, key_type, 
                  layer_index, head_index, importance_score, access_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    snapshot_id,
                    &entry.key_hash,
                    entry.key_data.as_deref(),
                    &entry.value_data,
                    &entry.key_type,
                    entry.layer_index,
                    entry.head_index,
                    entry.importance_score,
                    entry.access_count,
                ],
            )?;
        }
        
        // Update metadata
        let now = chrono::Utc::now().to_rfc3339();
        tx.execute(
            "INSERT OR REPLACE INTO kv_cache_metadata 
             (session_id, total_entries, total_size_bytes, last_cleared_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![session_id, entries.len() as i64, total_size_bytes as i64, &now],
        )?;
        
        tx.commit()?;
        
        Ok(snapshot_id)
    }
    
    /// Get recent KV snapshots for a session
    pub async fn get_recent_kv_snapshots(
        &self,
        session_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<crate::cache_management::cache_manager::KvSnapshot>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, message_id, snapshot_type, size_bytes, created_at
             FROM kv_snapshots 
             WHERE session_id = ?1 
             ORDER BY created_at DESC 
             LIMIT ?2"
        )?;
        
        let mut rows = stmt.query(rusqlite::params![session_id, limit as i64])?;
        let mut snapshots = Vec::new();
        
        while let Some(row) = rows.next()? {
            let created_at_str: String = row.get(5)?;
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| anyhow::anyhow!("Failed to parse timestamp: {}", e))?
                .with_timezone(&chrono::Utc);
            
            snapshots.push(crate::cache_management::cache_manager::KvSnapshot {
                id: row.get(0)?,
                session_id: row.get(1)?,
                message_id: row.get(2)?,
                snapshot_type: row.get(3)?,
                size_bytes: row.get(4)?,
                created_at,
            });
        }
        
        Ok(snapshots)
    }
    
    /// Get KV snapshot entries
    pub async fn get_kv_snapshot_entries(
        &self,
        snapshot_id: i64,
    ) -> anyhow::Result<Vec<KVEntry>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT key_hash, key_data, value_data, key_type, layer_index, 
                    head_index, importance_score, access_count, last_accessed
             FROM kv_cache_entries 
             WHERE snapshot_id = ?1"
        )?;
        
        let mut rows = stmt.query([snapshot_id])?;
        let mut entries = Vec::new();
        
        while let Some(row) = rows.next()? {
            let last_accessed_str: String = row.get(8)?;
            let last_accessed = chrono::DateTime::parse_from_rfc3339(&last_accessed_str)
                .map_err(|e| anyhow::anyhow!("Failed to parse timestamp: {}", e))?
                .with_timezone(&chrono::Utc);
            
            entries.push(KVEntry {
                key_hash: row.get(0)?,
                key_data: row.get(1)?,
                value_data: row.get(2)?,
                key_type: row.get(3)?,
                layer_index: row.get(4)?,
                head_index: row.get(5)?,
                importance_score: row.get(6)?,
                access_count: row.get(7)?,
                last_accessed,
            });
        }
        
        Ok(entries)
    }
    
    /// Search messages by keywords (for ConversationStore)
    pub async fn search_messages_by_keywords(
        &self,
        session_id: &str,
        keywords: &[String],
        limit: usize,
    ) -> anyhow::Result<Vec<StoredMessage>> {
        // Simple keyword search using LIKE pattern
        let patterns: Vec<String> = keywords.iter()
            .map(|k| format!("%{}%", k))
            .collect();
        
        let conn = self.pool.get()?;
        
        // Build query with multiple LIKE conditions
        let mut query = String::from(
            "SELECT id, session_id, message_index, role, content, tokens, 
                    timestamp, importance_score, embedding_generated
             FROM messages 
             WHERE session_id = ?1"
        );
        
        for _ in &patterns {
            query.push_str(" AND content LIKE ?");
        }
        
        query.push_str(" ORDER BY timestamp DESC LIMIT ?");
        
        let mut stmt = conn.prepare(&query)?;
        
        // Build parameters: session_id + patterns + limit
        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
        params.push(&session_id);
        for pattern in &patterns {
            params.push(pattern);
        }
        // FIX: Store in variable to avoid temporary reference
        let limit_i64 = limit as i64;
        params.push(&limit_i64);
        
        let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
        let mut messages = Vec::new();
        
        while let Some(row) = rows.next()? {
            let timestamp_str: String = row.get(6)?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(&timestamp_str)
                .map_err(|e| anyhow::anyhow!("Failed to parse timestamp: {}", e))?
                .with_timezone(&chrono::Utc);
            
            messages.push(StoredMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                message_index: row.get(2)?,
                role: row.get(3)?,
                content: row.get(4)?,
                tokens: row.get(5)?,
                timestamp,
                importance_score: row.get(7)?,
                embedding_generated: row.get(8)?,
            });
        }
        
        Ok(messages)
    }
    
    /// Update KV cache metadata
    pub async fn update_kv_cache_metadata(
        &self,
        session_id: &str,
        state: &SessionCacheState,
    ) -> anyhow::Result<()> {
        let conn = self.pool.get()?;
        let metadata_json = serde_json::to_string(&state.metadata)?;
        
        conn.execute(
            "INSERT OR REPLACE INTO kv_cache_metadata 
             (session_id, total_entries, total_size_bytes, conversation_count, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                session_id,
                state.entry_count as i64,
                state.cache_size_bytes as i64,
                state.conversation_count as i64,
                metadata_json,
            ],
        )?;
        
        Ok(())
    }
    
    /// Cleanup session snapshots
    pub async fn cleanup_session_snapshots(
        &self,
        session_id: &str,
    ) -> anyhow::Result<()> {
        let conn = self.pool.get()?;
        
        conn.execute(
            "DELETE FROM kv_snapshots WHERE session_id = ?1",
            [session_id],
        )?;
        
        conn.execute(
            "DELETE FROM kv_cache_metadata WHERE session_id = ?1",
            [session_id],
        )?;
        
        Ok(())
    }
    
    /// Prune old KV snapshots
    pub async fn prune_old_kv_snapshots(
        &self,
        keep_max: usize,
    ) -> anyhow::Result<usize> {
        let conn = self.pool.get()?;
        
        // Get snapshot IDs to delete (keep only the latest keep_max per session)
        let mut stmt = conn.prepare(
            "SELECT ks.id 
             FROM kv_snapshots ks
             WHERE (
                 SELECT COUNT(*) 
                 FROM kv_snapshots ks2 
                 WHERE ks2.session_id = ks.session_id 
                 AND ks2.created_at >= ks.created_at
             ) > ?1"
        )?;
        
        let ids_to_delete: Vec<i64> = stmt
            .query_map([keep_max as i64], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        
        if ids_to_delete.is_empty() {
            return Ok(0);
        }
        
        // Delete snapshots
        let placeholders = vec!["?"; ids_to_delete.len()].join(",");
        let query = format!("DELETE FROM kv_snapshots WHERE id IN ({})", placeholders);
        
        let mut stmt = conn.prepare(&query)?;
        let deleted = stmt.execute(rusqlite::params_from_iter(&ids_to_delete))?;
        
        Ok(deleted)
    }
}

impl Drop for MemoryDatabase {
    fn drop(&mut self) {
        // Perform a final checkpoint on shutdown
        if let Ok(conn) = self.pool.get() {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
    }
}
