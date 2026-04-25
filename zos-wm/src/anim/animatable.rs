//! Trait for types that can be linearly interpolated by an `AnimatedValue`.
//!
//! The trait is intentionally minimal: only `lerp(a, b, t)`. Bezier easing is
//! applied in `AnimatedValue::tick` *before* `lerp` is called, so `t` here is
//! already the post-easing fraction in (typically) `[0, 1]`. Implementations
//! must allow `t` outside `[0, 1]` because curves like `overshot` produce
//! eased fractions slightly past the endpoints.
//!
//! See `docs/research/phase-4-hyprland-animations.md` §7 for design.

use smithay::utils::{Logical, Point};

/// Types that can be linearly interpolated for animation purposes.
pub trait Animatable: Clone + Copy + std::fmt::Debug + 'static {
    /// Linear interpolation from `a` to `b` by `t`. `t` is the post-easing
    /// fraction; `t == 0.0` returns `a`, `t == 1.0` returns `b`. Values of
    /// `t` outside `[0, 1]` are valid (e.g. for overshoot curves).
    fn lerp(a: Self, b: Self, t: f32) -> Self;
}

impl Animatable for f32 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
}

impl Animatable for Point<f64, Logical> {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t as f64;
        (a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t).into()
    }
}

/// Premultiplied RGBA color for borders / surface tint. OkLab interpolation
/// for color is a future task; this is plain linear-RGBA lerp for now.
impl Animatable for [f32; 4] {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
            a[3] + (b[3] - a[3]) * t,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_lerp_midpoint() {
        assert_eq!(<f32 as Animatable>::lerp(0.0, 100.0, 0.5), 50.0);
        assert_eq!(<f32 as Animatable>::lerp(0.0, 100.0, 0.0), 0.0);
        assert_eq!(<f32 as Animatable>::lerp(0.0, 100.0, 1.0), 100.0);
    }

    #[test]
    fn f32_lerp_overshoot_allowed() {
        // Overshoot curves can produce t > 1.0; lerp must extrapolate.
        let v = <f32 as Animatable>::lerp(0.0, 10.0, 1.1);
        assert!((v - 11.0).abs() < 1e-5);
    }

    #[test]
    fn point_lerp_endpoints() {
        let a: Point<f64, Logical> = (0.0, 0.0).into();
        let b: Point<f64, Logical> = (100.0, 200.0).into();
        let mid = Point::<f64, Logical>::lerp(a, b, 0.5);
        assert_eq!(mid, Point::<f64, Logical>::from((50.0, 100.0)));
        assert_eq!(Point::<f64, Logical>::lerp(a, b, 1.0), b);
        assert_eq!(Point::<f64, Logical>::lerp(a, b, 0.0), a);
    }

    #[test]
    fn rgba_lerp_componentwise() {
        let a = [0.0_f32, 0.0, 0.0, 0.0];
        let b = [1.0_f32, 0.5, 0.25, 1.0];
        let mid = <[f32; 4] as Animatable>::lerp(a, b, 0.5);
        assert!((mid[0] - 0.5).abs() < 1e-5);
        assert!((mid[1] - 0.25).abs() < 1e-5);
        assert!((mid[2] - 0.125).abs() < 1e-5);
        assert!((mid[3] - 0.5).abs() < 1e-5);
    }
}
