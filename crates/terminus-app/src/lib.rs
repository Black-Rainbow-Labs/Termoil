//! Terminus application: windows, tabs, splits, config, IPC server (`docs/TDD.md` section 4.8).
//!
//! The library holds everything the GUI binary `terminus-gui` and the command line `terminus` share:
//! the configuration, sessions, the window and pane model, and `App::run_action`, the one place an
//! action becomes a change. The winit front end lives in [`gui`].

pub mod actions;
pub mod app;
pub mod config;
pub mod crash;
pub mod events;
pub mod gui;
pub mod headless;
pub mod inspect;
pub mod ipc_server;
pub mod keymap;
pub mod links;
pub mod model;
pub mod palette;
pub mod paths;
pub mod platform;
pub mod restore;
pub mod session;
pub mod shell_integration;
pub mod terminfo;
pub mod themes;
pub mod view;

pub use app::{App, Effect, Outcome};
