#!/bin/bash
set -ouex pipefail

# --- Robust GitHub download helpers ---
GH_CURL_OPTS=(--connect-timeout 30 --retry 5 --retry-delay 10 --retry-all-errors --max-time 300 -fsSL)
if [ -n "${GITHUB_TOKEN:-}" ]; then GH_CURL_OPTS+=(-H "Authorization: token ${GITHUB_TOKEN}"); fi

gh_curl() { curl "${GH_CURL_OPTS[@]}" "$@"; }
gh_clone() {
    local url="$1"; shift
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        url="${url/https:\/\/github.com/https://${GITHUB_TOKEN}@github.com}"
    fi
    git clone --depth 1 "$url" "$@"
}

# =============================================================================
# zOS Build Script
# Core system packages and configuration
# =============================================================================

# --- COPR Repos (not in Fedora repos) ---
dnf5 copr enable -y wezfurlong/wezterm-nightly
dnf5 copr enable -y atim/starship

# --- System Packages ---
# Note: btop, fastfetch, fish, jq, fzf, wl-clipboard, ripgrep are already in Bazzite
dnf5 install -y \
    wezterm \
    starship \
    zsh \
    bat \
    fd-find \
    zoxide \
    yq \
    htop \
    tldr \
    git-delta \
    git-crypt \
    liquidctl \
    libayatana-appindicator-gtk3

# --- eza (not in Fedora repos, install from GitHub release) ---
gh_curl -o /tmp/eza.tar.gz https://github.com/eza-community/eza/releases/latest/download/eza_x86_64-unknown-linux-gnu.tar.gz
tar -xzf /tmp/eza.tar.gz -C /usr/bin/
chmod +x /usr/bin/eza
rm /tmp/eza.tar.gz

# --- Fonts ---
dnf5 install -y \
    jetbrains-mono-fonts-all \
    fira-code-fonts \
    google-noto-sans-fonts \
    google-noto-serif-fonts \
    google-noto-emoji-fonts \
    google-noto-sans-mono-fonts

# --- JetBrainsMono Nerd Font (bundled in repo, no download needed) ---
cp -r /ctx/system_files/usr/share/fonts/jetbrains-mono-nerd /usr/share/fonts/
fc-cache -f /usr/share/fonts/jetbrains-mono-nerd/

# --- Default wallpaper (Catppuccin Mocha gradient) ---
mkdir -p /usr/share/zos
magick -size 3840x2160 gradient:'#1e1e2e'-'#181825' /usr/share/zos/wallpaper.png

# --- liquidctl udev rules (Fedora RPM doesn't ship them) ---
gh_curl -o /etc/udev/rules.d/71-liquidctl.rules \
    https://raw.githubusercontent.com/liquidctl/liquidctl/main/extra/linux/71-liquidctl.rules

# --- Netbird (mesh VPN) ---
NETBIRD_VERSION=$(gh_curl https://api.github.com/repos/netbirdio/netbird/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
gh_curl -o /tmp/netbird.tar.gz "https://github.com/netbirdio/netbird/releases/download/v${NETBIRD_VERSION}/netbird_${NETBIRD_VERSION}_linux_amd64.tar.gz"
tar -xzf /tmp/netbird.tar.gz -C /usr/bin/ netbird
chmod +x /usr/bin/netbird
rm /tmp/netbird.tar.gz

# --- Netbird UI (system tray GUI) ---
gh_curl -o /tmp/netbird-ui.tar.gz "https://github.com/netbirdio/netbird/releases/download/v${NETBIRD_VERSION}/netbird-ui-linux_${NETBIRD_VERSION}_linux_amd64.tar.gz"
tar -xzf /tmp/netbird-ui.tar.gz -C /tmp/
cp /tmp/netbird-ui /usr/bin/netbird-ui
chmod +x /usr/bin/netbird-ui
rm /tmp/netbird-ui.tar.gz

# Install netbird systemd service
mkdir -p /etc/netbird
cat > /usr/lib/systemd/system/netbird.service << 'NETBIRD_SVC'
[Unit]
Description=NetBird Daemon
Documentation=https://netbird.io/docs
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/bin/netbird service run
Restart=on-failure
RestartSec=5
CacheDirectory=netbird
ConfigurationDirectory=netbird
LogsDirectory=netbird
RuntimeDirectory=netbird
StateDirectory=netbird

[Install]
WantedBy=multi-user.target
NETBIRD_SVC

# --- CoolerControl (fan/pump/AIO control GUI) ---
curl -fsSL --retry 3 --retry-delay 5 -o /usr/bin/coolercontrold \
    "https://gitlab.com/coolercontrol/coolercontrol/-/releases/4.1.0/downloads/packages/coolercontrold_4.1.0"
chmod +x /usr/bin/coolercontrold
curl -fsSL --retry 3 --retry-delay 5 -o /usr/bin/coolercontrol \
    "https://gitlab.com/coolercontrol/coolercontrol/-/releases/4.1.0/downloads/packages/coolercontrol_4.1.0"
chmod +x /usr/bin/coolercontrol

# --- CoolerControl icon ---
mkdir -p /usr/share/icons/hicolor/scalable/apps
curl -fsSL --retry 3 --retry-delay 5 -o /usr/share/icons/hicolor/scalable/apps/coolercontrol.svg \
    "https://gitlab.com/coolercontrol/coolercontrol/-/raw/main/packaging/metadata/org.coolercontrol.CoolerControl.svg" || true

cat > /usr/lib/systemd/system/coolercontrold.service << 'COOLER_SVC'
[Unit]
Description=CoolerControl Daemon
After=network.target

[Service]
Type=simple
ExecStart=/usr/bin/coolercontrold
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
COOLER_SVC

# --- Update liquidctl to latest git (NZXT Kraken 2023 Elite support) ---
LIQUIDCTL_URL="https://github.com/liquidctl/liquidctl"
if [ -n "${GITHUB_TOKEN:-}" ]; then LIQUIDCTL_URL="https://${GITHUB_TOKEN}@github.com/liquidctl/liquidctl"; fi
pip install --break-system-packages "git+${LIQUIDCTL_URL}#egg=liquidctl" || true

# --- Enable services ---
systemctl enable podman.socket
systemctl enable sshd
systemctl enable netbird
systemctl enable coolercontrold
# docker.socket not available in Bazzite base — podman provides Docker-compatible socket

# --- Fix GPG keys for BIB (bootc-image-builder) compatibility ---
# BIB runs depsolve outside the image and can't access local GPG key files.
# Since the OS is immutable (no dnf at runtime), disabling gpgcheck is safe.
for repo in /etc/yum.repos.d/*terra*mesa*; do
    [ -f "$repo" ] && sed -i 's/gpgcheck=1/gpgcheck=0/g' "$repo"
done

echo "zOS core build complete."
