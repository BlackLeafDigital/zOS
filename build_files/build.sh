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
    git-delta

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

# --- Enable services ---
systemctl enable podman.socket
# docker.socket not available in Bazzite base — podman provides Docker-compatible socket

# --- Fix GPG keys for BIB (bootc-image-builder) compatibility ---
# BIB runs depsolve outside the image and can't access local GPG key files.
# Since the OS is immutable (no dnf at runtime), disabling gpgcheck is safe.
for repo in /etc/yum.repos.d/*terra*mesa*; do
    [ -f "$repo" ] && sed -i 's/gpgcheck=1/gpgcheck=0/g' "$repo"
done

echo "zOS core build complete."
