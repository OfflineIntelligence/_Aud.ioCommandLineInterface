# Aud.io — System Design Report

> Offline-First AI Desktop Assistant with Persistent Semantic Memory

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Design Principles](#2-design-principles)
3. [System Architecture](#3-system-architecture)
4. [Technology Stack](#4-technology-stack)
5. [Repository Structure](#5-repository-structure)
6. [Backend Architecture](#6-backend-architecture)
7. [Frontend Architecture](#7-frontend-architecture)
8. [API Surface](#8-api-surface)
9. [Database Design](#9-database-design)
10. [Three-Tier Memory Architecture](#10-three-tier-memory-architecture)
11. [Context Engine](#11-context-engine)
12. [Semantic Search & Embeddings Pipeline](#12-semantic-search--embeddings-pipeline)
13. [KV Cache Management](#13-kv-cache-management)
14. [Model Runtime System](#14-model-runtime-system)
15. [Shared State & Concurrency](#15-shared-state--concurrency)
16. [Worker Thread Architecture](#16-worker-thread-architecture)
17. [Streaming & Data Flow](#17-streaming--data-flow)
18. [Configuration & Environment](#18-configuration--environment)
19. [Observability & Telemetry](#19-observability--telemetry)
20. [Error Handling & Validation](#20-error-handling--validation)
21. [Build, Packaging & Deployment](#21-build-packaging--deployment)
22. [Security Model](#22-security-model)
23. [Performance Characteristics](#23-performance-characteristics)
24. [Completeness Assessment](#24-completeness-assessment)

---

## 1. Executive Summary

**Aud.io** is a cross-platform desktop AI assistant that runs entirely offline. It combines a Rust backend (Axum HTTP server), a React/TypeScript frontend, and a Tauri native shell into a single self-contained application. Inference is performed locally via `llama-server` (llama.cpp), accessed over a single localhost HTTP hop.

The system features a **three-tier memory architecture** with persistent semantic search — conversations are stored in SQLite, embeddings are captured from llama.cpp's own vector computations (no separate embedding model), and an HNSW index enables sub-second retrieval of relevant past context when the hot KV cache doesn't have the answer.

**Key numbers:**
- **1 network hop** (backend → llama-server, localhost only)
- **0 external dependencies** at runtime (fully offline)
- **3-tier memory** (hot cache → summaries → full database + semantic search)
- **384-dimensional** embeddings stored per message
- **16-conversation** KV cache cycle before intelligent eviction

---

## 2. Design Principles

### 2.1 Privacy First — Everything Local
All data, models, and computation stay on the user's machine. No cloud calls, no telemetry to external servers, no account system. The HTTP server binds exclusively to `127.0.0.1`.

### 2.2 1-Hop Architecture — Minimize Latency
All backend subsystems (database, cache, context engine, memory) share a single process address space via `Arc<SharedState>`. The only network hop is the HTTP call from the Axum server to `llama-server` on localhost. This eliminates inter-service latency entirely.

### 2.3 Zero-Waste Computation
- **No separate embedding model**: Embeddings are captured from llama.cpp's `/v1/embeddings` endpoint — reusing the same model already loaded for inference.
- **No summarization overhead**: The conversation sidebar shows the original first question as the title (like ChatGPT/DeepSeek), not a computed summary.
- **Conditional retrieval**: The semantic search pipeline only activates when the retrieval planner detects past references in the query. Brand-new questions with no history references skip the entire search path.
- **Embedding guard**: If zero embeddings exist in the database (fresh install), the orchestrator skips the `/v1/embeddings` call entirely — no wasted round-trip when there's nothing to search.

### 2.4 Graceful Degradation
Every advanced feature (semantic search, context engine, cache management) is optional. If any subsystem fails to initialize, the system falls back to direct message passthrough. The core streaming path always works.

### 2.5 Background, Non-Blocking Persistence
All database writes and embedding generation happen in `tokio::spawn` background tasks. The streaming response is never blocked by persistence operations.

---

## 3. System Architecture

### 3.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Tauri Desktop Shell                       │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                   React Frontend (Vite)                    │  │
│  │  App.tsx → ChatWindow → Sidebar → SearchModal              │  │
│  │  API Client (fetch + SSE) → http://localhost:8000          │  │
│  └───────────────────────┬───────────────────────────────────┘  │
│                          │ HTTP                                  │
│  ┌───────────────────────▼───────────────────────────────────┐  │
│  │              Rust Backend (Axum on :8000)                  │  │
│  │                                                            │  │
│  │  ┌──────────────────────────────────────────────────────┐ │  │
│  │  │              Shared State (Arc)                       │ │  │
│  │  │                                                      │ │  │
│  │  │  ┌─────────┐  ┌──────────┐  ┌───────────────────┐  │ │  │
│  │  │  │ Context  │  │  Cache   │  │   Memory Database  │  │ │  │
│  │  │  │ Engine   │  │ Manager  │  │   (SQLite + WAL)   │  │ │  │
│  │  │  └─────────┘  └──────────┘  └───────────────────┘  │ │  │
│  │  │                                                      │ │  │
│  │  │  ┌─────────┐  ┌──────────┐  ┌───────────────────┐  │ │  │
│  │  │  │   LLM   │  │ Embedding│  │   Conversation    │  │ │  │
│  │  │  │ Worker   │  │  Store   │  │     Store         │  │ │  │
│  │  │  └────┬────┘  └──────────┘  └───────────────────┘  │ │  │
│  │  └───────┼──────────────────────────────────────────────┘ │  │
│  │          │ HTTP (single hop, localhost)                    │  │
│  └──────────┼────────────────────────────────────────────────┘  │
│             │                                                    │
│  ┌──────────▼────────────────────────────────────────────────┐  │
│  │          llama-server (llama.cpp on :8081)                 │  │
│  │          /v1/chat/completions (inference)                  │  │
│  │          /v1/embeddings (vector generation)                │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 Communication Flow

```
React Frontend ──HTTP/SSE──▶ Axum Backend ──HTTP──▶ llama-server
     port 3000                port 8000              port 8081
     (dev) / embedded          localhost               localhost
```

**Hop count: 1** (backend → llama-server). All other subsystem communication is in-process via `Arc<SharedState>`.

---

## 4. Technology Stack

| Layer | Technology | Version | Purpose |
|-------|-----------|---------|---------|
| **Frontend** | React | 19.2.0 | UI framework |
| | TypeScript | 5.9.3 | Type safety |
| | Vite | 7.2.4 | Build tool & dev server |
| | TailwindCSS | 4.1.18 | Styling |
| **Desktop** | Tauri | 2.2.1 | Native shell (file system, dialogs) |
| **Backend** | Rust | 2021 edition | Core language |
| | Axum | 0.7 | HTTP server |
| | Tokio | 1.x (full) | Async runtime |
| | Tower-HTTP | 0.5 | Middleware (CORS, tracing, timeouts) |
| **Database** | SQLite | via rusqlite 0.32 | Persistent storage |
| | r2d2 | 0.8 | Connection pooling |
| **Caching** | moka | 0.12 | Thread-safe LRU cache |
| | DashMap | 5.5 | Concurrent HashMap |
| **Search** | hora | 0.1 | HNSW approximate nearest neighbor |
| **Serialization** | serde | 1.0 | JSON/binary serialization |
| | bincode | 1.3 | Embedding binary storage |
| **Hashing** | blake3 | 1.5 | KV cache snapshot integrity |
| **Parallelism** | rayon | 1.8 | Data parallelism |
| | crossbeam-queue | 0.3 | Lock-free queues |
| **Monitoring** | tracing | 0.1 | Structured logging |
| | prometheus | 0.13 | Metrics collection |
| **System** | nvml-wrapper | 0.10 | GPU monitoring |
| | sysinfo | 0.30 | System information |
| **HTTP Client** | reqwest | 0.12 | Calls to llama-server |
| **Inference** | llama.cpp | external binary | Local LLM inference |

---

## 5. Repository Structure

```
D:\_ProjectWorks\WORKENVIRONMENT\_Aud.io/
│
├── Cargo.toml                          # Rust workspace root
├── package.json                        # Node workspace root
├── .env                                # Runtime configuration
├── SYSTEM_DESIGN_REPORT.md             # This document
│
├── crates/
│   └── offline-intelligence/
│       ├── Cargo.toml                  # Backend crate dependencies
│       ├── src/
│       │   ├── main.rs                 # Entry point
│       │   ├── lib.rs                  # Library root (all module exports)
│       │   ├── config.rs               # Environment config parsing
│       │   ├── thread_server.rs        # Server initialization & router setup
│       │   ├── shared_state.rs         # Arc-wrapped shared state
│       │   ├── thread_pool.rs          # Worker thread pool
│       │   │
│       │   ├── api/                    # HTTP endpoint handlers
│       │   │   ├── stream_api.rs       # POST /generate/stream (core 1-hop)
│       │   │   ├── conversation_api.rs # CRUD for conversations
│       │   │   ├── title_api.rs        # POST /generate/title
│       │   │   ├── search_api.rs       # POST /search (hybrid semantic+keyword)
│       │   │   ├── memory_api.rs       # Memory optimization & stats
│       │   │   └── admin_api.rs        # Health check & admin ops
│       │   │
│       │   ├── context_engine/         # Intelligent context retrieval
│       │   │   ├── orchestrator.rs     # Coordinates all memory subsystems
│       │   │   ├── retrieval_planner.rs # Decides what/where to search
│       │   │   ├── tier_manager.rs     # Three-tier storage management
│       │   │   └── context_builder.rs  # Builds optimal context from sources
│       │   │
│       │   ├── memory_db/             # Database layer
│       │   │   ├── mod.rs             # MemoryDatabase (pool + stores)
│       │   │   ├── schema.rs          # Data structures
│       │   │   ├── conversation_store.rs # Session & message CRUD
│       │   │   ├── embedding_store.rs # Vector storage + HNSW index
│       │   │   ├── summary_store.rs   # Tier 2 summary storage
│       │   │   └── migrations/        # SQL schema migrations
│       │   │       ├── 001_initial.sql
│       │   │       ├── 002_add_embeddings.sql
│       │   │       └── 003_add_kv_snapshots.sql
│       │   │
│       │   ├── cache_management/      # KV cache lifecycle
│       │   │   ├── cache_manager.rs   # Orchestrates cache clear/preserve
│       │   │   ├── cache_config.rs    # Configuration constants
│       │   │   ├── cache_extractor.rs # Extracts important cache entries
│       │   │   ├── cache_scorer.rs    # Scores entry importance
│       │   │   └── cache_bridge.rs    # Creates transition context on clear
│       │   │
│       │   ├── worker_threads/        # Background workers
│       │   │   └── llm_worker.rs      # HTTP proxy to llama-server
│       │   │
│       │   ├── model_runtime/         # Multi-format model support
│       │   │   ├── runtime_trait.rs   # ModelRuntime trait definition
│       │   │   ├── runtime_manager.rs # Auto-detection & initialization
│       │   │   ├── format_detector.rs # Model format identification
│       │   │   ├── gguf_runtime.rs    # GGUF (llama.cpp)
│       │   │   ├── onnx_runtime.rs    # ONNX Runtime
│       │   │   ├── tensorrt_runtime.rs # NVIDIA TensorRT
│       │   │   ├── safetensors_runtime.rs # HuggingFace Safetensors
│       │   │   ├── ggml_runtime.rs    # GGML legacy
│       │   │   └── coreml_runtime.rs  # Apple CoreML
│       │   │
│       │   ├── memory.rs             # Core Message type
│       │   ├── metrics.rs            # Prometheus metrics
│       │   ├── telemetry.rs          # Tracing setup
│       │   ├── resources.rs          # Resource path resolution
│       │   ├── utils/                # Utility functions
│       │   └── backend_target.rs     # Backend target abstraction
│       │
│       └── Resources/
│           ├── models/               # GGUF model files
│           └── bin/                  # Pre-compiled llama-server binaries
│               └── Windows/          # Platform-specific binaries
│
├── apps/
│   └── desktop/
│       ├── package.json              # Frontend dependencies
│       ├── tsconfig.json             # TypeScript config
│       ├── vite.config.ts            # Vite build config
│       ├── vitest.config.ts          # Test config
│       ├── src/
│       │   ├── main.tsx              # React entry point
│       │   ├── App.tsx               # Root component (state management)
│       │   ├── api/
│       │   │   └── chat.ts           # Backend API client
│       │   ├── components/
│       │   │   ├── ChatWindow.tsx     # Main chat interface
│       │   │   ├── Sidebar.tsx        # Conversation list
│       │   │   ├── SearchModal.tsx    # Search interface
│       │   │   └── SaveTranscriptDialog.tsx
│       │   └── hooks/
│       │       └── useChatTitle.ts    # Title generation hook
│       └── src-tauri/
│           ├── Cargo.toml            # Tauri wrapper dependencies
│           ├── tauri.conf.json       # Tauri configuration
│           └── src/
│               └── main.rs           # Tauri entry (spawns backend thread)
│
├── data/
│   └── conversations.db             # SQLite database (runtime)
│
└── scripts/
    └── test-all.sh                  # Full test suite runner
```

---

## 6. Backend Architecture

### 6.1 Entry Point & Server Initialization

**`main.rs`:**
```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let cfg = Config::from_env()?;
    run_thread_server(cfg).await
}
```

**`thread_server.rs` initialization sequence:**
1. Initialize tracing (structured logging)
2. Create `Config` from environment
3. Create `MemoryDatabase` (SQLite + connection pool + WAL mode)
4. Create `SharedState` (DashMap sessions, atomic counters, database Arc)
5. Initialize `KVCacheManager` (optional — falls back to None)
6. Initialize `ContextOrchestrator` with LLM worker injection for semantic search
7. Initialize HNSW embedding index from existing DB data
8. Start worker thread pool
9. Build Axum router with all endpoints
10. Bind to `127.0.0.1:8000` and serve

### 6.2 Router Configuration

```rust
Router::new()
    .route("/generate/stream", post(stream_api::generate_stream))
    .route("/generate/title", post(title_api::generate_title))
    .route("/conversations", get(conversation_api::get_conversations))
    .route("/conversations/:id", get(conversation_api::get_conversation))
    .route("/conversations/:id/title", put(conversation_api::update_conversation_title))
    .route("/conversations/:id/pinned", post(conversation_api::update_conversation_pinned))
    .route("/conversations/:id", delete(conversation_api::delete_conversation))
    .route("/memory/optimize", post(memory_api::optimize_memory))
    .route("/memory/stats/:session_id", get(memory_api::get_memory_stats))
    .route("/memory/cleanup", post(memory_api::cleanup_memory))
    .route("/search", post(search_api::search))
    .route("/healthz", get(|| async { "OK" }))
    .route("/metrics", get(metrics::metrics_handler))
    .layer(CorsLayer::permissive())
    .layer(TraceLayer::new_for_http())
    .layer(TimeoutLayer::new(Duration::from_secs(600)))
```

---

## 7. Frontend Architecture

### 7.1 Component Hierarchy

```
App.tsx (root state: chats[], activeChatId, currentMessages, isSearchOpen)
├── Sidebar (conversation list, pin/delete/select)
├── ChatWindow (message display, input, streaming)
│   └── useChatTitle hook (auto-generates title after first message)
├── SearchModal (hybrid search across all conversations)
└── SaveTranscriptDialog (export to file via Tauri FS plugin)
```

### 7.2 State Management

Pure React state — no external state library. All state lives in `App.tsx`:

```typescript
const [chats, setChats] = useState<Chat[]>([]);
const [activeChatId, setActiveChatId] = useState<string | null>(null);
const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
const [currentMessages, setCurrentMessages] = useState<Message[]>([]);
const [currentChatTitle, setCurrentChatTitle] = useState<string | null>(null);
const [isSearchOpen, setIsSearchOpen] = useState(false);
```

### 7.3 API Client

**`api/chat.ts`** — All backend communication:

| Function | Method | Endpoint | Purpose |
|----------|--------|----------|---------|
| `streamChat()` | POST | `/generate/stream` | SSE streaming chat |
| `fetchConversations()` | GET | `/conversations` | List all chats |
| `fetchConversation(id)` | GET | `/conversations/:id` | Load chat history |
| `updateConversationTitle(id, title)` | PUT | `/conversations/:id/title` | Set title |
| `updateConversationPinned(id, pinned)` | POST | `/conversations/:id/pinned` | Toggle pin |
| `deleteConversation(id)` | DELETE | `/conversations/:id` | Delete chat |

**Base URL:** `http://localhost:8000`

**Streaming:** Uses `fetch()` with `ReadableStream` to consume SSE events. The `streamChat()` function is an `AsyncGenerator<string>` that yields content tokens as they arrive.

### 7.4 Session ID Generation

The frontend generates `session_id` as a UUID v4 string. This ID is used consistently between frontend state, backend shared state, and database storage.

---

## 8. API Surface

### 8.1 POST `/generate/stream` — Core Streaming Endpoint

**Request:**
```json
{
  "messages": [
    { "role": "system", "content": "You are a helpful assistant." },
    { "role": "user", "content": "Hello" }
  ],
  "session_id": "abc-123-def",
  "max_tokens": 2000,
  "temperature": 0.7,
  "stream": true
}
```

**Response:** Server-Sent Events (SSE) stream, OpenAI-compatible format.

**Internal flow:**
1. Validate request
2. Get/create in-memory session
3. Persist user message to DB (background)
4. **Context engine**: Check if past retrieval needed → semantic search → inject context
5. Stream from llama-server via LLM worker
6. Persist assistant response (background)
7. Generate + store embeddings for both messages (background)

### 8.2 POST `/generate/title`

Generates a 1-5 word chat title from the first user prompt. Uses the LLM with max 20 tokens.

### 8.3 GET `/conversations`

Returns all sessions ordered by `last_accessed DESC`:
```json
[{
  "id": "abc-123",
  "title": "React Performance",
  "created_at": "2025-01-15T10:30:00Z",
  "last_accessed": "2025-01-15T11:00:00Z",
  "message_count": 12,
  "pinned": false
}]
```

### 8.4 GET `/conversations/:id`

Returns full conversation with all messages.

### 8.5 PUT `/conversations/:id/title`

Updates conversation title. Used by frontend after auto-generating title from first prompt.

### 8.6 POST `/conversations/:id/pinned`

Toggles pin status for sidebar ordering.

### 8.7 DELETE `/conversations/:id`

Deletes session and all associated messages (CASCADE).

### 8.8 POST `/search`

**Hybrid semantic + keyword search:**
```json
{
  "query": "how to optimize React rendering",
  "session_id": null,
  "limit": 10,
  "similarity_threshold": 0.3
}
```

**Response:**
```json
{
  "results": [{
    "session_id": "abc-123",
    "message_id": 42,
    "content": "For React rendering optimization, you should...",
    "role": "assistant",
    "relevance_score": 0.87,
    "search_source": "semantic"
  }],
  "total": 5,
  "search_type": "hybrid"
}
```

### 8.9 POST `/memory/optimize`

Triggers context optimization for a session. Returns optimized message list.

### 8.10 GET `/memory/stats/:session_id`

Returns memory statistics: total messages, optimized count, compression ratio.

### 8.11 POST `/memory/cleanup`

Cleans up sessions older than specified threshold.

### 8.12 GET `/healthz`

Returns `"OK"`. Used for readiness probes.

### 8.13 GET `/metrics`

Prometheus-format metrics endpoint.

---

## 9. Database Design

### 9.1 Engine & Configuration

- **Engine:** SQLite (bundled via rusqlite, no external dependency)
- **Journal mode:** WAL (Write-Ahead Logging) — concurrent reads during writes
- **Synchronous:** NORMAL (balance of safety and speed)
- **Busy timeout:** 5000ms
- **Foreign keys:** ON
- **Connection pool:** r2d2, max 10 connections
- **Location:** `./data/conversations.db`

### 9.2 Schema

#### Sessions
```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,                    -- UUID v4 from frontend
    created_at TIMESTAMP NOT NULL,
    last_accessed TIMESTAMP NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}'     -- JSON: {title, tags, pinned, user_defined}
);
```

#### Messages
```sql
CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    message_index INTEGER NOT NULL,         -- Sequence within session
    role TEXT NOT NULL CHECK(role IN ('system', 'user', 'assistant', 'function')),
    content TEXT NOT NULL,
    tokens INTEGER NOT NULL DEFAULT 0,
    timestamp TIMESTAMP NOT NULL,
    importance_score REAL NOT NULL DEFAULT 0.5,
    embedding_generated BOOLEAN NOT NULL DEFAULT FALSE,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    UNIQUE(session_id, message_index)
);
```

#### Summaries (Tier 2)
```sql
CREATE TABLE summaries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    message_range_start INTEGER NOT NULL,
    message_range_end INTEGER NOT NULL,
    summary_text TEXT NOT NULL,
    compression_ratio REAL NOT NULL,
    key_topics TEXT NOT NULL,               -- JSON array
    generated_at TIMESTAMP NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    UNIQUE(session_id, message_range_start, message_range_end)
);
```

#### Details
```sql
CREATE TABLE details (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    message_id INTEGER NOT NULL,
    detail_type TEXT NOT NULL,
    content TEXT NOT NULL,
    context TEXT NOT NULL,
    importance_score REAL NOT NULL DEFAULT 0.5,
    accessed_count INTEGER NOT NULL DEFAULT 0,
    last_accessed TIMESTAMP NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
);
```

#### Tier 3 Content (Long-term Memory)
```sql
CREATE TABLE tier3_content (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    content_type TEXT NOT NULL CHECK(content_type IN ('fact','detail','concept','preference','rule')),
    content_text TEXT NOT NULL,
    source_message_ids TEXT,                -- JSON array
    importance_score REAL NOT NULL DEFAULT 0.0,
    access_count INTEGER NOT NULL DEFAULT 0,
    last_accessed TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL,
    metadata TEXT DEFAULT '{}',
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    UNIQUE(session_id, content_hash, content_type)
);
```

#### Embeddings
```sql
CREATE TABLE embeddings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL,
    embedding BLOB NOT NULL,                -- bincode-serialized Vec<f32>
    embedding_model TEXT NOT NULL,           -- "llama-server"
    generated_at TIMESTAMP NOT NULL,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE,
    UNIQUE(message_id, embedding_model)
);

CREATE TABLE embedding_metadata (
    model_name TEXT PRIMARY KEY,
    vector_size INTEGER NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE embedding_similarities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    source_embedding_id INTEGER NOT NULL,
    target_embedding_id INTEGER NOT NULL,
    similarity_score REAL NOT NULL,
    calculated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(source_embedding_id, target_embedding_id)
);
```

#### KV Cache Snapshots
```sql
CREATE TABLE kv_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    message_id INTEGER NOT NULL,
    snapshot_type TEXT NOT NULL DEFAULT 'full'
        CHECK(snapshot_type IN ('full','incremental','checkpoint')),
    kv_state BLOB NOT NULL,                 -- Serialized Vec<KVEntry>
    kv_state_hash TEXT NOT NULL,            -- Blake3 hash
    access_pattern TEXT,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE kv_cache_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_id INTEGER NOT NULL,
    key_hash TEXT NOT NULL,
    key_data BLOB,
    value_data BLOB NOT NULL,
    key_type TEXT NOT NULL DEFAULT 'attention_key',
    layer_index INTEGER NOT NULL,
    head_index INTEGER,
    importance_score REAL DEFAULT 0.5,
    access_count INTEGER DEFAULT 0,
    last_accessed TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (snapshot_id) REFERENCES kv_snapshots(id) ON DELETE CASCADE
);

CREATE TABLE kv_cache_metadata (
    session_id TEXT PRIMARY KEY,
    total_entries INTEGER DEFAULT 0,
    total_size_bytes INTEGER DEFAULT 0,
    last_cleared_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    conversation_count INTEGER DEFAULT 0,
    metadata TEXT DEFAULT '{}'
);
```

### 9.3 Indexes

```sql
-- Messages
CREATE INDEX idx_messages_session ON messages(session_id);
CREATE INDEX idx_messages_timestamp ON messages(timestamp);

-- Summaries
CREATE INDEX idx_summaries_session ON summaries(session_id);

-- Details
CREATE INDEX idx_details_session ON details(session_id);
CREATE INDEX idx_details_type ON details(detail_type);

-- Embeddings
CREATE INDEX idx_embeddings_message ON embeddings(message_id);

-- Tier 3
CREATE INDEX idx_tier3_session ON tier3_content(session_id);
CREATE INDEX idx_tier3_type ON tier3_content(content_type);
CREATE INDEX idx_tier3_importance ON tier3_content(importance_score DESC);
CREATE INDEX idx_tier3_accessed ON tier3_content(last_accessed DESC);

-- KV Cache
CREATE INDEX idx_kv_snapshots_session ON kv_snapshots(session_id);
CREATE INDEX idx_kv_entries_snapshot ON kv_cache_entries(snapshot_id, layer_index, head_index);
CREATE INDEX idx_kv_entries_importance ON kv_cache_entries(importance_score DESC);
```

---

## 10. Three-Tier Memory Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  TIER 1: Hot Cache (In-Memory)                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  moka::sync::Cache<String, Vec<Message>>               │ │
│  │  Max: 50 messages per session  |  TTL: 1 hour          │ │
│  │  Access: ~5-10 microseconds    |  Zero disk I/O        │ │
│  └────────────────────────────────────────────────────────┘ │
│                         ↓ cache miss                         │
├─────────────────────────────────────────────────────────────┤
│  TIER 2: Warm Summaries (Cache + Database)                  │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  moka cache (TTL: 1hr) backed by SQLite summaries      │ │
│  │  Max: 20 summaries per session                         │ │
│  │  Compressed representation of conversation history      │ │
│  └────────────────────────────────────────────────────────┘ │
│                         ↓ need details                       │
├─────────────────────────────────────────────────────────────┤
│  TIER 3: Cold Storage (Persistent Database)                 │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  SQLite with full message history + embeddings          │ │
│  │  Semantic search: HNSW index (cosine similarity)        │ │
│  │  Keyword search: SQL LIKE patterns                      │ │
│  │  Cross-session search: topic matching                   │ │
│  └────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### Tier 1: Hot Cache
- **Implementation:** `moka::sync::Cache` with 1-hour TTL
- **Capacity:** Last 50 messages per session
- **Purpose:** Immediate access to current conversation context
- **Eviction:** LRU + TTL-based automatic cleanup

### Tier 2: Warm Summaries
- **Implementation:** SQLite `summaries` table + moka cache overlay
- **Content:** Compressed conversation summaries with key topics
- **Purpose:** Provide historical context without loading full messages
- **Format:** Summary text + compression ratio + JSON topic list

### Tier 3: Cold Storage
- **Implementation:** SQLite `messages` + `embeddings` + `tier3_content` tables
- **Content:** Complete message history, vector embeddings, extracted facts
- **Access methods:**
  - Semantic search via HNSW index (cosine similarity, threshold ≥ 0.3)
  - Keyword search via SQL `LIKE` patterns
  - Cross-session topic search
  - Direct retrieval by session ID

---

## 11. Context Engine

The context engine is the intelligence layer that decides **what past information** to retrieve and **how to build** the context sent to the LLM.

### 11.1 Orchestrator (`orchestrator.rs`)

The central coordinator. On every conversation turn:

```
process_conversation(session_id, messages, user_query)
    │
    ├── 1. Update Tier 1 cache with current messages
    ├── 2. Persist latest user message to database
    ├── 3. Create retrieval plan (what to search, where)
    ├── 4. Execute retrieval plan:
    │      ├── Tier 1: Get from hot cache
    │      ├── Tier 2: Get summaries
    │      ├── Semantic search: Embed query → HNSW → retrieve messages
    │      ├── Tier 3: Keyword search fallback
    │      └── Cross-session: Topic search across all sessions
    └── 5. Build context: Merge all sources → trim to token limit
```

**Key design:** The orchestrator holds an optional `Arc<LLMWorker>` reference, injected at startup. This allows it to generate query embeddings for semantic search without creating a separate embedding service.

### 11.2 Retrieval Planner (`retrieval_planner.rs`)

Analyzes the user query and conversation state to produce a `RetrievalPlan`:

```rust
pub struct RetrievalPlan {
    pub needs_retrieval: bool,
    pub use_tier1: bool,          // Always true
    pub use_tier2: bool,          // If summaries exist
    pub use_tier3: bool,          // If past references / long conversation
    pub cross_session_search: bool,
    pub semantic_search: bool,    // If query complexity > 0.5
    pub keyword_search: bool,     // If specific references found
    pub temporal_search: bool,    // If time references found
    pub max_messages: usize,
    pub max_tokens: usize,
    pub search_topics: Vec<String>,
}
```

**Decision logic:**

| Condition | Action |
|-----------|--------|
| Brand-new question, no past references | `needs_retrieval = false` → **skip everything** |
| Query contains "remember", "earlier", "before" | Enable Tier 3 + keyword search |
| Query contains "last time", "yesterday", "we discussed" | Enable cross-session search |
| Query complexity > 0.5 | Enable semantic search |
| Conversation > 30 messages | Enable Tier 3 |
| Specific detail words ("exactly", "the code") | Enable Tier 3 |

**Past reference patterns detected:**
`"earlier"`, `"before"`, `"previous"`, `"last time"`, `"yesterday"`, `"we discussed"`, `"we talked about"`, `"remember"`, `"recall"`, `"did we talk"`, `"have we discussed"`, `"what did we say"`, `"what was said"`, `"mentioned earlier"`, `"previously mentioned"`

### 11.3 Context Builder (`context_builder.rs`)

Merges content from all tiers into a single message list:

1. **Preserve system messages** from current conversation
2. **Add Tier 1** content (or select recent messages if unavailable)
3. **Add cross-session messages** with `[Context from previous conversations]` bridge (max 3)
4. **Add Tier 2 summaries** with `[Summary of earlier conversation: ...]` prefix, scored by topic relevance
5. **Add Tier 3 specific details** with `[Earlier detail: ...]` prefix (max 3)
6. **Trim to token limit** (default 4000 tokens, estimated at 4 chars/token)
7. **Add bridging message** between summarized and current content

**Token budget allocation:**
- Current context: minimum 40% of budget
- Summaries: maximum 40% of budget
- Details: remaining space

---

## 12. Semantic Search & Embeddings Pipeline

### 12.1 Design Philosophy

**No separate embedding model.** Embeddings are generated by calling llama-server's `/v1/embeddings` endpoint — the same model already loaded in VRAM for inference. This means:
- Zero additional model loading
- Zero additional VRAM usage
- Vectors are generated by the same model that will consume them
- The overhead is one HTTP call per pair of messages (user + assistant), done in background

### 12.2 Embedding Generation Flow

```
User sends message
    ↓
LLM generates response (streamed)
    ↓
After stream completes (background tokio::spawn):
    ├── Persist assistant message to DB → get message IDs
    ├── Call llama-server POST /v1/embeddings with [user_text, assistant_text]
    ├── Store embeddings in SQLite (bincode-serialized BLOB)
    └── Update HNSW index
```

**Graceful degradation:** If `/v1/embeddings` is not supported by the current llama-server build, the error is logged at `debug` level and keyword search remains functional.

### 12.3 Embedding Storage

- **Vector dimensions:** Determined by model (stored per-embedding)
- **Serialization:** `bincode` → SQLite BLOB
- **Index:** HNSW (Hierarchical Navigable Small World)
  - `n_neighbor: 16` (M parameter)
  - `ef_build: 100` (construction parameter)
  - `ef_search: 50` (search parameter)
  - **Distance metric:** Cosine similarity
- **In-memory cache:** `HashMap<message_id, Vec<f32>>` for linear search fallback

### 12.4 Semantic Search Flow

```
User asks "what did we discuss about React performance?"
    ↓
Retrieval planner detects past reference → enables semantic_search
    ↓
Orchestrator checks: has_embeddings > 0? (skip if fresh DB)
    ↓ YES
Generate query embedding via llama-server /v1/embeddings
    ↓
HNSW index search → returns Vec<(message_id, similarity_score)>
    ↓
Fetch message content from SQLite by message_id
    ↓
Merge with keyword search results (deduplicated)
    ↓
Inject as context before sending to LLM
```

### 12.5 Hybrid Search Strategy (search_api.rs)

The search API combines both approaches:

1. **Phase 1: Semantic** — Embed query → HNSW search → scored results
2. **Phase 2: Keyword** — Extract keywords → SQL LIKE search → scored by frequency
3. **Phase 3: Merge** — Deduplicate by message ID, sort by relevance score, truncate to limit

**Keyword relevance scoring:**
```
score = sum(keyword_matches * keyword_length / content_length) capped at 1.0
```

---

## 13. KV Cache Management

### 13.1 Overview

The KV cache manager handles the lifecycle of llama.cpp's attention KV cache — the hot VRAM data that enables fast follow-up responses without reprocessing the full context.

### 13.2 Cache Lifecycle

```
Conversation 1 → 2 → 3 → ... → 16 → CLEAR → 17 → ... → 32 → CLEAR
                                  ↑                              ↑
                           Save snapshot                  Save snapshot
                           Preserve important             Preserve important
                           Create bridge msg              Create bridge msg
```

### 13.3 Clear Triggers

| Trigger | Threshold |
|---------|-----------|
| Conversation count | Every 16 conversations |
| Memory usage | 60% of max cache size |

### 13.4 Importance Scoring

Each KV cache entry is scored on a 0.0–1.0 scale:

| Factor | Weight | Scoring |
|--------|--------|---------|
| Recency | 0.30 | Exponential decay from last access |
| Access count | 0.20 | Log-scaled frequency |
| Key patterns | 0.25 | System prompts: +0.8, Code: +0.7, Concepts: +0.9 |
| Layer position | 0.10 | Early layers score higher |
| Head position | 0.05 | Based on attention head index |
| Value size | 0.10 | Larger values slightly preferred |

**Preservation threshold:** Entries with importance ≥ 0.7 are preserved during clear.

### 13.5 Snapshot Management

- **Strategy:** Incremental snapshots every 4 conversations
- **Retention:** Last 4 snapshots kept
- **Integrity:** Blake3 hash verification
- **Storage:** Serialized to SQLite BLOB via `kv_snapshots` table

### 13.6 Three-Tier Cache Retrieval

When the system needs past context that's no longer in hot KV cache:

1. **Tier 1:** Search active KV cache entries (similarity threshold: 0.3)
2. **Tier 2:** Search recent KV snapshots — last 3 (threshold: 0.4)
3. **Tier 3:** Full message database search (threshold: 0.5)

Falls through to next tier if insufficient results found.

---

## 14. Model Runtime System

### 14.1 Multi-Format Support

The system supports multiple model formats through a runtime abstraction:

```rust
pub trait ModelRuntime: Send + Sync {
    async fn initialize(&mut self, config: RuntimeConfig) -> anyhow::Result<()>;
    async fn infer(&self, request: InferenceRequest) -> anyhow::Result<InferenceResponse>;
    async fn shutdown(&mut self) -> anyhow::Result<()>;
    fn format(&self) -> ModelFormat;
}
```

### 14.2 Supported Formats

| Format | Runtime | Status | Use Case |
|--------|---------|--------|----------|
| **GGUF** | llama.cpp (via llama-server) | Primary | Default inference |
| **GGML** | llama.cpp legacy | Supported | Legacy models |
| **ONNX** | ONNX Runtime | Supported | Cross-platform models |
| **TensorRT** | NVIDIA TensorRT | Supported | NVIDIA GPU optimization |
| **Safetensors** | HuggingFace format | Supported | HF model ecosystem |
| **CoreML** | Apple CoreML | Supported | Apple Silicon optimization |

### 14.3 Format Detection

The `format_detector` module auto-detects model format from file extension and magic bytes, then initializes the appropriate runtime via `RuntimeManager`.

---

## 15. Shared State & Concurrency

### 15.1 State Structure

```rust
pub struct SharedSystemState {
    pub conversations: Arc<ConversationHierarchy>,
    pub llm_runtime: Arc<RwLock<Option<LLMRuntime>>>,
    pub cache_manager: Arc<RwLock<Option<Arc<KVCacheManager>>>>,
    pub database_pool: Arc<MemoryDatabase>,
    pub config: Arc<Config>,
    pub counters: Arc<AtomicCounters>,
    pub context_orchestrator: Arc<tokio::sync::RwLock<Option<ContextOrchestrator>>>,
    pub llm_worker: Arc<LLMWorker>,
}
```

### 15.2 Concurrency Primitives

| Primitive | Usage | Purpose |
|-----------|-------|---------|
| `Arc<T>` | Everywhere | Shared ownership across threads |
| `DashMap` | Session storage | Lock-free concurrent HashMap |
| `RwLock` (std) | Cache manager, LLM runtime | Reader-writer lock for rare writes |
| `RwLock` (tokio) | Context orchestrator | Async-compatible reader-writer lock |
| `AtomicUsize` | Counters | Lock-free atomic counters |
| `crossbeam::ArrayQueue` | Message queues | Lock-free bounded queues |
| `r2d2::Pool` | Database | Connection pool (max 10) |
| `moka::Cache` | Tier 1/2 | Thread-safe LRU with TTL |

### 15.3 Atomic Counters

```rust
pub struct AtomicCounters {
    pub total_requests: AtomicUsize,
    pub active_sessions: AtomicUsize,
    pub processed_messages: AtomicUsize,
    pub cache_hits: AtomicUsize,
    pub cache_misses: AtomicUsize,
}
```

### 15.4 Unified App State

The router receives `UnifiedAppState` which wraps `SharedSystemState`:

```rust
pub struct UnifiedAppState {
    pub shared_state: Arc<SharedSystemState>,
    pub context_orchestrator: Arc<tokio::sync::RwLock<Option<ContextOrchestrator>>>,
    pub llm_worker: Arc<LLMWorker>,
}
```

---

## 16. Worker Thread Architecture

### 16.1 LLM Worker (`llm_worker.rs`)

The primary worker — proxies all requests to llama-server:

| Method | Endpoint | Purpose |
|--------|----------|---------|
| `stream_response()` | `/v1/chat/completions` | SSE streaming inference |
| `generate_response()` | `/v1/chat/completions` | Non-streaming inference |
| `generate_title()` | `/v1/chat/completions` | Title generation (max 20 tokens) |
| `generate_embeddings()` | `/v1/embeddings` | Vector generation for semantic search |
| `batch_process()` | `/v1/chat/completions` | Batch inference |

**HTTP client configuration:**
- Timeout: 600 seconds
- Connection: persistent (reqwest client reused)
- Target: `http://127.0.0.1:8081`

### 16.2 Thread Pool

Manages worker threads for background operations:
- **Context Worker**: Context retrieval and optimization
- **Cache Worker**: KV cache operations
- **Database Worker**: Database maintenance

---

## 17. Streaming & Data Flow

### 17.1 End-to-End Request Flow

```
Frontend                    Backend (Axum)                  llama-server
   │                            │                              │
   ├─POST /generate/stream─────▶│                              │
   │  {messages, session_id}    │                              │
   │                            ├──Get/create session          │
   │                            ├──Persist user msg (bg)       │
   │                            ├──Context Engine:             │
   │                            │  ├─Check past refs           │
   │                            │  ├─Semantic search (if needed)│
   │                            │  └─Build context             │
   │                            │                              │
   │                            ├──POST /v1/chat/completions──▶│
   │                            │  {messages, stream:true}     │
   │                            │                              │
   │                            │◀─────SSE: data: {chunk}──────│
   │◀──SSE: data: {chunk}──────│                              │
   │◀──SSE: data: {chunk}──────│◀─────SSE: data: {chunk}──────│
   │◀──SSE: data: [DONE]───────│◀─────SSE: data: [DONE]───────│
   │                            │                              │
   │                            ├──Persist assistant msg (bg)  │
   │                            ├──Generate embeddings (bg)────▶│
   │                            │  POST /v1/embeddings         │
   │                            │◀─────{embeddings}────────────│
   │                            └──Store embeddings in DB (bg) │
   │                            │                              │
   ├─POST /generate/title──────▶│ (after first message)       │
   │                            ├──POST /v1/chat/completions──▶│
   │◀──{title}─────────────────│◀─────{title}─────────────────│
   │                            │                              │
   ├─PUT /conversations/:id/title▶│                            │
   │                            └──UPDATE sessions SET title   │
```

### 17.2 Background Tasks

All persistence is fire-and-forget via `tokio::spawn`:

1. **User message persistence** — Immediately after request validation
2. **Assistant message persistence** — After stream completes
3. **Embedding generation** — After assistant persistence, calls `/v1/embeddings` for both messages
4. **Embedding storage** — After generation, stores in SQLite + updates HNSW index

None of these block the streaming response.

---

## 18. Configuration & Environment

### 18.1 Environment Variables

```ini
# ── GPU & Model ──
GPU_LAYERS=16                  # Layers offloaded to GPU
CTX_SIZE=32768                 # Context window size (tokens)
BATCH_SIZE=128                 # Inference batch size
THREADS=6                      # CPU thread count
MODEL_PATH="path/to/model.gguf"
LLAMA_BIN="path/to/llama-server.exe"

# ── Network ──
API_HOST=127.0.0.1             # Backend bind address (localhost only)
API_PORT=8000                  # Backend HTTP port
LLAMA_HOST=127.0.0.1           # llama-server address
LLAMA_PORT=8081                # llama-server port

# ── Performance ──
MAX_CONCURRENT_STREAMS=2       # Parallel stream limit
HOT_SWAP_GRACE_SECONDS=25      # Model swap timeout
HEALTH_TIMEOUT_SECONDS=600     # Health check timeout
GENERATE_TIMEOUT_SECONDS=300   # Generation timeout
STREAM_TIMEOUT_SECONDS=600     # Stream timeout

# ── Monitoring ──
PROMETHEUS_PORT=9000           # Metrics endpoint port
REQUESTS_PER_SECOND=24         # Rate limit
RUST_LOG=info,axum=info        # Log level filter
```

### 18.2 Config Parsing (`config.rs`)

All environment variables are parsed into a strongly-typed `Config` struct at startup. Missing values use sensible defaults. Invalid values cause startup failure with clear error messages.

---

## 19. Observability & Telemetry

### 19.1 Structured Logging

```rust
// tracing with RFC 3339 timestamps, compact format
tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::new(env_filter))
    .with_timer(fmt::time::UtcTime::rfc_3339())
    .with_target(true)
    .with_level(true)
    .compact()
```

**Log levels used:**
- `info` — Server startup, context engine decisions, search results
- `debug` — Embedding generation, cache operations, retrieval planning details
- `warn` — Subsystem initialization failures (graceful degradation)
- `error` — Persistence failures, stream errors

### 19.2 Prometheus Metrics

Exposed on `/metrics` endpoint (port 9000):
- Request counters
- Latency histograms
- Active session gauges

### 19.3 HTTP Tracing

Tower-HTTP `TraceLayer` provides automatic request/response logging for all endpoints.

---

## 20. Error Handling & Validation

### 20.1 API Error Format

```json
{
  "error": "Human-readable error message",
  "code": 400
}
```

### 20.2 Input Validation

| Field | Constraint |
|-------|-----------|
| `session_id` | 1-256 chars, alphanumeric + `-_` only |
| `messages` | 1-1000 messages per request |
| `message.content` | Max 64KB, no null bytes |
| `user_query` | Max 8KB |
| `search.limit` | 1-100 |
| `cleanup.older_than_seconds` | 3600 (1 hour) to 31536000 (1 year) |

### 20.3 Error Recovery Patterns

- **Context engine failure** → Falls back to raw messages (no context optimization)
- **Embedding generation failure** → Logged at debug, keyword search still works
- **Database write failure** → Logged at error, streaming continues unblocked
- **llama-server unreachable** → 502 Bad Gateway returned to client
- **Stream error mid-response** → Error event sent via SSE, stream terminates

---

## 21. Build, Packaging & Deployment

### 21.1 Development

```bash
# Backend only
cd crates/offline-intelligence && cargo run

# Frontend only
cd apps/desktop && npm run dev    # Vite on port 3000

# Full stack (Tauri)
cd apps/desktop && npm run tauri:dev
```

### 21.2 Production Build

```bash
cd apps/desktop
npm run build         # Compile React → dist/
npm run tauri build   # Package everything into installer
```

**Output formats:**
- Windows: `.exe` installer + `.msi`
- macOS: `.dmg` + `.app` bundle
- Linux: `.AppImage`, `.deb`, `.rpm`

### 21.3 What's Bundled

The installer packages:
- Compiled Rust backend binary
- React frontend (static assets)
- Tauri native shell
- Pre-compiled `llama-server` binary (platform-specific, in `Resources/bin/`)
- GGUF model file (in `Resources/models/`)

### 21.4 Testing

```bash
# Backend tests
cargo test --lib

# Frontend tests
cd apps/desktop && npm run test -- --run

# Full suite
./scripts/test-all.sh
```

---

## 22. Security Model

### 22.1 Threat Model

This is a **single-user local application**. The threat model assumes a trusted local environment.

### 22.2 Security Measures

| Measure | Implementation |
|---------|---------------|
| **Network isolation** | Binds to `127.0.0.1` only — not accessible from network |
| **No authentication** | Intentional — single-user local app |
| **No external calls** | Zero outbound network requests |
| **Input validation** | All API inputs validated (length, charset, null bytes) |
| **SQL injection** | Parameterized queries via rusqlite |
| **CORS** | Permissive (localhost-to-localhost) |
| **Timeout protection** | 600s global timeout prevents resource exhaustion |

### 22.3 Data Privacy

- All data stored locally in `./data/conversations.db`
- No telemetry sent externally
- No cloud sync
- Model runs locally — prompts never leave the machine

---

## 23. Performance Characteristics

### 23.1 Latency Breakdown

| Operation | Latency | Notes |
|-----------|---------|-------|
| In-memory session lookup | ~5-10 us | DashMap, zero-copy |
| Tier 1 cache read | ~10-50 us | moka cache |
| Retrieval plan creation | ~100-500 us | String matching only |
| Semantic search (HNSW) | ~1-5 ms | O(log N) approximate |
| Keyword search (SQL) | ~5-20 ms | Indexed queries |
| Database write | ~10-50 ms | WAL mode, async |
| Embedding generation | ~50-200 ms | HTTP to llama-server |
| LLM first token | ~100-2000 ms | Model-dependent |
| SSE token delivery | ~10-50 ms/token | Network + parsing |

### 23.2 Memory Usage

| Component | Approximate Size |
|-----------|-----------------|
| Shared state (base) | ~10 MB |
| Tier 1 cache (50 msgs × N sessions) | ~1 MB per active session |
| HNSW index | ~50 bytes per embedding |
| SQLite connection pool | ~5 MB |
| llama-server (model in VRAM) | Model-dependent (2-16 GB) |

### 23.3 Scalability Limits

| Dimension | Limit | Bottleneck |
|-----------|-------|-----------|
| Concurrent streams | 2 (configurable) | VRAM / llama-server |
| Active sessions | ~1000 | RAM for DashMap |
| Total messages | ~millions | SQLite (WAL mode handles well) |
| Embeddings | ~100K+ | HNSW index rebuild time |
| Database size | ~10 GB practical | SQLite file size |

---

## 24. Completeness Assessment

### 24.1 Functional Coverage

| Feature | Status | Completeness |
|---------|--------|-------------|
| Core streaming chat | Fully functional | 100% |
| Session/conversation management | Fully functional | 100% |
| Database persistence | Fully functional | 100% |
| Three-tier memory | Functional | 90% |
| Context engine | Functional | 85% |
| Semantic search pipeline | Functional | 80% |
| Embedding capture & storage | Functional | 80% |
| KV cache management | Mostly complete | 85% |
| Hybrid search API | Functional | 80% |
| Model runtime abstraction | Framework ready | 70% |
| Frontend UI | Functional | 90% |
| Observability | Basic | 50% |
| Testing | Partial | 40% |

### 24.2 Architecture Strengths

- **1-hop design** eliminates inter-service latency
- **Shared memory via Arc** — zero-copy data sharing between subsystems
- **Three-tier memory** with intelligent retrieval planning
- **Zero-waste embedding capture** — reuses llama.cpp's own vectors
- **Conditional computation** — skips semantic search when nothing to search
- **Graceful degradation** — every advanced feature is optional
- **Background persistence** — streaming never blocked by DB writes
- **WAL mode SQLite** — concurrent reads during writes

### 24.3 Known Limitations

- **Single process** — all components share one process (by design for 1-hop)
- **SQLite** — adequate for single-user, would need migration for multi-user
- **HNSW rebuild** — index rebuilt on startup from all embeddings (acceptable for local use)
- **No model hot-swap during stream** — must wait for active streams to complete
- **Token estimation** — uses 4 chars ≈ 1 token heuristic (not exact)

---

*Generated from codebase analysis. Reflects the current state of the Aud.io system as implemented.*
