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
    hyprland-guiutils \
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
    nwg-displays \
    wlogout \
    blueman \
    network-manager-applet \
    qpwgraph \
    easyeffects \
    pavucontrol \
    qt5ct \
    qt6ct \
    papirus-icon-theme

# --- cliphist (clipboard history, not in Fedora 43 repos) ---
CURL_GH_OPTS=(--connect-timeout 10 --retry 3)
if [ -n "${GITHUB_TOKEN:-}" ]; then CURL_GH_OPTS+=(-H "Authorization: token ${GITHUB_TOKEN}"); fi
CLIPHIST_VERSION=$(curl -fsSL "${CURL_GH_OPTS[@]}" https://api.github.com/repos/sentriz/cliphist/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
curl -fsSL "${CURL_GH_OPTS[@]}" -o /usr/bin/cliphist "https://github.com/sentriz/cliphist/releases/download/v${CLIPHIST_VERSION}/v${CLIPHIST_VERSION}-linux-amd64"
chmod +x /usr/bin/cliphist

# --- Catppuccin Mocha cursors ---
CURSOR_URL="https://github.com/catppuccin/cursors/releases/latest/download/catppuccin-mocha-dark-cursors.zip"
curl -fsSL -o /tmp/catppuccin-cursors.zip "$CURSOR_URL"
unzip -o /tmp/catppuccin-cursors.zip -d /usr/share/icons/
rm /tmp/catppuccin-cursors.zip

# --- nwg-look (GTK settings editor for wlroots) ---
NWG_LOOK_URL="https://github.com/nwg-piotr/nwg-look/releases/latest/download/nwg-look-v0.2.7-1.x86_64.rpm"
dnf5 install -y "$NWG_LOOK_URL" || true

# --- greetd + ReGreet login ---
dnf5 install -y greetd greetd-selinux
mkdir -p /etc/greetd
cp /ctx/system_files/etc/greetd/config.toml /etc/greetd/
cp /ctx/system_files/etc/greetd/hyprland.conf /etc/greetd/
cp /ctx/system_files/etc/greetd/hyprpaper.conf /etc/greetd/
cp /ctx/system_files/etc/greetd/regreet.toml /etc/greetd/
# greetd RPM creates 'greetd' user via sysusers.d — add video/input groups
cp /ctx/system_files/usr/lib/sysusers.d/zos-greetd.conf /usr/lib/sysusers.d/
# ReGreet cache dir created at boot via tmpfiles.d
cp /ctx/system_files/usr/lib/tmpfiles.d/zos-regreet.conf /usr/lib/tmpfiles.d/
systemctl disable sddm || true
systemctl enable greetd

# --- Copy Hyprland session file for greetd ---
cp /ctx/system_files/usr/share/wayland-sessions/hyprland-zos.desktop \
   /usr/share/wayland-sessions/hyprland-zos.desktop
rm -f /usr/share/wayland-sessions/hyprland.desktop
rm -f /usr/share/wayland-sessions/hyprland-uwsm.desktop
rm -f /usr/share/wayland-sessions/plasma.desktop
rm -f /usr/share/xsessions/plasma.desktop

# --- Copy system-managed Hyprland configs (update with OS) ---
mkdir -p /usr/share/zos/hypr
cp /ctx/system_files/usr/share/zos/hypr/*.conf /usr/share/zos/hypr/
cp /ctx/system_files/usr/share/zos/version /usr/share/zos/version
mkdir -p /usr/share/zos/scripts
cp /ctx/system_files/usr/share/zos/scripts/*.sh /usr/share/zos/scripts/
chmod +x /usr/share/zos/scripts/*.sh

# --- Copy default Hyprland configs to skeleton (new user defaults) ---
cp -r /ctx/system_files/etc/skel/.config/hypr /etc/skel/.config/
cp -r /ctx/system_files/etc/skel/.config/waybar /etc/skel/.config/
cp -r /ctx/system_files/etc/skel/.config/wlogout /etc/skel/.config/

echo "Hyprland installation complete."
