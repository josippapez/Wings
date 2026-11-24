//! The sidebar page.

#[unsafe(no_mangle)]
pub fn start() {
    crate::views::start(crate::views::sidebar);
}
