//! Builds optimal context from multiple memory sources

use crate::memory::Message;
use crate::memory_db::{StoredMessage, Summary as DbSummary};
use tracing::{info, debug};

/// Builds context from multiple memory sources
pub struct ContextBuilder {
    config: ContextBuilderConfig,
}

/// Configuration for context building
#[derive(Debug, Clone)]
pub struct ContextBuilderConfig {
    pub max_total_tokens: usize,
    pub min_current_context_ratio: f32,
    pub max_summary_ratio: f32,
    pub preserve_system_messages: bool,
    pub enable_detail_injection: bool,
    pub detail_injection_threshold: f32,
}

impl Default for ContextBuilderConfig {
    fn default() -> Self {
        Self {
            max_total_tokens: 4000,
            min_current_context_ratio: 0.4,
            max_summary_ratio: 0.4,
            preserve_system_messages: true,
            enable_detail_injection: true,
            detail_injection_threshold: 0.7,
        }
    }
}

impl ContextBuilder {
    /// Create a new context builder
    pub fn new(config: ContextBuilderConfig) -> Self {
        Self {
            config,
        }
    }
    
    /// Build optimal context from multiple sources
    pub async fn build_context(
        &mut self,
        current_messages: &[Message],
        tier1_content: Option<Vec<Message>>,
        tier2_summaries: Option<Vec<DbSummary>>,
        tier3_messages: Option<Vec<StoredMessage>>,
        cross_session_messages: Option<Vec<StoredMessage>>, // NEW parameter
        user_query: Option<&str>,
    ) -> anyhow::Result<Vec<Message>> {
        info!("Building context from {} current messages", current_messages.len());
        
        // Start with current messages (incorporates tier1 if provided)
        let mut context = self.prepare_context_with_tier1(current_messages, tier1_content);
        
        // Add cross-session messages if available
        if let Some(ref cross_messages) = cross_session_messages {
            self.add_cross_session_context(&mut context, cross_messages, user_query)
                .await?;
        }
        
        // Add summaries if available
        if let Some(ref summaries) = tier2_summaries {
            self.add_summaries_to_context(&mut context, summaries, current_messages, user_query)
                .await?;
        }
        
        // Add specific details from full messages if needed
        if let Some(ref full_messages) = tier3_messages {
            self.add_specific_details(&mut context, full_messages, user_query)
                .await?;
        }
        
        // Ensure we don't exceed token limits
        self.trim_to_token_limit(&mut context);
        
        // Add bridging between summarized and current content
        self.add_bridging(&mut context, current_messages, tier2_summaries.as_ref())
            .await?;
        
        debug!("Built context with {} messages", context.len());
        
        Ok(context)
    }

    /// Add historical messages from other sessions to the current context
    async fn add_cross_session_context(
        &mut self,
        context: &mut Vec<Message>,
        cross_messages: &[StoredMessage],
        _user_query: Option<&str>,
    ) -> anyhow::Result<()> {
        if cross_messages.is_empty() {
            return Ok(());
        }
        
        // Create a bridging message to inform the model of the source
        let bridge = Message {
            role: "system".to_string(),
            content: "[Context from previous conversations]".to_string(),
        };
        context.insert(0, bridge);
        
        // Add relevant cross-session messages (limit to 3 to avoid context bloat)
        for message in cross_messages.iter().take(3) {
            let cross_msg = Message {
                role: message.role.clone(),
                content: format!("[From earlier: {}]", message.content),
            };
            context.insert(1, cross_msg); // Insert after bridge
        }
        
        Ok(())
    }
    
    /// Prepare context incorporating Tier 1 content if available
    fn prepare_context_with_tier1(
        &self, 
        current_messages: &[Message], 
        tier1_content: Option<Vec<Message>>
    ) -> Vec<Message> {
        let mut context = Vec::new();
        
        // Always preserve system messages from current
        if self.config.preserve_system_messages {
            for message in current_messages.iter().filter(|m| m.role == "system") {
                context.push(message.clone());
            }
        }
        
        // Use Tier 1 content if available, otherwise use recent current messages
        if let Some(tier1_messages) = tier1_content {
            context.extend(tier1_messages);
        } else {
            let recent_messages = self.select_recent_messages(current_messages);
            context.extend(recent_messages);
        }
        
        context
    }
    
    /// Select recent messages to keep
    fn select_recent_messages(&self, messages: &[Message]) -> Vec<Message> {
        if messages.is_empty() {
            return Vec::new();
        }
        
        let target_count = (messages.len() as f32 * self.config.min_current_context_ratio).ceil() as usize;
        let target_count = target_count.max(1).min(messages.len());
        
        messages.iter()
            .rev()
            .take(target_count)
            .rev()
            .cloned()
            .collect()
    }
    
    /// Add summaries to context
    async fn add_summaries_to_context(
        &mut self,
        context: &mut Vec<Message>,
        summaries: &[DbSummary],
        current_messages: &[Message],
        user_query: Option<&str>,
    ) -> anyhow::Result<()> {
        if summaries.is_empty() {
            return Ok(());
        }
        
        let relevant_summaries = self.select_relevant_summaries(summaries, current_messages, user_query);
        
        for summary in &relevant_summaries {
            let summary_message = self.summary_to_message(summary, current_messages);
            context.insert(0, summary_message);
        }
        
        Ok(())
    }
    
    fn select_relevant_summaries<'a>(
        &self,
        summaries: &'a [DbSummary],
        current_messages: &[Message],
        user_query: Option<&str>,
    ) -> Vec<&'a DbSummary> {
        let mut relevant = Vec::new();
        let current_topics = self.extract_topics(current_messages);
        
        let mut scored: Vec<(&DbSummary, f32)> = summaries.iter()
            .map(|summary| {
                let score = self.score_summary_relevance(summary, &current_topics, user_query);
                (summary, score)
            })
            .collect();
        
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        
        let mut total_tokens = 0;
        let max_summary_tokens = (self.config.max_total_tokens as f32 * self.config.max_summary_ratio) as usize;
        
        for (summary, score) in scored {
            if score < 0.3 { continue; }
            
            let summary_tokens = summary.summary_text.len() / 4;
            
            if total_tokens + summary_tokens > max_summary_tokens { break; }
            
            relevant.push(summary);
            total_tokens += summary_tokens;
        }
        
        relevant
    }
    
    fn score_summary_relevance(&self, summary: &DbSummary, current_topics: &[String], user_query: Option<&str>) -> f32 {
        let mut score = 0.0;
        
        // Topic matching
        for topic in current_topics {
            if summary.key_topics.iter().any(|t| t.to_lowercase().contains(&topic.to_lowercase())) {
                score += 0.4;
            }
        }
        
        // Query matching
        if let Some(query) = user_query {
            let query_lower = query.to_lowercase();
            for topic in &summary.key_topics {
                if query_lower.contains(&topic.to_lowercase()) {
                    score += 0.5;
                }
            }
        }
        
        // Recency scoring
        let age_hours = chrono::Utc::now().signed_duration_since(summary.generated_at).num_hours();
        let recency_score = 1.0 / (1.0 + age_hours as f32 / 24.0);
        score += recency_score * 0.3;
        
        // Compression ratio (more compressed = potentially more relevant for context)
        score += summary.compression_ratio.min(1.0) * 0.2;
        
        score.min(1.0)
    }
    
    fn summary_to_message(&self, summary: &DbSummary, current_messages: &[Message]) -> Message {
        let content = if current_messages.len() > 5 {
            format!("[Summary of earlier conversation: {}]", summary.summary_text)
        } else {
            format!("[Earlier: {}]", summary.summary_text)
        };
        Message { role: "system".to_string(), content }
    }

    async fn add_specific_details(
        &mut self, 
        context: &mut Vec<Message>, 
        full_messages: &[StoredMessage], 
        user_query: Option<&str>
    ) -> anyhow::Result<()> {
        if !self.config.enable_detail_injection || full_messages.is_empty() {
            return Ok(());
        }
        
        let detail_requests = self.extract_detail_requests(user_query);
        if detail_requests.is_empty() { 
            return Ok(()); 
        }
        
        let relevant_messages = self.find_relevant_details(full_messages, &detail_requests);
        for message in &relevant_messages {
            let detail_message = Message {
                role: message.role.clone(),
                content: format!("[Earlier detail: {}]", message.content),
            };
            
            // Insert details before the last user message if possible
            if let Some(pos) = context.iter().rposition(|m| m.role == "user") {
                context.insert(pos, detail_message);
            } else {
                context.insert(0, detail_message);
            }
        }
        
        Ok(())
    }

    fn extract_detail_requests(&self, user_query: Option<&str>) -> Vec<String> {
        let mut requests = Vec::new();
        if let Some(query) = user_query {
            let query_lower = query.to_lowercase();
            let words: Vec<&str> = query_lower.split_whitespace().collect();
            
            for i in 0..words.len().saturating_sub(1) {
                if ["the", "that", "those", "specific", "exact"].contains(&words[i]) {
                    let potential = words[i + 1..].iter()
                        .take(3)
                        .copied()
                        .collect::<Vec<&str>>()
                        .join(" ");
                    
                    if !potential.is_empty() { 
                        requests.push(potential); 
                    }
                }
            }
        }
        
        requests.dedup();
        requests
    }

    fn find_relevant_details<'a>(
        &self, 
        messages: &'a [StoredMessage], 
        detail_requests: &[String]
    ) -> Vec<&'a StoredMessage> {
        let mut relevant = Vec::new();
        
        for message in messages {
            let content_lower = message.content.to_lowercase();
            
            for request in detail_requests {
                if content_lower.contains(&request.to_lowercase()) {
                    relevant.push(message);
                    break;
                }
            }
            
            if relevant.len() >= 3 { 
                break; 
            }
        }
        
        relevant
    }

    fn trim_to_token_limit(&self, context: &mut Vec<Message>) {
        let mut total_tokens = 0;
        let mut to_remove = Vec::new();
        
        for (idx, message) in context.iter().enumerate() {
            let message_tokens = message.content.len() / 4;
            
            if total_tokens + message_tokens > self.config.max_total_tokens {
                to_remove.push(idx);
            } else {
                total_tokens += message_tokens;
            }
        }
        
        // Remove from end to preserve order
        for idx in to_remove.iter().rev() {
            context.remove(*idx);
        }
    }

    /// Add bridging between summarized and current content
    async fn add_bridging(
        &mut self,
        context: &mut Vec<Message>,
        _current_messages: &[Message],
        summaries: Option<&Vec<DbSummary>>,
    ) -> anyhow::Result<()> {
        if !self.config.enable_detail_injection || context.len() < 2 || summaries.is_none() {
            return Ok(());
        }
        
        let transition_idx = self.find_transition_point(context);
        
        if transition_idx > 0 && transition_idx < context.len() {
            let summary_count = context[..transition_idx].iter()
                .filter(|m| m.role == "system" && 
                        (m.content.starts_with("[Summary") || m.content.starts_with("[Earlier:")))
                .count();
            
            if summary_count > 0 {
                let bridge_message = Message {
                    role: "system".to_string(),
                    content: format!("[Continuing from earlier conversation with {} summary{}]",
                        summary_count, if summary_count > 1 { "s" } else { "" }),
                };
                
                context.insert(transition_idx, bridge_message);
            }
        }
        
        Ok(())
    }
    
    fn find_transition_point(&self, context: &[Message]) -> usize {
        for (idx, message) in context.iter().enumerate() {
            if !(message.role == "system" && 
                 (message.content.starts_with("[Summary") || message.content.starts_with("[Earlier:") || message.content.starts_with("[Context"))) {
                return idx;
            }
        }
        context.len()
    }
    
    fn extract_topics(&self, messages: &[Message]) -> Vec<String> {
        let mut topics = Vec::new();
        
        for message in messages.iter().rev().take(5) {
            let words: Vec<&str> = message.content.split_whitespace().collect();
            
            for i in 0..words.len().saturating_sub(2) {
                let word_lower = words[i].to_lowercase();
                
                if word_lower == "about" || word_lower == "regarding" {
                    let topic = words[i + 1..].iter()
                        .take(3)
                        .copied()
                        .collect::<Vec<&str>>()
                        .join(" ");
                    
                    if !topic.is_empty() { 
                        topics.push(topic); 
                    }
                }
                
                if ["what", "how", "why", "when", "where", "who", "which"].contains(&word_lower.as_str()) {
                    let topic = words[i + 1..].iter()
                        .take(4)
                        .copied()
                        .collect::<Vec<&str>>()
                        .join(" ");
                    
                    if !topic.is_empty() { 
                        topics.push(topic); 
                    }
                }
            }
        }
        
        topics.dedup();
        topics.truncate(3);
        topics
    }
}

impl Clone for ContextBuilder {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
        }
    }
}