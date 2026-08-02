#!/usr/bin/env bash
# ============================================================================
#  build.sh — Build & package the Que VS Code extension (.vsix)
#
#  This script:
#    1. Builds the que-lsp server binary (Rust, release mode)
#    2. Installs npm dependencies for the VS Code extension
#    3. Compiles TypeScript → JavaScript
#    4. Packages everything into a .vsix file ready for distribution
#
#  Usage:
#    ./build.sh              # full build (LSP server + extension + package)
#    ./build.sh --ext-only   # skip the Rust LSP build, only build & package the extension
#    ./build.sh --no-package # build everything but skip .vsix packaging
#
#  Prerequisites:
#    - Rust toolchain (cargo)
#    - Node.js + npm
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXT_DIR="$SCRIPT_DIR"
WORKSPACE_ROOT="$SCRIPT_DIR/../.."
LSP_DIR="$WORKSPACE_ROOT/lsp"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m' # No Color

info()  { echo -e "${BOLD}[INFO]${NC}  $*"; }
ok()    { echo -e "${GREEN}[OK]${NC}    $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

# ── Parse arguments ──────────────────────────────────────────────────────────

SKIP_LSP=false
SKIP_PACKAGE=false

for arg in "$@"; do
    case "$arg" in
        --ext-only)   SKIP_LSP=true ;;
        --no-package) SKIP_PACKAGE=true ;;
        -h|--help)
            echo "Usage: $0 [--ext-only] [--no-package]"
            echo ""
            echo "  --ext-only    Skip building the Rust LSP server"
            echo "  --no-package  Build everything but skip .vsix packaging"
            exit 0
            ;;
        *) error "Unknown argument: $arg"; exit 1 ;;
    esac
done

# ── Check prerequisites ─────────────────────────────────────────────────────

check_command() {
    if ! command -v "$1" &> /dev/null; then
        error "$1 is required but not found. Please install it first."
        exit 1
    fi
}

check_command node
check_command npm

if [ "$SKIP_LSP" = false ]; then
    check_command cargo
fi

# ── Step 1: Build the LSP server ────────────────────────────────────────────

if [ "$SKIP_LSP" = false ]; then
    info "Building que-lsp server (release)..."
    (cd "$WORKSPACE_ROOT" && cargo build --release -p que-lsp 2>&1)
    LSP_BINARY="$WORKSPACE_ROOT/target/release/que-lsp"
    if [ -f "$LSP_BINARY" ]; then
        ok "que-lsp binary: $LSP_BINARY"
    else
        error "que-lsp binary not found at $LSP_BINARY"
        exit 1
    fi
else
    warn "Skipping LSP server build (--ext-only)"
fi

# ── Step 2: Install npm dependencies ────────────────────────────────────────

info "Installing npm dependencies..."
(cd "$EXT_DIR" && npm install --ignore-scripts 2>&1)
ok "npm dependencies installed"

# ── Step 3: Compile TypeScript ───────────────────────────────────────────────

info "Compiling TypeScript..."
(cd "$EXT_DIR" && npm run compile 2>&1)

if [ -f "$EXT_DIR/out/extension.js" ]; then
    ok "TypeScript compiled → out/extension.js"
else
    error "Compilation failed: out/extension.js not found"
    exit 1
fi

# ── Step 4: Package the .vsix ───────────────────────────────────────────────

if [ "$SKIP_PACKAGE" = false ]; then
    info "Packaging .vsix..."
    (cd "$EXT_DIR" && npx @vscode/vsce package 2>&1)
    VSIX_FILE=$(ls -t "$EXT_DIR"/*.vsix 2>/dev/null | head -1)
    if [ -n "$VSIX_FILE" ]; then
        ok "Package created: $VSIX_FILE"
        echo ""
        echo -e "${BOLD}Install with:${NC}"
        echo "  code --install-extension $VSIX_FILE"
    else
        error "No .vsix file found after packaging"
        exit 1
    fi
else
    warn "Skipping .vsix packaging (--no-package)"
fi

echo ""
ok "Build complete!"
