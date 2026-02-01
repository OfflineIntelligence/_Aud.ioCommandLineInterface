#!/bin/bash
# Aud.io Cross-Platform Release Builder
#
# Builds the CLI for the specified target platform and packages it
# with the bundled llama.cpp binaries (NO models — users download their own).
#
# Usage:
#   ./scripts/build-release.sh                    # Build for current platform
#   ./scripts/build-release.sh windows-cuda       # Cross-compile for Windows + CUDA
#   ./scripts/build-release.sh all                # Build all platforms
#
# Output: dist/aud-<platform>.{zip,tar.gz}

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RESOURCES_BIN="$PROJECT_ROOT/crates/offline-intelligence/Resources/bin"
DIST_DIR="$PROJECT_ROOT/dist"
VERSION=$(grep '^version' "$PROJECT_ROOT/crates/offline-intelligence/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
WHITE='\033[1;37m'
DIM='\033[2m'
NC='\033[0m'

info()  { echo -e "  ${CYAN}→${NC} $1"; }
ok()    { echo -e "  ${GREEN}✓${NC} $1"; }
error() { echo -e "  ${RED}✗${NC} $1"; exit 1; }

# ── Platform Definitions ─────────────────────────────────────────────────────

build_platform() {
    local PLATFORM="$1"
    local TARGET BINARY_NAME ARCHIVE_EXT RESOURCE_SUBDIR RESOURCE_SUBPATH

    case "$PLATFORM" in
        windows-cuda)
            TARGET="x86_64-pc-windows-msvc"
            BINARY_NAME="aud.exe"
            ARCHIVE_EXT="zip"
            RESOURCE_SUBDIR="Windows"
            RESOURCE_SUBPATH="llama-b6970-bin-win-cuda-12.4-x64"
            ;;
        windows-cpu)
            TARGET="x86_64-pc-windows-msvc"
            BINARY_NAME="aud.exe"
            ARCHIVE_EXT="zip"
            RESOURCE_SUBDIR="Windows"
            RESOURCE_SUBPATH="llama-cpu"
            ;;
        linux-x64)
            TARGET="x86_64-unknown-linux-gnu"
            BINARY_NAME="aud"
            ARCHIVE_EXT="tar.gz"
            RESOURCE_SUBDIR="Linux"
            RESOURCE_SUBPATH="llama-b6970-bin-ubuntu-x64"
            ;;
        macos-arm64)
            TARGET="aarch64-apple-darwin"
            BINARY_NAME="aud"
            ARCHIVE_EXT="tar.gz"
            RESOURCE_SUBDIR="MacOS"
            RESOURCE_SUBPATH="llama-b6970-bin-macos-arm64"
            ;;
        macos-x64)
            TARGET="x86_64-apple-darwin"
            BINARY_NAME="aud"
            ARCHIVE_EXT="tar.gz"
            RESOURCE_SUBDIR="MacOS"
            RESOURCE_SUBPATH="llama-b6970-bin-macos-x64"
            ;;
        *)
            error "Unknown platform: $PLATFORM. Valid: windows-cuda, windows-cpu, linux-x64, macos-arm64, macos-x64"
            ;;
    esac

    local STAGE_DIR="$DIST_DIR/staging/aud-${PLATFORM}-v${VERSION}"
    local ARCHIVE_NAME="aud-${PLATFORM}-v${VERSION}.${ARCHIVE_EXT}"

    echo ""
    info "Building ${WHITE}${PLATFORM}${NC} (target: ${DIM}${TARGET}${NC})"

    # 1. Build the Rust binary
    info "Compiling Audio_cli → ${BINARY_NAME}..."
    cd "$PROJECT_ROOT"
    cargo build --release --bin Audio_cli --target "$TARGET" 2>&1 | tail -5

    local BUILT_BINARY="$PROJECT_ROOT/target/${TARGET}/release/Audio_cli"
    if [[ "$PLATFORM" == windows-* ]]; then
        BUILT_BINARY="${BUILT_BINARY}.exe"
    fi

    if [[ ! -f "$BUILT_BINARY" ]]; then
        error "Build failed — binary not found: $BUILT_BINARY"
    fi
    ok "Binary compiled: $(du -h "$BUILT_BINARY" | cut -f1)"

    # 2. Stage the release
    rm -rf "$STAGE_DIR"
    mkdir -p "$STAGE_DIR/bin"

    # Copy CLI binary (renamed to 'aud' or 'aud.exe')
    cp "$BUILT_BINARY" "$STAGE_DIR/${BINARY_NAME}"

    # 3. Bundle llama.cpp binaries (exclude .zip archives to save space)
    local LLAMA_SRC="$RESOURCES_BIN/$RESOURCE_SUBDIR/$RESOURCE_SUBPATH"
    if [[ -d "$LLAMA_SRC" ]]; then
        info "Bundling llama.cpp binaries from: ${DIM}${RESOURCE_SUBPATH}${NC}"

        # For Linux/macOS, binaries are in build/bin/
        if [[ -d "$LLAMA_SRC/build/bin" ]]; then
            # Copy shared libs and llama-server only (not all CLI tools)
            find "$LLAMA_SRC/build/bin" -maxdepth 1 \( -name "llama-server*" -o -name "*.so" -o -name "*.dylib" -o -name "*.metal" \) -exec cp {} "$STAGE_DIR/bin/" \;
        else
            # Windows: copy DLLs and llama-server.exe (exclude .zip archives)
            find "$LLAMA_SRC" -maxdepth 1 \( -name "llama-server*" -o -name "*.dll" \) -exec cp {} "$STAGE_DIR/bin/" \;
        fi

        # Copy LICENSE files
        find "$LLAMA_SRC" -maxdepth 2 -iname "LICENSE*" -exec cp {} "$STAGE_DIR/bin/" \; 2>/dev/null || true

        local BIN_SIZE=$(du -sh "$STAGE_DIR/bin" | cut -f1)
        ok "Bundled llama binaries: ${BIN_SIZE}"
    else
        echo -e "  ${RED}!${NC} Warning: llama.cpp binaries not found at: $LLAMA_SRC"
        echo -e "    ${DIM}The CLI will still work but users must provide their own llama-server binary.${NC}"
    fi

    # 4. Create the archive
    mkdir -p "$DIST_DIR"
    cd "$DIST_DIR/staging"

    if [[ "$ARCHIVE_EXT" == "zip" ]]; then
        (cd "$STAGE_DIR" && zip -r "$DIST_DIR/$ARCHIVE_NAME" . -x "*.zip") > /dev/null
    else
        tar -czf "$DIST_DIR/$ARCHIVE_NAME" -C "$DIST_DIR/staging" "$(basename "$STAGE_DIR")"
    fi

    local ARCHIVE_SIZE=$(du -h "$DIST_DIR/$ARCHIVE_NAME" | cut -f1)
    ok "Archive: ${WHITE}${ARCHIVE_NAME}${NC} (${ARCHIVE_SIZE})"

    # Cleanup staging
    rm -rf "$STAGE_DIR"
}

# ── Main ──────────────────────────────────────────────────────────────────────

main() {
    echo ""
    echo -e "  ${WHITE}Aud.io Release Builder${NC} v${VERSION}"
    echo -e "  ────────────────────────────────────────────"

    mkdir -p "$DIST_DIR/staging"

    local PLATFORM="${1:-auto}"

    if [[ "$PLATFORM" == "auto" ]]; then
        # Detect current platform
        case "$(uname -s)-$(uname -m)" in
            Linux-x86_64)  PLATFORM="linux-x64" ;;
            Darwin-arm64)  PLATFORM="macos-arm64" ;;
            Darwin-x86_64) PLATFORM="macos-x64" ;;
            MINGW*|MSYS*)  PLATFORM="windows-cuda" ;;
            *)             error "Cannot auto-detect platform: $(uname -s)-$(uname -m)" ;;
        esac
        info "Auto-detected platform: ${WHITE}${PLATFORM}${NC}"
    fi

    if [[ "$PLATFORM" == "all" ]]; then
        for p in windows-cuda windows-cpu linux-x64 macos-arm64 macos-x64; do
            build_platform "$p" || echo -e "  ${RED}!${NC} Skipped $p (build failed)"
        done
    else
        build_platform "$PLATFORM"
    fi

    # Cleanup staging directory
    rm -rf "$DIST_DIR/staging"

    echo ""
    echo -e "  ────────────────────────────────────────────"
    echo -e "  ${GREEN}✓${NC} Release artifacts in: ${DIM}${DIST_DIR}/${NC}"
    echo ""
    ls -lh "$DIST_DIR"/*.{zip,tar.gz} 2>/dev/null || true
    echo ""
}

main "$@"
