export default_tag := env("DEFAULT_TAG", "latest")
export bib_image := env("BIB_IMAGE", "quay.io/centos-bootc/bootc-image-builder:latest")

# On macOS, use the Colima zos profile (x86_64 with Rosetta)
export DOCKER_HOST := if os() == "macos" { "unix://" + env("HOME", "/tmp") + "/.colima/zos/docker.sock" } else { "" }

# Cross-platform QEMU flags
qemu_accel := if os() == "macos" { "-accel tcg,thread=multi -cpu qemu64" } else { "-M accel=kvm -cpu host" }
qemu_display := if os() == "macos" { "-display cocoa" } else { "-display gtk" }
qemu_bios := if os() == "macos" { "-bios /opt/homebrew/share/qemu/edk2-x86_64-code.fd" } else { "" }

# OVMF firmware paths for UEFI boot (Fedora-first, fallback to common locations)
ovmf_code := if os() == "macos" { "/opt/homebrew/share/qemu/edk2-x86_64-code.fd" } else { "/usr/share/edk2/ovmf/OVMF_CODE.fd" }
ovmf_vars := if os() == "macos" { "/opt/homebrew/share/qemu/edk2-i386-vars.fd" } else { "/usr/share/edk2/ovmf/OVMF_VARS.fd" }

# Build the AMD variant
build:
    docker build \
        --dns 1.1.1.1 --dns 8.8.8.8 \
        -f Containerfile \
        --build-arg BASE_IMAGE=ghcr.io/ublue-os/bazzite:stable \
        -t zos:{{default_tag}} \
        .

# Build the NVIDIA variant
build-nvidia:
    docker build \
        --dns 1.1.1.1 --dns 8.8.8.8 \
        -f Containerfile \
        --build-arg BASE_IMAGE=ghcr.io/ublue-os/bazzite-nvidia:stable \
        -t zos-nvidia:{{default_tag}} \
        .

# Build the zos-wm compositor locally with the udev+xwayland feature set (matches image build)
build-wm-local:
    cargo build --release -p zos-wm --features udev,xwayland

# Copy Docker image into podman/containers storage so BIB can access it
_load-image:
    docker run \
        --rm \
        --privileged \
        -v /var/run/docker.sock:/var/run/docker.sock \
        -v /var/lib/containers/storage:/var/lib/containers/storage \
        --entrypoint skopeo \
        {{bib_image}} \
        copy \
        docker-daemon:zos:{{default_tag}} \
        containers-storage:localhost/zos:{{default_tag}}

# Build a QCOW2 VM image
build-qcow2: build _load-image
    mkdir -p output
    docker run \
        --rm \
        -it \
        --privileged \
        -v $(pwd)/output:/output \
        -v $(pwd)/disk_config/disk.toml:/config.toml:ro \
        -v /var/lib/containers/storage:/var/lib/containers/storage \
        {{bib_image}} \
        --type qcow2 \
        --local \
        localhost/zos:{{default_tag}}

# Build an ISO installer
build-iso: build _load-image
    mkdir -p output
    docker run \
        --rm \
        -it \
        --privileged \
        -v $(pwd)/output:/output \
        -v $(pwd)/disk_config/iso-kde.toml:/config.toml:ro \
        -v /var/lib/containers/storage:/var/lib/containers/storage \
        {{bib_image}} \
        --type anaconda-iso \
        --local \
        localhost/zos:{{default_tag}}

# Run the QCOW2 in a VM (works on both Linux and macOS)
run-vm:
    @echo "Starting zOS VM..."
    qemu-system-x86_64 \
        {{qemu_accel}} \
        -smp 4 \
        -m 8G \
        -machine q35 \
        -drive file=output/qcow2/disk.qcow2,format=qcow2,if=virtio \
        {{qemu_display}} \
        -device virtio-vga \
        -nic user,model=virtio-net-pci \
        {{qemu_bios}}

# Run the QCOW2 in UEFI mode with persistent NVRAM (test dual-boot / efibootmgr changes)
run-vm-uefi:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Starting zOS UEFI VM..."
    OVMF_CODE="{{ovmf_code}}"
    OVMF_VARS_SRC="{{ovmf_vars}}"
    if [ ! -f "$OVMF_CODE" ]; then
        if [ -f /usr/share/OVMF/OVMF_CODE.fd ]; then
            OVMF_CODE=/usr/share/OVMF/OVMF_CODE.fd
            OVMF_VARS_SRC=/usr/share/OVMF/OVMF_VARS.fd
        else
            echo "ERROR: OVMF firmware not found. Install it with: sudo dnf install edk2-ovmf" >&2
            exit 1
        fi
    fi
    OVMF_VARS_VM=output/qcow2/OVMF_VARS.zos.fd
    if [ ! -f "$OVMF_VARS_VM" ]; then
        cp "$OVMF_VARS_SRC" "$OVMF_VARS_VM"
    fi
    qemu-system-x86_64 \
        {{qemu_accel}} \
        -smp 4 \
        -m 8G \
        -machine q35 \
        -drive if=pflash,format=raw,readonly=on,file=$OVMF_CODE \
        -drive if=pflash,format=raw,file=$OVMF_VARS_VM \
        -drive file=output/qcow2/disk.qcow2,format=qcow2,if=virtio \
        {{qemu_display}} \
        -device virtio-vga \
        -nic user,model=virtio-net-pci

# Create a blank disk for ISO installer testing
create-test-disk:
    mkdir -p output
    qemu-img create -f qcow2 output/test-install-disk.qcow2 40G

# Boot from ISO to test the installer
run-iso: create-test-disk
    @echo "Booting zOS installer..."
    qemu-system-x86_64 \
        {{qemu_accel}} \
        -smp 4 \
        -m 8G \
        -machine q35 \
        -drive file=output/test-install-disk.qcow2,format=qcow2,if=virtio \
        -cdrom output/bootiso/install.iso \
        -boot d \
        {{qemu_display}} \
        -device virtio-vga \
        -nic user,model=virtio-net-pci \
        {{qemu_bios}}

# Boot from disk after ISO installation completed
run-installed:
    @echo "Booting installed zOS..."
    qemu-system-x86_64 \
        {{qemu_accel}} \
        -smp 4 \
        -m 8G \
        -machine q35 \
        -drive file=output/test-install-disk.qcow2,format=qcow2,if=virtio \
        {{qemu_display}} \
        -device virtio-vga \
        -nic user,model=virtio-net-pci \
        {{qemu_bios}}

# Set up macOS dev environment (UTM already installed)
setup-mac:
    brew install qemu lima-additional-guestagents
    colima start --profile zos --arch x86_64 --cpu 4 --memory 6 --disk 100 --vm-type vz --vz-rosetta
    @echo "Done! Colima zos profile running (x86_64)."
    @echo "For UTM, create VMs manually (one-time):"
    @echo "  Emulate > x86_64 > Q35 > 8GB RAM > 4 CPUs"
    @echo "  QCOW2 VM: import output/qcow2/disk.qcow2"
    @echo "  ISO VM: blank 40G disk + mount ISO as CD-ROM"

# Run zos-settings in dev mode
dev:
    cargo run -p zos-settings

# Run zos-settings in release mode
dev-release:
    cargo run -p zos-settings --release

# Run zos CLI TUI
dev-cli:
    cargo run -p zos

# Run zos-wm compositor in nested winit mode (dev iteration)
# Filters the NVIDIA+winit EGL_BAD_SURFACE cosmetic log that fires once on startup.
dev-wm:
    #!/usr/bin/env bash
    : "${RUST_LOG:=info,smithay::backend::egl::ffi=off}"
    RUST_LOG="$RUST_LOG" cargo run -p zos-wm -- --winit

# Check all crates compile
check:
    cargo check --workspace

# Lint build scripts
lint:
    shellcheck build_files/build.sh build_files/scripts/*.sh

# Clean build artifacts
clean:
    rm -rf output _build_* _build-*
