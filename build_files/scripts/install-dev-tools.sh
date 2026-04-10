#!/bin/bash
set -ouex pipefail

# --- Robust GitHub download helpers ---
GH_CURL_OPTS=(--connect-timeout 30 --retry 5 --retry-delay 10 --retry-all-errors --max-time 300 -fsSL)
if [ -n "${GITHUB_TOKEN:-}" ]; then GH_CURL_OPTS+=(-H "Authorization: token ${GITHUB_TOKEN}"); fi

gh_curl() { curl "${GH_CURL_OPTS[@]}" "$@"; }

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
    llvm-devel \
    clang-devel \
    rust \
    cargo \
    clippy \
    rustfmt \
    openssl-devel \
    pkg-config \
    python3-pip \
    python3-devel \
    golang \
    ShellCheck \
    protobuf-compiler

# --- GUI / GTK / Wayland dev libraries ---
dnf5 install -y \
    gtk3-devel \
    gtk4-devel \
    libadwaita-devel \
    webkit2gtk4.1-devel \
    libsoup3-devel \
    javascriptcoregtk4.1-devel \
    glib2-devel \
    wayland-devel \
    libX11-devel \
    libxcb-devel \
    libinput-devel \
    vulkan-loader-devel \
    dbus-devel \
    pango-devel \
    cairo-devel \
    gdk-pixbuf2-devel \
    gtk-layer-shell-devel \
    gtk4-layer-shell-devel \
    libayatana-appindicator-gtk3-devel

# --- Multimedia dev libraries ---
# Runtime libs already in Bazzite; these are the -devel headers.
# ffmpeg/x264/x265/libfdk-aac/pipewire devel headers live in repos that are
# disabled by default on Bazzite. They're all installable when we enable the
# right repo per-transaction; no actual version conflict with Bazzite's runtime.
dnf5 install -y \
    libdav1d-devel \
    opus-devel \
    libvorbis-devel \
    lame-devel \
    pulseaudio-libs-devel \
    alsa-lib-devel

# Negativo17's fedora-multimedia repo is enabled=0 in Bazzite by default.
# --enablerepo enables it just for this transaction. Versions match Bazzite's
# installed runtime exactly (verified ffmpeg 7.1.3, x264 0.165, x265 4.1,
# libfdk-aac 2.0.3). Pulls in libav*-devel transitive deps (~14 MiB total).
dnf5 install -y --enablerepo=fedora-multimedia \
    ffmpeg-devel \
    x264-devel \
    x265-devel \
    libfdk-aac-devel

# pipewire-devel: upstream Fedora's package strict-requires a pipewire-libs
# version that Bazzite excludes (Bazzite ships its own pinned pipewire build).
# Bazzite ships their own matching pipewire-devel in the bazzite-multilib COPR
# (enabled=0 by default; --repo= overrides). Required for `cargo check -p
# zos-settings` after the audio panel rewrite added `pipewire = "0.8"`.
# See AUDIO_HANDOFF.md.
dnf5 install -y --repo=copr:copr.fedorainfracloud.org:ublue-os:bazzite-multilib pipewire-devel

# --- Android / mobile dev ---
# Note: bluez-libs-devel conflicts with Bazzite's custom bluez build
dnf5 install -y \
    android-tools \
    java-21-openjdk-devel

# --- Android SDK (system-wide cmdline-tools + minimum platforms) ---
# Lives under /usr/lib so it's read-only and shared across users. Users
# who want a writable per-user SDK can drop one at ~/Android/Sdk —
# zos-env.sh prefers $HOME over /usr/lib. Only baking one platform +
# build-tools combo to keep the image lean; gradle auto-downloads extras
# to ~/.android on first build.
mkdir -p /usr/lib/android-sdk/cmdline-tools
gh_curl -o /tmp/cmdline-tools.zip \
    https://dl.google.com/android/repository/commandlinetools-linux-14742923_latest.zip
unzip -q /tmp/cmdline-tools.zip -d /tmp/cmdline-tools-extract
mv /tmp/cmdline-tools-extract/cmdline-tools /usr/lib/android-sdk/cmdline-tools/latest
rm -rf /tmp/cmdline-tools.zip /tmp/cmdline-tools-extract

yes | /usr/lib/android-sdk/cmdline-tools/latest/bin/sdkmanager \
    --sdk_root=/usr/lib/android-sdk --licenses >/dev/null
/usr/lib/android-sdk/cmdline-tools/latest/bin/sdkmanager \
    --sdk_root=/usr/lib/android-sdk \
    "platform-tools" \
    "platforms;android-34" \
    "build-tools;34.0.0"

# --- Container + system tools ---
# Note: buildah, skopeo, distrobox already in Bazzite
dnf5 install -y \
    podman-compose \
    podman-docker \
    tmux \
    direnv \
    age

# --- Modern CLI tools (Fedora repos) ---
# Note: duf already in Bazzite
dnf5 install -y \
    du-dust

# --- procs (ps replacement, not in Fedora 43 repos) ---
PROCS_VERSION=$(gh_curl https://api.github.com/repos/dalance/procs/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
gh_curl -o /tmp/procs.zip "https://github.com/dalance/procs/releases/latest/download/procs-v${PROCS_VERSION}-x86_64-linux.zip"
unzip -o /tmp/procs.zip -d /usr/bin/
chmod +x /usr/bin/procs
rm /tmp/procs.zip

# --- atuin (shell history search) ---
gh_curl -o /tmp/atuin.tar.gz https://github.com/atuinsh/atuin/releases/latest/download/atuin-x86_64-unknown-linux-gnu.tar.gz
tar -xzf /tmp/atuin.tar.gz --strip-components=1 -C /usr/bin/ --wildcards '*/atuin'
chmod +x /usr/bin/atuin
rm /tmp/atuin.tar.gz


# --- lazygit (git TUI) ---
LAZYGIT_VERSION=$(gh_curl https://api.github.com/repos/jesseduffield/lazygit/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
gh_curl -o /tmp/lazygit.tar.gz "https://github.com/jesseduffield/lazygit/releases/latest/download/lazygit_${LAZYGIT_VERSION}_Linux_x86_64.tar.gz"
tar -xzf /tmp/lazygit.tar.gz -C /usr/bin/ lazygit
chmod +x /usr/bin/lazygit
rm /tmp/lazygit.tar.gz

# --- lazydocker (container management TUI) ---
LAZYDOCKER_VERSION=$(gh_curl https://api.github.com/repos/jesseduffield/lazydocker/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
gh_curl -o /tmp/lazydocker.tar.gz "https://github.com/jesseduffield/lazydocker/releases/latest/download/lazydocker_${LAZYDOCKER_VERSION}_Linux_x86_64.tar.gz"
tar -xzf /tmp/lazydocker.tar.gz -C /usr/bin/ lazydocker
chmod +x /usr/bin/lazydocker
rm /tmp/lazydocker.tar.gz

# --- xh (httpie alternative) ---
XH_VERSION=$(gh_curl https://api.github.com/repos/ducaale/xh/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
gh_curl -o /tmp/xh.tar.gz "https://github.com/ducaale/xh/releases/latest/download/xh-v${XH_VERSION}-x86_64-unknown-linux-musl.tar.gz"
tar -xzf /tmp/xh.tar.gz --strip-components=1 -C /usr/bin/ "xh-v${XH_VERSION}-x86_64-unknown-linux-musl/xh"
chmod +x /usr/bin/xh
rm /tmp/xh.tar.gz

# --- doggo (DNS client) ---
DOGGO_VERSION=$(gh_curl https://api.github.com/repos/mr-karan/doggo/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
gh_curl -o /tmp/doggo.tar.gz "https://github.com/mr-karan/doggo/releases/latest/download/doggo_${DOGGO_VERSION}_Linux_x86_64.tar.gz"
tar -xzf /tmp/doggo.tar.gz --strip-components=1 -C /usr/bin/ --wildcards '*/doggo'
chmod +x /usr/bin/doggo
rm /tmp/doggo.tar.gz

# --- yazi (TUI file manager) ---
gh_curl -o /tmp/yazi.zip "https://github.com/sxyazi/yazi/releases/latest/download/yazi-x86_64-unknown-linux-musl.zip"
unzip -o /tmp/yazi.zip -d /tmp/yazi
cp /tmp/yazi/yazi-x86_64-unknown-linux-musl/yazi /usr/bin/yazi
chmod +x /usr/bin/yazi
rm -rf /tmp/yazi /tmp/yazi.zip

# --- fx (interactive JSON explorer) ---
gh_curl -o /usr/bin/fx https://github.com/antonmedv/fx/releases/latest/download/fx_linux_amd64
chmod +x /usr/bin/fx

# --- sops (secrets management, not in Fedora repos) ---
SOPS_VERSION=$(gh_curl https://api.github.com/repos/getsops/sops/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
gh_curl -o /usr/bin/sops "https://github.com/getsops/sops/releases/latest/download/sops-v${SOPS_VERSION}.linux.amd64"
chmod +x /usr/bin/sops

# --- CUDA Toolkit (NVIDIA variant only) ---
if command -v nvidia-smi &>/dev/null; then
    # Replace /usr/local and /opt symlinks with real dirs — RPM cpio can't
    # unpack through symlinks, and bootc docs recommend real dirs for derivation
    # images (files become immutable, persist across updates).
    rm /usr/local && mkdir -p /usr/local
    rm /opt && mkdir -p /opt
    dnf5 config-manager addrepo --from-repofile=https://developer.download.nvidia.com/compute/cuda/repos/fedora43/x86_64/cuda-fedora43.repo
    dnf5 install -y cuda-toolkit
    cat > /etc/profile.d/cuda.sh << 'EOF'
export CUDA_HOME=/usr/local/cuda
export PATH="${CUDA_HOME}/bin:${PATH}"
EOF
fi

echo "Developer tools installation complete."
