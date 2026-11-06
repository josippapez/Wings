//! Plugin secrets, like API tokens, kept in the system keychain where the desktop app keeps them: the same service name
//! per plugin and install scope, so a token saved in either app is there for both. A plugin can store one and have
//! `wings.fetch` send it, but never read it back.
//!
//! The desktop also notes the names a plugin stored in `plugins.json`, to delete them when the plugin is removed. This
//! app never writes that file, so a token saved here is not on that list.

const SERVICE: &str = "dev.wings.app.plugin";

fn entry(plugin: &str, scope: &str, name: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(&format!("{SERVICE}.{plugin}.{scope}"), name).map_err(|e| e.to_string())
}

pub fn set(plugin: &str, scope: &str, name: &str, value: &str) -> Result<(), String> {
    entry(plugin, scope, name)?
        .set_password(value)
        .map_err(|e| e.to_string())
}

pub fn get(plugin: &str, scope: &str, name: &str) -> Result<Option<String>, String> {
    match entry(plugin, scope, name)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn delete(plugin: &str, scope: &str, name: &str) -> Result<(), String> {
    match entry(plugin, scope, name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
