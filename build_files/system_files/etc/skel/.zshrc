# =============================================================================
# zOS ZSH Configuration
# =============================================================================

# --- History ---
HISTSIZE=10000
SAVEHIST=10000
HISTFILE=~/.zsh_history
setopt HIST_IGNORE_ALL_DUPS
setopt HIST_SAVE_NO_DUPS
setopt SHARE_HISTORY

# --- Completion ---
autoload -Uz compinit
compinit
zstyle ':completion:*' matcher-list 'm:{a-z}={A-Za-z}'
zstyle ':completion:*' menu select

# --- Aliases ---
alias ls='eza --icons'
alias ll='eza -la --icons --git'
alias lt='eza --tree --icons -L 2'
alias cat='bat --paging=never'
alias grep='rg'
alias find='fd'
alias diff='delta'
alias top='btop'
alias ..='cd ..'
alias ...='cd ../..'

# --- Git aliases ---
alias gs='git status'
alias ga='git add'
alias gc='git commit'
alias gp='git push'
alias gl='git log --oneline --graph -20'
alias gd='git diff'
alias gco='git checkout'
alias gb='git branch'

# --- Docker/Podman ---
alias docker='podman'
alias dc='podman-compose'

# --- zoxide (smart cd) ---
if command -v zoxide &> /dev/null; then
    eval "$(zoxide init zsh)"
fi

# --- fzf ---
if command -v fzf &> /dev/null; then
    source <(fzf --zsh) 2>/dev/null || true
fi

# --- Starship prompt ---
if command -v starship &> /dev/null; then
    eval "$(starship init zsh)"
fi

# --- Homebrew (if installed via first-login) ---
if [ -d "$HOME/.linuxbrew" ]; then
    eval "$($HOME/.linuxbrew/bin/brew shellenv)"
elif [ -d "/home/linuxbrew/.linuxbrew" ]; then
    eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"
fi

# --- mise (dev tool version manager) ---
if command -v mise &> /dev/null; then
    eval "$(mise activate zsh)"
fi
