#!/bin/bash
# Aud.io CLI Installer
#
# Installs the Aud.io CLI binary + bundled llama.cpp binaries.
# NO models are included — run `aud setup` after install to choose
# offline (download a model) or online (use an API key).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/your-org/aud.io/main/install.sh | bash
#
# Environment variables:
#   AUD_VERSION  — specific version to install (default: latest)
#   AUD_DIR      — install directory (default: ~/.local/bin)

set -euo pipefail

REPO="your-org/aud.io"  # TODO: Replace with actual GitHub org/repo
VERSION="${AUD_VERSION:-latest}"
BINARY_NAME="aud"

# ── Colors ────────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; CYAN='\033[0;36m'
YELLOW='\033[1;33m'; WHITE='\033[1;37m'; DIM='\033[2m'; NC='\033[0m'

info()  { echo -e "  ${CYAN}→${NC} $1"; }
ok()    { echo -e "  ${GREEN}✓${NC} $1"; }
warn()  { echo -e "  ${YELLOW}!${NC} $1"; }
error() { echo -e "  ${RED}✗${NC} $1"; exit 1; }

# ── Detect Platform ──────────────────────────────────────────────────────────

detect_platform() {
    local OS ARCH GPU

    case "$(uname -s)" in
        Linux*)   OS="linux" ;;
        Darwin*)  OS="macos" ;;
        MINGW*|MSYS*|CYGWIN*) OS="windows" ;;
        *)        error "Unsupported OS: $(uname -s)" ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64)   ARCH="x64" ;;
        aarch64|arm64)  ARCH="arm64" ;;
        *)              error "Unsupported architecture: $(uname -m)" ;;
    esac

    # Determine platform key for release asset
    if [[ "$OS" == "windows" ]]; then
        if command -v nvidia-smi &>/dev/null; then
            echo "windows-cuda"
        else
            echo "windows-cpu"
        fi
    elif [[ "$OS" == "macos" ]]; then
        echo "macos-${ARCH}"
    else
        echo "linux-${ARCH}"
    fi
}

# ── Install ──────────────────────────────────────────────────────────────────

main() {
    echo ""
    echo -e "  ${WHITE}aud.io installer${NC}"
    echo -e "  ────────────────────────────────────────────"
    echo ""

    local PLATFORM
    PLATFORM=$(detect_platform)
    info "Detected platform: ${WHITE}${PLATFORM}${NC}"

    # Set install directory
    local INSTALL_DIR="${AUD_DIR:-${HOME}/.local/bin}"
    if [[ "$PLATFORM" == windows-* ]]; then
        INSTALL_DIR="${LOCALAPPDATA:-${HOME}/AppData/Local}/Aud.io/bin"
        BINARY_NAME="aud.exe"
    fi
    mkdir -p "$INSTALL_DIR"

    # Build asset name and download URL
    local ARCHIVE_EXT
    if [[ "$PLATFORM" == windows-* ]]; then
        ARCHIVE_EXT="zip"
    else
        ARCHIVE_EXT="tar.gz"
    fi

    local DOWNLOAD_URL
    if [[ "$VERSION" == "latest" ]]; then
        DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/aud-${PLATFORM}.${ARCHIVE_EXT}"
    else
        DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/aud-${PLATFORM}-v${VERSION}.${ARCHIVE_EXT}"
    fi

    info "Downloading from: ${DIM}${DOWNLOAD_URL}${NC}"

    # Download
    local TMPDIR
    TMPDIR=$(mktemp -d)
    local ARCHIVE="${TMPDIR}/aud-${PLATFORM}.${ARCHIVE_EXT}"

    if command -v curl &>/dev/null; then
        curl -fsSL --progress-bar "$DOWNLOAD_URL" -o "$ARCHIVE" || error "Download failed. Check your internet connection."
    elif command -v wget &>/dev/null; then
        wget -q --show-progress "$DOWNLOAD_URL" -O "$ARCHIVE" || error "Download failed."
    else
        error "Neither curl nor wget found. Please install one."
    fi

    # Extract
    info "Extracting..."
    if [[ "$ARCHIVE" == *.zip ]]; then
        unzip -q "$ARCHIVE" -d "$TMPDIR/extracted"
    else
        mkdir -p "$TMPDIR/extracted"
        tar -xzf "$ARCHIVE" -C "$TMPDIR/extracted"
    fi

    # Find and install the main binary
    local FOUND_BINARY
    FOUND_BINARY=$(find "$TMPDIR/extracted" -name "$BINARY_NAME" -type f | head -1)
    if [[ -z "$FOUND_BINARY" ]]; then
        error "Could not find $BINARY_NAME in downloaded archive."
    fi
    cp "$FOUND_BINARY" "${INSTALL_DIR}/${BINARY_NAME}"
    chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
    ok "Installed CLI: ${DIM}${INSTALL_DIR}/${BINARY_NAME}${NC}"

    # Install bundled llama binaries
    local LLAMA_DIR="${INSTALL_DIR}/llama"
    local FOUND_BIN_DIR
    FOUND_BIN_DIR=$(find "$TMPDIR/extracted" -name "bin" -type d | head -1)
    if [[ -n "$FOUND_BIN_DIR" ]] && [[ -d "$FOUND_BIN_DIR" ]]; then
        mkdir -p "$LLAMA_DIR"
        cp "$FOUND_BIN_DIR"/* "$LLAMA_DIR/" 2>/dev/null || true
        chmod +x "$LLAMA_DIR"/* 2>/dev/null || true
        ok "Installed llama.cpp binaries: ${DIM}${LLAMA_DIR}${NC}"
    fi

    # Cleanup
    rm -rf "$TMPDIR"

    # Add to PATH if needed
    if ! echo "$PATH" | tr ':' '\n' | grep -q "^${INSTALL_DIR}$"; then
        warn "${INSTALL_DIR} is not in your PATH"
        local SHELL_NAME RC_FILE
        SHELL_NAME=$(basename "${SHELL:-bash}")
        case "$SHELL_NAME" in
            bash) RC_FILE="$HOME/.bashrc" ;;
            zsh)  RC_FILE="$HOME/.zshrc" ;;
            fish) RC_FILE="$HOME/.config/fish/config.fish" ;;
            *)    RC_FILE="" ;;
        esac

        if [[ -n "$RC_FILE" ]]; then
            if [[ "$SHELL_NAME" == "fish" ]]; then
                echo "set -gx PATH \$PATH ${INSTALL_DIR}" >> "$RC_FILE"
            else
                echo "export PATH=\"\$PATH:${INSTALL_DIR}\"" >> "$RC_FILE"
            fi
            ok "Added to PATH in ${DIM}${RC_FILE}${NC}"
            warn "Run: ${WHITE}source ${RC_FILE}${NC} or open a new terminal"
        else
            warn "Add this to your shell profile:"
            echo -e "    export PATH=\"\$PATH:${INSTALL_DIR}\""
        fi
    fi

    echo ""
    echo -e "  ────────────────────────────────────────────"
    echo -e "  ${GREEN}✓${NC} Installation complete!"
    echo ""
    echo -e "  ${WHITE}What's included:${NC}"
    echo -e "    • Aud.io CLI (${BINARY_NAME})"
    echo -e "    • Bundled llama.cpp inference engine"
    echo -e "    • ${DIM}No models — you choose during setup${NC}"
    echo ""
    echo -e "  ${WHITE}Next steps:${NC}"
    echo -e "    ${CYAN}aud setup${NC}  — Choose: download a model (offline) or use an API key (online)"
    echo -e "    ${CYAN}aud code${NC}   — Start coding with AI"
    echo ""

    # Auto-run setup
    read -p "  Run setup now? [Y/n] " -n 1 -r
    echo ""
    if [[ $REPLY =~ ^[Yy]?$ ]]; then
        "${INSTALL_DIR}/${BINARY_NAME}" audio setup
    fi
}

main "$@"
