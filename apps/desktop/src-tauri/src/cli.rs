//! `wings plugin ...`, `wings statusline`, `wings hook` and the rest of the `wings` command, which talk to the running
//! Wings over its socket. The code lives in the shared `wings-connect` crate, which the native app uses too; this puts
//! it back under the name the rest of the app uses.

pub use wings_connect::cli::*;

#[cfg(test)]
mod tests {
    #[test]
    fn identifier_matches_the_tauri_config() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], super::IDENTIFIER);
    }
}
