//! `Memo<T>` — a derived, cached, reactive value.
//!
//! A `Memo` is conceptually `Signal<T> + Effect`: it owns a cached `Signal`
//! and an `Effect` that recomputes it whenever any of the signals read inside
//! `compute` change. The `PartialEq` bound lets us short-circuit when the
//! recomputed value matches the cache, so downstream effects don't re-run on
//! semantic no-ops.

use std::rc::Rc;

use super::effect::Effect;
use super::value::Signal;

/// A derived reactive value. Re-computes when its inputs change; downstream
/// readers only see updates when the new value is `!=` the cached one.
///
/// Cheap to clone — both the cached signal and the recompute effect are
/// reference-counted. The effect lives until the last clone is dropped.
pub struct Memo<T> {
    cached: Signal<T>,
    // Holds the recompute effect alive for the memo's lifetime. `Rc`-wrapped
    // so the memo can be cloned and shared.
    _effect: Rc<Effect>,
}

impl<T> Clone for Memo<T> {
    fn clone(&self) -> Self {
        Self {
            cached: self.cached.clone(),
            _effect: Rc::clone(&self._effect),
        }
    }
}

impl<T: Clone + PartialEq + 'static> Memo<T> {
    /// Create a memoized derivation. `compute` is invoked immediately to seed
    /// the cache and again any time a signal it reads changes.
    pub fn new<F>(compute: F) -> Self
    where
        F: Fn() -> T + 'static,
    {
        // Seed with the initial computed value.
        let cached = Signal::new(compute());
        let cached_for_effect = cached.clone();
        let effect = Effect::new(move || {
            let new_val = compute();
            // Skip the write when nothing changed — keeps downstream effects
            // from re-running on identity recomputes.
            let unchanged = *cached_for_effect.peek() == new_val;
            if !unchanged {
                cached_for_effect.set(new_val);
            }
        });
        Self {
            cached,
            _effect: Rc::new(effect),
        }
    }

    /// Read the memoized value, registering the current effect as a
    /// subscriber of the cache.
    pub fn get(&self) -> T {
        self.cached.get()
    }

    /// Read without subscribing. See `Signal::peek`.
    pub fn peek(&self) -> std::cell::Ref<'_, T> {
        self.cached.peek()
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Memo<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Memo")
            .field("cached", &self.cached)
            .finish()
    }
}
