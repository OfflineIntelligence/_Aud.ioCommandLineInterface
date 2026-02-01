//! Llama.cpp Binary Management
//!
//! Automatically detects the user's OS, architecture, and GPU to download
//! the correct llama-server binary. The binary is stored alongside models
//! in the platform AppData directory.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::info;

/// Supported platforms for llama.cpp binaries
#[derive(Debug, Clone, PartialEq)]
pub enum LlamaPlatform {
    WindowsCuda,
    WindowsCpu,
    MacOSMetal,
    MacOSCpu,
    LinuxCuda,
    LinuxCpu,
}

impl LlamaPlatform {
    /// Human-readable name
    pub fn display_name(&self) -> &str {
        match self {
            Self::WindowsCuda => "Windows (CUDA/NVIDIA GPU)",
            Self::WindowsCpu => "Windows (CPU only)",
            Self::MacOSMetal => "macOS (Metal/Apple Silicon)",
            Self::MacOSCpu => "macOS (CPU only)",
            Self::LinuxCuda => "Linux (CUDA/NVIDIA GPU)",
            Self::LinuxCpu => "Linux (CPU only)",
        }
    }

    /// Release asset suffix used in llama.cpp GitHub releases
    pub fn release_asset_pattern(&self) -> &str {
        match self {
            Self::WindowsCuda => "win-cuda-cu12",
            Self::WindowsCpu => "win-avx2-x64",
            Self::MacOSMetal => "macos-arm64",
            Self::MacOSCpu => "macos-x64",
            Self::LinuxCuda => "linux-cuda-cu12",
            Self::LinuxCpu => "linux-avx2-x64",
        }
    }

    /// Binary filename on this platform
    pub fn binary_name(&self) -> &str {
        match self {
            Self::WindowsCuda | Self::WindowsCpu => "llama-server.exe",
            _ => "llama-server",
        }
    }
}

/// Detect the best platform variant for this system
pub fn detect_platform() -> LlamaPlatform {
    if cfg!(target_os = "windows") {
        if has_nvidia_gpu() {
            LlamaPlatform::WindowsCuda
        } else {
            LlamaPlatform::WindowsCpu
        }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            LlamaPlatform::MacOSMetal
        } else {
            LlamaPlatform::MacOSCpu
        }
    } else {
        // Linux
        if has_nvidia_gpu() {
            LlamaPlatform::LinuxCuda
        } else {
            LlamaPlatform::LinuxCpu
        }
    }
}

/// Check if NVIDIA GPU is available via NVML
fn has_nvidia_gpu() -> bool {
    match nvml_wrapper::Nvml::init() {
        Ok(nvml) => nvml.device_count().map(|c| c > 0).unwrap_or(false),
        Err(_) => false,
    }
}

/// Get the directory where llama.cpp binaries are stored
pub fn get_binary_dir() -> Result<PathBuf> {
    let app_data = if cfg!(target_os = "windows") {
        dirs::data_dir()
            .context("Failed to get APPDATA directory")?
            .join("Aud.io")
    } else if cfg!(target_os = "macos") {
        dirs::data_dir()
            .context("Failed to get Library directory")?
            .join("Aud.io")
    } else {
        dirs::data_dir()
            .context("Failed to get .local/share directory")?
            .join("aud.io")
    };

    let bin_dir = app_data.join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    Ok(bin_dir)
}

/// Find an existing llama-server binary.
/// Search order:
///   1. LLAMA_BIN environment variable
///   2. AppData bin directory (downloaded binary)
///   3. Next to the CLI executable
///   4. System PATH
pub fn find_llama_binary() -> Option<PathBuf> {
    // 1. Environment variable
    if let Ok(env_path) = std::env::var("LLAMA_BIN") {
        let p = PathBuf::from(&env_path);
        if p.exists() {
            info!("Using llama binary from LLAMA_BIN env: {}", p.display());
            return Some(p);
        }
    }

    // 1b. LLAMA_BIN_DIR env (set by npm launcher or install scripts)
    if let Ok(bin_dir_env) = std::env::var("LLAMA_BIN_DIR") {
        let binary_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
        let p = PathBuf::from(&bin_dir_env).join(binary_name);
        if p.exists() {
            info!("Using llama binary from LLAMA_BIN_DIR: {}", p.display());
            return Some(p);
        }
        // Also search recursively in the dir
        if let Some(found) = find_binary_in_dir(Path::new(&bin_dir_env), binary_name) {
            info!("Using llama binary from LLAMA_BIN_DIR: {}", found.display());
            return Some(found);
        }
    }

    // 2. AppData bin directory
    if let Ok(bin_dir) = get_binary_dir() {
        let binary_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
        let p = bin_dir.join(binary_name);
        if p.exists() {
            info!("Using llama binary from AppData: {}", p.display());
            return Some(p);
        }
        // Also check llama subdirectory (install script layout)
        let p = bin_dir.join("llama").join(binary_name);
        if p.exists() {
            info!("Using llama binary from AppData/llama: {}", p.display());
            return Some(p);
        }
    }

    // 3. Next to CLI executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let binary_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
            let p = exe_dir.join("bin").join(binary_name);
            if p.exists() {
                info!("Using llama binary next to executable: {}", p.display());
                return Some(p);
            }
            // Also check directly next to exe
            let p = exe_dir.join(binary_name);
            if p.exists() {
                info!("Using llama binary next to executable: {}", p.display());
                return Some(p);
            }
        }
    }

    // 4. Bundled Resources directory (for dev builds and packaged distributions)
    {
        let resource_dirs = [
            // Relative to CWD (common during dev: running from project root)
            std::path::PathBuf::from("crates/offline-intelligence/Resources/bin"),
            // Relative to exe
            std::env::current_exe().ok()
                .and_then(|e| e.parent().map(|p| p.join("Resources").join("bin")))
                .unwrap_or_default(),
            // Relative to exe parent (e.g., target/debug/../Resources)
            std::env::current_exe().ok()
                .and_then(|e| e.parent().map(|p| p.to_path_buf()))
                .map(|p| p.join("..").join("..").join("crates").join("offline-intelligence").join("Resources").join("bin"))
                .unwrap_or_default(),
        ];

        let platform_subdir = if cfg!(target_os = "windows") {
            "Windows"
        } else if cfg!(target_os = "macos") {
            "MacOS"
        } else {
            "Linux"
        };

        let binary_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };

        for base in &resource_dirs {
            let platform_dir = base.join(platform_subdir);
            if platform_dir.exists() {
                // Search recursively for the binary in platform subdirectories
                if let Some(found) = find_binary_in_dir(&platform_dir, binary_name) {
                    info!("Using bundled llama binary from Resources: {}", found.display());
                    return Some(found);
                }
            }
        }
    }

    // 5. System PATH
    let binary_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    if let Ok(output) = std::process::Command::new(if cfg!(windows) { "where" } else { "which" })
        .arg(binary_name)
        .output()
    {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path_str.is_empty() {
                let p = PathBuf::from(&path_str);
                if p.exists() {
                    info!("Using llama binary from PATH: {}", p.display());
                    return Some(p);
                }
            }
        }
    }

    None
}

/// Find an existing model file.
/// Search order:
///   1. MODEL_PATH environment variable
///   2. AppData models directory (any .gguf file)
///   3. Next to executable in resources/models/
pub fn find_model_file() -> Option<PathBuf> {
    // 1. Environment variable
    if let Ok(env_path) = std::env::var("MODEL_PATH") {
        let p = PathBuf::from(&env_path);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. AppData models directory
    if let Ok(app_data) = get_app_data_dir() {
        let models_dir = app_data.join("models");
        if let Some(model) = find_first_gguf_in_dir(&models_dir) {
            return Some(model);
        }
    }

    // 3. Next to executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let resources = exe_dir.join("resources").join("models");
            if let Some(model) = find_first_gguf_in_dir(&resources) {
                return Some(model);
            }
            let models = exe_dir.join("models");
            if let Some(model) = find_first_gguf_in_dir(&models) {
                return Some(model);
            }
        }
    }

    None
}

fn get_app_data_dir() -> Result<PathBuf> {
    let dir = if cfg!(target_os = "windows") {
        dirs::data_dir().context("No APPDATA")?.join("Aud.io")
    } else if cfg!(target_os = "macos") {
        dirs::data_dir().context("No Library dir")?.join("Aud.io")
    } else {
        dirs::data_dir().context("No data dir")?.join("aud.io")
    };
    Ok(dir)
}

/// Find the first .gguf file in a directory (recursively one level into subdirs)
fn find_first_gguf_in_dir(dir: &Path) -> Option<PathBuf> {
    if !dir.exists() { return None; }

    // Check direct children
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext.to_str().map(|s| s.eq_ignore_ascii_case("gguf")).unwrap_or(false) {
                        return Some(path);
                    }
                }
            } else if path.is_dir() {
                // Check one level deeper (model subdirectories)
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.is_file() {
                            if let Some(ext) = sub_path.extension() {
                                if ext.to_str().map(|s| s.eq_ignore_ascii_case("gguf")).unwrap_or(false) {
                                    return Some(sub_path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Get hardware info as a displayable string
pub fn get_hardware_summary() -> String {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();
    sys.refresh_all();

    let total_ram_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    let cpu_cores = num_cpus::get();
    let platform = detect_platform();

    let gpu_info = match nvml_wrapper::Nvml::init() {
        Ok(nvml) => {
            if let Ok(count) = nvml.device_count() {
                if count > 0 {
                    if let Ok(device) = nvml.device_by_index(0) {
                        let name = device.name().unwrap_or_else(|_| "Unknown GPU".to_string());
                        let vram = device.memory_info()
                            .map(|m| format!("{:.1} GB VRAM", m.total as f64 / (1024.0 * 1024.0 * 1024.0)))
                            .unwrap_or_else(|_| "unknown VRAM".to_string());
                        format!("{} ({})", name, vram)
                    } else {
                        "NVIDIA GPU (details unavailable)".to_string()
                    }
                } else {
                    "No GPU detected".to_string()
                }
            } else {
                "No GPU detected".to_string()
            }
        }
        Err(_) => {
            if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
                "Apple Silicon (Metal)".to_string()
            } else {
                "No NVIDIA GPU detected".to_string()
            }
        }
    };

    format!(
        "RAM: {:.1} GB | CPU: {} cores | GPU: {} | Platform: {}",
        total_ram_gb, cpu_cores, gpu_info, platform.display_name()
    )
}

/// Download the llama-server binary for the detected platform.
/// Returns the path to the downloaded binary.
pub async fn download_llama_binary(
    on_progress: impl Fn(u64, u64) + Send,
) -> Result<PathBuf> {
    let platform = detect_platform();
    let bin_dir = get_binary_dir()?;
    let binary_path = bin_dir.join(platform.binary_name());

    info!("Downloading llama-server for: {}", platform.display_name());

    // Get the latest release URL from llama.cpp GitHub
    let client = reqwest::Client::builder()
        .user_agent("Aud.io/0.1.1")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    // Fetch latest release info
    let release_url = "https://api.github.com/repos/ggerganov/llama.cpp/releases/latest";
    let release: serde_json::Value = client.get(release_url)
        .send()
        .await
        .context("Failed to fetch llama.cpp releases")?
        .json()
        .await
        .context("Failed to parse release info")?;

    let assets = release["assets"]
        .as_array()
        .context("No assets in release")?;

    let pattern = platform.release_asset_pattern();
    let asset = assets.iter()
        .find(|a| {
            let name = a["name"].as_str().unwrap_or("");
            name.contains(pattern) && (name.ends_with(".zip") || name.ends_with(".tar.gz"))
        })
        .context(format!("No matching release asset found for pattern: {}", pattern))?;

    let download_url = asset["browser_download_url"]
        .as_str()
        .context("No download URL in asset")?;
    let asset_name = asset["name"]
        .as_str()
        .unwrap_or("llama-release.zip");
    let asset_size = asset["size"].as_u64().unwrap_or(0);

    info!("Downloading: {} ({:.1} MB)", asset_name, asset_size as f64 / 1_000_000.0);

    // Download with progress
    let resp = client.get(download_url)
        .send()
        .await
        .context("Failed to start binary download")?;

    let total_size = resp.content_length().unwrap_or(asset_size);
    let archive_path = bin_dir.join(asset_name);

    {
        use futures_util::StreamExt;
        let mut file = tokio::fs::File::create(&archive_path).await?;
        let mut stream = resp.bytes_stream();
        let mut downloaded: u64 = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Error reading download stream")?;
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
            downloaded += chunk.len() as u64;
            on_progress(downloaded, total_size);
        }
        tokio::io::AsyncWriteExt::flush(&mut file).await?;
    }

    info!("Download complete. Extracting...");

    // Extract the archive
    extract_archive(&archive_path, &bin_dir)?;

    // Find the llama-server binary in the extracted contents
    let found_binary = find_binary_in_dir(&bin_dir, platform.binary_name());

    if let Some(found) = found_binary {
        // Move to the expected location if not already there
        if found != binary_path {
            std::fs::copy(&found, &binary_path)?;
        }

        // Set executable permission on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755))?;
        }

        // Clean up archive
        let _ = std::fs::remove_file(&archive_path);

        info!("llama-server installed at: {}", binary_path.display());
        Ok(binary_path)
    } else {
        Err(anyhow::anyhow!(
            "Could not find {} in extracted archive. Contents of {}: {:?}",
            platform.binary_name(),
            bin_dir.display(),
            std::fs::read_dir(&bin_dir)
                .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect::<Vec<_>>())
                .unwrap_or_default()
        ))
    }
}

/// Extract a .zip or .tar.gz archive
fn extract_archive(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let archive_name = archive_path.to_string_lossy().to_lowercase();

    if archive_name.ends_with(".zip") {
        let file = std::fs::File::open(archive_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        archive.extract(dest_dir)?;
    } else if archive_name.ends_with(".tar.gz") || archive_name.ends_with(".tgz") {
        let file = std::fs::File::open(archive_path)?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(gz);
        archive.unpack(dest_dir)?;
    } else {
        return Err(anyhow::anyhow!("Unsupported archive format: {}", archive_path.display()));
    }

    Ok(())
}

/// Recursively find a binary by name in a directory
fn find_binary_in_dir(dir: &Path, binary_name: &str) -> Option<PathBuf> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(binary_name)).unwrap_or(false) {
                return Some(path);
            }
            if path.is_dir() {
                if let Some(found) = find_binary_in_dir(&path, binary_name) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Check if llama-server binary needs updating
pub fn needs_update() -> bool {
    // For now, always return false. Future: check version against latest release.
    false
}
