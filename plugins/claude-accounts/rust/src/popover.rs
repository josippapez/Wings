//! The popover page under the title bar button.

#[unsafe(no_mangle)]
pub fn start() {
    crate::views::start(crate::views::popover);
}
