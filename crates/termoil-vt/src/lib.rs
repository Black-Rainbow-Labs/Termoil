//! Termoil terminal core: parser, pages, screen, modes, selection, search, graphics store and
//! Unicode tables. No window, GPU or font dependency, so it builds for `wasm32-unknown-unknown`.

pub mod cell;
pub mod charset;
pub mod dump;
pub mod graphics;
mod hash;
pub mod modes;
pub mod page;
pub mod pagelist;
pub mod parser;
mod reflow;
pub mod scan;
pub mod screen;
pub mod search;
pub mod selection;
pub mod style;
pub mod terminal;
pub mod text;
pub mod unicode;

pub use terminal::{Event, Terminal, TerminalConfig};
