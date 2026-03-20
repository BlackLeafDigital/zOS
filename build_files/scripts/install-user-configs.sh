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

# --- System setup script (GRUB, dual-boot) ---
cp /ctx/scripts/zos-setup.sh /usr/bin/zos-setup
chmod +x /usr/bin/zos-setup

# --- First-login setup script ---
cp /ctx/scripts/zos-first-login.sh /usr/bin/zos-first-login
chmod +x /usr/bin/zos-first-login

echo "User configurations installed."
