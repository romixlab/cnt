//! Terminal views of `cnt` counters: list, read, watch for changes, the TUI and JSON output, shared by the `cnt` CLI
//! (debug probe) and `ww counters` (WireWeaver). Counters are decoded with `cnt_core` from the words of any
//! [`Source`](cnt_core::Source).

pub mod keys;
pub mod list;
pub mod output;
pub mod read;
pub mod theme;
pub mod tui;

pub use output::Format;
pub use read::{print, watch};
pub use theme::theme;
pub use tui::tui;
