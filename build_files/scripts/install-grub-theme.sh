#!/bin/bash
set -ouex pipefail

# =============================================================================
# Catppuccin Mocha GRUB Theme Installation
# =============================================================================
#
# Fetches catppuccin/grub at a pinned commit and installs the Mocha variant to
# /usr/share/grub/themes/catppuccin-mocha/ inside the immutable image layer.
#
# Runtime activation (copy to /boot/grub2/themes/ + write theme stanza to
# /boot/grub2/user.cfg) is performed by `sudo zos-system grub` or the
# `zos grub` TUI -- this script does NOT touch /boot.
# =============================================================================

# --- Robust GitHub download helpers (mirrored from install-hyprland.sh) ---
GH_CURL_OPTS=(--connect-timeout 30 --retry 5 --retry-delay 10 --retry-all-errors --max-time 300 -fsSL)
if [ -n "${GITHUB_TOKEN:-}" ]; then GH_CURL_OPTS+=(-H "Authorization: token ${GITHUB_TOKEN}"); fi

gh_clone() {
    local url="$1"; shift
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        url="${url/https:\/\/github.com/https://${GITHUB_TOKEN}@github.com}"
    fi
    git clone "$url" "$@"
}

# --- Pinned commit (bump intentionally; treat as supply-chain dependency) ---
CATPPUCCIN_GRUB_COMMIT="0a37ab19f654e77129b409fed371891c01ffd0b9"
CATPPUCCIN_GRUB_REPO="https://github.com/catppuccin/grub.git"

# --- Fetch ---
TMPDIR_THEME="$(mktemp -d)"
trap 'rm -rf "${TMPDIR_THEME}"' EXIT

gh_clone "${CATPPUCCIN_GRUB_REPO}" "${TMPDIR_THEME}"
cd "${TMPDIR_THEME}"
git checkout "${CATPPUCCIN_GRUB_COMMIT}"

# --- Install Mocha variant ---
# catppuccin/grub layout: src/<flavor>-grub-theme/{theme.txt, background.png, ...}
SRC_DIR="${TMPDIR_THEME}/src/catppuccin-mocha-grub-theme"
DEST_DIR="/usr/share/grub/themes/catppuccin-mocha"

if [ ! -d "${SRC_DIR}" ]; then
    echo "FATAL: expected ${SRC_DIR} not found in catppuccin/grub@${CATPPUCCIN_GRUB_COMMIT}" >&2
    echo "       Upstream layout may have changed -- inspect ${TMPDIR_THEME}" >&2
    exit 1
fi

mkdir -p "${DEST_DIR}"
cp -r "${SRC_DIR}/." "${DEST_DIR}/"

# Sanity check -- theme.txt is the GRUB-loadable manifest
if [ ! -f "${DEST_DIR}/theme.txt" ]; then
    echo "FATAL: ${DEST_DIR}/theme.txt missing after copy" >&2
    exit 1
fi

echo "[install-grub-theme] Catppuccin Mocha GRUB theme installed to ${DEST_DIR}"
echo "[install-grub-theme] Activate with: sudo zos-system grub"
