# zOS Environment — all users, all shells, all sessions (including non-interactive)

[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

if [ -d "/home/linuxbrew/.linuxbrew" ]; then
    eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv 2>/dev/null)"
elif [ -d "$HOME/.linuxbrew" ]; then
    eval "$("$HOME/.linuxbrew/bin/brew" shellenv 2>/dev/null)"
fi

[ -d "$HOME/.local/share/mise/shims" ] && PATH="$HOME/.local/share/mise/shims:$PATH"
[ -d /usr/lib/jvm/java-21-openjdk ] && export JAVA_HOME=/usr/lib/jvm/java-21-openjdk

if [ -d "$HOME/Android/Sdk" ]; then
    export ANDROID_HOME="$HOME/Android/Sdk"
    export ANDROID_SDK_ROOT="$ANDROID_HOME"
    PATH="$PATH:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin"
fi

export PATH
