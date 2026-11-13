//! What the main script knows about the panes' and the lists' pull requests, one module per concern (`src/main/`).
pub mod badges;
pub mod diff;
pub mod failing;
pub mod inbox;
pub mod links;
pub mod news;
pub mod problems;
pub mod projects;
pub mod remote;
pub mod retry;
pub mod state;
pub mod status;
pub mod tab;
#[cfg(test)]
mod budget;
