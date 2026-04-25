// === zos-settings theme — re-exports from zos-ui (Catppuccin Mocha) ===

// Full re-export shim retained intentionally for P5 (zos-ui adoption) — keeping
// every Catppuccin Mocha token reachable here so downstream pages can switch
// imports incrementally without touching this file again.
#[allow(unused_imports)]
pub use zos_ui::theme::{
    zos_theme, BASE, BLUE, CRUST, GREEN, LAVENDER, MANTLE, MAUVE, OVERLAY0, OVERLAY1, OVERLAY2,
    PEACH, PINK, RED, SUBTEXT0, SUBTEXT1, SURFACE0, SURFACE1, SURFACE2, TEXT, YELLOW,
};
