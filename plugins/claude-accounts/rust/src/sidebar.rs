//! The sidebar page.

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    crate::views::start(crate::views::sidebar);
}
