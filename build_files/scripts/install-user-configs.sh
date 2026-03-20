#!/bin/bash
set -ouex pipefail

# =============================================================================
# User Configuration Files
# Copy default configs to /etc/skel for new user setup
# =============================================================================

# --- Wezterm config ---
cp -r /ctx/system_files/etc/skel/.config/wezterm /etc/skel/.config/

# --- Starship config ---
mkdir -p /etc/skel/.config
cp /ctx/system_files/etc/skel/.config/starship.toml /etc/skel/.config/

# --- Shell configuration ---
cp /ctx/system_files/etc/skel/.zshrc /etc/skel/.zshrc

# --- Git config (with delta integration + multi-account template) ---
cp /ctx/system_files/etc/skel/.gitconfig /etc/skel/.gitconfig

# --- DNF wrapper (explains immutable OS alternatives) ---
mkdir -p /etc/skel/.local/bin
cp /ctx/system_files/etc/skel/.local/bin/dnf /etc/skel/.local/bin/dnf
chmod +x /etc/skel/.local/bin/dnf

# --- zos-system is built from Rust in Containerfile, already at /usr/bin/zos-system ---

# --- Auto-migration systemd user service ---
cp /ctx/system_files/usr/lib/systemd/user/zos-user-migrate.service \
   /usr/lib/systemd/user/zos-user-migrate.service
systemctl --global enable zos-user-migrate.service

# --- Legacy scripts (kept for compatibility, absorbed by zos-system) ---
cp /ctx/scripts/zos-setup.sh /usr/bin/zos-setup
chmod +x /usr/bin/zos-setup
cp /ctx/scripts/zos-first-login.sh /usr/bin/zos-first-login
chmod +x /usr/bin/zos-first-login

echo "User configurations installed."
