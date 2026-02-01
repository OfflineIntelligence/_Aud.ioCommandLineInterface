//! Coding agent — agentic loop that lets the LLM use tools.
//!
//! The agent works by:
//! 1. Sending conversation + system prompt (with tool definitions) to the LLM
//! 2. Parsing the LLM response for <tool_call> blocks
//! 3. Executing requested tools
//! 4. Feeding tool results back as a new message
//! 5. Repeating until the LLM gives a final answer with no tool calls
//!
//! This runs entirely through the existing HTTP backend (no direct llama-server calls).


use std::path::{Path, PathBuf};

use colored::Colorize;
use reqwest::Client;

use crate::tools;
use crate::utils::{get_home_directory, get_desktop_path, get_documents_path};

const MAX_AGENT_TURNS: usize = 25;

/// Detect the operating system and shell environment
fn get_platform_info() -> (String, String) {
    let os = if cfg!(windows) {
        "Windows".to_string()
    } else if cfg!(target_os = "macos") {
        "macOS".to_string()
    } else {
        "Linux".to_string()
    };

    let shell = if cfg!(windows) {
        // Check if PowerShell is available
        if std::process::Command::new("powershell")
            .arg("-Command")
            .arg("echo ok")
            .output()
            .is_ok()
        {
            "PowerShell".to_string()
        } else {
            "cmd.exe".to_string()
        }
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    };

    (os, shell)
}

/// Detect project type from project root markers
fn detect_project_type(project_root: &Path) -> String {
    let mut types = Vec::new();
    if project_root.join("Cargo.toml").exists() { types.push("Rust"); }
    if project_root.join("package.json").exists() { types.push("Node.js/JavaScript"); }
    if project_root.join("pyproject.toml").exists() || project_root.join("setup.py").exists() || project_root.join("requirements.txt").exists() { types.push("Python"); }
    if project_root.join("go.mod").exists() { types.push("Go"); }
    if project_root.join("pom.xml").exists() || project_root.join("build.gradle").exists() { types.push("Java"); }
    if project_root.join("CMakeLists.txt").exists() { types.push("C/C++"); }
    if project_root.join("composer.json").exists() { types.push("PHP"); }
    if project_root.join(".git").exists() { types.push("git"); }
    if types.is_empty() { "Unknown".to_string() } else { types.join(", ") }
}

/// Get git branch info if in a git repo
fn get_git_info(project_root: &Path) -> Option<String> {
    if !project_root.join(".git").exists() {
        return None;
    }
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(project_root)
        .output()
        .ok()?;
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}

/// Build the system prompt that instructs the LLM how to use tools.
pub fn build_system_prompt(project_root: &Path) -> String {
    let tool_defs = tools::tool_definitions();
    let tools_json: Vec<serde_json::Value> = tool_defs
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "parameters": serde_json::from_str::<serde_json::Value>(t.parameters).unwrap_or_default()
            })
        })
        .collect();

    let tools_str = serde_json::to_string_pretty(&tools_json).unwrap_or_default();

    let (os_name, shell) = get_platform_info();
    let home_dir = get_home_directory().map(|p| p.display().to_string()).unwrap_or_else(|_| "unknown".into());
    let desktop_path = get_desktop_path().map(|p| p.display().to_string()).unwrap_or_else(|_| "unknown".into());
    let documents_path = get_documents_path().map(|p| p.display().to_string()).unwrap_or_else(|_| "unknown".into());
    let project_type = detect_project_type(project_root);
    let git_branch = get_git_info(project_root).unwrap_or_else(|| "none".into());
    let username = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "user".into());

    format!(
r#"You are Aud.io CLI, a privacy-first offline coding assistant running entirely on the user's local machine. You NEVER reveal the name of the underlying model you run on. If asked "what model are you" or "who are you", respond that you are Aud.io CLI — an offline, local AI coding assistant. You are direct, precise, and take action immediately. You DO NOT ask for confirmation on straightforward tasks — you just do them.

## Environment
- OS: {os_name}
- Shell: {shell}
- User: {username}
- Home: {home_dir}
- Desktop: {desktop_path}
- Documents: {documents_path}
- Project root: {project_root}
- Project type: {project_type}
- Git branch: {git_branch}

## Available Tools
{tools_str}

## Tool Call Format
To use a tool, emit a block in EXACTLY this format (no extra text inside the block):

<tool_call>
{{"name": "tool_name", "params": {{"param1": "value1"}}}}
</tool_call>

## Rules

### Be Decisive
- For simple requests like "write a file" or "create X", just DO IT immediately with tool calls. Don't explain what you plan to do first.
- Use MULTIPLE tool calls in a single response when possible. Each in its own <tool_call> block.
- When you have enough information, respond WITHOUT any tool_call blocks.

### Paths
- ALWAYS use absolute paths for file operations. The user's Desktop is at: {desktop_path}
- ALWAYS use absolute paths for the user's directories. NEVER use placeholder paths like "path/to/your/Desktop" or "YourUsername".
- For project files, use paths relative to the project root: {project_root}
- When the user says "save to desktop" or "save on my desktop", write to: {desktop_path}/filename

### Shell Commands
- You are on {os_name} using {shell}. Use the correct syntax for this platform.
- On Windows: use `dir` not `ls`, `type` not `cat`, `echo %VAR%` not `echo $VAR`, `\\` not `/` in paths.
- On Windows: do NOT use Unix commands like `cd ~`, `echo $HOME`, `ls`. Use `cd %USERPROFILE%`, `echo %USERPROFILE%`, `dir`.
- Use shell_exec for git, build, test, and other CLI operations.

### File Operations
- NEVER guess file contents. Always read_file first before editing.
- Use replace_in_file for surgical edits (preferred). Use write_file only for new files or full rewrites.
- Keep edits minimal and focused. Preserve existing code style.

### Workflow
- For complex tasks, plan your approach, then execute step by step.
- After making changes, verify by reading the file or running tests.
- If a tool fails, analyze the error and try an alternative approach. Don't give up.
- If you encounter a permission error, try the alternative path or inform the user.

### Response Style
- Be concise. Don't narrate what you're about to do — just do it.
- After completing a task, briefly confirm what was done.
- If a task is genuinely ambiguous, ask ONE clarifying question.

### Identity
- You are **Aud.io CLI** — an offline, privacy-first AI coding assistant.
- You run 100% locally on the user's machine. No data leaves their computer.
- NEVER mention or reveal the underlying model name (e.g. do NOT say "I am Phi", "I am Llama", "I am Qwen", etc.)
- If asked who you are or what model you use, say: "I am Aud.io CLI, a local offline AI coding assistant."
- When asked about your capabilities, describe what you can actually do with your tools (listed above), not generic AI capabilities.

### Your Actual Capabilities (via tools)
- **Read, write, and edit files** on the user's system (read_file, write_file, replace_in_file)
- **Run shell commands** in the user's terminal (shell_exec) — build, test, git, etc.
- **Search codebases** by file name patterns (search_files) or content (search_content)
- **List directories** to explore project structure (list_dir)
- **Git integration** — check status, diff, and log (git_status, git_diff, git_log)
- **Project analysis** — detect project type, languages, structure (get_project_info)
- **Batch file reading** — read multiple files at once (batch_read_files)
- You CAN understand, analyze, and modify code repositories to implement features, fix bugs, and refactor.
- You operate agentically: you read code, make changes, run tests, and iterate until the task is done."#,
        os_name = os_name,
        shell = shell,
        username = username,
        home_dir = home_dir,
        desktop_path = desktop_path,
        documents_path = documents_path,
        project_root = project_root.display(),
        project_type = project_type,
        git_branch = git_branch,
        tools_str = tools_str,
    )
}

/// Parse all <tool_call>...</tool_call> blocks from an LLM response.
/// Returns (tool_calls, text_outside_tool_calls).
pub fn parse_tool_calls(response: &str) -> (Vec<(String, serde_json::Value)>, String) {
    let mut calls = Vec::new();
    let mut text = String::new();
    let mut remaining = response;

    loop {
        if let Some(start) = remaining.find("<tool_call>") {
            // Text before the tool call
            text.push_str(&remaining[..start]);

            let after_tag = &remaining[start + 11..]; // len("<tool_call>") = 11
            if let Some(end) = after_tag.find("</tool_call>") {
                let json_str = after_tag[..end].trim();
                match serde_json::from_str::<serde_json::Value>(json_str) {
                    Ok(val) => {
                        let name = val["name"].as_str().unwrap_or("").to_string();
                        let params = val["params"].clone();
                        if !name.is_empty() {
                            calls.push((name, params));
                        }
                    }
                    Err(e) => {
                        text.push_str(&format!("[Failed to parse tool call: {}]", e));
                    }
                }
                remaining = &after_tag[end + 12..]; // len("</tool_call>") = 12
            } else {
                // Unclosed tag — treat rest as text
                text.push_str(&remaining[start..]);
                break;
            }
        } else {
            text.push_str(remaining);
            break;
        }
    }

    (calls, text.trim().to_string())
}

/// Run the agent loop: send messages, parse tool calls, execute, repeat.
/// Returns when the LLM gives a final response with no tool calls.
pub async fn run_agent_loop(
    client: &Client,
    base_url: &str,
    session_id: &str,
    messages: &mut Vec<serde_json::Value>,
    project_root: &Path,
) -> anyhow::Result<String> {
    for _turn in 0..MAX_AGENT_TURNS {
        // Send to LLM (non-streaming for agent loop — we need the full response to parse tool calls)
        let body = serde_json::json!({
            "messages": messages,
            "session_id": session_id,
            "max_tokens": 4096,
            "temperature": 0.2,
            "stream": true
        });

        let resp = client
            .post(format!("{}/generate/stream", base_url))
            .json(&body)
            .send()
            .await?;

        // Collect the streamed response, printing text parts as they arrive
        let full_response = collect_stream_response(resp, project_root).await?;

        // Parse for tool calls
        let (tool_calls, text) = parse_tool_calls(&full_response);

        // Add assistant response to history
        messages.push(serde_json::json!({
            "role": "assistant",
            "content": full_response
        }));

        if tool_calls.is_empty() {
            // No tool calls — this is the final answer
            return Ok(text);
        }

        // Execute each tool and collect results
        let mut tool_results = Vec::new();
        for (name, params) in &tool_calls {
            // Display tool operation with styled UI
            crate::ui::tool_start(name, params, project_root);

            // Show diff preview for edit operations
            if name == "replace_in_file" {
                if let (Some(path), Some(old_s), Some(new_s)) = (
                    params["path"].as_str(),
                    params["old_string"].as_str(),
                    params["new_string"].as_str(),
                ) {
                    crate::ui::show_edit_diff(path, old_s, new_s, project_root);
                }
            }

            let result = tools::execute_tool(name, params, project_root);

            // Display result with styled UI
            crate::ui::tool_result(result.success, &result.output);

            // Add recovery hints for common errors
            let mut output = result.output.clone();
            if !result.success {
                let hint = generate_error_hint(name, &result.output, project_root);
                crate::ui::show_error_hint(&hint);
                output.push_str(&hint);
            }

            tool_results.push(serde_json::json!({
                "tool": name,
                "success": result.success,
                "output": output
            }));
        }

        // Feed tool results back to the LLM
        let results_str = serde_json::to_string_pretty(&tool_results)?;
        messages.push(serde_json::json!({
            "role": "user",
            "content": format!("Tool results:\n{}", results_str)
        }));

        println!();
    }

    Ok("Agent reached maximum turns. Please continue with a new message.".into())
}

/// Collect a streamed SSE response, printing text content as it arrives.
/// Hides <tool_call> XML blocks from the user output.
/// Returns the full text content (including tool call blocks for parsing).
async fn collect_stream_response(
    response: reqwest::Response,
    _project_root: &Path,
) -> anyhow::Result<String> {
    use futures_util::StreamExt;

    let mut full_response = String::new();
    let bytes_stream = response.bytes_stream();
    let mut stream = Box::pin(bytes_stream);
    let mut buffer = String::new();
    let mut printed_header = false;
    // Track whether we're inside a <tool_call> block to suppress printing
    let mut inside_tool_call = false;
    // Buffer text that might be part of a tool_call tag
    let mut pending_text = String::new();

    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                buffer.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].to_string();
                    buffer = buffer[pos + 1..].to_string();

                    let line = line.trim();
                    if line.starts_with("data: ") {
                        let data = &line[6..];
                        if data == "[DONE]" { continue; }
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                                full_response.push_str(content);

                                // Buffer and filter out tool_call blocks
                                pending_text.push_str(content);

                                // Check for tool_call start
                                if pending_text.contains("<tool_call>") {
                                    // Print everything before the tag
                                    if let Some(tag_pos) = pending_text.find("<tool_call>") {
                                        let before = &pending_text[..tag_pos];
                                        if !before.trim().is_empty() {
                                            if !printed_header {
                                                crate::ui::ai_response_header();
                                                printed_header = true;
                                            }
                                            crate::ui::ai_stream_token(before);
                                        }
                                    }
                                    inside_tool_call = true;
                                    pending_text.clear();
                                    continue;
                                }

                                // Check for tool_call end
                                if inside_tool_call {
                                    if pending_text.contains("</tool_call>") {
                                        inside_tool_call = false;
                                        // Discard everything in the tool call block
                                        if let Some(end_pos) = pending_text.find("</tool_call>") {
                                            pending_text = pending_text[end_pos + 12..].to_string();
                                        }
                                    } else {
                                        pending_text.clear();
                                    }
                                    continue;
                                }

                                // If we might be starting a tag, hold text
                                if pending_text.contains('<') && !pending_text.contains('>') {
                                    // Could be partial tag, keep buffering
                                    continue;
                                }

                                // Print buffered text
                                if !pending_text.is_empty() && !inside_tool_call {
                                    if !printed_header {
                                        crate::ui::ai_response_header();
                                        printed_header = true;
                                    }
                                    crate::ui::ai_stream_token(&pending_text);
                                    pending_text.clear();
                                }
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

    // Flush any remaining pending text
    if !pending_text.is_empty() && !inside_tool_call {
        if !printed_header {
            crate::ui::ai_response_header();
            printed_header = true;
        }
        crate::ui::ai_stream_token(&pending_text);
    }

    if printed_header {
        crate::ui::ai_response_end();
    }

    Ok(full_response)
}

/// Generate helpful hints when a tool fails, to help the LLM recover
fn generate_error_hint(tool_name: &str, error: &str, project_root: &Path) -> String {
    let error_lower = error.to_lowercase();

    if error_lower.contains("access is denied") || error_lower.contains("permission denied") {
        let home = get_home_directory().map(|p| p.display().to_string()).unwrap_or_default();
        return format!(
            "\n[Hint: Permission denied. The user's home directory is {}. Try writing to a location within the project root ({}) or the user's Documents folder instead.]",
            home, project_root.display()
        );
    }

    if error_lower.contains("cannot find the file") || error_lower.contains("no such file") {
        return format!(
            "\n[Hint: File not found. Use list_dir or search_files to find the correct path. Project root is {}]",
            project_root.display()
        );
    }

    if error_lower.contains("not a git repository") {
        return "\n[Hint: This directory is not a git repository. Use 'git init' first if needed.]".to_string();
    }

    if tool_name == "shell_exec" {
        if error_lower.contains("not recognized") || error_lower.contains("not found") {
            if cfg!(windows) {
                return "\n[Hint: Command not found. On Windows, use PowerShell syntax. Common alternatives: dir (not ls), type (not cat), echo %VAR% (not echo $VAR).]".to_string();
            } else {
                return "\n[Hint: Command not found. Check if the tool is installed and in PATH.]".to_string();
            }
        }
    }

    String::new()
}

/// Detect the project root by looking for common markers.
pub fn detect_project_root(start: &Path) -> PathBuf {
    let markers = [
        ".git", "Cargo.toml", "package.json", "go.mod", "pyproject.toml",
        "setup.py", "Makefile", "CMakeLists.txt", ".project", "pom.xml",
        "build.gradle", "composer.json",
    ];

    let mut dir = start.to_path_buf();
    loop {
        for marker in &markers {
            if dir.join(marker).exists() {
                return dir;
            }
        }
        if !dir.pop() {
            // No project root found — use start directory
            return start.to_path_buf();
        }
    }
}
