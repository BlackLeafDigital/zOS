#!/bin/bash
set -ouex pipefail

# =============================================================================
# Developer Tools
# System-level dev tool installation
# =============================================================================

# --- Core dev tools ---
dnf5 install -y \
    gcc \
    gcc-c++ \
    make \
    cmake \
    clang \
    llvm \
    openssl-devel \
    pkg-config \
    python3-pip \
    python3-devel \
    golang \
    ShellCheck

# --- Container dev tools ---
# Note: buildah, skopeo, distrobox already in Bazzite
dnf5 install -y \
    podman-compose \
    podman-docker \
    tmux

# --- Modern CLI tools via atim COPRs ---
dnf5 copr enable -y atim/zellij
dnf5 copr enable -y atim/lazygit
dnf5 copr enable -y atim/dust
dnf5 copr enable -y atim/duf
dnf5 copr enable -y atim/procs
dnf5 copr enable -y atim/xh
dnf5 copr enable -y atim/doggo
dnf5 copr enable -y atim/yazi

dnf5 install -y \
    zellij \
    lazygit \
    dust \
    duf \
    procs \
    xh \
    doggo \
    yazi

# --- lazydocker (container management TUI) ---
LAZYDOCKER_VERSION=$(curl -fsSL https://api.github.com/repos/jesseduffield/lazydocker/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
curl -fsSL -o /tmp/lazydocker.tar.gz "https://github.com/jesseduffield/lazydocker/releases/latest/download/lazydocker_${LAZYDOCKER_VERSION}_Linux_x86_64.tar.gz"
tar -xzf /tmp/lazydocker.tar.gz -C /usr/bin/ lazydocker
chmod +x /usr/bin/lazydocker
rm /tmp/lazydocker.tar.gz

# --- fx (interactive JSON explorer) ---
curl -fsSL -o /usr/bin/fx https://github.com/antonmedv/fx/releases/latest/download/fx_linux_amd64
chmod +x /usr/bin/fx

echo "Developer tools installation complete."
