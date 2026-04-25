//! Keybinding model for zos-wm.
//!
//! `Modifiers` is a bitflag set of compositor-relevant modifier keys.
//! `KeyCombo` is the lookup key in the bind table — modifier-set + key
//! identity. `BindKey` distinguishes keyboard syms from mouse buttons so
//! a single bind table covers both. `Action` is the verb dispatched when
//! a combo fires.
//!
//! The dispatcher itself lives in `input_handler.rs`; this module is just
//! the data model + default table.

use std::collections::HashMap;

use bitflags::bitflags;
use smithay::input::keyboard::Keysym;

bitflags! {
    /// Compositor-relevant modifier keys. Caps-lock + num-lock are NOT
    /// included; we treat them as kbd state, not bindable mods.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Modifiers: u8 {
        const SHIFT  = 0b0000_0001;
        const CTRL   = 0b0000_0010;
        const ALT    = 0b0000_0100;
        /// Super (logo / Win / Cmd).
        const SUPER  = 0b0000_1000;
        /// Often called "AltGr" — keep separate from ALT for layouts that distinguish.
        const ALTGR  = 0b0001_0000;
    }
}

/// A bindable input source — keyboard symbol or mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindKey {
    /// xkb keysym after layout/level resolution.
    Keysym(Keysym),
    /// Linux input button code (`BTN_LEFT = 0x110`, `BTN_RIGHT = 0x111`,
    /// `BTN_MIDDLE = 0x112`, etc.).
    MouseButton(u32),
}

/// Lookup key in the bind table. Two combos are equal iff their modifiers
/// AND their `BindKey` are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub modifiers: Modifiers,
    pub key:       BindKey,
}

impl KeyCombo {
    pub const fn new(modifiers: Modifiers, key: BindKey) -> Self {
        Self { modifiers, key }
    }
    pub const fn keysym(modifiers: Modifiers, sym: Keysym) -> Self {
        Self { modifiers, key: BindKey::Keysym(sym) }
    }
    pub const fn button(modifiers: Modifiers, button: u32) -> Self {
        Self { modifiers, key: BindKey::MouseButton(button) }
    }
}

/// Direction parameter for focus / movement actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction { Left, Right, Up, Down }

/// Action verbs the compositor responds to. Each variant carries its
/// parameters; the dispatcher matches on this and does the work.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    /// Spawn a process. argv[0] is the program; rest are args.
    Spawn(Vec<String>),
    /// Quit the compositor.
    Quit,
    /// Switch the active TTY (only meaningful on the udev backend).
    VtSwitch(i32),

    // Window control
    CloseWindow,
    /// Begin a compositor-initiated move grab on the window under the cursor.
    BeginMove,
    /// Begin a compositor-initiated resize grab on the window under the cursor.
    BeginResize,
    ToggleFullscreen,
    ToggleMaximize,
    /// Toggle this window's floating/tiled override state.
    ToggleFloating,

    // Workspaces
    SwitchToWorkspace(u32),
    MoveWindowToWorkspace(u32),
    /// Toggle the focused workspace's tiling mode.
    ToggleWorkspaceTiling,

    // Focus
    /// MRU forward (Alt+Tab).
    FocusNext,
    /// MRU backward (Alt+Shift+Tab).
    FocusPrev,
    /// Focus a window in a spatial direction.
    FocusDirection(Direction),
    /// Move the focused window in a spatial direction.
    MoveWindow(Direction),

    // Anvil debug knobs (preserved from existing KeyAction so we don't
    // regress the dev experience until they're replaced by zos-cli).
    ScaleUp,
    ScaleDown,
    RotateOutput,
    ToggleTint,
    TogglePreview,
    Screen(usize),

    // Utility
    /// No-op. Useful as a placeholder while binds.toml is being edited.
    Nop,
}

/// Hardcoded default bindings. Mirrors anvil's existing key handling
/// (Quit, Run, Screen 1..9, ScaleUp/Down, RotateOutput) plus zOS additions
/// (Super+Q close, Super+1..9 ws switch, Super+Shift+1..9 move-to-ws).
///
/// These are merged with `~/.config/zos-wm/binds.toml` at startup once the
/// TOML loader lands.
pub fn default_bindings() -> HashMap<KeyCombo, Action> {
    use Action::*;
    let mut m: HashMap<KeyCombo, Action> = HashMap::new();

    // --- Compositor lifecycle ---
    m.insert(KeyCombo::keysym(Modifiers::CTRL | Modifiers::ALT, Keysym::BackSpace), Quit);
    m.insert(KeyCombo::keysym(Modifiers::SUPER, Keysym::q),            CloseWindow);
    m.insert(KeyCombo::keysym(Modifiers::SUPER, Keysym::Return),       Spawn(vec!["wezterm".into()]));
    m.insert(KeyCombo::keysym(Modifiers::SUPER, Keysym::space),        Spawn(vec!["zos-launcher".into()]));
    m.insert(KeyCombo::keysym(Modifiers::SUPER, Keysym::v),            ToggleFloating);
    m.insert(KeyCombo::keysym(Modifiers::SUPER | Modifiers::SHIFT, Keysym::T), ToggleWorkspaceTiling);

    // --- VT switch (udev-only effect; on winit it's a Nop) ---
    for (i, sym) in (1..=12).zip([
        Keysym::XF86_Switch_VT_1, Keysym::XF86_Switch_VT_2, Keysym::XF86_Switch_VT_3,
        Keysym::XF86_Switch_VT_4, Keysym::XF86_Switch_VT_5, Keysym::XF86_Switch_VT_6,
        Keysym::XF86_Switch_VT_7, Keysym::XF86_Switch_VT_8, Keysym::XF86_Switch_VT_9,
        Keysym::XF86_Switch_VT_10, Keysym::XF86_Switch_VT_11, Keysym::XF86_Switch_VT_12,
    ]) {
        m.insert(KeyCombo::keysym(Modifiers::CTRL | Modifiers::ALT, sym), VtSwitch(i));
    }

    // --- Anvil debug carry-overs (CTRL+ALT+...) — preserved verbatim ---
    m.insert(KeyCombo::keysym(Modifiers::CTRL | Modifiers::ALT, Keysym::r), RotateOutput);
    m.insert(KeyCombo::keysym(Modifiers::CTRL | Modifiers::ALT, Keysym::p), ScaleUp);
    m.insert(KeyCombo::keysym(Modifiers::CTRL | Modifiers::ALT, Keysym::m), ScaleDown);
    m.insert(KeyCombo::keysym(Modifiers::CTRL | Modifiers::ALT, Keysym::t), ToggleTint);
    m.insert(KeyCombo::keysym(Modifiers::CTRL | Modifiers::ALT, Keysym::w), TogglePreview);
    for (i, sym) in (1..=9_usize).zip([
        Keysym::_1, Keysym::_2, Keysym::_3, Keysym::_4, Keysym::_5,
        Keysym::_6, Keysym::_7, Keysym::_8, Keysym::_9,
    ]) {
        m.insert(KeyCombo::keysym(Modifiers::CTRL | Modifiers::ALT, sym), Screen(i));
    }

    // --- Workspace switching (Super+1..9 + Super+Shift+1..9) ---
    for (i, sym) in (1..=9_u32).zip([
        Keysym::_1, Keysym::_2, Keysym::_3, Keysym::_4, Keysym::_5,
        Keysym::_6, Keysym::_7, Keysym::_8, Keysym::_9,
    ]) {
        m.insert(KeyCombo::keysym(Modifiers::SUPER, sym),                                SwitchToWorkspace(i));
        m.insert(KeyCombo::keysym(Modifiers::SUPER | Modifiers::SHIFT, sym),             MoveWindowToWorkspace(i));
    }

    // --- Focus cycling ---
    m.insert(KeyCombo::keysym(Modifiers::ALT, Keysym::Tab),                              FocusNext);
    m.insert(KeyCombo::keysym(Modifiers::ALT | Modifiers::SHIFT, Keysym::Tab),           FocusPrev);

    // --- Directional focus (Super+H/J/K/L) ---
    m.insert(KeyCombo::keysym(Modifiers::SUPER, Keysym::h), FocusDirection(Direction::Left));
    m.insert(KeyCombo::keysym(Modifiers::SUPER, Keysym::j), FocusDirection(Direction::Down));
    m.insert(KeyCombo::keysym(Modifiers::SUPER, Keysym::k), FocusDirection(Direction::Up));
    m.insert(KeyCombo::keysym(Modifiers::SUPER, Keysym::l), FocusDirection(Direction::Right));

    // --- Move window directionally (Super+Shift+H/J/K/L) ---
    m.insert(KeyCombo::keysym(Modifiers::SUPER | Modifiers::SHIFT, Keysym::H), MoveWindow(Direction::Left));
    m.insert(KeyCombo::keysym(Modifiers::SUPER | Modifiers::SHIFT, Keysym::J), MoveWindow(Direction::Down));
    m.insert(KeyCombo::keysym(Modifiers::SUPER | Modifiers::SHIFT, Keysym::K), MoveWindow(Direction::Up));
    m.insert(KeyCombo::keysym(Modifiers::SUPER | Modifiers::SHIFT, Keysym::L), MoveWindow(Direction::Right));

    // --- Window state toggles ---
    m.insert(KeyCombo::keysym(Modifiers::SUPER, Keysym::f), ToggleFullscreen);
    m.insert(KeyCombo::keysym(Modifiers::SUPER, Keysym::F), ToggleMaximize);

    // --- Mouse binds (Super+LMB / Super+RMB) ---
    // BTN_LEFT = 0x110 = 272, BTN_RIGHT = 0x111 = 273
    m.insert(KeyCombo::button(Modifiers::SUPER, 272), BeginMove);
    m.insert(KeyCombo::button(Modifiers::SUPER, 273), BeginResize);

    m
}

// ============================================================================
// User TOML loader: ~/.config/zos/binds.toml
// ----------------------------------------------------------------------------
// Users can add or override keybinds without recompiling. The file format is:
//
//     [[bind]]
//     mods = ["SUPER", "SHIFT"]
//     key = "1"                # OR button = 274
//     action = "MoveWindowToWorkspace"
//     args = [1]               # action-dependent
//
// Parse failures (missing file, malformed entries, unknown actions) are logged
// at WARN level and the affected entries are skipped — the compositor still
// boots with the surviving defaults + valid user entries.
// ============================================================================

/// One [[bind]] entry from binds.toml.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UserBindEntry {
    #[serde(default)]
    pub mods:   Vec<String>,
    pub key:    Option<String>,
    pub button: Option<u32>,
    pub action: String,
    #[serde(default)]
    pub args:   Vec<toml::Value>,
}

/// Top-level structure of binds.toml.
#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct UserBinds {
    #[serde(default, rename = "bind")]
    pub binds: Vec<UserBindEntry>,
}

/// Resolve `~/.config/zos/binds.toml`, honoring `XDG_CONFIG_HOME` if set.
pub fn user_binds_path() -> std::path::PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return std::path::PathBuf::from(xdg).join("zos/binds.toml");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    std::path::PathBuf::from(home).join(".config/zos/binds.toml")
}

/// Read + parse `~/.config/zos/binds.toml` and return its (combo, action)
/// entries. On any error (missing file, bad TOML, malformed entry) returns
/// what could be parsed and logs a warning for the rest.
pub fn load_user_bindings() -> Vec<(KeyCombo, Action)> {
    let path = user_binds_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = ?e, "failed to read binds.toml");
            return Vec::new();
        }
    };
    let parsed: UserBinds = match toml::from_str(&content) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "failed to parse binds.toml");
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    for entry in parsed.binds {
        match parse_user_bind(&entry) {
            Ok(combo_action) => out.push(combo_action),
            Err(e) => {
                tracing::warn!(error = %e, "skipping malformed bind in binds.toml");
            }
        }
    }
    tracing::info!(count = out.len(), path = %path.display(), "loaded user bindings");
    out
}

fn parse_user_bind(entry: &UserBindEntry) -> Result<(KeyCombo, Action), String> {
    let mods = parse_modifiers(&entry.mods)?;
    let bind_key = match (&entry.key, entry.button) {
        (Some(k), None)    => BindKey::Keysym(parse_keysym(k)?),
        (None, Some(b))    => BindKey::MouseButton(b),
        (Some(_), Some(_)) => return Err("bind has both `key` and `button`".into()),
        (None, None)       => return Err("bind has neither `key` nor `button`".into()),
    };
    let action = parse_action(&entry.action, &entry.args)?;
    Ok((KeyCombo::new(mods, bind_key), action))
}

fn parse_modifiers(strs: &[String]) -> Result<Modifiers, String> {
    let mut m = Modifiers::empty();
    for s in strs {
        match s.to_uppercase().as_str() {
            "SHIFT"                => m |= Modifiers::SHIFT,
            "CTRL" | "CONTROL"     => m |= Modifiers::CTRL,
            "ALT"                  => m |= Modifiers::ALT,
            "SUPER" | "LOGO" | "MOD" => m |= Modifiers::SUPER,
            "ALTGR"                => m |= Modifiers::ALTGR,
            other                  => return Err(format!("unknown modifier: {}", other)),
        }
    }
    Ok(m)
}

fn parse_keysym(s: &str) -> Result<Keysym, String> {
    use smithay::input::keyboard::xkb;
    let sym = xkb::keysym_from_name(s, xkb::KEYSYM_NO_FLAGS);
    // KEY_NoSymbol == 0 — keysym_from_name returns this for unknown names.
    if sym.raw() == 0 {
        return Err(format!("unknown keysym: {}", s));
    }
    Ok(sym)
}

fn parse_action(name: &str, args: &[toml::Value]) -> Result<Action, String> {
    use Action::*;
    match name {
        "Quit"                  => Ok(Quit),
        "CloseWindow"           => Ok(CloseWindow),
        "BeginMove"             => Ok(BeginMove),
        "BeginResize"           => Ok(BeginResize),
        "ToggleFullscreen"      => Ok(ToggleFullscreen),
        "ToggleMaximize"        => Ok(ToggleMaximize),
        "ToggleFloating"        => Ok(ToggleFloating),
        "ToggleWorkspaceTiling" => Ok(ToggleWorkspaceTiling),
        "FocusNext"             => Ok(FocusNext),
        "FocusPrev"             => Ok(FocusPrev),
        "ScaleUp"               => Ok(ScaleUp),
        "ScaleDown"             => Ok(ScaleDown),
        "RotateOutput"          => Ok(RotateOutput),
        "ToggleTint"            => Ok(ToggleTint),
        "TogglePreview"         => Ok(TogglePreview),
        "Nop"                   => Ok(Nop),
        "Spawn" => {
            let argv: Vec<String> = args
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if argv.is_empty() {
                return Err("Spawn needs at least one string arg".into());
            }
            Ok(Spawn(argv))
        }
        "VtSwitch" => {
            let n = args
                .first()
                .and_then(|v| v.as_integer())
                .ok_or("VtSwitch needs an integer arg")?;
            Ok(VtSwitch(n as i32))
        }
        "SwitchToWorkspace" => {
            let id = args
                .first()
                .and_then(|v| v.as_integer())
                .ok_or("SwitchToWorkspace needs an integer arg")?;
            Ok(SwitchToWorkspace(id as u32))
        }
        "MoveWindowToWorkspace" => {
            let id = args
                .first()
                .and_then(|v| v.as_integer())
                .ok_or("MoveWindowToWorkspace needs an integer arg")?;
            Ok(MoveWindowToWorkspace(id as u32))
        }
        "Screen" => {
            let n = args
                .first()
                .and_then(|v| v.as_integer())
                .ok_or("Screen needs an integer arg")?;
            Ok(Screen(n as usize))
        }
        "FocusDirection" => {
            let dir = args
                .first()
                .and_then(|v| v.as_str())
                .ok_or("FocusDirection needs a string arg")?;
            Ok(FocusDirection(parse_direction(dir)?))
        }
        "MoveWindow" => {
            let dir = args
                .first()
                .and_then(|v| v.as_str())
                .ok_or("MoveWindow needs a string arg")?;
            Ok(MoveWindow(parse_direction(dir)?))
        }
        other => Err(format!("unknown action: {}", other)),
    }
}

fn parse_direction(s: &str) -> Result<Direction, String> {
    match s.to_lowercase().as_str() {
        "left"  => Ok(Direction::Left),
        "right" => Ok(Direction::Right),
        "up"    => Ok(Direction::Up),
        "down"  => Ok(Direction::Down),
        other   => Err(format!("unknown direction: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toml_yields_empty_binds() {
        let parsed: UserBinds = toml::from_str("").expect("empty TOML deserializes");
        assert!(parsed.binds.is_empty());
    }

    #[test]
    fn parses_spawn_and_integer_arg_actions() {
        let src = r#"
            [[bind]]
            mods = ["SUPER"]
            key = "e"
            action = "Spawn"
            args = ["nautilus"]

            [[bind]]
            mods = ["SUPER", "SHIFT"]
            key = "1"
            action = "MoveWindowToWorkspace"
            args = [1]

            [[bind]]
            mods = ["SUPER"]
            button = 274
            action = "ToggleFloating"
        "#;
        let parsed: UserBinds = toml::from_str(src).expect("valid TOML");
        assert_eq!(parsed.binds.len(), 3);

        let entries: Vec<(KeyCombo, Action)> = parsed
            .binds
            .iter()
            .map(|e| parse_user_bind(e).expect("valid bind"))
            .collect();

        // Spawn
        assert_eq!(entries[0].0.modifiers, Modifiers::SUPER);
        assert!(matches!(entries[0].0.key, BindKey::Keysym(_)));
        assert_eq!(entries[0].1, Action::Spawn(vec!["nautilus".into()]));

        // MoveWindowToWorkspace(1)
        assert_eq!(
            entries[1].0.modifiers,
            Modifiers::SUPER | Modifiers::SHIFT
        );
        assert_eq!(entries[1].1, Action::MoveWindowToWorkspace(1));

        // Mouse button 274 -> ToggleFloating
        assert_eq!(entries[2].0.key, BindKey::MouseButton(274));
        assert_eq!(entries[2].1, Action::ToggleFloating);
    }

    #[test]
    fn rejects_unknown_action_modifier_and_direction() {
        // Unknown action.
        let bad_action = UserBindEntry {
            mods:   vec!["SUPER".into()],
            key:    Some("a".into()),
            button: None,
            action: "FlipMonitor".into(),
            args:   vec![],
        };
        assert!(parse_user_bind(&bad_action).is_err());

        // Unknown modifier.
        let bad_mod = UserBindEntry {
            mods:   vec!["HYPER".into()],
            key:    Some("a".into()),
            button: None,
            action: "Quit".into(),
            args:   vec![],
        };
        assert!(parse_user_bind(&bad_mod).is_err());

        // Unknown direction inside FocusDirection.
        let bad_dir = UserBindEntry {
            mods:   vec!["SUPER".into()],
            key:    Some("a".into()),
            button: None,
            action: "FocusDirection".into(),
            args:   vec![toml::Value::String("backward".into())],
        };
        assert!(parse_user_bind(&bad_dir).is_err());

        // Both key and button -> error.
        let both = UserBindEntry {
            mods:   vec!["SUPER".into()],
            key:    Some("a".into()),
            button: Some(272),
            action: "Quit".into(),
            args:   vec![],
        };
        assert!(parse_user_bind(&both).is_err());

        // Neither key nor button -> error.
        let neither = UserBindEntry {
            mods:   vec!["SUPER".into()],
            key:    None,
            button: None,
            action: "Quit".into(),
            args:   vec![],
        };
        assert!(parse_user_bind(&neither).is_err());
    }

    #[test]
    fn round_trip_sample_binds_toml() {
        let src = r#"
            [[bind]]
            mods = ["SUPER"]
            key = "Tab"
            action = "FocusNext"

            [[bind]]
            mods = ["SUPER"]
            key = "h"
            action = "FocusDirection"
            args = ["left"]

            [[bind]]
            mods = ["CTRL", "ALT"]
            key = "BackSpace"
            action = "Quit"

            [[bind]]
            mods = ["SUPER"]
            key = "Return"
            action = "Spawn"
            args = ["wezterm", "--config", "font_size=14"]
        "#;
        let parsed: UserBinds = toml::from_str(src).expect("valid TOML");
        let entries: Vec<(KeyCombo, Action)> = parsed
            .binds
            .iter()
            .map(|e| parse_user_bind(e).expect("valid bind"))
            .collect();
        assert_eq!(entries.len(), 4);

        // FocusNext on Super+Tab.
        assert_eq!(entries[0].1, Action::FocusNext);
        assert_eq!(entries[0].0.modifiers, Modifiers::SUPER);

        // FocusDirection(Left) on Super+h.
        assert_eq!(entries[1].1, Action::FocusDirection(Direction::Left));

        // Quit on Ctrl+Alt+BackSpace.
        assert_eq!(entries[2].1, Action::Quit);
        assert_eq!(
            entries[2].0.modifiers,
            Modifiers::CTRL | Modifiers::ALT
        );

        // Spawn carries every string arg verbatim.
        assert_eq!(
            entries[3].1,
            Action::Spawn(vec![
                "wezterm".into(),
                "--config".into(),
                "font_size=14".into(),
            ])
        );
    }
}
