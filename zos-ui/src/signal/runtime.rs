//! Thread-local reactive runtime.
//!
//! Owns the effect "slab" (a `Vec<Option<...>>` with a free-list), tracks the
//! currently-running effect so signals can self-register as deps, and holds a
//! deduplicated queue of effects that need to re-run after a `Signal::set`.
//!
//! Single-threaded by construction (`thread_local!` + `Rc`/`RefCell`/`Cell`).
//! Crossing threads is not supported and not needed for the UI use case.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;

/// Slot in the effect slab. `None` = freed (and on the free-list).
type EffectSlot = Option<Box<dyn FnMut()>>;

pub(crate) struct Runtime {
    /// Effect slab. Index = `EffectId`. `None` = freed slot, available for
    /// reuse via `free_list`.
    effects: RefCell<Vec<EffectSlot>>,
    /// Indices of freed slots that can be reused on next `alloc_effect`.
    free_list: RefCell<Vec<usize>>,
    /// `EffectId` currently executing — signals read during this run register
    /// themselves as deps of this id.
    current: Cell<Option<usize>>,
    /// FIFO of effect ids that need to re-run after the current `set()` call
    /// finishes mutating state.
    pending: RefCell<Vec<usize>>,
    /// Mirror of `pending` for O(1) dedupe — multiple signal sets in one batch
    /// don't queue the same effect twice.
    pending_set: RefCell<HashSet<usize>>,
    /// Re-entrancy guard for `flush_pending`. The outermost `set()` drives the
    /// flush; nested sets just enqueue.
    flushing: Cell<bool>,
}

impl Runtime {
    pub(crate) fn new() -> Self {
        Self {
            effects: RefCell::new(Vec::new()),
            free_list: RefCell::new(Vec::new()),
            current: Cell::new(None),
            pending: RefCell::new(Vec::new()),
            pending_set: RefCell::new(HashSet::new()),
            flushing: Cell::new(false),
        }
    }

    /// Allocate a new effect, returning its id.
    pub(crate) fn alloc_effect(&self, f: Box<dyn FnMut()>) -> usize {
        if let Some(id) = self.free_list.borrow_mut().pop() {
            self.effects.borrow_mut()[id] = Some(f);
            id
        } else {
            let mut effects = self.effects.borrow_mut();
            let id = effects.len();
            effects.push(Some(f));
            id
        }
    }

    /// Free an effect. Drops the closure and recycles the slot.
    pub(crate) fn free_effect(&self, id: usize) {
        if let Some(slot) = self.effects.borrow_mut().get_mut(id) {
            *slot = None;
        }
        self.free_list.borrow_mut().push(id);
        // If the effect was queued, drop it from the queue.
        if self.pending_set.borrow_mut().remove(&id) {
            self.pending.borrow_mut().retain(|&q| q != id);
        }
    }

    /// Run a single effect by id, with `current` set so signals can register
    /// it as a subscriber. Take-and-restore the closure to allow re-entrant
    /// signal sets without double-borrowing the effect slab.
    pub(crate) fn run_effect(&self, id: usize) {
        let mut taken = match self.effects.borrow_mut().get_mut(id) {
            Some(slot) => slot.take(),
            None => return,
        };
        let Some(ref mut f) = taken else { return };

        let prev = self.current.replace(Some(id));
        f();
        self.current.set(prev);

        // Restore the closure unless the slot was freed during the run.
        let mut effects = self.effects.borrow_mut();
        if let Some(slot) = effects.get_mut(id)
            && slot.is_none()
        {
            // Was the id put on the free-list during the run? If so, the
            // closure we held is stale — drop it. Otherwise restore.
            let on_free_list = self.free_list.borrow().contains(&id);
            if !on_free_list {
                *slot = taken;
            }
        }
        // If the slot was repopulated mid-run (shouldn't happen for the
        // same id), we leak the taken closure — drop it implicitly.
    }

    /// Enqueue an effect to re-run. Dedupes against already-queued ids.
    pub(crate) fn queue_effect(&self, id: usize) {
        // Don't queue freed effects.
        if self.effects.borrow().get(id).is_none_or(Option::is_none) {
            // Slot is None — but during a run the closure is taken out, so we
            // can't tell from is_none alone. Cross-check the free_list.
            if self.free_list.borrow().contains(&id) {
                return;
            }
        }
        if self.pending_set.borrow_mut().insert(id) {
            self.pending.borrow_mut().push(id);
        }
    }

    /// Drain the pending queue, running each effect once. Re-entrant calls
    /// (a flush triggered while already flushing) are no-ops — the outer
    /// flush picks up newly enqueued effects naturally.
    pub(crate) fn flush_pending(&self) {
        if self.flushing.get() {
            return;
        }
        self.flushing.set(true);
        loop {
            let next = {
                let mut pending = self.pending.borrow_mut();
                if pending.is_empty() {
                    break;
                }
                let id = pending.remove(0);
                self.pending_set.borrow_mut().remove(&id);
                id
            };
            self.run_effect(next);
        }
        self.flushing.set(false);
    }

    /// Currently-running effect id, if any. Signals call this in `get()` to
    /// know who to subscribe.
    pub(crate) fn current_effect(&self) -> Option<usize> {
        self.current.get()
    }
}

thread_local! {
    pub(crate) static RUNTIME: Runtime = Runtime::new();
}
