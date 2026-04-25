//! Timer-driven helpers for reactive signal updates.
//!
//! [`use_interval`] and [`use_timeout`] schedule callbacks against a
//! thread-local timer registry. Both return RAII guards ([`Interval`] /
//! [`Timeout`]) that cancel the timer on drop — store the guard in your
//! component state to keep the timer alive.
//!
//! ## Single-threaded polling model
//!
//! The reactive runtime is single-threaded ([`thread_local!`] on
//! `Rc`/`RefCell`), so timers cannot fire from a background thread without
//! cross-thread plumbing we don't want to maintain. Instead, the host
//! application polls the registry by calling [`tick_timers`] from its event
//! loop — typically once per frame, or on a periodic `Tick` message coming
//! from `iced::time::every`.
//!
//! ```ignore
//! // Inside your iced app's `update`:
//! Message::Tick => {
//!     zos_ui::signal::tick_timers();
//!     Task::none()
//! }
//!
//! // And in `subscription`:
//! iced::time::every(Duration::from_millis(16)).map(|_| Message::Tick)
//! ```
//!
//! ## Example
//!
//! ```
//! use std::time::Duration;
//! use zos_ui::signal::{Signal, use_interval};
//!
//! let count = Signal::new(0);
//! let count_for_cb = count.clone();
//! let _interval = use_interval(Duration::from_millis(100), move || {
//!     let cur = *count_for_cb.peek();
//!     count_for_cb.set(cur + 1);
//! });
//! // The interval keeps firing as long as `_interval` is alive AND
//! // `tick_timers()` is being called from the host event loop.
//! ```

use std::cell::RefCell;
use std::time::{Duration, Instant};

thread_local! {
    static TIMER_REGISTRY: RefCell<TimerRegistry> = RefCell::new(TimerRegistry::default());
}

#[derive(Default)]
struct TimerRegistry {
    intervals: Vec<Option<IntervalSlot>>,
    timeouts: Vec<Option<TimeoutSlot>>,
    free_intervals: Vec<usize>,
    free_timeouts: Vec<usize>,
}

struct IntervalSlot {
    every: Duration,
    next: Instant,
    cb: Box<dyn FnMut()>,
}

struct TimeoutSlot {
    fires_at: Instant,
    cb: Option<Box<dyn FnOnce()>>,
}

impl TimerRegistry {
    fn add_interval(&mut self, every: Duration, cb: Box<dyn FnMut()>, now: Instant) -> usize {
        let slot = IntervalSlot {
            every,
            next: now + every,
            cb,
        };
        if let Some(idx) = self.free_intervals.pop() {
            self.intervals[idx] = Some(slot);
            idx
        } else {
            self.intervals.push(Some(slot));
            self.intervals.len() - 1
        }
    }

    fn remove_interval(&mut self, id: usize) {
        if let Some(slot) = self.intervals.get_mut(id)
            && slot.is_some()
        {
            *slot = None;
            self.free_intervals.push(id);
        }
    }

    fn add_timeout(&mut self, after: Duration, cb: Box<dyn FnOnce()>, now: Instant) -> usize {
        let slot = TimeoutSlot {
            fires_at: now + after,
            cb: Some(cb),
        };
        if let Some(idx) = self.free_timeouts.pop() {
            self.timeouts[idx] = Some(slot);
            idx
        } else {
            self.timeouts.push(Some(slot));
            self.timeouts.len() - 1
        }
    }

    fn remove_timeout(&mut self, id: usize) {
        if let Some(slot) = self.timeouts.get_mut(id)
            && slot.is_some()
        {
            *slot = None;
            self.free_timeouts.push(id);
        }
    }

    fn tick(&mut self, now: Instant) {
        // Snapshot the current set of interval ids. We can't iterate over
        // `self.intervals` directly while invoking callbacks, because a
        // callback may itself add or cancel timers, mutating the Vec.
        let interval_ids: Vec<usize> = self
            .intervals
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| slot.as_ref().map(|_| idx))
            .collect();

        for idx in interval_ids {
            // Pull the slot out for the duration of the callback so the
            // callback can freely re-enter the registry (e.g., to register
            // another timer).
            let Some(mut slot) = self.intervals.get_mut(idx).and_then(|s| s.take()) else {
                continue;
            };
            let mut cancelled = false;
            while slot.next <= now {
                (slot.cb)();
                // Guard against `every == 0` — bump by at least 1ns so we
                // don't spin forever on a misconfigured interval.
                let step = if slot.every.is_zero() {
                    Duration::from_nanos(1)
                } else {
                    slot.every
                };
                slot.next += step;
                // If a callback cancelled this interval (slot was removed),
                // bail out and don't put it back.
                if self
                    .intervals
                    .get(idx)
                    .map(|s| s.is_none())
                    .unwrap_or(true)
                    && self.free_intervals.contains(&idx)
                {
                    cancelled = true;
                    break;
                }
            }
            if !cancelled
                && let Some(s) = self.intervals.get_mut(idx)
            {
                *s = Some(slot);
            }
        }

        // Same dance for timeouts. Take the slot out, fire it, free it.
        let timeout_ids: Vec<usize> = self
            .timeouts
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| slot.as_ref().map(|_| idx))
            .collect();

        for idx in timeout_ids {
            let Some(mut slot) = self.timeouts.get_mut(idx).and_then(|s| s.take()) else {
                continue;
            };
            if slot.fires_at <= now {
                if let Some(cb) = slot.cb.take() {
                    cb();
                }
                // Slot stays None; mark the index free unless a callback
                // already cancelled it (which would have done so on a `None`
                // slot and pushed a duplicate — guard against that).
                if !self.free_timeouts.contains(&idx) {
                    self.free_timeouts.push(idx);
                }
            } else {
                // Not fired yet — put it back.
                if let Some(s) = self.timeouts.get_mut(idx) {
                    *s = Some(slot);
                }
            }
        }
    }

    #[cfg(test)]
    fn interval_count(&self) -> usize {
        self.intervals.iter().filter(|s| s.is_some()).count()
    }

    #[cfg(test)]
    fn timeout_count(&self) -> usize {
        self.timeouts.iter().filter(|s| s.is_some()).count()
    }
}

/// RAII guard returned by [`use_interval`]. Cancels the interval on drop.
pub struct Interval {
    id: usize,
}

impl Drop for Interval {
    fn drop(&mut self) {
        TIMER_REGISTRY.with(|reg| reg.borrow_mut().remove_interval(self.id));
    }
}

impl std::fmt::Debug for Interval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Interval").field("id", &self.id).finish()
    }
}

/// RAII guard returned by [`use_timeout`]. Cancels the timeout on drop, if it
/// has not already fired.
pub struct Timeout {
    id: usize,
}

impl Drop for Timeout {
    fn drop(&mut self) {
        TIMER_REGISTRY.with(|reg| reg.borrow_mut().remove_timeout(self.id));
    }
}

impl std::fmt::Debug for Timeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Timeout").field("id", &self.id).finish()
    }
}

/// Schedule `cb` to fire every `every` duration. Returns a guard whose
/// [`Drop`] cancels the interval — store it in your component state to keep
/// the timer alive.
///
/// Timers do not fire on their own; the host application must call
/// [`tick_timers`] from its event loop.
pub fn use_interval(every: Duration, cb: impl FnMut() + 'static) -> Interval {
    let now = Instant::now();
    let id = TIMER_REGISTRY.with(|reg| reg.borrow_mut().add_interval(every, Box::new(cb), now));
    Interval { id }
}

/// Schedule `cb` to fire once after `after` duration. Returns a guard whose
/// [`Drop`] cancels the timeout if it has not yet fired.
///
/// Timers do not fire on their own; the host application must call
/// [`tick_timers`] from its event loop.
pub fn use_timeout(after: Duration, cb: impl FnOnce() + 'static) -> Timeout {
    let now = Instant::now();
    let id = TIMER_REGISTRY.with(|reg| reg.borrow_mut().add_timeout(after, Box::new(cb), now));
    Timeout { id }
}

/// Advance all registered timers using the current [`Instant`]. Call this
/// from your app's frame loop (e.g., from an iced subscription's `Tick`
/// message).
pub fn tick_timers() {
    let now = Instant::now();
    TIMER_REGISTRY.with(|reg| reg.borrow_mut().tick(now));
}

/// Test-only helper: advance the registry to a synthetic `Instant`. Avoids
/// the flakiness of relying on real wall-clock sleeps in unit tests.
#[cfg(test)]
pub(crate) fn tick_at(now: Instant) {
    TIMER_REGISTRY.with(|reg| reg.borrow_mut().tick(now));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Each test should start from a clean registry — other tests in this
    /// file run on the same thread and share the thread-local. We snapshot
    /// the slot counts at the start and only assert deltas.
    fn registry_counts() -> (usize, usize) {
        TIMER_REGISTRY.with(|reg| {
            let r = reg.borrow();
            (r.interval_count(), r.timeout_count())
        })
    }

    #[test]
    fn use_interval_fires_multiple_times() {
        let (start_iv, _) = registry_counts();

        let count: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
        let count_cb = count.clone();
        let interval = use_interval(Duration::from_millis(10), move || {
            *count_cb.borrow_mut() += 1;
        });

        // One slot allocated.
        let (after_alloc_iv, _) = registry_counts();
        assert_eq!(after_alloc_iv - start_iv, 1);

        // Walk a synthetic clock forward: t=15ms (one fire), then t=25ms
        // (one more fire), then t=100ms (catch-up fires the rest).
        let t0 = Instant::now();
        tick_at(t0 + Duration::from_millis(15));
        assert_eq!(*count.borrow(), 1);

        tick_at(t0 + Duration::from_millis(25));
        assert_eq!(*count.borrow(), 2);

        // Jump well past several intervals — the catch-up loop fires once
        // per missed interval (10ms cadence, t=100 means roughly the 10th
        // boundary; at this point the interval has caught up to 10 fires).
        tick_at(t0 + Duration::from_millis(100));
        assert!(
            *count.borrow() >= 9,
            "expected catch-up to fire many times; got {}",
            *count.borrow()
        );

        drop(interval);
    }

    #[test]
    fn use_interval_drop_cancels() {
        let (start_iv, _) = registry_counts();

        let count: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
        let count_cb = count.clone();
        let interval = use_interval(Duration::from_millis(5), move || {
            *count_cb.borrow_mut() += 1;
        });

        let t0 = Instant::now();
        tick_at(t0 + Duration::from_millis(7));
        assert_eq!(*count.borrow(), 1);

        // Drop the guard — interval slot must be released and never fire
        // again.
        drop(interval);
        let (after_drop_iv, _) = registry_counts();
        assert_eq!(after_drop_iv, start_iv);

        // Even if the host keeps ticking, the count stays put.
        tick_at(t0 + Duration::from_millis(50));
        tick_at(t0 + Duration::from_millis(500));
        assert_eq!(*count.borrow(), 1);
    }

    #[test]
    fn use_timeout_fires_once_after_duration() {
        let (_, start_to) = registry_counts();

        let fired: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
        let fired_cb = fired.clone();
        let timeout = use_timeout(Duration::from_millis(20), move || {
            *fired_cb.borrow_mut() += 1;
        });

        let (_, after_alloc_to) = registry_counts();
        assert_eq!(after_alloc_to - start_to, 1);

        let t0 = Instant::now();
        // Before the deadline — no fire.
        tick_at(t0 + Duration::from_millis(5));
        assert_eq!(*fired.borrow(), 0);

        // Past the deadline — exactly one fire.
        tick_at(t0 + Duration::from_millis(25));
        assert_eq!(*fired.borrow(), 1);

        // Subsequent ticks must not re-fire.
        tick_at(t0 + Duration::from_millis(100));
        tick_at(t0 + Duration::from_millis(200));
        assert_eq!(*fired.borrow(), 1);

        // Slot was released after firing.
        let (_, after_fire_to) = registry_counts();
        assert_eq!(after_fire_to, start_to);

        // Dropping the (already-fired) guard is a no-op.
        drop(timeout);
    }

    #[test]
    fn use_timeout_drop_before_fire_cancels() {
        let (_, start_to) = registry_counts();

        let fired: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
        let fired_cb = fired.clone();
        let timeout = use_timeout(Duration::from_millis(50), move || {
            *fired_cb.borrow_mut() += 1;
        });

        let (_, after_alloc_to) = registry_counts();
        assert_eq!(after_alloc_to - start_to, 1);

        // Cancel before the deadline.
        drop(timeout);

        let (_, after_drop_to) = registry_counts();
        assert_eq!(after_drop_to, start_to);

        // Even if we tick way past the original deadline, the callback is
        // gone and never fires.
        let t0 = Instant::now();
        tick_at(t0 + Duration::from_millis(100));
        tick_at(t0 + Duration::from_millis(1000));
        assert_eq!(*fired.borrow(), 0);
    }

    #[test]
    fn use_timeout_integrates_with_signal() {
        // Smoke test: the documented Signal-driven pattern actually compiles
        // and behaves as expected once the user calls `tick_timers`.
        use crate::signal::Signal;

        let value = Signal::new(0i32);
        let value_cb = value.clone();
        let _to = use_timeout(Duration::from_millis(10), move || {
            value_cb.set(42);
        });

        let t0 = Instant::now();
        tick_at(t0 + Duration::from_millis(5));
        assert_eq!(*value.peek(), 0);

        tick_at(t0 + Duration::from_millis(15));
        assert_eq!(*value.peek(), 42);
    }
}
