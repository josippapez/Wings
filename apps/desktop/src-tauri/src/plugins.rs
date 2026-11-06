//! Third-party plugins: a folder with `wings-plugin.json` and web code that runs sandboxed in the UI.
//! The manifest, the permission checks and the exec runner live in the shared `wings-plugins` crate, so every app
//! enforces the same rules; this puts them back under the names the rest of the app uses.

pub use wings_plugins::{exec::*, fetch::*, loading::*, manifest::*, panes::*, permissions::*, post::*, transcript::*};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn every_plugin_in_the_repo_loads() {
        wings_plugins::set_wings_version(env!("CARGO_PKG_VERSION"));
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let mut checked = 0;
        for entry in std::fs::read_dir(&root).unwrap() {
            let dir = entry.unwrap().path();
            if dir.join("wings-plugin.json").exists() {
                load(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
                checked += 1;
            }
        }
        assert!(checked > 0);
    }
}
