//! The sidebar for a subagent, Wings' right-hand panel (`contributes.sidebars`, `--features subagent`). It is the chat's
//! code started as that one page: `subagent_tab.rs` runs it and `subagent_view.rs` draws it.

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    crate::subagent_tab::start_sidebar();
}
