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
    wofi \
    grim \
    slurp \
    swappy \
    brightnessctl \
    playerctl \
    pamixer \
    wlogout \
    qpwgraph \
    easyeffects \
    pavucontrol \
    qt5ct \
    qt6ct \
    papirus-icon-theme

# --- HyprPanel (Ubuntu-style panel with quick settings, replaces waybar+swaync+blueman+nm-applet) ---
# Note: power-profiles-daemon conflicts with Bazzite's tuned-ppd
dnf5 install -y \
    hyprpanel \
    libgtop2 \
    swww \
    xwaylandvideobridge

# Keep waybar as fallback (user can switch in autostart)
dnf5 install -y waybar

# --- cursor-clip (clipboard manager) is built in the Containerfile Rust stage ---

# --- wl-clip-persist is built in a separate Containerfile stage ---
# Binary is already at /usr/bin/wl-clip-persist

# --- Catppuccin Mocha cursors ---
CURSOR_URL="https://github.com/catppuccin/cursors/releases/latest/download/catppuccin-mocha-dark-cursors.zip"
curl -fsSL --retry 3 --retry-delay 5 -o /tmp/catppuccin-cursors.zip "$CURSOR_URL"
unzip -o /tmp/catppuccin-cursors.zip -d /usr/share/icons/
rm /tmp/catppuccin-cursors.zip

# --- hyprswitch/hyprshell (macOS/Windows-style Alt+Tab window switcher) ---
HYPRSHELL_VERSION=$(curl -fsSL --retry 3 --retry-delay 5 https://api.github.com/repos/h3rmt/hyprswitch/releases/latest | grep -oP '"tag_name":\s*"v\K[^"]+')
curl -fsSL --retry 3 --retry-delay 5 -o /tmp/hyprshell.tar.zst "https://github.com/h3rmt/hyprswitch/releases/download/v${HYPRSHELL_VERSION}/hyprshell-${HYPRSHELL_VERSION}-x86_64.tar.zst"
tar --use-compress-program=unzstd -xf /tmp/hyprshell.tar.zst -C /usr/bin/
chmod +x /usr/bin/hyprswitch 2>/dev/null || chmod +x /usr/bin/hyprshell 2>/dev/null
rm /tmp/hyprshell.tar.zst

# --- nwg-displays (Hyprland monitor config GUI, not in Fedora repos) ---
dnf5 install -y gtk-layer-shell python3-gobject python3-i3ipc python3-build python3-installer python3-setuptools python3-wheel
git clone --depth 1 https://github.com/nwg-piotr/nwg-displays.git /tmp/nwg-displays
cd /tmp/nwg-displays && /ctx/scripts/nwg-install.sh
cd / && rm -rf /tmp/nwg-displays

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
# --- Copy default Hyprland configs to skeleton (new user defaults) ---
cp -r /ctx/system_files/etc/skel/.config/hypr /etc/skel/.config/
cp -r /ctx/system_files/etc/skel/.config/waybar /etc/skel/.config/
cp -r /ctx/system_files/etc/skel/.config/wlogout /etc/skel/.config/

echo "Hyprland installation complete."
