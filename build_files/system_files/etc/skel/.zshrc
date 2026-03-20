# =============================================================================
# zOS User Shell Configuration
# Oh My Zsh + Powerlevel10k + zOS system config
# =============================================================================

# Enable Powerlevel10k instant prompt (must be near top of .zshrc)
if [[ -r "${XDG_CACHE_HOME:-$HOME/.cache}/p10k-instant-prompt-${(%):-%n}.zsh" ]]; then
  source "${XDG_CACHE_HOME:-$HOME/.cache}/p10k-instant-prompt-${(%):-%n}.zsh"
fi

# --- Oh My Zsh ---
export ZSH="$HOME/.oh-my-zsh"
ZSH_THEME="powerlevel10k/powerlevel10k"

plugins=(
  git
  zsh-autosuggestions
  zsh-syntax-highlighting
)

source $ZSH/oh-my-zsh.sh

# --- Powerlevel10k config ---
[[ ! -f ~/.p10k.zsh ]] || source ~/.p10k.zsh
