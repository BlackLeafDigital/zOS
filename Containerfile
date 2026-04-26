# Base image - override with --build-arg for NVIDIA variant
ARG BASE_IMAGE="ghcr.io/ublue-os/bazzite:stable"
ARG GH_TOKEN=""

# Rust sources context - isolated so script edits don't invalidate the Rust build layer
FROM scratch AS rust-ctx
COPY Cargo.toml /Cargo.toml
COPY Cargo.lock /Cargo.lock
COPY zos-core /zos-core
COPY zos-cli /zos-cli
COPY zos-settings /zos-settings
COPY zos-dock /zos-dock
COPY zos-daemon /zos-daemon
COPY zos-wm /zos-wm
COPY zos-ui /zos-ui
COPY zos-ui-macros /zos-ui-macros
COPY zos-panel /zos-panel
COPY zos-power /zos-power
COPY zos-monitors /zos-monitors
COPY zos-notify /zos-notify
COPY zos-launcher /zos-launcher

# Build scripts + system_files context - isolated so Rust edits don't invalidate script layers
FROM scratch AS build-ctx
COPY build_files /

FROM ${BASE_IMAGE}

### BUILD Rust workspace (zos-cli + zos-settings + zos-dock + zos-daemon + zos-wm + shell apps)
ARG GH_TOKEN
RUN --mount=type=bind,from=rust-ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=tmpfs,dst=/tmp \
    dnf5 install -y rust cargo gtk4-devel libadwaita-devel gtk3-devel libayatana-appindicator-gtk3-devel gtk4-layer-shell-devel clang-devel clang-libs git libseat-devel libdisplay-info-devel libdrm-devel libinput-devel libxkbcommon-devel systemd-devel pixman-devel wayland-devel mingw64-gcc mingw64-binutils mingw64-headers mingw64-crt && \
    dnf5 install -y --repo=copr:copr.fedorainfracloud.org:ublue-os:bazzite-multilib pipewire-devel && \
    dnf5 download mesa-libgbm-devel mesa-libEGL-devel --destdir /tmp/ && \
    rpm -Uvh --nodeps --replacepkgs /tmp/mesa-libgbm-devel*.rpm /tmp/mesa-libEGL-devel*.rpm && \
    rm -f /tmp/mesa-libgbm-devel*.rpm /tmp/mesa-libEGL-devel*.rpm && \
    export HOME=/tmp && \
    if [ -n "$GH_TOKEN" ]; then git config --global url."https://${GH_TOKEN}@github.com/".insteadOf "https://github.com/"; fi && \
    cd /ctx && \
    CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/cargo-target \
    cargo build --release -p zos -p zos-settings -p zos-dock -p zos-daemon && \
    git clone --depth 1 https://github.com/Linus789/wl-clip-persist.git /tmp/wl-clip-persist && \
    cd /tmp/wl-clip-persist && \
    CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/cargo-target cargo build --release && \
    cp /tmp/cargo-target/release/wl-clip-persist /usr/bin/wl-clip-persist && \
    cd /ctx && \
    git clone --depth 1 https://github.com/Sirulex/cursor-clip.git /tmp/cursor-clip && \
    cd /tmp/cursor-clip && \
    CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/cargo-target cargo build --release && \
    cp /tmp/cargo-target/release/cursor-clip /usr/bin/cursor-clip && \
    git clone --depth 1 https://github.com/sgtaziz/lian-li-linux.git /tmp/lian-li-linux && \
    cd /tmp/lian-li-linux && \
    CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/cargo-target cargo build --release -p lianli-daemon 2>/dev/null || true && \
    cp /tmp/cargo-target/release/lianli-daemon /usr/bin/lianli-daemon 2>/dev/null || true && \
    cd /ctx && \
    cp /tmp/cargo-target/release/zos /usr/bin/zos && \
    cp /tmp/cargo-target/release/zos-settings /usr/bin/zos-settings && \
    cp /tmp/cargo-target/release/zos-dock /usr/bin/zos-dock && \
    cp /tmp/cargo-target/release/zos-daemon /usr/bin/zos-daemon && \
    CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/cargo-target \
    cargo build --release -p zos-wm --features udev,xwayland && \
    cp /tmp/cargo-target/release/zos-wm /usr/bin/zos-wm && \
    test -x /usr/bin/zos-wm && \
    CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/cargo-target \
    cargo build --release -p zos-panel -p zos-power -p zos-monitors -p zos-notify -p zos-launcher && \
    cp /tmp/cargo-target/release/zos-panel /usr/bin/zos-panel && \
    cp /tmp/cargo-target/release/zos-power /usr/bin/zos-power && \
    cp /tmp/cargo-target/release/zos-launcher /usr/bin/zos-launcher && \
    cp /tmp/cargo-target/release/zos-monitors /usr/bin/zos-monitors && \
    cp /tmp/cargo-target/release/zos-notify /usr/bin/zos-notify && \
    dnf5 install -y adwaita-icon-theme

### BUILD ReGreet (GTK4 login greeter)
# Source cloned from upstream; no ctx mount needed
ARG GH_TOKEN
RUN --mount=type=cache,dst=/var/cache \
    --mount=type=tmpfs,dst=/tmp \
    dnf5 install -y rust cargo gtk4-devel git && \
    export HOME=/tmp && \
    if [ -n "$GH_TOKEN" ]; then git config --global url."https://${GH_TOKEN}@github.com/".insteadOf "https://github.com/"; fi && \
    git clone --depth 1 https://github.com/rharish101/ReGreet.git /tmp/regreet && \
    cd /tmp/regreet && \
    CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/cargo-target cargo build --release && \
    cp /tmp/cargo-target/release/regreet /usr/bin/regreet

### MODIFICATIONS
# Four separate RUNs -> four independent OCI layers.
# Editing a single script only invalidates its own layer (and any after it).
ARG GH_TOKEN

# Layer: core system packages, fonts, services
RUN --mount=type=bind,from=build-ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=cache,dst=/var/log \
    --mount=type=tmpfs,dst=/tmp \
    export GITHUB_TOKEN="${GH_TOKEN}" && \
    /ctx/build.sh

# Layer: Hyprland + compositor ecosystem
RUN --mount=type=bind,from=build-ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=cache,dst=/var/log \
    --mount=type=tmpfs,dst=/tmp \
    export GITHUB_TOKEN="${GH_TOKEN}" && \
    /ctx/scripts/install-hyprland.sh

# Layer: Catppuccin Mocha GRUB theme into /usr/share/grub/themes/
RUN --mount=type=bind,from=build-ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=cache,dst=/var/log \
    --mount=type=tmpfs,dst=/tmp \
    export GITHUB_TOKEN="${GH_TOKEN}" && \
    /ctx/scripts/install-grub-theme.sh

# Layer: developer toolchain (largest - compilers, -devel headers, Android SDK, CUDA)
RUN --mount=type=bind,from=build-ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=cache,dst=/var/log \
    --mount=type=tmpfs,dst=/tmp \
    export GITHUB_TOKEN="${GH_TOKEN}" && \
    /ctx/scripts/install-dev-tools.sh

# Layer: user default configs deployed to /etc/skel
RUN --mount=type=bind,from=build-ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=cache,dst=/var/log \
    --mount=type=tmpfs,dst=/tmp \
    export GITHUB_TOKEN="${GH_TOKEN}" && \
    /ctx/scripts/install-user-configs.sh

### LINTING
RUN bootc container lint
