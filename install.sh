#!/bin/bash
set -euo pipefail

# Riptide install script
# Usage: curl -fsSL https://github.com/fezzik-the-giant/riptide/releases/download/latest/install.sh | bash

REPO="fezzik-the-giant/riptide"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
}

# Detect OS and architecture
detect_platform() {
    local os_type=$(uname -s)
    local arch=$(uname -m)

    case "$os_type" in
        Linux)
            OS="linux"
            ;;
        Darwin)
            OS="darwin"
            ;;
        *)
            log_error "Unsupported OS: $os_type"
            exit 1
            ;;
    esac

    case "$arch" in
        x86_64)
            ARCH="x86_64"
            ;;
        aarch64|arm64)
            ARCH="aarch64"
            ;;
        *)
            log_error "Unsupported architecture: $arch"
            exit 1
            ;;
    esac

    log_info "Detected: $OS/$ARCH"
}

# Get latest release version
get_latest_version() {
    local api_url="https://api.github.com/repos/$REPO/releases/latest"

    if command -v curl &> /dev/null; then
        VERSION=$(curl -s "$api_url" | grep '"tag_name"' | head -1 | cut -d'"' -f4)
    elif command -v wget &> /dev/null; then
        VERSION=$(wget -q -O- "$api_url" | grep '"tag_name"' | head -1 | cut -d'"' -f4)
    else
        log_error "curl or wget required to download latest version"
        exit 1
    fi

    if [ -z "$VERSION" ]; then
        log_error "Failed to get latest version"
        exit 1
    fi

    log_info "Latest version: $VERSION"
}

# Determine the correct binary name
get_binary_name() {
    if [ "$OS" = "linux" ]; then
        BINARY_NAME="riptide-${VERSION}-x86_64-linux-gnu"
    elif [ "$OS" = "darwin" ]; then
        if [ "$ARCH" = "x86_64" ]; then
            BINARY_NAME="riptide-${VERSION}-x86_64-apple-darwin"
        else
            BINARY_NAME="riptide-${VERSION}-aarch64-apple-darwin"
        fi
    fi

    log_info "Binary: $BINARY_NAME"
}

# Download and extract binary
download_and_extract() {
    local download_url="https://github.com/$REPO/releases/download/$VERSION/$BINARY_NAME.tar.gz"
    local checksum_url="https://github.com/$REPO/releases/download/$VERSION/$BINARY_NAME.tar.gz.sha256"

    log_info "Downloading from: $download_url"

    local temp_dir=$(mktemp -d)
    trap "rm -rf $temp_dir" EXIT

    # Download binary
    if command -v curl &> /dev/null; then
        curl -fsSL "$download_url" -o "$temp_dir/riptide.tar.gz"
        if [ -n "${CHECKSUM_CHECK:-1}" ]; then
            curl -fsSL "$checksum_url" -o "$temp_dir/riptide.tar.gz.sha256"
        fi
    elif command -v wget &> /dev/null; then
        wget -q "$download_url" -O "$temp_dir/riptide.tar.gz"
        if [ -n "${CHECKSUM_CHECK:-1}" ]; then
            wget -q "$checksum_url" -O "$temp_dir/riptide.tar.gz.sha256"
        fi
    fi

    # Verify checksum if available
    if [ -f "$temp_dir/riptide.tar.gz.sha256" ]; then
        log_info "Verifying checksum..."
        cd "$temp_dir"
        if command -v sha256sum &> /dev/null; then
            sha256sum -c riptide.tar.gz.sha256
        elif command -v shasum &> /dev/null; then
            shasum -a 256 -c riptide.tar.gz.sha256
        else
            log_warn "sha256sum/shasum not found, skipping verification"
        fi
    fi

    # Extract binary
    log_info "Extracting..."
    tar -xzf "$temp_dir/riptide.tar.gz" -C "$temp_dir"

    # Make executable and copy to install directory
    chmod +x "$temp_dir/riptide"

    if [ "$INSTALL_DIR" = "/usr/local/bin" ] || [ "$INSTALL_DIR" = "/usr/bin" ]; then
        log_info "Installing to $INSTALL_DIR (requires sudo)"
        sudo cp "$temp_dir/riptide" "$INSTALL_DIR/riptide"
    else
        log_info "Installing to $INSTALL_DIR"
        mkdir -p "$INSTALL_DIR"
        cp "$temp_dir/riptide" "$INSTALL_DIR/riptide"
    fi
}

# Verify installation
verify_install() {
    if command -v riptide &> /dev/null; then
        local installed_version=$(riptide --version 2>/dev/null || echo "unknown")
        log_info "Riptide installed successfully!"
        log_info "Version: $installed_version"
        log_info "Location: $(command -v riptide)"
    else
        log_error "Failed to verify installation"
        log_warn "Make sure $INSTALL_DIR is in your PATH"
        exit 1
    fi
}

# Main installation flow
main() {
    log_info "Installing Riptide..."
    detect_platform
    get_latest_version
    get_binary_name
    download_and_extract
    verify_install
    log_info "Installation complete!"
}

main "$@"
