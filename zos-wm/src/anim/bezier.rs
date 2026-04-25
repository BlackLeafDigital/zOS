//! Cubic Bezier curve with 2 control points (P0=(0,0), P3=(1,1) implicit).
//!
//! Hyprland-style: bake 255 evenly-spaced (x, y) samples at construction
//! time, then per-frame `y_for_x(t)` does a binary search to find the
//! bracketing samples and linearly interpolates between them.
//!
//! See `docs/research/phase-4-hyprland-animations.md` §3 for design.

const BAKE_SAMPLES: usize = 255;

#[derive(Debug, Clone)]
pub struct BezierCurve {
    /// Control point P1 (P0 = (0, 0) is implicit).
    p1: (f32, f32),
    /// Control point P2 (P3 = (1, 1) is implicit).
    p2: (f32, f32),
    /// Pre-baked (x, y) samples at parameter `u` evenly spaced in [0, 1].
    samples: Vec<(f32, f32)>,
}

impl BezierCurve {
    /// Construct a new cubic Bezier curve with control points P1 and P2.
    /// P0 = (0, 0) and P3 = (1, 1) are implicit. Bakes 255 samples on
    /// construction; this is a one-time cost per curve.
    pub fn new(p1: (f32, f32), p2: (f32, f32)) -> Self {
        let mut samples = Vec::with_capacity(BAKE_SAMPLES);
        for i in 0..BAKE_SAMPLES {
            let u = i as f32 / (BAKE_SAMPLES - 1) as f32;
            samples.push((Self::eval_x(u, p1, p2), Self::eval_y(u, p1, p2)));
        }
        Self { p1, p2, samples }
    }

    /// Linear curve: y == x.
    pub fn linear() -> Self {
        Self::new((0.0, 0.0), (1.0, 1.0))
    }

    /// Hyprland's default ease-out: snappy decel.
    pub fn default() -> Self {
        Self::new((0.0, 0.75), (0.15, 1.0))
    }

    /// Past-the-target spring-back curve.
    pub fn overshot() -> Self {
        Self::new((0.05, 0.9), (0.1, 1.05))
    }

    /// Smooth-out curve (used for window-out animations).
    pub fn smooth_out() -> Self {
        Self::new((0.36, 0.0), (0.66, -0.56))
    }

    /// Smooth-in curve (used for fade animations).
    pub fn smooth_in() -> Self {
        Self::new((0.25, 1.0), (0.5, 1.0))
    }

    /// Returns the curve-mapped y given progress `x` in `[0, 1]`.
    /// Uses binary search over the baked samples (~8 iterations) plus a
    /// linear interp between bracketing samples.
    pub fn y_for_x(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let mut lo = 0usize;
        let mut hi = self.samples.len() - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self.samples[mid].0 < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (x0, y0) = self.samples[lo];
        let (x1, y1) = self.samples[hi];
        if (x1 - x0).abs() < f32::EPSILON {
            return y0;
        }
        let t = (x - x0) / (x1 - x0);
        y0 + t * (y1 - y0)
    }

    /// Returns the configured P1 control point.
    pub fn p1(&self) -> (f32, f32) {
        self.p1
    }

    /// Returns the configured P2 control point.
    pub fn p2(&self) -> (f32, f32) {
        self.p2
    }

    /// Cubic Bezier x-coord at parameter `u`, with P0=(0,0) and P3=(1,1).
    fn eval_x(u: f32, p1: (f32, f32), p2: (f32, f32)) -> f32 {
        let inv = 1.0 - u;
        3.0 * inv * inv * u * p1.0 + 3.0 * inv * u * u * p2.0 + u * u * u
    }

    /// Cubic Bezier y-coord at parameter `u`, with P0=(0,0) and P3=(1,1).
    fn eval_y(u: f32, p1: (f32, f32), p2: (f32, f32)) -> f32 {
        let inv = 1.0 - u;
        3.0 * inv * inv * u * p1.1 + 3.0 * inv * u * u * p2.1 + u * u * u
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn linear_curve_is_identity() {
        let c = BezierCurve::linear();
        for i in 0..=10 {
            let x = i as f32 / 10.0;
            let y = c.y_for_x(x);
            assert!(
                approx_eq(y, x, 1e-3),
                "linear: y_for_x({x}) = {y}, expected {x}"
            );
        }
    }

    #[test]
    fn default_curve_eases_out() {
        // The "default" curve is a snappy ease-out; at x=0.5 it should be
        // well past 0.5 on the y axis.
        let c = BezierCurve::default();
        let y = c.y_for_x(0.5);
        assert!(y > 0.5, "default: y_for_x(0.5) = {y}, expected > 0.5");
    }

    #[test]
    fn endpoints_are_anchored() {
        let c = BezierCurve::overshot();
        assert!(approx_eq(c.y_for_x(0.0), 0.0, 1e-3));
        assert!(approx_eq(c.y_for_x(1.0), 1.0, 1e-3));
    }

    #[test]
    fn overshot_passes_one_midcurve() {
        // Overshot's P2 has y > 1, so somewhere in the curve y > 1.
        let c = BezierCurve::overshot();
        let mut max_y = f32::MIN;
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            max_y = max_y.max(c.y_for_x(x));
        }
        assert!(
            max_y > 1.0,
            "overshot: expected mid-curve y > 1.0, got max_y = {max_y}"
        );
    }

    #[test]
    fn x_outside_range_is_clamped() {
        let c = BezierCurve::linear();
        assert!(approx_eq(c.y_for_x(-0.5), 0.0, 1e-3));
        assert!(approx_eq(c.y_for_x(1.5), 1.0, 1e-3));
    }
}
