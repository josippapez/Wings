//! What an extension is granted as it loads: no more than Wings' dialog showed the person as they added it.

use objc2::{MainThreadMarker, rc::Retained};
use objc2_foundation::{NSSet, NSString};
use objc2_web_kit::{WKWebExtensionMatchPattern, WKWebExtensionPermission};

/// Of what WebKit read that the extension asks for, what the person allowed (`allowed`: the permissions and sites Wings'
/// dialog showed): a permission by its name (`permitted`), a site when an allowed pattern covers it (`<all_urls>` covers
/// `https://*/*`). A manifest changed since asks for more, and gets no more.
pub(super) fn granted(
    permissions: &NSSet<WKWebExtensionPermission>,
    patterns: &NSSet<WKWebExtensionMatchPattern>,
    (allowed_permissions, allowed_hosts): &(Vec<String>, Vec<String>),
    mtm: MainThreadMarker,
) -> (Vec<Retained<NSString>>, Vec<Retained<WKWebExtensionMatchPattern>>) {
    // SAFETY: WebKit's own patterns, made and compared on the main thread: WebKit ends a process that makes one off it.
    unsafe {
        let allowed: Vec<_> =
            allowed_hosts.iter().filter_map(|host| WKWebExtensionMatchPattern::matchPatternWithString(&NSString::from_str(host), mtm)).collect();
        (
            permissions.iter().filter(|permission| permitted(&permission.to_string(), allowed_permissions)).collect(),
            patterns.iter().filter(|pattern| allowed.iter().any(|allowed| allowed.matchesPattern(Some(pattern)))).collect(),
        )
    }
}

/// Whether the permission `name` is granted: one the dialog showed, and never `nativeMessaging`, which reaches apps
/// outside Wings, as the dialog says (`web::Ask::words`).
fn permitted(name: &str, allowed: &[String]) -> bool {
    name != "nativeMessaging" && allowed.iter().any(|allowed| allowed == name)
}

#[cfg(test)]
mod tests {
    /// Its manifest asks for more than the dialog showed: it gets only what was shown, and never native messaging. (The
    /// sites' half is WebKit's `matchesPattern`, which only runs on the main thread, where no test runs.)
    #[test]
    fn an_extension_is_granted_only_the_permissions_the_dialog_showed() {
        let shown = ["storage", "tabs", "nativeMessaging"].map(String::from);
        let granted: Vec<&str> = ["storage", "cookies", "tabs", "nativeMessaging", "history"].into_iter().filter(|name| super::permitted(name, &shown)).collect();
        assert_eq!(granted, ["storage", "tabs"]);
    }
}
