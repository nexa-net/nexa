#!/bin/sh
set -e

# NexaNet Installer
# Usage: curl -sSfL https://raw.githubusercontent.com/nexa-net/nexa/main/install.sh | sh
#
# Environment variables:
#   INSTALL_DIR   Override install directory (default: /usr/local/bin)
#   VERSION       Install a specific version (default: latest)

GITHUB_ORG="nexa-net"
NEXA_HOME="${HOME}/.nexa"

# ────────────────────── helpers ──────────────────────

info() {
    printf '  %s\n' "$1"
}

error() {
    printf 'Error: %s\n' "$1" >&2
    exit 1
}

check_command() {
    command -v "$1" >/dev/null 2>&1 || error "'$1' is required but not found. Please install it first."
}

# ────────────────────── detection ──────────────────────

detect_platform() {
    PLATFORM="$(uname -s | tr '[:upper:]' '[:lower:]')"
    case "$PLATFORM" in
        linux)  PLATFORM="linux" ;;
        darwin) PLATFORM="darwin" ;;
        *)      error "unsupported platform: $PLATFORM" ;;
    esac
}

detect_arch() {
    ARCH="$(uname -m)"
    case "$ARCH" in
        x86_64|amd64)   ARCH="amd64" ;;
        aarch64|arm64)  ARCH="arm64" ;;
        *)              error "unsupported architecture: $ARCH" ;;
    esac
}

# ────────────────────── download ──────────────────────

get_latest_version() {
    REPO="$1"
    VERSION_TAG=$(curl -sSf "https://api.github.com/repos/${GITHUB_ORG}/${REPO}/releases/latest" 2>/dev/null \
        | grep '"tag_name"' \
        | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$VERSION_TAG" ]; then
        return 1
    fi
    echo "$VERSION_TAG"
}

download_and_install() {
    REPO="$1"
    BINARY="$2"

    info "Downloading ${BINARY}..."

    if [ -n "$VERSION" ]; then
        VERSION_TAG="v${VERSION#v}"
    else
        VERSION_TAG=$(get_latest_version "$REPO") || {
            info "Warning: no release found for ${REPO}, skipping"
            return 0
        }
    fi

    URL="https://github.com/${GITHUB_ORG}/${REPO}/releases/download/${VERSION_TAG}/${BINARY}-${PLATFORM}-${ARCH}.tar.gz"

    TMPDIR=$(mktemp -d)
    trap "rm -rf '$TMPDIR'" EXIT

    HTTP_CODE=$(curl -sSL -w '%{http_code}' -o "$TMPDIR/${BINARY}.tar.gz" "$URL" 2>/dev/null) || true

    if [ "$HTTP_CODE" != "200" ]; then
        info "Warning: failed to download ${BINARY} ${VERSION_TAG} (HTTP ${HTTP_CODE})"
        info "URL: ${URL}"
        rm -rf "$TMPDIR"
        return 0
    fi

    tar -xzf "$TMPDIR/${BINARY}.tar.gz" -C "$TMPDIR" 2>/dev/null || {
        info "Warning: failed to extract ${BINARY} archive"
        rm -rf "$TMPDIR"
        return 0
    }

    if [ -f "$TMPDIR/${BINARY}" ]; then
        install -m 755 "$TMPDIR/${BINARY}" "${INSTALL_DIR}/${BINARY}"
        info "Installed ${BINARY} ${VERSION_TAG} -> ${INSTALL_DIR}/${BINARY}"
    else
        info "Warning: binary '${BINARY}' not found in archive"
    fi

    rm -rf "$TMPDIR"
    trap - EXIT
}

# ────────────────────── main ──────────────────────

main() {
    check_command curl
    check_command tar

    detect_platform
    detect_arch

    # Determine install directory
    if [ -z "$INSTALL_DIR" ]; then
        INSTALL_DIR="/usr/local/bin"
    fi

    if [ ! -w "$INSTALL_DIR" ] && [ "$(id -u)" != "0" ]; then
        INSTALL_DIR="${NEXA_HOME}/bin"
        mkdir -p "$INSTALL_DIR"
    fi

    printf '\n'
    printf '  _   _                _   _      _   \n'
    printf ' | \\ | | _____  ____ | \\ | | ___| |_ \n'
    printf ' |  \\| |/ _ \\ \\/ / _\\|  \\| |/ _ \\ __|\n'
    printf ' | |\\  |  __/>  < (_|| |\\  |  __/ |_ \n'
    printf ' |_| \\_|\\___/_/\\_\\__||_| \\_|\\___|\\__|\n'
    printf '\n'
    printf '  NexaNet Installer\n'
    printf '\n'
    info "Platform:     ${PLATFORM}/${ARCH}"
    info "Install dir:  ${INSTALL_DIR}"
    printf '\n'

    if [ "$INSTALL_DIR" = "${NEXA_HOME}/bin" ]; then
        info "Note: installing to ${INSTALL_DIR} (no write access to /usr/local/bin)"
        info "Add to your shell profile:"
        info "  export PATH=\"${INSTALL_DIR}:\$PATH\""
        printf '\n'
    fi

    download_and_install "nexad" "nexad"
    download_and_install "nexa-cli" "nexa"

    printf '\n'
    info "Installation complete."
    printf '\n'
    info "Get started:"
    info "  nexad                 # Start the daemon"
    info "  nexa status           # Check cluster status"
    info "  nexa deploy app.yaml  # Deploy a service"
    printf '\n'
}

main
