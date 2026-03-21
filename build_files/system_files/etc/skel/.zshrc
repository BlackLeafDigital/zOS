# =============================================================================
# zOS User Shell Configuration
# Oh My Zsh + Powerlevel10k + zOS system config
# =============================================================================

# Enable Powerlevel10k instant prompt (must be near top of .zshrc)
# Guard: only runs in actual zsh (not bash sourcing this file)
if [ -n "$ZSH_VERSION" ]; then
  if [[ -r "${XDG_CACHE_HOME:-$HOME/.cache}/p10k-instant-prompt-${(%):-%n}.zsh" ]]; then
    source "${XDG_CACHE_HOME:-$HOME/.cache}/p10k-instant-prompt-${(%):-%n}.zsh"
  fi
fi

# --- Oh My Zsh ---
export ZSH="$HOME/.oh-my-zsh"
ZSH_THEME="powerlevel10k/powerlevel10k"

plugins=(
  git
  zsh-autosuggestions
  zsh-syntax-highlighting
)

[ -f "$ZSH/oh-my-zsh.sh" ] && source "$ZSH/oh-my-zsh.sh"

# --- Re-init tools that Oh My Zsh overwrites ---
command -v atuin &> /dev/null && eval "$(atuin init zsh)"
command -v zoxide &> /dev/null && eval "$(zoxide init zsh)"

# --- Powerlevel10k config ---
[ -f ~/.p10k.zsh ] && source ~/.p10k.zsh
