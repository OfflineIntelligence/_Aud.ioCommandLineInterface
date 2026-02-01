//! Coding agent tools — file I/O, search, and shell execution.
//!
//! Each tool takes a JSON `params` object and returns a string result.
//! Tools are designed to be safe: they operate within a project root,
//! and shell execution is explicit and user-visible.
//!
//! Enhanced with intelligent path resolution and sophisticated confirmation workflows.

pub mod system_tools;
pub mod file_advanced;
pub mod network_tools;

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::debug;
use crate::utils::{resolve_path_with_fallbacks, ConfirmationSystem, OperationContext, 
                   OperationType, create_operation_context};

/// Global confirmation system instance
static mut CONFIRMATION_SYSTEM: Option<ConfirmationSystem> = None;

/// Initialize the confirmation system
pub fn initialize_confirmation_system() {
    unsafe {
        if CONFIRMATION_SYSTEM.is_none() {
            CONFIRMATION_SYSTEM = Some(ConfirmationSystem::new());
        }
    }
}

/// Get mutable reference to confirmation system
fn get_confirmation_system() -> Option<&'static mut ConfirmationSystem> {
    unsafe { CONFIRMATION_SYSTEM.as_mut() }
}
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: &'static str, // JSON schema as string
}

/// All available tools for the coding agent.
pub fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "read_file",
            description: "Read the contents of a file. Returns the full file content with line numbers.",
            parameters: r#"{"type":"object","properties":{"path":{"type":"string","description":"Relative or absolute file path"}},"required":["path"]}"#,
        },
        ToolDef {
            name: "write_file",
            description: "Write content to a file. Creates the file if it doesn't exist, overwrites if it does. Creates parent directories as needed.",
            parameters: r#"{"type":"object","properties":{"path":{"type":"string","description":"File path to write to"},"content":{"type":"string","description":"Full file content to write"}},"required":["path","content"]}"#,
        },
        ToolDef {
            name: "replace_in_file",
            description: "Replace an exact string in a file with new content. The old_string must match exactly (including whitespace/indentation). Use this for surgical edits instead of rewriting entire files.",
            parameters: r#"{"type":"object","properties":{"path":{"type":"string","description":"File path"},"old_string":{"type":"string","description":"Exact string to find and replace"},"new_string":{"type":"string","description":"Replacement string"}},"required":["path","old_string","new_string"]}"#,
        },
        ToolDef {
            name: "list_dir",
            description: "List files and directories at a path. Returns names with [DIR] or [FILE] prefix. Non-recursive by default.",
            parameters: r#"{"type":"object","properties":{"path":{"type":"string","description":"Directory path (default: project root)"},"recursive":{"type":"boolean","description":"If true, list recursively (max 200 entries)"}},"required":[]}"#,
        },
        ToolDef {
            name: "search_files",
            description: "Search for files by name pattern (glob). Returns matching file paths.",
            parameters: r#"{"type":"object","properties":{"pattern":{"type":"string","description":"Glob pattern, e.g. '**/*.rs', 'src/**/*.ts'"},"path":{"type":"string","description":"Directory to search in (default: project root)"}},"required":["pattern"]}"#,
        },
        ToolDef {
            name: "search_content",
            description: "Search file contents for a regex pattern (like grep/ripgrep). Returns matching lines with file paths and line numbers.",
            parameters: r#"{"type":"object","properties":{"pattern":{"type":"string","description":"Regex pattern to search for"},"path":{"type":"string","description":"Directory to search in (default: project root)"},"file_pattern":{"type":"string","description":"Optional glob to filter files, e.g. '*.rs'"}},"required":["pattern"]}"#,
        },
        ToolDef {
            name: "shell_exec",
            description: "Execute a shell command and return stdout/stderr. Use for build, test, git, and other CLI operations. Commands run from the project root.",
            parameters: r#"{"type":"object","properties":{"command":{"type":"string","description":"Shell command to execute"}},"required":["command"]}"#,
        },
        ToolDef {
            name: "git_status",
            description: "Get git status of the project. Returns branch, staged/unstaged changes, and untracked files.",
            parameters: r#"{"type":"object","properties":{},"required":[]}"#,
        },
        ToolDef {
            name: "git_diff",
            description: "Show git diff of current changes (unstaged). Optionally specify a file path.",
            parameters: r#"{"type":"object","properties":{"path":{"type":"string","description":"Optional file path to diff"}},"required":[]}"#,
        },
        ToolDef {
            name: "git_log",
            description: "Show recent git commits. Returns last 10 commits with hash, author, and message.",
            parameters: r#"{"type":"object","properties":{"count":{"type":"integer","description":"Number of commits to show (default: 10)"}},"required":[]}"#,
        },
        ToolDef {
            name: "get_project_info",
            description: "Get project overview: type, dependencies, structure, git state. Use this to understand a project before making changes.",
            parameters: r#"{"type":"object","properties":{},"required":[]}"#,
        },
        ToolDef {
            name: "batch_read_files",
            description: "Read multiple files in one call. More efficient than multiple read_file calls. Returns contents of all files.",
            parameters: r#"{"type":"object","properties":{"paths":{"type":"array","items":{"type":"string"},"description":"Array of file paths to read"}},"required":["paths"]}"#,
        },
    ]
}

/// Result from a tool execution.
#[derive(Debug, Serialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
}

/// Execute a tool by name with JSON params.
pub fn execute_tool(
    name: &str,
    params: &serde_json::Value,
    project_root: &Path,
) -> ToolResult {
    debug!("Executing tool '{}' with params: {}", name, params);
    
    // Initialize confirmation system if not already done
    initialize_confirmation_system();
    
    // Check if confirmation is needed for operations
    if let Some(context) = create_operation_context_for_tool(name, params, project_root) {
        if let Some(confirmation_system) = get_confirmation_system() {
            match confirmation_system.request_confirmation(&context) {
                Ok(true) => {
                    // Operation approved, continue
                },
                Ok(false) => {
                    return ToolResult {
                        success: false,
                        output: "Operation cancelled by user".into(),
                    };
                },
                Err(e) => {
                    return ToolResult {
                        success: false,
                        output: format!("Confirmation error: {}", e),
                    };
                }
            }
        }
    }
    
    match name {
        "read_file" => tool_read_file(params, project_root),
        "write_file" => tool_write_file_enhanced(params, project_root),
        "replace_in_file" => tool_replace_in_file_enhanced(params, project_root),
        "list_dir" => tool_list_dir(params, project_root),
        "search_files" => tool_search_files(params, project_root),
        "search_content" => tool_search_content(params, project_root),
        "shell_exec" => tool_shell_exec_enhanced(params, project_root),
        "git_status" => tool_git_status(project_root),
        "git_diff" => tool_git_diff(params, project_root),
        "git_log" => tool_git_log(params, project_root),
        "get_project_info" => tool_get_project_info(project_root),
        "batch_read_files" => tool_batch_read_files(params, project_root),
        _ => ToolResult {
            success: false,
            output: format!("Unknown tool: {}", name),
        },
    }
}

/// Create operation context for tool execution
fn create_operation_context_for_tool(
    tool_name: &str, 
    params: &serde_json::Value, 
    project_root: &Path
) -> Option<OperationContext> {
    match tool_name {
        "write_file" => {
            let path_str = params["path"].as_str()?;
            let content = params["content"].as_str().unwrap_or("");
            
            // Use enhanced path resolution
            let resolved_path = match resolve_path_with_fallbacks(path_str, project_root) {
                Ok(resolved) => resolved,
                Err(_) => return None, // Skip confirmation if path resolution fails
            };
            
            let operation_type = if resolved_path.path.exists() {
                OperationType::ModifyFile
            } else {
                OperationType::WriteFile
            };
            
            let _risk_level = resolved_path.risk_level();
            let alternatives = resolved_path.alternatives
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            
            Some(create_operation_context(
                operation_type,
                Some(&resolved_path.path.to_string_lossy()),
                Some(content),
                alternatives,
            ))
        },
        "replace_in_file" => {
            let path_str = params["path"].as_str()?;
            let old_string = params["old_string"].as_str().unwrap_or("");
            
            let resolved_path = match resolve_path_with_fallbacks(path_str, project_root) {
                Ok(resolved) => resolved,
                Err(_) => return None,
            };
            
            Some(create_operation_context(
                OperationType::ModifyFile,
                Some(&resolved_path.path.to_string_lossy()),
                Some(old_string),
                resolved_path.alternatives
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect(),
            ))
        },
        "shell_exec" => {
            let command = params["command"].as_str()?;
            
            Some(create_operation_context(
                OperationType::ExecuteCommand,
                None,
                Some(command),
                vec![],
            ))
        },
        "delete_file" => {
            let path_str = params["path"].as_str()?;
            
            let resolved_path = match resolve_path_with_fallbacks(path_str, project_root) {
                Ok(resolved) => resolved,
                Err(_) => return None,
            };
            
            Some(create_operation_context(
                OperationType::DeleteFile,
                Some(&resolved_path.path.to_string_lossy()),
                None,
                resolved_path.alternatives
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect(),
            ))
        },
        _ => None,
    }
}

/// Enhanced write file tool with better path resolution and error handling
fn tool_write_file_enhanced(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let path_str = match params["path"].as_str() {
        Some(p) => p,
        None => return ToolResult { success: false, output: "Missing 'path' parameter".into() },
    };
    let content = match params["content"].as_str() {
        Some(c) => c,
        None => return ToolResult { success: false, output: "Missing 'content' parameter".into() },
    };
    
    // Use enhanced path resolution
    let resolved_path = match resolve_path_with_fallbacks(path_str, project_root) {
        Ok(resolved) => resolved,
        Err(e) => return ToolResult {
            success: false,
            output: format!("Failed to resolve path '{}': {}", path_str, e),
        },
    };
    
    // Try to create parent directories
    if let Some(parent) = resolved_path.path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            // If we can't create directories, try alternatives
            if !resolved_path.alternatives.is_empty() {
                return try_alternative_paths(
                    &resolved_path.alternatives, 
                    content, 
                    "write file"
                );
            }
            return ToolResult {
                success: false,
                output: format!("Failed to create directories: {}", e),
            };
        }
    }
    
    // Attempt to write to the resolved path
    match std::fs::write(&resolved_path.path, content) {
        Ok(()) => ToolResult {
            success: true,
            output: format!("Written {} bytes to {} (source: {:?})", 
                           content.len(), 
                           resolved_path.path.display(),
                           resolved_path.source),
        },
        Err(e) => {
            // Try alternatives if primary path fails
            if !resolved_path.alternatives.is_empty() {
                try_alternative_paths(&resolved_path.alternatives, content, "write file")
            } else {
                ToolResult {
                    success: false,
                    output: format!("Failed to write '{}': {}", resolved_path.path.display(), e),
                }
            }
        }
    }
}

/// Enhanced replace in file tool
fn tool_replace_in_file_enhanced(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let path_str = match params["path"].as_str() {
        Some(p) => p,
        None => return ToolResult { success: false, output: "Missing 'path' parameter".into() },
    };
    let old_string = match params["old_string"].as_str() {
        Some(s) => s,
        None => return ToolResult { success: false, output: "Missing 'old_string' parameter".into() },
    };
    let new_string = match params["new_string"].as_str() {
        Some(s) => s,
        None => return ToolResult { success: false, output: "Missing 'new_string' parameter".into() },
    };
    
    // Use enhanced path resolution
    let resolved_path = match resolve_path_with_fallbacks(path_str, project_root) {
        Ok(resolved) => resolved,
        Err(e) => return ToolResult {
            success: false,
            output: format!("Failed to resolve path '{}': {}", path_str, e),
        },
    };
    
    let content = match std::fs::read_to_string(&resolved_path.path) {
        Ok(c) => c,
        Err(e) => return ToolResult {
            success: false,
            output: format!("Failed to read '{}': {}", resolved_path.path.display(), e),
        },
    };
    
    let count = content.matches(old_string).count();
    if count == 0 {
        return ToolResult {
            success: false,
            output: format!("String not found in '{}'. Make sure old_string matches exactly.", 
                           resolved_path.path.display()),
        };
    }
    
    let new_content = content.replacen(old_string, new_string, 1);
    match std::fs::write(&resolved_path.path, &new_content) {
        Ok(()) => ToolResult {
            success: true,
            output: format!("Replaced 1 occurrence in {} ({} total matches)", 
                           resolved_path.path.display(), count),
        },
        Err(e) => ToolResult {
            success: false,
            output: format!("Failed to write '{}': {}", resolved_path.path.display(), e),
        },
    }
}

/// Enhanced shell execution tool
fn tool_shell_exec_enhanced(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let command = match params["command"].as_str() {
        Some(c) => c,
        None => return ToolResult { success: false, output: "Missing 'command' parameter".into() },
    };

    debug!("Shell exec: {}", command);

    #[cfg(windows)]
    let child_result = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", command])
        .current_dir(project_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("HOME", std::env::var("USERPROFILE").unwrap_or_default())
        .spawn();

    #[cfg(not(windows))]
    let child_result = Command::new("sh")
        .args(["-c", command])
        .current_dir(project_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    match child_result {
        Ok(mut child) => {
            // Wait with 120-second timeout
            let timeout = std::time::Duration::from_secs(120);
            let start = std::time::Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        // Process finished
                        let stdout = child.stdout.take()
                            .map(|mut s| { let mut buf = Vec::new(); std::io::Read::read_to_end(&mut s, &mut buf).ok(); buf })
                            .unwrap_or_default();
                        let stderr = child.stderr.take()
                            .map(|mut s| { let mut buf = Vec::new(); std::io::Read::read_to_end(&mut s, &mut buf).ok(); buf })
                            .unwrap_or_default();

                        let stdout_str = String::from_utf8_lossy(&stdout);
                        let stderr_str = String::from_utf8_lossy(&stderr);
                        let exit_code = status.code().unwrap_or(-1);

                        let mut out = String::new();
                        if !stdout_str.is_empty() {
                            out.push_str(&stdout_str);
                        }
                        if !stderr_str.is_empty() {
                            if !out.is_empty() { out.push('\n'); }
                            out.push_str("[stderr]\n");
                            out.push_str(&stderr_str);
                        }
                        out.push_str(&format!("\n[exit code: {}]", exit_code));

                        return ToolResult {
                            success: exit_code == 0,
                            output: out,
                        };
                    }
                    Ok(None) => {
                        // Still running
                        if start.elapsed() > timeout {
                            let _ = child.kill();
                            return ToolResult {
                                success: false,
                                output: "Command timed out after 120 seconds".into(),
                            };
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(e) => {
                        return ToolResult {
                            success: false,
                            output: format!("Failed to wait for command: {}", e),
                        };
                    }
                }
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: format!("Failed to spawn command '{}': {}", command, e),
        },
    }
}

/// Try alternative paths when primary path fails
fn try_alternative_paths(alternatives: &[PathBuf], content: &str, operation: &str) -> ToolResult {
    for alternative in alternatives {
        match std::fs::write(alternative, content) {
            Ok(()) => {
                return ToolResult {
                    success: true,
                    output: format!("{} succeeded at alternative location: {}", 
                                   operation, 
                                   alternative.display()),
                };
            },
            Err(_) => continue, // Try next alternative
        }
    }
    
    ToolResult {
        success: false,
        output: format!("{} failed at all attempted locations", operation),
    }
}

/// Resolve a path relative to project root (legacy function for compatibility)
fn resolve_path(path_str: &str, project_root: &Path) -> PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        project_root.join(p)
    }
}

/// Check if an operation is potentially destructive
fn is_destructive_operation(tool_name: &str, params: &serde_json::Value, project_root: &Path) -> bool {
    match tool_name {
        "write_file" => {
            // Writing to an existing file is potentially destructive
            if let Some(path_str) = params["path"].as_str() {
                // Use enhanced path resolution to check existence
                if let Ok(resolved) = resolve_path_with_fallbacks(path_str, project_root) {
                    resolved.path.exists()
                } else {
                    false
                }
            } else {
                false
            }
        },
        "replace_in_file" => true, // Always destructive
        "shell_exec" => true,      // Potentially destructive
        "delete_file" => true,     // Always destructive
        _ => false
    }
}

fn tool_read_file(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let path_str = match params["path"].as_str() {
        Some(p) => p,
        None => return ToolResult { success: false, output: "Missing 'path' parameter".into() },
    };
    let path = resolve_path(path_str, project_root);

    // Guard: skip files larger than 10MB
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 10 * 1024 * 1024 {
            return ToolResult {
                success: false,
                output: format!("File too large ({:.1} MB). Max 10 MB.", meta.len() as f64 / 1_048_576.0),
            };
        }
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let numbered: String = content
                .lines()
                .enumerate()
                .map(|(i, line)| format!("{:>4} | {}", i + 1, line))
                .collect::<Vec<_>>()
                .join("\n");
            ToolResult {
                success: true,
                output: format!("File: {}\n{}", path.display(), numbered),
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: format!("Failed to read '{}': {}", path.display(), e),
        },
    }
}

fn tool_write_file(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let path_str = match params["path"].as_str() {
        Some(p) => p,
        None => return ToolResult { success: false, output: "Missing 'path' parameter".into() },
    };
    let content = match params["content"].as_str() {
        Some(c) => c,
        None => return ToolResult { success: false, output: "Missing 'content' parameter".into() },
    };
    let path = resolve_path(path_str, project_root);

    // Create parent dirs
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return ToolResult {
                success: false,
                output: format!("Failed to create directories: {}", e),
            };
        }
    }

    match std::fs::write(&path, content) {
        Ok(()) => ToolResult {
            success: true,
            output: format!("Written {} bytes to {}", content.len(), path.display()),
        },
        Err(e) => ToolResult {
            success: false,
            output: format!("Failed to write '{}': {}", path.display(), e),
        },
    }
}

fn tool_replace_in_file(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let path_str = match params["path"].as_str() {
        Some(p) => p,
        None => return ToolResult { success: false, output: "Missing 'path' parameter".into() },
    };
    let old_string = match params["old_string"].as_str() {
        Some(s) => s,
        None => return ToolResult { success: false, output: "Missing 'old_string' parameter".into() },
    };
    let new_string = match params["new_string"].as_str() {
        Some(s) => s,
        None => return ToolResult { success: false, output: "Missing 'new_string' parameter".into() },
    };
    let path = resolve_path(path_str, project_root);

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return ToolResult {
            success: false,
            output: format!("Failed to read '{}': {}", path.display(), e),
        },
    };

    let count = content.matches(old_string).count();
    if count == 0 {
        return ToolResult {
            success: false,
            output: format!("String not found in '{}'. Make sure old_string matches exactly.", path.display()),
        };
    }

    let new_content = content.replacen(old_string, new_string, 1);
    match std::fs::write(&path, &new_content) {
        Ok(()) => ToolResult {
            success: true,
            output: format!("Replaced 1 occurrence in {} ({} total matches)", path.display(), count),
        },
        Err(e) => ToolResult {
            success: false,
            output: format!("Failed to write '{}': {}", path.display(), e),
        },
    }
}

fn tool_list_dir(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let path_str = params["path"].as_str().unwrap_or(".");
    let recursive = params["recursive"].as_bool().unwrap_or(false);
    let path = resolve_path(path_str, project_root);

    if !path.is_dir() {
        return ToolResult {
            success: false,
            output: format!("'{}' is not a directory", path.display()),
        };
    }

    let mut entries = Vec::new();
    let max_entries = 200;

    if recursive {
        collect_entries_recursive(&path, &path, &mut entries, max_entries);
    } else {
        match std::fs::read_dir(&path) {
            Ok(dir) => {
                for entry in dir.flatten() {
                    if entries.len() >= max_entries { break; }
                    let meta = entry.metadata();
                    let prefix = if meta.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
                        "[DIR] "
                    } else {
                        "[FILE]"
                    };
                    let name = entry.file_name().to_string_lossy().to_string();
                    entries.push(format!("{} {}", prefix, name));
                }
            }
            Err(e) => return ToolResult {
                success: false,
                output: format!("Failed to read directory '{}': {}", path.display(), e),
            },
        }
    }

    let truncated = if entries.len() >= max_entries {
        format!("\n... (truncated at {} entries)", max_entries)
    } else {
        String::new()
    };

    ToolResult {
        success: true,
        output: format!("Directory: {}\n{}{}", path.display(), entries.join("\n"), truncated),
    }
}

fn collect_entries_recursive(base: &Path, dir: &Path, entries: &mut Vec<String>, max: usize) {
    if entries.len() >= max { return; }
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        if entries.len() >= max { break; }
        let path = entry.path();
        let relative = path.strip_prefix(base).unwrap_or(&path);
        let name = relative.to_string_lossy().to_string();

        // Skip hidden dirs and common noise
        let name_normalized = name.replace('\\', "/");
        if name.starts_with('.') || name_normalized.contains("node_modules") || name_normalized.contains("target/debug") || name_normalized.contains("target/release") || name_normalized.contains(".git/") {
            continue;
        }

        let is_dir = path.is_dir();
        let prefix = if is_dir { "[DIR] " } else { "[FILE]" };
        entries.push(format!("{} {}", prefix, name));

        if is_dir {
            collect_entries_recursive(base, &path, entries, max);
        }
    }
}

fn tool_search_files(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let pattern = match params["pattern"].as_str() {
        Some(p) => p,
        None => return ToolResult { success: false, output: "Missing 'pattern' parameter".into() },
    };
    let path_str = params["path"].as_str().unwrap_or(".");
    let base = resolve_path(path_str, project_root);

    // Normalize base path to forward slashes (glob crate requires it on all platforms)
    let base_str = base.display().to_string().replace('\\', "/");
    let pattern_normalized = pattern.replace('\\', "/");
    let glob_pattern = if pattern_normalized.contains('/') {
        format!("{}/{}", base_str, pattern_normalized)
    } else {
        format!("{}/**/{}", base_str, pattern_normalized)
    };

    match glob::glob(&glob_pattern) {
        Ok(paths) => {
            let mut results: Vec<String> = Vec::new();
            for entry in paths.flatten() {
                if results.len() >= 100 { break; }
                let relative = entry.strip_prefix(&base)
                    .unwrap_or(&entry)
                    .to_string_lossy()
                    .to_string();
                results.push(relative);
            }
            if results.is_empty() {
                ToolResult { success: true, output: format!("No files matching '{}'", pattern) }
            } else {
                ToolResult {
                    success: true,
                    output: format!("Found {} files:\n{}", results.len(), results.join("\n")),
                }
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: format!("Invalid glob pattern '{}': {}", pattern, e),
        },
    }
}

fn tool_search_content(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let pattern = match params["pattern"].as_str() {
        Some(p) => p,
        None => return ToolResult { success: false, output: "Missing 'pattern' parameter".into() },
    };
    let path_str = params["path"].as_str().unwrap_or(".");
    let file_pattern = params["file_pattern"].as_str();
    let base = resolve_path(path_str, project_root);

    let regex = match regex::Regex::new(pattern) {
        Ok(r) => r,
        Err(e) => return ToolResult {
            success: false,
            output: format!("Invalid regex '{}': {}", pattern, e),
        },
    };

    let mut results = Vec::new();
    let max_results = 50;

    search_content_recursive(&base, &base, &regex, file_pattern, &mut results, max_results);

    if results.is_empty() {
        ToolResult { success: true, output: format!("No matches for '{}'", pattern) }
    } else {
        let truncated = if results.len() >= max_results {
            format!("\n... (showing first {} matches)", max_results)
        } else {
            String::new()
        };
        ToolResult {
            success: true,
            output: format!("{}{}", results.join("\n"), truncated),
        }
    }
}

fn search_content_recursive(
    base: &Path,
    dir: &Path,
    regex: &regex::Regex,
    file_pattern: Option<&str>,
    results: &mut Vec<String>,
    max: usize,
) {
    if results.len() >= max { return; }
    let Ok(read) = std::fs::read_dir(dir) else { return };

    for entry in read.flatten() {
        if results.len() >= max { break; }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden and noise
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }

        if path.is_dir() {
            search_content_recursive(base, &path, regex, file_pattern, results, max);
        } else if path.is_file() {
            // Check file pattern filter
            if let Some(fp) = file_pattern {
                let fname = path.file_name().unwrap_or_default().to_string_lossy();
                if let Ok(g) = glob::Pattern::new(fp) {
                    if !g.matches(&fname) { continue; }
                }
            }

            // Read file once — skip binary (contains null bytes in first 512)
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let check_len = bytes.len().min(512);
            if bytes[..check_len].contains(&0) { continue; }

            if let Ok(content) = std::str::from_utf8(&bytes) {
                let relative = path.strip_prefix(base).unwrap_or(&path);
                for (i, line) in content.lines().enumerate() {
                    if results.len() >= max { break; }
                    if regex.is_match(line) {
                        results.push(format!("{}:{}: {}", relative.display(), i + 1, line.trim()));
                    }
                }
            }
        }
    }
}

// ─── Git Tools ────────────────────────────────────────────────────────────────

fn tool_git_status(project_root: &Path) -> ToolResult {
    let output = Command::new("git")
        .args(["status", "--short", "--branch"])
        .current_dir(project_root)
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if out.status.success() {
                ToolResult {
                    success: true,
                    output: if stdout.is_empty() { "Clean working tree".into() } else { stdout.to_string() },
                }
            } else {
                ToolResult { success: false, output: stderr.to_string() }
            }
        }
        Err(e) => ToolResult { success: false, output: format!("Not a git repository or git not found: {}", e) },
    }
}

fn tool_git_diff(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let mut args = vec!["diff"];
    let path_str = params["path"].as_str();
    if let Some(p) = path_str {
        args.push("--");
        args.push(p);
    }

    let output = Command::new("git")
        .args(&args)
        .current_dir(project_root)
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.is_empty() {
                ToolResult { success: true, output: "No unstaged changes".into() }
            } else {
                let truncated = if stdout.len() > 16000 {
                    format!("{}...\n(truncated)", &stdout[..16000])
                } else {
                    stdout.to_string()
                };
                ToolResult { success: true, output: truncated }
            }
        }
        Err(e) => ToolResult { success: false, output: format!("Git diff failed: {}", e) },
    }
}

fn tool_git_log(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let count = params["count"].as_u64().unwrap_or(10);
    let count_str = format!("-{}", count);

    let output = Command::new("git")
        .args(["log", &count_str, "--oneline", "--decorate"])
        .current_dir(project_root)
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.is_empty() {
                ToolResult { success: true, output: "No commits yet".into() }
            } else {
                ToolResult { success: true, output: stdout.to_string() }
            }
        }
        Err(e) => ToolResult { success: false, output: format!("Git log failed: {}", e) },
    }
}

// ─── Project Info Tool ────────────────────────────────────────────────────────

fn tool_get_project_info(project_root: &Path) -> ToolResult {
    let mut info = String::new();
    info.push_str(&format!("Project root: {}\n", project_root.display()));

    // Detect project type
    let markers = [
        ("Cargo.toml", "Rust (Cargo)"),
        ("package.json", "Node.js/JavaScript"),
        ("pyproject.toml", "Python (pyproject)"),
        ("setup.py", "Python (setup.py)"),
        ("requirements.txt", "Python (requirements)"),
        ("go.mod", "Go"),
        ("pom.xml", "Java (Maven)"),
        ("build.gradle", "Java (Gradle)"),
        ("CMakeLists.txt", "C/C++ (CMake)"),
        ("Makefile", "Makefile"),
        ("composer.json", "PHP (Composer)"),
    ];

    let mut detected = Vec::new();
    for (file, lang) in &markers {
        if project_root.join(file).exists() {
            detected.push(*lang);
        }
    }
    let type_str = if detected.is_empty() { "Unknown".to_string() } else { detected.join(", ") };
    info.push_str(&format!("Type: {}\n", type_str));

    // Git info
    if project_root.join(".git").exists() {
        if let Ok(out) = Command::new("git").args(["branch", "--show-current"]).current_dir(project_root).output() {
            let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
            info.push_str(&format!("Git branch: {}\n", if branch.is_empty() { "detached HEAD".into() } else { branch }));
        }
        if let Ok(out) = Command::new("git").args(["status", "--short"]).current_dir(project_root).output() {
            let status = String::from_utf8_lossy(&out.stdout);
            let changed = status.lines().count();
            info.push_str(&format!("Git changes: {} file(s)\n", changed));
        }
    }

    // List top-level files/dirs
    if let Ok(entries) = std::fs::read_dir(project_root) {
        let mut items: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') && name != ".gitignore" { return None; }
                if name == "node_modules" || name == "target" || name == "__pycache__" || name == ".git" { return None; }
                let prefix = if e.path().is_dir() { "📁" } else { "📄" };
                Some(format!("  {} {}", prefix, name))
            })
            .collect();
        items.sort();
        info.push_str(&format!("\nStructure:\n{}\n", items.join("\n")));
    }

    ToolResult { success: true, output: info }
}

// ─── Batch Read Files Tool ────────────────────────────────────────────────────

fn tool_batch_read_files(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let paths = match params["paths"].as_array() {
        Some(arr) => arr,
        None => return ToolResult { success: false, output: "Missing 'paths' array parameter".into() },
    };

    let mut results = Vec::new();
    let mut had_errors = false;

    for path_val in paths {
        let path_str = match path_val.as_str() {
            Some(s) => s,
            None => continue,
        };
        let path = resolve_path(path_str, project_root);
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let numbered: String = content
                    .lines()
                    .enumerate()
                    .map(|(i, line)| format!("{:>4} | {}", i + 1, line))
                    .collect::<Vec<_>>()
                    .join("\n");
                results.push(format!("=== {} ===\n{}", path.display(), numbered));
            }
            Err(e) => {
                results.push(format!("=== {} ===\nError: {}", path.display(), e));
                had_errors = true;
            }
        }
    }

    ToolResult {
        success: !had_errors || !results.is_empty(),
        output: results.join("\n\n"),
    }
}

fn tool_shell_exec(params: &serde_json::Value, project_root: &Path) -> ToolResult {
    let command = match params["command"].as_str() {
        Some(c) => c,
        None => return ToolResult { success: false, output: "Missing 'command' parameter".into() },
    };

    debug!("Shell exec: {}", command);

    #[cfg(windows)]
    let child_result = Command::new("cmd")
        .args(["/C", command])
        .current_dir(project_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    #[cfg(not(windows))]
    let child_result = Command::new("sh")
        .args(["-c", command])
        .current_dir(project_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    match child_result {
        Ok(mut child) => {
            // Wait with 120-second timeout
            let timeout = std::time::Duration::from_secs(120);
            let start = std::time::Instant::now();

            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        // Process finished
                        let stdout = child.stdout.take()
                            .map(|mut s| { let mut buf = Vec::new(); std::io::Read::read_to_end(&mut s, &mut buf).ok(); buf })
                            .unwrap_or_default();
                        let stderr = child.stderr.take()
                            .map(|mut s| { let mut buf = Vec::new(); std::io::Read::read_to_end(&mut s, &mut buf).ok(); buf })
                            .unwrap_or_default();

                        let stdout_str = String::from_utf8_lossy(&stdout);
                        let stderr_str = String::from_utf8_lossy(&stderr);
                        let exit_code = status.code().unwrap_or(-1);

                        let mut out = String::new();
                        if !stdout_str.is_empty() {
                            out.push_str(&stdout_str);
                        }
                        if !stderr_str.is_empty() {
                            if !out.is_empty() { out.push('\n'); }
                            out.push_str("[stderr]\n");
                            out.push_str(&stderr_str);
                        }
                        out.push_str(&format!("\n[exit code: {}]", exit_code));

                        // Truncate very long output
                        if out.len() > 16000 {
                            out.truncate(16000);
                            out.push_str("\n... (output truncated)");
                        }

                        return ToolResult {
                            success: exit_code == 0,
                            output: out,
                        };
                    }
                    Ok(None) => {
                        // Still running
                        if start.elapsed() > timeout {
                            let _ = child.kill();
                            return ToolResult {
                                success: false,
                                output: format!("Command timed out after {}s: {}", timeout.as_secs(), command),
                            };
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(e) => {
                        return ToolResult {
                            success: false,
                            output: format!("Failed to wait for process: {}", e),
                        };
                    }
                }
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: format!("Failed to execute command: {}", e),
        },
    }
}
