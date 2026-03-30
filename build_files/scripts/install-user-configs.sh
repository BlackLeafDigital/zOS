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

# --- Shell configuration (system-level, not in skel — ~/.zshrc is the user's) ---
mkdir -p /usr/share/zos
cp /ctx/system_files/usr/share/zos/zshrc /usr/share/zos/zshrc
cp /ctx/system_files/usr/share/zos/config-versions.json /usr/share/zos/config-versions.json

# Source zOS config from global zshrc (loaded before ~/.zshrc)
{
    echo ''
    echo '# --- zOS shell configuration ---'
    echo '[ -f /usr/share/zos/zshrc ] && source /usr/share/zos/zshrc'
} >> /etc/zshrc

# --- Oh My Zsh + Powerlevel10k (baked into skel for instant setup) ---
export ZSH="/etc/skel/.oh-my-zsh"
export KEEP_ZSHRC=yes
export HOME=/tmp/omz-build
mkdir -p "$HOME"
touch "$HOME/.zshrc"
sh -c "$(curl -fsSL https://raw.githubusercontent.com/ohmyzsh/ohmyzsh/master/tools/install.sh)" "" --unattended --keep-zshrc
unset HOME
git clone --depth 1 https://github.com/zsh-users/zsh-autosuggestions ${ZSH}/custom/plugins/zsh-autosuggestions
git clone --depth 1 https://github.com/zsh-users/zsh-syntax-highlighting ${ZSH}/custom/plugins/zsh-syntax-highlighting
git clone --depth 1 https://github.com/romkatv/powerlevel10k.git ${ZSH}/custom/themes/powerlevel10k

# --- User .zshrc and .p10k.zsh ---
cp /ctx/system_files/etc/skel/.zshrc /etc/skel/.zshrc
cp /ctx/system_files/etc/skel/.p10k.zsh /etc/skel/.p10k.zsh

# --- Git config (with delta integration + multi-account template) ---
cp /ctx/system_files/etc/skel/.gitconfig /etc/skel/.gitconfig

# --- DNF wrapper (explains immutable OS alternatives) ---
mkdir -p /etc/skel/.local/bin
cp /ctx/system_files/etc/skel/.local/bin/dnf /etc/skel/.local/bin/dnf
chmod +x /etc/skel/.local/bin/dnf

# --- Hyprpaper config (default wallpaper) ---
cp /ctx/system_files/etc/skel/.config/hypr/hyprpaper.conf /etc/skel/.config/hypr/

# --- HyprPanel config (Catppuccin Mocha theme + zOS bar layout) ---
mkdir -p /etc/skel/.config/hyprpanel
cp /ctx/system_files/etc/skel/.config/hyprpanel/config.json /etc/skel/.config/hyprpanel/
cp /ctx/system_files/etc/skel/.config/hyprpanel/modules.json /etc/skel/.config/hyprpanel/

# --- Hyprshell config (window switcher + launcher) ---
mkdir -p /etc/skel/.config/hyprshell
cp /ctx/system_files/etc/skel/.config/hyprshell/config.ron /etc/skel/.config/hyprshell/
cp /ctx/system_files/etc/skel/.config/hyprshell/styles.css /etc/skel/.config/hyprshell/

# --- PipeWire virtual audio devices (VoiceMeeter-style routing) ---
mkdir -p /etc/skel/.config/pipewire/pipewire.conf.d
cp /ctx/system_files/etc/skel/.config/pipewire/pipewire.conf.d/10-zos-virtual-devices.conf \
   /etc/skel/.config/pipewire/pipewire.conf.d/

# --- Flatpak overrides for dev apps (full filesystem access) ---
mkdir -p /var/lib/flatpak/overrides
cat > /var/lib/flatpak/overrides/com.visualstudio.code << 'FLATPAK_EOF'
[Context]
filesystems=home;host;/tmp;
sockets=ssh-auth;
FLATPAK_EOF

# Floorp browser — allow mDNS (.local) resolution + filesystem access
cat > /var/lib/flatpak/overrides/one.ablaze.floorp << 'FLATPAK_EOF'
[Context]
filesystems=home;
sockets=system-bus;session-bus;
FLATPAK_EOF

# --- System limits (nofile, nproc, memlock, core) ---
mkdir -p /etc/security/limits.d
cp /ctx/system_files/etc/security/limits.d/99-zos.conf /etc/security/limits.d/

# --- zos-settings desktop entry, icon + polkit policy ---
cp /ctx/system_files/usr/share/applications/zos-settings.desktop /usr/share/applications/

# --- CoolerControl desktop entry ---
cp /ctx/system_files/usr/share/applications/coolercontrol.desktop /usr/share/applications/
mkdir -p /usr/share/icons/hicolor/scalable/apps
cp /ctx/system_files/usr/share/icons/hicolor/scalable/apps/zos-settings.svg /usr/share/icons/hicolor/scalable/apps/
cp /ctx/system_files/usr/share/icons/hicolor/scalable/apps/zos-settings-symbolic.svg /usr/share/icons/hicolor/scalable/apps/

# Generate PNG icons at all standard sizes from both SVGs
for size in 16 24 32 48 64 128 256; do
    dir=/usr/share/icons/hicolor/${size}x${size}/apps
    mkdir -p $dir
    magick /usr/share/icons/hicolor/scalable/apps/zos-settings.svg -resize ${size}x${size} $dir/zos-settings.png
    magick /usr/share/icons/hicolor/scalable/apps/zos-settings-symbolic.svg -resize ${size}x${size} $dir/zos-settings-symbolic.png
done
gtk-update-icon-cache /usr/share/icons/hicolor/ 2>/dev/null || true
mkdir -p /usr/share/polkit-1/actions
cp /ctx/system_files/usr/share/polkit-1/actions/com.zos.settings.policy /usr/share/polkit-1/actions/

# --- zos is built from Rust in Containerfile, already at /usr/bin/zos ---

# --- Auto-migration systemd user service ---
cp /ctx/system_files/usr/lib/systemd/user/zos-user-migrate.service \
   /usr/lib/systemd/user/zos-user-migrate.service
systemctl --global enable zos-user-migrate.service

# --- Global environment for all shells (profile.d) ---
cp /ctx/system_files/etc/profile.d/zos-env.sh /etc/profile.d/zos-env.sh

# --- Set zsh as default shell for all users ---
# chsh in first-login fails silently on Fedora Atomic; set it system-wide
sed -i 's|^SHELL=.*|SHELL=/usr/bin/zsh|' /etc/default/useradd
# Also update /etc/passwd template so new users get zsh
if ! grep -q '/usr/bin/zsh' /etc/shells; then
    echo '/usr/bin/zsh' >> /etc/shells
fi

# --- zos-skel-sync (deploy missing skel configs on login) ---
cp /ctx/system_files/usr/bin/zos-skel-sync /usr/bin/zos-skel-sync
chmod +x /usr/bin/zos-skel-sync

# --- Legacy scripts (kept for compatibility, absorbed by zos) ---
cp /ctx/scripts/zos-setup.sh /usr/bin/zos-setup
chmod +x /usr/bin/zos-setup
cp /ctx/scripts/zos-first-login.sh /usr/bin/zos-first-login
chmod +x /usr/bin/zos-first-login

echo "User configurations installed."
