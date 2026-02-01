// Audio_cli — Command-line interface for Aud.io
//
// Cross-platform CLI that wraps the offline-intelligence backend.
// Provides interactive chat, conversation management, and model operations.

use std::io::Write;

use clap::{Parser, Subcommand};
use colored::Colorize;
use dotenvy::dotenv;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use tokio::time::{sleep, Duration};

use offline_intelligence::{config::Config, run_thread_server};
use offline_intelligence::agent;
use offline_intelligence::tools;
use offline_intelligence::terminal::{initialize_mouse_support, shutdown_mouse_support};
use offline_intelligence::model_management::llama_binary;
use offline_intelligence::ui;

/// Aud.io CLI — privacy-first, offline AI chat
#[derive(Parser)]
#[command(name = "audio", version, about = "Audio CLI — privacy-first, offline AI coding agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start an interactive chat session
    Chat {
        /// Resume an existing session by ID
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Start a coding agent session (read/write files, run commands)
    Code {
        /// Project directory (default: current directory)
        #[arg(short, long)]
        path: Option<String>,
        /// Resume an existing session by ID
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Manage conversations
    #[command(subcommand)]
    Conversations(ConversationCmd),
    /// Manage models
    #[command(subcommand)]
    Models(ModelCmd),
    /// Start the HTTP API server only
    Server,
    /// Audio CLI management commands (setup, info, etc.)
    #[command(subcommand)]
    Audio(AudioCmd),
}

#[derive(Subcommand)]
enum ConversationCmd {
    /// List all conversations
    List,
    /// Show messages in a conversation
    Show {
        /// Conversation ID
        id: String,
    },
    /// Delete a conversation
    Delete {
        /// Conversation ID
        id: String,
    },
    /// Toggle pin on a conversation
    Pin {
        /// Conversation ID
        id: String,
    },
}

#[derive(Subcommand)]
enum ModelCmd {
    /// List installed models
    List,
    /// Search for models
    Search {
        /// Search query
        query: String,
    },
    /// Install a model
    Install {
        /// Model identifier
        model: String,
    },
    /// Remove an installed model
    Remove {
        /// Model identifier
        model: String,
    },
    /// Show hardware info and recommendations
    Info,
    /// Use an online model (OpenRouter, OpenAI-compatible API)
    Online {
        /// Provider: "openrouter", "openai", or a custom base URL
        #[arg(short, long, default_value = "openrouter")]
        provider: String,
        /// Model identifier (e.g., "meta-llama/llama-3-8b-instruct")
        #[arg(short, long)]
        model: Option<String>,
        /// API key (or set OPENROUTER_API_KEY / OPENAI_API_KEY env var)
        #[arg(short, long)]
        key: Option<String>,
    },
    /// Switch back to offline (local) model
    Offline,
}

#[derive(Subcommand)]
enum AudioCmd {
    /// First-run setup: download llama-server binary and select a model
    Setup,
    /// Show system info (hardware, paths, installed components)
    Info,
}

const BASE_URL: &str = "http://127.0.0.1:8000";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env early so LLAMA_BIN and MODEL_PATH are available for setup checks
    dotenv().ok();

    // Register Ctrl+C handler once globally
    ctrlc::set_handler(move || {
        println!("\nGoodbye.");
        std::process::exit(0);
    }).ok();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Server) => run_server().await,
        Some(Commands::Chat { session }) => run_chat(session).await,
        Some(Commands::Code { path, session }) => {
            // Check if online mode is configured — skip offline setup if so
            let has_online_config = dirs::data_dir()
                .map(|d| if cfg!(windows) { d.join("Aud.io") } else { d.join("aud.io") })
                .map(|d| d.join("online_config.json").exists())
                .unwrap_or(false);

            // Auto-run setup wizard if no online config AND (llama-server or model is missing)
            if !has_online_config && (llama_binary::find_llama_binary().is_none() || llama_binary::find_model_file().is_none()) {
                println!();
                println!("{}", "  First-time setup required.".yellow().bold());
                println!("  {}", "Aud.io needs a model to run — either offline (local) or online (API).".dimmed());
                println!();
                run_setup().await?;
            }
            run_code(path, session).await
        }
        Some(Commands::Conversations(cmd)) => run_conversations(cmd).await,
        Some(Commands::Models(cmd)) => run_models(cmd).await,
        Some(Commands::Audio(cmd)) => match cmd {
            AudioCmd::Setup => run_setup().await,
            AudioCmd::Info => run_audio_info().await,
        },
        // Default: interactive chat
        None => run_chat(None).await,
    }
}

// ─── First-Run Setup Wizard ──────────────────────────────────────────────────

async fn run_setup() -> anyhow::Result<()> {
    println!();
    println!("  {} {}", "●".cyan().bold(), "audio setup".cyan().bold());
    println!("  {}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".dimmed());
    println!();

    // Show hardware info
    let hw = llama_binary::get_hardware_summary();
    println!("  {} {}", "Hardware:".white().bold(), hw);
    println!();

    // Step 1: Check llama-server binary (bundled or available)
    let has_binary = if let Some(existing) = llama_binary::find_llama_binary() {
        println!("  {} llama-server found: {}", "■".green().bold(), existing.display().to_string().dimmed());
        true
    } else {
        println!("  {} llama-server binary not found.", "■".yellow());
        println!("  {} Bundled binaries should be included with the installation.", "".dimmed());
        println!("  {} You can manually set LLAMA_BIN in .env if needed.", "Tip:".dimmed());
        false
    };

    println!();

    // Step 2: Choose mode — Offline (local model) vs Online (API)
    println!("  {} How would you like to use Aud.io?", "●".cyan().bold());
    println!();
    println!("    {} {} — Download a free model and run locally (private, no internet needed)", "1".cyan().bold(), "Offline".white().bold());
    println!("      {}", "Downloads a GGUF model from HuggingFace (~2-8 GB)".dimmed());
    println!("    {} {} — Use an API key (OpenRouter, OpenAI, or custom)", "2".cyan().bold(), "Online".white().bold());
    println!("      {}", "No download needed. Requires an API key. Free tiers available on OpenRouter.".dimmed());
    println!();

    print!("  Select mode [1]: ");
    std::io::stdout().flush()?;
    let mut mode_input = String::new();
    std::io::stdin().read_line(&mut mode_input)?;
    let mode_choice = mode_input.trim();
    let mode_choice = if mode_choice.is_empty() { "1" } else { mode_choice };

    println!();

    match mode_choice {
        "2" => {
            // ── Online Mode Setup ──
            setup_online_mode().await?;
        }
        _ => {
            // ── Offline Mode Setup ──
            if !has_binary {
                println!("  {} llama-server is required for offline mode.", "■".red().bold());
                println!("  {} Re-install Aud.io to get bundled binaries, or switch to online mode.", "Tip:".dimmed());
                println!();
                println!("  {}", "─".repeat(55));
                return Ok(());
            }
            setup_offline_mode().await?;
        }
    }

    println!();
    println!("  {}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".dimmed());
    println!("  {} Setup complete. Run {} to start coding.", "■".green().bold(), "audio code".cyan());
    println!();

    Ok(())
}

/// Online mode setup: configure API provider and key
async fn setup_online_mode() -> anyhow::Result<()> {
    println!("  {} {}", "●".cyan().bold(), "Online Mode Setup".white().bold());
    println!("  {}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".dimmed());
    println!();

    // Choose provider
    println!("  Select a provider:");
    println!();
    println!("    {} {} — Free tier available, many models", "1".cyan().bold(), "OpenRouter (recommended)".white().bold());
    println!("      {}", "Sign up at https://openrouter.ai → free credits to start".dimmed());
    println!("    {} {} — GPT-4o, GPT-4o-mini, etc.", "2".cyan().bold(), "OpenAI".white());
    println!("    {} {} — Any OpenAI-compatible endpoint", "3".cyan().bold(), "Custom API URL".white());
    println!();

    print!("  Provider [1]: ");
    std::io::stdout().flush()?;
    let mut prov_input = String::new();
    std::io::stdin().read_line(&mut prov_input)?;
    let prov = prov_input.trim();
    let prov = if prov.is_empty() { "1" } else { prov };

    let (api_base_url, provider_name, default_model, env_key_name) = match prov {
        "2" => (
            "https://api.openai.com/v1".to_string(),
            "OpenAI",
            "gpt-4o-mini",
            "OPENAI_API_KEY",
        ),
        "3" => {
            print!("  Enter API base URL: ");
            std::io::stdout().flush()?;
            let mut url_input = String::new();
            std::io::stdin().read_line(&mut url_input)?;
            let url = url_input.trim().to_string();
            if url.is_empty() || !url.starts_with("http") {
                println!("  {} Invalid URL.", "■".red());
                return Ok(());
            }
            (url, "Custom", "gpt-3.5-turbo", "API_KEY")
        }
        _ => (
            "https://openrouter.ai/api/v1".to_string(),
            "OpenRouter",
            "meta-llama/llama-3-8b-instruct",
            "OPENROUTER_API_KEY",
        ),
    };

    // API key
    let existing_key = std::env::var(env_key_name).unwrap_or_default();
    let api_key = if !existing_key.is_empty() {
        println!("  {} Using API key from {} environment variable.", "■".green().bold(), env_key_name);
        existing_key
    } else {
        println!();
        if provider_name == "OpenRouter" {
            println!("  {} Get a free API key at: {}", "●".cyan().bold(), "https://openrouter.ai/keys".white());
        }
        print!("  Enter your API key: ");
        std::io::stdout().flush()?;
        let mut key_input = String::new();
        std::io::stdin().read_line(&mut key_input)?;
        let key = key_input.trim().to_string();
        if key.is_empty() {
            println!("  {} API key is required for online mode.", "■".red());
            println!("  {} You can set it later: {} models online --key <key>", "Tip:".dimmed(), "audio".white());
            return Ok(());
        }
        key
    };

    // Model selection
    println!();
    print!("  Model ID [{}]: ", default_model.dimmed());
    std::io::stdout().flush()?;
    let mut model_input = String::new();
    std::io::stdin().read_line(&mut model_input)?;
    let model_id = model_input.trim();
    let model_id = if model_id.is_empty() { default_model } else { model_id };

    // Save online config
    let config_dir = dirs::data_dir()
        .map(|d| if cfg!(windows) { d.join("Aud.io") } else { d.join("aud.io") })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    std::fs::create_dir_all(&config_dir)?;

    let online_config = serde_json::json!({
        "mode": "online",
        "provider": provider_name,
        "api_base_url": api_base_url,
        "model_id": model_id,
        "api_key": api_key,
    });

    let config_path = config_dir.join("online_config.json");
    std::fs::write(&config_path, serde_json::to_string_pretty(&online_config)?)?;

    println!();
    println!("  {} Online mode configured!", "■".green().bold());
    println!("    {} {}", "Provider:".dimmed(), provider_name.white());
    println!("    {} {}", "Model:".dimmed(), model_id.white());
    println!("    {} {}", "Config:".dimmed(), config_path.display().to_string().dimmed());
    println!();
    println!("  {} Switch to offline later with: {} models offline", "Tip:".dimmed(), "audio".white());

    Ok(())
}

/// Offline mode setup: select and download a model from HuggingFace/Ollama
async fn setup_offline_mode() -> anyhow::Result<()> {
    println!("  {} {}", "●".cyan().bold(), "Offline Mode Setup".white().bold());
    println!("  {}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".dimmed());
    println!();

    // Check if a model already exists
    if let Some(existing_model) = llama_binary::find_model_file() {
        println!("  {} Model already installed: {}", "■".green().bold(), existing_model.display().to_string().dimmed());
        return Ok(());
    }

    // Choose download source
    println!("  Where would you like to download a model from?");
    println!();
    println!("    {} {} — Direct GGUF downloads, largest selection", "1".cyan().bold(), "HuggingFace (recommended)".white().bold());
    println!("    {} {} — If you have Ollama installed", "2".cyan().bold(), "Ollama".white());
    println!();

    print!("  Source [1]: ");
    std::io::stdout().flush()?;
    let mut src_input = String::new();
    std::io::stdin().read_line(&mut src_input)?;
    let src = src_input.trim();
    let src = if src.is_empty() { "1" } else { src };

    println!();

    match src {
        "2" => {
            // Ollama flow
            println!("  {} Ollama models — run one of these:", "●".cyan().bold());
            println!();
            println!("    {} {}", "audio models install".white(), "ollama:llama3".cyan());
            println!("    {} {}", "audio models install".white(), "ollama:mistral".cyan());
            println!("    {} {}", "audio models install".white(), "ollama:codellama".cyan());
            println!("    {} {}", "audio models install".white(), "ollama:phi3".cyan());
            println!();
            println!("  {} Make sure Ollama is running: {}", "Tip:".dimmed(), "ollama serve".white());
        }
        _ => {
            // HuggingFace flow — show hardware-based recommendations
            let mut sys = sysinfo::System::new_all();
            sys.refresh_memory();
            let total_ram_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);

            let models: Vec<(&str, &str, &str, &str, f64)> = if total_ram_gb >= 16.0 {
                vec![
                    ("1", "Qwen2.5 Coder 7B (best for coding)", "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF", "qwen2.5-coder-7b-instruct-q4_k_m.gguf", 4.7),
                    ("2", "Mistral 7B Instruct v0.3", "MistralAI/Mistral-7B-Instruct-v0.3-GGUF", "mistral-7b-instruct-v0.3-q4_k_m.gguf", 4.4),
                    ("3", "Llama 3.1 8B Instruct", "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF", "Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf", 4.9),
                    ("4", "Code Llama 13B Instruct (needs 12+ GB)", "TheBloke/CodeLlama-13B-Instruct-GGUF", "codellama-13b-instruct.Q4_K_M.gguf", 7.9),
                ]
            } else if total_ram_gb >= 8.0 {
                vec![
                    ("1", "Phi-3.5 Mini (small & capable)", "bartowski/Phi-3.5-mini-instruct-GGUF", "Phi-3.5-mini-instruct-Q4_K_M.gguf", 2.2),
                    ("2", "Qwen2.5 Coder 3B", "Qwen/Qwen2.5-Coder-3B-Instruct-GGUF", "qwen2.5-coder-3b-instruct-q4_k_m.gguf", 2.0),
                    ("3", "Llama 3.2 3B Instruct", "bartowski/Llama-3.2-3B-Instruct-GGUF", "Llama-3.2-3B-Instruct-Q4_K_M.gguf", 2.0),
                ]
            } else {
                vec![
                    ("1", "Phi-3.5 Mini (lightweight)", "bartowski/Phi-3.5-mini-instruct-GGUF", "Phi-3.5-mini-instruct-Q4_K_M.gguf", 2.2),
                    ("2", "Qwen2.5 Coder 1.5B", "Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF", "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf", 1.0),
                ]
            };

            println!("  {} models for your system ({:.0} GB RAM):", "Recommended".white().bold(), total_ram_gb);
            println!();
            for (num, name, _repo, _file, size_gb) in &models {
                println!("    {} {} ({:.1} GB download)", num.cyan().bold(), name.white(), size_gb);
            }
            println!();

            print!("  Select a model [1]: ");
            std::io::stdout().flush()?;
            let mut choice_input = String::new();
            std::io::stdin().read_line(&mut choice_input)?;
            let choice = choice_input.trim();
            let choice = if choice.is_empty() { "1" } else { choice };

            if let Some((_num, name, repo_id, filename, _size)) = models.iter().find(|(n, _, _, _, _)| *n == choice) {
                println!();
                println!("  {} Downloading {} from HuggingFace...", "●".cyan().bold(), name);
                println!();

                // Direct download from HuggingFace
                let url = format!("https://huggingface.co/{}/resolve/main/{}", repo_id, filename);
                let models_dir = dirs::data_dir()
                    .map(|d| if cfg!(windows) { d.join("Aud.io") } else { d.join("aud.io") })
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("models");
                std::fs::create_dir_all(&models_dir)?;
                let dest_path = models_dir.join(filename);

                let client = reqwest::Client::builder()
                    .user_agent("Aud.io/0.1.1")
                    .build()?;

                let resp = client.get(&url).send().await;
                match resp {
                    Ok(response) if response.status().is_success() => {
                        let total_size = response.content_length().unwrap_or(0);
                        let pb = ProgressBar::new(total_size);
                        pb.set_style(
                            ProgressStyle::default_bar()
                                .template("  {msg}\n  [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                                .unwrap()
                                .progress_chars("━╸─"),
                        );
                        pb.set_message(format!("▼ Downloading {}", filename));

                        {
                            use futures_util::StreamExt;
                            let mut file = tokio::fs::File::create(&dest_path).await?;
                            let mut stream = response.bytes_stream();
                            let mut downloaded: u64 = 0;

                            while let Some(chunk) = stream.next().await {
                                let chunk = chunk?;
                                tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
                                downloaded += chunk.len() as u64;
                                pb.set_position(downloaded);
                            }
                            tokio::io::AsyncWriteExt::flush(&mut file).await?;
                        }

                        pb.finish_with_message("Download complete!");
                        println!();
                        println!("  {} Model installed: {}", "■".green().bold(), dest_path.display().to_string().dimmed());
                    }
                    Ok(response) => {
                        println!("  {} Download failed (HTTP {})", "■".red().bold(), response.status());
                        println!("  {} Try manually: {} models install {}", "Tip:".dimmed(), "audio".white(), repo_id);
                    }
                    Err(e) => {
                        println!("  {} Download failed: {}", "■".red().bold(), e);
                        println!("  {} Try manually: {} models install {}", "Tip:".dimmed(), "audio".white(), repo_id);
                    }
                }
            } else {
                println!("  {} Invalid selection.", "■".yellow());
            }
        }
    }

    Ok(())
}

// ─── Audio Info ──────────────────────────────────────────────────────────────

async fn run_audio_info() -> anyhow::Result<()> {
    println!();
    println!("  {} {}", "●".cyan().bold(), "audio system info".cyan().bold());
    println!("  {}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".dimmed());
    println!();

    let hw = llama_binary::get_hardware_summary();
    println!("  {} {}", "Hardware:".white().bold(), hw);

    match llama_binary::find_llama_binary() {
        Some(p) => println!("  {} {}", "llama-server:".white().bold(), p.display()),
        None => println!("  {} {}", "llama-server:".white().bold(), "not installed".red()),
    }

    match llama_binary::find_model_file() {
        Some(p) => println!("  {} {}", "Model:".white().bold(), p.display()),
        None => println!("  {} {}", "Model:".white().bold(), "none installed".red()),
    }

    if let Ok(bin_dir) = llama_binary::get_binary_dir() {
        println!("  {} {}", "Bin dir:".white().bold(), bin_dir.display());
    }

    let app_data = if cfg!(target_os = "windows") {
        dirs::data_dir().map(|d| d.join("Aud.io"))
    } else if cfg!(target_os = "macos") {
        dirs::data_dir().map(|d| d.join("Aud.io"))
    } else {
        dirs::data_dir().map(|d| d.join("aud.io"))
    };
    if let Some(d) = app_data {
        println!("  {} {}", "Data dir:".white().bold(), d.display());
        println!("  {} {}", "Models dir:".white().bold(), d.join("models").display());
    }

    println!();
    Ok(())
}

// ─── Server ──────────────────────────────────────────────────────────────────

async fn run_server() -> anyhow::Result<()> {
    dotenv().ok();
    let cfg = Config::from_env()?;
    println!("  {} {}", "●".cyan().bold(), "Starting Audio server...".cyan().bold());
    run_thread_server(cfg).await
}

// ─── Background server boot (for chat/commands that need the API) ────────────

async fn ensure_server_running() -> anyhow::Result<Client> {
    let client = Client::new();

    // Check if server is already running
    if client.get(format!("{}/healthz", BASE_URL)).send().await.is_ok() {
        return Ok(client);
    }

    // Boot server in background
    let spinner = ui::startup_spinner();
    spinner.set_message("Loading model...");

    tokio::spawn(async {
        dotenv().ok();
        if let Ok(cfg) = Config::from_env() {
            let _ = run_thread_server(cfg).await;
        }
    });

    // Wait for server to be ready
    for i in 0..120 {
        sleep(Duration::from_secs(1)).await;
        if i == 5 { spinner.set_message("Starting inference engine..."); }
        if i == 15 { spinner.set_message("Warming up..."); }
        if client.get(format!("{}/healthz", BASE_URL)).send().await.is_ok() {
            spinner.finish_and_clear();
            return Ok(client);
        }
    }

    spinner.finish_and_clear();
    anyhow::bail!("Backend server did not become ready within 120 seconds");
}

// ─── Chat REPL ───────────────────────────────────────────────────────────────

async fn run_chat(session: Option<String>) -> anyhow::Result<()> {
    let client = ensure_server_running().await?;
    let session_id = session.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Initialize mouse support
    let mouse_handler = match initialize_mouse_support() {
        Ok(handler) => {
            println!("{}", "Mouse support enabled".dimmed());
            Some(handler)
        },
        Err(e) => {
            println!("{}", format!("Mouse support unavailable: {}", e).yellow());
            None
        }
    };

    println!();
    println!("  {} {}", "●".cyan().bold(), "Audio Interactive Chat".cyan().bold());
    println!("  {}", "Type your message and press Enter. Commands:".dimmed());
    println!("  {}", "  /new     — start a new session".dimmed());
    println!("  {}", "  /history — list conversations".dimmed());
    println!("  {}", "  /exit    — quit".dimmed());
    println!();

    let config = rustyline::Config::builder()
        .edit_mode(rustyline::EditMode::Emacs)
        .auto_add_history(true)
        .build();
    let mut rl = rustyline::Editor::<(), rustyline::history::DefaultHistory>::with_config(config)?;
    let mut current_session = session_id;
    let mut messages: Vec<serde_json::Value> = Vec::new();

    loop {
        // Plain-text prompt — ANSI color codes break cursor positioning on Windows
        let prompt = "you> ";
        let line = match rl.readline(prompt) {
            Ok(l) => l,
            Err(rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("Input error: {}", e);
                break;
            }
        };

        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        // Handle slash commands
        match input {
            "/exit" | "/quit" => break,
            "/new" => {
                current_session = uuid::Uuid::new_v4().to_string();
                messages.clear();
                println!("{}", "New session started.".cyan());
                continue;
            }
            "/history" => {
                print_conversations(&client).await?;
                continue;
            }
            _ if input.starts_with('/') => {
                println!("{}", format!("Unknown command: {}", input).yellow());
                continue;
            }
            _ => {}
        }

        // Add user message to conversation history
        messages.push(serde_json::json!({"role": "user", "content": input}));

        let body = serde_json::json!({
            "messages": messages,
            "session_id": current_session,
            "max_tokens": 2000,
            "temperature": 0.7,
            "stream": true
        });

        let resp = client
            .post(format!("{}/generate/stream", BASE_URL))
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(response) => {
                print!("  {} {} {} {} ", "◣".cyan().bold(), "Audio".white().bold(), "│".dimmed(), "CLI".cyan().bold());
                std::io::stdout().flush()?;

                let mut full_response = String::new();
                let bytes_stream = response.bytes_stream();
                use futures_util::StreamExt;
                let mut stream = Box::pin(bytes_stream);
                let mut buffer = String::new();

                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(bytes) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));
                            // Process SSE lines
                            while let Some(pos) = buffer.find('\n') {
                                let line = buffer[..pos].to_string();
                                buffer = buffer[pos + 1..].to_string();

                                let line = line.trim();
                                if line.starts_with("data: ") {
                                    let data = &line[6..];
                                    if data == "[DONE]" {
                                        continue;
                                    }
                                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                                        if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                                            print!("{}", content);
                                            std::io::stdout().flush()?;
                                            full_response.push_str(content);
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("\n{}", format!("Stream error: {}", e).red());
                            break;
                        }
                    }
                }
                // Add assistant response to conversation history
                if !full_response.is_empty() {
                    messages.push(serde_json::json!({"role": "assistant", "content": full_response}));
                }
                println!();
                println!();
            }
            Err(e) => {
                eprintln!("{}", format!("Request failed: {}", e).red());
                // Remove the user message we added since the request failed
                messages.pop();
            }
        }
    }

    // Cleanup mouse support
    if let Some(handler) = mouse_handler {
        let _ = shutdown_mouse_support(&handler);
    }

    println!("{}", "Goodbye.".dimmed());
    Ok(())
}

// ─── Coding Agent Helpers ────────────────────────────────────────────────────

fn detect_project_type_for_display(project_root: &std::path::Path) -> String {
    let mut types = Vec::new();
    if project_root.join("Cargo.toml").exists() { types.push("Rust"); }
    if project_root.join("package.json").exists() { types.push("JS"); }
    if project_root.join("pyproject.toml").exists() || project_root.join("requirements.txt").exists() { types.push("Python"); }
    if project_root.join("go.mod").exists() { types.push("Go"); }
    if project_root.join("pom.xml").exists() || project_root.join("build.gradle").exists() { types.push("Java"); }
    if project_root.join("CMakeLists.txt").exists() { types.push("C/C++"); }
    if project_root.join(".git").exists() { types.push("git"); }
    if types.is_empty() { "unknown".to_string() } else { types.join(", ") }
}

fn get_git_branch_for_display(project_root: &std::path::Path) -> Option<String> {
    if !project_root.join(".git").exists() { return None; }
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(project_root)
        .output()
        .ok()?;
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}

// ─── Coding Agent REPL ───────────────────────────────────────────────────────

async fn run_code(path: Option<String>, session: Option<String>) -> anyhow::Result<()> {
    let client = ensure_server_running().await?;
    let session_id = session.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Initialize mouse support
    let mouse_handler = match initialize_mouse_support() {
        Ok(handler) => Some(handler),
        Err(_) => None,
    };

    // Determine project root
    let start_dir = match path {
        Some(ref p) => std::path::PathBuf::from(p),
        None => std::env::current_dir()?,
    };
    let project_root = agent::detect_project_root(&start_dir);

    // Detect project info for welcome screen
    let project_name = project_root.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".into());
    let project_type = detect_project_type_for_display(&project_root);
    let git_branch = get_git_branch_for_display(&project_root);

    // Clean welcome screen
    ui::welcome_screen(
        &project_name,
        &project_type,
        git_branch.as_deref(),
        &project_root,
    );

    let config = rustyline::Config::builder()
        .edit_mode(rustyline::EditMode::Emacs)
        .auto_add_history(true)
        .build();
    let mut rl = rustyline::Editor::<(), rustyline::history::DefaultHistory>::with_config(config)?;
    let mut current_session = session_id;

    // Build system prompt with tool definitions
    let system_prompt = agent::build_system_prompt(&project_root);
    let mut messages: Vec<serde_json::Value> = vec![
        serde_json::json!({"role": "system", "content": system_prompt}),
    ];

    loop {
        let prompt = "> ";
        let line = match rl.readline(prompt) {
            Ok(l) => l,
            Err(rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("Input error: {}", e);
                break;
            }
        };

        let input = line.trim();
        if input.is_empty() { continue; }

        // Handle slash commands
        match input {
            "/exit" | "/quit" | "/q" => break,
            "/new" => {
                current_session = uuid::Uuid::new_v4().to_string();
                let system_prompt = agent::build_system_prompt(&project_root);
                messages = vec![
                    serde_json::json!({"role": "system", "content": system_prompt}),
                ];
                println!("{}", "New session started.".cyan());
                continue;
            }
            "/tools" => {
                ui::show_tools(&tools::tool_definitions());
                continue;
            }
            "/history" => {
                print_conversations(&client).await?;
                continue;
            }
            "/help" => {
                ui::show_help();
                continue;
            }
            "/git" => {
                let result = tools::execute_tool("git_status", &serde_json::json!({}), &project_root);
                println!("{}", result.output);
                println!();
                continue;
            }
            "/diff" => {
                let result = tools::execute_tool("git_diff", &serde_json::json!({}), &project_root);
                println!("{}", result.output);
                println!();
                continue;
            }
            "/clear" | "/cls" => {
                // Clear screen
                if cfg!(windows) {
                    let _ = std::process::Command::new("cmd").args(["/C", "cls"]).status();
                } else {
                    print!("\x1B[2J\x1B[1;1H");
                    std::io::stdout().flush().ok();
                }
                continue;
            }
            "/model" => {
                // Show basic model info from config/server
                match client.get(format!("{}/models", BASE_URL)).send().await {
                    Ok(resp) => {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            println!("{}", "Active model info:".cyan().bold());
                            println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default().dimmed());
                        }
                    }
                    Err(e) => println!("{}", format!("Could not fetch model info: {}", e).yellow()),
                }
                println!();
                continue;
            }
            "/chat" => {
                println!("{}", "Switching to plain chat mode...".cyan());
                if let Some(handler) = mouse_handler {
                    let _ = shutdown_mouse_support(&handler);
                }
                return run_chat(Some(current_session)).await;
            }
            _ if input.starts_with('/') => {
                println!("{}", format!("Unknown command: {}. Type /help for commands.", input).yellow());
                continue;
            }
            _ => {}
        }

        // Add user message
        messages.push(serde_json::json!({"role": "user", "content": input}));

        // Run the agent loop — this handles tool calls internally
        match agent::run_agent_loop(
            &client,
            BASE_URL,
            &current_session,
            &mut messages,
            &project_root,
        ).await {
            Ok(_final_text) => {
                // Agent loop already printed output via streaming
                println!();
            }
            Err(e) => {
                eprintln!("{}", format!("Agent error: {}", e).red());
                // Remove the user message on failure
                if messages.last().map(|m| m["role"].as_str() == Some("user")).unwrap_or(false) {
                    messages.pop();
                }
            }
        }
    }

    // Cleanup mouse support
    if let Some(handler) = mouse_handler {
        let _ = shutdown_mouse_support(&handler);
    }

    println!("{}", "Goodbye.".dimmed());
    Ok(())
}

// ─── Conversations ───────────────────────────────────────────────────────────

async fn run_conversations(cmd: ConversationCmd) -> anyhow::Result<()> {
    let client = ensure_server_running().await?;

    match cmd {
        ConversationCmd::List => {
            print_conversations(&client).await?;
        }
        ConversationCmd::Show { id } => {
            let resp: serde_json::Value = client
                .get(format!("{}/conversations/{}", BASE_URL, id))
                .send()
                .await?
                .json()
                .await?;

            let title = resp["title"].as_str().unwrap_or("Untitled");
            println!("{}", format!("Conversation: {}", title).cyan().bold());
            println!();

            if let Some(messages) = resp["messages"].as_array() {
                for msg in messages {
                    let role = msg["role"].as_str().unwrap_or("?");
                    let content = msg["content"].as_str().unwrap_or("");
                    let label = match role {
                        "user" => "you>".green().bold().to_string(),
                        "assistant" => "ai>".purple().bold().to_string(),
                        "system" => "sys>".dimmed().to_string(),
                        _ => format!("{}>", role),
                    };
                    println!("{} {}", label, content);
                    println!();
                }
            }
        }
        ConversationCmd::Delete { id } => {
            client
                .delete(format!("{}/conversations/{}", BASE_URL, id))
                .send()
                .await?;
            println!("{}", "Conversation deleted.".green());
        }
        ConversationCmd::Pin { id } => {
            // Toggle pin — we fetch current state first
            let resp: serde_json::Value = client
                .get(format!("{}/conversations", BASE_URL))
                .send()
                .await?
                .json()
                .await?;

            let current_pinned = resp["conversations"]
                .as_array()
                .and_then(|convs| convs.iter().find(|c| c["id"].as_str() == Some(&id)))
                .and_then(|c| c["pinned"].as_bool())
                .unwrap_or(false);

            client
                .post(format!("{}/conversations/{}/pinned", BASE_URL, id))
                .json(&serde_json::json!({"pinned": !current_pinned}))
                .send()
                .await?;

            let status = if !current_pinned { "pinned" } else { "unpinned" };
            println!("{}", format!("Conversation {}.", status).green());
        }
    }

    Ok(())
}

async fn print_conversations(client: &Client) -> anyhow::Result<()> {
    let resp: serde_json::Value = client
        .get(format!("{}/conversations", BASE_URL))
        .send()
        .await?
        .json()
        .await?;

    if let Some(convs) = resp["conversations"].as_array() {
        if convs.is_empty() {
            println!("{}", "No conversations yet.".dimmed());
            return Ok(());
        }
        
        // Enhanced conversation display with borders, metadata, and visual formatting
        let width = 75;
        let border = "─".repeat(width - 2);
        
        // Header with clean rectangular borders
        println!("  ┌{}┐", "─".repeat(width));
        println!("  │ {:^width$} │", "● CONVERSATION HISTORY".cyan().bold(), width = width - 4);
        println!("  ├{}┤", border);
        println!("  │{}│", " ".repeat(width - 2));

        // Column headers
        println!("  │ {:<3} {:<45} {:<8} {:<8} │",
            "#".cyan().bold(),
            "Title".cyan().bold(),
            "Msgs".cyan().bold(),
            "Status".cyan().bold()
        );
        println!("  ├{}┼{}┼{}┼{}┤",
            "─".repeat(5),
            "─".repeat(47),
            "─".repeat(10),
            "─".repeat(10)
        );
        
        for (index, conv) in convs.iter().enumerate() {
            let _id = conv["id"].as_str().unwrap_or("?");
            let title = conv["title"].as_str().unwrap_or("Untitled");
            let count = conv["message_count"].as_u64().unwrap_or(0);
            let pinned = conv["pinned"].as_bool().unwrap_or(false);
            let _created_at = conv["created_at"].as_str().unwrap_or("");
            
            // Format title (truncate if too long)
            let display_title = if title.len() > 42 {
                format!("{}...", &title[..39])
            } else {
                title.to_string()
            };
            
            // Status indicator
            let status = if pinned {
                "PINNED".yellow().bold()
            } else {
                "ACTIVE".green()
            };
            
            // Format the conversation row
            println!("  │ {:<3} {:<45} {:<8} {:<8} │",
                (index + 1).to_string().cyan().bold(),
                display_title.white(),
                count.to_string().dimmed(),
                status
            );
        }
        
        println!("  │{}│", " ".repeat(width - 2));
        println!("  ├{}┤", border);

        // Footer with instructions
        let instruction = "↑/↓ Navigate  Enter Select  q Quit";
        let padding = (width - 4 - instruction.len()) / 2;
        println!("  │ {:>padding$}{}{:>padding$} │",
            "",
            instruction.dimmed(),
            "",
            padding = padding
        );

        println!("  └{}┘", "─".repeat(width));
        println!();
        
        // Additional info
        println!("{} conversations loaded • {} pinned", 
            convs.len().to_string().cyan().bold(),
            convs.iter().filter(|c| c["pinned"].as_bool().unwrap_or(false)).count().to_string().yellow().bold()
        );
        println!();
    }

    Ok(())
}

// ─── Models ──────────────────────────────────────────────────────────────────

async fn run_models(cmd: ModelCmd) -> anyhow::Result<()> {
    let client = ensure_server_running().await?;

    match cmd {
        ModelCmd::List => {
            let resp: serde_json::Value = client
                .get(format!("{}/models", BASE_URL))
                .send()
                .await?
                .json()
                .await?;

            println!("{}", "Models:".cyan().bold());
            println!();
            if let Some(models) = resp["models"].as_array() {
                if models.is_empty() {
                    println!("  {}", "No models installed.".dimmed());
                    println!();
                    println!("  Get started:");
                    println!("    {} {}", "audio models search".white(), "\"qwen 7b\"".dimmed());
                    println!("    {} {}", "audio models install".white(), "<model-id>".dimmed());
                } else {
                    for m in models {
                        let name = m["name"].as_str().unwrap_or("unknown");
                        let format = m["format"].as_str().unwrap_or("?");
                        let size = m["size_bytes"].as_u64().unwrap_or(0);
                        let size_gb = size as f64 / 1_073_741_824.0;
                        let status = m["status"].as_str().unwrap_or("unknown");
                        let status_display = match status {
                            "Installed" => "installed".green(),
                            "Downloading" => "downloading".yellow(),
                            _ => status.dimmed(),
                        };
                        println!(
                            "  {} {} {} {:.1} GB",
                            status_display,
                            name.white().bold(),
                            format!("({})", format).dimmed(),
                            size_gb,
                        );
                    }
                }
            } else {
                println!("{}", format!("{}", resp).dimmed());
            }
            println!();
        }
        ModelCmd::Search { query } => {
            let resp: serde_json::Value = client
                .get(format!("{}/models/search?query={}&limit=10", BASE_URL, query))
                .send()
                .await?
                .json()
                .await?;

            println!("{}", format!("Search results for '{}':", query).cyan().bold());
            println!();
            if let Some(models) = resp["models"].as_array() {
                for m in models {
                    let name = m["name"].as_str().unwrap_or("unknown");
                    let desc = m["description"].as_str().unwrap_or("");
                    println!("  {} — {}", name.white().bold(), desc.dimmed());
                }
            }
            println!();
        }
        ModelCmd::Install { model } => {
            let body = serde_json::json!({
                "model_id": model,
                "model_name": model,
                "source": "huggingface",
                "size_bytes": 0,
                "format": "gguf"
            });

            let resp: serde_json::Value = client
                .post(format!("{}/models/install", BASE_URL))
                .json(&body)
                .send()
                .await?
                .json()
                .await?;

            let msg = resp["message"].as_str().unwrap_or("Install started.");
            println!("{}", msg.green());

            // Poll progress
            if let Some(download_id) = resp["download_id"].as_str() {
                let pb = ProgressBar::new(100);
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template("  {msg} [{bar:40.cyan/blue}] {pos}%")
                        .unwrap()
                        .progress_chars("━╸─"),
                );
                pb.set_message("↓ Downloading");

                loop {
                    sleep(Duration::from_secs(2)).await;
                    let progress: serde_json::Value = client
                        .get(format!("{}/models/progress?download_id={}", BASE_URL, download_id))
                        .send()
                        .await?
                        .json()
                        .await?;

                    let pct = progress["progress_percent"].as_f64().unwrap_or(0.0);
                    pb.set_position(pct as u64);

                    if let Some(status) = progress["status"].as_str() {
                        if status == "completed" || status == "failed" {
                            pb.finish_with_message(format!("Download {}", status));
                            break;
                        }
                    }

                    if pct >= 100.0 {
                        pb.finish_with_message("Download complete.");
                        break;
                    }
                }
            }
        }
        ModelCmd::Remove { model } => {
            client
                .delete(format!("{}/models/remove?model_id={}", BASE_URL, model))
                .send()
                .await?;
            println!("{}", "Model removed.".green());
        }
        ModelCmd::Info => {
            let resp: serde_json::Value = client
                .get(format!("{}/hardware/info", BASE_URL))
                .send()
                .await?
                .json()
                .await?;

            println!("{}", "Hardware Info:".cyan().bold());
            println!("{}", serde_json::to_string_pretty(&resp)?);
            println!();

            let recs: serde_json::Value = client
                .get(format!("{}/hardware/recommendations", BASE_URL))
                .send()
                .await?
                .json()
                .await?;

            println!("{}", "Recommendations:".cyan().bold());
            if let Some(items) = recs["recommendations"].as_array() {
                for r in items {
                    if let Some(s) = r.as_str() {
                        println!("  - {}", s);
                    }
                }
            }
        }
        ModelCmd::Online { provider, model, key } => {
            // Resolve API key: explicit flag > env var
            let api_key = key.unwrap_or_else(|| {
                std::env::var("OPENROUTER_API_KEY")
                    .or_else(|_| std::env::var("OPENAI_API_KEY"))
                    .unwrap_or_default()
            });

            if api_key.is_empty() {
                println!();
                println!("  {} API key required for online models.", "■".red().bold());
                println!();
                println!("  Set one of these environment variables:");
                println!("    {} (for OpenRouter)", "OPENROUTER_API_KEY".white().bold());
                println!("    {} (for OpenAI)", "OPENAI_API_KEY".white().bold());
                println!();
                println!("  Or pass it directly:");
                println!("    {} {}", "audio models online --key".white(), "sk-...".dimmed());
                println!();
                return Ok(());
            }

            // Resolve provider URL
            let (api_base_url, provider_name) = match provider.as_str() {
                "openrouter" => ("https://openrouter.ai/api/v1".to_string(), "OpenRouter"),
                "openai" => ("https://api.openai.com/v1".to_string(), "OpenAI"),
                url if url.starts_with("http") => (url.to_string(), "Custom"),
                _ => {
                    println!("  {} Unknown provider '{}'. Use 'openrouter', 'openai', or a full URL.", "■".red(), provider);
                    return Ok(());
                }
            };

            // Default model per provider
            let model_id = model.unwrap_or_else(|| {
                match provider_name {
                    "OpenRouter" => "meta-llama/llama-3-8b-instruct".to_string(),
                    "OpenAI" => "gpt-4o-mini".to_string(),
                    _ => "meta-llama/llama-3-8b-instruct".to_string(),
                }
            });

            // Save the online config to AppData
            let config_dir = dirs::data_dir()
                .map(|d| if cfg!(windows) { d.join("Aud.io") } else { d.join("aud.io") })
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            std::fs::create_dir_all(&config_dir)?;

            let online_config = serde_json::json!({
                "mode": "online",
                "provider": provider_name,
                "api_base_url": api_base_url,
                "model_id": model_id,
                "api_key": api_key,
            });

            let config_path = config_dir.join("online_config.json");
            std::fs::write(&config_path, serde_json::to_string_pretty(&online_config)?)?;

            println!();
            println!("  {} Online model configured!", "■".green().bold());
            println!();
            println!("  {} {}", "Provider:".white().bold(), provider_name);
            println!("  {} {}", "Model:".white().bold(), model_id);
            println!("  {} {}", "API URL:".white().bold(), api_base_url);
            println!("  {} {}", "Config:".white().bold(), config_path.display().to_string().dimmed());
            println!();
            println!("  {} Run `audio code` to start coding with the online model.", "Next:".cyan());
            println!("  {} Switch back to offline with `audio models offline`.", "Tip:".dimmed());
            println!();
        }
        ModelCmd::Offline => {
            let config_dir = dirs::data_dir()
                .map(|d| if cfg!(windows) { d.join("Aud.io") } else { d.join("aud.io") })
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let config_path = config_dir.join("online_config.json");

            if config_path.exists() {
                std::fs::remove_file(&config_path)?;
            }

            println!();
            println!("  {} Switched to offline mode.", "■".green().bold());
            println!("  {} Will use the local model on next `audio code`.", "".dimmed());
            println!();
        }
    }

    Ok(())
}
