//! The accounts Wings knows, the projects that use them, and the names an account may have.

use serde_json::{Value, json};
use std::collections::HashMap;
use wings_plugin::{host, task::join};

#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    /// `None` is the default account, `~/.claude`.
    pub name: Option<String>,
    pub dir: String,
    pub email: Option<String>,
    pub signed_in: bool,
    pub shares_setup: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
}

impl Project {
    pub fn from_json(value: &Value) -> Self {
        let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
        Self { id: text("id"), name: text("name"), path: text("path") }
    }
}

/// Everything the pages show: the accounts, which account each project has (absent: the default), and the projects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    pub accounts: Vec<Account>,
    pub assigned: HashMap<String, String>,
    pub projects: Vec<Project>,
}

impl Loaded {
    /// The account a project uses: its name, or `None` for the default.
    pub fn account_of(&self, project_id: &str) -> Option<&str> {
        self.assigned.get(project_id).map(String::as_str)
    }

    /// The names of the projects that use `account`.
    pub fn projects_using(&self, account: &Account) -> Vec<&str> {
        self.projects.iter().filter(|p| self.account_of(&p.id) == account.name.as_deref()).map(|p| p.name.as_str()).collect()
    }
}

/// What a picker and a message call an account: its email when known, else its name, and "Default" for `~/.claude`.
pub fn account_label(account: &Account) -> String {
    match &account.email {
        None => account.name.clone().unwrap_or_else(|| "Default".into()),
        Some(email) => format!("{email} ({})", account.name.as_deref().unwrap_or("default")),
    }
}

/// What a button that starts Claude as `account` calls it: its email when known, else its name, and "Default" for `~/.claude`.
pub fn start_label(account: &Account) -> String {
    account.email.clone().or_else(|| account.name.clone()).unwrap_or_else(|| "Default".into())
}

/// What the account list says under a name: who it is, or that nobody is signed in (with who it was, as the config still names them).
pub fn sign_in_label(account: &Account) -> String {
    if account.signed_in {
        return account.email.clone().unwrap_or_else(|| "Signed in".into());
    }
    match &account.email {
        Some(email) => format!("Not signed in (was {email})"),
        None => "Not signed in".into(),
    }
}

/// Why `name` can't be an account's name, or `None` when it can. What Wings accepts is `[a-z0-9][a-z0-9-]{0,31}`.
pub fn name_problem(name: &str, existing: &[Account]) -> Option<String> {
    if name.chars().count() > 32 {
        return Some("Use 32 characters or fewer.".into());
    }
    if !name.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-')) {
        return Some("Use only lowercase letters, digits and dashes.".into());
    }
    if name.starts_with('-') {
        return Some("Start with a letter or a digit.".into());
    }
    if existing.iter().any(|a| a.name.as_deref() == Some(name)) {
        return Some(format!("There is an account called {name} already."));
    }
    None
}

/// The accounts and projects, as Wings has them now.
pub async fn load() -> Result<Loaded, String> {
    let (accounts, projects) = join(host::call("claudeAccounts", json!({})), host::projects()).await;
    let (accounts, projects) = (accounts?, projects?);
    Ok(Loaded {
        accounts: accounts["accounts"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|a| Account {
                name: a["name"].as_str().map(str::to_owned),
                dir: a["dir"].as_str().unwrap_or_default().to_owned(),
                email: a["email"].as_str().map(str::to_owned),
                signed_in: a["signedIn"] == true,
                shares_setup: a["sharesSetup"] == true,
            })
            .collect(),
        assigned: accounts["projects"]
            .as_object()
            .map(|map| map.iter().filter_map(|(id, name)| Some((id.clone(), name.as_str()?.to_owned()))).collect())
            .unwrap_or_default(),
        projects: projects.as_array().map(Vec::as_slice).unwrap_or_default().iter().map(Project::from_json).collect(),
    })
}
