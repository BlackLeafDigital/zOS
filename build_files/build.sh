#!/bin/bash
set -ouex pipefail

# =============================================================================
# zOS Build Script
# Core system packages and configuration
# =============================================================================

# --- System Packages ---
dnf5 install -y \
    wezterm \
    starship \
    zsh \
    fish \
    fastfetch \
    bat \
    eza \
    fd-find \
    ripgrep \
    fzf \
    zoxide \
    jq \
    yq \
    htop \
    btop \
    tldr \
    git-delta \
    wl-clipboard

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
systemctl enable docker.socket

echo "zOS core build complete."
