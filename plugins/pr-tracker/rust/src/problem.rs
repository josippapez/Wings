//! What a block or a tab shows for a host that needs signing in, a token or a CLI: a short message and its button.

use crate::{forge::{AccountKind, ForgeError}, forges};
use serde::{Deserialize, Serialize};
use wings_plugin::{cli::Kind, remote::Remote};

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Problem {
    pub message: String,
    /// The label of the button: `Sign in`, `Add token` or `Install`.
    pub action: String,
}

/// The problem an error says its host has, or none for an error that is only a failure.
pub fn shown(remote: &Remote, error: &ForgeError) -> Option<Problem> {
    let (message, action) = match error {
        ForgeError::Cli(error) if error.kind == Kind::Missing => (forges::words(remote).missing, "Install"),
        ForgeError::Cli(error) if error.kind == Kind::SignedOut => (forges::words(remote).signed_out, "Sign in"),
        ForgeError::Account { kind: AccountKind::Token, message } => (message.clone(), "Add token"),
        _ => return None,
    };
    Some(Problem { message, action: action.to_owned() })
}
