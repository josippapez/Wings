//! The sidebar for the session of the chat in front, Wings' right-hand panel (`contributes.sidebars`, `--features
//! session`). It is the chat's code started as that one page: `session_side.rs` runs and draws it.

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    crate::session_side::start_sidebar();
}
