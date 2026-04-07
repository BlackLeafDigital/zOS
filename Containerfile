# Base image - override with --build-arg for NVIDIA variant
ARG BASE_IMAGE="ghcr.io/ublue-os/bazzite:stable"
ARG GH_TOKEN=""

# Build context stage - scripts are mounted, not copied into final image
FROM scratch AS ctx
COPY build_files /
COPY Cargo.toml /Cargo.toml
COPY Cargo.lock /Cargo.lock
COPY zos-core /zos-core
COPY zos-cli /zos-cli
COPY zos-settings /zos-settings
COPY zos-dock /zos-dock
COPY zos-daemon /zos-daemon

FROM ${BASE_IMAGE}

### BUILD Rust workspace (zos-cli + zos-settings + zos-dock + zos-daemon)
ARG GH_TOKEN
RUN --mount=type=bind,from=ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=tmpfs,dst=/tmp \
    dnf5 install -y rust cargo gtk4-devel libadwaita-devel gtk3-devel libayatana-appindicator-gtk3-devel gtk4-layer-shell-devel clang-devel git && \
    dnf5 install -y --repo=copr:copr.fedorainfracloud.org:ublue-os:bazzite-multilib pipewire-devel && \
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
    dnf5 install -y adwaita-icon-theme

### BUILD ReGreet (GTK4 login greeter)
ARG GH_TOKEN
RUN --mount=type=bind,from=ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=tmpfs,dst=/tmp \
    dnf5 install -y rust cargo gtk4-devel git && \
    export HOME=/tmp && \
    if [ -n "$GH_TOKEN" ]; then git config --global url."https://${GH_TOKEN}@github.com/".insteadOf "https://github.com/"; fi && \
    git clone --depth 1 https://github.com/rharish101/ReGreet.git /tmp/regreet && \
    cd /tmp/regreet && \
    CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/cargo-target cargo build --release && \
    cp /tmp/cargo-target/release/regreet /usr/bin/regreet

### MODIFICATIONS
ARG GH_TOKEN
RUN --mount=type=bind,from=ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=cache,dst=/var/log \
    --mount=type=tmpfs,dst=/tmp \
    export GITHUB_TOKEN="${GH_TOKEN}" && \
    /ctx/build.sh && \
    /ctx/scripts/install-hyprland.sh && \
    /ctx/scripts/install-dev-tools.sh && \
    /ctx/scripts/install-user-configs.sh

### LINTING
RUN bootc container lint
