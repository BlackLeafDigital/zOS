#!/bin/bash
# =============================================================================
# zOS First Login Setup
# Runs on first login to install user-space tools
# =============================================================================

MARKER="$HOME/.config/zos-setup-done"

if [ -f "$MARKER" ]; then
    exit 0
fi

echo "============================================"
echo "  Welcome to zOS! Setting up your system..."
echo "============================================"
echo ""

# --- Install Homebrew (user-space package manager) ---
if ! command -v brew &> /dev/null; then
    echo "[zOS] Installing Homebrew for CLI tools..."
    /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)" < /dev/null
    eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"
fi

# --- Install mise (dev tool version manager) ---
if ! command -v mise &> /dev/null; then
    echo "[zOS] Installing mise (runtime version manager)..."
    brew install mise
fi

# --- Install common dev runtimes via mise ---
echo "[zOS] Setting up Node.js, Python, Bun, Zig, PHP, and Flutter..."
mise use --global node@lts
mise use --global python@latest
mise use --global bun@latest
mise use --global zig@latest
mise use --global php@latest
mise use --global flutter@stable

# --- Install pnpm ---
if command -v node &> /dev/null; then
    echo "[zOS] Installing pnpm..."
    npm install -g pnpm
fi

# --- Install uv (Python package manager) ---
echo "[zOS] Installing uv (fast Python package manager)..."
brew install uv

# --- Rust toolchain is pre-installed via dnf (system-level) ---
# No rustup needed — /usr/bin/rustc and /usr/bin/cargo are in the image

# --- Install GitHub CLI ---
if ! command -v gh &> /dev/null; then
    echo "[zOS] Installing GitHub CLI..."
    brew install gh
fi

# --- Change default shell to zsh ---
if [ "$SHELL" != "$(which zsh)" ]; then
    echo "[zOS] Setting zsh as default shell..."
    chsh -s "$(which zsh)" 2>/dev/null || true
fi

# --- Install Nerd Fonts for user ---
echo "[zOS] Installing JetBrainsMono Nerd Font..."
mkdir -p "$HOME/.local/share/fonts"
if [ ! -f "$HOME/.local/share/fonts/JetBrainsMonoNerdFont-Regular.ttf" ]; then
    FONT_URL="https://github.com/ryanoasis/nerd-fonts/releases/latest/download/JetBrainsMono.tar.xz"
    curl -fsSL "$FONT_URL" | tar -xJ -C "$HOME/.local/share/fonts/"
    fc-cache -fv "$HOME/.local/share/fonts/" > /dev/null 2>&1
fi

# --- Mark setup as done ---
mkdir -p "$(dirname "$MARKER")"
touch "$MARKER"

echo ""
echo "============================================"
echo "  zOS setup complete! Restart your terminal."
echo "============================================"
