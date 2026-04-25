//! A single animatable value driven by elapsed wall-clock time and a
//! Bezier curve.
//!
//! Lifecycle:
//! - `new(initial)` — constructs a settled value (`begun == goal == initial`).
//! - `animate_to(goal, curve, duration)` — schedules a transition from the
//!   current value to `goal` over `duration`, eased through `curve`.
//! - `warp_to(goal)` — instantly snaps to `goal` (no animation).
//! - `tick(now)` — advances the interpolated value based on monotonic time.
//!   Idempotent if the value is settled.
//! - `value()` — current interpolated value.
//! - `is_animating()` — whether the value is currently in transit.
//!
//! Wall-clock-driven (not vsync-driven), so the same animation looks the
//! same on a 60Hz panel as on a 144Hz panel. Skipped ticks just produce
//! larger interpolation jumps; start/end are unaffected.
//!
//! See `docs/research/phase-4-hyprland-animations.md` §7 for design.

use std::time::{Duration, Instant};

use super::animatable::Animatable;
use super::bezier::BezierCurve;

#[derive(Debug, Clone)]
pub struct AnimatedValue<T: Animatable> {
    /// Where the current transition began.
    begun_value: T,
    /// Latest interpolated value.
    current: T,
    /// Where the current transition is heading.
    goal: T,
    /// Elapsed time within the current transition (last ticked).
    elapsed: Duration,
    /// Wall-clock time the transition started; `None` when settled.
    started_at: Option<Instant>,
    /// Total duration of the current transition.
    duration: Duration,
    /// Easing curve for the current transition.
    curve: BezierCurve,
    /// Cached so `is_animating` is O(1).
    animating: bool,
}

impl<T: Animatable> AnimatedValue<T> {
    /// Construct a settled value: `begun == current == goal == initial`.
    pub fn new(initial: T) -> Self {
        Self {
            begun_value: initial,
            current: initial,
            goal: initial,
            elapsed: Duration::ZERO,
            started_at: None,
            duration: Duration::ZERO,
            curve: BezierCurve::linear(),
            animating: false,
        }
    }

    /// Schedule a transition from the current value to `goal` over `duration`,
    /// eased through `curve`. If `duration` is zero, this is equivalent to
    /// `warp_to(goal)`.
    pub fn animate_to(&mut self, goal: T, curve: BezierCurve, duration: Duration) {
        if duration.is_zero() {
            self.warp_to(goal);
            return;
        }
        self.begun_value = self.current;
        self.goal = goal;
        self.elapsed = Duration::ZERO;
        self.started_at = Some(Instant::now());
        self.duration = duration;
        self.curve = curve;
        self.animating = true;
    }

    /// Instantly snap to `goal` with no animation. Settles the value.
    pub fn warp_to(&mut self, goal: T) {
        self.begun_value = goal;
        self.current = goal;
        self.goal = goal;
        self.started_at = None;
        self.duration = Duration::ZERO;
        self.elapsed = Duration::ZERO;
        self.animating = false;
    }

    /// Advance the interpolated value based on monotonic time. Idempotent if
    /// the value is settled. Pass the same `now` to all `tick` calls within
    /// a frame for consistent state across animated values.
    pub fn tick(&mut self, now: Instant) {
        if !self.animating {
            return;
        }
        let started = match self.started_at {
            Some(t) => t,
            None => {
                self.animating = false;
                return;
            }
        };
        let elapsed = now.duration_since(started);
        self.elapsed = elapsed;
        if elapsed >= self.duration {
            self.current = self.goal;
            self.animating = false;
            self.started_at = None;
            return;
        }
        let t = elapsed.as_secs_f32() / self.duration.as_secs_f32();
        let eased = self.curve.y_for_x(t);
        self.current = T::lerp(self.begun_value, self.goal, eased);
    }

    /// The current interpolated value.
    pub fn value(&self) -> T {
        self.current
    }

    /// The target of the current transition (or current value if settled).
    pub fn goal(&self) -> T {
        self.goal
    }

    /// Whether the value is currently in transit.
    pub fn is_animating(&self) -> bool {
        self.animating
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn new_is_settled() {
        let v = AnimatedValue::<f32>::new(42.0);
        assert!(!v.is_animating());
        assert_eq!(v.value(), 42.0);
        assert_eq!(v.goal(), 42.0);
    }

    #[test]
    fn warp_to_snaps_immediately() {
        let mut v = AnimatedValue::<f32>::new(0.0);
        v.animate_to(100.0, BezierCurve::linear(), Duration::from_secs(10));
        assert!(v.is_animating());
        v.warp_to(50.0);
        assert!(!v.is_animating());
        assert_eq!(v.value(), 50.0);
        assert_eq!(v.goal(), 50.0);
    }

    #[test]
    fn animate_to_zero_duration_is_warp() {
        let mut v = AnimatedValue::<f32>::new(0.0);
        v.animate_to(100.0, BezierCurve::linear(), Duration::ZERO);
        assert!(!v.is_animating());
        assert_eq!(v.value(), 100.0);
    }

    #[test]
    fn tick_walks_toward_goal() {
        let mut v = AnimatedValue::<f32>::new(0.0);
        v.animate_to(100.0, BezierCurve::linear(), Duration::from_millis(40));
        let start = v.started_at.unwrap();

        // Halfway through duration: linear curve should give ~50.0.
        let mid = start + Duration::from_millis(20);
        v.tick(mid);
        assert!(v.is_animating());
        let mid_val = v.value();
        assert!(
            mid_val > 25.0 && mid_val < 75.0,
            "expected mid value in (25, 75), got {mid_val}"
        );

        // Past duration: should clamp to goal and settle.
        let after = start + Duration::from_millis(100);
        v.tick(after);
        assert!(!v.is_animating());
        assert_eq!(v.value(), 100.0);
    }

    #[test]
    fn tick_after_settle_is_noop() {
        let mut v = AnimatedValue::<f32>::new(0.0);
        v.animate_to(10.0, BezierCurve::linear(), Duration::from_millis(1));
        // Force settle by ticking far in the future.
        sleep(Duration::from_millis(2));
        v.tick(Instant::now());
        assert!(!v.is_animating());
        let before = v.value();
        // Subsequent ticks shouldn't change anything.
        v.tick(Instant::now() + Duration::from_secs(1));
        assert_eq!(v.value(), before);
        assert!(!v.is_animating());
    }

    #[test]
    fn animate_to_starts_from_current_not_begun() {
        // If you re-target mid-flight, the new transition begins from the
        // currently-interpolated value, not from the original begun_value.
        let mut v = AnimatedValue::<f32>::new(0.0);
        v.animate_to(100.0, BezierCurve::linear(), Duration::from_millis(1000));
        // Don't tick; current is still 0.0. Re-target.
        v.animate_to(50.0, BezierCurve::linear(), Duration::from_millis(100));
        // begun should be 0.0 (the current value at retarget), goal should be 50.0.
        assert_eq!(v.begun_value, 0.0);
        assert_eq!(v.goal(), 50.0);
    }
}
