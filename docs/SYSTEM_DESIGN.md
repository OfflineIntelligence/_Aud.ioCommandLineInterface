# Aud.io CLI — System Design Document

> Internal developer documentation for the Aud.io offline AI coding assistant.

---

## 1. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Audio_cli (Binary)                        │
│  CLI entry point · Subcommands · REPL loop · UI rendering        │
├──────────────────────────┬──────────────────────────────────────┤
│      Agent Loop          │         UI Module                     │
│  System prompt builder   │  Spinners · Tool display              │
│  Tool call parser        │  Diff view · Welcome screen           │
│  Stream response handler │  Status bar · Error hints             │
├──────────────────────────┴──────────────────────────────────────┤
│                     HTTP API Server (port 8000)                   │
│  Axum · SSE streaming · Session management · Model proxy         │
├─────────────────────────────────────────────────────────────────┤
│                   Thread Server (Orchestrator)                    │
│  Thread pool · LLM worker · Context engine · Cache manager       │
├─────────────────┬──────────────────┬────────────────────────────┤
│  Model Runtime  │  Memory Database │  Model Management           │
│  GGUF (llama)   │  SQLite (r2d2)   │  Registry · Downloader      │
│  ONNX           │  Conversations   │  Storage · Recommender      │
│  TensorRT       │  Summaries       │  Llama binary management    │
│  Safetensors    │  Embeddings      │  Online model support       │
│  CoreML         │  KV Cache        │                             │
│  Online API     │                  │                             │
├─────────────────┴──────────────────┴────────────────────────────┤
│                    llama-server (port 8001)                       │
│  llama.cpp · GGUF model · CUDA/Metal/CPU · OpenAI-compat API     │
└─────────────────────────────────────────────────────────────────┘
```

### Request Flow

```
User Input → CLI REPL → Agent Loop → HTTP POST /generate/stream
                                          ↓
                                    API Server (8000)
                                          ↓
                                    llama-server (8001) or Online API
                                          ↓
                                    SSE stream back
                                          ↓
                                    Agent parses response
                                          ↓
                              ┌── Text? → Print to terminal
                              └── Tool call? → Execute tool → Feed result → Loop
```

---

## 2. Crate Structure

```
crates/offline-intelligence/
├── src/
│   ├── main.rs                    # HTTP server binary entry point
│   ├── cli.rs                     # Audio_cli binary entry point (REPL, subcommands)
│   ├── lib.rs                     # Library root (module declarations)
│   ├── config.rs                  # Configuration from .env / auto-detection
│   ├── agent.rs                   # Agent loop, system prompt, tool parsing, streaming
│   ├── tools.rs                   # Tool implementations (file I/O, shell, git, search)
│   ├── ui.rs                      # Terminal UI components (spinners, tool display, etc.)
│   │
│   ├── model_runtime/             # Multi-format model runtime abstraction
│   │   ├── runtime_trait.rs       # ModelRuntime trait, ModelFormat enum, RuntimeConfig
│   │   ├── runtime_manager.rs     # Lock-free runtime selection and lifecycle
│   │   ├── gguf_runtime.rs        # llama.cpp via llama-server (primary)
│   │   ├── online_runtime.rs      # Cloud API proxy (OpenRouter, OpenAI)
│   │   ├── onnx_runtime.rs        # ONNX Runtime adapter (stub)
│   │   ├── tensorrt_runtime.rs    # TensorRT adapter (stub)
│   │   ├── safetensors_runtime.rs # Safetensors adapter (stub)
│   │   ├── ggml_runtime.rs        # GGML legacy adapter (stub)
│   │   ├── coreml_runtime.rs      # CoreML adapter (stub)
│   │   └── format_detector.rs     # Auto-detect model format from file extension
│   │
│   ├── model_management/          # Model lifecycle management
│   │   ├── mod.rs                 # ModelManager service
│   │   ├── registry.rs            # Model catalog (16 pre-populated models)
│   │   ├── downloader.rs          # Download from HuggingFace/Ollama/OpenRouter
│   │   ├── storage.rs             # Platform-specific AppData storage
│   │   ├── recommendation.rs      # Hardware-aware model recommendations
│   │   ├── progress.rs            # Download progress tracking
│   │   └── llama_binary.rs        # llama-server binary auto-download & detection
│   │
│   ├── memory_db/                 # SQLite conversation persistence
│   │   ├── mod.rs                 # MemoryDatabase (connection pool, transactions)
│   │   ├── schema.rs              # SQL schema definitions
│   │   ├── migration.rs           # Schema versioning (currently v3)
│   │   ├── conversation_store.rs  # Messages CRUD
│   │   ├── summary_store.rs       # Conversation summaries
│   │   └── embedding_store.rs     # Vector embeddings + HNSW index
│   │
│   ├── context_engine/            # Smart context window management
│   │   ├── orchestrator.rs        # Context orchestration
│   │   └── tier_manager.rs        # Tiered importance-based message selection
│   │
│   ├── api/                       # HTTP API routes
│   │   └── stream_api.rs          # SSE streaming endpoint
│   │
│   ├── terminal/                  # Terminal interaction
│   │   └── mouse_handler.rs       # Mouse event support
│   │
│   ├── utils/                     # Shared utilities
│   │   ├── path_resolver.rs       # Smart path resolution (Desktop/Documents/Home)
│   │   └── mod.rs                 # Confirmation system, path exports
│   │
│   ├── thread_server.rs           # Server orchestrator (initializes all subsystems)
│   ├── thread_pool.rs             # Worker thread management
│   ├── worker_threads/            # Background workers
│   │   └── llm_worker.rs          # LLM inference worker
│   ├── shared_state.rs            # Cross-thread shared state (ArcSwap)
│   ├── cache_management/          # KV cache management
│   ├── capabilities/              # Hardware capability detection
│   ├── resources/                 # Static resources
│   ├── admin/                     # Admin API
│   ├── metrics/                   # Prometheus metrics
│   └── telemetry/                 # Logging setup
│
├── .env                           # Dev-mode configuration (MODEL_PATH, LLAMA_BIN)
├── Cargo.toml                     # Dependencies
└── Resources/                     # Dev-mode bundled files
    ├── models/                    # Local GGUF model file
    └── bin/                       # llama-server binaries per platform
```

---

## 3. Key Subsystems

### 3.1 Agent Loop (`agent.rs`)

The core intelligence engine. Implements a tool-calling agent that:

1. **Builds system prompt** with injected environment context (OS, shell, paths, project info)
2. **Sends messages** to the LLM via HTTP SSE streaming
3. **Parses XML tool calls** from the LLM response (`<tool_call>...</tool_call>`)
4. **Executes tools** and feeds results back as user messages
5. **Loops** up to 25 turns until the LLM produces a final text-only response

**System Prompt Architecture:**
- Injected at runtime with real user paths (Desktop, Documents, Home)
- OS-specific shell guidance (PowerShell on Windows, bash on Linux/macOS)
- Identity enforcement: "You are Aud.io CLI" (never reveals model name)
- Full tool descriptions with actual capabilities listed

**Stream Handling:**
- SSE chunks are buffered and parsed in real-time
- `<tool_call>` XML blocks are suppressed from user output
- Partial tags are buffered to avoid display glitches
- Text content is streamed token-by-token to the terminal

### 3.2 Tools (`tools.rs`)

13 tools available to the agent:

| Tool | Description | Risk Level |
|------|-------------|------------|
| `read_file` | Read file contents with line numbers | Safe |
| `write_file` | Create/overwrite files | Destructive |
| `replace_in_file` | Find-and-replace in files | Destructive |
| `list_dir` | List directory contents | Safe |
| `search_files` | Glob pattern file search | Safe |
| `search_content` | Regex content search (ripgrep-style) | Safe |
| `shell_exec` | Execute shell commands | Dangerous |
| `delete_file` | Delete files | Destructive |
| `git_status` | Git repository status | Safe |
| `git_diff` | Git diff (staged/unstaged) | Safe |
| `git_log` | Recent commit history | Safe |
| `get_project_info` | Detect project type, languages, structure | Safe |
| `batch_read_files` | Read multiple files at once | Safe |

**Path Resolution:**
- `resolve_path()` — resolves relative paths against project root
- `resolve_path_with_fallbacks()` — smart resolution with Desktop/Documents/Home handling
- `extract_subpath_after_keyword()` — extracts file/subfolder from special directory paths

**Shell Execution:**
- Windows: PowerShell (`-NoProfile -NonInteractive -Command`)
- Unix: `/bin/bash -c`
- 30-second timeout, output truncated at 10KB
- Working directory set to project root

### 3.3 Model Runtime (`model_runtime/`)

Trait-based abstraction layer supporting multiple model formats:

```rust
#[async_trait]
pub trait ModelRuntime: Send + Sync {
    fn supported_format(&self) -> ModelFormat;
    async fn initialize(&mut self, config: RuntimeConfig) -> Result<()>;
    async fn is_ready(&self) -> bool;
    fn base_url(&self) -> String;
    async fn generate(&self, request: InferenceRequest) -> Result<InferenceResponse>;
    async fn generate_stream(&self, request: InferenceRequest) -> Result<Box<dyn Stream<...>>>;
    async fn shutdown(&mut self) -> Result<()>;
    fn metadata(&self) -> RuntimeMetadata;
}
```

**GGUF Runtime (primary):**
- Spawns `llama-server` as child process on port 8001
- Proxies via OpenAI-compatible `/v1/chat/completions` endpoint
- Health check loop waits up to 60s for startup
- Process killed on shutdown/drop

**Online Runtime:**
- Proxies to OpenRouter, OpenAI, or custom OpenAI-compatible endpoints
- Auth via `Authorization: Bearer` header
- Config stored in `AppData/Aud.io/online_config.json`

**Runtime Manager:**
- Lock-free via `ArcSwap` for atomic runtime pointer swapping
- Supports hot-swap (change model without restart)
- Auto-detects format from file extension

### 3.4 Model Management (`model_management/`)

**Storage Locations:**
| Platform | Path |
|----------|------|
| Windows | `%APPDATA%\Aud.io\models\` |
| macOS | `~/Library/Application Support/Aud.io/models/` |
| Linux | `~/.local/share/aud.io/models/` |

**llama-server Binary Detection (`llama_binary.rs`):**
Search order: `LLAMA_BIN` env → AppData/bin → next to CLI exe → system PATH

**Binary Auto-Download:**
- Fetches latest release from `ggerganov/llama.cpp` GitHub
- Detects platform: Windows CUDA/CPU, macOS Metal/x64, Linux CUDA/CPU
- GPU detection via NVML (`nvml-wrapper`)
- Downloads, extracts archive (zip/tar.gz), places in AppData/bin

**Model Registry:**
- 16 pre-populated models with metadata (size, format, source, requirements)
- Search by name/description
- Status tracking (Available, Downloading, Installed, Failed)

### 3.5 Database (`memory_db/`)

**Technology:** SQLite via r2d2 connection pool (max 10 connections)

**Location:** `./data/conversations.db` (relative to server working directory)

**Schema (version 3):**

```sql
-- Core tables
messages (id, session_id, message_index, role, content, tokens, timestamp, importance_score, embedding_generated)
sessions (id, title, created_at, updated_at, message_count, pinned)

-- Context management
summaries (id, session_id, summary_text, token_count, created_at)

-- Semantic search
embeddings (id, message_id, embedding_vector, created_at)

-- KV cache persistence
kv_snapshots (id, session_id, message_id, snapshot_type, kv_state, kv_state_hash, size_bytes, created_at)
kv_cache_entries (id, snapshot_id, key_hash, key_data, value_data, key_type, layer_index, head_index, importance_score, access_count, last_accessed)
kv_cache_metadata (session_id, total_entries, total_size_bytes, conversation_count, metadata, last_cleared_at)
```

**Pragmas:** WAL journal mode, foreign keys ON, synchronous NORMAL, 5s busy timeout

**Stores:**
- `ConversationStore` — CRUD for sessions and messages
- `SummaryStore` — conversation summaries for context compression
- `EmbeddingStore` — vector embeddings with HNSW index (via `hora` crate)

### 3.6 UI Module (`ui.rs`)

**Components:**
- `welcome_screen()` — branded welcome with project info
- `thinking_spinner()` — animated dots while waiting for LLM
- `reasoning_spinner()` — rotating circle for reasoning
- `working_spinner()` — progress bar for executing tasks
- `startup_spinner()` — server startup animation
- `tool_start()` — styled tool operation display with icons (→ Read, ← Write, ≡ Edit, $ Run, ⌕ Search, ⎇ Git)
- `tool_result()` — success/error result with truncation
- `show_edit_diff()` — compact diff view for file edits (- red / + green)
- `ai_response_header()` — styled AI response prefix
- `ai_stream_token()` — token-by-token streaming output
- `show_help()` — formatted help menu
- `show_tools()` — formatted tool list
- `status_bar()` — model/mode/session info
- `separator()` / `light_separator()` — visual dividers

---

## 4. Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `MODEL_PATH` | Path to GGUF model file | Auto-detected |
| `LLAMA_BIN` | Path to llama-server binary | Auto-detected |
| `GPU_LAYERS` | Number of GPU layers to offload | 16 |
| `CTX_SIZE` | Context window size (tokens) | 32768 |
| `BATCH_SIZE` | Inference batch size | 128 |
| `THREADS` | CPU threads for inference | 6 |
| `API_HOST` / `API_PORT` | API server bind address | 127.0.0.1:8000 |
| `LLAMA_HOST` / `LLAMA_PORT` | llama-server bind address | 127.0.0.1:8001 |
| `OPENROUTER_API_KEY` | API key for OpenRouter online models | (none) |
| `OPENAI_API_KEY` | API key for OpenAI online models | (none) |

### Auto-Detection (no .env required for distribution)

1. **llama-server binary:** AppData/bin → next to exe → PATH
2. **Model file:** MODEL_PATH env → AppData/models → next to exe
3. **Platform/GPU:** NVML for NVIDIA, architecture for Apple Silicon
4. **Project info:** cargo.toml/package.json/pom.xml detection, `.git/HEAD` for branch

---

## 5. CLI Subcommands

```
Audio_cli [COMMAND]

Commands:
  code            Start a coding agent session (default)
  chat            Start a plain chat session
  server          Start the HTTP API server only
  audio setup     First-run wizard (download llama-server + select model)
  audio info      Show system info (hardware, paths, components)
  models list     List installed models
  models search   Search model catalog
  models install  Download and install a model
  models remove   Remove an installed model
  models info     Show hardware info and recommendations
  models online   Configure online model (OpenRouter/OpenAI)
  models offline  Switch back to local model
  conversations   Manage conversation history
```

### Interactive Commands (in `code` mode)

| Command | Action |
|---------|--------|
| `/new` | Start fresh session |
| `/tools` | List available tools |
| `/history` | List past conversations |
| `/git` | Show git status |
| `/diff` | Show git diff |
| `/clear` | Clear screen |
| `/model` | Show active model info |
| `/chat` | Switch to plain chat mode |
| `/help` | Show command list |
| `/exit` | Quit |

---

## 6. Distribution

### Install Scripts

**Linux/macOS:**
```bash
curl -fsSL https://raw.githubusercontent.com/<org>/aud.io/main/install.sh | bash
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/<org>/aud.io/main/install.ps1 | iex
```

### First-Run Flow

```
curl install → CLI binary in PATH
    ↓
aud code (first run)
    ↓
"First-time setup required"
    ↓
aud setup
    ↓
1. Detect hardware (RAM, CPU, GPU via NVML)
2. Download llama-server for detected platform (from llama.cpp GitHub releases)
3. Show model recommendations based on RAM/VRAM
4. User selects model → downloads from HuggingFace
    ↓
aud code → fully operational, 100% offline
```

### File Layout (installed)

```
~/.local/bin/aud                    # CLI binary (Linux/macOS)
%LOCALAPPDATA%\Aud.io\bin\aud.exe   # CLI binary (Windows)

# AppData (auto-created):
%APPDATA%\Aud.io\                   # Windows
~/Library/Application Support/Aud.io/ # macOS
~/.local/share/aud.io/              # Linux
    ├── bin/
    │   └── llama-server(.exe)      # Auto-downloaded inference engine
    ├── models/
    │   └── <model-name>/
    │       └── model.gguf          # Downloaded model files
    └── online_config.json          # Online mode configuration (optional)
```

---

## 7. Security Model

- **Offline by default:** No data leaves the machine unless online mode is explicitly configured
- **Auto-approve safe operations:** Reads, searches, git status
- **Confirmation for destructive ops:** Write, delete, shell execution
- **Path sandboxing:** Tools resolve paths relative to project root
- **No credential storage:** API keys from env vars only, never persisted in model state

---

## 8. Dependencies (Key)

| Crate | Purpose |
|-------|---------|
| `axum` | HTTP server |
| `reqwest` | HTTP client (LLM proxy, downloads) |
| `tokio` | Async runtime |
| `rusqlite` + `r2d2` | SQLite with connection pool |
| `sysinfo` | System information (RAM, CPU) |
| `nvml-wrapper` | NVIDIA GPU detection |
| `indicatif` | Progress bars and spinners |
| `colored` | Terminal ANSI colors |
| `crossterm` | Terminal control |
| `rustyline` | REPL input with history |
| `clap` | CLI argument parsing |
| `serde_json` | JSON serialization |
| `flate2` + `tar` + `zip` | Archive extraction |
| `async-stream` | SSE stream construction |
| `arc-swap` | Lock-free atomic pointer swapping |
| `hora` | HNSW approximate nearest neighbor index |
