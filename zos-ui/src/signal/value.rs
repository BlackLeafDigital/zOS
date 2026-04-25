//! `Signal<T>` — the reactive cell.
//!
//! `get()` registers the currently-running effect as a subscriber.
//! `set()`/`update()` enqueue all subscribers and flush.
//! `peek()` reads without subscribing — use it inside effects that should
//! mutate but not re-trigger themselves.

use std::cell::{Ref, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use super::runtime::RUNTIME;

pub(crate) struct SignalInner<T> {
    pub(crate) value: T,
    /// EffectIds subscribed to this signal. A `HashSet` so the same effect
    /// reading the same signal multiple times in one run only subscribes once.
    pub(crate) subscribers: HashSet<usize>,
}

/// A reactive cell. Cheap to clone (`Rc` under the hood).
pub struct Signal<T> {
    inner: Rc<RefCell<SignalInner<T>>>,
}

impl<T: 'static> Signal<T> {
    /// Create a new signal wrapping `initial`.
    pub fn new(initial: T) -> Self {
        Self {
            inner: Rc::new(RefCell::new(SignalInner {
                value: initial,
                subscribers: HashSet::new(),
            })),
        }
    }

    /// Read the value without registering a subscription. Useful inside
    /// effects/memos that should not re-trigger themselves on write.
    pub fn peek(&self) -> Ref<'_, T> {
        Ref::map(self.inner.borrow(), |inner| &inner.value)
    }

    /// Replace the value and notify subscribers.
    pub fn set(&self, value: T) {
        self.inner.borrow_mut().value = value;
        self.notify();
    }

    /// Mutate in place, then notify subscribers.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        f(&mut self.inner.borrow_mut().value);
        self.notify();
    }

    fn notify(&self) {
        // Snapshot subscribers so the queue/run path can't mutate the set we
        // iterate (an effect might subscribe to/unsubscribe from this signal
        // mid-flush).
        let subs: Vec<usize> = self.inner.borrow().subscribers.iter().copied().collect();
        RUNTIME.with(|rt| {
            for id in subs {
                rt.queue_effect(id);
            }
            rt.flush_pending();
        });
    }
}

impl<T: Clone + 'static> Signal<T> {
    /// Read the value, registering the currently-running effect as a
    /// subscriber.
    pub fn get(&self) -> T {
        RUNTIME.with(|rt| {
            if let Some(eff) = rt.current_effect() {
                self.inner.borrow_mut().subscribers.insert(eff);
            }
        });
        self.inner.borrow().value.clone()
    }
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signal")
            .field("value", &self.inner.borrow().value)
            .field("subscribers", &self.inner.borrow().subscribers.len())
            .finish()
    }
}
