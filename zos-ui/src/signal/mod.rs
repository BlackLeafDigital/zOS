//! Lightweight reactive signal system.
//!
//! Three primitives:
//! - [`Signal<T>`] — a reactive cell. `get()` registers a dependency,
//!   `set()`/`update()` notify subscribers.
//! - [`Effect`] — a side-effecting computation. Runs once on creation to
//!   collect deps, re-runs whenever any read signal updates. Drop the
//!   handle to deregister.
//! - [`Memo<T>`] — a cached derivation. Recomputes when inputs change,
//!   suppresses no-op writes via `PartialEq`.
//!
//! ## Threading
//!
//! The runtime is `thread_local!` and uses `Rc`/`RefCell`/`Cell` internally.
//! Signals, memos, and effects are **single-threaded only** — they cannot
//! cross thread boundaries. This matches the iced/Wayland UI thread model
//! where all rendering and input happens on one thread.
//!
//! ## Example
//!
//! ```
//! use zos_ui::signal::{Signal, Memo, Effect};
//! use std::cell::RefCell;
//! use std::rc::Rc;
//!
//! let count = Signal::new(0);
//! let count_for_memo = count.clone();
//! let doubled = Memo::new(move || count_for_memo.get() * 2);
//!
//! let log: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
//! let log_eff = log.clone();
//! let doubled_eff = doubled.clone();
//! let _eff = Effect::new(move || log_eff.borrow_mut().push(doubled_eff.get()));
//!
//! count.set(5);
//! assert_eq!(*log.borrow(), vec![0, 10]);
//! ```

pub mod effect;
pub mod memo;
mod runtime;
pub mod timer;
pub mod value;

pub use effect::Effect;
pub use memo::Memo;
pub use timer::{Interval, Timeout, tick_timers, use_interval, use_timeout};
pub use value::Signal;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn signal_get_returns_initial_value() {
        let s = Signal::new(42);
        assert_eq!(s.get(), 42);
    }

    #[test]
    fn signal_set_updates_value_and_notifies() {
        let s = Signal::new(0);
        let log: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));

        let s_eff = s.clone();
        let log_eff = log.clone();
        let _eff = Effect::new(move || {
            log_eff.borrow_mut().push(s_eff.get());
        });

        // First entry from the initial run.
        assert_eq!(*log.borrow(), vec![0]);

        s.set(1);
        s.set(2);
        s.set(3);

        assert_eq!(*log.borrow(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn multiple_subscribers_all_fire() {
        let s = Signal::new(0);
        let count_a: Rc<RefCell<i32>> = Rc::new(RefCell::new(0));
        let count_b: Rc<RefCell<i32>> = Rc::new(RefCell::new(0));

        let s_a = s.clone();
        let a = count_a.clone();
        let _eff_a = Effect::new(move || {
            let _ = s_a.get();
            *a.borrow_mut() += 1;
        });

        let s_b = s.clone();
        let b = count_b.clone();
        let _eff_b = Effect::new(move || {
            let _ = s_b.get();
            *b.borrow_mut() += 1;
        });

        // Initial run for each effect.
        assert_eq!(*count_a.borrow(), 1);
        assert_eq!(*count_b.borrow(), 1);

        s.set(10);
        assert_eq!(*count_a.borrow(), 2);
        assert_eq!(*count_b.borrow(), 2);
    }

    #[test]
    fn memo_dedupes_identical_recompute() {
        let s = Signal::new(0);
        let s_for_memo = s.clone();
        let memo = Memo::new(move || s_for_memo.get() / 10);

        let runs: Rc<RefCell<i32>> = Rc::new(RefCell::new(0));
        let runs_eff = runs.clone();
        let memo_for_eff = memo.clone();
        let _eff = Effect::new(move || {
            let _ = memo_for_eff.get();
            *runs_eff.borrow_mut() += 1;
        });

        // Initial.
        assert_eq!(*runs.borrow(), 1);

        // Changing s 0 -> 5 keeps memo at 0; downstream effect should NOT run.
        s.set(5);
        assert_eq!(memo.get(), 0);
        assert_eq!(*runs.borrow(), 1);

        // Crossing the divisor boundary moves memo 0 -> 1 — effect runs.
        s.set(15);
        assert_eq!(memo.get(), 1);
        assert_eq!(*runs.borrow(), 2);
    }

    #[test]
    fn dropping_effect_unsubscribes() {
        let s = Signal::new(0);
        let runs: Rc<RefCell<i32>> = Rc::new(RefCell::new(0));

        let s_eff = s.clone();
        let runs_eff = runs.clone();
        let eff = Effect::new(move || {
            let _ = s_eff.get();
            *runs_eff.borrow_mut() += 1;
        });
        assert_eq!(*runs.borrow(), 1);

        s.set(1);
        assert_eq!(*runs.borrow(), 2);

        drop(eff);

        s.set(2);
        s.set(3);
        // No new runs after the effect is dropped.
        assert_eq!(*runs.borrow(), 2);
    }

    #[test]
    fn batched_set_dedupes_via_pending_queue() {
        // Two signals, one effect reading both. A nested effect that reads
        // `a` and writes `b` re-enters the runtime mid-flush.
        //
        // Contract: the dedupe `pending_set` collapses redundant queues —
        // when both `a.set` (direct) and the nested effect's `b.set` queue
        // the first effect during the same flush, it should not run more
        // than twice (once per signal change), and never three+ times.
        let a = Signal::new(0);
        let b = Signal::new(0);

        let runs: Rc<RefCell<i32>> = Rc::new(RefCell::new(0));
        let a_eff = a.clone();
        let b_eff = b.clone();
        let runs_eff = runs.clone();

        let _eff = Effect::new(move || {
            let _ = a_eff.get();
            let _ = b_eff.get();
            *runs_eff.borrow_mut() += 1;
        });

        let runs_b: Rc<RefCell<i32>> = Rc::new(RefCell::new(0));
        let runs_b_eff = runs_b.clone();
        let nested_a = a.clone();
        let nested_b = b.clone();
        let _nested = Effect::new(move || {
            let _ = nested_a.get();
            *runs_b_eff.borrow_mut() += 1;
            let cur = *nested_b.peek();
            nested_b.set(cur + 1);
        });

        let baseline = *runs.borrow();
        assert!(baseline >= 1);

        a.set(1);
        let after = *runs.borrow();
        assert!(after - baseline <= 2, "first effect ran {} times for one a.set", after - baseline);
        assert!(after - baseline >= 1);
    }

    #[test]
    fn signal_update_in_place() {
        let s: Signal<Vec<i32>> = Signal::new(vec![1, 2, 3]);
        let observed: Rc<RefCell<Vec<Vec<i32>>>> = Rc::new(RefCell::new(Vec::new()));

        let s_eff = s.clone();
        let observed_eff = observed.clone();
        let _eff = Effect::new(move || {
            observed_eff.borrow_mut().push(s_eff.get());
        });

        s.update(|v| v.push(4));

        assert_eq!(
            *observed.borrow(),
            vec![vec![1, 2, 3], vec![1, 2, 3, 4]]
        );
    }

    #[test]
    fn peek_does_not_subscribe() {
        let s = Signal::new(0);
        let runs: Rc<RefCell<i32>> = Rc::new(RefCell::new(0));

        let s_eff = s.clone();
        let runs_eff = runs.clone();
        let _eff = Effect::new(move || {
            // `peek` reads without registering a dep.
            let _ = *s_eff.peek();
            *runs_eff.borrow_mut() += 1;
        });

        assert_eq!(*runs.borrow(), 1);

        s.set(1);
        s.set(2);

        // No new runs — peek doesn't subscribe.
        assert_eq!(*runs.borrow(), 1);
    }
}
