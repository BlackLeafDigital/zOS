#!/usr/bin/env python3
# =============================================================================
# zos-system — CLI tool for zOS system management
# Installed to /usr/bin/zos-system
# Handles: status, config migration, GRUB/dual-boot, first-login setup, diagnostics
# =============================================================================

import argparse
import datetime
import json
import os
import shutil
import subprocess
import sys
import textwrap
from pathlib import Path


# =============================================================================
# ANSI Color Helpers
# =============================================================================

class C:
    """ANSI color codes. Automatically disabled when output is not a TTY."""
    _enabled = sys.stdout.isatty()

    RESET   = "\033[0m"
    BOLD    = "\033[1m"
    DIM     = "\033[2m"
    RED     = "\033[31m"
    GREEN   = "\033[32m"
    YELLOW  = "\033[33m"
    BLUE    = "\033[34m"
    MAGENTA = "\033[35m"
    CYAN    = "\033[36m"
    WHITE   = "\033[37m"

    @classmethod
    def disable(cls):
        for attr in ("RESET", "BOLD", "DIM", "RED", "GREEN", "YELLOW",
                      "BLUE", "MAGENTA", "CYAN", "WHITE"):
            setattr(cls, attr, "")

    @classmethod
    def init(cls):
        if not cls._enabled:
            cls.disable()


def ok(msg: str) -> str:
    return f"  {C.GREEN}[OK]{C.RESET}  {msg}"

def fail(msg: str) -> str:
    return f"  {C.RED}[FAIL]{C.RESET} {msg}"

def warn(msg: str) -> str:
    return f"  {C.YELLOW}[WARN]{C.RESET} {msg}"

def info(msg: str) -> str:
    return f"  {C.BLUE}[INFO]{C.RESET} {msg}"

def header(msg: str) -> str:
    return f"\n{C.BOLD}{C.CYAN}{msg}{C.RESET}"

def step(msg: str) -> str:
    return f"  {C.BLUE}>>>{C.RESET} {msg}"


# =============================================================================
# Constants
# =============================================================================

VERSION_FILE = Path("/usr/share/zos/version")
IMAGE_INFO_FILE = Path("/usr/share/ublue-os/image-info.json")
SKEL_DIR = Path("/etc/skel")
STATE_DIR = Path.home() / ".config" / "zos"
STATE_FILE = STATE_DIR / "state.json"
BACKUP_DIR = STATE_DIR / "backups"
SETUP_MARKER = Path.home() / ".config" / "zos-setup-done"

CONFIG_AREAS: dict[str, dict] = {
    "hypr": {
        "special": True,
        "skel_files": [".config/hypr/hyprland.conf"],
        "optional_skel_files": [
            ".config/hypr/monitors.conf",
            ".config/hypr/user-settings.conf",
            ".config/hypr/user-keybinds.conf",
            ".config/hypr/user-windowrules.conf",
        ],
        "loader_marker": "usr/share/zos/hypr",
    },
    "waybar": {
        "skel_files": [
            ".config/waybar/config.jsonc",
            ".config/waybar/style.css",
        ],
    },
    "wlogout": {
        "skel_files": [
            ".config/wlogout/layout",
            ".config/wlogout/style.css",
        ],
    },
    "zshrc": {
        "skel_files": [".zshrc"],
    },
    "gitconfig": {
        "skel_files": [".gitconfig"],
    },
}

EXPECTED_PACKAGES = [
    "waybar", "clipse", "wl-clip-persist", "hypridle", "hyprlock",
    "hyprpicker", "wlogout", "wezterm", "btop", "eza", "bat", "ripgrep",
    "fd-find", "zoxide", "starship", "delta",
]

DEPRECATED_HYPR_KEYWORDS = [
    "windowrulev2",
    "new_optimizations",
    "xray",
    "smart_split",
    "exec = mako",
    "exec-once = mako",
]


# =============================================================================
# Utility Functions
# =============================================================================

def get_system_version() -> int:
    """Read the system version from /usr/share/zos/version."""
    try:
        return int(VERSION_FILE.read_text().strip())
    except (FileNotFoundError, ValueError):
        return 0


def get_image_info() -> dict | None:
    """Read image info JSON if available."""
    try:
        return json.loads(IMAGE_INFO_FILE.read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def load_state() -> dict:
    """Load user migration state, creating defaults if missing."""
    try:
        data = json.loads(STATE_FILE.read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        data = {}
    # Ensure every area has a version entry
    for area in CONFIG_AREAS:
        if area not in data:
            data[area] = 0
    return data


def save_state(state: dict) -> None:
    """Write user migration state to disk."""
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    STATE_FILE.write_text(json.dumps(state, indent=2) + "\n")


def run_cmd(cmd: list[str], **kwargs) -> subprocess.CompletedProcess:
    """Run a command, returning the CompletedProcess. Returns rc=127 if not found."""
    try:
        return subprocess.run(cmd, capture_output=True, text=True, **kwargs)
    except FileNotFoundError:
        return subprocess.CompletedProcess(cmd, 127, stdout="", stderr=f"{cmd[0]}: not found")


def cmd_available(name: str) -> bool:
    """Check if a command is available on PATH."""
    return shutil.which(name) is not None


def today_stamp() -> str:
    return datetime.datetime.now().strftime("%Y%m%d")


def backup_file(src: Path, area: str) -> Path | None:
    """Back up a file to the zOS backup directory. Returns backup path or None."""
    if not src.exists():
        return None
    BACKUP_DIR.mkdir(parents=True, exist_ok=True)
    dest = BACKUP_DIR / f"{src.name}.{area}.{today_stamp()}"
    # Avoid clobbering existing backups from same day
    counter = 0
    final = dest
    while final.exists():
        counter += 1
        final = dest.with_suffix(f".{counter}")
    shutil.copy2(src, final)
    return final


def copy_skel_file(rel_path: str, overwrite: bool = True) -> bool:
    """Copy a file from /etc/skel/ to the user's home. Returns True if copied."""
    src = SKEL_DIR / rel_path
    dest = Path.home() / rel_path
    if not src.exists():
        return False
    if not overwrite and dest.exists():
        return False
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dest)
    return True


# =============================================================================
# Command: status
# =============================================================================

def cmd_status(_args: argparse.Namespace) -> int:
    print(header("zOS System Status"))

    # OS version
    sys_ver = get_system_version()
    if sys_ver:
        print(ok(f"System version: {C.BOLD}{sys_ver}{C.RESET}"))
    else:
        print(warn("System version file not found (/usr/share/zos/version)"))

    # Image info
    img = get_image_info()
    if img:
        print(ok(f"Image: {C.BOLD}{img.get('image-name', 'unknown')}{C.RESET}"))
        print(info(f"  Base: {img.get('base-image', 'unknown')}"))
        print(info(f"  Tag:  {img.get('image-tag', 'unknown')}"))
        print(info(f"  Built: {img.get('timestamp', 'unknown')}"))
    else:
        print(info("No image-info.json found (expected on non-ublue systems)"))

    # Migration state
    print(header("Config Migration Status"))
    state = load_state()
    for area in CONFIG_AREAS:
        user_ver = state.get(area, 0)
        if user_ver >= sys_ver and sys_ver > 0:
            status = f"{C.GREEN}up to date{C.RESET}"
        elif sys_ver > 0:
            status = f"{C.YELLOW}migration available{C.RESET} (system={sys_ver}, user={user_ver})"
        else:
            status = f"{C.DIM}unknown (no system version){C.RESET}"
        print(f"  {C.BOLD}{area:12}{C.RESET}  {status}")

    # Setup marker
    print(header("First-Login Setup"))
    if SETUP_MARKER.exists():
        print(ok("First-login setup has been completed"))
    else:
        print(warn("First-login setup has NOT run yet (run: zos-system setup)"))

    print()
    return 0


# =============================================================================
# Command: migrate
# =============================================================================

def cmd_migrate(args: argparse.Namespace) -> int:
    apply = args.apply or args.auto
    silent = args.auto
    dry_run = not apply

    sys_ver = get_system_version()
    if sys_ver == 0:
        msg = "Cannot migrate: system version file not found"
        if silent:
            print(msg, file=sys.stderr)
        else:
            print(fail(msg))
        return 1

    state = load_state()
    actions: list[str] = []
    errors: list[str] = []

    if not silent and dry_run:
        print(header("Migration Dry Run"))
        print(info(f"System version: {sys_ver}"))
        print(info("Use --apply to execute, or --auto for silent mode"))
        print()

    for area, cfg in CONFIG_AREAS.items():
        user_ver = state.get(area, 0)
        if sys_ver <= user_ver:
            if not silent and not dry_run:
                print(ok(f"{area}: already up to date (v{user_ver})"))
            continue

        is_special = cfg.get("special", False)

        if is_special and area == "hypr":
            _migrate_hypr(cfg, dry_run, silent, actions, errors)
        else:
            _migrate_standard(area, cfg, dry_run, silent, actions, errors)

        if not dry_run and area not in [a.split(":")[0] for a in errors]:
            state[area] = sys_ver

    # Persist state
    if not dry_run:
        save_state(state)

    # Summary
    if dry_run and not silent:
        if not actions:
            print(info("Everything is up to date. Nothing to migrate."))
        print()
        return 0

    if silent:
        # Auto mode: only output on error
        if errors:
            for e in errors:
                print(e, file=sys.stderr)
            return 1
        if actions:
            # Send desktop notification
            try:
                subprocess.run(
                    ["notify-send", "zOS Updated",
                     "System configs have been updated. Press SUPER+F1 for keybindings."],
                    check=False, capture_output=True,
                )
            except FileNotFoundError:
                pass
        return 0

    # Interactive apply summary
    if errors:
        print()
        for e in errors:
            print(fail(e))
    if actions:
        print()
        print(header("Migration Complete"))
        for a in actions:
            print(ok(a))
    else:
        print(info("Everything is up to date. Nothing to migrate."))
    print()
    return 1 if errors else 0


def _migrate_hypr(
    cfg: dict,
    dry_run: bool,
    silent: bool,
    actions: list[str],
    errors: list[str],
) -> None:
    """Handle the special Hyprland migration (thin loader pattern)."""
    area = "hypr"
    main_conf = Path.home() / ".config/hypr/hyprland.conf"
    loader_marker = cfg.get("loader_marker", "")

    # Check if existing config is the old monolithic style
    is_monolithic = False
    if main_conf.exists():
        try:
            content = main_conf.read_text()
            if loader_marker and loader_marker not in content:
                is_monolithic = True
        except OSError:
            pass

    if dry_run:
        if is_monolithic:
            print(warn(f"hypr: would back up monolithic hyprland.conf"))
            print(step(f"hypr: would replace with thin loader from /etc/skel/"))
        elif not main_conf.exists():
            print(step(f"hypr: would install hyprland.conf from /etc/skel/"))
        else:
            print(step(f"hypr: would overwrite hyprland.conf with latest from /etc/skel/"))

        for rel in cfg.get("optional_skel_files", []):
            dest = Path.home() / rel
            if not dest.exists():
                print(step(f"hypr: would create {dest.name} from /etc/skel/"))
            else:
                print(info(f"hypr: {dest.name} exists, would skip"))
        actions.append("hypr: dry run complete")
        return

    # Back up monolithic config
    if is_monolithic:
        bp = backup_file(main_conf, area)
        if bp and not silent:
            print(info(f"hypr: backed up monolithic config to {bp.name}"))

    # Copy the main config (thin loader)
    for rel in cfg.get("skel_files", []):
        if copy_skel_file(rel, overwrite=True):
            actions.append(f"hypr: installed {Path(rel).name}")
        else:
            errors.append(f"hypr: failed to copy {rel} from /etc/skel/")

    # Optional files: create only if missing
    for rel in cfg.get("optional_skel_files", []):
        if copy_skel_file(rel, overwrite=False):
            actions.append(f"hypr: created {Path(rel).name} (new)")


def _migrate_standard(
    area: str,
    cfg: dict,
    dry_run: bool,
    silent: bool,
    actions: list[str],
    errors: list[str],
) -> None:
    """Migrate a standard config area: back up existing, copy from skel."""
    for rel in cfg.get("skel_files", []):
        dest = Path.home() / rel
        if dry_run:
            if dest.exists():
                print(step(f"{area}: would back up and replace {dest.name}"))
            else:
                print(step(f"{area}: would install {dest.name}"))
            actions.append(f"{area}: dry run complete")
            continue

        # Back up existing
        if dest.exists():
            bp = backup_file(dest, area)
            if bp and not silent:
                print(info(f"{area}: backed up {dest.name} to {bp.name}"))

        if copy_skel_file(rel, overwrite=True):
            actions.append(f"{area}: installed {Path(rel).name}")
        else:
            errors.append(f"{area}: failed to copy {rel} from /etc/skel/")


# =============================================================================
# Command: grub
# =============================================================================

def cmd_grub(_args: argparse.Namespace) -> int:
    if os.geteuid() != 0:
        print(fail("zos-system grub must be run as root (sudo zos-system grub)"))
        return 1

    print(header("zOS GRUB / Dual-Boot Setup"))
    changes: list[str] = []

    # --- GRUB timeout ---
    grub_cfg = Path("/boot/grub2/user.cfg")
    try:
        if grub_cfg.exists():
            content = grub_cfg.read_text()
            lines = content.splitlines()
            found = False
            for i, line in enumerate(lines):
                if line.startswith("set timeout="):
                    lines[i] = "set timeout=15"
                    found = True
                    break
            if not found:
                lines.append("set timeout=15")
            grub_cfg.write_text("\n".join(lines) + "\n")
        else:
            grub_cfg.parent.mkdir(parents=True, exist_ok=True)
            grub_cfg.write_text("set timeout=15\n")
        changes.append("GRUB timeout set to 15 seconds")
    except OSError as e:
        print(fail(f"Failed to configure GRUB: {e}"))
        return 1

    # --- Windows detection ---
    windows_bls = Path("/boot/loader/entries/windows.conf")

    if windows_bls.exists():
        changes.append("Windows boot entry already exists (skipped)")
    else:
        windows_efi = _find_windows_efi()
        if windows_efi:
            try:
                windows_bls.parent.mkdir(parents=True, exist_ok=True)
                windows_bls.write_text(f"title Windows\nefi {windows_efi}\n")
                changes.append("Windows boot entry added")
            except OSError as e:
                print(fail(f"Failed to create Windows boot entry: {e}"))
                changes.append(f"Failed to create Windows boot entry: {e}")
        else:
            changes.append("Windows not detected on any EFI partition (skipped)")

    # --- Summary ---
    print(header("Setup Complete"))
    for change in changes:
        print(ok(change))
    print()
    print(info("Reboot to apply GRUB changes."))
    print()
    return 0


def _find_windows_efi() -> str | None:
    """Scan EFI partitions for the Windows bootloader."""
    # Check current ESP first
    for esp_mount in ("/boot/efi", "/efi"):
        bootmgr = Path(esp_mount) / "EFI/Microsoft/Boot/bootmgfw.efi"
        if Path(esp_mount).is_mount() and bootmgr.exists():
            return "/EFI/Microsoft/Boot/bootmgfw.efi"

    # Scan other EFI partitions via lsblk
    EFI_GUID = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
    result = run_cmd(["lsblk", "-rno", "PATH,PARTTYPE"])
    if result.returncode != 0:
        return None

    import tempfile
    for line in result.stdout.splitlines():
        parts = line.split()
        if len(parts) < 2:
            continue
        dev_path, parttype = parts[0], parts[1]
        if parttype.lower() != EFI_GUID:
            continue

        tmpdir = tempfile.mkdtemp()
        try:
            mount_result = run_cmd(["mount", "-o", "ro", dev_path, tmpdir])
            if mount_result.returncode != 0:
                continue
            try:
                bootmgr = Path(tmpdir) / "EFI/Microsoft/Boot/bootmgfw.efi"
                if bootmgr.exists():
                    return "/EFI/Microsoft/Boot/bootmgfw.efi"
            finally:
                run_cmd(["umount", tmpdir])
        finally:
            try:
                os.rmdir(tmpdir)
            except OSError:
                pass

    return None


# =============================================================================
# Command: setup
# =============================================================================

def cmd_setup(_args: argparse.Namespace) -> int:
    if SETUP_MARKER.exists():
        print(ok("First-login setup has already been completed."))
        print(info(f"Marker: {SETUP_MARKER}"))
        print(info("To re-run, delete the marker file and run again."))
        return 0

    print(header("zOS First-Login Setup"))
    print()

    all_ok = True

    # --- Homebrew ---
    all_ok &= _setup_step(
        "Installing Homebrew",
        check_cmd="brew",
        install_cmds=[
            ["bash", "-c",
             "curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh | /bin/bash -s -- </dev/null"],
        ],
    )
    # Source brew for subsequent steps
    _source_brew()

    # --- mise ---
    all_ok &= _setup_step(
        "Installing mise (runtime version manager)",
        check_cmd="mise",
        install_cmds=[["brew", "install", "mise"]],
    )

    # --- Node LTS + Python via mise ---
    all_ok &= _setup_step(
        "Setting up Node.js LTS via mise",
        install_cmds=[["mise", "use", "--global", "node@lts"]],
    )
    all_ok &= _setup_step(
        "Setting up Python via mise",
        install_cmds=[["mise", "use", "--global", "python@latest"]],
    )

    # --- pnpm ---
    all_ok &= _setup_step(
        "Installing pnpm",
        install_cmds=[["npm", "install", "-g", "pnpm"]],
    )

    # --- uv ---
    all_ok &= _setup_step(
        "Installing uv (Python package manager)",
        install_cmds=[["brew", "install", "uv"]],
    )

    # --- Rust ---
    all_ok &= _setup_step(
        "Installing Rust toolchain",
        check_cmd="rustup",
        install_cmds=[
            ["bash", "-c",
             "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"],
        ],
    )

    # --- GitHub CLI ---
    all_ok &= _setup_step(
        "Installing GitHub CLI",
        check_cmd="gh",
        install_cmds=[["brew", "install", "gh"]],
    )

    # --- Set zsh as default shell ---
    current_shell = os.environ.get("SHELL", "")
    zsh_path = shutil.which("zsh")
    if zsh_path and current_shell != zsh_path:
        all_ok &= _setup_step(
            "Setting zsh as default shell",
            install_cmds=[["chsh", "-s", zsh_path]],
        )
    else:
        print(ok("zsh is already the default shell"))

    # --- Nerd Font ---
    font_dir = Path.home() / ".local/share/fonts"
    font_file = font_dir / "JetBrainsMonoNerdFont-Regular.ttf"
    if not font_file.exists():
        font_dir.mkdir(parents=True, exist_ok=True)
        font_url = "https://github.com/ryanoasis/nerd-fonts/releases/latest/download/JetBrainsMono.tar.xz"
        all_ok &= _setup_step(
            "Installing JetBrainsMono Nerd Font",
            install_cmds=[
                ["bash", "-c",
                 f"curl -fsSL '{font_url}' | tar -xJ -C '{font_dir}/'"],
                ["fc-cache", "-fv", str(font_dir)],
            ],
        )
    else:
        print(ok("JetBrainsMono Nerd Font already installed"))

    # --- Mark setup as done ---
    SETUP_MARKER.parent.mkdir(parents=True, exist_ok=True)
    SETUP_MARKER.touch()

    print()
    if all_ok:
        print(header("Setup Complete!"))
        print(info("Restart your terminal to pick up all changes."))
    else:
        print(header("Setup Finished With Errors"))
        print(warn("Some steps failed. Check the output above."))
        print(info("You can re-run this command after fixing issues."))
        SETUP_MARKER.unlink(missing_ok=True)
    print()
    return 0 if all_ok else 1


def _source_brew() -> None:
    """Add Homebrew to PATH for the current process."""
    brew_paths = [
        Path.home() / ".linuxbrew/bin/brew",
        Path("/home/linuxbrew/.linuxbrew/bin/brew"),
    ]
    for brew_bin in brew_paths:
        if brew_bin.exists():
            result = run_cmd([str(brew_bin), "shellenv"])
            if result.returncode == 0:
                for line in result.stdout.splitlines():
                    # Parse export statements like: export PATH="/home/linuxbrew/.linuxbrew/bin:..."
                    line = line.strip()
                    if line.startswith("export "):
                        kv = line[7:]  # strip "export "
                        if "=" in kv:
                            key, val = kv.split("=", 1)
                            # Strip surrounding quotes
                            val = val.strip('"').strip("'")
                            os.environ[key] = val
            break


def _setup_step(
    description: str,
    check_cmd: str | None = None,
    install_cmds: list[list[str]] | None = None,
) -> bool:
    """Run a setup step. Returns True on success."""
    if check_cmd and cmd_available(check_cmd):
        print(ok(f"{description} (already installed)"))
        return True

    print(step(description + "..."))
    for cmd in (install_cmds or []):
        result = subprocess.run(cmd, capture_output=False)
        if result.returncode != 0:
            print(fail(f"Failed: {' '.join(cmd[:3])}..."))
            return False

    print(ok(description))
    return True


# =============================================================================
# Command: doctor
# =============================================================================

def cmd_doctor(_args: argparse.Namespace) -> int:
    print(header("zOS Doctor"))
    print()
    passed = 0
    failed = 0
    warned = 0

    # --- Hyprland ---
    result = run_cmd(["hyprctl", "version"])
    if result.returncode == 0:
        # Extract version from output
        version_line = ""
        for line in result.stdout.splitlines():
            if "Hyprland" in line or "Tag" in line:
                version_line = line.strip()
                break
        print(ok(f"Hyprland is running{f' ({version_line})' if version_line else ''}"))
        passed += 1
    else:
        print(fail("Hyprland is not running or hyprctl not available"))
        failed += 1

    # --- NVIDIA ---
    if cmd_available("nvidia-smi"):
        result = run_cmd(["nvidia-smi", "--query-gpu=driver_version", "--format=csv,noheader"])
        if result.returncode == 0:
            driver = result.stdout.strip().split("\n")[0]
            print(ok(f"NVIDIA driver loaded (version {driver})"))
            passed += 1
        else:
            print(fail("nvidia-smi found but driver query failed"))
            failed += 1
    else:
        print(info("NVIDIA driver not present (AMD or no discrete GPU)"))

    # --- PipeWire ---
    result = run_cmd(["systemctl", "--user", "is-active", "pipewire"])
    if result.returncode == 0 and "active" in result.stdout:
        print(ok("PipeWire is running"))
        passed += 1
    else:
        print(fail("PipeWire is not running"))
        failed += 1

    # --- Expected packages ---
    print(header("Installed Packages"))
    for pkg in EXPECTED_PACKAGES:
        if cmd_available(pkg):
            print(ok(pkg))
            passed += 1
        else:
            # Some packages have different binary names
            alt_names = {
                "fd-find": "fd",
                "ripgrep": "rg",
            }
            alt = alt_names.get(pkg)
            if alt and cmd_available(alt):
                print(ok(f"{pkg} (as {alt})"))
                passed += 1
            else:
                print(fail(f"{pkg} not found"))
                failed += 1

    # --- Deprecated Hyprland syntax ---
    print(header("Hyprland Config Lint"))
    hypr_conf = Path.home() / ".config/hypr/hyprland.conf"
    if hypr_conf.exists():
        try:
            content = hypr_conf.read_text()
            found_deprecated = False
            for keyword in DEPRECATED_HYPR_KEYWORDS:
                # Check each line for the keyword (not in comments)
                for lineno, line in enumerate(content.splitlines(), 1):
                    stripped = line.strip()
                    if stripped.startswith("#"):
                        continue
                    if keyword in stripped:
                        print(warn(f"Deprecated: '{keyword}' found at line {lineno}"))
                        found_deprecated = True
                        warned += 1
                        break  # One warning per keyword is enough
            if not found_deprecated:
                print(ok("No deprecated syntax found"))
                passed += 1
        except OSError as e:
            print(fail(f"Could not read hyprland.conf: {e}"))
            failed += 1
    else:
        print(info("No hyprland.conf found (not on Hyprland?)"))

    # --- Check for user config files ---
    print(header("User Config Files"))
    config_files = {
        "Hyprland config": Path.home() / ".config/hypr/hyprland.conf",
        "Waybar config": Path.home() / ".config/waybar/config.jsonc",
        "Waybar style": Path.home() / ".config/waybar/style.css",
        "Wezterm config": Path.home() / ".config/wezterm/wezterm.lua",
        "Starship config": Path.home() / ".config/starship.toml",
        "zshrc": Path.home() / ".zshrc",
        "gitconfig": Path.home() / ".gitconfig",
    }
    for name, path in config_files.items():
        if path.exists():
            print(ok(name))
            passed += 1
        else:
            print(warn(f"{name} missing ({path})"))
            warned += 1

    # --- Summary ---
    print(header("Summary"))
    total = passed + failed + warned
    print(f"  {C.GREEN}{passed} passed{C.RESET}, "
          f"{C.RED}{failed} failed{C.RESET}, "
          f"{C.YELLOW}{warned} warnings{C.RESET} "
          f"({total} checks)")
    print()
    return 1 if failed > 0 else 0


# =============================================================================
# Argument Parser
# =============================================================================

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="zos-system",
        description="zOS system management tool",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=textwrap.dedent("""\
            examples:
              zos-system status            Show system and config status
              zos-system migrate           Dry run — show what would change
              zos-system migrate --apply   Apply config migrations
              zos-system migrate --auto    Silent mode (for systemd)
              sudo zos-system grub         Configure GRUB and dual-boot
              zos-system setup             Run first-login setup
              zos-system doctor            Run diagnostic checks
        """),
    )

    sub = parser.add_subparsers(dest="command", required=True)

    # status
    sub.add_parser("status", help="Show system and config migration status")

    # migrate
    migrate_parser = sub.add_parser("migrate", help="Migrate user configs to latest system defaults")
    migrate_group = migrate_parser.add_mutually_exclusive_group()
    migrate_group.add_argument(
        "--apply", action="store_true",
        help="Actually apply migrations (default is dry run)",
    )
    migrate_group.add_argument(
        "--auto", action="store_true",
        help="Silent mode for systemd: apply and only output on error",
    )

    # grub
    sub.add_parser("grub", help="Configure GRUB timeout and Windows dual-boot")

    # setup
    sub.add_parser("setup", help="Run first-login setup (Homebrew, mise, Rust, etc.)")

    # doctor
    sub.add_parser("doctor", help="Run diagnostic checks")

    return parser


# =============================================================================
# Main
# =============================================================================

COMMANDS = {
    "status": cmd_status,
    "migrate": cmd_migrate,
    "grub": cmd_grub,
    "setup": cmd_setup,
    "doctor": cmd_doctor,
}


def main() -> int:
    C.init()
    parser = build_parser()
    args = parser.parse_args()
    handler = COMMANDS.get(args.command)
    if handler is None:
        parser.print_help()
        return 1
    try:
        return handler(args)
    except KeyboardInterrupt:
        print()
        return 130
    except Exception as e:
        print(fail(f"Unexpected error: {e}"), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
