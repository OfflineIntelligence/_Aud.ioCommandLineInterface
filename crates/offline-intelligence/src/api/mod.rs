// Server/src/api/mod.rs
//! API module - External interfaces for the memory system

pub mod memory_api;
pub mod search_api;
pub mod admin_api;
pub mod title_api;
pub mod conversation_api;
pub mod stream_api;
pub mod model_api;

// Re-export API handlers
pub use memory_api::{memory_optimize, memory_stats, memory_cleanup};
pub use title_api::{generate_title, GenerateTitleRequest, GenerateTitleResponse};
pub use conversation_api::{get_conversations, get_conversation, update_conversation_title, delete_conversation, update_conversation_pinned};
pub use stream_api::generate_stream;
pub use model_api::*;