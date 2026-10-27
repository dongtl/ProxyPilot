//! Everything ProxyPilot decides, without any UI.
//!
//! The Tauri shell only wires these pieces to windows, the tray and IPC, so the
//! behaviour that matters (which traffic goes where, how the core is replaced,
//! what counts as a proxy port) can be tested on its own and reused when the
//! macOS shell arrives. Platform calls live in `sys`.

pub mod acl;
pub mod catalog;
pub mod config;
pub mod core;
pub mod detect;
pub mod diagnose;
pub mod domain;
pub mod envcheck;
pub mod model;
pub mod presets;
pub mod probe;
pub mod settings;
pub mod sys;
pub mod traffic;

pub use model::*;
