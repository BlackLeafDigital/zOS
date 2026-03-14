#!/bin/bash
set -ouex pipefail

# =============================================================================
# Hyprland Installation
# Optional tiling WM session alongside KDE Plasma
# =============================================================================

# --- Hyprland COPR (retired from Fedora 43) ---
dnf5 copr enable -y sdegler/hyprland

# --- Install Hyprland and ecosystem ---
# From COPR: hyprland, hyprpaper, hyprlock, hypridle, xdg-desktop-portal-hyprland, hyprpolkitagent
# From Fedora repos: waybar, wofi, mako, grim, slurp, swappy, brightnessctl, playerctl, pamixer
# polkit-kde already in Bazzite
dnf5 install -y \
    hyprland \
    hyprpaper \
    hyprlock \
    hypridle \
    hyprpolkitagent \
    xdg-desktop-portal-hyprland \
    waybar \
    wofi \
    mako \
    grim \
    slurp \
    swappy \
    brightnessctl \
    playerctl \
    pamixer

# --- Copy Hyprland session file for SDDM/login screen ---
cp /ctx/system_files/usr/share/wayland-sessions/hyprland-zos.desktop \
   /usr/share/wayland-sessions/hyprland-zos.desktop

# --- Copy default Hyprland configs to skeleton (new user defaults) ---
cp -r /ctx/system_files/etc/skel/.config/hypr /etc/skel/.config/
cp -r /ctx/system_files/etc/skel/.config/waybar /etc/skel/.config/

echo "Hyprland installation complete."
