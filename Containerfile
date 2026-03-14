# Build context stage - scripts are mounted, not copied into final image
FROM scratch AS ctx
COPY build_files /

# Base image - override with --build-arg for NVIDIA variant
ARG BASE_IMAGE="ghcr.io/ublue-os/bazzite:stable"
FROM ${BASE_IMAGE}

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
