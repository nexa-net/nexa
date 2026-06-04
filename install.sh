#!/bin/sh
set -eu

# Helyos Installer
# Usage: curl -sSfL https://raw.githubusercontent.com/helyos-labs/helyos/main/install.sh | sh
#
# Uninstall: curl -sSfL https://raw.githubusercontent.com/helyos-labs/helyos/main/install.sh | sh -s -- --uninstall
#
# Environment variables:
#   INSTALL_DIR   Override install directory (default: /usr/local/bin)
#   VERSION       Install a specific version (default: latest)
#   NO_SERVICE    Set to 1 to skip auto-start service installation
#   NO_START      Set to 1 to skip launching helyosd after install
#   FORCE         Set to 1 to skip upgrade prompt and always overwrite
#   UNINSTALL     Set to 1 to uninstall Helyos

# Default optional environment variables (safe under set -u)
INSTALL_DIR="${INSTALL_DIR:-}"
VERSION="${VERSION:-}"
NO_SERVICE="${NO_SERVICE:-}"
NO_START="${NO_START:-}"
FORCE="${FORCE:-}"
UNINSTALL="${UNINSTALL:-}"

GITHUB_ORG="helyos-labs"
HELYOS_HOME="${HOME}/.helyos"

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

dim() {
    printf '  \033[2m%s\033[0m\n' "$1"
}

error() {
    printf 'Error: %s\n' "$1" >&2
    exit 1
}

check_command() {
    command -v "$1" >/dev/null 2>&1 || error "'$1' is required but not found. Please install it first."
}

# Read a single character from the terminal (works even when piped via curl | sh)
ask_yes_no() {
    PROMPT="$1"
    DEFAULT="$2"

    # Non-interactive — use default
    if [ ! -t 0 ] && [ ! -e /dev/tty ]; then
        [ "$DEFAULT" = "y" ] && return 0 || return 1
    fi

    if [ "$DEFAULT" = "y" ]; then
        printf '  %s [Y/n] ' "$PROMPT"
    else
        printf '  %s [y/N] ' "$PROMPT"
    fi

    # Read from /dev/tty so it works in curl | sh
    if [ -e /dev/tty ]; then
        REPLY=$(dd bs=1 count=1 2>/dev/null < /dev/tty) || true
    else
        read -r REPLY
    fi
    printf '\n'

    case "$REPLY" in
        [yY]) return 0 ;;
        [nN]) return 1 ;;
        '')   [ "$DEFAULT" = "y" ] && return 0 || return 1 ;;
        *)    [ "$DEFAULT" = "y" ] && return 0 || return 1 ;;
    esac
}

# Ask the user to pick between numbered choices (returns via $CHOICE)
ask_choice() {
    PROMPT="$1"
    shift

    printf '\n'
    info "$PROMPT"
    printf '\n'

    I=1
    for OPT in "$@"; do
        printf '  \033[1m%d)\033[0m %s\n' "$I" "$OPT"
        I=$((I + 1))
    done
    printf '\n'
    printf '  Choice: '

    if [ -e /dev/tty ]; then
        REPLY=$(dd bs=1 count=1 2>/dev/null < /dev/tty) || true
    else
        read -r REPLY
    fi
    printf '\n'

    CHOICE="${REPLY:-1}"
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
    if [ -n "${ZSH_VERSION:-}" ] || [ "$(basename "${SHELL:-sh}")" = "zsh" ]; then
        SHELL_PROFILE="${HOME}/.zshrc"
    elif [ -n "${BASH_VERSION:-}" ] || [ "$(basename "${SHELL:-sh}")" = "bash" ]; then
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

# Get the version of an installed binary (e.g. "0.1.0")
get_installed_version() {
    BINARY_PATH="$1"
    if [ -f "$BINARY_PATH" ]; then
        "$BINARY_PATH" --version 2>/dev/null | awk '{print $NF}' || true
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

    if [ -n "$VERSION" ]; then
        VERSION_TAG="v${VERSION#v}"
    else
        VERSION_TAG=$(get_latest_version "$REPO") || {
            warn "Warning: no release found for ${REPO}, skipping"
            return 0
        }
    fi

    REMOTE_VERSION="${VERSION_TAG#v}"
    LOCAL_VERSION=$(get_installed_version "${INSTALL_DIR}/${BINARY}")

    if [ -n "$LOCAL_VERSION" ]; then
        if [ "$LOCAL_VERSION" = "$REMOTE_VERSION" ]; then
            success "${BINARY} ${LOCAL_VERSION} is already up to date"
            return 0
        fi

        # Different version — needs upgrade/downgrade
        if [ "$FORCE" = "1" ]; then
            info "Updating ${BINARY}: ${LOCAL_VERSION} -> ${REMOTE_VERSION}"
        elif [ ! -e /dev/tty ]; then
            # Non-interactive (piped) — update automatically
            info "Updating ${BINARY}: ${LOCAL_VERSION} -> ${REMOTE_VERSION}"
        else
            ask_choice "${BINARY} ${LOCAL_VERSION} is installed. Version ${REMOTE_VERSION} is available." \
                "Update to ${REMOTE_VERSION}" \
                "Reinstall ${REMOTE_VERSION} (overwrite)" \
                "Skip"

            case "$CHOICE" in
                1) info "Updating ${BINARY}..." ;;
                2) info "Reinstalling ${BINARY}..." ;;
                3) dim "Skipped ${BINARY}"; return 0 ;;
                *) dim "Skipped ${BINARY}"; return 0 ;;
            esac
        fi

        NEEDS_RESTART=1
    else
        info "Downloading ${BINARY}..."
    fi

    ARTIFACT="${BINARY}-${PLATFORM}-${ARCH}.tar.gz"
    URL="https://github.com/${GITHUB_ORG}/${REPO}/releases/download/${VERSION_TAG}/${ARTIFACT}"
    CHECKSUM_URL="https://github.com/${GITHUB_ORG}/${REPO}/releases/download/${VERSION_TAG}/sha256sums.txt"

    TMPDIR=$(mktemp -d)
    trap "rm -rf '$TMPDIR'" EXIT

    HTTP_CODE=$(curl -sSL -w '%{http_code}' -o "$TMPDIR/${ARTIFACT}" "$URL" 2>/dev/null) || true

    if [ "$HTTP_CODE" != "200" ]; then
        warn "Warning: failed to download ${BINARY} ${VERSION_TAG} (HTTP ${HTTP_CODE})"
        rm -rf "$TMPDIR"
        return 0
    fi

    # Verify SHA-256 checksum
    CHECKSUM_CODE=$(curl -sSL -w '%{http_code}' -o "$TMPDIR/sha256sums.txt" "$CHECKSUM_URL" 2>/dev/null) || true
    if [ "$CHECKSUM_CODE" = "200" ]; then
        EXPECTED=$(grep "${ARTIFACT}" "$TMPDIR/sha256sums.txt" | awk '{print $1}')
        if [ -n "$EXPECTED" ]; then
            if command -v sha256sum >/dev/null 2>&1; then
                ACTUAL=$(sha256sum "$TMPDIR/${ARTIFACT}" | awk '{print $1}')
            else
                ACTUAL=$(shasum -a 256 "$TMPDIR/${ARTIFACT}" | awk '{print $1}')
            fi
            if [ "$ACTUAL" != "$EXPECTED" ]; then
                error "Checksum verification failed for ${ARTIFACT} (expected ${EXPECTED}, got ${ACTUAL})"
            fi
            dim "Checksum verified: ${ARTIFACT}"
        else
            warn "Warning: artifact not found in sha256sums.txt, skipping verification"
        fi
    else
        warn "Warning: sha256sums.txt not available, skipping checksum verification"
    fi

    tar -xzf "$TMPDIR/${ARTIFACT}" -C "$TMPDIR" 2>/dev/null || {
        warn "Warning: failed to extract ${BINARY} archive"
        rm -rf "$TMPDIR"
        return 0
    }

    if [ -f "$TMPDIR/${BINARY}" ]; then
        install -m 755 "$TMPDIR/${BINARY}" "${INSTALL_DIR}/${BINARY}"
        if [ -n "$LOCAL_VERSION" ]; then
            success "Updated ${BINARY}: ${LOCAL_VERSION} -> ${REMOTE_VERSION}"
        else
            success "Installed ${BINARY} ${REMOTE_VERSION}"
        fi
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
    MARKER="# Helyos"

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

# ────────────────────── service management ──────────────────────

stop_helyosd_service() {
    if [ "$PLATFORM" = "darwin" ]; then
        PLIST_FILE="${HOME}/Library/LaunchAgents/net.helyos.helyosd.plist"
        if [ -f "$PLIST_FILE" ]; then
            launchctl bootout "gui/$(id -u)" "$PLIST_FILE" 2>/dev/null || true
            sleep 1
        fi
    elif command -v systemctl >/dev/null 2>&1; then
        systemctl --user stop helyosd.service 2>/dev/null || true
        sleep 1
    fi
    # Also kill any stray helyosd process
    pkill -x helyosd 2>/dev/null || true
}

install_launchd_service() {
    PLIST_DIR="${HOME}/Library/LaunchAgents"
    PLIST_FILE="${PLIST_DIR}/net.helyos.helyosd.plist"
    LOG_DIR="${HELYOS_HOME}/log"

    mkdir -p "$PLIST_DIR" "$LOG_DIR"

    cat > "$PLIST_FILE" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>net.helyos.helyosd</string>
    <key>ProgramArguments</key>
    <array>
        <string>${INSTALL_DIR}/helyosd</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>${LOG_DIR}/helyosd.log</string>
    <key>StandardErrorPath</key>
    <string>${LOG_DIR}/helyosd.err</string>
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

    launchctl bootstrap "gui/$(id -u)" "$PLIST_FILE" 2>/dev/null || \
        launchctl load "$PLIST_FILE" 2>/dev/null || true

    success "Installed launchd service (auto-starts on login)"
    info "Logs: ${LOG_DIR}/helyosd.log"
}

install_systemd_service() {
    UNIT_DIR="${HOME}/.config/systemd/user"
    UNIT_FILE="${UNIT_DIR}/helyosd.service"
    LOG_DIR="${HELYOS_HOME}/log"

    mkdir -p "$UNIT_DIR" "$LOG_DIR"

    cat > "$UNIT_FILE" <<UNIT
[Unit]
Description=Helyos daemon
After=network-online.target docker.service
Wants=network-online.target

[Service]
Type=simple
ExecStart=${INSTALL_DIR}/helyosd
Restart=on-failure
RestartSec=5
Environment=HOME=${HOME}
Environment=PATH=${INSTALL_DIR}:/usr/local/bin:/usr/bin:/bin

# Hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=${HELYOS_HOME}
PrivateTmp=true
ProtectClock=true
ProtectKernelModules=true
ProtectKernelTunables=true
ProtectControlGroups=true
RestrictSUIDSGID=true
LockPersonality=true

[Install]
WantedBy=default.target
UNIT

    systemctl --user daemon-reload 2>/dev/null || true
    systemctl --user enable helyosd.service 2>/dev/null || true

    success "Installed systemd user service (auto-starts on login)"
    info "Logs: journalctl --user -u helyosd -f"
}

install_service() {
    if [ "$NO_SERVICE" = "1" ]; then
        return 0
    fi

    if [ ! -f "${INSTALL_DIR}/helyosd" ]; then
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

# ────────────────────── start helyosd ──────────────────────

start_helyosd() {
    if [ "$NO_START" = "1" ]; then
        return 0
    fi

    if [ ! -f "${INSTALL_DIR}/helyosd" ]; then
        return 0
    fi

    info "Starting helyosd..."

    if [ "$PLATFORM" = "darwin" ]; then
        # launchd already started it via bootstrap, just verify
        sleep 2
        if curl -sf http://localhost:6443/health >/dev/null 2>&1; then
            success "helyosd is running on http://localhost:6443"
        else
            warn "helyosd may still be starting — check: helyos status"
        fi
    elif [ "$PLATFORM" = "linux" ] && command -v systemctl >/dev/null 2>&1; then
        systemctl --user start helyosd.service 2>/dev/null || true
        sleep 2
        if curl -sf http://localhost:6443/health >/dev/null 2>&1; then
            success "helyosd is running on http://localhost:6443"
        else
            warn "helyosd may still be starting — check: helyos status"
        fi
    else
        # No service manager — start in background
        nohup "${INSTALL_DIR}/helyosd" > "${HELYOS_HOME}/log/helyosd.log" 2>&1 &
        sleep 2
        if curl -sf http://localhost:6443/health >/dev/null 2>&1; then
            success "helyosd is running on http://localhost:6443 (PID $!)"
        else
            warn "helyosd may still be starting — check: helyos status"
        fi
    fi
}

# ────────────────────── uninstall ──────────────────────

uninstall() {
    printf '\n'
    info "Uninstalling Helyos..."
    printf '\n'

    # Stop running services
    stop_helyosd_service

    # Remove launchd service (macOS)
    PLIST="$HOME/Library/LaunchAgents/net.helyos.helyosd.plist"
    if [ -f "$PLIST" ]; then
        launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
        rm -f "$PLIST"
        success "Removed launchd service"
    fi

    # Remove systemd service (Linux)
    UNIT_FILE="${HOME}/.config/systemd/user/helyosd.service"
    if [ -f "$UNIT_FILE" ]; then
        systemctl --user disable helyosd.service 2>/dev/null || true
        rm -f "$UNIT_FILE"
        systemctl --user daemon-reload 2>/dev/null || true
        success "Removed systemd service"
    fi

    # Determine install directory
    if [ -z "$INSTALL_DIR" ]; then
        if [ -f "/usr/local/bin/helyosd" ]; then
            INSTALL_DIR="/usr/local/bin"
        elif [ -f "${HELYOS_HOME}/bin/helyosd" ]; then
            INSTALL_DIR="${HELYOS_HOME}/bin"
        else
            INSTALL_DIR="/usr/local/bin"
        fi
    fi

    # Remove binaries
    for bin in helyosd helyos; do
        if [ -f "${INSTALL_DIR}/${bin}" ]; then
            rm -f "${INSTALL_DIR}/${bin}"
            success "Removed ${INSTALL_DIR}/${bin}"
        fi
    done

    printf '\n'
    info "Binaries and services removed."
    info "Data directory preserved at: ${HELYOS_HOME}"
    info "To remove all data: rm -rf ${HELYOS_HOME}"
    printf '\n'
}

# ────────────────────── main ──────────────────────

main() {
    # Handle --uninstall flag
    if [ "${1:-}" = "--uninstall" ] || [ "${UNINSTALL:-}" = "1" ]; then
        detect_platform
        uninstall
        exit 0
    fi

    check_command curl
    check_command tar

    detect_platform
    detect_arch

    NEEDS_RESTART=""

    # Determine install directory
    if [ -z "$INSTALL_DIR" ]; then
        INSTALL_DIR="/usr/local/bin"
    fi

    if [ ! -w "$INSTALL_DIR" ] && [ "$(id -u)" != "0" ]; then
        INSTALL_DIR="${HELYOS_HOME}/bin"
    fi

    mkdir -p "$INSTALL_DIR" "${HELYOS_HOME}/data" "${HELYOS_HOME}/log"

    # Detect existing installation
    EXISTING_HELYOSD=$(get_installed_version "${INSTALL_DIR}/helyosd")
    EXISTING_HELYOS=$(get_installed_version "${INSTALL_DIR}/helyos")

    cat <<'BANNER'

 _   _  _____  _     __   __  ___   ____
| | | || ____|| |    \ \ / / / _ \ / ___|
| |_| ||  _|  | |     \ V / | | | |\___ \
|  _  || |___ | |___   | |  | |_| | ___) |
|_| |_||_____||_____|  |_|   \___/ |____/

BANNER

    if [ -n "$EXISTING_HELYOSD" ] || [ -n "$EXISTING_HELYOS" ]; then
        printf '  Helyos Updater\n'
    else
        printf '  Helyos Installer\n'
    fi

    printf '\n'
    info "Platform:     ${PLATFORM}/${ARCH}"
    info "Install dir:  ${INSTALL_DIR}"

    if [ -n "$EXISTING_HELYOSD" ]; then
        info "Installed:    helyosd ${EXISTING_HELYOSD}, helyos ${EXISTING_HELYOS:-n/a}"
    fi

    printf '\n'

    # Stop running helyosd before overwriting binaries (if updating)
    if [ -n "$EXISTING_HELYOSD" ]; then
        stop_helyosd_service
    fi

    download_and_install "helyosd" "helyosd"
    download_and_install "helyos-cli" "helyos"

    printf '\n'

    setup_path
    install_service
    start_helyosd

    printf '\n'
    if [ -n "$EXISTING_HELYOSD" ]; then
        success "Update complete!"
    else
        success "Installation complete!"
    fi
    printf '\n'
    info "Try it now:"
    info "  helyos status           # Check cluster status"
    info "  helyos deploy app.yaml  # Deploy a service"
    printf '\n'
    if [ "$PLATFORM" = "darwin" ]; then
        info "helyosd starts automatically on login."
        info "  Stop:    launchctl bootout gui/\$(id -u) ~/Library/LaunchAgents/net.helyos.helyosd.plist"
        info "  Restart: launchctl kickstart -k gui/\$(id -u)/net.helyos.helyosd"
    elif command -v systemctl >/dev/null 2>&1; then
        info "helyosd starts automatically on login."
        info "  Stop:    systemctl --user stop helyosd"
        info "  Restart: systemctl --user restart helyosd"
        info "  Logs:    journalctl --user -u helyosd -f"
    fi
    printf '\n'
}

main "$@"
