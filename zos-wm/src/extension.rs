//! In-process compile-time extension trait.
//!
//! Use this for things that MUST run inside the render/event loop:
//! animations, layout algorithms, custom keybinding handlers.
//!
//! Plugins implement `Extension` and register at compositor startup.
//! Out-of-process plugins use the IPC socket instead.

use std::time::Instant;

pub trait Extension: Send {
    /// Human-readable name.
    fn name(&self) -> &str;

    /// Called once at startup, after AnvilState is fully initialized
    /// but before the first frame is rendered.
    fn init(&mut self) {}

    /// Called at the start of every frame, before render. Return value
    /// is informational only (e.g., for telemetry).
    fn pre_frame(&mut self, _now: Instant) {}

    /// Called at the end of every frame, after render.
    fn post_frame(&mut self, _now: Instant) {}

    /// Called when the compositor shuts down. Last chance to clean up.
    fn shutdown(&mut self) {}
}

/// A registry of compile-time extensions. Compositor's main.rs builds
/// one of these at startup; init() runs every extension.
pub struct ExtensionRegistry {
    pub extensions: Vec<Box<dyn Extension>>,
}

impl std::fmt::Debug for ExtensionRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Box<dyn Extension> isn't Debug (and we don't want to require it on
        // the trait), so just print the registered extension names. Lets
        // AnvilState keep its blanket `#[derive(Debug)]`.
        f.debug_struct("ExtensionRegistry")
            .field(
                "extensions",
                &self
                    .extensions
                    .iter()
                    .map(|e| e.name())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self {
            extensions: Vec::new(),
        }
    }

    pub fn register(&mut self, ext: Box<dyn Extension>) {
        tracing::info!(name = ext.name(), "registering compositor extension");
        self.extensions.push(ext);
    }

    pub fn init_all(&mut self) {
        for ext in self.extensions.iter_mut() {
            ext.init();
        }
    }

    pub fn pre_frame_all(&mut self, now: Instant) {
        for ext in self.extensions.iter_mut() {
            ext.pre_frame(now);
        }
    }

    pub fn post_frame_all(&mut self, now: Instant) {
        for ext in self.extensions.iter_mut() {
            ext.post_frame(now);
        }
    }

    pub fn shutdown_all(&mut self) {
        for ext in self.extensions.iter_mut() {
            ext.shutdown();
        }
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Example extension that just logs frame count. Use as a template
/// when writing real extensions. Tests + docs reference this.
pub struct LogFrameCount {
    count: u64,
}

impl LogFrameCount {
    pub fn new() -> Self {
        Self { count: 0 }
    }
}

impl Default for LogFrameCount {
    fn default() -> Self {
        Self::new()
    }
}

impl Extension for LogFrameCount {
    fn name(&self) -> &str {
        "log-frame-count"
    }
    fn pre_frame(&mut self, _now: Instant) {
        self.count = self.count.wrapping_add(1);
        if self.count.is_multiple_of(120) {
            tracing::debug!(frames = self.count, "log-frame-count extension tick");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CountExt {
        init: u32,
        pre: u32,
        post: u32,
        shut: u32,
    }

    impl Extension for CountExt {
        fn name(&self) -> &str {
            "count-ext"
        }
        fn init(&mut self) {
            self.init += 1;
        }
        fn pre_frame(&mut self, _now: Instant) {
            self.pre += 1;
        }
        fn post_frame(&mut self, _now: Instant) {
            self.post += 1;
        }
        fn shutdown(&mut self) {
            self.shut += 1;
        }
    }

    #[test]
    fn registry_dispatches_to_all() {
        let mut reg = ExtensionRegistry::new();
        reg.register(Box::new(CountExt {
            init: 0,
            pre: 0,
            post: 0,
            shut: 0,
        }));
        reg.register(Box::new(LogFrameCount::new()));
        assert_eq!(reg.extensions.len(), 2);
        reg.init_all();
        let now = Instant::now();
        reg.pre_frame_all(now);
        reg.post_frame_all(now);
        reg.shutdown_all();
    }
}
