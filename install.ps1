# Aud.io CLI Installer for Windows
#
# Installs the Aud.io CLI + bundled llama.cpp binaries.
# NO models included — run `aud setup` to choose offline or online mode.
#
# Usage:
#   irm https://raw.githubusercontent.com/your-org/aud.io/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

$Repo = "your-org/aud.io"  # TODO: Replace with actual GitHub org/repo
$Version = if ($env:AUD_VERSION) { $env:AUD_VERSION } else { "latest" }
$BinaryName = "aud.exe"

function Write-Info($msg) { Write-Host "  → $msg" -ForegroundColor Cyan }
function Write-Ok($msg) { Write-Host "  ✓ $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "  ! $msg" -ForegroundColor Yellow }
function Write-Err($msg) { Write-Host "  ✗ $msg" -ForegroundColor Red; exit 1 }

Write-Host ""
Write-Host "  aud.io installer" -ForegroundColor White
Write-Host "  ────────────────────────────────────────────"
Write-Host ""

# Detect architecture
$Arch = if ([Environment]::Is64BitOperatingSystem) { "x64" } else { Write-Err "32-bit Windows is not supported" }

# Detect GPU for variant selection
$HasNvidia = $false
try {
    $null = Get-Command nvidia-smi -ErrorAction Stop
    $HasNvidia = $true
} catch {}

$PlatformKey = if ($HasNvidia) { "windows-cuda" } else { "windows-cpu" }
$GpuInfo = if ($HasNvidia) { "CUDA (NVIDIA)" } else { "CPU only" }
Write-Info "Detected: Windows / $Arch / $GpuInfo → $PlatformKey"

# Install directory
$InstallDir = Join-Path $env:LOCALAPPDATA "Aud.io\bin"
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# Download URL
$AssetName = "aud-$PlatformKey.zip"
if ($Version -eq "latest") {
    $DownloadUrl = "https://github.com/$Repo/releases/latest/download/$AssetName"
} else {
    $DownloadUrl = "https://github.com/$Repo/releases/download/$Version/aud-$PlatformKey-v$Version.zip"
}

Write-Info "Downloading: $AssetName"

# Download
$TmpDir = Join-Path $env:TEMP "aud-install-$(Get-Random)"
New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null
$Archive = Join-Path $TmpDir $AssetName

try {
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $Archive -UseBasicParsing
    $ProgressPreference = 'Continue'
} catch {
    Write-Err "Download failed: $_"
}

# Extract
Write-Info "Extracting..."
Expand-Archive -Path $Archive -DestinationPath "$TmpDir\extracted" -Force

# Find and install the CLI binary
$Found = Get-ChildItem -Path "$TmpDir\extracted" -Recurse -Filter $BinaryName | Select-Object -First 1
if ($Found) {
    Copy-Item $Found.FullName (Join-Path $InstallDir $BinaryName) -Force
    Write-Ok "Installed CLI: $InstallDir\$BinaryName"
} else {
    Write-Err "Could not find $BinaryName in downloaded archive"
}

# Install bundled llama binaries
$LlamaDir = Join-Path $InstallDir "llama"
$FoundBinDir = Get-ChildItem -Path "$TmpDir\extracted" -Recurse -Directory -Filter "bin" | Select-Object -First 1
if ($FoundBinDir) {
    if (-not (Test-Path $LlamaDir)) {
        New-Item -ItemType Directory -Path $LlamaDir -Force | Out-Null
    }
    Copy-Item "$($FoundBinDir.FullName)\*" $LlamaDir -Force -Recurse
    Write-Ok "Installed llama.cpp binaries: $LlamaDir"
}

# Cleanup
Remove-Item -Recurse -Force $TmpDir

# Add to PATH
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    Write-Ok "Added to user PATH"
    Write-Warn "Open a new terminal for PATH changes to take effect"
} else {
    Write-Ok "Already in PATH"
}

Write-Host ""
Write-Host "  ────────────────────────────────────────────"
Write-Host "  ✓ Installation complete!" -ForegroundColor Green
Write-Host ""
Write-Host "  What's included:" -ForegroundColor White
Write-Host "    • Aud.io CLI (aud.exe)"
Write-Host "    • Bundled llama.cpp inference engine"
Write-Host "    • No models — you choose during setup" -ForegroundColor DarkGray
Write-Host ""
Write-Host "  Next steps:" -ForegroundColor White
Write-Host "    aud setup  — Choose: download a model (offline) or use an API key (online)" -ForegroundColor Cyan
Write-Host "    aud code   — Start coding with AI" -ForegroundColor Cyan
Write-Host ""

# Auto-run setup
$response = Read-Host "  Run setup now? [Y/n]"
if ($response -eq "" -or $response -match "^[Yy]") {
    & (Join-Path $InstallDir $BinaryName) audio setup
}
