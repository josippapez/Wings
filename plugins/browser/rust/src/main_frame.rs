//! The main script: it answers the agent tools (`agent_tools.rs`). The tabs and the settings page do the rest.

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    crate::agent_tools::start();
}
