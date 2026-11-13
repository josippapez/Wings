//! Calling a forge that takes a token (Bitbucket Cloud, Gitea/Forgejo) with `wings.fetch`, which sends the token from the keychain as a Bearer header.

use crate::forge::{AccountKind, ForgeError};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap};
use wings_plugin::host;

/// A host reached with a token.
pub struct Account {
    /// Where its problem is kept, like `bitbucket`.
    pub key: String,
    /// The name of the secret the token is saved under in Settings.
    pub secret: String,
    pub name: String,
    /// What a 403 most likely means, since a token without the scope is refused with it.
    pub scope_hint: &'static str,
    /// The same for a write (a reply, an approval), which needs its own scopes.
    pub write_hint: &'static str,
}

thread_local! {
    /// The signed-in user's id per secret, so mine-or-review is decided without asking again.
    static USERS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn needs_token(message: String) -> ForgeError {
    ForgeError::account(AccountKind::Token, message)
}

/// What `wings.fetch` answers for a call that sends the account's token.
async fn send(account: &Account, url: &str, method: &str, headers: Value, body: Option<String>) -> Result<(u16, String), ForgeError> {
    let mut params = json!({ "url": url, "method": method, "headers": headers, "bearer": account.secret });
    if let Some(body) = body {
        params["body"] = body.into();
    }
    let answer = host::call("fetch", params).await?;
    Ok((answer["status"].as_u64().unwrap_or(0) as u16, answer["body"].as_str().unwrap_or_default().to_owned()))
}

/// The body of a successful GET. A missing or refused token is an `Account` error; any other failure just fails.
pub async fn forge_get(account: &Account, url: &str, accept: &str) -> Result<String, ForgeError> {
    if !host::secret_has(&account.secret).await? {
        return Err(needs_token(format!("Add a {} token in Settings to see this branch's pull request.", account.name)));
    }
    let (status, body) = send(account, url, "GET", json!({ "Accept": accept }), None).await?;
    match status {
        401 => Err(needs_token(format!("{} refused the token. Add a new one in Settings.", account.name))),
        403 => Err(format!("{} refused the request (403). {}", account.name, account.scope_hint).into()),
        404 => Err(format!("{} found no such repository, or the token can't see it.", account.name).into()),
        200..=299 => Ok(body),
        _ => Err(format!("{} answered {status}.", account.name).into()),
    }
}

/// POSTs a JSON body (none for an action like approve) and gives the answer's body. Only a URL that the manifest's
/// `POST` entries name gets through, with the body they pin.
pub async fn forge_post(account: &Account, url: &str, body: Option<&Value>) -> Result<String, ForgeError> {
    if !host::secret_has(&account.secret).await? {
        return Err(needs_token(format!("Add a {} token in Settings to reply or review.", account.name)));
    }
    let headers = if body.is_some() { json!({ "Accept": "application/json", "Content-Type": "application/json" }) } else { json!({ "Accept": "application/json" }) };
    let (status, text) = send(account, url, "POST", headers, body.map(Value::to_string)).await?;
    match status {
        401 => Err(needs_token(format!("{} refused the token. Add a new one in Settings.", account.name))),
        403 => Err(format!("{} refused it (403). {}", account.name, account.write_hint).into()),
        404 => Err(format!("{} found no such pull request, or the token can't see it.", account.name).into()),
        200..=299 => Ok(text),
        _ => Err(format!("{} answered {status}: {}", account.name, reason(&text)).into()),
    }
}

/// The reason a forge gave in an error body, for the user to read.
fn reason(body: &str) -> String {
    let text = match serde_json::from_str::<Value>(body) {
        Ok(found) => found["error"]["message"].as_str().or(found["message"].as_str()).map_or_else(|| body.to_owned(), str::to_owned),
        Err(_) => body.to_owned(),
    };
    text.chars().take(200).collect()
}

pub async fn forge_json(account: &Account, url: &str) -> Result<Value, ForgeError> {
    let body = forge_get(account, url, "application/json").await?;
    serde_json::from_str(&body).map_err(|error| ForgeError::Other(error.to_string()))
}

/// The id of the account the token belongs to, which `pick` reads from the answer of `url`.
pub async fn forge_user(account: &Account, url: &str, pick: fn(&Value) -> Option<String>) -> Result<String, ForgeError> {
    if let Some(id) = USERS.with(|users| users.borrow().get(&account.secret).cloned()) {
        return Ok(id);
    }
    let id = pick(&forge_json(account, url).await?).filter(|id| !id.is_empty()).ok_or_else(|| ForgeError::Other(format!("{} didn't say who the token belongs to.", account.name)))?;
    USERS.with(|users| users.borrow_mut().insert(account.secret.clone(), id.clone()));
    Ok(id)
}

/// Forgets the users, so a token that was replaced is read again.
pub fn forget_users() {
    USERS.with(|users| users.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use wings_plugin::task::block_on;

    fn account() -> Account {
        Account { key: "bitbucket".into(), secret: "bitbucket".into(), name: "Bitbucket".into(), scope_hint: "scopes", write_hint: "write scopes" }
    }

    fn answer_with(status: u64, body: &'static str, has: bool) {
        host::testing::answer(move |method, params| match method {
            "secretHas" => Ok(json!(has)),
            "fetch" => {
                assert_eq!(params["bearer"], "bitbucket");
                Ok(json!({ "status": status, "body": body }))
            }
            _ => Err("unexpected".into()),
        });
    }

    #[test]
    fn a_missing_or_refused_token_is_an_account_problem_and_other_failures_just_fail() {
        answer_with(200, "{}", false);
        let missing = block_on(async { forge_get(&account(), "https://x", "application/json").await }).unwrap_err();
        assert!(matches!(&missing, ForgeError::Account { kind: AccountKind::Token, message } if message == "Add a Bitbucket token in Settings to see this branch's pull request."));
        answer_with(401, "", true);
        assert!(matches!(block_on(async { forge_get(&account(), "https://x", "application/json").await }), Err(ForgeError::Account { .. })));
        answer_with(403, "", true);
        assert_eq!(block_on(async { forge_get(&account(), "https://x", "application/json").await }).unwrap_err().to_string(), "Bitbucket refused the request (403). scopes");
        answer_with(500, "", true);
        assert_eq!(block_on(async { forge_get(&account(), "https://x", "application/json").await }).unwrap_err().to_string(), "Bitbucket answered 500.");
    }

    #[test]
    fn a_failed_post_says_why_the_forge_gave_and_a_user_is_asked_once() {
        answer_with(422, r#"{"error":{"message":"No parent"}}"#, true);
        assert_eq!(block_on(async { forge_post(&account(), "https://x", None).await }).unwrap_err().to_string(), "Bitbucket answered 422: No parent");
        answer_with(200, r#"{"uuid":"{me}"}"#, true);
        let pick: fn(&Value) -> Option<String> = |user| user["uuid"].as_str().map(str::to_owned);
        assert_eq!(block_on(async move { forge_user(&account(), "https://x/user", pick).await }).unwrap(), "{me}");
        answer_with(500, "", true);
        assert_eq!(block_on(async move { forge_user(&account(), "https://x/user", pick).await }).unwrap(), "{me}");
        forget_users();
        assert!(block_on(async move { forge_user(&account(), "https://x/user", pick).await }).is_err());
    }
}
