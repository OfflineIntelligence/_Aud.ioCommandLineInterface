//! Utilities module - Common utility functions for text processing, topic extraction, path resolution, and confirmation workflows

pub mod text_utils;
pub mod topic_extractor;
pub mod path_resolver;
pub mod confirmation;

// Re-export commonly used utilities
pub use text_utils::TextUtils;
pub use topic_extractor::TopicExtractor;
pub use path_resolver::{resolve_path_with_fallbacks, ResolvedPath, PathSource, PathRisk, get_home_directory, get_desktop_path, get_documents_path};
pub use confirmation::{ConfirmationSystem, OperationContext, OperationType, RiskLevel, ImpactLevel, create_operation_context};
