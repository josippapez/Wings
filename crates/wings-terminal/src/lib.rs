//! PTY ownership shared by Wings hosts, independent of their output transport.

pub mod shell;

mod process;
pub use portable_pty::{CommandBuilder, ExitStatus, PtySize};
pub use process::{cwd_of, PtyProcess};

#[cfg(feature = "grid")]
pub mod grid;
#[cfg(feature = "grid")]
pub mod history;
#[cfg(feature = "grid")]
pub mod search;
