export default_tag := env("DEFAULT_TAG", "latest")
export bib_image := env("BIB_IMAGE", "quay.io/centos-bootc/bootc-image-builder:latest")

# Build the AMD variant locally
build:
    podman build \
        --build-arg BASE_IMAGE=ghcr.io/ublue-os/bazzite:stable \
        -t zos:{{default_tag}} \
        .

# Build the NVIDIA variant locally
build-nvidia:
    podman build \
        --build-arg BASE_IMAGE=ghcr.io/ublue-os/bazzite-nvidia:stable \
        -t zos-nvidia:{{default_tag}} \
        .

# Build a QCOW2 VM image
build-qcow2: build
    mkdir -p output
    podman run \
        --rm \
        -it \
        --privileged \
        --pull=newer \
        --security-opt label=type:unconfined_t \
        -v $(pwd)/output:/output \
        -v $(pwd)/disk_config/disk.toml:/config.toml:ro \
        {{bib_image}} \
        --type qcow2 \
        --local \
        localhost/zos:{{default_tag}}

# Build an ISO
build-iso: build
    mkdir -p output
    podman run \
        --rm \
        -it \
        --privileged \
        --pull=newer \
        --security-opt label=type:unconfined_t \
        -v $(pwd)/output:/output \
        -v $(pwd)/disk_config/iso-kde.toml:/config.toml:ro \
        {{bib_image}} \
        --type anaconda-iso \
        --local \
        localhost/zos:{{default_tag}}

# Run the QCOW2 in a VM
run-vm:
    @echo "Starting zOS VM..."
    qemu-system-x86_64 \
        -M accel=kvm \
        -cpu host \
        -smp 4 \
        -m 8G \
        -drive file=output/qcow2/disk.qcow2,format=qcow2 \
        -display gtk \
        -device virtio-vga-gl \
        -nic user,model=virtio-net-pci

# Lint build scripts
lint:
    shellcheck build_files/build.sh build_files/scripts/*.sh

# Clean build artifacts
clean:
    rm -rf output _build_* _build-*
