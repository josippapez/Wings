use std::path::{Component, Path, PathBuf};

/// A relative path made only of normal components, or `None` for an absolute path, `..`, a drive prefix or a backslash
/// (Windows tools write those, and `..\` would escape on a machine that reads it as a separator).
pub(crate) fn safe_relative(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('\\') || name.contains('\0') {
        return None;
    }
    let mut out = PathBuf::new();
    for c in Path::new(name).components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}
