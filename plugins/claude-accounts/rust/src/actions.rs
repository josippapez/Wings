//! What the controls do: the calls to Wings, and loading the accounts again afterwards.

use crate::{
    accounts::{Project, account_label, load, name_problem},
    state::{Action, Note, with},
};
use serde_json::{Value, json};
use std::{cell::RefCell, future::Future};
use wings_plugin::{
    host::{self, Pending},
    messages,
    task::{SingleFlight, spawn},
};

/// A project's account changed, or an account was added: the other page loads the list again.
pub const CHANGED: &str = "accounts-changed";

/// The picker can't hold "no name", so the default account is this. It isn't a valid account name.
pub const DEFAULT: &str = "~default";

thread_local! {
    static RELOAD: SingleFlight = SingleFlight::new(refresh);
}

async fn refresh() {
    let loaded = load().await;
    with(|s| match loaded {
        Ok(data) => {
            s.data = Some(data);
            s.error = None;
        }
        Err(error) => s.error = Some(error),
    });
}

/// Loads the accounts and projects, once at a time however often it is asked.
pub fn reload() -> impl Future<Output = ()> {
    RELOAD.with(SingleFlight::call)
}

/// Reloads the list after a change and tells the other page to.
async fn changed() {
    reload().await;
    messages::tell(CHANGED, &Value::Null);
}

/// Runs `work` as the action `key`, which is busy meanwhile and keeps why it failed. Whether it worked.
pub async fn act(key: &str, work: impl Future<Output = Result<(), String>>) -> bool {
    with(|s| s.actions.insert(key.to_owned(), Action { busy: true, error: None }));
    let result = work.await;
    let worked = result.is_ok();
    with(|s| s.actions.insert(key.to_owned(), Action { busy: false, error: result.err() }));
    worked
}

/// `wings.openPane`. `account` absent runs the terminal under the project's account, `Some(None)` under the default
/// account and `Some(Some(name))` under that one.
fn open_pane(command: &str, cwd: &str, account: Option<Option<&str>>) -> Pending<Result<Value, String>> {
    let mut params = json!({ "command": command, "args": [], "cwd": cwd, "placement": "tab" });
    if let Some(account) = account {
        params["claudeAccount"] = account.into();
    }
    host::call("openPane", params)
}

fn set_account(project: &str, name: Option<&str>) -> Pending<Result<Value, String>> {
    host::call("setClaudeAccount", json!({ "projectId": project, "name": name }))
}

/// Gives `project` the account in the picker's `value`.
pub async fn pick(project: Project, value: String) {
    let (data, picked, busy) = with(|s| (s.data.clone(), s.picked.clone(), s.action("swap").busy));
    let Some(data) = data.filter(|_| !busy) else { return };
    let Some(account) = data.accounts.iter().find(|a| a.name.as_deref().unwrap_or(DEFAULT) == value) else { return };
    let shown = picked.unwrap_or_else(|| data.account_of(&project.id).map(str::to_owned));
    if account.name == shown {
        return;
    }
    with(|s| s.picked = Some(account.name.clone()));
    let worked = act("swap", async {
        set_account(&project.id, account.name.as_deref()).await?;
        changed().await;
        Ok(())
    })
    .await;
    with(|s| {
        s.picked = None;
        s.note = worked.then(|| Note { added: false, label: account_label(account) });
    });
}

/// Opens `claude` in a new terminal in the project, under the project's account.
pub async fn open_claude(project: Project) {
    act("open", async { open_pane("claude", &project.path, None).await.map(drop) }).await;
}

/// Opens `claude` in a new terminal in the project as the account named `name` (none: the default), and leaves the
/// project's own account as it is.
pub async fn start_claude(project: Project, name: Option<String>) {
    with(|s| s.starting = Some(name.clone().unwrap_or_default()));
    act("start", async { open_pane("claude", &project.path, Some(name.as_deref())).await.map(drop) }).await;
}

/// Turns on or off that the named account uses the default account's setup.
pub async fn share_setup(name: String, on: bool) {
    with(|s| s.shares.insert(name.clone(), on));
    act(&format!("share:{name}"), async {
        host::call("shareClaudeSetup", json!({ "name": name, "on": on })).await?;
        changed().await;
        Ok(())
    })
    .await;
    with(|s| s.shares.remove(&name));
}

/// The terminal a sign-in opened, followed through the panes Wings lists.
pub struct SignIn {
    pub pane: String,
    pub ran: bool,
    pub over: bool,
}

impl SignIn {
    /// True once, when the sign-in is over: `claude` has run in the pane and is gone, or the pane is closed.
    pub fn hear(&mut self, panes: &[Value]) -> bool {
        if self.over {
            return false;
        }
        let pane = panes.iter().find(|p| p["paneId"] == self.pane.as_str());
        if pane.is_some_and(|p| p["command"] == "claude") {
            self.ran = true;
        } else if pane.is_none() || self.ran {
            self.over = true;
        }
        self.over
    }
}

/// Opens a terminal in `cwd` as the account running `claude auth login`, and loads the accounts again once `claude`
/// has ended in it or the terminal was closed. The shell stays open after login, so the pane's command is what says it
/// is over; a terminal where `claude` never shows is left to the reload when Wings comes to the front.
pub async fn sign_in(key: String, name: Option<String>, cwd: String) {
    act(&key, async {
        let opened = open_pane("claude auth login", &cwd, Some(name.as_deref())).await?;
        let pane_id = opened["paneId"].as_str().unwrap_or_default().to_owned();
        // There is no way to stop listening to the panes, so the listener goes quiet once it is over.
        let watch = RefCell::new(SignIn { pane: pane_id, ran: false, over: false });
        host::on_panes(move |panes| {
            if watch.borrow_mut().hear(panes) {
                spawn(changed());
            }
        });
        Ok(())
    })
    .await;
}

/// Closes the form that adds an account, empty.
pub fn close_form() {
    with(|s| {
        s.adding = false;
        s.name.clear();
        s.submitted = false;
        s.actions.remove("add");
    });
}

/// Gives the project a new account of the typed name and opens Claude in a new terminal there to sign in.
pub async fn add_account(project: Project) {
    let (name, data) = with(|s| {
        s.submitted = true;
        (s.name.clone(), s.data.clone())
    });
    let Some(data) = data else { return };
    if name.is_empty() || name_problem(&name, &data.accounts).is_some() {
        return;
    }
    act("add", async {
        set_account(&project.id, Some(&name)).await?;
        changed().await;
        with(|s| s.note = Some(Note { added: true, label: name.clone() }));
        close_form();
        open_pane("claude", &project.path, None).await.map(drop)
    })
    .await;
}
