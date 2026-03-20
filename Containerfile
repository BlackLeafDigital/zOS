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

FROM ${BASE_IMAGE}

### BUILD Rust workspace (zos-cli + zos-settings)
RUN --mount=type=bind,from=ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=tmpfs,dst=/tmp \
    dnf5 install -y rust cargo gtk4-devel libadwaita-devel && \
    cd /ctx && \
    CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/cargo-target \
    cargo build --release -p zos -p zos-settings && \
    cp /tmp/cargo-target/release/zos /usr/bin/zos && \
    cp /tmp/cargo-target/release/zos-settings /usr/bin/zos-settings && \
    dnf5 remove -y rust cargo gtk4-devel libadwaita-devel

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
    cp /tmp/cargo-target/release/regreet /usr/bin/regreet && \
    dnf5 remove -y rust cargo gtk4-devel git

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
