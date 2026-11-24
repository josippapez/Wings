//! The popover page under the title bar button.

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    crate::views::start(crate::views::popover);
}
