// src/cache_management/mod.rs

//! KV Cache management system for efficient context preservation and retrieval

pub mod cache_bridge;
pub mod cache_config;
pub mod cache_extractor;
pub mod cache_manager;
pub mod cache_scorer;

// Re-exports
pub use cache_bridge::{CacheContextBridge, CacheBridgeStats, CacheTransition, TransitionType};
pub use cache_config::{KVCacheConfig, RetrievalStrategy, SnapshotStrategy, CachePreservationConfig};
pub use cache_extractor::{CacheExtractor, CacheExtractorConfig, ExtractedCacheEntry, CacheEntryType, KVEntry};
pub use cache_manager::{
    KVCacheManager, SessionCacheState, CacheStatistics, CacheOperation, CacheOperationType,
    ClearReason, CacheClearResult, RetrievalResult, RetrievedEntry, CacheProcessingResult,
    CacheStatisticsExport, MaintenanceResult
};
pub use cache_scorer::{CacheEntryScorer, CacheScoringConfig};

/// Create a default KV cache manager
pub fn create_default_cache_manager(
    config: KVCacheConfig,
    database: std::sync::Arc<crate::memory_db::MemoryDatabase>,
) -> anyhow::Result<KVCacheManager> {
    KVCacheManager::new(config, database)
}