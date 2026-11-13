//! What is wrong with a pane's host. A CLI that is missing or signed out is kept by the CLI helper; a token that is
//! missing or refused, and a CLI that can't list the PRs, are kept here, since the helper has nothing for them.

use crate::{forge::{AccountKind, ForgeError}, forges, problem::Problem as Shown};
use std::{cell::RefCell, collections::HashMap};
use wings_plugin::{cli::Kind, remote::Remote};

#[derive(Clone, Debug, PartialEq)]
pub enum Problem {
    Token(String),
    Unavailable(String),
    /// A CLI that is missing or signed out: how the pane offers to install it or sign in.
    Missing(String),
    SignedOut(String),
}

impl Problem {
    pub fn message(&self) -> &str {
        match self {
            Self::Token(message) | Self::Unavailable(message) | Self::Missing(message) | Self::SignedOut(message) => message,
        }
    }
}

thread_local! {
    /// Problem key → what a forge's own code found wrong.
    static OWN: RefCell<HashMap<String, (AccountKind, String)>> = RefCell::new(HashMap::new());
    /// Problem key → the one-time code of the sign-in running for it, which the badge shows.
    static CODES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// What is wrong with the host of a remote, or none.
pub fn problem_of(remote: Option<&Remote>) -> Option<Problem> {
    let remote = remote.filter(|remote| forges::is_known(remote))?;
    let key = forges::problem_key(remote);
    if let Some((kind, _)) = forges::provider_of(remote).and_then(forges::cli_of).and_then(|cli| cli.problem(&key)) {
        let words = forges::words(remote);
        return Some(if kind == Kind::Missing { Problem::Missing(words.missing) } else { Problem::SignedOut(words.signed_out) });
    }
    OWN.with(|own| own.borrow().get(&key).map(|(kind, message)| if *kind == AccountKind::Token { Problem::Token(message.clone()) } else { Problem::Unavailable(message.clone()) }))
}

/// Notes what an error from a host says about its account, and tells whether it was that, which leaves a pane with no PR
/// to show and its badge saying why, rather than a failure.
pub fn explains(remote: &Remote, error: &ForgeError) -> bool {
    if let ForgeError::Account { kind, message } = error {
        OWN.with(|own| own.borrow_mut().insert(forges::problem_key(remote), (*kind, message.clone())));
        return true;
    }
    // The CLI helper has kept a CLI that is missing or signed out.
    error.is_cli_setup()
}

/// The host answered, so whatever its token or CLI had wrong is over.
pub fn answered(remote: &Remote) {
    OWN.with(|own| own.borrow_mut().remove(&forges::problem_key(remote)));
}

/// What a block or a tab shows for a host that needs signing in, a token or a CLI. None for anything else.
pub fn shown_problem(remote: Option<&Remote>) -> Option<Shown> {
    let (message, action) = match problem_of(remote)? {
        Problem::Missing(message) => (message, "Install"),
        Problem::SignedOut(message) => (message, "Sign in"),
        Problem::Token(message) => (message, "Add token"),
        Problem::Unavailable(_) => return None,
    };
    Some(Shown { message, action: action.to_owned() })
}

pub fn code_for(key: &str) -> Option<String> {
    CODES.with(|codes| codes.borrow().get(key).cloned())
}

pub fn set_code(key: &str, code: &str) {
    CODES.with(|codes| codes.borrow_mut().insert(key.to_owned(), code.to_owned()));
}

pub fn clear_code(key: &str) {
    CODES.with(|codes| codes.borrow_mut().remove(key));
}

pub fn forget_problems() {
    OWN.with(|own| own.borrow_mut().clear());
    CODES.with(|codes| codes.borrow_mut().clear());
}
