//! Animation overrides loaded from `~/.config/zos/animations.toml`.
//!
//! TOML shape:
//!
//! ```toml
//! global_enabled = true
//!
//! [curves]
//! my_curve = { p1 = [0.0, 0.5], p2 = [0.3, 1.0] }
//!
//! [windows_in]
//! speed = 5.0
//! curve = "overshot"
//! enabled = true
//!
//! [windows_out]
//! speed = 4.0
//! curve = "smoothOut"
//! ```
//!
//! All fields are optional. Missing fields fall through to whatever
//! defaults the consumer (e.g. `zos-wm`'s `AnimationManager`) provides.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Top-level shape of `animations.toml`.
///
/// Use `AnimationOverrides::default()` for "no overrides at all" — every
/// field is `Option<_>` or a `HashMap` that's empty by default.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AnimationOverrides {
    /// Master switch — `Some(false)` disables every animation regardless of
    /// individual property settings. `None` means "don't override".
    pub global_enabled: Option<bool>,
    /// User-defined named bezier curves. Keys are referenced by
    /// `PropertyOverride::curve`.
    #[serde(default)]
    pub curves: HashMap<String, BezierCurveOverride>,
    pub windows_in: Option<PropertyOverride>,
    pub windows_out: Option<PropertyOverride>,
    pub fade_in: Option<PropertyOverride>,
    pub fade_out: Option<PropertyOverride>,
    pub workspaces: Option<PropertyOverride>,
}

/// Two control points of a cubic bezier (`p0=(0,0)`, `p3=(1,1)` are
/// implicit). Mirrors Hyprland's `bezier` config syntax.
#[derive(Debug, Clone, Deserialize)]
pub struct BezierCurveOverride {
    pub p1: [f32; 2],
    pub p2: [f32; 2],
}

/// Per-animation-property override. Each field is independently optional
/// so a user can override `speed` without disturbing `curve`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PropertyOverride {
    /// Animation duration multiplier (Hyprland-style — higher = faster).
    pub speed: Option<f32>,
    /// Name of a curve, either a built-in or a key from `[curves]`.
    pub curve: Option<String>,
    pub enabled: Option<bool>,
    /// Optional style hint (e.g. `"slide"`, `"popin"`); semantics are
    /// owned by the consumer.
    pub style: Option<String>,
}

/// Load `<config_dir>/animations.toml`. On missing or malformed file,
/// returns empty overrides + logs a warning via `tracing`.
pub fn load_animations() -> AnimationOverrides {
    let path = super::config_dir().join("animations.toml");
    load_animations_from(&path)
}

/// Load animations from a specific path. Useful for tests and for callers
/// that want to honour a `--config` flag.
pub fn load_animations_from(path: &Path) -> AnimationOverrides {
    match std::fs::read_to_string(path) {
        Ok(content) => match toml::from_str::<AnimationOverrides>(&content) {
            Ok(overrides) => {
                tracing::debug!(path = %path.display(), "loaded animation overrides");
                overrides
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = ?e,
                    "failed to parse animations.toml; using defaults"
                );
                AnimationOverrides::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => AnimationOverrides::default(),
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = ?e,
                "failed to read animations.toml; using defaults"
            );
            AnimationOverrides::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toml_deserializes_to_default() {
        let parsed: AnimationOverrides = toml::from_str("").expect("empty TOML must parse");
        assert!(parsed.global_enabled.is_none());
        assert!(parsed.curves.is_empty());
        assert!(parsed.windows_in.is_none());
        assert!(parsed.windows_out.is_none());
        assert!(parsed.fade_in.is_none());
        assert!(parsed.fade_out.is_none());
        assert!(parsed.workspaces.is_none());
    }

    #[test]
    fn doc_example_parses() {
        let toml_src = r#"
global_enabled = true

[curves]
my_curve = { p1 = [0.0, 0.5], p2 = [0.3, 1.0] }

[windows_in]
speed = 5.0
curve = "overshot"
enabled = true

[windows_out]
speed = 4.0
curve = "smoothOut"
"#;
        let parsed: AnimationOverrides =
            toml::from_str(toml_src).expect("doc-comment example must parse");

        assert_eq!(parsed.global_enabled, Some(true));

        let curve = parsed
            .curves
            .get("my_curve")
            .expect("my_curve must be present");
        assert_eq!(curve.p1, [0.0, 0.5]);
        assert_eq!(curve.p2, [0.3, 1.0]);

        let win_in = parsed.windows_in.expect("windows_in must be present");
        assert_eq!(win_in.speed, Some(5.0));
        assert_eq!(win_in.curve.as_deref(), Some("overshot"));
        assert_eq!(win_in.enabled, Some(true));
        assert!(win_in.style.is_none());

        let win_out = parsed.windows_out.expect("windows_out must be present");
        assert_eq!(win_out.speed, Some(4.0));
        assert_eq!(win_out.curve.as_deref(), Some("smoothOut"));
        assert!(win_out.enabled.is_none());
    }

    #[test]
    fn missing_file_returns_default() {
        let overrides = load_animations_from(Path::new(
            "/this/path/should/not/exist/animations.toml",
        ));
        assert!(overrides.global_enabled.is_none());
        assert!(overrides.curves.is_empty());
        assert!(overrides.windows_in.is_none());
    }

    #[test]
    fn malformed_file_returns_default() {
        let dir = std::env::temp_dir().join(format!(
            "zos-ui-animations-malformed-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("animations.toml");
        std::fs::write(&path, "this is = not [valid toml").unwrap();

        let overrides = load_animations_from(&path);
        assert!(overrides.global_enabled.is_none());
        assert!(overrides.windows_in.is_none());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
