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

# --- Container + system tools ---
# Note: buildah, skopeo, distrobox already in Bazzite
dnf5 install -y \
    podman-compose \
    podman-docker \
    tmux \
    direnv

# --- Modern CLI tools (Fedora repos) ---
# Note: duf already in Bazzite
dnf5 install -y \
    du-dust

# --- procs (ps replacement, not in Fedora 43 repos) ---
PROCS_VERSION=$(curl -fsSL https://api.github.com/repos/dalance/procs/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
curl -fsSL -o /tmp/procs.zip "https://github.com/dalance/procs/releases/latest/download/procs-v${PROCS_VERSION}-x86_64-linux.zip"
unzip -o /tmp/procs.zip -d /usr/bin/
chmod +x /usr/bin/procs
rm /tmp/procs.zip

# --- atuin (shell history search) ---
curl -fsSL -o /tmp/atuin.tar.gz https://github.com/atuinsh/atuin/releases/latest/download/atuin-x86_64-unknown-linux-gnu.tar.gz
tar -xzf /tmp/atuin.tar.gz --strip-components=1 -C /usr/bin/ --wildcards '*/atuin'
chmod +x /usr/bin/atuin
rm /tmp/atuin.tar.gz

# --- zellij (terminal multiplexer) ---
curl -fsSL -o /tmp/zellij.tar.gz https://github.com/zellij-org/zellij/releases/latest/download/zellij-x86_64-unknown-linux-musl.tar.gz
tar -xzf /tmp/zellij.tar.gz -C /usr/bin/
chmod +x /usr/bin/zellij
rm /tmp/zellij.tar.gz

# --- lazygit (git TUI) ---
LAZYGIT_VERSION=$(curl -fsSL https://api.github.com/repos/jesseduffield/lazygit/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
curl -fsSL -o /tmp/lazygit.tar.gz "https://github.com/jesseduffield/lazygit/releases/latest/download/lazygit_${LAZYGIT_VERSION}_Linux_x86_64.tar.gz"
tar -xzf /tmp/lazygit.tar.gz -C /usr/bin/ lazygit
chmod +x /usr/bin/lazygit
rm /tmp/lazygit.tar.gz

# --- lazydocker (container management TUI) ---
LAZYDOCKER_VERSION=$(curl -fsSL https://api.github.com/repos/jesseduffield/lazydocker/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
curl -fsSL -o /tmp/lazydocker.tar.gz "https://github.com/jesseduffield/lazydocker/releases/latest/download/lazydocker_${LAZYDOCKER_VERSION}_Linux_x86_64.tar.gz"
tar -xzf /tmp/lazydocker.tar.gz -C /usr/bin/ lazydocker
chmod +x /usr/bin/lazydocker
rm /tmp/lazydocker.tar.gz

# --- xh (httpie alternative) ---
XH_VERSION=$(curl -fsSL https://api.github.com/repos/ducaale/xh/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
curl -fsSL -o /tmp/xh.tar.gz "https://github.com/ducaale/xh/releases/latest/download/xh-v${XH_VERSION}-x86_64-unknown-linux-musl.tar.gz"
tar -xzf /tmp/xh.tar.gz --strip-components=1 -C /usr/bin/ "xh-v${XH_VERSION}-x86_64-unknown-linux-musl/xh"
chmod +x /usr/bin/xh
rm /tmp/xh.tar.gz

# --- doggo (DNS client) ---
DOGGO_VERSION=$(curl -fsSL https://api.github.com/repos/mr-karan/doggo/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
curl -fsSL -o /tmp/doggo.tar.gz "https://github.com/mr-karan/doggo/releases/latest/download/doggo_${DOGGO_VERSION}_Linux_x86_64.tar.gz"
tar -xzf /tmp/doggo.tar.gz -C /usr/bin/ doggo
chmod +x /usr/bin/doggo
rm /tmp/doggo.tar.gz

# --- yazi (TUI file manager) ---
curl -fsSL -o /tmp/yazi.zip "https://github.com/sxyazi/yazi/releases/latest/download/yazi-x86_64-unknown-linux-musl.zip"
unzip -o /tmp/yazi.zip -d /tmp/yazi
cp /tmp/yazi/yazi-x86_64-unknown-linux-musl/yazi /usr/bin/yazi
chmod +x /usr/bin/yazi
rm -rf /tmp/yazi /tmp/yazi.zip

# --- fx (interactive JSON explorer) ---
curl -fsSL -o /usr/bin/fx https://github.com/antonmedv/fx/releases/latest/download/fx_linux_amd64
chmod +x /usr/bin/fx

echo "Developer tools installation complete."
