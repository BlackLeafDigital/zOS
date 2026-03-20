#!/bin/bash
set -euo pipefail

# =============================================================================
# zOS System Setup
# Post-install system configuration (GRUB, dual-boot, etc.)
# Safe to re-run — idempotent
# Usage: sudo zos-setup
# =============================================================================

if [ "$(id -u)" -ne 0 ]; then
    echo "Error: zos-setup must be run as root (sudo zos-setup)"
    exit 1
fi

echo "========================================"
echo "  zOS System Setup"
echo "========================================"
echo ""

CHANGES=()

# --- GRUB Timeout ---
GRUB_USER_CFG="/boot/grub2/user.cfg"

if [ -f "$GRUB_USER_CFG" ] && grep -q "^set timeout=" "$GRUB_USER_CFG"; then
    sed -i 's/^set timeout=.*/set timeout=15/' "$GRUB_USER_CFG"
else
    mkdir -p "$(dirname "$GRUB_USER_CFG")"
    echo "set timeout=15" >> "$GRUB_USER_CFG"
fi
CHANGES+=("GRUB timeout set to 15 seconds")

# --- Windows Detection ---
WINDOWS_BLS="/boot/loader/entries/windows.conf"

if [ -f "$WINDOWS_BLS" ]; then
    CHANGES+=("Windows boot entry already exists (skipped)")
else
    WINDOWS_EFI=""

    # Check the current ESP for Windows bootloader
    ESP_MOUNT=""
    if mountpoint -q /boot/efi 2>/dev/null; then
        ESP_MOUNT="/boot/efi"
    elif mountpoint -q /efi 2>/dev/null; then
        ESP_MOUNT="/efi"
    fi

    if [ -n "$ESP_MOUNT" ] && [ -f "$ESP_MOUNT/EFI/Microsoft/Boot/bootmgfw.efi" ]; then
        WINDOWS_EFI="/EFI/Microsoft/Boot/bootmgfw.efi"
    fi

    # Also scan other potential EFI partitions
    if [ -z "$WINDOWS_EFI" ]; then
        while IFS= read -r part; do
            TMPDIR=$(mktemp -d)
            if mount -o ro "$part" "$TMPDIR" 2>/dev/null; then
                if [ -f "$TMPDIR/EFI/Microsoft/Boot/bootmgfw.efi" ]; then
                    WINDOWS_EFI="/EFI/Microsoft/Boot/bootmgfw.efi"
                    umount "$TMPDIR"
                    rmdir "$TMPDIR"
                    break
                fi
                umount "$TMPDIR"
            fi
            rmdir "$TMPDIR"
        done < <(lsblk -rno PATH,PARTTYPE | grep -i "c12a7328-f81f-11d2-ba4b-00a0c93ec93b" | awk '{print $1}')
    fi

    if [ -n "$WINDOWS_EFI" ]; then
        mkdir -p "$(dirname "$WINDOWS_BLS")"
        cat > "$WINDOWS_BLS" << EOF
title Windows
efi $WINDOWS_EFI
EOF
        CHANGES+=("Windows boot entry added")
    else
        CHANGES+=("Windows not detected on any EFI partition (skipped)")
    fi
fi

# --- Summary ---
echo ""
echo "========================================"
echo "  Setup Complete"
echo "========================================"
for change in "${CHANGES[@]}"; do
    echo "  - $change"
done
echo ""
echo "Reboot to apply GRUB changes."
