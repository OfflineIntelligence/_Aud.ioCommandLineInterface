//! Thread-based server implementation
//!
//! This module provides the server startup that uses thread-based
//! shared memory architecture. All API handlers access state through
//! Arc-wrapped shared memory (UnifiedAppState) — zero network hops
//! between components. The only network call is to the localhost llama-server.

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, debug};

use crate::{
    config::Config,
    shared_state::{SharedState, UnifiedAppState},
    thread_pool::{ThreadPool, ThreadPoolConfig},
    worker_threads::{ContextWorker, CacheWorker, DatabaseWorker, LLMWorker},
    memory_db::MemoryDatabase,
    model_management::ModelManager,
};

/// Thread-based unified application state (internal, used during initialization)
#[derive(Clone)]
pub struct ThreadBasedAppState {
    pub shared_state: Arc<SharedState>,
    pub thread_pool: Arc<RwLock<Option<ThreadPool>>>,
    pub context_worker: Arc<ContextWorker>,
    pub cache_worker: Arc<CacheWorker>,
    pub database_worker: Arc<DatabaseWorker>,
    pub llm_worker: Arc<LLMWorker>,
}

/// Run server with thread-based architecture
pub async fn run_thread_server(cfg: Config) -> anyhow::Result<()> {
    crate::telemetry::init_tracing();
    crate::metrics::init_metrics();
    cfg.print_config();

    info!("Starting thread-based server architecture");

    // Initialize database
    let memory_db_path = std::path::Path::new("./data/conversations.db");
    let memory_database = match MemoryDatabase::new(memory_db_path) {
        Ok(db) => {
            info!("Memory database initialized at: {}", memory_db_path.display());
            Arc::new(db)
        }
        Err(e) => {
            warn!("Failed to initialize memory database: {}. Falling back to in-memory.", e);
            Arc::new(MemoryDatabase::new_in_memory()?)
        }
    };

    // Initialize shared state (creates LLM worker internally with backend_url)
    let mut shared_state = SharedState::new(cfg.clone(), memory_database.clone())?;

    // Initialize Model Manager
    info!("📦 Initializing Model Manager");
    match ModelManager::new() {
        Ok(model_manager) => {
            let model_manager_arc = Arc::new(model_manager);
            // Initialize the model manager
            if let Err(e) = model_manager_arc.initialize().await {
                warn!("⚠️  Model manager initialization failed: {}", e);
            } else {
                info!("✅ Model manager initialized successfully");
            }
            shared_state.model_manager = Some(model_manager_arc);
        }
        Err(e) => {
            warn!("⚠️  Failed to create model manager: {}", e);
        }
    }

    let shared_state = Arc::new(shared_state);

    // Initialize Runtime Manager for multi-format model support
    info!("🚀 Initializing Runtime Manager for multi-format model support");
    let runtime_manager = Arc::new(crate::model_runtime::RuntimeManager::new());
    
    // Configure and initialize the runtime based on detected model format
    let runtime_config = crate::model_runtime::RuntimeConfig {
        model_path: std::path::PathBuf::from(&cfg.model_path),
        format: crate::model_runtime::ModelFormat::GGUF, // Will be auto-detected
        host: cfg.llama_host.clone(),
        port: cfg.llama_port,
        context_size: cfg.ctx_size,
        batch_size: cfg.batch_size,
        threads: cfg.threads,
        gpu_layers: cfg.gpu_layers,
        runtime_binary: Some(std::path::PathBuf::from(&cfg.llama_bin)),
        extra_config: serde_json::json!({}),
    };
    
    // Initialize with automatic format detection
    match runtime_manager.initialize_auto(runtime_config).await {
        Ok(base_url) => {
            info!("✅ Model runtime initialized successfully");
            info!("   Runtime endpoint: {}", base_url);
            // Update backend_url in shared state config if needed
        }
        Err(e) => {
            warn!("⚠️  Runtime initialization failed: {}", e);
            warn!("   The system will attempt to use the configured backend_url directly");
        }
    }

    // Initialize workers
    let _context_worker: Arc<ContextWorker> = Arc::new(ContextWorker::new(shared_state.clone()));
    let _cache_worker: Arc<CacheWorker> = Arc::new(CacheWorker::new(shared_state.clone()));
    let _database_worker: Arc<DatabaseWorker> = Arc::new(DatabaseWorker::new(shared_state.clone()));
    let _llm_worker = shared_state.llm_worker.clone();

    // Initialize cache manager
    let cache_manager = match crate::cache_management::create_default_cache_manager(
        crate::cache_management::KVCacheConfig::default(),
        memory_database.clone(),
    ) {
        Ok(manager) => {
            info!("Cache manager initialized successfully");
            Some(Arc::new(manager))
        }
        Err(e) => {
            warn!("Failed to initialize cache manager: {}, cache features disabled", e);
            None
        }
    };

    // Initialize context orchestrator
    let context_orchestrator = match crate::context_engine::create_default_orchestrator(
        memory_database.clone(),
    ).await {
        Ok(mut orchestrator) => {
            // Inject LLM worker so the orchestrator can generate query embeddings
            // for semantic search when the hot KV cache doesn't have the answer.
            orchestrator.set_llm_worker(shared_state.llm_worker.clone());
            info!("Context orchestrator initialized with semantic search support");
            Some(orchestrator)
        }
        Err(e) => {
            warn!("Failed to initialize context orchestrator: {}. Memory features disabled.", e);
            None
        }
    };

    // Initialize thread pool
    let thread_pool_config = ThreadPoolConfig::new(&cfg);
    let mut thread_pool = ThreadPool::new(thread_pool_config, shared_state.clone());
    thread_pool.start().await?;

    // Update shared state with initialized components
    {
        let mut cache_guard = shared_state.cache_manager.write()
            .map_err(|_| anyhow::anyhow!("Failed to acquire cache manager write lock"))?;
        *cache_guard = cache_manager;

        // LLM runtime is now managed by RuntimeManager, no need to initialize here
        // shared_state.initialize_llm_runtime()?;  // Removed - handled by RuntimeManager
    }

    // Initialize embedding HNSW index from any previously stored embeddings
    // This makes semantic search available immediately on startup.
    if let Err(e) = shared_state.database_pool.embeddings.initialize_index("llama-server") {
        debug!("Embedding index init: {} (will build on first embedding store)", e);
    } else {
        info!("Embedding HNSW index loaded from existing data");
    }

    // Set context orchestrator (tokio RwLock for async access from handlers)
    {
        let mut orch_guard = shared_state.context_orchestrator.write().await;
        *orch_guard = context_orchestrator;
    }

    // Build the unified app state for the router
    let unified_state = UnifiedAppState::new(shared_state.clone());

    // Start HTTP server
    info!("Starting HTTP server on {}:{}", cfg.api_host, cfg.api_port);
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", cfg.api_host, cfg.api_port)).await?;

    let app = build_compatible_router(unified_state);

    axum::serve(listener, app).await?;

    Ok(())
}

/// Build router for 1-hop architecture
fn build_compatible_router(state: UnifiedAppState) -> axum::Router {
    use axum::{
        Router,
        routing::{get, post, put, delete},
    };
    use tower_http::{
        cors::{Any, CorsLayer},
        trace::TraceLayer,
        timeout::TimeoutLayer,
    };
    use std::time::Duration;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST, axum::http::Method::PUT, axum::http::Method::DELETE])
        .allow_headers(Any);

    Router::new()
        // Core 1-hop streaming endpoint
        .route("/generate/stream", post(crate::api::stream_api::generate_stream))
        // Title generation via shared memory -> LLM worker
        .route("/generate/title", post(crate::api::title_api::generate_title))
        // Conversation CRUD via shared memory -> database
        .route("/conversations", get(crate::api::conversation_api::get_conversations))
        .route("/conversations/:id", get(crate::api::conversation_api::get_conversation))
        .route("/conversations/:id/title", put(crate::api::conversation_api::update_conversation_title))
        .route("/conversations/:id/pinned", post(crate::api::conversation_api::update_conversation_pinned))
        .route("/conversations/:id", delete(crate::api::conversation_api::delete_conversation))
        // Model management endpoints
        .route("/models", get(crate::api::model_api::list_models))
        .route("/models/search", get(crate::api::model_api::search_models))
        .route("/models/install", post(crate::api::model_api::install_model))
        .route("/models/remove", delete(crate::api::model_api::remove_model))
        .route("/models/progress", get(crate::api::model_api::get_download_progress))
        .route("/models/downloads", get(crate::api::model_api::get_active_downloads))
        .route("/models/cancel", post(crate::api::model_api::cancel_download))
        .route("/models/recommendations", get(crate::api::model_api::get_recommended_models))
        .route("/models/preferences", post(crate::api::model_api::update_preferences))
        .route("/hardware/recommendations", get(crate::api::model_api::get_hardware_recommendations))
        .route("/hardware/info", get(crate::api::model_api::get_hardware_info))
        .route("/healthz", get(|| async { "OK" }))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(Duration::from_secs(600)))
        .with_state(state)
}
