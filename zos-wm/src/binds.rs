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
