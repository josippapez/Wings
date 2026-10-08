//! Plugin secrets, like API tokens, kept in the system keychain. A plugin can store one and have
//! `wings.fetch` send it, but never read it back, so a token pasted once stays out of plugin code.

const SERVICE: &str = "dev.wings.app.plugin";

pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn entry(plugin_id: &str, name: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(&format!("{SERVICE}.{plugin_id}"), name).map_err(|e| e.to_string())
}

pub fn set(plugin_id: &str, name: &str, value: &str) -> Result<(), String> {
    entry(plugin_id, name)?.set_password(value).map_err(|e| e.to_string())
}

pub fn get(plugin_id: &str, name: &str) -> Result<Option<String>, String> {
    match entry(plugin_id, name)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn delete(plugin_id: &str, name: &str) -> Result<(), String> {
    match entry(plugin_id, name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

