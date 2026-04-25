//! Registry of named curves + default property config.
//!
//! `AnimationManager` holds the named bezier curves and per-property
//! configuration (speed, curve, style, enabled) for window/workspace/fade
//! animations. Mirrors Hyprland's `CAnimationManager` + `AnimationTree`,
//! flattened: parent inheritance is not implemented for v1 (defaults are
//! sufficient and TOML config parsing is deferred to a later task).
//!
//! See `docs/research/phase-4-hyprland-animations.md` §2.1, §7, §8 for
//! design.

use std::collections::HashMap;
use std::time::Duration;

use super::bezier::BezierCurve;

/// Per-property animation config (mirrors `SAnimationPropertyConfig`).
#[derive(Debug, Clone)]
pub struct AnimationProperty {
    /// Hyprland-style speed multiplier. Bigger = slower. Duration in
    /// milliseconds is `speed * 100`, clamped to `[50, 5000]`.
    pub speed: f32,
    /// Easing curve for this property.
    pub curve: BezierCurve,
    /// Animation style (slide / popin / fade / none).
    pub style: AnimationStyle,
    /// Whether the animation is enabled. When false, transitions warp.
    pub enabled: bool,
}

/// Animation style for a property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationStyle {
    /// Slide from the nearest screen edge to the goal rect.
    Slide,
    /// Grow from a small centered rect to the goal rect.
    PopIn,
    /// Cross-fade only; position/size warp instantly.
    Fade,
    /// No animation.
    None,
}

impl AnimationProperty {
    /// Compute the duration in `Duration` from `speed`. Hyprland convention:
    /// `duration_ms = speed * 100`. Clamped to `[50, 5000]ms` to avoid
    /// pathological config values.
    pub fn duration(&self) -> Duration {
        let ms = (self.speed * 100.0).clamp(50.0, 5000.0);
        Duration::from_millis(ms as u64)
    }
}

#[derive(Debug, Clone)]
pub struct AnimationManager {
    /// Named bezier curves, addressable by name (e.g. `"overshot"`).
    pub curves: HashMap<String, BezierCurve>,
    /// Window-open animation property.
    pub windows_in: AnimationProperty,
    /// Window-close animation property.
    pub windows_out: AnimationProperty,
    /// Window/surface fade-in property.
    pub fade_in: AnimationProperty,
    /// Window/surface fade-out property.
    pub fade_out: AnimationProperty,
    /// Workspace-switch animation property.
    pub workspaces: AnimationProperty,
    /// Master enable flag — when false, all animations are disabled.
    pub global_enabled: bool,
}

impl Default for AnimationManager {
    fn default() -> Self {
        let mut curves = HashMap::new();
        curves.insert("linear".into(), BezierCurve::linear());
        curves.insert("default".into(), BezierCurve::default());
        curves.insert("overshot".into(), BezierCurve::overshot());
        curves.insert("smoothOut".into(), BezierCurve::smooth_out());
        curves.insert("smoothIn".into(), BezierCurve::smooth_in());

        Self {
            curves,
            windows_in: AnimationProperty {
                speed: 5.0,
                curve: BezierCurve::overshot(),
                style: AnimationStyle::Slide,
                enabled: true,
            },
            windows_out: AnimationProperty {
                speed: 4.0,
                curve: BezierCurve::smooth_out(),
                style: AnimationStyle::Slide,
                enabled: true,
            },
            fade_in: AnimationProperty {
                speed: 5.0,
                curve: BezierCurve::smooth_in(),
                style: AnimationStyle::Fade,
                enabled: true,
            },
            fade_out: AnimationProperty {
                speed: 5.0,
                curve: BezierCurve::smooth_in(),
                style: AnimationStyle::Fade,
                enabled: true,
            },
            workspaces: AnimationProperty {
                speed: 6.0,
                curve: BezierCurve::default(),
                style: AnimationStyle::Slide,
                enabled: true,
            },
            global_enabled: true,
        }
    }
}

impl AnimationManager {
    /// Construct a manager with the zOS default curve set + default property
    /// bindings. See `docs/research/phase-4-hyprland-animations.md` §8 for
    /// the values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a curve by name. Returns `None` if no such curve is
    /// registered.
    pub fn curve(&self, name: &str) -> Option<&BezierCurve> {
        self.curves.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_named_curves() {
        let m = AnimationManager::new();
        assert!(m.curve("linear").is_some());
        assert!(m.curve("default").is_some());
        assert!(m.curve("overshot").is_some());
        assert!(m.curve("smoothOut").is_some());
        assert!(m.curve("smoothIn").is_some());
        assert!(m.curve("nonexistent").is_none());
    }

    #[test]
    fn duration_follows_hyprland_formula() {
        let p = AnimationProperty {
            speed: 5.0,
            curve: BezierCurve::linear(),
            style: AnimationStyle::Slide,
            enabled: true,
        };
        assert_eq!(p.duration(), Duration::from_millis(500));
    }

    #[test]
    fn duration_clamps_extreme_speeds() {
        let too_fast = AnimationProperty {
            speed: 0.0,
            curve: BezierCurve::linear(),
            style: AnimationStyle::Slide,
            enabled: true,
        };
        assert_eq!(too_fast.duration(), Duration::from_millis(50));

        let too_slow = AnimationProperty {
            speed: 1000.0,
            curve: BezierCurve::linear(),
            style: AnimationStyle::Slide,
            enabled: true,
        };
        assert_eq!(too_slow.duration(), Duration::from_millis(5000));
    }

    #[test]
    fn defaults_match_documented_bindings() {
        // From docs/research/phase-4-hyprland-animations.md §8.
        let m = AnimationManager::new();
        assert_eq!(m.windows_in.speed, 5.0);
        assert_eq!(m.windows_in.style, AnimationStyle::Slide);
        assert_eq!(m.windows_out.speed, 4.0);
        assert_eq!(m.fade_in.speed, 5.0);
        assert_eq!(m.fade_out.speed, 5.0);
        assert_eq!(m.workspaces.speed, 6.0);
        assert!(m.global_enabled);
    }
}
