//! `Effect` — a reactive side-effecting computation.
//!
//! On creation the effect runs once to register its dependencies. Whenever
//! any read signal is `set`, the effect is queued and re-run. Dropping the
//! effect frees its slot in the runtime; further signal updates won't fire
//! it.
//!
//! Effects are owned by handles — keep the `Effect` alive as long as you
//! want it to run. A common pattern is to store it on the component holding
//! the signals it observes.

use super::runtime::RUNTIME;

/// Handle to a reactive effect. Drops the effect when dropped.
pub struct Effect {
    id: usize,
}

impl Effect {
    /// Allocate the effect, run it once to register dependencies, and return
    /// a handle. Drop the handle to deregister.
    pub fn new<F>(f: F) -> Self
    where
        F: FnMut() + 'static,
    {
        let id = RUNTIME.with(|rt| {
            let id = rt.alloc_effect(Box::new(f));
            rt.run_effect(id);
            id
        });
        Self { id }
    }

    /// Effect id (for debug / introspection only).
    #[doc(hidden)]
    pub fn id(&self) -> usize {
        self.id
    }
}

impl Drop for Effect {
    fn drop(&mut self) {
        RUNTIME.with(|rt| rt.free_effect(self.id));
    }
}

impl std::fmt::Debug for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Effect").field("id", &self.id).finish()
    }
}
