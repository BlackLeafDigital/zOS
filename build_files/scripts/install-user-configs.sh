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

# --- First-login setup script ---
cp /ctx/scripts/zos-first-login.sh /usr/bin/zos-first-login
chmod +x /usr/bin/zos-first-login

echo "User configurations installed."
