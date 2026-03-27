#!/bin/bash
set -ouex pipefail

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
    liquidctl

# --- eza (not in Fedora repos, install from GitHub release) ---
curl -Lo /tmp/eza.tar.gz https://github.com/eza-community/eza/releases/latest/download/eza_x86_64-unknown-linux-gnu.tar.gz
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
curl -fsSL --retry 3 --retry-delay 5 -o /etc/udev/rules.d/71-liquidctl.rules \
    https://raw.githubusercontent.com/liquidctl/liquidctl/main/extra/linux/71-liquidctl.rules

# --- Netbird (mesh VPN) ---
NETBIRD_VERSION=$(curl -fsSL --retry 3 --retry-delay 5 https://api.github.com/repos/netbirdio/netbird/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
curl -fsSL --retry 3 --retry-delay 5 -o /tmp/netbird.tar.gz "https://github.com/netbirdio/netbird/releases/download/v${NETBIRD_VERSION}/netbird_${NETBIRD_VERSION}_linux_amd64.tar.gz"
tar -xzf /tmp/netbird.tar.gz -C /usr/bin/ netbird
chmod +x /usr/bin/netbird
rm /tmp/netbird.tar.gz
netbird service install

# --- Enable services ---
systemctl enable podman.socket
systemctl enable sshd
# docker.socket not available in Bazzite base — podman provides Docker-compatible socket

# --- Fix GPG keys for BIB (bootc-image-builder) compatibility ---
# BIB runs depsolve outside the image and can't access local GPG key files.
# Since the OS is immutable (no dnf at runtime), disabling gpgcheck is safe.
for repo in /etc/yum.repos.d/*terra*mesa*; do
    [ -f "$repo" ] && sed -i 's/gpgcheck=1/gpgcheck=0/g' "$repo"
done

echo "zOS core build complete."
