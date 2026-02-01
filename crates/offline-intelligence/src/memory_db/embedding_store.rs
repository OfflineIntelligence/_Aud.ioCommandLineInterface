// "D:\_ProjectWorks\AUDIO_Interface\Server\src\memory_db\embedding_store.rs"
//! Embedding storage and retrieval operations with ANN indexing support

use crate::memory_db::schema::*;
use rusqlite::{params, Result, Row};
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use tracing::{info, warn};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use hora::core::ann_index::ANNIndex;
use hora::core::metrics::Metric;
use hora::index::hnsw_idx::HNSWIndex;
use hora::index::hnsw_params::HNSWParams; 

/// Manages embedding storage and retrieval with HNSW-based ANN indexing
#[derive(Debug, Clone, serde::Serialize)]
pub struct EmbeddingStats {
    pub total_embeddings: usize,
    pub dimension: usize,
    pub index_type: String,
}

pub struct EmbeddingStore {
    pool: Arc<Pool<SqliteConnectionManager>>,
    // ANN index for fast similarity search
    ann_index: RwLock<Option<HNSWIndex<f32, i64>>>,
    // In-memory cache for linear search fallbacks
    embedding_cache: RwLock<HashMap<i64, Vec<f32>>>,
}

impl EmbeddingStore {
    pub fn new(pool: Arc<Pool<SqliteConnectionManager>>) -> Self {
        Self {
            pool,
            ann_index: RwLock::new(None),
            embedding_cache: RwLock::new(HashMap::new()),
        }
    }

    fn get_conn(&self) -> anyhow::Result<r2d2::PooledConnection<SqliteConnectionManager>> {
        self.pool.get().map_err(|e| anyhow::anyhow!("Failed to get connection from pool: {}", e))
    }

    pub fn initialize_index(&self, model: &str) -> anyhow::Result<()> {
        let conn = self.get_conn()?;
        
        let mut stmt = conn.prepare(
            "SELECT id, message_id, embedding FROM embeddings WHERE embedding_model = ?1"
        )?;
        
        let mut rows = stmt.query([model])?;
        
        // Fix: Use correct field names for HNSWParams based on error message
        let params = HNSWParams {
            // According to error: n_neighbor is likely the equivalent of 'm' in other implementations
            n_neighbor: 16,
            // According to error: ef_build is likely the equivalent of 'ef_construction'
            ef_build: 100,
            // According to error: ef_search should be available
            ef_search: 50,
            ..Default::default()
        };
        
        // Initialize HNSW index with dimension and a reference to params
        let mut index = HNSWIndex::<f32, i64>::new(
            384, // dimension
            &params,
        );
        
        let mut cache = self.embedding_cache.write().unwrap();
        
        while let Some(row) = rows.next()? {
            let message_id: i64 = row.get(1)?;
            let embedding_bytes: Vec<u8> = row.get(2)?;
            let embedding: Vec<f32> = bincode::deserialize(&embedding_bytes)
                .map_err(|e| anyhow::anyhow!("Deserialization error: {}", e))?;
            
            // Ignore duplicate insert errors while rebuilding the ANN index
            let _ = index.add(&embedding, message_id);
            cache.insert(message_id, embedding);
        }
        
        // Build the index with the Metric
        index.build(Metric::CosineSimilarity)
            .map_err(|e| anyhow::anyhow!("Failed to build index: {}", e))?;
        
        *self.ann_index.write().unwrap() = Some(index);
        info!("ANN index initialized with {} embeddings", cache.len());
        Ok(())
    }

    pub fn store_embedding(&self, embedding: &Embedding) -> anyhow::Result<()> {
        let embedding_bytes = bincode::serialize(&embedding.embedding)?;
        let conn = self.get_conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO embeddings (message_id, embedding, embedding_model, generated_at) VALUES (?1, ?2, ?3, ?4)",
            params![embedding.message_id, embedding_bytes, &embedding.embedding_model, embedding.generated_at.to_rfc3339()],
        )?;

        let mut cache = self.embedding_cache.write().unwrap();
        cache.insert(embedding.message_id, embedding.embedding.clone());

        if let Some(ref mut index) = *self.ann_index.write().unwrap() {
            // Ignore add errors; rebuild ensures index stays consistent
            let _ = index.add(&embedding.embedding, embedding.message_id);
            // Re-building after every add is expensive, but necessary for HNSW 
            // if you want immediate searchability of the new item.
            index.build(Metric::CosineSimilarity)
                .map_err(|e| anyhow::anyhow!("Failed to rebuild index: {}", e))?;
        }
        Ok(())
    }

    pub fn find_similar_embeddings(
        &self,
        query_embedding: &[f32],
        model: &str,
        limit: i32,
        similarity_threshold: f32,
    ) -> anyhow::Result<Vec<(i64, f32)>> {
        if model.is_empty() || model.len() > 100 {
            return Err(anyhow::anyhow!("Invalid model name"));
        }

        {
            let index_guard = self.ann_index.read().unwrap();
            if let Some(index) = &*index_guard {
                let results = index.search(query_embedding, limit as usize);
                
                let mut scored_results = Vec::new();
                for id in &results {
                    if let Some(embedding) = self.embedding_cache.read().unwrap().get(id) {
                        let sim = cosine_similarity(query_embedding, embedding);
                        if sim >= similarity_threshold {
                            scored_results.push((*id, sim));
                        }
                    }
                }
                
                scored_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                return Ok(scored_results);
            }
        }

        warn!("ANN index not available, falling back to safe linear search");
        self.find_similar_embeddings_linear(query_embedding, model, limit, similarity_threshold)
    }

    fn find_similar_embeddings_linear(
        &self,
        query_embedding: &[f32],
        model: &str,
        limit: i32,
        similarity_threshold: f32,
    ) -> anyhow::Result<Vec<(i64, f32)>> {
        let conn = self.get_conn()?;
        let mut stmt = conn.prepare(
            "SELECT message_id, embedding FROM embeddings WHERE embedding_model = ?1"
        )?;
        let mut rows = stmt.query([model])?;
        
        let mut matches = Vec::new();
        while let Some(row) = rows.next()? {
            let message_id: i64 = row.get(0)?;
            let embedding_bytes: Vec<u8> = row.get(1)?;
            let embedding: Vec<f32> = bincode::deserialize(&embedding_bytes)
                .map_err(|e| anyhow::anyhow!("Bincode error: {}", e))?;
            
            let sim = cosine_similarity(query_embedding, &embedding);
            if sim >= similarity_threshold {
                matches.push((message_id, sim));
            }
        }
        
        matches.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        matches.truncate(limit as usize);
        Ok(matches)
    }

    pub fn get_embedding_by_message_id(&self, message_id: i64, model: &str) -> anyhow::Result<Option<Embedding>> {
        let conn = self.get_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, message_id, embedding, embedding_model, generated_at
             FROM embeddings WHERE message_id = ?1 AND embedding_model = ?2"
        )?;
        
        let mut rows = stmt.query(params![message_id, model])?;
        if let Some(row) = rows.next()? {
            Ok(Some(self.row_to_embedding(row)?))
        } else {
            Ok(None)
        }
    }

    fn row_to_embedding(&self, row: &Row) -> Result<Embedding> {
        let embedding_bytes: Vec<u8> = row.get(2)?;
        let embedding: Vec<f32> = bincode::deserialize(&embedding_bytes)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        
        let generated_at_str: String = row.get(4)?;
        let generated_at = chrono::DateTime::parse_from_rfc3339(&generated_at_str)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?
            .with_timezone(&chrono::Utc);
        
        Ok(Embedding {
            id: row.get(0)?,
            message_id: row.get(1)?,
            embedding,
            embedding_model: row.get(3)?,
            generated_at,
        })
    }

    pub fn get_stats(&self) -> anyhow::Result<EmbeddingStats> {
        let conn = self.get_conn()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM embeddings",
            [],
            |row| row.get(0)
        )?;
        
        let mut stmt = conn.prepare("SELECT embedding FROM embeddings LIMIT 1")?;
        let dimension = if let Some(row) = stmt.query([])?.next()? {
            let embedding_bytes: Vec<u8> = row.get(0)?;
            let embedding: Vec<f32> = bincode::deserialize(&embedding_bytes)
                .map_err(|e| anyhow::anyhow!("Deserialization error: {}", e))?;
            embedding.len()
        } else {
            0
        };
        
        let index_type = if self.ann_index.read().unwrap().is_some() {
            "HNSW".to_string()
        } else {
            "Linear".to_string()
        };
        
        Ok(EmbeddingStats {
            total_embeddings: count as usize,
            dimension,
            index_type,
        })
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() { return 0.0; }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot / (norm_a * norm_b) }
}