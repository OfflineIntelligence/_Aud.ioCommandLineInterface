//! Terminal interaction utilities for mouse support and enhanced UI
//!
//! This module provides cross-platform terminal interaction capabilities
//! including mouse event handling, cursor positioning, and enhanced
//! visual formatting for the CLI interface.

pub mod mouse_handler;

pub use mouse_handler::{
    MouseHandler,
    MouseEvent,
    ClickPosition,
    initialize_mouse_support,
    shutdown_mouse_support,
};