// "D:\_ProjectWorks\AUDIO_Interface\Server\src\memory_db\schema.rs"
//! Database schema definitions for the memory system

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Represents a conversation session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub metadata: SessionMetadata,
}

// Chat persistence: Session metadata with serde defaults for backward compatibility with existing database records
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionMetadata {
    pub title: Option<String>,
    #[serde(default)]  // Handle old records missing this field
    pub tags: Vec<String>,
    #[serde(default)]  // Handle old records missing this field
    pub user_defined: HashMap<String, String>,
    #[serde(default)]  // Handle old records missing this field
    pub pinned: bool,
}

/// Represents a single message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: i64,
    pub session_id: String,
    pub message_index: i32,
    pub role: String,
    pub content: String,
    pub tokens: i32,
    pub timestamp: DateTime<Utc>,
    pub importance_score: f32,
    pub embedding_generated: bool,
}

/// Represents a conversation summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: i64,
    pub session_id: String,
    pub message_range_start: i32,
    pub message_range_end: i32,
    pub summary_text: String,
    pub compression_ratio: f32,
    pub key_topics: Vec<String>,
    pub generated_at: DateTime<Utc>,
}

/// Represents a preserved detail from a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detail {
    pub id: i64,
    pub session_id: String,
    pub message_id: i64,
    pub detail_type: String,
    pub content: String,
    pub context: String,
    pub importance_score: f32,
    pub accessed_count: i32,
    pub last_accessed: DateTime<Utc>,
}

/// Represents an embedding vector for semantic search
#[derive(Debug, Clone)]
pub struct Embedding {
    pub id: i64,
    pub message_id: i64,
    pub embedding: Vec<f32>,
    pub embedding_model: String,
    pub generated_at: DateTime<Utc>,
}

/// Represents KV snapshot for cache management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvSnapshot {
    pub id: i64,
    pub session_id: String,
    pub message_id: i64,
    pub snapshot_type: String,
    pub kv_state: Vec<u8>,
    pub kv_state_hash: String,
    pub access_pattern: Option<String>,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
}

/// Represents search result
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub message: StoredMessage,
    pub similarity_score: f32,
    pub source: SearchSource,
}

#[derive(Debug, Clone)]
pub enum SearchSource {
    Semantic,
    Keyword,
    Hybrid,
}

/// Database statistics
#[derive(Debug, Clone)]
pub struct DatabaseStats {
    pub total_sessions: i64,
    pub total_messages: i64,
    pub total_summaries: i64,
    pub total_details: i64,
    pub total_embeddings: i64,
    pub database_size_bytes: i64,
}

/// SQL statements for table creation
pub const SCHEMA_SQL: &str = "
-- Sessions table
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL,
    last_accessed TIMESTAMP NOT NULL,
    metadata TEXT NOT NULL
);

-- Messages table
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    message_index INTEGER NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    tokens INTEGER NOT NULL,
    timestamp TIMESTAMP NOT NULL,
    importance_score REAL NOT NULL DEFAULT 0.5,
    embedding_generated BOOLEAN NOT NULL DEFAULT FALSE,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    UNIQUE(session_id, message_index)
);

-- Summaries table
CREATE TABLE IF NOT EXISTS summaries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    message_range_start INTEGER NOT NULL,
    message_range_end INTEGER NOT NULL,
    summary_text TEXT NOT NULL,
    compression_ratio REAL NOT NULL,
    key_topics TEXT NOT NULL,
    generated_at TIMESTAMP NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    UNIQUE(session_id, message_range_start, message_range_end)
);

-- Details table
CREATE TABLE IF NOT EXISTS details (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    message_id INTEGER NOT NULL,
    detail_type TEXT NOT NULL,
    content TEXT NOT NULL,
    context TEXT NOT NULL,
    importance_score REAL NOT NULL,
    accessed_count INTEGER NOT NULL DEFAULT 0,
    last_accessed TIMESTAMP NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
);

-- Embeddings table
CREATE TABLE IF NOT EXISTS embeddings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL,
    embedding BLOB NOT NULL,
    embedding_model TEXT NOT NULL,
    generated_at TIMESTAMP NOT NULL,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE,
    UNIQUE(message_id, embedding_model)
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages (session_id);
CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages (timestamp);
CREATE INDEX IF NOT EXISTS idx_summaries_session ON summaries (session_id);
CREATE INDEX IF NOT EXISTS idx_details_session ON details (session_id);
CREATE INDEX IF NOT EXISTS idx_details_type ON details (detail_type);
CREATE INDEX IF NOT EXISTS idx_embeddings_message ON embeddings (message_id);
";