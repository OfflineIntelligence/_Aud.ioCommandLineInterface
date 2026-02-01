//! Advanced file operation tools for enhanced file management.
//!
//! This module provides tools for:
//! - Creating temporary files safely
//! - Backing up files before modification
//! - Secure file operations with rollback capabilities
//! - Advanced file manipulation

use serde::Serialize;
use std::path::{Path, PathBuf};
use tracing::debug;
use crate::utils::path_resolver::resolve_path_with_fallbacks;

/// Tool definition for advanced file tools
#[derive(Debug, Clone, Serialize)]
pub struct AdvancedFileToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: &'static str,
}

/// Get all advanced file tool definitions
pub fn advanced_file_tool_definitions() -> Vec<AdvancedFileToolDef> {
    vec![
        AdvancedFileToolDef {
            name: "create_temp_file",
            description: "Create a temporary file with optional content and extension",
            parameters: r#"{"type":"object","properties":{"content":{"type":"string","description":"Content to write to the temp file"},"extension":{"type":"string","description":"File extension for the temp file"},"prefix":{"type":"string","description":"Prefix for the temp file name"}},"required":[]}"#,
        },
        AdvancedFileToolDef {
            name: "backup_file",
            description: "Create a backup of a file before modification",
            parameters: r#"{"type":"object","properties":{"path":{"type":"string","description":"Path to the file to backup"},"backup_suffix":{"type":"string","description":"Suffix to append to backup filename"}},"required":["path"]}"#,
        },
        AdvancedFileToolDef {
            name: "secure_write_file",
            description: "Write file with backup and rollback capabilities",
            parameters: r#"{"type":"object","properties":{"path":{"type":"string","description":"Destination path"},"content":{"type":"string","description":"Content to write"},"create_backup":{"type":"boolean","description":"Create backup before writing"},"validate_write":{"type":"boolean","description":"Verify write was successful"}},"required":["path","content"]}"#,
        },
        AdvancedFileToolDef {
            name: "atomic_file_swap",
            description: "Atomically swap two files with rollback on failure",
            parameters: r#"{"type":"object","properties":{"source":{"type":"string","description":"Source file path"},"destination":{"type":"string","description":"Destination file path"},"backup_original":{"type":"boolean","description":"Backup the original destination file"}},"required":["source","destination"]}"#,
        },
        AdvancedFileToolDef {
            name: "cleanup_temp_files",
            description: "Clean up temporary files created by the system",
            parameters: r#"{"type":"object","properties":{"older_than_hours":{"type":"number","description":"Only clean files older than specified hours"},"dry_run":{"type":"boolean","description":"Show what would be cleaned without actually cleaning"}},"required":[]}"#,
        },
    ]
}

/// Result from advanced file tool execution
#[derive(Debug, Serialize)]
pub struct AdvancedFileToolResult {
    pub success: bool,
    pub output: String,
}

/// Execute an advanced file tool by name with JSON params
pub fn execute_advanced_file_tool(
    name: &str,
    params: &serde_json::Value,
    project_root: &Path,
) -> AdvancedFileToolResult {
    debug!("Executing advanced file tool '{}' with params: {}", name, params);
    
    match name {
        "create_temp_file" => tool_create_temp_file(params),
        "backup_file" => tool_backup_file(params, project_root),
        "secure_write_file" => tool_secure_write_file(params, project_root),
        "atomic_file_swap" => tool_atomic_file_swap(params, project_root),
        "cleanup_temp_files" => tool_cleanup_temp_files(params),
        _ => AdvancedFileToolResult {
            success: false,
            output: format!("Unknown advanced file tool: {}", name),
        },
    }
}

/// Create a temporary file with optional content
fn tool_create_temp_file(params: &serde_json::Value) -> AdvancedFileToolResult {
    let content = params["content"].as_str().unwrap_or("");
    let extension = params["extension"].as_str().unwrap_or("tmp");
    let prefix = params["prefix"].as_str().unwrap_or("aud_io_");
    
    match create_temporary_file(content, extension, prefix) {
        Ok((temp_path, file_handle)) => {
            // Keep the file handle alive until we're done with it
            std::mem::forget(file_handle);
            
            AdvancedFileToolResult {
                success: true,
                output: format!("Created temporary file: {}\nSize: {} bytes", 
                               temp_path.display(), content.len()),
            }
        }
        Err(e) => AdvancedFileToolResult {
            success: false,
            output: format!("Failed to create temporary file: {}", e),
        },
    }
}

/// Create a backup of a file
fn tool_backup_file(params: &serde_json::Value, project_root: &Path) -> AdvancedFileToolResult {
    let path_str = match params["path"].as_str() {
        Some(p) => p,
        None => return AdvancedFileToolResult {
            success: false,
            output: "Missing 'path' parameter".into(),
        },
    };
    
    let backup_suffix = params["backup_suffix"].as_str().unwrap_or(".backup");
    
    match resolve_path_with_fallbacks(path_str, project_root) {
        Ok(resolved) => {
            if !resolved.path.exists() {
                return AdvancedFileToolResult {
                    success: false,
                    output: format!("File does not exist: {}", resolved.path.display()),
                };
            }
            
            let backup_path = resolved.path.with_extension(format!("{}{}", 
                resolved.path.extension().unwrap_or_default().to_string_lossy(),
                backup_suffix));
            
            match std::fs::copy(&resolved.path, &backup_path) {
                Ok(_) => AdvancedFileToolResult {
                    success: true,
                    output: format!("Created backup: {}\nOriginal: {}", 
                                   backup_path.display(), resolved.path.display()),
                },
                Err(e) => AdvancedFileToolResult {
                    success: false,
                    output: format!("Failed to create backup: {}", e),
                },
            }
        }
        Err(e) => AdvancedFileToolResult {
            success: false,
            output: format!("Failed to resolve path '{}': {}", path_str, e),
        },
    }
}

/// Write file with backup and validation
fn tool_secure_write_file(params: &serde_json::Value, project_root: &Path) -> AdvancedFileToolResult {
    let path_str = match params["path"].as_str() {
        Some(p) => p,
        None => return AdvancedFileToolResult {
            success: false,
            output: "Missing 'path' parameter".into(),
        },
    };
    
    let content = match params["content"].as_str() {
        Some(c) => c,
        None => return AdvancedFileToolResult {
            success: false,
            output: "Missing 'content' parameter".into(),
        },
    };
    
    let create_backup = params["create_backup"].as_bool().unwrap_or(true);
    let validate_write = params["validate_write"].as_bool().unwrap_or(true);
    
    match resolve_path_with_fallbacks(path_str, project_root) {
        Ok(resolved) => {
            let target_path = &resolved.path;
            let mut backup_path = None;
            
            // Create backup if requested and file exists
            if create_backup && target_path.exists() {
                let backup = target_path.with_extension(format!("{}.backup", 
                    target_path.extension().unwrap_or_default().to_string_lossy()));
                match std::fs::copy(target_path, &backup) {
                    Ok(_) => backup_path = Some(backup),
                    Err(e) => {
                        return AdvancedFileToolResult {
                            success: false,
                            output: format!("Failed to create backup: {}", e),
                        };
                    }
                }
            }
            
            // Ensure parent directory exists
            if let Some(parent) = target_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    // Cleanup backup if we created one
                    if let Some(backup) = &backup_path {
                        let _ = std::fs::remove_file(backup);
                    }
                    return AdvancedFileToolResult {
                        success: false,
                        output: format!("Failed to create parent directories: {}", e),
                    };
                }
            }
            
            // Write the file
            match std::fs::write(target_path, content) {
                Ok(_) => {
                    // Validate write if requested
                    if validate_write {
                        match std::fs::read_to_string(target_path) {
                            Ok(read_content) if read_content == content => {
                                // Success
                                let mut output = format!("Successfully wrote {} bytes to {}", 
                                                        content.len(), target_path.display());
                                if let Some(backup) = backup_path {
                                    output.push_str(&format!("\nBackup created: {}", backup.display()));
                                }
                                AdvancedFileToolResult { success: true, output }
                            }
                            Ok(_) => {
                                // Content mismatch - rollback
                                if let Some(backup) = backup_path {
                                    let _ = std::fs::copy(&backup, target_path);
                                    let _ = std::fs::remove_file(&backup);
                                    AdvancedFileToolResult {
                                        success: false,
                                        output: "Write validation failed - rollback performed".into(),
                                    }
                                } else {
                                    AdvancedFileToolResult {
                                        success: false,
                                        output: "Write validation failed".into(),
                                    }
                                }
                            }
                            Err(e) => {
                                // Validation read failed - try to preserve backup
                                AdvancedFileToolResult {
                                    success: false,
                                    output: format!("Write validation failed: {}", e),
                                }
                            }
                        }
                    } else {
                        // No validation requested
                        let mut output = format!("Successfully wrote {} bytes to {}", 
                                                content.len(), target_path.display());
                        if let Some(backup) = backup_path {
                            output.push_str(&format!("\nBackup created: {}", backup.display()));
                        }
                        AdvancedFileToolResult { success: true, output }
                    }
                }
                Err(e) => {
                    // Write failed - cleanup backup
                    if let Some(backup) = backup_path {
                        let _ = std::fs::remove_file(&backup);
                    }
                    AdvancedFileToolResult {
                        success: false,
                        output: format!("Failed to write file: {}", e),
                    }
                }
            }
        }
        Err(e) => AdvancedFileToolResult {
            success: false,
            output: format!("Failed to resolve path '{}': {}", path_str, e),
        },
    }
}

/// Atomically swap two files
fn tool_atomic_file_swap(params: &serde_json::Value, project_root: &Path) -> AdvancedFileToolResult {
    let source_str = match params["source"].as_str() {
        Some(s) => s,
        None => return AdvancedFileToolResult {
            success: false,
            output: "Missing 'source' parameter".into(),
        },
    };
    
    let dest_str = match params["destination"].as_str() {
        Some(d) => d,
        None => return AdvancedFileToolResult {
            success: false,
            output: "Missing 'destination' parameter".into(),
        },
    };
    
    let backup_original = params["backup_original"].as_bool().unwrap_or(true);
    
    match (resolve_path_with_fallbacks(source_str, project_root), 
           resolve_path_with_fallbacks(dest_str, project_root)) {
        (Ok(source_resolved), Ok(dest_resolved)) => {
            let source_path = &source_resolved.path;
            let dest_path = &dest_resolved.path;
            
            // Check if source exists
            if !source_path.exists() {
                return AdvancedFileToolResult {
                    success: false,
                    output: format!("Source file does not exist: {}", source_path.display()),
                };
            }
            
            let mut backup_path = None;
            
            // Backup destination if it exists and backup is requested
            if backup_original && dest_path.exists() {
                let backup = dest_path.with_extension(format!("{}.pre_swap", 
                    dest_path.extension().unwrap_or_default().to_string_lossy()));
                match std::fs::copy(dest_path, &backup) {
                    Ok(_) => backup_path = Some(backup),
                    Err(e) => {
                        return AdvancedFileToolResult {
                            success: false,
                            output: format!("Failed to backup destination: {}", e),
                        };
                    }
                }
            }
            
            // Perform atomic swap using temporary file
            let temp_swap = dest_path.with_extension("swap_tmp");
            
            // Move destination to temp (if exists)
            let dest_existed = dest_path.exists();
            if dest_existed {
                if let Err(e) = std::fs::rename(dest_path, &temp_swap) {
                    // Cleanup backup if we created one
                    if let Some(backup) = backup_path {
                        let _ = std::fs::remove_file(&backup);
                    }
                    return AdvancedFileToolResult {
                        success: false,
                        output: format!("Failed to move destination to temp: {}", e),
                    };
                }
            }
            
            // Move source to destination
            match std::fs::rename(source_path, dest_path) {
                Ok(_) => {
                    // If destination existed, move temp to source location
                    if dest_existed {
                        if let Err(e) = std::fs::rename(&temp_swap, source_path) {
                            // Rollback - restore destination from temp
                            let _ = std::fs::rename(dest_path, source_path);
                            if dest_existed {
                                let _ = std::fs::rename(&temp_swap, dest_path);
                            }
                            // Cleanup backup
                            if let Some(backup) = backup_path {
                                let _ = std::fs::remove_file(&backup);
                            }
                            return AdvancedFileToolResult {
                                success: false,
                                output: format!("Failed to complete swap: {}", e),
                            };
                        }
                    } else {
                        // Remove temp file since destination didn't exist originally
                        let _ = std::fs::remove_file(&temp_swap);
                    }
                    
                    let mut output = format!("Successfully swapped files:\n  {} ↔ {}", 
                                            source_path.display(), dest_path.display());
                    if let Some(backup) = backup_path {
                        output.push_str(&format!("\nOriginal destination backed up to: {}", backup.display()));
                    }
                    AdvancedFileToolResult { success: true, output }
                }
                Err(e) => {
                    // Rollback - restore destination from temp if we moved it
                    if dest_existed {
                        let _ = std::fs::rename(&temp_swap, dest_path);
                    }
                    // Cleanup backup
                    if let Some(backup) = backup_path {
                        let _ = std::fs::remove_file(&backup);
                    }
                    AdvancedFileToolResult {
                        success: false,
                        output: format!("Failed to move source to destination: {}", e),
                    }
                }
            }
        }
        (Err(e1), Err(e2)) => AdvancedFileToolResult {
            success: false,
            output: format!("Failed to resolve paths - Source: {}, Destination: {}", e1, e2),
        },
        (Err(e), _) => AdvancedFileToolResult {
            success: false,
            output: format!("Failed to resolve source path '{}': {}", source_str, e),
        },
        (_, Err(e)) => AdvancedFileToolResult {
            success: false,
            output: format!("Failed to resolve destination path '{}': {}", dest_str, e),
        },
    }
}

/// Clean up temporary files
fn tool_cleanup_temp_files(params: &serde_json::Value) -> AdvancedFileToolResult {
    let older_than_hours = params["older_than_hours"].as_f64().unwrap_or(24.0);
    let dry_run = params["dry_run"].as_bool().unwrap_or(false);
    
    let temp_dir = std::env::temp_dir();
    
    let cutoff_time = std::time::SystemTime::now() 
        - std::time::Duration::from_secs((older_than_hours * 3600.0) as u64);
    
    let mut cleaned_files = Vec::new();
    let mut failed_files = Vec::new();
    let mut skipped_files = Vec::new();
    
    if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            
            // Only process files (not directories)
            if path.is_file() {
                match entry.metadata() {
                    Ok(metadata) => {
                        if let Ok(modified) = metadata.modified() {
                            if modified < cutoff_time {
                                if dry_run {
                                    skipped_files.push(path);
                                } else {
                                    match std::fs::remove_file(&path) {
                                        Ok(_) => cleaned_files.push(path),
                                        Err(_) => failed_files.push(path),
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => failed_files.push(path),
                }
            }
        }
    }
    
    let mut output = String::new();
    if dry_run {
        output.push_str(&format!("🔍 Dry run - would clean {} temp files\n", cleaned_files.len()));
    } else {
        output.push_str(&format!("🧹 Cleaned {} temp files\n", cleaned_files.len()));
    }
    output.push_str(&format!("Failed to process {} files\n", failed_files.len()));
    output.push_str(&format!("Skipped {} recent files\n", skipped_files.len()));
    
    if !cleaned_files.is_empty() || !skipped_files.is_empty() || !failed_files.is_empty() {
        output.push_str(&format!("\nDetails:\n"));
        if !cleaned_files.is_empty() {
            output.push_str(&format!("Cleaned files:\n"));
            for file in &cleaned_files {
                output.push_str(&format!("  - {}\n", file.display()));
            }
        }
        if !skipped_files.is_empty() {
            output.push_str(&format!("Skipped recent files:\n"));
            for file in &skipped_files {
                output.push_str(&format!("  - {}\n", file.display()));
            }
        }
        if !failed_files.is_empty() {
            output.push_str(&format!("Failed files:\n"));
            for file in &failed_files {
                output.push_str(&format!("  - {}\n", file.display()));
            }
        }
    }
    
    AdvancedFileToolResult {
        success: true,
        output,
    }
}

/// Helper function to create a temporary file
fn create_temporary_file(content: &str, extension: &str, prefix: &str) -> Result<(PathBuf, std::fs::File), std::io::Error> {
    use std::fs::File;
    use std::io::Write;
    
    let temp_dir = std::env::temp_dir();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let filename = format!("{}{}_{}.{}", prefix, timestamp, rand::random::<u32>(), extension);
    let temp_path = temp_dir.join(filename);
    
    let mut file = File::create(&temp_path)?;
    file.write_all(content.as_bytes())?;
    file.flush()?;
    
    Ok((temp_path, file))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_advanced_file_tool_definitions() {
        let tools = advanced_file_tool_definitions();
        assert!(!tools.is_empty());
        assert!(tools.iter().any(|t| t.name == "create_temp_file"));
    }
    
    #[test]
    fn test_create_temp_file() {
        let params = serde_json::json!({
            "content": "test content",
            "extension": "txt",
            "prefix": "test_"
        });
        
        let result = tool_create_temp_file(&params);
        assert!(result.success);
        assert!(result.output.contains("Created temporary file"));
    }
}