//! System information and management tools for the agentic CLI.
//!
//! This module provides tools for:
//! - Getting detailed system information
//! - Finding writable paths
//! - Checking permissions
//! - System diagnostics

use serde::Serialize;
use std::path::Path;
use tracing::debug;
use crate::capabilities::detector::{PlatformCapabilityDetector, ToolAvailabilityChecker};
use crate::utils::path_resolver::resolve_path_with_fallbacks;

/// Tool definition for system tools
#[derive(Debug, Clone, Serialize)]
pub struct SystemToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: &'static str,
}

/// Get all system tool definitions
pub fn system_tool_definitions() -> Vec<SystemToolDef> {
    vec![
        SystemToolDef {
            name: "get_system_info",
            description: "Get detailed system information including hardware, OS, and capabilities",
            parameters: r#"{"type":"object","properties":{},"required":[]}"#,
        },
        SystemToolDef {
            name: "find_writable_paths",
            description: "Find directories where the current user can write files",
            parameters: r#"{"type":"object","properties":{"search_paths":{"type":"array","items":{"type":"string"},"description":"Paths to search for writable locations"}},"required":[]}"#,
        },
        SystemToolDef {
            name: "check_permissions",
            description: "Check permissions for a specific path or file",
            parameters: r#"{"type":"object","properties":{"path":{"type":"string","description":"Path to check permissions for"}},"required":["path"]}"#,
        },
        SystemToolDef {
            name: "get_available_tools",
            description: "List all available system tools and commands",
            parameters: r#"{"type":"object","properties":{"filter":{"type":"string","description":"Filter tools by name pattern"}},"required":[]}"#,
        },
        SystemToolDef {
            name: "diagnose_environment",
            description: "Run comprehensive environment diagnostics",
            parameters: r#"{"type":"object","properties":{},"required":[]}"#,
        },
    ]
}

/// Result from system tool execution
#[derive(Debug, Serialize)]
pub struct SystemToolResult {
    pub success: bool,
    pub output: String,
}

/// Execute a system tool by name with JSON params
pub fn execute_system_tool(
    name: &str,
    params: &serde_json::Value,
    project_root: &Path,
) -> SystemToolResult {
    debug!("Executing system tool '{}' with params: {}", name, params);
    
    match name {
        "get_system_info" => tool_get_system_info(),
        "find_writable_paths" => tool_find_writable_paths(params, project_root),
        "check_permissions" => tool_check_permissions(params, project_root),
        "get_available_tools" => tool_get_available_tools(params),
        "diagnose_environment" => tool_diagnose_environment(),
        _ => SystemToolResult {
            success: false,
            output: format!("Unknown system tool: {}", name),
        },
    }
}

/// Get detailed system information
fn tool_get_system_info() -> SystemToolResult {
    let mut detector = PlatformCapabilityDetector::new();
    
    match detector.detect_all_capabilities() {
        Ok(capabilities) => {
            let mut info = String::new();
            
            // Basic system info
            info.push_str(&format!("🖥️  System Information\n"));
            info.push_str(&format!("=====================\n\n"));
            
            info.push_str(&format!("Platform: {}\n", capabilities.platform));
            info.push_str(&format!("Permission Level: {:?}\n", capabilities.permission_level));
            info.push_str(&format!("CPU Cores: {}\n", capabilities.system_resources.cpu_cores));
            info.push_str(&format!("Total Memory: {} MB\n", capabilities.system_resources.total_memory_mb));
            info.push_str(&format!("Available Memory: {} MB\n", capabilities.system_resources.available_memory_mb));
            
            // Available commands
            info.push_str(&format!("\n🔧 Available Commands ({})\n", capabilities.available_commands.len()));
            info.push_str(&format!("=========================\n"));
            for cmd in &capabilities.available_commands {
                info.push_str(&format!("  - {}\n", cmd));
            }
            
            // Platform features
            info.push_str(&format!("\n⚡ Platform Features\n"));
            info.push_str(&format!("==================\n"));
            info.push_str(&format!("GUI Available: {}\n", capabilities.platform_features.has_gui));
            info.push_str(&format!("Network Access: {}\n", capabilities.platform_features.has_network_access));
            info.push_str(&format!("Symlinks Supported: {}\n", capabilities.platform_features.supports_symlinks));
            info.push_str(&format!("Case Sensitive FS: {}\n", capabilities.platform_features.case_sensitive_fs));
            
            if cfg!(unix) {
                info.push_str(&format!("Has Sudo: {}\n", capabilities.platform_features.has_sudo));
            }
            
            info.push_str(&format!("Has Docker: {}\n", capabilities.platform_features.has_docker));
            info.push_str(&format!("Has Git: {}\n", capabilities.platform_features.has_git));
            info.push_str(&format!("Has Python: {}\n", capabilities.platform_features.has_python));
            info.push_str(&format!("Has Node.js: {}\n", capabilities.platform_features.has_nodejs));
            
            // Detected paths
            info.push_str(&format!("\n📂 Detected Paths\n"));
            info.push_str(&format!("===============\n"));
            info.push_str(&format!("Home Directory: {}\n", capabilities.detected_paths.home_directory));
            info.push_str(&format!("Temp Directory: {}\n", capabilities.detected_paths.temp_directory));
            
            if let Some(ref desktop) = capabilities.detected_paths.desktop_directory {
                info.push_str(&format!("Desktop: {}\n", desktop));
            }
            
            if let Some(ref documents) = capabilities.detected_paths.documents_directory {
                info.push_str(&format!("Documents: {}\n", documents));
            }
            
            info.push_str(&format!("\nWritable Paths ({}):\n", capabilities.detected_paths.writable_paths.len()));
            for path in &capabilities.detected_paths.writable_paths {
                info.push_str(&format!("  - {}\n", path));
            }
            
            info.push_str(&format!("\nExecutable Paths ({}):\n", capabilities.detected_paths.executable_paths.len()));
            for path in &capabilities.detected_paths.executable_paths {
                info.push_str(&format!("  - {}\n", path));
            }
            
            SystemToolResult {
                success: true,
                output: info,
            }
        }
        Err(e) => SystemToolResult {
            success: false,
            output: format!("Failed to detect system capabilities: {}", e),
        },
    }
}

/// Find writable paths
fn tool_find_writable_paths(params: &serde_json::Value, project_root: &Path) -> SystemToolResult {
    let mut search_paths = Vec::new();
    
    // Get search paths from parameters or use defaults
    if let Some(paths_array) = params["search_paths"].as_array() {
        for path_value in paths_array {
            if let Some(path_str) = path_value.as_str() {
                search_paths.push(path_str.to_string());
            }
        }
    }
    
    // Add default search paths if none provided
    if search_paths.is_empty() {
        search_paths.extend(vec![
            "./".to_string(),
            "../".to_string(),
            project_root.to_string_lossy().to_string(),
        ]);
        
        // Add system paths
        if let Ok(current_dir) = std::env::current_dir() {
            search_paths.push(current_dir.to_string_lossy().to_string());
        }
        
        if let Ok(home_dir) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            search_paths.push(home_dir);
        }
        
        if let Ok(temp_dir) = std::env::var("TEMP").or_else(|_| std::env::var("TMP")) {
            search_paths.push(temp_dir);
        }
    }
    
    let mut writable_paths = Vec::new();
    let mut checked_paths = Vec::new();
    
    for path_str in &search_paths {
        match resolve_path_with_fallbacks(path_str, project_root) {
            Ok(resolved) => {
                checked_paths.push(format!("{} ({:?})", resolved.path.display(), resolved.source));
                
                // Test writability
                let test_file = resolved.path.join(".write_test_tmp");
                if std::fs::write(&test_file, "").is_ok() {
                    writable_paths.push(resolved.path.to_string_lossy().to_string());
                    let _ = std::fs::remove_file(&test_file);
                }
            }
            Err(e) => {
                checked_paths.push(format!("{} (resolution failed: {})", path_str, e));
            }
        }
    }
    
    let mut output = String::new();
    output.push_str(&format!("🔍 Path Writability Check\n"));
    output.push_str(&format!("========================\n\n"));
    output.push_str(&format!("Checked {} paths:\n", checked_paths.len()));
    for path in &checked_paths {
        output.push_str(&format!("  - {}\n", path));
    }
    
    output.push_str(&format!("\n✅ Writable paths ({}):\n", writable_paths.len()));
    if writable_paths.is_empty() {
        output.push_str("  No writable paths found in the checked locations.\n");
        output.push_str("  Consider running with elevated permissions or checking different directories.\n");
    } else {
        for path in &writable_paths {
            output.push_str(&format!("  - {}\n", path));
        }
    }
    
    SystemToolResult {
        success: true,
        output,
    }
}

/// Check permissions for a specific path
fn tool_check_permissions(params: &serde_json::Value, project_root: &Path) -> SystemToolResult {
    let path_str = match params["path"].as_str() {
        Some(p) => p,
        None => return SystemToolResult {
            success: false,
            output: "Missing 'path' parameter".into(),
        },
    };
    
    match resolve_path_with_fallbacks(path_str, project_root) {
        Ok(resolved) => {
            let mut output = String::new();
            output.push_str(&format!("🔐 Permission Check for: {}\n", resolved.path.display()));
            output.push_str(&format!("================================\n\n"));
            
            output.push_str(&format!("Source: {:?}\n", resolved.source));
            output.push_str(&format!("Accessible: {}\n", resolved.is_accessible));
            output.push_str(&format!("Writable: {}\n", resolved.is_writable()));
            output.push_str(&format!("Risk Level: {:?}\n", resolved.risk_level()));
            
            // Check file metadata if exists
            if resolved.path.exists() {
                if let Ok(metadata) = std::fs::metadata(&resolved.path) {
                    output.push_str(&format!("\n📄 File Information:\n"));
                    output.push_str(&format!("Size: {} bytes\n", metadata.len()));
                    
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        output.push_str(&format!("Permissions: {:o}\n", metadata.mode()));
                        output.push_str(&format!("Owner UID: {}\n", metadata.uid()));
                        output.push_str(&format!("Owner GID: {}\n", metadata.gid()));
                    }
                    
                    if let Ok(modified) = metadata.modified() {
                        output.push_str(&format!("Last Modified: {:?}\n", modified));
                    }
                }
            } else {
                output.push_str(&format!("\n⚠️  File/Directory does not exist\n"));
            }
            
            // Check parent directory permissions
            if let Some(parent) = resolved.path.parent() {
                output.push_str(&format!("\n📁 Parent Directory ({}) Permissions:\n", parent.display()));
                
                if parent.exists() {
                    if let Ok(parent_metadata) = std::fs::metadata(parent) {
                        output.push_str(&format!("Exists: true\n"));
                        output.push_str(&format!("Readable: {}\n", parent_metadata.permissions().readonly()));
                        
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            let mode = parent_metadata.permissions().mode();
                            output.push_str(&format!("Mode: {:o}\n", mode));
                            output.push_str(&format!("Writable: {}\n", mode & 0o222 != 0));
                            output.push_str(&format!("Executable: {}\n", mode & 0o111 != 0));
                        }
                    }
                } else {
                    output.push_str(&format!("Exists: false\n"));
                }
            }
            
            SystemToolResult {
                success: true,
                output,
            }
        }
        Err(e) => SystemToolResult {
            success: false,
            output: format!("Failed to resolve path '{}': {}", path_str, e),
        },
    }
}

/// Get available system tools
fn tool_get_available_tools(params: &serde_json::Value) -> SystemToolResult {
    let mut detector = PlatformCapabilityDetector::new();
    
    match detector.detect_all_capabilities() {
        Ok(capabilities) => {
            let checker = ToolAvailabilityChecker::new(capabilities);
            let mut output = String::new();
            
            output.push_str(&format!("🔧 Available System Tools\n"));
            output.push_str(&format!("========================\n\n"));
            
            // Get filter if provided
            let filter = params["filter"].as_str().unwrap_or("");
            
            if !filter.is_empty() {
                output.push_str(&format!("Filtered by: '{}'\n\n", filter));
                let matching_tools = checker.get_matching_tools(filter);
                output.push_str(&format!("Found {} matching tools:\n", matching_tools.len()));
                for tool in matching_tools {
                    output.push_str(&format!("  - {}\n", tool));
                }
            } else {
                output.push_str(&format!("All available commands ({}):\n", checker.get_system_resources().cpu_cores));
                for tool in checker.get_available_commands() {
                    output.push_str(&format!("  - {}\n", tool));
                }
            }
            
            // Show system resources
            let resources = checker.get_system_resources();
            output.push_str(&format!("\n📊 System Resources:\n"));
            output.push_str(&format!("  CPU Cores: {}\n", resources.cpu_cores));
            output.push_str(&format!("  Total Memory: {} MB\n", resources.total_memory_mb));
            output.push_str(&format!("  Available Memory: {} MB\n", resources.available_memory_mb));
            
            SystemToolResult {
                success: true,
                output,
            }
        }
        Err(e) => SystemToolResult {
            success: false,
            output: format!("Failed to detect capabilities: {}", e),
        },
    }
}

/// Run comprehensive environment diagnostics
fn tool_diagnose_environment() -> SystemToolResult {
    let mut output = String::new();
    output.push_str(&format!("🏥 Environment Diagnostics\n"));
    output.push_str(&format!("=========================\n\n"));
    
    // Run system info check
    let system_info_result = tool_get_system_info();
    output.push_str(&format!("{}\n", system_info_result.output));
    
    // Check current working directory
    output.push_str(&format!("\n📍 Current Environment\n"));
    output.push_str(&format!("=====================\n"));
    
    if let Ok(current_dir) = std::env::current_dir() {
        output.push_str(&format!("Current Directory: {}\n", current_dir.display()));
    }
    
    // Check environment variables
    output.push_str(&format!("\n🌐 Environment Variables\n"));
    output.push_str(&format!("======================\n"));
    let important_vars = ["PATH", "HOME", "USERPROFILE", "TEMP", "TMP", "USER", "USERNAME"];
    for var in &important_vars {
        if let Ok(value) = std::env::var(var) {
            output.push_str(&format!("{}: {}\n", var, value));
        }
    }
    
    // Check disk space
    output.push_str(&format!("\n💾 Disk Space\n"));
    output.push_str(&format!("============\n"));
    if let Ok(current_dir) = std::env::current_dir() {
        if let Some(_root) = current_dir.to_str() {
            #[cfg(unix)]
            {
                use libc::statvfs;
                use std::ffi::CString;
                
                let c_path = CString::new(root).unwrap();
                let mut stat: statvfs = unsafe { std::mem::zeroed() };
                
                if unsafe { statvfs(c_path.as_ptr(), &mut stat) } == 0 {
                    let block_size = stat.f_frsize as u64;
                    let total_blocks = stat.f_blocks;
                    let available_blocks = stat.f_bavail;
                    
                    let total_mb = (total_blocks * block_size) / (1024 * 1024);
                    let available_mb = (available_blocks * block_size) / (1024 * 1024);
                    
                    output.push_str(&format!("Total Space: {} MB\n", total_mb));
                    output.push_str(&format!("Available Space: {} MB\n", available_mb));
                    output.push_str(&format!("Usage: {:.1}%\n", 
                        ((total_mb - available_mb) as f64 / total_mb as f64) * 100.0));
                }
            }
        }
    }
    
    SystemToolResult {
        success: true,
        output,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_system_tool_definitions() {
        let tools = system_tool_definitions();
        assert!(!tools.is_empty());
        assert!(tools.iter().any(|t| t.name == "get_system_info"));
    }
    
    #[test]
    fn test_get_system_info() {
        let result = tool_get_system_info();
        assert!(result.success);
        assert!(!result.output.is_empty());
        assert!(result.output.contains("System Information"));
    }
}