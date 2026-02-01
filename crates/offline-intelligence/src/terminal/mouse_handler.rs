//! Mouse event handling for terminal interactions
//!
//! This module provides cross-platform mouse support for the CLI,
//! enabling click-to-select functionality and cursor positioning.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use crossterm::{
    event::{poll, read, Event, KeyCode, KeyEvent, MouseButton, MouseEventKind},
};
use colored::Colorize;

/// Represents a mouse event
#[derive(Debug, Clone)]
pub enum MouseEvent {
    LeftClick(ClickPosition),
    RightClick(ClickPosition),
    MiddleClick(ClickPosition),
    ScrollUp(ClickPosition),
    ScrollDown(ClickPosition),
    Move(ClickPosition),
}

/// Represents a click position with line and column coordinates
#[derive(Debug, Clone, Copy)]
pub struct ClickPosition {
    pub line: u16,
    pub column: u16,
}

impl ClickPosition {
    pub fn new(line: u16, column: u16) -> Self {
        Self { line, column }
    }
}

/// Manages mouse event handling and coordinate mapping
pub struct MouseHandler {
    running: Arc<AtomicBool>,
    click_callback: Option<Box<dyn Fn(ClickPosition) -> bool + Send + Sync>>,
}

impl MouseHandler {
    /// Create a new mouse handler
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            click_callback: None,
        }
    }

    /// Set the callback function for handling clicks
    pub fn set_click_callback<F>(&mut self, callback: F)
    where
        F: Fn(ClickPosition) -> bool + Send + Sync + 'static,
    {
        self.click_callback = Some(Box::new(callback));
    }

    /// Start mouse event listening in a background thread
    pub fn start_listening(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.running.store(true, Ordering::Relaxed);
        let running = self.running.clone();
        let has_callback = self.click_callback.is_some();

        // Spawn background thread for mouse events
        thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                if let Ok(true) = poll(Duration::from_millis(50)) {
                    if let Ok(event) = read() {
                        // For now, we'll handle events without the callback
                        // The actual callback handling will be done at the usage site
                        match event {
                            Event::Mouse(mouse_event) => {
                                // Mouse events are captured but handled externally
                            }
                            Event::Key(KeyEvent { code: KeyCode::Char('q'), .. }) => {
                                println!("{}", "\nExiting...".yellow());
                                std::process::exit(0);
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Stop mouse event listening
    pub fn stop_listening(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.running.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Handle individual terminal events
    fn handle_event(event: Event, callback: &Option<Box<dyn Fn(ClickPosition) -> bool + Send + Sync>>) {
        match event {
            Event::Mouse(mouse_event) => {
                let position = ClickPosition::new(mouse_event.row, mouse_event.column);
                
                match mouse_event.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(cb) = callback {
                            cb(position);
                        }
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        // Handle mouse up if needed
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        // Handle drag events
                    }
                    MouseEventKind::ScrollUp => {
                        // Handle scroll up
                    }
                    MouseEventKind::ScrollDown => {
                        // Handle scroll down
                    }
                    _ => {}
                }
            }
            Event::Key(KeyEvent { code: KeyCode::Char('q'), .. }) => {
                // Allow quitting with 'q' key
                println!("{}", "\nExiting...".yellow());
                std::process::exit(0);
            }
            _ => {}
        }
    }

    /// Convert terminal coordinates to conversation index
    /// Assumes conversations start at line 3 (after header)
    pub fn position_to_conversation_index(&self, position: ClickPosition, conversation_count: usize) -> Option<usize> {
        // Conversations start at line 3, each conversation takes 1 line
        let start_line = 3;
        let end_line = start_line + conversation_count as u16;
        
        if position.line >= start_line && position.line < end_line {
            let index = (position.line - start_line) as usize;
            if index < conversation_count {
                Some(index)
            } else {
                None
            }
        } else {
            None
        }
    }
}

impl Default for MouseHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize mouse support globally
pub fn initialize_mouse_support() -> Result<MouseHandler, Box<dyn std::error::Error>> {
    let mut handler = MouseHandler::new();
    handler.start_listening()?;
    Ok(handler)
}

/// Shutdown mouse support and cleanup
pub fn shutdown_mouse_support(handler: &MouseHandler) -> Result<(), Box<dyn std::error::Error>> {
    handler.stop_listening()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_conversion() {
        let handler = MouseHandler::new();
        
        // Test valid positions
        assert_eq!(handler.position_to_conversation_index(ClickPosition::new(3, 5), 10), Some(0));
        assert_eq!(handler.position_to_conversation_index(ClickPosition::new(4, 5), 10), Some(1));
        assert_eq!(handler.position_to_conversation_index(ClickPosition::new(12, 5), 10), Some(9));
        
        // Test invalid positions
        assert_eq!(handler.position_to_conversation_index(ClickPosition::new(2, 5), 10), None); // Before start
        assert_eq!(handler.position_to_conversation_index(ClickPosition::new(13, 5), 10), None); // After end
        assert_eq!(handler.position_to_conversation_index(ClickPosition::new(5, 5), 2), None); // Index out of bounds
    }
}