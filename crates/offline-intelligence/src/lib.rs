// _Aud.io/offline-intelligence/crates/src/lib.rs

pub mod admin;
pub mod api;
pub mod backend_target;
pub mod config;
pub mod context_engine;
pub mod memory;
pub mod memory_db;
pub mod metrics;
pub mod resources;
pub mod cache_management;
pub mod telemetry;
pub mod utils;
pub mod capabilities;
pub mod shared_state;
pub mod thread_pool;
pub mod worker_threads;
pub mod thread_server;
pub mod model_runtime;
pub mod model_management;
pub mod tools;
pub mod agent;
pub mod terminal;
pub mod ui;

pub use admin::*;
pub use backend_target::*;
pub use config::*;
pub use metrics::*;
pub use cache_management::*;
pub use thread_server::*;

use std::sync::Arc;
use tracing::{info, warn};

use memory_db::MemoryDatabase;

async fn init_cache_manager(
    memory_database: Arc<MemoryDatabase>,
) -> anyhow::Result<Option<Arc<cache_management::KVCacheManager>>> {
    let cache_config = cache_management::KVCacheConfig::default();
    
    match KVCacheManager::new(cache_config, memory_database) {
        Ok(manager) => {
            info!("Cache manager initialized successfully");
            Ok(Some(Arc::new(manager)))
        }
        Err(e) => {
            warn!("Failed to initialize cache manager: {}, cache features disabled", e);
            Ok(None)
        }
    }
}