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

    /// Apply overrides loaded from `zos_ui::config::load_animations()` on
    /// top of the defaults. Missing fields in the override leave defaults
    /// in place. Curve names referenced by string ("overshot", "default",
    /// "smoothOut", custom) resolve against `self.curves`.
    pub fn with_overrides(mut self, overrides: zos_ui::config::AnimationOverrides) -> Self {
        if let Some(global) = overrides.global_enabled {
            self.global_enabled = global;
        }

        // Register custom curves from TOML before resolving property curves
        // so a property override can reference a freshly-defined curve.
        for (name, c) in &overrides.curves {
            self.curves.insert(
                name.clone(),
                BezierCurve::new((c.p1[0], c.p1[1]), (c.p2[0], c.p2[1])),
            );
        }

        let curves = self.curves.clone();
        apply_property_override(&mut self.windows_in, &overrides.windows_in, &curves);
        apply_property_override(&mut self.windows_out, &overrides.windows_out, &curves);
        apply_property_override(&mut self.fade_in, &overrides.fade_in, &curves);
        apply_property_override(&mut self.fade_out, &overrides.fade_out, &curves);
        apply_property_override(&mut self.workspaces, &overrides.workspaces, &curves);

        self
    }
}

fn apply_property_override(
    prop: &mut AnimationProperty,
    over: &Option<zos_ui::config::PropertyOverride>,
    curves: &HashMap<String, BezierCurve>,
) {
    let Some(over) = over else {
        return;
    };
    if let Some(speed) = over.speed {
        prop.speed = speed;
    }
    if let Some(enabled) = over.enabled {
        prop.enabled = enabled;
    }
    if let Some(curve_name) = &over.curve {
        if let Some(curve) = curves.get(curve_name) {
            prop.curve = curve.clone();
        } else {
            tracing::warn!(
                curve = %curve_name,
                "TOML override referenced unknown curve; keeping default"
            );
        }
    }
    if let Some(style) = &over.style {
        prop.style = match style.as_str() {
            "slide" => AnimationStyle::Slide,
            "popin" | "popIn" => AnimationStyle::PopIn,
            "fade" => AnimationStyle::Fade,
            "none" => AnimationStyle::None,
            _ => {
                tracing::warn!(
                    style = %style,
                    "TOML override referenced unknown style; keeping default"
                );
                prop.style
            }
        };
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

    #[test]
    fn empty_overrides_leave_defaults_intact() {
        let baseline = AnimationManager::new();
        let overridden =
            AnimationManager::new().with_overrides(zos_ui::config::AnimationOverrides::default());

        assert_eq!(overridden.global_enabled, baseline.global_enabled);
        assert_eq!(overridden.windows_in.speed, baseline.windows_in.speed);
        assert_eq!(overridden.windows_in.style, baseline.windows_in.style);
        assert_eq!(overridden.windows_in.enabled, baseline.windows_in.enabled);
        assert_eq!(overridden.windows_out.speed, baseline.windows_out.speed);
        assert_eq!(overridden.fade_in.speed, baseline.fade_in.speed);
        assert_eq!(overridden.fade_out.speed, baseline.fade_out.speed);
        assert_eq!(overridden.workspaces.speed, baseline.workspaces.speed);
        // Default-curve set is preserved.
        assert!(overridden.curve("overshot").is_some());
    }

    #[test]
    fn speed_override_applied() {
        let overrides = zos_ui::config::AnimationOverrides {
            windows_in: Some(zos_ui::config::PropertyOverride {
                speed: Some(9.5),
                ..Default::default()
            }),
            ..Default::default()
        };
        let m = AnimationManager::new().with_overrides(overrides);
        assert_eq!(m.windows_in.speed, 9.5);
        // Other properties untouched.
        assert_eq!(m.windows_out.speed, 4.0);
    }

    #[test]
    fn enabled_and_global_overrides_applied() {
        let overrides = zos_ui::config::AnimationOverrides {
            global_enabled: Some(false),
            workspaces: Some(zos_ui::config::PropertyOverride {
                enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let m = AnimationManager::new().with_overrides(overrides);
        assert!(!m.global_enabled);
        assert!(!m.workspaces.enabled);
        // Other props still enabled (default).
        assert!(m.windows_in.enabled);
    }

    #[test]
    fn curve_name_override_resolves_builtin() {
        let overrides = zos_ui::config::AnimationOverrides {
            windows_in: Some(zos_ui::config::PropertyOverride {
                curve: Some("smoothOut".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let baseline_smooth_out = BezierCurve::smooth_out();
        let m = AnimationManager::new().with_overrides(overrides);
        // The chosen curve should now match `smoothOut`.
        assert_eq!(m.windows_in.curve.p1(), baseline_smooth_out.p1());
        assert_eq!(m.windows_in.curve.p2(), baseline_smooth_out.p2());
    }

    #[test]
    fn unknown_curve_keeps_default() {
        let baseline = AnimationManager::new();
        let overrides = zos_ui::config::AnimationOverrides {
            windows_in: Some(zos_ui::config::PropertyOverride {
                curve: Some("doesNotExist".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let m = AnimationManager::new().with_overrides(overrides);
        // Curve unchanged from default (overshot).
        assert_eq!(m.windows_in.curve.p1(), baseline.windows_in.curve.p1());
        assert_eq!(m.windows_in.curve.p2(), baseline.windows_in.curve.p2());
    }

    #[test]
    fn custom_curve_registered_then_referenced() {
        let mut curves = HashMap::new();
        curves.insert(
            "myFlat".to_string(),
            zos_ui::config::BezierCurveOverride {
                p1: [0.1, 0.2],
                p2: [0.3, 0.4],
            },
        );
        let overrides = zos_ui::config::AnimationOverrides {
            curves,
            windows_in: Some(zos_ui::config::PropertyOverride {
                curve: Some("myFlat".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let m = AnimationManager::new().with_overrides(overrides);
        assert!(m.curve("myFlat").is_some());
        let expected = BezierCurve::new((0.1, 0.2), (0.3, 0.4));
        assert_eq!(m.windows_in.curve.p1(), expected.p1());
        assert_eq!(m.windows_in.curve.p2(), expected.p2());
    }

    #[test]
    fn style_override_applied_and_unknown_kept() {
        let overrides = zos_ui::config::AnimationOverrides {
            windows_in: Some(zos_ui::config::PropertyOverride {
                style: Some("popin".into()),
                ..Default::default()
            }),
            windows_out: Some(zos_ui::config::PropertyOverride {
                style: Some("not-a-real-style".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let baseline = AnimationManager::new();
        let m = AnimationManager::new().with_overrides(overrides);
        assert_eq!(m.windows_in.style, AnimationStyle::PopIn);
        // Unknown style leaves default in place.
        assert_eq!(m.windows_out.style, baseline.windows_out.style);
    }
}
