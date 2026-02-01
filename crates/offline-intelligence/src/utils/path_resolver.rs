//! Advanced path resolution with environment variable expansion and fallback mechanisms.
//!
//! This module provides intelligent path resolution that handles:
//! - Environment variable expansion
//! - Platform-specific path construction
//! - Accessibility validation
//! - Fallback path discovery
//! - User directory resolution

use std::path::{Path, PathBuf};
use tracing::debug;

/// Enhanced path resolution result with metadata
#[derive(Debug)]
pub struct ResolvedPath {
    pub path: PathBuf,
    pub source: PathSource,
    pub is_accessible: bool,
    pub alternatives: Vec<PathBuf>,
}

/// Source of the resolved path
#[derive(Debug, Clone, PartialEq)]
pub enum PathSource {
    Direct,
    EnvironmentVariable(String),
    PlatformDefault,
    Fallback,
    UserHome,
    Desktop,
    Documents,
}

/// Risk level for path operations
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PathRisk {
    Low,
    Medium,
    High,
}

impl ResolvedPath {
    /// Check if the path is writable
    pub fn is_writable(&self) -> bool {
        self.is_accessible && self.path.parent()
            .map(|parent| {
                parent.exists() && parent.metadata()
                    .map(|meta| !meta.permissions().readonly())
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    /// Get risk level for operations on this path
    pub fn risk_level(&self) -> PathRisk {
        if self.source == PathSource::PlatformDefault || self.source == PathSource::Fallback {
            return PathRisk::High;
        }
        
        if !self.is_accessible {
            return PathRisk::High;
        }
        
        // Check if path is in system directories
        let path_str = self.path.to_string_lossy().to_lowercase();
        if path_str.contains("program files") || 
           path_str.contains("windows") || 
           path_str.contains("/etc/") ||
           path_str.contains("/system/") {
            PathRisk::High
        } else if path_str.contains("desktop") || path_str.contains("documents") {
            PathRisk::Medium
        } else {
            PathRisk::Low
        }
    }
}

/// Resolve a path with intelligent fallback mechanisms
pub fn resolve_path_with_fallbacks(path_str: &str, project_root: &Path) -> Result<ResolvedPath, anyhow::Error> {
    debug!("Resolving path: {}", path_str);
    
    // 1. Try direct resolution first
    if let Ok(direct_path) = resolve_direct_path(path_str, project_root) {
        if direct_path.exists() || direct_path.parent().map(|p| p.exists()).unwrap_or(false) {
            return Ok(ResolvedPath {
                path: direct_path.clone(),
                source: PathSource::Direct,
                is_accessible: is_path_accessible(&direct_path),
                alternatives: vec![],
            });
        }
    }
    
    // 2. Expand environment variables
    if let Ok(expanded_path) = expand_environment_variables(path_str) {
        if expanded_path != path_str {
            let resolved = PathBuf::from(&expanded_path);
            if resolved.exists() || resolved.parent().map(|p| p.exists()).unwrap_or(false) {
                return Ok(ResolvedPath {
                    path: resolved.clone(),
                    source: PathSource::EnvironmentVariable(find_env_var_name(path_str)),
                    is_accessible: is_path_accessible(&resolved),
                    alternatives: vec![],
                });
            }
        }
    }
    
    // 3. Handle special path patterns
    if let Some(special_path) = resolve_special_paths(path_str)? {
        return Ok(ResolvedPath {
            path: special_path.path,
            source: special_path.source,
            is_accessible: special_path.is_accessible,
            alternatives: special_path.alternatives,
        });
    }
    
    // 4. Try platform-specific resolution
    let platform_path = resolve_platform_specific(path_str, project_root)?;
    if platform_path.exists() || platform_path.parent().map(|p| p.exists()).unwrap_or(false) {
        return Ok(ResolvedPath {
            path: platform_path.clone(),
            source: PathSource::PlatformDefault,
            is_accessible: is_path_accessible(&platform_path),
            alternatives: vec![],
        });
    }
    
    // 5. Suggest fallback paths
    let alternatives = suggest_alternative_paths(path_str, project_root)?;
    
    // Return the best available path with alternatives
    Ok(ResolvedPath {
        path: platform_path,
        source: PathSource::Fallback,
        is_accessible: false,
        alternatives,
    })
}

/// Direct path resolution relative to project root
fn resolve_direct_path(path_str: &str, project_root: &Path) -> Result<PathBuf, anyhow::Error> {
    let path = Path::new(path_str);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(project_root.join(path))
    }
}

/// Expand environment variables in path strings
fn expand_environment_variables(path_str: &str) -> Result<String, anyhow::Error> {
    let mut result = path_str.to_string();
    
    // Common environment variables to expand
    let env_vars = [
        ("USERPROFILE", "USERPROFILE"),
        ("HOME", "HOME"),
        ("TEMP", "TEMP"),
        ("TMP", "TEMP"),
        ("APPDATA", "APPDATA"),
        ("LOCALAPPDATA", "LOCALAPPDATA"),
    ];
    
    for (placeholder, env_var) in &env_vars {
        if result.contains(placeholder) {
            if let Ok(value) = std::env::var(env_var) {
                result = result.replace(placeholder, &value);
            }
        }
    }
    
    // Handle tilde expansion on Unix-like systems
    if cfg!(unix) && result.starts_with('~') {
        if let Ok(home) = std::env::var("HOME") {
            result = result.replacen('~', &home, 1);
        }
    }
    
    Ok(result)
}

/// Find the environment variable name from a path string
fn find_env_var_name(path_str: &str) -> String {
    if path_str.contains("USERPROFILE") {
        "USERPROFILE".to_string()
    } else if path_str.contains("HOME") {
        "HOME".to_string()
    } else if path_str.contains("TEMP") || path_str.contains("TMP") {
        "TEMP".to_string()
    } else {
        "UNKNOWN".to_string()
    }
}

/// Extract the subpath after a special directory keyword (Desktop, Documents, etc.)
/// For example: "Desktop/pyt.py" -> Some("pyt.py")
/// "C:\Users\someone\Desktop\file.txt" -> Some("file.txt")
/// "path/to/your/Desktop/pyt.py" -> Some("pyt.py")
fn extract_subpath_after_keyword(path_str: &str, keyword: &str) -> Option<String> {
    let lower = path_str.to_lowercase();
    let kw_lower = keyword.to_lowercase();
    if let Some(pos) = lower.find(&kw_lower) {
        let after = &path_str[pos + keyword.len()..];
        // Strip leading separators
        let trimmed = after.trim_start_matches(|c| c == '/' || c == '\\');
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Resolve special path patterns like Desktop, Documents, etc.
fn resolve_special_paths(path_str: &str) -> Result<Option<ResolvedPath>, anyhow::Error> {
    let lower_path = path_str.to_lowercase();

    // Handle Desktop resolution
    if lower_path.contains("desktop") {
        if let Ok(desktop_dir) = get_desktop_path() {
            // Extract any subpath after "Desktop" (e.g., "Desktop/pyt.py" -> "pyt.py")
            let is_accessible = is_path_accessible(&desktop_dir);
            let full_path = match extract_subpath_after_keyword(path_str, "Desktop") {
                Some(subpath) => desktop_dir.join(subpath),
                None => desktop_dir,
            };
            return Ok(Some(ResolvedPath {
                path: full_path,
                source: PathSource::Desktop,
                is_accessible,
                alternatives: vec![],
            }));
        }
    }

    // Handle Documents resolution
    if lower_path.contains("documents") || lower_path.contains("my documents") {
        let keyword = if lower_path.contains("my documents") { "my documents" } else { "documents" };
        if let Ok(documents_dir) = get_documents_path() {
            let is_accessible = is_path_accessible(&documents_dir);
            let full_path = match extract_subpath_after_keyword(path_str, keyword) {
                Some(subpath) => documents_dir.join(subpath),
                None => documents_dir,
            };
            return Ok(Some(ResolvedPath {
                path: full_path,
                source: PathSource::Documents,
                is_accessible,
                alternatives: vec![],
            }));
        }
    }

    // Handle user home directory
    if lower_path.contains("home") || lower_path.contains("users") {
        if let Ok(home_path) = get_home_directory() {
            return Ok(Some(ResolvedPath {
                path: home_path,
                source: PathSource::UserHome,
                is_accessible: true,
                alternatives: vec![],
            }));
        }
    }

    Ok(None)
}

/// Get the user's Desktop path
pub fn get_desktop_path() -> Result<PathBuf, anyhow::Error> {
    #[cfg(windows)]
    {
        if let Ok(user_profile) = std::env::var("USERPROFILE") {
            let desktop_path = PathBuf::from(&user_profile).join("Desktop");
            if desktop_path.exists() {
                return Ok(desktop_path);
            }
            // OneDrive Desktop redirect
            let onedrive_desktop = PathBuf::from(&user_profile).join("OneDrive").join("Desktop");
            if onedrive_desktop.exists() {
                return Ok(onedrive_desktop);
            }
            // Even if Desktop doesn't exist yet, return the standard path
            // so the caller can create it
            return Ok(PathBuf::from(user_profile).join("Desktop"));
        }
    }

    #[cfg(unix)]
    {
        if let Ok(home) = std::env::var("HOME") {
            let desktop_path = PathBuf::from(home).join("Desktop");
            return Ok(desktop_path);
        }
    }

    Err(anyhow::anyhow!("Could not determine Desktop path"))
}

/// Get the user's Documents path
pub fn get_documents_path() -> Result<PathBuf, anyhow::Error> {
    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        use winapi::um::combaseapi::CoTaskMemFree;
        use winapi::um::knownfolders::FOLDERID_Documents;
        use winapi::um::shlobj::SHGetKnownFolderPath;
        use winapi::um::winnt::PWSTR;
        
        unsafe {
            let mut path_ptr: PWSTR = std::ptr::null_mut();
            let result = SHGetKnownFolderPath(&FOLDERID_Documents, 0, std::ptr::null_mut(), &mut path_ptr);
            
            if result == 0 && !path_ptr.is_null() {
                let len = (0..).take_while(|&i| *path_ptr.offset(i) != 0).count();
                let slice = std::slice::from_raw_parts(path_ptr, len);
                let os_string = OsString::from_wide(slice);
                CoTaskMemFree(path_ptr as *mut _);
                
                let documents_path = PathBuf::from(os_string);
                if documents_path.exists() {
                    return Ok(documents_path);
                }
            }
        }
        
        // Fallback
        if let Ok(user_profile) = std::env::var("USERPROFILE") {
            return Ok(PathBuf::from(user_profile).join("Documents"));
        }
    }
    
    #[cfg(unix)]
    {
        if let Ok(home) = std::env::var("HOME") {
            return Ok(PathBuf::from(home).join("Documents"));
        }
    }
    
    Err(anyhow::anyhow!("Could not determine Documents path"))
}

/// Get the user's home directory
pub fn get_home_directory() -> Result<PathBuf, anyhow::Error> {
    #[cfg(windows)]
    {
        if let Ok(user_profile) = std::env::var("USERPROFILE") {
            return Ok(PathBuf::from(user_profile));
        }
    }

    #[cfg(unix)]
    {
        if let Ok(home) = std::env::var("HOME") {
            return Ok(PathBuf::from(home));
        }
    }

    Err(anyhow::anyhow!("Could not determine home directory"))
}

/// Platform-specific path resolution
fn resolve_platform_specific(path_str: &str, project_root: &Path) -> Result<PathBuf, anyhow::Error> {
    #[cfg(windows)]
    {
        // Handle Windows-specific paths
        let normalized = path_str.replace('/', "\\");
        if normalized.starts_with('\\') {
            // Absolute Windows path
            return Ok(PathBuf::from(normalized));
        }
    }
    
    #[cfg(unix)]
    {
        // Handle Unix-specific paths
        if path_str.starts_with('/') {
            // Absolute Unix path
            return Ok(PathBuf::from(path_str));
        }
    }
    
    // Default to project-relative path
    Ok(project_root.join(path_str))
}

/// Check if a path is accessible (exists and readable)
fn is_path_accessible(path: &Path) -> bool {
    path.exists() && 
    std::fs::metadata(path)
        .map(|meta| {
            let _perms = meta.permissions();
            // Check if readable (on Unix) or just exists (on Windows)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                perms.mode() & 0o444 != 0 // Check read permissions for user/group/others
            }
            #[cfg(windows)]
            {
                true // Windows handles permissions differently
            }
        })
        .unwrap_or(false)
}

/// Suggest alternative writable paths when primary path fails
fn suggest_alternative_paths(_path_str: &str, project_root: &Path) -> Result<Vec<PathBuf>, anyhow::Error> {
    let mut alternatives = Vec::new();
    
    // 1. Project root
    alternatives.push(project_root.to_path_buf());
    
    // 2. Temp directory
    let temp_dir = std::env::temp_dir();
    alternatives.push(temp_dir);
    
    // 3. Current directory
    if let Ok(current_dir) = std::env::current_dir() {
        alternatives.push(current_dir);
    }
    
    // 4. User home directory
    if let Ok(home_dir) = get_home_directory() {
        alternatives.push(home_dir);
    }
    
    // Filter to only accessible paths
    let accessible_alternatives: Vec<PathBuf> = alternatives
        .into_iter()
        .filter(|path| is_path_accessible(path))
        .collect();
    
    Ok(accessible_alternatives)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_expand_environment_variables() {
        // Platform-aware test
        #[cfg(windows)]
        {
            let result = expand_environment_variables("%USERPROFILE%/test").unwrap();
            assert!(!result.contains("%USERPROFILE%")); // Should be expanded
            
            // Test with $HOME-style (might not exist on Windows)
            let result2 = expand_environment_variables("$TEMP/test").unwrap();
            assert!(!result2.contains("$TEMP")); // Should be expanded if exists
        }
        
        #[cfg(unix)]
        {
            let result = expand_environment_variables("$HOME/test").unwrap();
            assert!(!result.contains("$HOME")); // Should be expanded
        }
    }
    
    #[test]
    fn test_resolve_path_with_fallbacks() {
        let project_root = std::env::current_dir().unwrap();
        let result = resolve_path_with_fallbacks("test.txt", &project_root).unwrap();
        assert_eq!(result.path, project_root.join("test.txt"));
        assert_eq!(result.source, PathSource::Direct);
    }
}