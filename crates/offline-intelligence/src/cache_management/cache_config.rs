//! Configuration for the KV cache management system

use serde::{Deserialize, Serialize};

/// Configuration for the KV cache management system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KVCacheConfig {
    /// Whether cache management is enabled
    pub enabled: bool,
    
    /// Whether retrieval is enabled
    pub retrieval_enabled: bool,
    
    /// Number of conversations before clearing cache
    pub clear_after_conversations: usize,
    
    /// Memory threshold percentage (0.6 = 60%) for clearing
    pub memory_threshold_percent: f32,
    
    /// Whether to create bridging sentences between cached and retrieved content
    pub bridge_enabled: bool,
    
    /// Maximum entries to keep in KV cache after clearing
    pub max_cache_entries: usize,
    
    /// Minimum importance score to preserve entries during clearing
    pub min_importance_to_preserve: f32,
    
    /// Whether to generate embeddings for cache retrieval
    pub generate_cache_embeddings: bool,
    
    /// Retrieval strategy to use
    pub retrieval_strategy: RetrievalStrategy,
    
    /// Whether to preserve system prompts in cache
    pub preserve_system_prompts: bool,
    
    /// Whether to preserve code-related KV entries
    pub preserve_code_entries: bool,
    
    /// Snapshot strategy to use
    pub snapshot_strategy: SnapshotStrategy,
}

impl Default for KVCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retrieval_enabled: true,
            clear_after_conversations: 16,  // Clear after 16 conversations
            memory_threshold_percent: 0.6,  // 60% memory usage
            bridge_enabled: true,
            max_cache_entries: 1000,
            min_importance_to_preserve: 0.7,
            generate_cache_embeddings: true,
            retrieval_strategy: RetrievalStrategy::KeywordThenSemantic,
            preserve_system_prompts: true,
            preserve_code_entries: true,
            snapshot_strategy: SnapshotStrategy::Incremental {
                interval_conversations: 4,  // Snapshot every 4 conversations
                max_snapshots: 4,           // Keep last 4 snapshots
            },
        }
    }
}

/// Different retrieval strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RetrievalStrategy {
    /// Keyword matching only
    KeywordOnly,
    /// Semantic search only
    SemanticOnly,
    /// Keyword then semantic as fallback
    KeywordThenSemantic,
    /// Semantic then keyword as fallback  
    SemanticThenKeyword,
    /// Hybrid approach
    Hybrid {
        keyword_weight: f32,
        semantic_weight: f32,
    },
}

/// Different snapshot strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SnapshotStrategy {
    /// No snapshots
    None,
    /// Full snapshot every N conversations
    Full {
        interval_conversations: usize,
    },
    /// Incremental snapshots
    Incremental {
        interval_conversations: usize,
        max_snapshots: usize,
    },
    /// Adaptive based on importance
    Adaptive {
        min_importance_threshold: f32,
        max_snapshots: usize,
    },
}

/// Configuration for cache entry preservation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachePreservationConfig {
    /// Preserve attention keys
    pub preserve_attention_keys: bool,
    
    /// Preserve attention values
    pub preserve_attention_values: bool,
    
    /// Preserve FFN keys
    pub preserve_ffn_keys: bool,
    
    /// Preserve FFN values
    pub preserve_ffn_values: bool,
    
    /// Preserve entries from early layers
    pub preserve_early_layers: bool,
    
    /// Preserve entries from late layers
    pub preserve_late_layers: bool,
    
    /// Custom patterns to preserve (regex for key matching)
    pub custom_patterns: Vec<String>,
}

impl Default for CachePreservationConfig {
    fn default() -> Self {
        Self {
            preserve_attention_keys: true,
            preserve_attention_values: true,
            preserve_ffn_keys: false,
            preserve_ffn_values: false,
            preserve_early_layers: true,
            preserve_late_layers: false,
            custom_patterns: Vec::new(),
        }
    }
}