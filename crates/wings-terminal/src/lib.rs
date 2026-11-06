//! PTY ownership shared by Wings hosts, independent of their output transport.

pub mod shell;

mod process;
pub use process::PtyProcess;
pub use portable_pty::{CommandBuilder, ExitStatus, PtySize};

#[cfg(feature = "grid")]
pub mod grid;
#[cfg(feature = "grid")]
pub mod history;
#[cfg(feature = "grid")]
pub use alacritty_terminal;
