# Base image - override with --build-arg for NVIDIA variant
ARG BASE_IMAGE="ghcr.io/ublue-os/bazzite:stable"

# Build context stage - scripts are mounted, not copied into final image
FROM scratch AS ctx
COPY build_files /
COPY zos-system /zos-system

FROM ${BASE_IMAGE}

### BUILD zos-system (Rust TUI)
RUN --mount=type=bind,from=ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=tmpfs,dst=/tmp \
    dnf5 install -y rust cargo && \
    cd /ctx/zos-system && \
    CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/cargo-target cargo build --release && \
    cp /tmp/cargo-target/release/zos-system /usr/bin/zos-system && \
    dnf5 remove -y rust cargo

### BUILD ReGreet (GTK4 login greeter)
RUN --mount=type=bind,from=ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=tmpfs,dst=/tmp \
    dnf5 install -y rust cargo gtk4-devel git && \
    git clone --depth 1 https://github.com/rharish101/ReGreet.git /tmp/regreet && \
    cd /tmp/regreet && \
    CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/cargo-target cargo build --release && \
    cp /tmp/cargo-target/release/regreet /usr/bin/regreet && \
    dnf5 remove -y rust cargo gtk4-devel git

### MODIFICATIONS
RUN --mount=type=bind,from=ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=cache,dst=/var/log \
    --mount=type=tmpfs,dst=/tmp \
    /ctx/build.sh && \
    /ctx/scripts/install-hyprland.sh && \
    /ctx/scripts/install-dev-tools.sh && \
    /ctx/scripts/install-user-configs.sh

### LINTING
RUN bootc container lint
