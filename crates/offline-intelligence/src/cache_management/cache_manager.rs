//! Main KV cache management engine

use crate::memory::Message;
use crate::memory_db::MemoryDatabase;
use crate::cache_management::cache_config::{KVCacheConfig, SnapshotStrategy};
use crate::cache_management::cache_extractor::{CacheExtractor, ExtractedCacheEntry, KVEntry};
use crate::cache_management::cache_scorer::{CacheEntryScorer, CacheScoringConfig};
use crate::cache_management::cache_bridge::CacheContextBridge;

use std::sync::Arc;
use std::collections::HashMap;
use tracing::{info, debug};
use chrono::{Utc, DateTime};
use serde::Serialize;

/// Main KV cache management engine
pub struct KVCacheManager {
    config: KVCacheConfig,
    database: Arc<MemoryDatabase>,
    cache_extractor: CacheExtractor,
    cache_scorer: CacheEntryScorer,
    context_bridge: CacheContextBridge,
    statistics: CacheStatistics,
    session_state: HashMap<String, SessionCacheState>,
}

#[derive(Debug, Clone)]
pub struct KvSnapshot {
    pub id: i64,
    pub session_id: String,
    pub message_id: i64,
    pub snapshot_type: String,
    pub size_bytes: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionCacheState {
    pub session_id: String,
    pub conversation_count: usize,
    pub last_cleared_at: Option<DateTime<Utc>>,
    pub last_snapshot_id: Option<i64>,
    pub cache_size_bytes: usize,
    pub entry_count: usize,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CacheStatistics {
    pub total_clears: usize,
    pub total_retrievals: usize,
    pub entries_preserved: usize,
    pub entries_cleared: usize,
    pub entries_retrieved: usize,
    pub last_operation: Option<DateTime<Utc>>,
    pub operation_history: Vec<CacheOperation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheOperation {
    pub operation_type: CacheOperationType,
    pub timestamp: DateTime<Utc>,
    pub entries_affected: usize,
    pub session_id: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum CacheOperationType {
    Clear,
    Retrieve,
    Snapshot,
    Restore,
}

#[derive(Debug, Clone, Serialize)]
pub enum ClearReason {
    ConversationLimit,
    MemoryThreshold,
    Manual,
    ErrorRecovery,
}

#[derive(Debug, Clone)]
pub struct CacheClearResult {
    pub entries_to_keep: Vec<ExtractedCacheEntry>,
    pub entries_cleared: usize,
    pub bridge_message: String,
    pub snapshot_id: Option<i64>,
    pub preserved_keywords: Vec<String>,
    pub clear_reason: ClearReason,
}

#[derive(Debug, Clone, Default)]
pub struct RetrievalResult {
    pub retrieved_entries: Vec<RetrievedEntry>,
    pub bridge_message: Option<String>,
    pub search_duration_ms: u64,
    pub keywords_used: Vec<String>,
    pub tiers_searched: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct RetrievedEntry {
    pub entry: KVEntry,
    pub similarity_score: f32,
    pub source_tier: u8,
    pub matched_keywords: Vec<String>,
    pub retrieval_time: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CacheProcessingResult {
    pub should_clear_cache: bool,
    pub clear_result: Option<CacheClearResult>,
    pub should_retrieve: bool,
    pub retrieval_result: Option<RetrievalResult>,
    pub bridge_messages: Vec<String>,
    pub updated_session_state: SessionCacheState,
}

impl KVCacheManager {
    /// Create a new KV cache manager
    pub fn new(
        config: KVCacheConfig,
        database: Arc<MemoryDatabase>,
    ) -> anyhow::Result<Self> {
        let cache_extractor = CacheExtractor::new(Default::default());
        
        let scoring_config = CacheScoringConfig::default();
        let cache_scorer = CacheEntryScorer::new(scoring_config);
        
        let context_bridge = CacheContextBridge::new(20);
        
        Ok(Self {
            config,
            database,
            cache_extractor,
            cache_scorer,
            context_bridge,
            statistics: CacheStatistics::new(),
            session_state: HashMap::new(),
        })
    }
    
    /// Initialize or get session state
    fn get_or_create_session_state(&mut self, session_id: &str) -> &mut SessionCacheState {
        self.session_state.entry(session_id.to_string())
            .or_insert_with(|| SessionCacheState {
                session_id: session_id.to_string(),
                conversation_count: 0,
                last_cleared_at: None,
                last_snapshot_id: None,
                cache_size_bytes: 0,
                entry_count: 0,
                metadata: HashMap::new(),
            })
    }
    
    /// Process a conversation and manage cache
    pub async fn process_conversation(
        &mut self,
        session_id: &str,
        messages: &[Message],
        current_kv_entries: &[KVEntry],
        current_cache_size_bytes: usize,
        max_cache_size_bytes: usize,
    ) -> anyhow::Result<CacheProcessingResult> {
        debug!("Processing conversation for session: {}", session_id);
        
        // First, check conditions without mutable borrow
        let current_conversation_count = self.session_state
            .get(session_id)
            .map(|s| s.conversation_count)
            .unwrap_or(0);
        
        let should_clear_by_conversation = self.should_clear_by_conversation(current_conversation_count + 1);
        let should_clear_by_memory = self.should_clear_by_memory(current_cache_size_bytes, max_cache_size_bytes);
        
        // Now get mutable reference
        let session_state = self.get_or_create_session_state(session_id);
        session_state.conversation_count += 1;
        session_state.cache_size_bytes = current_cache_size_bytes;
        session_state.entry_count = current_kv_entries.len();
        
        let mut result = CacheProcessingResult {
            should_clear_cache: false,
            clear_result: None,
            should_retrieve: false,
            retrieval_result: None,
            bridge_messages: Vec::new(),
            updated_session_state: session_state.clone(),
        };
        
        if should_clear_by_conversation || should_clear_by_memory {
            let clear_reason = if should_clear_by_conversation {
                ClearReason::ConversationLimit
            } else {
                ClearReason::MemoryThreshold
            };
            
            // Release the mutable borrow before calling clear_cache
            let _ = session_state;
            
            let clear_result = self.clear_cache(session_id, current_kv_entries, clear_reason).await?;
            result.should_clear_cache = true;
            result.clear_result = Some(clear_result.clone());
            result.bridge_messages.push(clear_result.bridge_message);
            
            // Update session state after clearing
            if let Some(state) = self.session_state.get_mut(session_id) {
                state.conversation_count = 0;
                state.last_cleared_at = Some(Utc::now());
                result.updated_session_state = state.clone();
            }
        }
        
        // Check if we should retrieve context
        let should_retrieve = self.should_retrieve_context(messages);
        if should_retrieve {
            let last_user_message = messages.iter()
                .rev()
                .find(|m| m.role == "user")
                .map(|m| &m.content)
                .map_or("", |v| v);
            
            if !last_user_message.is_empty() {
                let retrieval_result = self.retrieve_context(session_id, last_user_message, current_kv_entries).await?;
                if !retrieval_result.retrieved_entries.is_empty() {
                    result.should_retrieve = true;
                    result.retrieval_result = Some(retrieval_result.clone());
                    if let Some(bridge_msg) = &retrieval_result.bridge_message {
                        result.bridge_messages.push(bridge_msg.clone());
                    }
                }
            }
        }
        
        // Update database metadata
        if let Some(state) = self.session_state.get(session_id) {
            self.update_session_metadata(session_id, state).await?;
        }
        
        Ok(result)
    }
    
    /// Check if cache needs to be cleared based on conversation count
    pub fn should_clear_by_conversation(&self, conversation_count: usize) -> bool {
        conversation_count >= self.config.clear_after_conversations
    }
    
    /// Check if cache needs to be cleared based on memory usage
    pub fn should_clear_by_memory(&self, current_usage_bytes: usize, max_memory_bytes: usize) -> bool {
        if max_memory_bytes == 0 {
            return false;
        }
        
        let usage_percent = current_usage_bytes as f32 / max_memory_bytes as f32;
        usage_percent >= self.config.memory_threshold_percent
    }
    
    /// Check if we should retrieve context for current messages
    fn should_retrieve_context(&self, messages: &[Message]) -> bool {
        if !self.config.retrieval_enabled {
            return false;
        }
        
        // Check last user message for complex queries
        if let Some(last_user) = messages.iter().rev().find(|m| m.role == "user") {
            let content = &last_user.content;
            // Retrieve for questions, complex requests, or code
            content.contains('?') ||
            content.len() > 100 ||
            content.contains("```") ||
            content.contains("explain") ||
            content.contains("how to") ||
            content.contains("what is")
        } else {
            false
        }
    }
    
    /// Clear KV cache intelligently
    pub async fn clear_cache(
        &mut self,
        session_id: &str,
        current_entries: &[KVEntry],
        reason: ClearReason,
    ) -> anyhow::Result<CacheClearResult> {
        info!("Clearing KV cache for session {}: {:?}", session_id, reason);
        
        let start_time = std::time::Instant::now();
        
        // 1. Extract important entries
        let extracted = self.cache_extractor.extract_entries(current_entries, &self.cache_scorer);
        
        // 2. Filter entries to preserve
        let to_preserve = self.cache_extractor.filter_preserved_entries(
            &extracted,
            self.config.min_importance_to_preserve,
            self.config.preserve_system_prompts,
            self.config.preserve_code_entries,
        );
        
        // 3. Create snapshot if configured
        let snapshot_id = if self.should_create_snapshot(&reason) {
            Some(self.create_snapshot(session_id, &to_preserve).await?)
        } else {
            None
        };
        
        // 4. Extract keywords from preserved entries
        let preserved_keywords: Vec<String> = to_preserve.iter()
            .flat_map(|e| e.keywords.clone())
            .take(10)
            .collect();
        
        // 5. Generate bridge message
        let bridge_message = self.context_bridge.create_clear_bridge(
            current_entries.len().saturating_sub(to_preserve.len()),
            to_preserve.len(),
            &preserved_keywords,
        );
        
        // 6. Update statistics
        self.statistics.record_clear(
            current_entries.len(),
            to_preserve.len(),
            reason.clone(),
            session_id,
        );
        
        // 7. Update session state
        if let Some(state) = self.session_state.get_mut(session_id) {
            state.entry_count = to_preserve.len();
            state.last_snapshot_id = snapshot_id;
            state.last_cleared_at = Some(Utc::now());
            state.metadata.insert("last_clear_reason".to_string(), format!("{:?}", reason));
        }
        
        let duration = start_time.elapsed();
        debug!("Cache clear completed in {:?}", duration);
        
        Ok(CacheClearResult {
            entries_to_keep: to_preserve.clone(), // CLONE FIXED HERE
            entries_cleared: current_entries.len().saturating_sub(to_preserve.len()),
            bridge_message,
            snapshot_id,
            preserved_keywords,
            clear_reason: reason,
        })
    }
    
    /// Check if we should create a snapshot
    fn should_create_snapshot(&self, reason: &ClearReason) -> bool {
        if !self.config.enabled {
            return false;
        }
        
        match &self.config.snapshot_strategy {
            SnapshotStrategy::None => false,
            SnapshotStrategy::Full { interval_conversations: _ } => true,
            SnapshotStrategy::Incremental { interval_conversations: _, max_snapshots: _ } => {
                matches!(reason, ClearReason::ConversationLimit)
            }
            SnapshotStrategy::Adaptive { min_importance_threshold: _, max_snapshots: _ } => true,
        }
    }
    
    /// Create a snapshot of preserved entries
    async fn create_snapshot(
        &self,
        session_id: &str,
        preserved_entries: &[ExtractedCacheEntry],
    ) -> anyhow::Result<i64> {
        debug!("Creating KV snapshot for session: {}", session_id);
        
        // Convert to database format
        let db_entries: Vec<KVEntry> = preserved_entries.iter()
            .map(|entry| {
                KVEntry {
                    key_hash: entry.key_hash.clone(),
                    key_data: entry.key_data.clone(),
                    value_data: entry.value_data.clone(),
                    key_type: entry.entry_type.to_string(),
                    layer_index: entry.layer_index,
                    head_index: entry.head_index,
                    importance_score: entry.importance_score,
                    access_count: entry.access_count,
                    last_accessed: Utc::now(),
                }
            })
            .collect();
        
        // Store in database
        let snapshot_id = self.database.create_kv_snapshot(session_id, &db_entries).await?;
        
        info!("Created KV snapshot {} with {} entries", snapshot_id, db_entries.len());
        Ok(snapshot_id)
    }
    
    /// Retrieve relevant context from all tiers
    pub async fn retrieve_context(
        &mut self,
        session_id: &str,
        query: &str,
        current_cache_entries: &[KVEntry],
    ) -> anyhow::Result<RetrievalResult> {
        debug!("Retrieving context for query: {}", query);
        
        let start_time = std::time::Instant::now();
        let keywords = self.extract_keywords(query);
        
        let mut results = Vec::new();
        let mut searched_tiers = Vec::new();
        
        // Tier 1: Search active cache
        if !current_cache_entries.is_empty() {
            searched_tiers.push(1);
            let tier1_results = self.search_tier1(current_cache_entries, &keywords).await?;
            results.extend(tier1_results);
        }
        
        // Tier 2: Search KV snapshots if Tier 1 insufficient
        if results.len() < 5 {
            searched_tiers.push(2);
            let tier2_results = self.search_tier2(session_id, &keywords).await?;
            results.extend(tier2_results);
        }
        
        // Tier 3: Search complete messages if still insufficient
        if results.len() < 3 {
            searched_tiers.push(3);
            let tier3_results = self.search_tier3(session_id, &keywords).await?;
            results.extend(tier3_results);
        }
        
        // Sort all results by similarity score
        results.sort_by(|a, b| b.similarity_score.partial_cmp(&a.similarity_score)
            .unwrap_or(std::cmp::Ordering::Equal));
        
        // Limit total results
        results.truncate(20);
        
        // Update engagement scores for retrieved entries
        for result in &results {
            self.cache_scorer.update_engagement(&result.entry.key_hash, true);
        }
        
        // Generate bridge message if we found results
        let bridge_message = if !results.is_empty() {
            let primary_tier = results.iter()
                .map(|r| r.source_tier)
                .max()
                .unwrap_or(1);
            
            let avg_similarity = results.iter()
                .map(|r| r.similarity_score)
                .sum::<f32>() / results.len() as f32;
            
            Some(self.context_bridge.create_retrieval_bridge(
                results.len(),
                primary_tier,
                &keywords,
                Some(avg_similarity),
            ))
        } else {
            None
        };
        
        let duration = start_time.elapsed();
        
        // Update statistics
        self.statistics.record_retrieval(
            results.len(),
            searched_tiers.clone(),
            keywords.len(),
            session_id,
        );
        
        Ok(RetrievalResult {
            retrieved_entries: results,
            bridge_message,
            search_duration_ms: duration.as_millis() as u64,
            keywords_used: keywords,
            tiers_searched: searched_tiers,
        })
    }
    
    /// Search Tier 1 (Active KV cache)
    async fn search_tier1(
        &self,
        entries: &[KVEntry],
        keywords: &[String],
    ) -> anyhow::Result<Vec<RetrievedEntry>> {
        let mut results = Vec::new();
        
        for entry in entries {
            let similarity = self.calculate_keyword_similarity(entry, keywords);
            if similarity > 0.3 { // Threshold for Tier 1
                let matched_keywords = self.get_matching_keywords(entry, keywords);
                results.push(RetrievedEntry {
                    entry: entry.clone(),
                    similarity_score: similarity,
                    source_tier: 1,
                    matched_keywords,
                    retrieval_time: Utc::now(),
                });
            }
        }
        
        // Sort by similarity and access count
        results.sort_by(|a, b| {
            b.similarity_score.partial_cmp(&a.similarity_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.entry.access_count.cmp(&a.entry.access_count))
        });
        
        results.truncate(10); // Limit results
        
        debug!("Tier 1 search found {} results", results.len());
        Ok(results)
    }
    
    /// Search Tier 2 (KV snapshots)
    async fn search_tier2(
        &self,
        session_id: &str,
        keywords: &[String],
    ) -> anyhow::Result<Vec<RetrievedEntry>> {
        // Get recent snapshots (max 3 for performance)
        let snapshots = self.database.get_recent_kv_snapshots(session_id, 3).await?;
        
        let mut all_results = Vec::new();
        
        for snapshot in snapshots {
            // Search snapshot entries
            let entries = self.database.get_kv_snapshot_entries(snapshot.id).await?;
            
            for entry in entries {
                let similarity = self.calculate_keyword_similarity(&entry, keywords);
                if similarity > 0.4 { // Higher threshold for Tier 2
                    let matched_keywords = self.get_matching_keywords(&entry, keywords);
                    all_results.push(RetrievedEntry {
                        entry,
                        similarity_score: similarity,
                        source_tier: 2,
                        matched_keywords,
                        retrieval_time: Utc::now(),
                    });
                }
            }
        }
        
        // Sort and limit
        all_results.sort_by(|a, b| b.similarity_score.partial_cmp(&a.similarity_score)
            .unwrap_or(std::cmp::Ordering::Equal));
        all_results.truncate(15);
        
        debug!("Tier 2 search found {} results", all_results.len());
        Ok(all_results)
    }
    
    /// Search Tier 3 (Complete messages)
    async fn search_tier3(
        &self,
        session_id: &str,
        keywords: &[String],
    ) -> anyhow::Result<Vec<RetrievedEntry>> {
        if keywords.is_empty() {
            return Ok(Vec::new());
        }
        
        // Search messages by keywords
        let messages = self.database.conversations.search_messages_by_keywords(
            session_id,
            keywords,
            20,
        ).await?;
        
        let mut results = Vec::new();
        
        for message in messages {
            // Convert message to KV entry for consistency
            let entry = KVEntry {
                key_hash: format!("msg_{}", message.id),
                key_data: Some(message.content.as_bytes().to_vec()),
                value_data: message.content.as_bytes().to_vec(),
                key_type: "message".to_string(),
                layer_index: 0,
                head_index: None,
                importance_score: message.importance_score,
                access_count: 1,
                last_accessed: message.timestamp,
            };
            
            let similarity = self.calculate_keyword_similarity(&entry, keywords);
            if similarity > 0.5 { // Highest threshold for Tier 3
                results.push(RetrievedEntry {
                    entry,
                    similarity_score: similarity,
                    source_tier: 3,
                    matched_keywords: keywords.to_vec(),
                    retrieval_time: Utc::now(),
                });
            }
        }
        
        // Sort by timestamp (most recent first) and similarity
        results.sort_by(|a, b| {
            b.entry.last_accessed.cmp(&a.entry.last_accessed)
                .then(b.similarity_score.partial_cmp(&a.similarity_score)
                    .unwrap_or(std::cmp::Ordering::Equal))
        });
        
        results.truncate(10);
        
        debug!("Tier 3 search found {} results", results.len());
        Ok(results)
    }
    
    fn calculate_keyword_similarity(&self, entry: &KVEntry, keywords: &[String]) -> f32 {
        if keywords.is_empty() {
            return 0.0;
        }
        
        let entry_keywords = self.cache_scorer.extract_keywords(entry.key_data.as_deref());
        if entry_keywords.is_empty() {
            return 0.0;
        }
        
        // Simple keyword matching with partial matches
        let mut matches = 0.0;
        for keyword in keywords {
            let keyword_lower = keyword.to_lowercase();
            for entry_keyword in &entry_keywords {
                let entry_lower = entry_keyword.to_lowercase();
                if entry_lower.contains(&keyword_lower) || keyword_lower.contains(&entry_lower) {
                    matches += 1.0;
                    break;
                }
            }
        }
        
        matches / keywords.len() as f32
    }
    
    fn get_matching_keywords(&self, entry: &KVEntry, keywords: &[String]) -> Vec<String> {
        let entry_keywords = self.cache_scorer.extract_keywords(entry.key_data.as_deref());
        let mut matches = Vec::new();
        
        for keyword in keywords {
            let keyword_lower = keyword.to_lowercase();
            for entry_keyword in &entry_keywords {
                let entry_lower = entry_keyword.to_lowercase();
                if entry_lower.contains(&keyword_lower) || keyword_lower.contains(&entry_lower) {
                    matches.push(keyword.clone());
                    break;
                }
            }
        }
        
        matches
    }
    
    fn extract_keywords(&self, text: &str) -> Vec<String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        words.iter()
            .filter(|w| w.len() > 3)
            .map(|w| w.to_lowercase())
            .filter(|w| !self.is_stop_word(w))
            .collect()
    }
    
    fn is_stop_word(&self, word: &str) -> bool {
        let stop_words = [
            "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for",
            "of", "with", "by", "is", "am", "are", "was", "were", "be", "been",
            "being", "have", "has", "had", "do", "does", "did", "will", "would",
            "shall", "should", "may", "might", "must", "can", "could",
        ];
        stop_words.contains(&word)
    }
    
    /// Update session metadata in database
    async fn update_session_metadata(
        &self,
        session_id: &str,
        state: &SessionCacheState,
    ) -> anyhow::Result<()> {
        self.database.update_kv_cache_metadata(session_id, state).await
    }
    
    /// Get cache statistics
    pub fn get_statistics(&self) -> &CacheStatistics {
        &self.statistics
    }
    
    /// Get session state
    pub fn get_session_state(&self, session_id: &str) -> Option<&SessionCacheState> {
        self.session_state.get(session_id)
    }
    
    /// Get all session states
    pub fn get_all_session_states(&self) -> &HashMap<String, SessionCacheState> {
        &self.session_state
    }
    
    /// Restore cache from snapshot
    pub async fn restore_from_snapshot(
        &mut self,
        session_id: &str,
        snapshot_id: i64,
    ) -> anyhow::Result<Vec<KVEntry>> {
        info!("Restoring cache from snapshot {} for session {}", snapshot_id, session_id);
        
        let entries: Vec<KVEntry> = self.database.get_kv_snapshot_entries(snapshot_id).await?;
        
        // Update session state
        if let Some(state) = self.session_state.get_mut(session_id) {
            state.entry_count = entries.len();
            state.last_snapshot_id = Some(snapshot_id);
        }
        
        // Generate bridge message
        let bridge_message = self.context_bridge.create_restore_bridge(
            entries.len(),
            None, // Could calculate snapshot age if needed
        );
        
        info!("{}", bridge_message);
        
        // Update statistics
        self.statistics.record_restore(entries.len(), session_id);
        
        Ok(entries)
    }
    
    /// Manual cache clear (for testing or admin purposes)
    pub async fn manual_clear_cache(
        &mut self,
        session_id: &str,
        current_entries: &[KVEntry],
    ) -> anyhow::Result<CacheClearResult> {
        self.clear_cache(session_id, current_entries, ClearReason::Manual).await
    }
    
    /// Check cache health and perform maintenance if needed
    pub async fn perform_maintenance(&mut self) -> anyhow::Result<MaintenanceResult> {
        let mut result = MaintenanceResult {
            sessions_cleaned: 0,
            snapshots_pruned: 0,
            errors: Vec::new(),
        };
        
        // Clean up old session states (inactive for > 24 hours)
        let cutoff = Utc::now() - chrono::Duration::hours(24);
        let sessions_to_clean: Vec<String> = self.session_state.iter()
            .filter(|(_, state)| {
                state.last_cleared_at.is_none_or(|dt| dt < cutoff)
            })
            .map(|(id, _)| id.clone())
            .collect();
        
        for session_id in sessions_to_clean {
            if let Err(e) = self.cleanup_session(&session_id).await {
                result.errors.push(format!("Failed to cleanup session {}: {}", session_id, e));
            } else {
                result.sessions_cleaned += 1;
            }
        }
        
        // Prune old snapshots if configured
        if let SnapshotStrategy::Incremental { max_snapshots, .. } = &self.config.snapshot_strategy {
            let pruned = self.prune_old_snapshots(*max_snapshots).await?;
            result.snapshots_pruned = pruned;
        }
        
        Ok(result)
    }
    
    /// Cleanup a specific session
    async fn cleanup_session(&mut self, session_id: &str) -> anyhow::Result<()> {
        self.session_state.remove(session_id);
        self.database.cleanup_session_snapshots(session_id).await?;
        Ok(())
    }
    
    /// Prune old snapshots
    async fn prune_old_snapshots(&self, keep_max: usize) -> anyhow::Result<usize> {
        self.database.prune_old_kv_snapshots(keep_max).await
    }
    
    /// Export cache statistics
    pub fn export_statistics(&self) -> CacheStatisticsExport {
        CacheStatisticsExport {
            total_clears: self.statistics.total_clears,
            total_retrievals: self.statistics.total_retrievals,
            entries_preserved: self.statistics.entries_preserved,
            entries_cleared: self.statistics.entries_cleared,
            entries_retrieved: self.statistics.entries_retrieved,
            active_sessions: self.session_state.len(),
            last_operation: self.statistics.last_operation,
            operation_history_count: self.statistics.operation_history.len(),
        }
    }
    
    /// Get configuration
    pub fn get_config(&self) -> &KVCacheConfig {
        &self.config
    }
    
    /// Update configuration
    pub fn update_config(&mut self, config: KVCacheConfig) {
        self.config = config;
    }
    
    /// Get cache scorer reference
    pub fn cache_scorer(&self) -> &CacheEntryScorer {
        &self.cache_scorer
    }
    
    /// Get mutable cache scorer reference
    pub fn cache_scorer_mut(&mut self) -> &mut CacheEntryScorer {
        &mut self.cache_scorer
    }
    
    /// Reset statistics
    pub fn reset_statistics(&mut self) {
        self.statistics = CacheStatistics::new();
    }
}

impl CacheStatistics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_clear(
        &mut self,
        total_entries: usize,
        preserved_entries: usize,
        reason: ClearReason,
        session_id: &str,
    ) {
        self.total_clears += 1;
        self.entries_preserved += preserved_entries;
        self.entries_cleared += total_entries - preserved_entries;
        self.last_operation = Some(Utc::now());
        
        self.operation_history.push(CacheOperation {
            operation_type: CacheOperationType::Clear,
            timestamp: Utc::now(),
            entries_affected: total_entries,
            session_id: session_id.to_string(),
            details: format!("{:?}", reason),
        });
        
        // Keep only last 100 operations
        if self.operation_history.len() > 100 {
            self.operation_history.remove(0);
        }
    }
    
    pub fn record_retrieval(
        &mut self,
        retrieved_count: usize,
        tiers_searched: Vec<u8>,
        keywords_count: usize,
        session_id: &str,
    ) {
        self.total_retrievals += 1;
        self.entries_retrieved += retrieved_count;
        self.last_operation = Some(Utc::now());
        
        self.operation_history.push(CacheOperation {
            operation_type: CacheOperationType::Retrieve,
            timestamp: Utc::now(),
            entries_affected: retrieved_count,
            session_id: session_id.to_string(),
            details: format!("Tiers: {:?}, Keywords: {}", tiers_searched, keywords_count),
        });
        
        // Keep only last 100 operations
        if self.operation_history.len() > 100 {
            self.operation_history.remove(0);
        }
    }
    
    pub fn record_restore(&mut self, restored_count: usize, session_id: &str) {
        self.operation_history.push(CacheOperation {
            operation_type: CacheOperationType::Restore,
            timestamp: Utc::now(),
            entries_affected: restored_count,
            session_id: session_id.to_string(),
            details: "Cache restored from snapshot".to_string(),
        });
        
        // Keep only last 100 operations
        if self.operation_history.len() > 100 {
            self.operation_history.remove(0);
        }
    }
    
    pub fn record_snapshot(&mut self, snapshot_id: i64, entry_count: usize, session_id: &str) {
        self.operation_history.push(CacheOperation {
            operation_type: CacheOperationType::Snapshot,
            timestamp: Utc::now(),
            entries_affected: entry_count,
            session_id: session_id.to_string(),
            details: format!("Snapshot ID: {}", snapshot_id),
        });
        
        // Keep only last 100 operations
        if self.operation_history.len() > 100 {
            self.operation_history.remove(0);
        }
    }
}

impl RetrievalResult {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn total_entries(&self) -> usize {
        self.retrieved_entries.len()
    }
    
    pub fn average_similarity(&self) -> f32 {
        if self.retrieved_entries.is_empty() {
            return 0.0;
        }
        self.retrieved_entries.iter()
            .map(|e| e.similarity_score)
            .sum::<f32>() / self.retrieved_entries.len() as f32
    }
    
    pub fn is_empty(&self) -> bool {
        self.retrieved_entries.is_empty()
    }
    
    pub fn entries_by_tier(&self, tier: u8) -> Vec<&RetrievedEntry> {
        self.retrieved_entries.iter()
            .filter(|e| e.source_tier == tier)
            .collect()
    }
    
    pub fn primary_tier(&self) -> u8 {
        if self.retrieved_entries.is_empty() {
            return 0;
        }
        
        // Count entries by tier
        let mut tier_counts = HashMap::new();
        for entry in &self.retrieved_entries {
            *tier_counts.entry(entry.source_tier).or_insert(0) += 1;
        }
        
        // Return tier with most entries
        tier_counts.into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(tier, _)| tier)
            .unwrap_or(0)
    }
    
    pub fn add_tier_results(&mut self, tier: u8, results: Vec<RetrievedEntry>) {
        self.tiers_searched.push(tier);
        self.retrieved_entries.extend(results);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheStatisticsExport {
    pub total_clears: usize,
    pub total_retrievals: usize,
    pub entries_preserved: usize,
    pub entries_cleared: usize,
    pub entries_retrieved: usize,
    pub active_sessions: usize,
    pub last_operation: Option<DateTime<Utc>>,
    pub operation_history_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceResult {
    pub sessions_cleaned: usize,
    pub snapshots_pruned: usize,
    pub errors: Vec<String>,
}