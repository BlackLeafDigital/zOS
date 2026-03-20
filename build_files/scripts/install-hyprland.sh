#!/bin/bash
set -ouex pipefail

# =============================================================================
# Hyprland Installation
# Optional tiling WM session alongside KDE Plasma
# =============================================================================

# --- Hyprland COPR (retired from Fedora 43) ---
dnf5 copr enable -y sdegler/hyprland

# --- Install Hyprland and ecosystem ---
# From COPR: hyprland, hyprpaper, hyprlock, hypridle, xdg-desktop-portal-hyprland,
#            hyprpolkitagent, hyprpicker
# From Fedora repos: everything else
dnf5 install -y \
    hyprland \
    hyprpaper \
    hyprlock \
    hypridle \
    hyprpolkitagent \
    hyprpicker \
    xdg-desktop-portal-hyprland \
    waybar \
    wofi \
    SwayNotificationCenter \
    grim \
    slurp \
    swappy \
    brightnessctl \
    playerctl \
    pamixer \
    wdisplays \
    cliphist \
    wlogout \
    blueman \
    NetworkManager-applet \
    qpwgraph \
    easyeffects \
    pavucontrol \
    qt5ct \
    qt6ct \
    qt5-qtwayland \
    qt6-qtwayland \
    papirus-icon-theme

# --- Catppuccin Mocha cursors ---
CURSOR_URL="https://github.com/catppuccin/cursors/releases/latest/download/catppuccin-mocha-dark-cursors.zip"
curl -fsSL -o /tmp/catppuccin-cursors.zip "$CURSOR_URL"
unzip -o /tmp/catppuccin-cursors.zip -d /usr/share/icons/
rm /tmp/catppuccin-cursors.zip

# --- nwg-look (GTK settings editor for wlroots) ---
NWG_LOOK_URL="https://github.com/nwg-piotr/nwg-look/releases/latest/download/nwg-look-v0.2.7-1.x86_64.rpm"
dnf5 install -y "$NWG_LOOK_URL" || true

# --- Copy Hyprland session file for SDDM/login screen ---
cp /ctx/system_files/usr/share/wayland-sessions/hyprland-zos.desktop \
   /usr/share/wayland-sessions/hyprland-zos.desktop

# --- Copy default Hyprland configs to skeleton (new user defaults) ---
cp -r /ctx/system_files/etc/skel/.config/hypr /etc/skel/.config/
cp -r /ctx/system_files/etc/skel/.config/waybar /etc/skel/.config/
cp -r /ctx/system_files/etc/skel/.config/wlogout /etc/skel/.config/

echo "Hyprland installation complete."
