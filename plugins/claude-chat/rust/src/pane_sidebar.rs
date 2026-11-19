//! The sidebar for a pane a Claude Code plugin opened, Wings' right-hand panel (`contributes.sidebars`, `--features pane`).
//! It is the chat's code started as that one page: `mod_pane.rs` runs and draws it.

#[unsafe(no_mangle)]
pub fn start() {
    crate::mod_pane::start_sidebar();
}
