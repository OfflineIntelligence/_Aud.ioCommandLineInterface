# Aud.io System Design Report

**Version**: 1.0  
**Date**: January 30, 2026  
**Classification**: Internal Technical Documentation  

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Design Principles](#2-design-principles)
3. [System Architecture Overview](#3-system-architecture-overview)
4. [Technology Stack](#4-technology-stack)
5. [Core Components](#5-core-components)
6. [Data Flow Architecture](#6-data-flow-architecture)
7. [Database Design](#7-database-design)
8. [Memory Architecture](#8-memory-architecture)
9. [Concurrency Model](#9-concurrency-model)
10. [API Design](#10-api-design)
11. [Protocols and Interfaces](#11-protocols-and-interfaces)
12. [Security Model](#12-security-model)
13. [Performance Characteristics](#13-performance-characteristics)
14. [Deployment Architecture](#14-deployment-architecture)
15. [Monitoring and Observability](#15-monitoring-and-observability)
16. [Error Handling Strategy](#16-error-handling-strategy)
17. [Dependencies and Integration](#17-dependencies-and-integration)

---

## 1. Executive Summary

Aud.io is a privacy-first, offline-capable AI assistant desktop application built with a zero-dependency architecture. The system employs a **1-hop network architecture** where all components communicate through shared memory, with the sole external network call being to a local `llama-server` process for LLM inference.

### Key Architectural Features:
- **Zero external dependencies** at runtime
- **Single network hop** (localhost only)
- **Thread-based shared memory architecture**
- **Three-tier memory hierarchy** with semantic search
- **Multi-format model support** (GGUF, ONNX, TensorRT, etc.)
- **KV cache management** with intelligent eviction
- **Background persistence** with non-blocking operations

### System Scale:
- **Components**: 17 major modules
- **API Endpoints**: 18 HTTP routes
- **Database Tables**: 6 core tables + indexes
- **Concurrency**: Worker threads + async runtime
- **Supported Models**: 6+ formats via abstraction layer

---

## 2. Design Principles

### 2.1 Privacy First – Everything Local
All computation, data storage, and model inference occur on the user's device. No cloud services, telemetry, or external API calls except for localhost communication with the embedded LLM server.

### 2.2 Zero Network Dependency
The only network communication is a single localhost HTTP hop to `llama-server`. All inter-component communication uses shared memory (`Arc<SharedState>`).

### 2.3 Zero-Waste Computation
- **No separate embedding model**: Uses LLM's `/v1/embeddings` endpoint
- **Conditional retrieval**: Semantic search only triggers when past references detected
- **Lazy initialization**: Subsystems gracefully degrade if unavailable
- **Background persistence**: Database writes never block streaming responses

### 2.4 Functional Composition
All critical operations are pure functions with deterministic behavior under concurrent load. System components can be composed and reconfigured without side effects.

### 2.5 Graceful Degradation
Every advanced feature (semantic search, context engine, cache management) is optional. The system falls back to direct passthrough if any subsystem fails.

---

## 3. System Architecture Overview

### 3.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Tauri Desktop Shell                       │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  Frontend (React/Vite)                                    │  │
│  │  API Client → http://127.0.0.1:8000                       │  │
│  └───────────────────────┬───────────────────────────────────┘  │
│                          │                                      │
└──────────────────────────┼──────────────────────────────────────┘
                           │ HTTP (localhost only)
┌──────────────────────────▼──────────────────────────────────────┐
│                   Rust Backend (Axum Server)                    │
│  Port: 8000                                                     │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │           Shared State (Arc<SharedSystemState>)          │ │
│  │  • Conversations                                          │ │
│  │  • LLM Runtime                                            │ │
│  │  • Cache Manager                                          │ │
│  │  • Database Pool                                          │ │
│  │  • Context Orchestrator                                   │ │
│  └─────────────┬─────────────────────────────────────────────┘ │
│                │                                               │
│  ┌─────────────▼─────────────┐  ┌──────────────────────────┐ │
│  │     Worker Threads        │  │    HTTP Handlers (API)    │ │
│  │                           │  │                          │ │
│  │  • Context Worker         │◄─┤  • Stream API            │ │
│  │  • Cache Worker           │  │  • Conversation API      │ │
│  │  • Database Worker        │  │  • Model API             │ │
│  │  • LLM Worker             │  │  • Search API            │ │
│  └───────────────────────────┘  └──────────────────────────┘ │
└────────────────────────────────────────────────────────────────┘
                           │
                           │ HTTP (localhost)
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                    llama-server Process                         │
│  Port: 8001                                                     │
│  Model: GGUF/GGML/ONNX/TensorRT/Safetensors/CoreML              │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 Component Relationships

```
Main Entry Point (main.rs)
        │
        ▼
    Config Loading
        │
        ▼
Thread Server Initialization
        │
        ├─► Database Initialization
        ├─► Shared State Creation
        ├─► Model Manager Setup
        ├─► Runtime Manager Initialization
        ├─► Worker Thread Pool Startup
        ├─► Context Orchestrator Setup
        ├─► Cache Manager Initialization
        └─► HTTP Server Startup
```

---

## 4. Technology Stack

### 4.1 Backend Stack

| Layer | Technology | Version | Purpose |
|-------|------------|---------|---------|
| **Language** | Rust | 2021 edition | Systems programming |
| **Framework** | Axum | 0.7 | HTTP server |
| **Async Runtime** | Tokio | 1.x | Asynchronous execution |
| **Database** | SQLite | via rusqlite 0.32 | Persistent storage |
| **Connection Pool** | r2d2 | 0.8 | Database connections |
| **Serialization** | Serde | 1.0 | JSON/data serialization |
| **Logging** | Tracing | 0.1 | Structured logging |
| **Metrics** | Prometheus | 0.13 | System metrics |
| **HTTP Client** | Reqwest | 0.12 | External HTTP calls |

### 4.2 Model Runtime Stack

| Format | Runtime | Implementation |
|--------|---------|----------------|
| GGUF | Custom | `gguf_runtime.rs` |
| GGML | Custom | `ggml_runtime.rs` |
| ONNX | Custom | `onnx_runtime.rs` |
| TensorRT | Custom | `tensorrt_runtime.rs` |
| Safetensors | Custom | `safetensors_runtime.rs` |
| CoreML | Custom | `coreml_runtime.rs` |

### 4.3 Frontend Stack (Reference)

| Technology | Version | Purpose |
|------------|---------|---------|
| React | 19.2.0 | UI Framework |
| TypeScript | 5.9.3 | Type safety |
| Vite | 7.2.4 | Build tool |
| TailwindCSS | 4.1.18 | Styling |
| Tauri | 2.2.1 | Desktop shell |

---

## 5. Core Components

### 5.1 Shared State Management

**Module**: `shared_state.rs`  
**Purpose**: Central coordination point for all system components

#### Key Structures:
```rust
pub struct SharedSystemState {
    pub conversations: Arc<ConversationHierarchy>,
    pub llm_runtime: Arc<RwLock<Option<LLMRuntime>>>,
    pub cache_manager: Arc<RwLock<Option<Arc<KVCacheManager>>>>,
    pub database_pool: Arc<MemoryDatabase>,
    pub config: Arc<Config>,
    pub counters: Arc<AtomicCounters>,
    pub context_orchestrator: Arc<tokio::sync::RwLock<Option<ContextOrchestrator>>>,
}
```

#### Concurrency Primitives Used:
- `Arc<T>` - Shared ownership across threads
- `RwLock` (std/tokio) - Reader-writer locks
- `DashMap` - Lock-free concurrent HashMap
- `AtomicUsize` - Lock-free counters

### 5.2 HTTP API Layer

**Module**: `api/`  
**Purpose**: External interface for frontend communication

#### Endpoints:
| Method | Path | Handler | Purpose |
|--------|------|---------|---------|
| POST | `/generate/stream` | `stream_api::generate_stream` | Main chat streaming |
| POST | `/generate/title` | `title_api::generate_title` | Auto-generate titles |
| GET | `/conversations` | `conversation_api::get_conversations` | List conversations |
| GET | `/conversations/:id` | `conversation_api::get_conversation` | Get conversation |
| PUT | `/conversations/:id/title` | `conversation_api::update_conversation_title` | Update title |
| POST | `/conversations/:id/pinned` | `conversation_api::update_conversation_pinned` | Pin/unpin |
| DELETE | `/conversations/:id` | `conversation_api::delete_conversation` | Delete conversation |
| POST | `/models/*` | Various | Model management |
| POST | `/search` | `search_api::search` | Semantic search |
| GET | `/healthz` | Health check | System status |
| GET | `/metrics` | Metrics endpoint | Prometheus metrics |

### 5.3 Database Layer

**Module**: `memory_db/`  
**Purpose**: Persistent storage for conversations, embeddings, and metadata

#### Schema Overview:
```sql
-- Core tables
sessions          -- Conversation sessions
messages          -- Individual messages
summaries         -- Compressed conversation segments
details           -- Preserved message details
embeddings        -- 384-dim semantic vectors
kv_snapshots      -- KV cache snapshots
kv_cache_entries  -- Individual cache entries
kv_cache_metadata -- Cache statistics
```

#### Key Stores:
- **ConversationStore**: Session and message CRUD
- **SummaryStore**: Tier-2 compressed storage
- **EmbeddingStore**: Vector storage with HNSW index
- **Transaction Manager**: ACID guarantees

### 5.4 Context Engine

**Module**: `context_engine/`  
**Purpose**: Intelligent context retrieval and optimization

#### Sub-components:
1. **RetrievalPlanner** (`retrieval_planner.rs`)
   - Analyzes queries for past references
   - Creates retrieval plans based on context needs
   
2. **TierManager** (`tier_manager.rs`)
   - Manages three-tier storage hierarchy
   - Handles data movement between tiers
   
3. **ContextBuilder** (`context_builder.rs`)
   - Constructs optimal context from retrieved content
   - Balances token limits and relevance
   
4. **ContextOrchestrator** (`orchestrator.rs`)
   - Coordinates all context subsystems
   - Main entry point for context operations

### 5.5 Cache Management

**Module**: `cache_management/`  
**Purpose**: KV cache lifecycle and intelligent management

#### Key Components:
- **CacheManager**: Orchestrates cache clear/preserve decisions
- **CacheExtractor**: Captures KV cache state from LLM
- **CacheScorer**: Ranks cache entries by importance
- **CacheBridge**: Transitions between cache states

#### Strategies:
- **SnapshotStrategy**: When to create KV snapshots
- **RetrievalStrategy**: When to restore from cache
- **ClearReason**: Intelligent eviction policies

### 5.6 Model Runtime System

**Module**: `model_runtime/`  
**Purpose**: Multi-format model hosting abstraction

#### Architecture:
```
Runtime Trait ← GGUF Runtime
            ← ONNX Runtime
            ← TensorRT Runtime
            ← Safetensors Runtime
            ← GGML Runtime
            ← CoreML Runtime
```

#### Key Features:
- Automatic format detection
- Unified OpenAI-compatible API
- Cross-platform support
- Resource-aware loading

### 5.7 Worker Thread System

**Module**: `worker_threads/`  
**Purpose**: Dedicated threads for specialized operations

#### Worker Types:
1. **ContextWorker**: Handles context engine operations
2. **CacheWorker**: Manages cache extraction and restoration
3. **DatabaseWorker**: Handles database persistence
4. **LLMWorker**: Proxies requests to llama-server

#### Communication Pattern:
- Message passing via channels
- Non-blocking operations
- Background task execution

---

## 6. Data Flow Architecture

### 6.1 Primary Chat Flow

```
1. Client Request
   ↓
2. Stream API Handler (/generate/stream)
   ↓
3. Session Lookup/Creation (SharedState)
   ↓
4. User Message Persistence (DatabaseWorker - Background)
   ↓
5. Context Engine Processing
   ├── Retrieval Planner analyzes query
   ├── Tier Manager checks caches
   ├── Semantic Search (if needed)
   └── Context Builder optimizes context
   ↓
6. LLM Worker (HTTP to localhost:8001)
   ↓
7. Streaming Response (SSE to client)
   ↓
8. Assistant Response Persistence (Background)
   ↓
9. Embedding Generation (Background)
```

### 6.2 Semantic Search Flow

```
1. Query Analysis
   ↓
2. Past Reference Detection
   ↓
3. Embedding Generation (Query → llama-server)
   ↓
4. HNSW Index Search
   ↓
5. Similarity Scoring
   ↓
6. Result Ranking
   ↓
7. Context Integration
```

### 6.3 Cache Lifecycle

```
1. Cache Extraction
   ├── LLM generates response
   ├── CacheExtractor captures KV state
   └── Snapshot stored in database
   ↓
2. Cache Storage
   ├── Tier-1: Hot cache (memory)
   ├── Tier-2: Compressed summaries
   └── Tier-3: Database snapshots
   ↓
3. Cache Retrieval
   ├── CacheManager evaluates need
   ├── Restore strategy selection
   └── Cache state reconstruction
   ↓
4. Cache Eviction
   ├── Intelligent scoring
   ├── Size/age considerations
   └── Snapshot cleanup
```

---

## 7. Database Design

### 7.1 Schema Diagram

```
sessions (1)
    │
    ├── messages (*) ─── embeddings (1:1)
    │                   │
    │                   └── kv_cache_entries (*)
    │
    ├── summaries (*)
    │
    └── details (*)
```

### 7.2 Table Specifications

#### Sessions Table
```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL,
    last_accessed TIMESTAMP NOT NULL,
    metadata TEXT NOT NULL  -- JSON metadata
);
```

#### Messages Table
```sql
CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    message_index INTEGER NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    tokens INTEGER NOT NULL,
    timestamp TIMESTAMP NOT NULL,
    importance_score REAL NOT NULL DEFAULT 0.5,
    embedding_generated BOOLEAN NOT NULL DEFAULT FALSE,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
```

#### Embeddings Table
```sql
CREATE TABLE embeddings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL,
    embedding BLOB NOT NULL,  -- 384-dim f32 vector
    embedding_model TEXT NOT NULL,
    generated_at TIMESTAMP NOT NULL,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
);
```

#### KV Snapshots Table
```sql
CREATE TABLE kv_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    message_id INTEGER NOT NULL,
    kv_state BLOB NOT NULL,      -- Serialized cache state
    kv_state_hash TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
```

### 7.3 Indexing Strategy

```sql
-- Performance indexes
CREATE INDEX idx_messages_session ON messages (session_id);
CREATE INDEX idx_messages_timestamp ON messages (timestamp);
CREATE INDEX idx_embeddings_message ON embeddings (message_id);
CREATE INDEX idx_kv_snapshots_session ON kv_snapshots (session_id);
```

### 7.4 Migration System

**Module**: `migration.rs`  
**Versions**: 3 migrations applied automatically

1. **001_initial.sql**: Base schema creation
2. **002_add_embeddings.sql**: Embedding storage
3. **003_add_kv_snapshots.sql**: KV cache snapshots

---

## 8. Memory Architecture

### 8.1 Three-Tier Hierarchy

```
Tier 1: Hot Cache (RAM)
├── Current conversation context
├── Recently accessed messages
└── Active KV cache state
│   Capacity: Configurable (default: session-based)
│   Access: Sub-millisecond

Tier 2: Warm Cache (Compressed)
├── Compressed conversation summaries
├── Frequently accessed historical data
└── Metadata and indices
│   Capacity: Database-constrained
│   Access: Millisecond range

Tier 3: Cold Storage (Persistent)
├── Full conversation history
├── All embeddings
├── KV snapshots
└── System metadata
│   Capacity: Disk-constrained
│   Access: 10-100ms (SSD)
```

### 8.2 Memory Management Policies

#### Tier 1 (Hot Cache):
- **Eviction**: LRU with temporal weighting
- **Size**: Configurable max tokens
- **Persistence**: Volatile (lost on restart)

#### Tier 2 (Warm Cache):
- **Eviction**: Frequency-based compression
- **Size**: Configurable percentage of Tier 3
- **Persistence**: Compressed to database

#### Tier 3 (Cold Storage):
- **Eviction**: Age-based cleanup
- **Size**: Unlimited (disk capacity)
- **Persistence**: Permanent SQLite storage

### 8.3 KV Cache Management

#### Snapshot Creation:
- Triggered after assistant responses
- Captures full KV state from LLM
- Stored as serialized binary blobs
- Hash-based deduplication

#### Restoration Strategy:
- **Exact Match**: Restore from identical snapshot
- **Partial Match**: Merge with current context
- **Fallback**: Regenerate from scratch

#### Scoring Algorithm:
```rust
score = α × recency + β × frequency + γ × importance + δ × size_efficiency
```

---

## 9. Concurrency Model

### 9.1 Threading Architecture

```
Main Thread
├── Axum HTTP Server
├── Shared State Coordinator
└── Worker Thread Pool
    ├── Context Worker Thread
    ├── Cache Worker Thread
    ├── Database Worker Thread
    └── LLM Worker Thread
```

### 9.2 Shared Memory Patterns

#### Arc-Based Sharing:
```rust
// Shared immutable data
Arc<Config>
Arc<MemoryDatabase>
Arc<KVCacheManager>

// Shared mutable data
Arc<RwLock<Option<LLMRuntime>>>
Arc<tokio::sync::RwLock<Option<ContextOrchestrator>>>
Arc<DashMap<String, SessionData>>
```

#### Locking Strategy:
- **Read-heavy**: RwLock for infrequent writes
- **Write-heavy**: DashMap for lock-free operations
- **Counters**: Atomic primitives for zero-lock operations

### 9.3 Async/Await Usage

#### Where Async is Used:
- Database operations (connection pooling)
- HTTP requests to llama-server
- File I/O operations
- Background tasks

#### Where Blocking is Acceptable:
- CPU-bound computations
- Cache operations
- In-memory data structures

### 9.4 Message Passing

#### Channels Used:
- `tokio::sync::mpsc` - Multi-producer, single consumer
- `tokio::sync::oneshot` - One-time response channels
- `crossbeam::ArrayQueue` - Lock-free bounded queues

#### Worker Communication:
```
API Handler → Channel → Worker Thread → Background Task
```

---

## 10. API Design

### 10.1 RESTful Principles

#### Resource-Oriented Design:
- `/conversations` - Collection of chat sessions
- `/conversations/{id}` - Individual session
- `/models` - Model management
- `/search` - Search operations

#### HTTP Methods Mapping:
- `GET` - Retrieve resources
- `POST` - Create resources/actions
- `PUT` - Update entire resources
- `PATCH` - Partial updates
- `DELETE` - Remove resources

### 10.2 Request/Response Format

#### Stream API Request:
```json
{
  "model": "local-llm",
  "messages": [
    {"role": "user", "content": "Hello"}
  ],
  "session_id": "session-123",
  "max_tokens": 2000,
  "temperature": 0.7,
  "stream": true
}
```

#### Stream API Response (SSE):
```text
data: {"choices": [{"delta": {"content": "Hello"}}]}
data: {"choices": [{"delta": {"content": " world"}}]}
data: [DONE]
```

### 10.3 Error Handling

#### Standard Error Format:
```json
{
  "error": {
    "type": "validation_error",
    "message": "Invalid session ID",
    "code": 400
  }
}
```

#### Status Codes:
- `200 OK` - Success
- `400 Bad Request` - Invalid input
- `404 Not Found` - Resource missing
- `500 Internal Server Error` - System error
- `503 Service Unavailable` - Temporary unavailability

---

## 11. Protocols and Interfaces

### 11.1 Internal Protocols

#### Shared State Protocol:
- **Access Pattern**: Arc-wrapped structs
- **Mutability**: RwLock for controlled mutation
- **Lifetime**: Application lifetime

#### Worker Communication Protocol:
- **Message Format**: Rust enums/structs
- **Transport**: Tokio channels
- **Guarantees**: At-least-once delivery

#### Database Protocol:
- **Interface**: Trait-based store abstractions
- **Transactions**: Explicit begin/commit/rollback
- **Connection**: Pooled connections (max 10)

### 11.2 External Protocols

#### HTTP API Protocol:
- **Version**: HTTP/1.1
- **Format**: JSON for requests/responses
- **Streaming**: Server-Sent Events (SSE)
- **Authentication**: None (localhost only)

#### LLM Server Protocol:
- **Compatibility**: OpenAI API v1
- **Endpoints**: `/v1/chat/completions`, `/v1/embeddings`
- **Format**: JSON with streaming support

### 11.3 Serialization Formats

#### JSON:
- API requests/responses
- Configuration files
- Database metadata storage

#### Binary:
- Embedding vectors (384-dim f32)
- KV cache snapshots
- Model weights (external)

#### Custom Formats:
- Cache state serialization (bincode)
- Message packaging (custom structs)

---

## 12. Security Model

### 12.1 Threat Model

#### Assumptions:
- Trusted local environment
- No network exposure except localhost
- User controls their own data

#### Mitigated Risks:
- Data exfiltration (everything local)
- Network interception (no external traffic)
- Authentication bypass (no auth required)

#### Remaining Risks:
- Local privilege escalation
- Disk encryption concerns
- Physical device security

### 12.2 Security Controls

#### Network Security:
- Bind only to `127.0.0.1`
- No external listening ports
- CORS permissive (localhost only)

#### Data Security:
- Local filesystem storage only
- No cloud synchronization
- User-controlled data location

#### Access Control:
- Implicit user trust (single-user system)
- No multi-user isolation
- File permissions via OS

### 12.3 Privacy Guarantees

#### Data Processing:
- All computation on-device
- No telemetry collection
- No analytics transmission
- No third-party services

#### Data Storage:
- Configurable storage location
- User-controlled backup
- No automatic data sharing
- Clear data ownership

---

## 13. Performance Characteristics

### 13.1 Latency Benchmarks

#### 1-Hop Architecture Benefits:
- **API Handler to Shared State**: ~100ns (Arc dereference)
- **Shared State to Database**: ~1μs (connection pool)
- **Database to LLM Worker**: ~50μs (channel message)
- **LLM Worker to llama-server**: ~1ms (localhost HTTP)
- **Total Internal Latency**: <2ms

#### Streaming Performance:
- **First Token Time**: 50-200ms (model dependent)
- **Tokens/Second**: 20-50 (depending on hardware)
- **End-to-End Latency**: 100ms + generation time

### 13.2 Throughput Metrics

#### Concurrent Connections:
- **Max Streams**: Configurable (default: 4)
- **Queue Size**: Configurable (default: 100)
- **Requests/Second**: 24 (configurable)

#### Database Performance:
- **Writes**: 1000+/second (batched)
- **Reads**: 5000+/second (indexed)
- **Search**: 100+/second (HNSW index)

### 13.3 Resource Utilization

#### Memory Usage:
- **Base Memory**: 50-100MB (application)
- **Model Memory**: 2-16GB (depending on model)
- **Cache Memory**: 100MB-1GB (configurable)
- **Database Memory**: 10-100MB (SQLite cache)

#### CPU Usage:
- **Idle**: ~1-5% (background maintenance)
- **Streaming**: 80-100% (GPU/CPU bound)
- **Background Tasks**: 5-15% (persistence/indexing)

#### Disk Usage:
- **Database Growth**: ~100KB-1MB per conversation
- **KV Snapshots**: ~10-100MB per active session
- **Log Files**: Configurable rotation

---

## 14. Deployment Architecture

### 14.1 Single-Binary Distribution

#### Packaging Strategy:
- **Self-contained executable**: All dependencies bundled
- **Embedded resources**: Models, binaries, configs
- **Zero-install**: Copy and run
- **Cross-platform**: Windows, macOS, Linux builds

#### Directory Structure:
```
aud.io-app/
├── aud.io.exe          # Main executable
├── resources/
│   ├── models/         # Embedded models
│   ├── bin/            # Runtime binaries
│   └── config/         # Default configurations
└── data/               # User data directory
    └── conversations.db # SQLite database
```

### 14.2 Runtime Requirements

#### Minimum System:
- **CPU**: 4-core modern processor
- **RAM**: 8GB (4GB minimum)
- **Disk**: 10GB free space
- **OS**: Windows 10+, macOS 12+, Linux kernel 5.4+

#### Recommended System:
- **CPU**: 8-core with AVX2 support
- **RAM**: 16GB+
- **GPU**: CUDA-compatible (NVIDIA) or Metal (Apple)
- **Disk**: SSD with 20GB+ free

### 14.3 Configuration Management

#### Environment Variables:
```bash
# Core settings
MODEL_PATH=/path/to/model.gguf
LLAMA_BIN=/path/to/llama-server
API_PORT=8000
LLAMA_PORT=8001

# Performance tuning
GPU_LAYERS=20
CTX_SIZE=8192
THREADS=8
BATCH_SIZE=512

# Advanced settings
MAX_CONCURRENT_STREAMS=4
REQUESTS_PER_SECOND=24
```

#### Configuration Precedence:
1. Environment variables
2. `.env` file
3. Compiled defaults
4. Auto-detection

---

## 15. Monitoring and Observability

### 15.1 Logging System

#### Tracing Implementation:
- **Framework**: `tracing` crate
- **Format**: Structured JSON/logs
- **Levels**: DEBUG, INFO, WARN, ERROR
- **Fields**: Timestamp, target, level, message, spans

#### Log Categories:
```rust
// Component-specific targets
axum::server          // HTTP server logs
memory_db::store      // Database operations
context_engine::plan  // Context planning
cache_manager::ops    // Cache operations
llm_worker::stream    // LLM interactions
```

### 15.2 Metrics Collection

#### Prometheus Metrics:
```rust
// Counter metrics
requests_total{route="/generate/stream", status="200"}
cache_hits_total
cache_misses_total

// Gauge metrics
active_sessions
queue_depth
database_connections

// Histogram metrics
request_duration_seconds
queue_wait_time_seconds
```

#### Custom Metrics:
- Session counts and activity
- Cache hit/miss ratios
- Database performance
- LLM response times

### 15.3 Health Monitoring

#### Health Check Endpoint:
```
GET /healthz
← 200 OK "OK"
```

#### Health Indicators:
- Database connectivity
- LLM server availability
- Worker thread status
- Resource utilization

#### Readiness Probes:
- All subsystems initialized
- Model loaded and responding
- Database migrations complete
- Required ports available

---

## 16. Error Handling Strategy

### 16.1 Error Classification

#### Recoverable Errors:
- Temporary database locks
- Network timeouts to LLM
- Cache miss scenarios
- Missing optional features

#### Non-Recoverable Errors:
- Critical system initialization failures
- Corrupted database
- Missing required binaries
- Insufficient system resources

### 16.2 Failure Recovery

#### Graceful Degradation:
```rust
// Feature fallback chain
context_engine → semantic_search → direct_passthrough
cache_manager → snapshot_restore → regenerate
database_ops → in_memory_cache → temporary_failure
```

#### Retry Logic:
- **Database operations**: Exponential backoff (3 retries)
- **LLM requests**: Linear backoff (2 retries)
- **Cache operations**: Immediate retry once

#### Circuit Breaker Pattern:
- Track failure rates per subsystem
- Temporarily disable failing components
- Gradual recovery testing

### 16.3 Error Reporting

#### User-Facing Errors:
- Clear, actionable messages
- No internal implementation details
- Suggested recovery steps
- Error codes for support

#### Developer Logs:
- Full error chains with context
- Stack traces for panics
- Performance impact measurements
- Correlation IDs for tracing

---

## 17. Dependencies and Integration

### 17.1 External Dependencies

#### Runtime Dependencies:
- **None** - Fully self-contained
- All libraries statically linked
- No dynamic linking requirements

#### Build Dependencies:
- **Rust toolchain**: 1.70+
- **Cargo**: Package management
- **LLVM**: For compilation (macOS/Linux)

#### Optional Integrations:
- **CUDA toolkit**: For NVIDIA GPU acceleration
- **Metal**: For Apple GPU acceleration
- **OpenCL**: For alternative GPU backends

### 17.2 Internal Module Dependencies

#### Core Dependency Graph:
```
main.rs
├── config.rs
├── thread_server.rs
│   ├── shared_state.rs
│   ├── memory_db/
│   ├── context_engine/
│   ├── cache_management/
│   └── worker_threads/
├── api/
│   ├── stream_api.rs
│   ├── conversation_api.rs
│   └── model_api.rs
└── model_runtime/
    ├── runtime_manager.rs
    └── format-specific runtimes
```

#### Circular Dependency Prevention:
- Clear module boundaries
- Trait-based abstractions
- Dependency injection patterns
- Compile-time enforcement

### 17.3 Version Compatibility

#### Backward Compatibility:
- Database schema migrations
- API versioning strategy
- Config file evolution
- Model format support

#### Forward Compatibility:
- Extensible configuration
- Plugin architecture groundwork
- Modular design for future features
- Semantic versioning adherence

---

## Appendix A: System Configuration Reference

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MODEL_PATH` | Auto-detect | Path to LLM model file |
| `LLAMA_BIN` | Required | Path to llama-server binary |
| `API_HOST` | 127.0.0.1 | HTTP server bind address |
| `API_PORT` | 8000 | HTTP server port |
| `LLAMA_HOST` | 127.0.0.1 | LLM server bind address |
| `LLAMA_PORT` | 8001 | LLM server port |
| `GPU_LAYERS` | auto | Number of GPU layers |
| `CTX_SIZE` | auto | Context window size |
| `THREADS` | auto | CPU thread count |
| `BATCH_SIZE` | auto | Processing batch size |

### Performance Tuning Parameters

| Parameter | Impact | Tuning Guidance |
|-----------|--------|-----------------|
| `MAX_CONCURRENT_STREAMS` | Throughput | Match CPU cores |
| `REQUESTS_PER_SECOND` | Rate limiting | Based on hardware |
| `CACHE_SIZE_MB` | Memory usage | 10-20% of RAM |
| `DB_CONNECTION_POOL` | Database perf | 5-20 connections |
| `QUEUE_SIZE` | Backpressure | 50-200 items |

---

## Appendix B: Troubleshooting Guide

### Common Issues and Solutions

#### LLM Server Not Starting:
1. Verify `LLAMA_BIN` path exists
2. Check model file permissions
3. Ensure sufficient VRAM/CPU memory
4. Validate model format compatibility

#### Database Connection Failures:
1. Check `data/` directory permissions
2. Verify disk space availability
3. Restart application to reset connections
4. Check for database corruption

#### Performance Bottlenecks:
1. Monitor resource utilization
2. Adjust `THREADS` and `BATCH_SIZE`
3. Optimize `CTX_SIZE` for model
4. Consider GPU acceleration

#### Cache Issues:
1. Clear cache via API or restart
2. Check cache size configuration
3. Review cache hit/miss metrics
4. Validate snapshot integrity

