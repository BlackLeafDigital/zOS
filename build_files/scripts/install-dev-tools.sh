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
    podman-compose

echo "Developer tools installation complete."
