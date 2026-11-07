//! Third-party plugins: a folder with `wings-plugin.json` and web code that runs sandboxed in the UI.
//! The manifest, the permission checks and the exec runner live in the shared `wings-plugins` crate, so every app
//! enforces the same rules; this puts them back under the names the rest of the app uses.

pub use wings_plugins::{exec::*, fetch::*, loading::*, manifest::*, panes::*, permissions::*, post::*, transcript::*};

/// The name of a Claude Code account a plugin passes (`shareClaudeSetup`), once the plugin may use accounts and the name is
/// one: up to 32 lowercase letters, digits and dashes. Nothing else becomes part of a folder name.
pub fn claude_account_name<'a>(plugin: &Plugin, name: &'a str) -> Result<&'a str, String> {
    if !may_use_claude_accounts(plugin) {
        return Err(format!("{} may not use Claude Code accounts", plugin.manifest.id));
    }
    if !crate::claude_accounts::valid_name(name) {
        return Err(format!("{name:?} isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit"));
    }
    Ok(name)
}

/// The account `openPane` asked for, checked: none asked for (the project's own) is always fine; any other needs the
/// `claudeAccounts` permission, and a name has to be one. The default account (`name: None`) takes no name.
pub fn pane_account(plugin: &Plugin, pick: Option<&crate::claude_accounts::AccountPick>) -> Result<(), String> {
    match pick {
        None => Ok(()),
        Some(pick) => match &pick.name {
            Some(name) => claude_account_name(plugin, name).map(|_| ()),
            None if may_use_claude_accounts(plugin) => Ok(()),
            None => Err(format!("{} may not use Claude Code accounts", plugin.manifest.id)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::{fs, path::Path};

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

    #[test]
    fn a_plugin_names_a_claude_account_only_with_the_permission_and_a_valid_name() {
        use crate::claude_accounts::AccountPick;
        let dir = std::env::temp_dir().join(format!("wings-claude-account-names-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |permissions: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": permissions });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir).unwrap()
        };
        let (without, with) = (manifest(serde_json::json!({ "panes": ["claude"] })), manifest(serde_json::json!({ "claudeAccounts": true })));
        let pick = |name: Option<&str>| AccountPick { name: name.map(String::from) };
        // shareClaudeSetup
        assert_eq!(claude_account_name(&with, "work"), Ok("work"));
        assert!(claude_account_name(&without, "work").is_err());
        for bad in ["", "../x", "Work", "a/b", "-a", "a b"] {
            assert!(claude_account_name(&with, bad).is_err(), "{bad:?}");
        }
        // openPane: an account of the project's own needs nothing; another, or the default, needs the permission.
        assert!(pane_account(&without, None).is_ok());
        assert!(pane_account(&without, Some(&pick(Some("work")))).is_err());
        assert!(pane_account(&without, Some(&pick(None))).is_err());
        assert!(pane_account(&with, Some(&pick(Some("work")))).is_ok());
        assert!(pane_account(&with, Some(&pick(None))).is_ok());
        assert!(pane_account(&with, Some(&pick(Some("../etc")))).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
