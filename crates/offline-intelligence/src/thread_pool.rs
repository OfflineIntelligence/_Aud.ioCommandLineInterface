//! Thread pool management for worker threads
//!
//! This module provides the infrastructure for managing dedicated worker threads
//! for different system components, enabling efficient parallel processing.

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle};
use tokio::sync::mpsc;
use tracing::{info, error};

use crate::{
    shared_state::SharedState,
    config::Config,
};

/// Configuration for thread pool sizing
#[derive(Debug, Clone)]
pub struct ThreadPoolConfig {
    pub context_engine_threads: usize,
    pub cache_manager_threads: usize,
    pub database_threads: usize,
    pub llm_threads: usize,
    pub io_threads: usize,
}

impl ThreadPoolConfig {
    pub fn new(config: &Config) -> Self {
        // Scale thread counts based on system resources
        let cpu_cores = num_cpus::get();
        
        Self {
            context_engine_threads: (cpu_cores / 4).max(2).min(4),
            cache_manager_threads: 1.max(cpu_cores / 8).min(2),
            database_threads: config.max_concurrent_streams as usize,
            llm_threads: 1, // LLM inference is typically single-threaded per model
            io_threads: (cpu_cores / 2).max(2).min(4),
        }
    }
}

/// System-wide command types for thread communication
pub enum SystemCommand {
    // Conversation operations
    ProcessMessage {
        session_id: String,
        message: crate::memory::Message,
        sender: Box<dyn FnOnce(anyhow::Result<crate::memory::Message>) + Send>,
    },
    
    // LLM operations
    GenerateResponse {
        session_id: String,
        context: Vec<crate::memory::Message>,
        sender: Box<dyn FnOnce(anyhow::Result<String>) + Send>,
    },
    
    // Cache operations
    UpdateCache {
        session_id: String,
        entries: Vec<crate::cache_management::cache_extractor::KVEntry>,
        sender: Box<dyn FnOnce(anyhow::Result<()>) + Send>,
    },
    
    // Database operations
    PersistConversation {
        session_id: String,
        messages: Vec<crate::memory::Message>,
        sender: Box<dyn FnOnce(anyhow::Result<()>) + Send>,
    },
    
    // Administrative operations
    Shutdown,
}

/// Worker thread implementation
pub struct WorkerThread {
    command_receiver: mpsc::UnboundedReceiver<SystemCommand>,
    shared_state: Arc<SharedState>,
    thread_handle: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
}

impl WorkerThread {
    pub fn new(
        name: String,
        command_receiver: mpsc::UnboundedReceiver<SystemCommand>,
        shared_state: Arc<SharedState>,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let shared_state_clone = shared_state.clone();
        
        let thread_handle = thread::Builder::new()
            .name(name.clone())
            .spawn({
                let receiver = command_receiver; // Move receiver into closure
                move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to create worker thread runtime");
                    
                    rt.block_on(async move {
                        Self::run_worker_loop(receiver, shared_state_clone, running_clone).await;
                    });
                }
            })
            .expect("Failed to spawn worker thread");
        
        info!("Spawned worker thread: {}", name);
        
        Self {
            command_receiver: mpsc::unbounded_channel().1, // Create dummy receiver
            shared_state,
            thread_handle: Some(thread_handle),
            running,
        }
    }
    
    async fn run_worker_loop(
        mut receiver: mpsc::UnboundedReceiver<SystemCommand>,
        shared_state: Arc<SharedState>,
        running: Arc<AtomicBool>,
    ) {
        while running.load(Ordering::Relaxed) {
            tokio::select! {
                command = receiver.recv() => {
                    match command {
                        Some(cmd) => {
                            if let Err(e) = Self::handle_command(cmd, &shared_state).await {
                                error!("Worker thread command failed: {}", e);
                            }
                        }
                        None => {
                            info!("Worker thread command channel closed");
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                    // Periodic maintenance tasks could go here
                }
            }
        }
        
        info!("Worker thread shutting down");
    }
    
    async fn handle_command(
        command: SystemCommand,
        shared_state: &Arc<SharedState>,
    ) -> anyhow::Result<()> {
        match command {
            SystemCommand::ProcessMessage { session_id, message, sender } => {
                let result = Self::process_message(shared_state, session_id, message).await;
                sender(result);
            }
            SystemCommand::GenerateResponse { session_id, context, sender } => {
                let result = Self::generate_response(shared_state, session_id, context).await;
                sender(result);
            }
            SystemCommand::UpdateCache { session_id, entries, sender } => {
                let result = Self::update_cache(shared_state, session_id, entries).await;
                sender(result);
            }
            SystemCommand::PersistConversation { session_id, messages, sender } => {
                let result = Self::persist_conversation(shared_state, session_id, messages).await;
                sender(result);
            }
            SystemCommand::Shutdown => {
                // Graceful shutdown handled by running flag
            }
        }
        Ok(())
    }
    
    async fn process_message(
        shared_state: &Arc<SharedState>,
        session_id: String,
        message: crate::memory::Message,
    ) -> anyhow::Result<crate::memory::Message> {
        // Get or create session
        let session = shared_state.get_or_create_session(&session_id).await;
        let mut session_guard = session.write()
            .map_err(|_| anyhow::anyhow!("Failed to acquire session write lock"))?;
        
        // Add message to session
        session_guard.messages.push(message.clone());
        session_guard.last_accessed = std::time::Instant::now();
        
        // Update metrics
        shared_state.counters.inc_processed_messages();
        
        Ok(message)
    }
    
    async fn generate_response(
        _shared_state: &Arc<SharedState>,
        _session_id: String,
        _context: Vec<crate::memory::Message>,
    ) -> anyhow::Result<String> {
        // Placeholder for LLM integration
        // In full implementation, this would call the direct llama.cpp interface
        Ok("Generated response placeholder".to_string())
    }
    
    async fn update_cache(
        shared_state: &Arc<SharedState>,
        session_id: String,
        entries: Vec<crate::cache_management::cache_extractor::KVEntry>,
    ) -> anyhow::Result<()> {
        let cache_guard = shared_state.cache_manager.read()
            .map_err(|_| anyhow::anyhow!("Failed to acquire cache manager read lock"))?;
        if let Some(_cache_manager) = &*cache_guard {
            // Update cache with new entries
            // Implementation would depend on the specific cache manager API
            info!("Updating cache for session {} with {} entries", session_id, entries.len());
        }
        Ok(())
    }
    
    async fn persist_conversation(
        _shared_state: &Arc<SharedState>,
        session_id: String,
        messages: Vec<crate::memory::Message>,
    ) -> anyhow::Result<()> {
        // Persist to database using shared connection pool
        info!("Persisting conversation {} with {} messages", session_id, messages.len());
        // Actual implementation would use shared_state.database_pool
        Ok(())
    }
}

impl Drop for WorkerThread {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Thread pool manager for coordinating worker threads
pub struct ThreadPool {
    config: ThreadPoolConfig,
    shared_state: Arc<SharedState>,
    workers: Vec<WorkerThread>,
    command_senders: Vec<mpsc::UnboundedSender<SystemCommand>>,
}

impl ThreadPool {
    pub fn new(config: ThreadPoolConfig, shared_state: Arc<SharedState>) -> Self {
        Self {
            config,
            shared_state,
            workers: Vec::new(),
            command_senders: Vec::new(),
        }
    }
    
    pub async fn start(&mut self) -> anyhow::Result<()> {
        info!("Starting thread pool with config: {:?}", self.config);
        
        // Create command channels
        let mut channels = Vec::new();
        for i in 0..self.config.context_engine_threads {
            let (tx, rx) = mpsc::unbounded_channel();
            channels.push((format!("context-worker-{}", i), tx, rx));
        }
        
        // Spawn worker threads
        for (name, tx, rx) in channels {
            let worker = WorkerThread::new(
                name,
                rx,
                self.shared_state.clone(),
            );
            self.workers.push(worker);
            self.command_senders.push(tx);
        }
        
        info!("Thread pool started with {} workers", self.workers.len());
        Ok(())
    }
    
    pub async fn send_command(&self, command: SystemCommand) -> anyhow::Result<()> {
        // Simple round-robin distribution for now
        static NEXT_WORKER: AtomicBool = AtomicBool::new(false);
        let worker_index = if NEXT_WORKER.fetch_xor(true, Ordering::Relaxed) { 0 } else { 1 };
        let sender_index = worker_index % self.command_senders.len();
        
        self.command_senders[sender_index]
            .send(command)
            .map_err(|_| anyhow::anyhow!("Failed to send command to worker thread"))
    }
    
    pub async fn shutdown(&mut self) -> anyhow::Result<()> {
        info!("Shutting down thread pool");
        
        // Send shutdown commands
        for sender in &self.command_senders {
            let _ = sender.send(SystemCommand::Shutdown);
        }
        
        // Drop workers to trigger cleanup
        self.workers.clear();
        self.command_senders.clear();
        
        info!("Thread pool shutdown complete");
        Ok(())
    }
}

// Convenience re-exports
pub use self::SystemCommand as Command;