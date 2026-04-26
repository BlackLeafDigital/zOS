# zOS Testing Guide

Three test paths, ordered by iteration speed. Use the cheapest one that exercises what you changed.

## 1. Nested winit (fastest iteration)

Command: `just dev-wm` (`Justfile:150-153`).

Runs `cargo run -p zos-wm -- --winit` from inside an existing Wayland session
(Hyprland on the daily-driver, Plasma in CI). zos-wm opens as a nested
toplevel inside the host compositor with the full input/render/IPC pipeline,
no DRM or seat required.

Sub-minute rebuild loop. Use this to verify layout, input bindings, and IPC
commands (`zos compositor windows`, `zos compositor workspaces`).

Limitation: no real DRM, no real outputs, no greetd flow. Useful for
code-correctness, not session-correctness.

### What "good" looks like

- Window opens.
- A client launches into it (`weston-terminal`, `wezterm`).
- `Super+Q` closes the focused window.
- `Super+1..9` switches workspaces.
- `Super+drag` floats a tiled window.

## 2. Full UEFI VM

Build, convert, boot:

```
just build              # AMD; or `just build-nvidia` for NVIDIA variant
just build-qcow2
just run-vm-uefi
```

Host prereqs (Fedora):

```
sudo dnf install edk2-ovmf qemu-system-x86 just podman
```

The Justfile drives `docker build` (not `podman build`) — on Linux that's
plain Docker; on macOS it's Colima. Image build still uses podman/bootc
under the hood for the OCI → bootc conversion.

What it does: builds the OCI image, loads it into containers-storage, runs
bootc-image-builder to produce `output/qcow2/disk.qcow2`, boots it under
QEMU with OVMF firmware and per-VM NVRAM at
`output/qcow2/OVMF_VARS.zos.fd`.

At the greeter (greetd → ReGreet) two sessions appear: **Hyprland zOS** and
**zOS (zos-wm)**. Pick the second to test zos-wm.

Limitations:

- virtio-vga only (no virgl), so zos-wm runs on llvmpipe — software GLES.
  Fine for layout/input/IPC sanity, useless for measuring framerate or
  reproducing NVIDIA-specific behavior.
- Shell apps (zos-panel, zos-notify, zos-power, zos-monitors) **do not
  autostart** yet. See `CHANGELOG.md` and `zos-wm/STATUS.md`.

Persistent NVRAM: `efibootmgr --bootnext` and `zos boot persistent windows`
changes survive across `just run-vm-uefi` invocations — useful for
exercising the dual-boot Part D code path. Wipe by deleting
`output/qcow2/OVMF_VARS.zos.fd`.

### What "good" looks like

- greetd appears.
- Selecting the zos-wm session reaches a desktop (decoration-less, no panel).
- `Super+Space` opens zos-launcher.

## 3. Real hardware

Primary target: RTX 4090 + AMD iGPU + 3× 1080p60.

Build once:

```
just build-nvidia
```

Rebase to the local image:

```
sudo bootc switch --enforce-container-sigpolicy \
    ostree-unverified-image:docker://localhost/zos-nvidia:latest
sudo systemctl reboot
```

Or push to ghcr.io via the Forgejo workflow and `bootc switch` the
published reference.

At the greeter pick **zOS (zos-wm)**. The session falls through to
`start-zos-wm` (`build_files/system_files/usr/bin/start-zos-wm:41`) which
`exec`s `/usr/bin/zos-wm --tty-udev`.

**Only after the panic→error guardrails ship** (Part A of the plan at
`/home/zach/.claude/plans/zos-delme-md-please-read-this-peaceful-garden.md`).
Until then, NVIDIA `SwapBuffersError::ContextLost` will crash the
compositor. Recovery: pick the **Hyprland zOS** session at the greeter.

### What "good" looks like

- Same checklist as the VM, plus:
- Rendering at native refresh rate.
- Output management via `zos compositor` CLI subcommands.

## Known broken / deferred

Cross-reference `CHANGELOG.md` (Phase 8 readiness section will be rewritten
honestly in a follow-up commit). Highlights:

- Shell-app autostart not wired.
- zos-monitors writes Hyprland config under zos-wm (silent no-op).
- Compositor IPC stub returns empty data under zos-wm (panel sees no
  workspaces).
- Lock surface render path.
- Rounded corners not rendered (compiled but not integrated).
- Tearing-control / gamma-control protocols accept and discard.

## Troubleshooting

- `just run-vm-uefi` fails with `OVMF_CODE.fd not found` →
  `sudo dnf install edk2-ovmf` on Fedora, `sudo apt install ovmf` on Debian.
- `just build` fails with podman/docker errors → on Linux verify `docker`
  works (the Justfile uses `docker build`, not `podman build`).
- VM boots but no zos-wm session in greeter → check
  `/usr/share/wayland-sessions/zos-wm.desktop` exists in the image.
- VM boots zos-wm but black screen → llvmpipe is slow on first frame; wait
  5–10s. If still black, check `journalctl -u greetd` (no `zos-wm` user unit
  exists yet).

See also: `/home/zach/.claude/plans/zos-delme-md-please-read-this-peaceful-garden.md` for the active testing/fix plan.
