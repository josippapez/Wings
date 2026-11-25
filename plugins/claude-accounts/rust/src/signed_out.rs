//! Says once, in an inbox notification, that an account is signed out: when Wings starts and each time it comes to the
//! front, so it is found before the first Claude session fails with "Not logged in". The Sign in button is in the
//! Claude accounts sidebar and popover.

use crate::accounts::{Account, account_label, load};
use std::{cell::RefCell, collections::HashSet};
use wings_plugin::{host, task::spawn};

thread_local! {
    /// The accounts, by name (none: the default), already told about while they stayed signed out.
    static TOLD: RefCell<HashSet<Option<String>>> = RefCell::new(HashSet::new());
}

/// The accounts in `accounts` that are signed out and not in `told`, which then holds exactly the signed-out ones: an
/// account that signs in and out again is told again.
pub fn newly_signed_out<'a>(told: &mut HashSet<Option<String>>, accounts: &'a [Account]) -> Vec<&'a Account> {
    let out: Vec<&Account> = accounts.iter().filter(|account| !account.signed_in).collect();
    let fresh = out.iter().copied().filter(|account| !told.contains(&account.name)).collect();
    *told = out.iter().map(|account| account.name.clone()).collect();
    fresh
}

async fn check() {
    let Ok(data) = load().await else { return };
    let fresh = TOLD.with(|told| newly_signed_out(&mut told.borrow_mut(), &data.accounts));
    for account in fresh {
        drop(host::notify("Claude account signed out", &format!("{} is not signed in. Open Claude accounts and press Sign in.", account_label(account))));
    }
}

/// Checks now and each time Wings comes to the front.
pub fn watch() {
    spawn(check());
    host::on_focus(|focused| {
        if focused {
            spawn(check());
        }
    });
}
