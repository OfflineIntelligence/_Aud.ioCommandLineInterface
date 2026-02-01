//! Extracts and preserves important KV cache entries

use regex::Regex;
use std::collections::HashMap;
use tracing::{debug, trace};

/// Types of KV cache entries that can be preserved
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CacheEntryType {
    AttentionKey,
    AttentionValue,
    FFNKey,
    FFNValue,
    SystemPrompt,
    CodeBlock,
    ImportantConcept,
    Question,
    NumericData,
    Custom(String),
}

impl std::fmt::Display for CacheEntryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheEntryType::AttentionKey => write!(f, "attention_key"),
            CacheEntryType::AttentionValue => write!(f, "attention_value"),
            CacheEntryType::FFNKey => write!(f, "ffn_key"),
            CacheEntryType::FFNValue => write!(f, "ffn_value"),
            CacheEntryType::SystemPrompt => write!(f, "system_prompt"),
            CacheEntryType::CodeBlock => write!(f, "code_block"),
            CacheEntryType::ImportantConcept => write!(f, "important_concept"),
            CacheEntryType::Question => write!(f, "question"),
            CacheEntryType::NumericData => write!(f, "numeric_data"),
            CacheEntryType::Custom(name) => write!(f, "{}", name),
        }
    }
}

/// Represents a KV cache entry (simplified for extraction)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]  // FIX: Added Serialize and Deserialize
pub struct KVEntry {
    pub key_hash: String,
    pub key_data: Option<Vec<u8>>,
    pub value_data: Vec<u8>,
    pub key_type: String,
    pub layer_index: i32,
    pub head_index: Option<i32>,
    pub importance_score: f32,
    pub access_count: i32,
    pub last_accessed: chrono::DateTime<chrono::Utc>,
}

/// An extracted KV cache entry with its metadata
#[derive(Debug, Clone)]
pub struct ExtractedCacheEntry {
    pub entry_type: CacheEntryType,
    pub key_hash: String,
    pub key_data: Option<Vec<u8>>,
    pub value_data: Vec<u8>,
    pub layer_index: i32,
    pub head_index: Option<i32>,
    pub importance_score: f32,
    pub access_count: i32,
    pub keywords: Vec<String>,
}

/// Extracts important KV cache entries
pub struct CacheExtractor {
    patterns: HashMap<CacheEntryType, Regex>,
    config: CacheExtractorConfig,
}

#[derive(Debug, Clone)]
pub struct CacheExtractorConfig {
    pub min_value_size: usize,
    pub max_value_size: usize,
    pub extract_keywords: bool,
    pub keyword_min_length: usize,
}

impl Default for CacheExtractorConfig {
    fn default() -> Self {
        Self {
            min_value_size: 10,
            max_value_size: 10000,
            extract_keywords: true,
            keyword_min_length: 3,
        }
    }
}

// Forward declare CacheEntryScorer trait
pub trait CacheEntryScorer {
    fn extract_keywords(&self, key_data: Option<&[u8]>) -> Vec<String>;
}

impl CacheExtractor {
    /// Create a new cache extractor
    pub fn new(config: CacheExtractorConfig) -> Self {
        let mut patterns = HashMap::new();
        
        // System prompt patterns
        patterns.insert(
            CacheEntryType::SystemPrompt,
            Regex::new(r"(?i)(system|instruction|prompt|assistant_role|you are|your role)").unwrap(),
        );
        
        // Code block patterns
        patterns.insert(
            CacheEntryType::CodeBlock,
            Regex::new(r"```|\b(def|function|class|import|return|print|let|const|var)\b|\b(python|rust|javascript|java|c\+\+|go|sql)\b").unwrap(),
        );
        
        // Important concept patterns
        patterns.insert(
            CacheEntryType::ImportantConcept,
            Regex::new(r"(?i)\b(important|crucial|critical|essential|must|need|require|urgent|priority|key|main|primary)\b").unwrap(),
        );
        
        // Question patterns
        patterns.insert(
            CacheEntryType::Question,
            Regex::new(r"\?$|^(what|how|why|when|where|who|explain|describe|can you|could you|would you|should you)").unwrap(),
        );
        
        // Numeric data patterns
        patterns.insert(
            CacheEntryType::NumericData,
            Regex::new(r"\b\d+(?:\.\d+)?%?\b|\b(date|time|age|year|month|day|hour|minute|second)\b").unwrap(),
        );
        
        Self { patterns, config }
    }
    
    /// Add a custom pattern
    pub fn add_custom_pattern(&mut self, name: String, pattern: Regex) {
        self.patterns.insert(CacheEntryType::Custom(name), pattern);
    }
    
    /// Extract important entries from KV cache
    pub fn extract_entries(
        &self,
        entries: &[KVEntry],
        scorer: &impl CacheEntryScorer,
    ) -> Vec<ExtractedCacheEntry> {
        let mut extracted = Vec::new();
        
        for entry in entries {
            // Skip if value size is outside bounds
            if entry.value_data.len() < self.config.min_value_size 
                || entry.value_data.len() > self.config.max_value_size {
                continue;
            }
            
            // Determine entry type based on key patterns
            let entry_type = self.classify_entry(entry);
            
            // Extract keywords if enabled
            let keywords = if self.config.extract_keywords {
                scorer.extract_keywords(entry.key_data.as_deref())
            } else {
                Vec::new()
            };
            
            let extracted_entry = ExtractedCacheEntry {
                entry_type,
                key_hash: entry.key_hash.clone(),
                key_data: entry.key_data.clone(),
                value_data: entry.value_data.clone(),
                layer_index: entry.layer_index,
                head_index: entry.head_index,
                importance_score: entry.importance_score,
                access_count: entry.access_count,
                keywords,
            };
            
            trace!("Extracted cache entry: {} (score: {})", 
                extracted_entry.entry_type, extracted_entry.importance_score);
            
            extracted.push(extracted_entry);
        }
        
        // Sort by importance score
        extracted.sort_by(|a, b| b.importance_score.partial_cmp(&a.importance_score)
            .unwrap_or(std::cmp::Ordering::Equal));
        
        debug!("Extracted {} important cache entries", extracted.len());
        extracted
    }
    
    fn classify_entry(&self, entry: &KVEntry) -> CacheEntryType {
        // First check key type
        let key_type_str = entry.key_type.as_str();
        let base_type = match key_type_str {
            "attention_key" => CacheEntryType::AttentionKey,
            "attention_value" => CacheEntryType::AttentionValue,
            "ffn_key" => CacheEntryType::FFNKey,
            "ffn_value" => CacheEntryType::FFNValue,
            _ => CacheEntryType::AttentionKey, // Default
        };
        
        // Check for special patterns in key data
        if let Some(key_data) = &entry.key_data {
            if let Ok(key_str) = std::str::from_utf8(key_data) {
                for (entry_type, pattern) in &self.patterns {
                    if pattern.is_match(key_str) {
                        // Return the first matching pattern
                        return entry_type.clone();
                    }
                }
            }
        }
        
        base_type
    }
    
    /// Filter entries that should be preserved
    pub fn filter_preserved_entries(
        &self,
        entries: &[ExtractedCacheEntry],
        min_importance: f32,
        preserve_system: bool,
        preserve_code: bool,
    ) -> Vec<ExtractedCacheEntry> {
        entries.iter()
            .filter(|entry| {
                // Check importance threshold
                if entry.importance_score < min_importance {
                    return false;
                }
                
                // Check specific preservation rules
                match &entry.entry_type {
                    CacheEntryType::SystemPrompt if preserve_system => true,
                    CacheEntryType::CodeBlock if preserve_code => true,
                    CacheEntryType::ImportantConcept => true,
                    CacheEntryType::AttentionKey | CacheEntryType::AttentionValue => true,
                    _ => entry.importance_score >= min_importance * 1.2, // Higher threshold for others
                }
            })
            .cloned()
            .collect()
    }
}