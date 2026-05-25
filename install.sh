#!/bin/sh
set -e

# NexaNet Installer
# Usage: curl -sSfL https://raw.githubusercontent.com/nexa-net/nexa/main/install.sh | sh
#
# Environment variables:
#   INSTALL_DIR   Override install directory (default: /usr/local/bin)
#   VERSION       Install a specific version (default: latest)
#   NO_SERVICE    Set to 1 to skip auto-start service installation
#   NO_START      Set to 1 to skip launching nexad after install

GITHUB_ORG="nexa-net"
NEXA_HOME="${HOME}/.nexa"

# ────────────────────── helpers ──────────────────────

info() {
    printf '  %s\n' "$1"
}

warn() {
    printf '  \033[33m%s\033[0m\n' "$1"
}

success() {
    printf '  \033[32m%s\033[0m\n' "$1"
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

detect_shell_profile() {
    if [ -n "$ZSH_VERSION" ] || [ "$(basename "$SHELL")" = "zsh" ]; then
        SHELL_PROFILE="${HOME}/.zshrc"
    elif [ -n "$BASH_VERSION" ] || [ "$(basename "$SHELL")" = "bash" ]; then
        if [ -f "${HOME}/.bash_profile" ]; then
            SHELL_PROFILE="${HOME}/.bash_profile"
        else
            SHELL_PROFILE="${HOME}/.bashrc"
        fi
    elif [ -f "${HOME}/.profile" ]; then
        SHELL_PROFILE="${HOME}/.profile"
    else
        SHELL_PROFILE=""
    fi
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
            warn "Warning: no release found for ${REPO}, skipping"
            return 0
        }
    fi

    URL="https://github.com/${GITHUB_ORG}/${REPO}/releases/download/${VERSION_TAG}/${BINARY}-${PLATFORM}-${ARCH}.tar.gz"

    TMPDIR=$(mktemp -d)
    trap "rm -rf '$TMPDIR'" EXIT

    HTTP_CODE=$(curl -sSL -w '%{http_code}' -o "$TMPDIR/${BINARY}.tar.gz" "$URL" 2>/dev/null) || true

    if [ "$HTTP_CODE" != "200" ]; then
        warn "Warning: failed to download ${BINARY} ${VERSION_TAG} (HTTP ${HTTP_CODE})"
        rm -rf "$TMPDIR"
        return 0
    fi

    tar -xzf "$TMPDIR/${BINARY}.tar.gz" -C "$TMPDIR" 2>/dev/null || {
        warn "Warning: failed to extract ${BINARY} archive"
        rm -rf "$TMPDIR"
        return 0
    }

    if [ -f "$TMPDIR/${BINARY}" ]; then
        install -m 755 "$TMPDIR/${BINARY}" "${INSTALL_DIR}/${BINARY}"
        success "Installed ${BINARY} ${VERSION_TAG} -> ${INSTALL_DIR}/${BINARY}"
    else
        warn "Warning: binary '${BINARY}' not found in archive"
    fi

    rm -rf "$TMPDIR"
    trap - EXIT
}

# ────────────────────── PATH setup ──────────────────────

setup_path() {
    detect_shell_profile

    PATH_LINE="export PATH=\"${INSTALL_DIR}:\$PATH\""
    MARKER="# NexaNet"

    if echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
        return 0
    fi

    if [ -z "$SHELL_PROFILE" ]; then
        warn "Could not detect shell profile. Add manually:"
        warn "  ${PATH_LINE}"
        return 0
    fi

    if [ -f "$SHELL_PROFILE" ] && grep -q "$MARKER" "$SHELL_PROFILE" 2>/dev/null; then
        return 0
    fi

    printf '\n%s\n%s\n' "$MARKER" "$PATH_LINE" >> "$SHELL_PROFILE"
    info "Added PATH to ${SHELL_PROFILE}"

    export PATH="${INSTALL_DIR}:$PATH"
}

# ────────────────────── auto-start service ──────────────────────

install_launchd_service() {
    PLIST_DIR="${HOME}/Library/LaunchAgents"
    PLIST_FILE="${PLIST_DIR}/net.nexa.nexad.plist"
    LOG_DIR="${NEXA_HOME}/log"

    mkdir -p "$PLIST_DIR" "$LOG_DIR"

    cat > "$PLIST_FILE" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>net.nexa.nexad</string>
    <key>ProgramArguments</key>
    <array>
        <string>${INSTALL_DIR}/nexad</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>${LOG_DIR}/nexad.log</string>
    <key>StandardErrorPath</key>
    <string>${LOG_DIR}/nexad.err</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>${INSTALL_DIR}:/usr/local/bin:/usr/bin:/bin</string>
        <key>HOME</key>
        <string>${HOME}</string>
    </dict>
</dict>
</plist>
PLIST

    launchctl bootout "gui/$(id -u)" "$PLIST_FILE" 2>/dev/null || true
    launchctl bootstrap "gui/$(id -u)" "$PLIST_FILE" 2>/dev/null || \
        launchctl load "$PLIST_FILE" 2>/dev/null || true

    success "Installed launchd service (auto-starts on login)"
    info "Logs: ${LOG_DIR}/nexad.log"
}

install_systemd_service() {
    UNIT_DIR="${HOME}/.config/systemd/user"
    UNIT_FILE="${UNIT_DIR}/nexad.service"
    LOG_DIR="${NEXA_HOME}/log"

    mkdir -p "$UNIT_DIR" "$LOG_DIR"

    cat > "$UNIT_FILE" <<UNIT
[Unit]
Description=NexaNet daemon
After=network-online.target docker.service
Wants=network-online.target

[Service]
Type=simple
ExecStart=${INSTALL_DIR}/nexad
Restart=on-failure
RestartSec=5
Environment=HOME=${HOME}
Environment=PATH=${INSTALL_DIR}:/usr/local/bin:/usr/bin:/bin

[Install]
WantedBy=default.target
UNIT

    systemctl --user daemon-reload 2>/dev/null || true
    systemctl --user enable nexad.service 2>/dev/null || true

    success "Installed systemd user service (auto-starts on login)"
    info "Logs: journalctl --user -u nexad -f"
}

install_service() {
    if [ "$NO_SERVICE" = "1" ]; then
        return 0
    fi

    if [ ! -f "${INSTALL_DIR}/nexad" ]; then
        return 0
    fi

    info "Setting up auto-start..."

    if [ "$PLATFORM" = "darwin" ]; then
        install_launchd_service
    elif [ "$PLATFORM" = "linux" ] && command -v systemctl >/dev/null 2>&1; then
        install_systemd_service
    else
        warn "Auto-start not available on this system (no launchd or systemd)"
        return 0
    fi
}

# ────────────────────── start nexad ──────────────────────

start_nexad() {
    if [ "$NO_START" = "1" ]; then
        return 0
    fi

    if [ ! -f "${INSTALL_DIR}/nexad" ]; then
        return 0
    fi

    info "Starting nexad..."

    if [ "$PLATFORM" = "darwin" ]; then
        # launchd already started it via bootstrap, just verify
        sleep 2
        if curl -sf http://localhost:6443/health >/dev/null 2>&1; then
            success "nexad is running on http://localhost:6443"
        else
            warn "nexad may still be starting — check: nexa status"
        fi
    elif [ "$PLATFORM" = "linux" ] && command -v systemctl >/dev/null 2>&1; then
        systemctl --user start nexad.service 2>/dev/null || true
        sleep 2
        if curl -sf http://localhost:6443/health >/dev/null 2>&1; then
            success "nexad is running on http://localhost:6443"
        else
            warn "nexad may still be starting — check: nexa status"
        fi
    else
        # No service manager — start in background
        nohup "${INSTALL_DIR}/nexad" > "${NEXA_HOME}/log/nexad.log" 2>&1 &
        sleep 2
        if curl -sf http://localhost:6443/health >/dev/null 2>&1; then
            success "nexad is running on http://localhost:6443 (PID $!)"
        else
            warn "nexad may still be starting — check: nexa status"
        fi
    fi
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
    fi

    mkdir -p "$INSTALL_DIR" "${NEXA_HOME}/data" "${NEXA_HOME}/log"

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

    download_and_install "nexad" "nexad"
    download_and_install "nexa-cli" "nexa"

    printf '\n'

    setup_path
    install_service
    start_nexad

    printf '\n'
    success "Installation complete!"
    printf '\n'
    info "Try it now:"
    info "  nexa status           # Check cluster status"
    info "  nexa deploy app.yaml  # Deploy a service"
    printf '\n'
    if [ "$PLATFORM" = "darwin" ]; then
        info "nexad starts automatically on login."
        info "  Stop:    launchctl bootout gui/\$(id -u) ~/Library/LaunchAgents/net.nexa.nexad.plist"
        info "  Restart: launchctl kickstart -k gui/\$(id -u)/net.nexa.nexad"
    elif command -v systemctl >/dev/null 2>&1; then
        info "nexad starts automatically on login."
        info "  Stop:    systemctl --user stop nexad"
        info "  Restart: systemctl --user restart nexad"
        info "  Logs:    journalctl --user -u nexad -f"
    fi
    printf '\n'
}

main
