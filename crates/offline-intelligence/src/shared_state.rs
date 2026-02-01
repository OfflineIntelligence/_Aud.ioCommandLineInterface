//! Shared state management for thread-based architecture
//!
//! This module provides the core shared memory infrastructure that enables
//! efficient communication between worker threads while maintaining thread safety.

use std::sync::{Arc, RwLock, atomic::{AtomicUsize, Ordering}};
use dashmap::DashMap;
use tracing::info;

use crate::{
    config::Config,
    context_engine::ContextOrchestrator,
    memory_db::MemoryDatabase,
    cache_management::KVCacheManager,
    worker_threads::LLMWorker,
    model_management::ModelManager,
};

/// Core shared system state container
pub struct SharedSystemState {
    /// Conversation data with hierarchical locking
    pub conversations: Arc<ConversationHierarchy>,

    /// LLM runtime for direct inference
    pub llm_runtime: Arc<RwLock<Option<LLMRuntime>>>,

    /// Cache management system
    pub cache_manager: Arc<RwLock<Option<Arc<KVCacheManager>>>>,

    /// Database connection pool
    pub database_pool: Arc<MemoryDatabase>,

    /// Configuration (read-only after initialization)
    pub config: Arc<Config>,

    /// Atomic counters for performance tracking
    pub counters: Arc<AtomicCounters>,

    /// Context orchestrator for memory management (tokio RwLock for async access)
    pub context_orchestrator: Arc<tokio::sync::RwLock<Option<ContextOrchestrator>>>,

    /// LLM worker for inference operations
    pub llm_worker: Arc<LLMWorker>,

    /// Model management system
    pub model_manager: Option<Arc<ModelManager>>,
}

/// Hierarchical conversation storage for reduced lock contention
pub struct ConversationHierarchy {
    /// Coarse-grained session-level locks
    pub sessions: DashMap<String, Arc<RwLock<SessionData>>>,

    /// Fine-grained message-level queues for hot paths
    pub message_queues: DashMap<String, Arc<crossbeam_queue::ArrayQueue<PendingMessage>>>,

    /// Lock-free counters for performance metrics
    pub counters: Arc<AtomicCounters>,
}

/// Session-level data structure
#[derive(Debug, Clone)]
pub struct SessionData {
    pub session_id: String,
    pub messages: Vec<crate::memory::Message>,
    pub last_accessed: std::time::Instant,
    pub pinned: bool,
}

/// Pending message for asynchronous processing
#[derive(Debug, Clone)]
pub struct PendingMessage {
    pub message: crate::memory::Message,
    pub timestamp: std::time::Instant,
}

/// Atomic counters for system metrics
pub struct AtomicCounters {
    pub total_requests: AtomicUsize,
    pub active_sessions: AtomicUsize,
    pub processed_messages: AtomicUsize,
    pub cache_hits: AtomicUsize,
    pub cache_misses: AtomicUsize,
}

impl AtomicCounters {
    pub fn new() -> Self {
        Self {
            total_requests: AtomicUsize::new(0),
            active_sessions: AtomicUsize::new(0),
            processed_messages: AtomicUsize::new(0),
            cache_hits: AtomicUsize::new(0),
            cache_misses: AtomicUsize::new(0),
        }
    }

    pub fn inc_total_requests(&self) -> usize {
        self.total_requests.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn inc_processed_messages(&self) -> usize {
        self.processed_messages.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn inc_cache_hit(&self) -> usize {
        self.cache_hits.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn inc_cache_miss(&self) -> usize {
        self.cache_misses.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// Direct LLM runtime integration
pub struct LLMRuntime {
    pub model_path: String,
    pub context_size: u32,
    pub batch_size: u32,
    pub threads: u32,
    pub gpu_layers: u32,
    // Note: Actual llama.cpp integration would go here
    // For now, we'll maintain the existing HTTP proxy approach
    // but prepare the structure for direct integration
}

impl SharedSystemState {
    pub fn new(config: Config, database: Arc<MemoryDatabase>) -> anyhow::Result<Self> {
        info!("Initializing shared system state");

        let conversations = Arc::new(ConversationHierarchy {
            sessions: DashMap::new(),
            message_queues: DashMap::new(),
            counters: Arc::new(AtomicCounters::new()),
        });

        let config = Arc::new(config);
        let counters = Arc::new(AtomicCounters::new());

        // Create LLM worker with backend URL from config
        let backend_url = config.backend_url.clone();
        let llm_worker = Arc::new(LLMWorker::new_with_backend(backend_url));

        Ok(Self {
            conversations,
            llm_runtime: Arc::new(RwLock::new(None)),
            cache_manager: Arc::new(RwLock::new(None)),
            database_pool: database,
            config,
            counters,
            context_orchestrator: Arc::new(tokio::sync::RwLock::new(None)),
            llm_worker,
            model_manager: None,
        })
    }

    /// Set LLM worker (replaces the default one created during initialization)
    pub fn set_llm_worker(&self, _worker: Arc<LLMWorker>) {
        // The llm_worker is now initialized in new() with the backend URL.
        // This method is kept for backward compatibility but is a no-op since
        // we initialize with the correct backend URL during construction.
        info!("LLM worker already initialized with backend URL");
    }

    /// Initialize LLM runtime with current configuration
    pub fn initialize_llm_runtime(&self) -> anyhow::Result<()> {
        let mut runtime_guard = self.llm_runtime.try_write()
            .map_err(|_| anyhow::anyhow!("Failed to acquire LLM runtime write lock"))?;

        let runtime = LLMRuntime {
            model_path: self.config.model_path.clone(),
            context_size: self.config.ctx_size,
            batch_size: self.config.batch_size,
            threads: self.config.threads,
            gpu_layers: self.config.gpu_layers,
        };

        *runtime_guard = Some(runtime);
        info!("LLM runtime initialized");
        Ok(())
    }

    /// Get or create session data with proper locking
    pub async fn get_or_create_session(&self, session_id: &str) -> Arc<RwLock<SessionData>> {
        // Fast path: try to get existing session
        if let Some(session) = self.conversations.sessions.get(session_id) {
            return session.clone();
        }

        // Slow path: create new session
        let new_session = Arc::new(RwLock::new(SessionData {
            session_id: session_id.to_string(),
            messages: Vec::new(),
            last_accessed: std::time::Instant::now(),
            pinned: false,
        }));

        self.conversations.sessions.insert(session_id.to_string(), new_session.clone());
        self.counters.active_sessions.fetch_add(1, Ordering::Relaxed);

        new_session
    }

    /// Queue message for asynchronous processing
    pub fn queue_message(&self, session_id: &str, message: crate::memory::Message) -> bool {
        let queue = self.conversations.message_queues
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(crossbeam_queue::ArrayQueue::new(1000)));

        queue.push(PendingMessage {
            message,
            timestamp: std::time::Instant::now(),
        }).is_ok()
    }

    /// Process queued messages for a session
    pub async fn process_queued_messages(&self, session_id: &str) -> Vec<PendingMessage> {
        let mut messages = Vec::new();

        if let Some(queue) = self.conversations.message_queues.get(session_id) {
            while let Some(msg) = queue.pop() {
                messages.push(msg);
            }
        }

        messages
    }
}

impl ConversationHierarchy {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
            message_queues: DashMap::new(),
            counters: Arc::new(AtomicCounters::new()),
        }
    }
}

/// Unified application state for all API handlers.
/// This is the single state type used by the Axum router, providing access
/// to all subsystems through shared memory (Arc) rather than network hops.
#[derive(Clone)]
pub struct UnifiedAppState {
    pub shared_state: Arc<SharedSystemState>,
    pub context_orchestrator: Arc<tokio::sync::RwLock<Option<ContextOrchestrator>>>,
    pub llm_worker: Arc<LLMWorker>,
}

impl UnifiedAppState {
    pub fn new(shared_state: Arc<SharedSystemState>) -> Self {
        let context_orchestrator = shared_state.context_orchestrator.clone();
        let llm_worker = shared_state.llm_worker.clone();
        Self {
            shared_state,
            context_orchestrator,
            llm_worker,
        }
    }
}

// Re-exports for convenience
pub use self::SharedSystemState as SharedState;
