//! Capability detector implementation with platform-specific optimizations.

use super::*;

/// Platform-specific capability detector
pub struct PlatformCapabilityDetector {
    base_detector: CapabilityDetector,
}

impl PlatformCapabilityDetector {
    /// Create a new platform-specific detector
    pub fn new() -> Self {
        Self {
            base_detector: CapabilityDetector::new(),
        }
    }

    /// Detect all capabilities with platform optimizations
    pub fn detect_all_capabilities(&mut self) -> Result<SystemCapabilities, anyhow::Error> {
        let mut capabilities = self.base_detector.detect_capabilities()?;
        
        // Apply platform-specific enhancements
        self.enhance_with_platform_features(&mut capabilities)?;
        
        Ok(capabilities)
    }

    /// Enhance capabilities with platform-specific information
    fn enhance_with_platform_features(&self, capabilities: &mut SystemCapabilities) -> Result<(), anyhow::Error> {
        #[cfg(windows)]
        self.enhance_windows_capabilities(capabilities)?;
        
        #[cfg(unix)]
        self.enhance_unix_capabilities(capabilities)?;
        
        Ok(())
    }

    /// Enhance capabilities with Windows-specific information
    #[cfg(windows)]
    fn enhance_windows_capabilities(&self, capabilities: &mut SystemCapabilities) -> Result<(), anyhow::Error> {
        use winapi::um::sysinfoapi::{GetSystemInfo, SYSTEM_INFO};
        use winapi::um::processthreadsapi::GetCurrentProcessId;
        
        // Get detailed system information
        let mut system_info: SYSTEM_INFO = unsafe { std::mem::zeroed() };
        unsafe { GetSystemInfo(&mut system_info); }
        
        // Add Windows-specific features
        capabilities.platform_features.has_gui = true; // Windows typically has GUI
        
        // Detect Windows-specific paths
        if let Ok(app_data) = std::env::var("APPDATA") {
            capabilities.detected_paths.executable_paths.push(app_data);
        }
        
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            capabilities.detected_paths.writable_paths.push(local_app_data);
        }
        
        Ok(())
    }

    /// Enhance capabilities with Unix-specific information
    #[cfg(unix)]
    fn enhance_unix_capabilities(&self, capabilities: &mut SystemCapabilities) -> Result<(), anyhow::Error> {
        // Check for common Unix tools
        let unix_tools = ["sudo", "chmod", "chown", "ln", "tar", "gzip"];
        for &tool in &unix_tools {
            if self.base_detector.is_command_available(tool) {
                capabilities.available_commands.push(tool.to_string());
            }
        }
        
        // Detect Unix-specific paths
        let unix_paths = ["/usr/local/bin", "/opt", "/var/tmp"];
        for &path in &unix_paths {
            if Path::new(path).exists() {
                capabilities.detected_paths.executable_paths.push(path.to_string());
            }
        }
        
        Ok(())
    }
}

/// Tool availability checker
pub struct ToolAvailabilityChecker {
    pub(crate) capabilities: SystemCapabilities,
}

impl ToolAvailabilityChecker {
    /// Create a new tool availability checker
    pub fn new(capabilities: SystemCapabilities) -> Self {
        Self { capabilities }
    }

    /// Check if a specific tool is available
    pub fn is_tool_available(&self, tool_name: &str) -> bool {
        self.capabilities.available_commands.contains(&tool_name.to_lowercase())
    }

    /// Get available tools that match a pattern
    pub fn get_matching_tools(&self, pattern: &str) -> Vec<String> {
        self.capabilities.available_commands
            .iter()
            .filter(|tool| tool.contains(pattern))
            .cloned()
            .collect()
    }

    /// Check if required dependencies are met
    pub fn check_dependencies(&self, required_deps: &[&str]) -> Vec<(String, bool)> {
        required_deps.iter()
            .map(|&dep| (dep.to_string(), self.is_tool_available(dep)))
            .collect()
    }

    /// Get available commands
    pub fn get_available_commands(&self) -> &Vec<String> {
        &self.capabilities.available_commands
    }

    /// Get system resources information
    pub fn get_system_resources(&self) -> &SystemResources {
        &self.capabilities.system_resources
    }

    /// Check if system meets minimum requirements
    pub fn meets_minimum_requirements(&self, min_memory_mb: u64, min_cpu_cores: usize) -> bool {
        self.capabilities.system_resources.total_memory_mb >= min_memory_mb &&
        self.capabilities.system_resources.cpu_cores >= min_cpu_cores
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_availability_checker() {
        let mut detector = PlatformCapabilityDetector::new();
        let capabilities = detector.detect_all_capabilities().unwrap();
        
        let checker = ToolAvailabilityChecker::new(capabilities);
        
        // Platform-aware tool checking - be more flexible
        #[cfg(windows)]
        {
            // At least one common Windows tool should be available
            let has_windows_tool = checker.is_tool_available("dir") || 
                                 checker.is_tool_available("cmd") || 
                                 checker.is_tool_available("powershell") ||
                                 checker.is_tool_available("whoami");
            assert!(has_windows_tool, "No Windows tools detected as available");
        }
        
        #[cfg(unix)]
        {
            // At least one common Unix tool should be available
            let has_unix_tool = checker.is_tool_available("ls") || 
                              checker.is_tool_available("pwd") || 
                              checker.is_tool_available("whoami");
            assert!(has_unix_tool, "No Unix tools detected as available");
        }
    }
}