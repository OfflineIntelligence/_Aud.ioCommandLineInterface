//! CLI UI Components
//!
//! Provides beautiful terminal UI components for the Audio CLI:
//! - Mixed iconography: triangles (logo), squares, circles, hexagons
//! - Animated spinners (square chase, triangle rotation, hexagon pulse)
//! - Styled tool operation display (read, write, edit, shell)
//! - Welcome screen with single triangle brand mark
//! - Status bar with model/session info
//! - Diff display for file edits
//! - Progress indicators

use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

// ─── ANSI Helpers ────────────────────────────────────────────────────────────

/// Sleek rectangular separator line
pub fn separator() {
    println!("  {}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".dimmed());
}

/// Light separator
pub fn light_separator() {
    println!("  {}", "┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄".dimmed());
}

/// Print a blank line
pub fn blank() {
    println!();
}

// ─── Welcome Screen ──────────────────────────────────────────────────────────

/// Display the branded welcome screen — single triangle + Audio | CLI
pub fn welcome_screen(project_name: &str, project_type: &str, git_branch: Option<&str>, project_root: &Path) {
    blank();

    // Single isosceles triangle logo followed by brand name
    println!("  {}  {} {} {}", "◣".cyan().bold(), "Audio".white().bold(), "│".dimmed(), "CLI".cyan().bold());

    blank();

    // Tagline
    println!("  {}  {}", "○".dimmed(), "Privacy-first offline AI coding agent".dimmed());

    blank();
    separator();
    blank();

    // Project info with circle bullet
    print!("  {} {} {}",
        "●".green(),
        project_name.white().bold(),
        format!("({})", project_type).dimmed()
    );
    if let Some(branch) = git_branch {
        print!("  {} {}", "⎇".dimmed(), branch.green());
    }
    println!();
    println!("    {}", project_root.display().to_string().dimmed());
    blank();

    // Hint
    println!("  {} {}", "○".dimmed(), "Type a message or /help for commands".dimmed());
    blank();
}

// ─── Spinners ────────────────────────────────────────────────────────────────

/// Create a thinking spinner (square chase animation)
pub fn thinking_spinner() -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["□ □ □ □ □ □", "■ □ □ □ □ □", "□ ■ □ □ □ □", "□ □ ■ □ □ □", "□ □ □ ■ □ □", "□ □ □ □ ■ □", "□ □ □ □ □ ■"])
            .template("  {spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb.set_message("Thinking...");
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

/// Create a reasoning spinner (rotating triangle directions — kept as-is)
pub fn reasoning_spinner() -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["▲", "▶", "▼", "◀"])
            .template("  {spinner:.yellow} {msg}")
            .unwrap(),
    );
    pb.set_message("Reasoning...");
    pb.enable_steady_tick(Duration::from_millis(200));
    pb
}

/// Create a building/executing spinner (hexagon pulse fill)
pub fn working_spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&[
                "⬡ ⬡ ⬡ ⬡ ⬡",
                "⬢ ⬡ ⬡ ⬡ ⬡",
                "⬢ ⬢ ⬡ ⬡ ⬡",
                "⬢ ⬢ ⬢ ⬡ ⬡",
                "⬢ ⬢ ⬢ ⬢ ⬡",
                "⬢ ⬢ ⬢ ⬢ ⬢",
                "⬡ ⬢ ⬢ ⬢ ⬢",
                "⬡ ⬡ ⬢ ⬢ ⬢",
                "⬡ ⬡ ⬡ ⬢ ⬢",
                "⬡ ⬡ ⬡ ⬡ ⬢",
            ])
            .template("  {spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(120));
    pb
}

/// Create a server startup spinner (square progress bar)
pub fn startup_spinner() -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&[
                "■□□□□□□□",
                "■■□□□□□□",
                "■■■□□□□□",
                "■■■■□□□□",
                "■■■■■□□□",
                "■■■■■■□□",
                "■■■■■■■□",
                "■■■■■■■■",
            ])
            .template("  {spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb.set_message("Starting...");
    pb.enable_steady_tick(Duration::from_millis(200));
    pb
}

// ─── Progress Bar Style ──────────────────────────────────────────────────────

/// Create a styled download progress bar
pub fn download_progress_bar(total: u64, filename: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {msg}\n  [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("━╸─"),
    );
    pb.set_message(format!("↓ Downloading {}", filename));
    pb
}

/// Create a generic task progress bar
pub fn task_progress_bar(total: u64, msg: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {msg} [{bar:30.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("━╸─"),
    );
    pb.set_message(msg.to_string());
    pb
}

// ─── Tool Operation Display ──────────────────────────────────────────────────

/// Display a tool call start with mixed iconography
pub fn tool_start(name: &str, params: &serde_json::Value, project_root: &Path) {
    let (icon, label, detail) = match name {
        "read_file" => {
            let path = params["path"].as_str().unwrap_or("?");
            let short = shorten_path(path, project_root);
            ("▶".cyan().bold().to_string(), "Read".cyan().to_string(), short)
        }
        "write_file" => {
            let path = params["path"].as_str().unwrap_or("?");
            let short = shorten_path(path, project_root);
            let size_hint = params["content"].as_str().map(|c| format!("({} bytes)", c.len())).unwrap_or_default();
            ("⬢".green().bold().to_string(), "Write".green().to_string(), format!("{} {}", short, size_hint.dimmed()))
        }
        "replace_in_file" => {
            let path = params["path"].as_str().unwrap_or("?");
            let short = shorten_path(path, project_root);
            ("■".yellow().bold().to_string(), "Edit".yellow().to_string(), short)
        }
        "shell_exec" => {
            let cmd = params["command"].as_str().unwrap_or("?");
            let truncated = if cmd.len() > 80 { format!("{}...", &cmd[..77]) } else { cmd.to_string() };
            ("⬡".purple().bold().to_string(), "Run".purple().to_string(), truncated)
        }
        "list_dir" => {
            let path = params["path"].as_str().unwrap_or(".");
            ("▽".blue().bold().to_string(), "List".blue().to_string(), shorten_path(path, project_root))
        }
        "search_content" => {
            let pattern = params["pattern"].as_str().unwrap_or("?");
            ("···".magenta().bold().to_string(), "Search".magenta().to_string(), format!("/{}/", pattern))
        }
        "search_files" => {
            let pattern = params["pattern"].as_str().unwrap_or("?");
            ("···".magenta().bold().to_string(), "Glob".magenta().to_string(), pattern.to_string())
        }
        "git_status" => {
            ("●".green().bold().to_string(), "Git".green().to_string(), "status".to_string())
        }
        "git_diff" => {
            let path = params["path"].as_str().unwrap_or("");
            ("●".green().bold().to_string(), "Git".green().to_string(), format!("diff {}", path))
        }
        "git_log" => {
            let count = params["count"].as_u64().unwrap_or(10);
            ("●".green().bold().to_string(), "Git".green().to_string(), format!("log (last {})", count))
        }
        "batch_read_files" => {
            let count = params["paths"].as_array().map(|a| a.len()).unwrap_or(0);
            ("▶".cyan().bold().to_string(), "Read".cyan().to_string(), format!("{} files", count))
        }
        "get_project_info" => {
            ("●".blue().bold().to_string(), "Info".blue().to_string(), "project".to_string())
        }
        _ => {
            ("○".dimmed().to_string(), name.dimmed().to_string(), String::new())
        }
    };

    println!();
    println!("  {} {} {}", icon, label, detail.white());
}

/// Display a tool result with box indicators
pub fn tool_result(success: bool, output: &str) {
    if success {
        let preview = truncate_output(output, 100);
        if !preview.is_empty() {
            println!("  {} {}", "■".green().bold(), preview.dimmed());
        } else {
            println!("  {}", "■".green().bold());
        }
    } else {
        let preview = truncate_output(output, 120);
        println!("  {} {}", "■".red().bold(), preview.red());
    }
}

/// Display a tool result for write/edit operations with extra detail
pub fn tool_write_result(path: &str, bytes: usize, source: &str) {
    println!("  {} Written {} bytes to {} {}",
        "■".green().bold(),
        bytes.to_string().white().bold(),
        shorten_path_str(path).white(),
        format!("({})", source).dimmed()
    );
}

// ─── AI Response ─────────────────────────────────────────────────────────────

/// Print the AI response header — isosceles triangle + Audio | CLI
pub fn ai_response_header() {
    println!();
    print!("  {} {} {} {} ", "◣".cyan().bold(), "Audio".white().bold(), "│".dimmed(), "CLI".cyan().bold());
    std::io::stdout().flush().ok();
}

/// Print streamed text content (call repeatedly as tokens arrive)
pub fn ai_stream_token(token: &str) {
    print!("{}", token);
    std::io::stdout().flush().ok();
}

/// End AI response
pub fn ai_response_end() {
    println!();
}

// ─── Diff Display ────────────────────────────────────────────────────────────

/// Display a compact diff for file edits (replace_in_file)
pub fn show_edit_diff(path: &str, old_str: &str, new_str: &str, project_root: &Path) {
    let short_path = shorten_path(path, project_root);
    println!();
    println!("  {} {} {}", "■".yellow().bold(), "Edit".yellow(), short_path.white());
    separator();

    // Show removed lines
    for line in old_str.lines().take(5) {
        println!("  {} {}", "−".red(), line.red());
    }
    if old_str.lines().count() > 5 {
        println!("    {} {} more lines", "...".red(), old_str.lines().count() - 5);
    }

    // Show added lines
    for line in new_str.lines().take(5) {
        println!("  {} {}", "+".green(), line.green());
    }
    if new_str.lines().count() > 5 {
        println!("    {} {} more lines", "...".green(), new_str.lines().count() - 5);
    }
}

// ─── Status Bar ──────────────────────────────────────────────────────────────

/// Print a status line showing model and session info (hexagon prefix)
pub fn status_bar(model_name: &str, mode: &str, session_turns: usize) {
    let turns_str = if session_turns > 0 {
        format!(" │ {} turns", session_turns)
    } else {
        String::new()
    };

    println!("  {}",
        format!("⬢ {} │ {} │ Audio{}", mode, model_name, turns_str).dimmed()
    );
}

// ─── Help Display ────────────────────────────────────────────────────────────

/// Display the help menu with circular bullet points
pub fn show_help() {
    blank();
    println!("  {} {}", "●".cyan().bold(), "Commands".white().bold());
    separator();
    println!("  {} {:<14} {}", "○".cyan(), "/new".cyan(), "Start a new session".dimmed());
    println!("  {} {:<14} {}", "○".cyan(), "/tools".cyan(), "List available tools".dimmed());
    println!("  {} {:<14} {}", "○".cyan(), "/history".cyan(), "List past conversations".dimmed());
    println!("  {} {:<14} {}", "○".cyan(), "/git".cyan(), "Show git status".dimmed());
    println!("  {} {:<14} {}", "○".cyan(), "/diff".cyan(), "Show git diff".dimmed());
    println!("  {} {:<14} {}", "○".cyan(), "/clear".cyan(), "Clear screen".dimmed());
    println!("  {} {:<14} {}", "○".cyan(), "/model".cyan(), "Show active model info".dimmed());
    println!("  {} {:<14} {}", "○".cyan(), "/chat".cyan(), "Switch to plain chat mode".dimmed());
    println!("  {} {:<14} {}", "○".cyan(), "/exit".cyan(), "Quit".dimmed());
    blank();
}

/// Display available tools with circular bullets
pub fn show_tools(tools: &[crate::tools::ToolDef]) {
    blank();
    println!("  {} {}", "●".cyan().bold(), "Available Tools".white().bold());
    separator();
    for t in tools {
        println!("  {} {:<20} {}", "○".cyan(), t.name.cyan(), t.description.dimmed());
    }
    blank();
}

// ─── Error Recovery ──────────────────────────────────────────────────────────

/// Display an error hint (yellow box)
pub fn show_error_hint(hint: &str) {
    if !hint.is_empty() {
        println!("  {} {}", "■".yellow().bold(), hint.yellow());
    }
}

// ─── Utility Functions ───────────────────────────────────────────────────────

/// Shorten a file path relative to project root
fn shorten_path(path: &str, project_root: &Path) -> String {
    let root_str = project_root.to_string_lossy();
    if path.starts_with(root_str.as_ref()) {
        let relative = &path[root_str.len()..];
        let trimmed = relative.trim_start_matches(|c| c == '/' || c == '\\');
        if trimmed.is_empty() {
            ".".to_string()
        } else {
            trimmed.to_string()
        }
    } else {
        // Try to shorten home dir
        if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            if path.starts_with(&home) {
                return format!("~{}", &path[home.len()..]);
            }
        }
        path.to_string()
    }
}

/// Shorten path without project root context
fn shorten_path_str(path: &str) -> String {
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        if path.starts_with(&home) {
            return format!("~{}", &path[home.len()..]);
        }
    }
    path.to_string()
}

/// Truncate output for display
fn truncate_output(output: &str, max_chars: usize) -> String {
    let first_line = output.lines().next().unwrap_or("");
    if first_line.len() > max_chars {
        let end = first_line.char_indices()
            .take_while(|(i, _)| *i < max_chars)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(max_chars.min(first_line.len()));
        format!("{}...", &first_line[..end])
    } else if output.lines().count() > 1 {
        format!("{} (+{} lines)", first_line, output.lines().count() - 1)
    } else {
        first_line.to_string()
    }
}
