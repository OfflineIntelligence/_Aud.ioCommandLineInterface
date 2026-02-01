<p align="center">
  <h1 align="center">🔊 Aud.io</h1>
  <p align="center">
    <strong>Privacy-first, offline AI coding assistant</strong>
  </p>
  <p align="center">
    A fully local AI coding assistant that runs 100% on your machine. No cloud, no telemetry, no API keys required.
  </p>
</p>

<p align="center">
  <a href="#installation">Installation</a> •
  <a href="#features">Features</a> •
  <a href="#usage">Usage</a> •
  <a href="#architecture">Architecture</a> •
  <a href="#contributing">Contributing</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License">
  <img src="https://img.shields.io/badge/rust-2021-orange.svg" alt="Rust 2021">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg" alt="Platform">
</p>

---

## Why Aud.io?

**Your code stays on your machine.** Unlike cloud-based AI assistants, Aud.io runs entirely locally using open-source models. No internet required, no data leaves your computer, no subscription fees.

- 🔒 **100% Offline** — All inference happens locally using llama.cpp
- 🚀 **Zero Dependencies** — Single binary, batteries included
- 🛠️ **Agentic Coding** — Read, write, edit files and run commands autonomously
- ⚡ **Hardware Aware** — Automatically detects GPU (CUDA/Metal) for acceleration
- 🎯 **Tool-Calling Agent** — 13 tools for file I/O, search, shell, and git operations

---

## Installation

### One-Line Install

**Linux / macOS:**
```bash
curl -fsSL https://raw.githubusercontent.com/aud-io/aud.io/main/install.sh | bash
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/aud-io/aud.io/main/install.ps1 | iex
```

### What Gets Installed

- `aud` CLI binary
- Bundled llama.cpp inference engine
- **No models included** — you choose during setup

### First-Run Setup

After installation, run the setup wizard:

```bash
aud setup
```

You'll be asked to choose:
1. **Offline Mode** — Download a free GGUF model from HuggingFace (~2-8 GB)
2. **Online Mode** — Use an API key (OpenRouter, OpenAI, or custom endpoint)

---

## Features

### 🤖 Agentic Coding Assistant

Aud.io is a tool-calling agent that can autonomously:

| Capability | Tools |
|------------|-------|
| **File Operations** | `read_file`, `write_file`, `replace_in_file`, `batch_read_files` |
| **Search** | `search_files` (glob), `search_content` (regex) |
| **Directory** | `list_dir` (recursive supported) |
| **Shell** | `shell_exec` (PowerShell on Windows, bash on Unix) |
| **Git** | `git_status`, `git_diff`, `git_log` |
| **Project Info** | `get_project_info` (detects project type, languages, structure) |

### 🖥️ Terminal UI

- Animated spinners for thinking/reasoning states
- Styled tool operation display with icons
- Compact diff view for file edits
- Beautiful welcome screen with project info
- Native mouse support

### 🧠 Memory Architecture

Three-tier memory hierarchy with semantic search:

1. **Tier 1 (Hot)** — Current conversation in RAM
2. **Tier 2 (Warm)** — Compressed summaries
3. **Tier 3 (Cold)** — Full history in SQLite with embeddings

### 🔌 Multi-Format Model Support

| Format | Description |
|--------|-------------|
| **GGUF** | Primary format via llama.cpp (recommended) |
| **ONNX** | ONNX Runtime support |
| **TensorRT** | NVIDIA TensorRT acceleration |
| **CoreML** | Apple Neural Engine |
| **Safetensors** | Direct safetensors loading |
| **Online** | OpenRouter, OpenAI, custom APIs |

### 📦 Model Management

- 16+ pre-configured models in the catalog
- Hardware-aware recommendations based on your RAM/VRAM
- Download from HuggingFace with progress tracking
- Hot-swap models without restarting

---

## Usage

### Start a Coding Session

```bash
# Start in current directory
aud code

# Start in a specific project
aud code --path /path/to/project

# Resume a previous session
aud code --session <session-id>
```

### Interactive Commands

| Command | Description |
|---------|-------------|
| `/new` | Start a new session |
| `/tools` | List available tools |
| `/history` | List past conversations |
| `/git` | Show git status |
| `/diff` | Show git diff |
| `/model` | Show active model info |
| `/chat` | Switch to plain chat mode |
| `/clear` | Clear screen |
| `/help` | Show all commands |
| `/exit` | Quit |

### Model Management

```bash
# List installed models
aud models list

# Search for models
aud models search "coding"

# Install a model
aud models install Qwen/Qwen2.5-Coder-7B-Instruct-GGUF

# Show hardware info and recommendations
aud models info

# Configure online mode
aud models online --provider openrouter --key <your-key>

# Switch back to offline
aud models offline
```

### Conversation Management

```bash
# List all conversations
aud conversations list

# Show messages from a conversation
aud conversations show <id>

# Delete a conversation
aud conversations delete <id>

# Pin/unpin a conversation
aud conversations pin <id>
```

### HTTP API Server

Run the backend server standalone:

```bash
aud server
```

The server exposes a REST API on `http://127.0.0.1:8000`:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/generate/stream` | POST | Streaming chat completion (SSE) |
| `/generate/title` | POST | Auto-generate conversation titles |
| `/conversations` | GET | List conversations |
| `/conversations/:id` | GET/DELETE | Get or delete conversation |
| `/search` | POST | Semantic search |
| `/models` | GET/POST | Model management |
| `/healthz` | GET | Health check |
| `/metrics` | GET | Prometheus metrics |

---

## Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Aud.io CLI (Rust)                        │
│  REPL · Agent Loop · Tool Execution · Terminal UI               │
├──────────────────────────────────────────────────────────────────┤
│                   HTTP API Server (port 8000)                   │
│  Axum · SSE Streaming · Session Management                      │
├─────────────────┬──────────────────┬────────────────────────────┤
│  Model Runtime  │  Memory Database │  Context Engine            │
│  GGUF/ONNX/etc  │  SQLite + HNSW   │  Tiered Retrieval          │
├─────────────────┴──────────────────┴────────────────────────────┤
│                    llama-server (port 8001)                     │
│  llama.cpp · CUDA/Metal/CPU · OpenAI-compatible API             │
└─────────────────────────────────────────────────────────────────┘
```

### Key Design Principles

1. **Privacy First** — All computation happens locally. No telemetry, no cloud.
2. **Zero Network Dependency** — Only localhost HTTP for LLM inference.
3. **Zero-Waste Computation** — No separate embedding model; reuses LLM's embedding endpoint.
4. **Graceful Degradation** — Every advanced feature is optional; falls back safely.

### Directory Structure

```
crates/offline-intelligence/
├── src/
│   ├── main.rs                 # HTTP server entry point
│   ├── cli.rs                  # CLI entry point (REPL, commands)
│   ├── agent.rs                # Agentic loop, system prompt, tool parsing
│   ├── tools.rs                # 13 tool implementations
│   ├── ui.rs                   # Terminal UI components
│   ├── model_runtime/          # Multi-format model runtime abstraction
│   ├── model_management/       # Model catalog, downloader, storage
│   ├── memory_db/              # SQLite persistence + embeddings
│   ├── context_engine/         # Smart context window management
│   ├── cache_management/       # KV cache lifecycle
│   ├── worker_threads/         # Background workers
│   └── api/                    # HTTP API handlers
└── Resources/
    └── bin/                    # Bundled llama.cpp binaries
```

### Database Schema

```sql
-- Core tables
sessions          -- Conversation sessions
messages          -- Individual messages
summaries         -- Compressed conversation segments
embeddings        -- 384-dim semantic vectors (HNSW indexed)
kv_snapshots      -- KV cache snapshots
```

---

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MODEL_PATH` | Auto-detect | Path to GGUF model file |
| `LLAMA_BIN` | Auto-detect | Path to llama-server binary |
| `GPU_LAYERS` | Auto | Number of GPU layers to offload |
| `CTX_SIZE` | 32768 | Context window size |
| `THREADS` | Auto | CPU threads for inference |
| `BATCH_SIZE` | 128 | Inference batch size |
| `API_PORT` | 8000 | HTTP server port |
| `LLAMA_PORT` | 8001 | llama-server port |

### Data Locations

| Platform | Path |
|----------|------|
| Windows | `%APPDATA%\Aud.io\` |
| macOS | `~/Library/Application Support/Aud.io/` |
| Linux | `~/.local/share/aud.io/` |

---

## System Requirements

### Minimum

- **CPU**: 4-core modern processor
- **RAM**: 8 GB
- **Disk**: 10 GB free space
- **OS**: Windows 10+, macOS 12+, Linux (kernel 5.4+)

### Recommended

- **CPU**: 8-core with AVX2 support
- **RAM**: 16 GB+
- **GPU**: NVIDIA RTX series (CUDA) or Apple Silicon (Metal)
- **Disk**: SSD with 20 GB+ free

---

## Building from Source

### Prerequisites

- Rust 1.70+ (`rustup` recommended)
- For GPU support:
  - NVIDIA: CUDA Toolkit 12.0+
  - Apple: Xcode Command Line Tools

### Build

```bash
# Clone the repository
git clone https://github.com/aud-io/aud.io.git
cd aud.io

# Build release binary
cargo build --release

# The binaries are in target/release/
# - offline-intelligence (HTTP server)
# - Audio_cli (CLI)
```

### Running in Development

```bash
# Copy .env.example to .env and configure paths
cp .env.example .env

# Run the CLI
cargo run --bin Audio_cli -- code

# Run the server
cargo run --bin offline-intelligence
```

---

## Security Model

- **Offline by default** — No data leaves your machine unless you explicitly configure online mode
- **Confirmation for destructive operations** — Write, delete, and shell commands prompt for approval
- **Path sandboxing** — Tools resolve paths relative to project root
- **No credential storage** — API keys from env vars only, never persisted in model state

---

## Contributing

We welcome contributions! Here's how to get started:

1. **Fork** the repository
2. **Create** a feature branch: `git checkout -b feature/my-feature`
3. **Commit** your changes: `git commit -am 'Add my feature'`
4. **Push** to the branch: `git push origin feature/my-feature`
5. **Open** a Pull Request

### Development Guidelines

- Follow Rust idioms and the existing code style
- Add tests for new functionality
- Update documentation for user-facing changes
- Keep commits atomic and well-described

---

## License

Aud.io is licensed under the **Apache License 2.0**. See [LICENSE](LICENSE) for details.

---

## Acknowledgments

- [llama.cpp](https://github.com/ggerganov/llama.cpp) — High-performance LLM inference
- [Axum](https://github.com/tokio-rs/axum) — Rust web framework
- [Tokio](https://tokio.rs/) — Async runtime
- The open-source model community (Meta, Qwen, Mistral, Microsoft, etc.)

---

<p align="center">
  <sub>Built with ❤️ for developers who value privacy</sub>
</p>