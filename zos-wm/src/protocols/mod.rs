//! Wayland protocol implementations local to zos-wm.
//!
//! Protocols in this directory are ones that smithay (at our pinned commit
//! `27af99ef492ab4d7dc5cd2e625374d2beb2772f7`) does not yet ship as a built-in
//! handler module. Each submodule defines the dispatch and state-management
//! glue needed for `AnvilState` to advertise the corresponding Wayland global.

pub mod tearing_control;
pub mod gamma_control;
pub mod output_management;
