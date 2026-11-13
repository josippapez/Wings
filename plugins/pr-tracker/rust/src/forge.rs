//! What a forge is to the rest of the tracker (`lib/forges/types.ts`): how a call to one fails, and what each provider
//! module in `forges/` offers. `forges/mod.rs` hands a call to the module of the link's or the remote's provider.
//!
//! Every provider module has these as plain async functions (the TypeScript's `Forge` object), where its provider has them:
//!
//! ```text
//! problem_key(&Remote) -> String           repo_key(&Remote) -> String
//! cli() -> Cli (gh, glab, az only)         words(&Remote) -> Words      sign_in(&Remote, on_code) -> Result<(), String>
//! from_session(&str) -> Option<Link>       (GitHub only)
//! find(cwd, &Remote, branch) -> Result<Option<Link>, ForgeError>
//! me(&Remote) -> Result<Me, ForgeError>    list(cwd, &Remote) -> Result<Vec<ListedPr>, ForgeError>
//! link_of(&PrArgs) -> Result<Link, ForgeError>
//! status(&Link, fresh: bool) -> Result<Status, ForgeError>        overview(&Link) -> Result<Overview, ForgeError>
//! diff(&Link, &Status) -> Result<(String, Vec<Comment>), ForgeError>   comments(&Link) -> Result<Vec<Comment>, ForgeError>
//! failure(&Link, &Failing) -> Result<Vec<FailureDetail>, ForgeError>
//! run(&Link, &RunRef) -> Result<Run, ForgeError>                  run_log(&Link, &RunRef, &RunStep) -> Result<Vec<String>, ForgeError>
//! reply(&Link, &Comment root, &str body) -> Result<Comment, ForgeError>
//! approve(&Link, sha: &str) -> Result<(), ForgeError>             vote(&Link, &str) -> Result<(), ForgeError>   (Azure DevOps)
//! pushes(&Link) -> Result<Vec<Push>, ForgeError>                  snippet(&Link, &Comment, &[Push], Option<&str>, now: bool) -> Result<Option<Vec<SnippetLine>>, ForgeError>
//! forget()
//! ```

use std::fmt;
use wings_plugin::cli::{CliError, Kind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccountKind {
    /// There is no token, or it was refused.
    Token,
    /// The provider's CLI can't list the pull requests.
    Unavailable,
}

/// How a forge call fails. A CLI that is missing or signed out is `Cli`, with the CLI helper's kind; a token that is
/// missing or refused, and a CLI that can't list the PRs, are `Account` (`AccountProblem` in the TypeScript).
#[derive(Clone, Debug)]
pub enum ForgeError {
    Cli(CliError),
    Account { kind: AccountKind, message: String },
    Other(String),
}

impl ForgeError {
    pub fn account(kind: AccountKind, message: impl Into<String>) -> Self {
        Self::Account { kind, message: message.into() }
    }

    /// The error for `Cached` and `Batch` loads, which fail with text: it keeps what kind of error it was, and
    /// `From<String>` reads it back.
    pub fn to_wire(&self) -> String {
        match self {
            Self::Cli(error) => {
                let tag = match error.kind {
                    Kind::Missing => 'm',
                    Kind::SignedOut => 's',
                    Kind::Failed => 'f',
                };
                format!("\u{1}c{tag}{}", error.message)
            }
            Self::Account { kind, message } => format!("\u{1}a{}{message}", if *kind == AccountKind::Token { 't' } else { 'u' }),
            Self::Other(message) => message.clone(),
        }
    }

    /// Whether it says the CLI is not installed or signed out, which `problems` keeps for the pane's badge.
    pub fn is_cli_setup(&self) -> bool {
        matches!(self, Self::Cli(error) if error.kind != Kind::Failed)
    }
}

impl fmt::Display for ForgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => f.write_str(&error.message),
            Self::Account { message, .. } | Self::Other(message) => f.write_str(message),
        }
    }
}

impl From<CliError> for ForgeError {
    fn from(error: CliError) -> Self {
        Self::Cli(error)
    }
}

impl From<String> for ForgeError {
    fn from(text: String) -> Self {
        let mut chars = text.chars();
        if chars.next() == Some('\u{1}') {
            let (class, tag) = (chars.next(), chars.next());
            let message = chars.as_str().to_owned();
            match (class, tag) {
                (Some('c'), Some(tag)) => {
                    let kind = match tag {
                        'm' => Kind::Missing,
                        's' => Kind::SignedOut,
                        _ => Kind::Failed,
                    };
                    return Self::Cli(CliError { kind, message });
                }
                (Some('a'), Some(tag)) => return Self::Account { kind: if tag == 't' { AccountKind::Token } else { AccountKind::Unavailable }, message },
                _ => {}
            }
        }
        Self::Other(text)
    }
}

impl From<&str> for ForgeError {
    fn from(text: &str) -> Self {
        Self::Other(text.to_owned())
    }
}

/// What the badge and the block say when the CLI is missing or signed out.
pub struct Words {
    pub signed_out: String,
    pub missing: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_keeps_its_kind_through_a_cache_that_carries_text() {
        let signed_out = ForgeError::Cli(CliError { kind: Kind::SignedOut, message: "run gh auth login".into() });
        assert!(matches!(ForgeError::from(signed_out.to_wire()), ForgeError::Cli(CliError { kind: Kind::SignedOut, ref message }) if message == "run gh auth login"));
        let token = ForgeError::account(AccountKind::Token, "Add a token");
        assert!(matches!(ForgeError::from(token.to_wire()), ForgeError::Account { kind: AccountKind::Token, ref message } if message == "Add a token"));
        assert!(matches!(ForgeError::from("plain".to_owned()), ForgeError::Other(ref message) if message == "plain"));
    }
}
