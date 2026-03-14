#!/bin/bash
set -ouex pipefail

# =============================================================================
# Hyprland Installation
# Optional tiling WM session alongside KDE Plasma
# =============================================================================

# --- Install Hyprland and ecosystem ---
dnf5 install -y \
    hyprland \
    hyprpaper \
    hyprlock \
    hypridle \
    xdg-desktop-portal-hyprland \
    waybar \
    wofi \
    mako \
    grim \
    slurp \
    swappy \
    brightnessctl \
    playerctl \
    pamixer \
    polkit-kde

# --- Copy Hyprland session file for SDDM/login screen ---
cp /ctx/system_files/usr/share/wayland-sessions/hyprland-zos.desktop \
   /usr/share/wayland-sessions/hyprland-zos.desktop

# --- Copy default Hyprland configs to skeleton (new user defaults) ---
cp -r /ctx/system_files/etc/skel/.config/hypr /etc/skel/.config/
cp -r /ctx/system_files/etc/skel/.config/waybar /etc/skel/.config/

echo "Hyprland installation complete."
