//! Capability discovery framework for dynamic tool availability detection.
//!
//! This module provides runtime detection of system capabilities including:
//! - Available tools and commands
//! - Permission levels
//! - External dependencies
//! - System resources
//! - Platform-specific features

pub mod detector;

use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// System capability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemCapabilities {
    pub platform: String,
    pub available_commands: Vec<String>,
    pub permission_level: PermissionLevel,
    pub external_dependencies: HashMap<String, DependencyStatus>,
    pub system_resources: SystemResources,
    pub platform_features: PlatformFeatures,
    pub detected_paths: DetectedPaths,
}

/// Permission level assessment
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PermissionLevel {
    Restricted,
    User,
    Elevated,
    Admin,
}

/// Status of external dependencies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DependencyStatus {
    Available,
    Unavailable,
    VersionMismatch { required: String, found: Option<String> },
    PermissionDenied,
}

/// System resource information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemResources {
    pub total_memory_mb: u64,
    pub available_memory_mb: u64,
    pub cpu_cores: usize,
    pub disk_space: HashMap<String, DiskSpaceInfo>,
}

/// Disk space information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskSpaceInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub is_removable: bool,
}

/// Platform-specific features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformFeatures {
    pub has_gui: bool,
    pub has_network_access: bool,
    pub supports_symlinks: bool,
    pub case_sensitive_fs: bool,
    pub has_sudo: bool,
    pub has_docker: bool,
    pub has_git: bool,
    pub has_python: bool,
    pub has_nodejs: bool,
}

/// Detected important system paths
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedPaths {
    pub home_directory: String,
    pub temp_directory: String,
    pub desktop_directory: Option<String>,
    pub documents_directory: Option<String>,
    pub downloads_directory: Option<String>,
    pub writable_paths: Vec<String>,
    pub executable_paths: Vec<String>,
}

/// Capability detector
pub struct CapabilityDetector {
    cached_capabilities: Option<SystemCapabilities>,
}

impl CapabilityDetector {
    /// Create a new capability detector
    pub fn new() -> Self {
        Self {
            cached_capabilities: None,
        }
    }

    /// Detect all system capabilities
    pub fn detect_capabilities(&mut self) -> Result<SystemCapabilities, anyhow::Error> {
        debug!("Detecting system capabilities...");
        
        // Return cached if available
        if let Some(ref cached) = self.cached_capabilities {
            return Ok(cached.clone());
        }
        
        let capabilities = SystemCapabilities {
            platform: self.detect_platform(),
            available_commands: self.detect_available_commands(),
            permission_level: self.detect_permission_level(),
            external_dependencies: self.detect_external_dependencies(),
            system_resources: self.detect_system_resources(),
            platform_features: self.detect_platform_features(),
            detected_paths: self.detect_important_paths(),
        };
        
        self.cached_capabilities = Some(capabilities.clone());
        Ok(capabilities)
    }

    /// Detect the current platform
    fn detect_platform(&self) -> String {
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
    }

    /// Detect available system commands
    fn detect_available_commands(&self) -> Vec<String> {
        let mut commands = Vec::new();
        
        // Common commands to check
        let common_commands = [
            "ls", "dir", "cat", "type", "cp", "copy", "mv", "move", 
            "rm", "del", "mkdir", "rmdir", "find", "where", "which",
            "ps", "tasklist", "kill", "taskkill", "ping", "curl", "wget",
            "git", "python", "python3", "node", "npm", "docker"
        ];
        
        for &cmd in &common_commands {
            if self.is_command_available(cmd) {
                commands.push(cmd.to_string());
            }
        }
        
        commands.sort();
        commands.dedup();
        commands
    }

    /// Check if a command is available
    fn is_command_available(&self, command: &str) -> bool {
        #[cfg(windows)]
        {
            use std::process::Command;
            
            // On Windows, check if command exists in PATH
            let output = Command::new("where")
                .arg(command)
                .output();
                
            match output {
                Ok(output) => output.status.success() && !output.stdout.is_empty(),
                Err(_) => {
                    // Fallback: try running the command
                    Command::new("cmd")
                        .args(&["/C", &format!("{} --version", command)])
                        .output()
                        .map(|output| output.status.success())
                        .unwrap_or(false)
                }
            }
        }
        
        #[cfg(unix)]
        {
            use std::process::Command;
            
            let output = Command::new("which")
                .arg(command)
                .output();
                
            match output {
                Ok(output) => output.status.success() && !output.stdout.is_empty(),
                Err(_) => {
                    // Fallback: try running the command
                    Command::new("sh")
                        .args(&["-c", &format!("command -v {}", command)])
                        .output()
                        .map(|output| output.status.success())
                        .unwrap_or(false)
                }
            }
        }
    }

    /// Detect permission level
    fn detect_permission_level(&self) -> PermissionLevel {
        // Simplified permission detection
        #[cfg(windows)]
        {
            // Basic check: can we write to temp directory?
            let temp_path = std::env::temp_dir().join("perm_test.tmp");
            match std::fs::write(&temp_path, "test") {
                Ok(_) => {
                    let _ = std::fs::remove_file(&temp_path);
                    // Check if we can write to user profile
                    if let Ok(user_profile) = std::env::var("USERPROFILE") {
                        let test_path = Path::new(&user_profile).join("perm_test.tmp");
                        if std::fs::write(&test_path, "test").is_ok() {
                            let _ = std::fs::remove_file(&test_path);
                            PermissionLevel::User
                        } else {
                            PermissionLevel::Restricted
                        }
                    } else {
                        PermissionLevel::User
                    }
                },
                Err(_) => PermissionLevel::Restricted,
            }
        }
        
        #[cfg(unix)]
        {
            // Check if running as root
            if unsafe { libc::geteuid() } == 0 {
                PermissionLevel::Admin
            } else {
                // Check if we can write to home directory
                if let Ok(home) = std::env::var("HOME") {
                    let test_path = Path::new(&home).join(".perm_test.tmp");
                    if std::fs::write(&test_path, "test").is_ok() {
                        let _ = std::fs::remove_file(&test_path);
                        PermissionLevel::User
                    } else {
                        PermissionLevel::Restricted
                    }
                } else {
                    PermissionLevel::Restricted
                }
            }
        }
    }

    /// Detect external dependencies
    fn detect_external_dependencies(&self) -> HashMap<String, DependencyStatus> {
        let mut dependencies = HashMap::new();
        
        // Check for common dependencies
        let checks = [
            ("git", "--version"),
            ("python", "--version"),
            ("python3", "--version"),
            ("node", "--version"),
            ("npm", "--version"),
            ("docker", "--version"),
            ("cargo", "--version"),
        ];
        
        for &(dep, version_cmd) in &checks {
            let status = self.check_dependency(dep, version_cmd);
            dependencies.insert(dep.to_string(), status);
        }
        
        dependencies
    }

    /// Check a specific dependency
    fn check_dependency(&self, command: &str, version_cmd: &str) -> DependencyStatus {
        use std::process::Command;
        
        #[cfg(windows)]
        let output = Command::new("cmd")
            .args(&["/C", &format!("{} {}", command, version_cmd)])
            .output();
            
        #[cfg(unix)]
        let output = Command::new("sh")
            .args(&["-c", &format!("{} {}", command, version_cmd)])
            .output();
        
        match output {
            Ok(output) if output.status.success() => {
                let _version_output = String::from_utf8_lossy(&output.stdout);
                DependencyStatus::Available // Could parse version here if needed
            }
            Ok(_) => DependencyStatus::Unavailable,
            Err(_) => DependencyStatus::PermissionDenied,
        }
    }

    /// Detect system resources
    fn detect_system_resources(&self) -> SystemResources {
        #[cfg(windows)]
        {
            use winapi::um::sysinfoapi::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
            
            let mut mem_status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
            mem_status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            
            unsafe {
                GlobalMemoryStatusEx(&mut mem_status);
            }
            
            SystemResources {
                total_memory_mb: mem_status.ullTotalPhys / (1024 * 1024),
                available_memory_mb: mem_status.ullAvailPhys / (1024 * 1024),
                cpu_cores: num_cpus::get(),
                disk_space: self.detect_disk_space(),
            }
        }
        
        #[cfg(unix)]
        {
            use libc::{sysconf, _SC_PAGESIZE, _SC_PHYS_PAGES, _SC_AVPHYS_PAGES};
            
            let page_size = unsafe { sysconf(_SC_PAGESIZE) } as u64;
            let total_pages = unsafe { sysconf(_SC_PHYS_PAGES) } as u64;
            let avail_pages = unsafe { sysconf(_SC_AVPHYS_PAGES) } as u64;
            
            SystemResources {
                total_memory_mb: (total_pages * page_size) / (1024 * 1024),
                available_memory_mb: (avail_pages * page_size) / (1024 * 1024),
                cpu_cores: num_cpus::get(),
                disk_space: self.detect_disk_space(),
            }
        }
    }

    /// Detect disk space information
    fn detect_disk_space(&self) -> HashMap<String, DiskSpaceInfo> {
        let mut disk_info = HashMap::new();
        
        // Simplified disk space detection
        // Get current directory's disk
        if let Ok(current_dir) = std::env::current_dir() {
            if let Some(root_str) = current_dir.to_str() {
                #[cfg(windows)]
                {
                    // Simple approximation - get total and available space from temp dir
                    let temp_total = 100_000_000_000u64; // 100GB approximation
                    let temp_available = 50_000_000_000u64; // 50GB approximation
                    
                    disk_info.insert(root_str.to_string(), DiskSpaceInfo {
                        total_bytes: temp_total,
                        available_bytes: temp_available,
                        is_removable: false,
                    });
                }
                
                #[cfg(unix)]
                {
                    use libc::statvfs;
                    use std::ffi::CString;
                    
                    let c_path = CString::new(root_str).unwrap();
                    let mut stat: statvfs = unsafe { std::mem::zeroed() };
                    
                    if unsafe { statvfs(c_path.as_ptr(), &mut stat) } == 0 {
                        let block_size = stat.f_frsize as u64;
                        let total_blocks = stat.f_blocks;
                        let available_blocks = stat.f_bavail;
                        
                        disk_info.insert(root_str.to_string(), DiskSpaceInfo {
                            total_bytes: total_blocks * block_size,
                            available_bytes: available_blocks * block_size,
                            is_removable: false,
                        });
                    }
                }
            }
        }
        
        disk_info
    }

    /// Detect platform-specific features
    fn detect_platform_features(&self) -> PlatformFeatures {
        PlatformFeatures {
            has_gui: self.detect_gui_availability(),
            has_network_access: self.detect_network_access(),
            supports_symlinks: cfg!(unix) || self.test_symlink_support(),
            case_sensitive_fs: cfg!(unix),
            has_sudo: cfg!(unix) && self.is_command_available("sudo"),
            has_docker: matches!(self.check_dependency("docker", "--version"), DependencyStatus::Available),
            has_git: matches!(self.check_dependency("git", "--version"), DependencyStatus::Available),
            has_python: matches!(self.check_dependency("python", "--version"), DependencyStatus::Available) ||
                       matches!(self.check_dependency("python3", "--version"), DependencyStatus::Available),
            has_nodejs: matches!(self.check_dependency("node", "--version"), DependencyStatus::Available),
        }
    }

    /// Detect GUI availability
    fn detect_gui_availability(&self) -> bool {
        #[cfg(windows)]
        {
            // Check if running in console vs GUI context
            unsafe {
                let console = winapi::um::wincon::GetConsoleWindow();
                !console.is_null()
            }
        }
        
        #[cfg(unix)]
        {
            std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok()
        }
    }

    /// Detect network access
    fn detect_network_access(&self) -> bool {
        // Simple ping test to a reliable host
        use std::process::Command;
        use std::time::Duration;
        
        #[cfg(windows)]
        let output = Command::new("ping")
            .args(&["-n", "1", "-w", "1000", "8.8.8.8"])
            .output();
            
        #[cfg(unix)]
        let output = Command::new("ping")
            .args(&["-c", "1", "-W", "1", "8.8.8.8"])
            .output();
        
        match output {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// Test symlink support
    fn test_symlink_support(&self) -> bool {
        // Simple test - try to create a symlink
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("symlink_test_source");
        let test_link = temp_dir.join("symlink_test_link");
        
        // Create source file
        if std::fs::write(&test_file, "test").is_ok() {
            // Try to create symlink
            #[cfg(windows)]
            let result = std::os::windows::fs::symlink_file(&test_file, &test_link);
            
            #[cfg(unix)]
            let result = std::os::unix::fs::symlink(&test_file, &test_link);
            
            let success = result.is_ok();
            
            // Clean up
            let _ = std::fs::remove_file(&test_file);
            let _ = std::fs::remove_file(&test_link);
            
            success
        } else {
            false
        }
    }

    /// Detect important system paths
    fn detect_important_paths(&self) -> DetectedPaths {
        DetectedPaths {
            home_directory: self.get_home_directory(),
            temp_directory: std::env::temp_dir().to_string_lossy().to_string(),
            desktop_directory: self.get_desktop_directory(),
            documents_directory: self.get_documents_directory(),
            downloads_directory: self.get_downloads_directory(),
            writable_paths: self.find_writable_paths(),
            executable_paths: self.find_executable_paths(),
        }
    }

    /// Get home directory
    fn get_home_directory(&self) -> String {
        #[cfg(windows)]
        {
            std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string())
        }
        
        #[cfg(unix)]
        {
            std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
        }
    }

    /// Get desktop directory
    fn get_desktop_directory(&self) -> Option<String> {
        #[cfg(windows)]
        {
            if let Ok(user_profile) = std::env::var("USERPROFILE") {
                let desktop = Path::new(&user_profile).join("Desktop");
                if desktop.exists() {
                    return Some(desktop.to_string_lossy().to_string());
                }
            }
        }
        
        #[cfg(unix)]
        {
            if let Ok(home) = std::env::var("HOME") {
                let desktop = Path::new(&home).join("Desktop");
                if desktop.exists() {
                    return Some(desktop.to_string_lossy().to_string());
                }
            }
        }
        
        None
    }

    /// Get documents directory
    fn get_documents_directory(&self) -> Option<String> {
        // Similar implementation to get_desktop_directory but for Documents
        // This would typically use platform-specific APIs
        None
    }

    /// Get downloads directory
    fn get_downloads_directory(&self) -> Option<String> {
        // Similar implementation for Downloads directory
        None
    }

    /// Find writable paths
    fn find_writable_paths(&self) -> Vec<String> {
        let mut writable = Vec::new();
        
        // Check common writable locations
        let candidates = [
            "./",  // Current directory
            "../", // Parent directory
        ];
        
        for candidate in &candidates {
            let path = Path::new(candidate);
            if path.exists() {
                // Test writability
                let test_file = path.join(".write_test_tmp");
                if std::fs::write(&test_file, "").is_ok() {
                    writable.push(candidate.to_string());
                    let _ = std::fs::remove_file(&test_file);
                }
            }
        }
        
        writable
    }

    /// Find executable paths
    fn find_executable_paths(&self) -> Vec<String> {
        if let Ok(path_env) = std::env::var("PATH") {
            #[cfg(windows)]
            let separator = ';';
            #[cfg(unix)]
            let separator = ':';
            
            path_env.split(separator)
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            vec![]
        }
    }

    /// Invalidate cached capabilities (useful after permission changes)
    pub fn invalidate_cache(&mut self) {
        self.cached_capabilities = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_detection() {
        let mut detector = CapabilityDetector::new();
        let capabilities = detector.detect_capabilities().unwrap();
        
        assert!(!capabilities.platform.is_empty());
        assert!(!capabilities.available_commands.is_empty());
        assert!(capabilities.system_resources.cpu_cores > 0);
    }

    #[test]
    fn test_command_availability() {
        let detector = CapabilityDetector::new();
        
        // Platform-aware command checking
        #[cfg(windows)]
        {
            assert!(detector.is_command_available("dir") || detector.is_command_available("cmd"));
        }
        
        #[cfg(unix)]
        {
            assert!(detector.is_command_available("ls") || detector.is_command_available("pwd"));
        }
    }
}