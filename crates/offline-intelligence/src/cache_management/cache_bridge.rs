//! Creates smooth transitions between cached and retrieved content
use tracing::debug;

/// Creates bridging sentences for smooth cache context transitions
pub struct CacheContextBridge {
    cache_history: Vec<CacheTransition>,
    _max_history: usize, // Reserved for future adaptive history sizing
    // Memory limits to prevent unbounded growth
    max_transition_history: usize,
}

#[derive(Debug, Clone)]
pub struct CacheTransition {
    pub transition_type: TransitionType,
    pub preserved_entries: usize,
    pub retrieved_entries: usize,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum TransitionType {
    CacheCleared,
    CacheRetrieved,
    CacheRestored,
    CacheExtended,
}

#[derive(Debug, Clone)]
pub struct CacheBridgeStats {
    pub total_transitions: usize,
    pub avg_preserved_entries: f32,
    pub avg_retrieved_entries: f32,
    pub last_transition_type: Option<TransitionType>,
}

impl CacheContextBridge {
    /// Create a new cache context bridge
    pub fn new(max_history: usize) -> Self {
        Self {
            cache_history: Vec::new(),
            _max_history: max_history,
            max_transition_history: 50,
        }
    }

    /// Create a bridge message when cache is cleared
    pub fn create_clear_bridge(
        &mut self,
        cleared_count: usize,
        preserved_count: usize,
        keywords: &[String],
    ) -> String {
        self.record_transition(
            TransitionType::CacheCleared,
            preserved_count,
            0,
            keywords,
        );
        
        let keyword_list = if keywords.is_empty() {
            "various topics".to_string()
        } else {
            keywords.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
        };
        
        format!(
            "[Cache Management] Cleared {} entries from cache, preserved {} important entries related to: {}. Continuing with optimized context.",
            cleared_count, preserved_count, keyword_list
        )
    }

    /// Create a bridge message when content is retrieved
    pub fn create_retrieval_bridge(
        &mut self,
        retrieved_count: usize,
        source_tier: u8,
        keywords: &[String],
        similarity_score: Option<f32>,
    ) -> String {
        self.record_transition(
            TransitionType::CacheRetrieved,
            0,
            retrieved_count,
            keywords,
        );
        
        let source_desc = match source_tier {
            1 => "active cache",
            2 => "recent snapshots", 
            3 => "long-term memory",
            _ => "storage",
        };
        
        let similarity_text = similarity_score
            .map(|s| format!(" (similarity: {:.2})", s))
            .unwrap_or_default();
        
        let keyword_list = if keywords.is_empty() {
            "relevant context".to_string()
        } else {
            format!("'{}'", keywords.iter().take(3).cloned().collect::<Vec<_>>().join("', '"))
        };
        
        format!(
            "[Memory Retrieval] Retrieved {} entries from {} for {}{}. Integrating into current context.",
            retrieved_count, source_desc, keyword_list, similarity_text
        )
    }

    /// Create a bridge message when cache is restored
    pub fn create_restore_bridge(
        &mut self,
        restored_count: usize,
        snapshot_age: Option<std::time::Duration>,
    ) -> String {
        self.record_transition(
            TransitionType::CacheRestored,
            restored_count,
            0,
            &[],
        );
        
        let age_text = snapshot_age
            .map(|d| {
                let minutes = d.as_secs() / 60;
                if minutes > 0 {
                    format!(" ({} minutes old)", minutes)
                } else {
                    String::new()
                }
            })
            .unwrap_or_default();
        
        format!(
            "[Cache Restoration] Restored {} entries from previous snapshot{}. Context has been expanded.",
            restored_count, age_text
        )
    }

    fn record_transition(
        &mut self,
        transition_type: TransitionType,
        preserved: usize,
        retrieved: usize,
        keywords: &[String],
    ) {
        let transition = CacheTransition {
            transition_type: transition_type.clone(),
            preserved_entries: preserved,
            retrieved_entries: retrieved,
            timestamp: chrono::Utc::now(),
            keywords: keywords.to_vec(),
        };
        
        self.cache_history.push(transition);
        
        // Enforce memory limits
        if self.cache_history.len() > self.max_transition_history {
            let excess = self.cache_history.len() - self.max_transition_history;
            self.cache_history.drain(0..excess);
        }
        
        debug!("Recorded cache transition: {:?}", transition_type);
    }

    /// Get transition statistics
    pub fn get_stats(&self) -> CacheBridgeStats {
        let total = self.cache_history.len();
        let avg_preserved = if total > 0 {
            self.cache_history.iter().map(|t| t.preserved_entries).sum::<usize>() as f32 / total as f32
        } else { 0.0 };
        let avg_retrieved = if total > 0 {
            self.cache_history.iter().map(|t| t.retrieved_entries).sum::<usize>() as f32 / total as f32
        } else { 0.0 };
        
        CacheBridgeStats {
            total_transitions: total,
            avg_preserved_entries: avg_preserved,
            avg_retrieved_entries: avg_retrieved,
            last_transition_type: self.cache_history.last().map(|t| t.transition_type.clone()),
        }
    }

    /// Clear transition history
    pub fn clear_history(&mut self) {
        self.cache_history.clear();
        self.cache_history.shrink_to_fit();
    }
}