//! The rest of Wings' calls and messages as typed functions, used as `host::<name>`: panes, tabs, the clipboard,
//! tools of other plugins, events between plugins, worktrees and Claude Code accounts. Each sends the params the app's
//! `plugin/calls.rs` reads, which the tests below pin.
//!
//! `tests/host_coverage.rs` fails when the app accepts a call or sends a message that has no function in the SDK.

use super::{Pending, call, on};
use serde_json::{Map, Value, json};

type Answer = Pending<Result<Value, String>>;

/// `wings.focusPane`: brings the pane `pane` to the front. Needs `permissions.panes`.
pub fn focus_pane(pane: &str) -> Answer {
    call("focusPane", json!({ "paneId": pane }))
}

/// `wings.copyText`: puts `text` on the clipboard, only right after your click or key in the page and for up to 16 MB.
pub fn copy_text(text: &str) -> Answer {
    call("copyText", json!({ "text": text }))
}

/// `wings.closeView`: closes the plugin's sidebar or view `id`, wherever Wings shows it.
pub fn close_view(id: &str) -> Answer {
    call("closeView", json!({ "id": id }))
}

/// `wings.project`: the project this page is about, `{ id, name, path, ... }`, or null. A tab's page gets its own tab's
/// project, not the one on screen.
pub fn project() -> Answer {
    call("project", json!({}))
}

/// Hears the project on screen, as `project` answers it (or null), now and on every change.
pub fn on_project(listener: impl Fn(&Value) + 'static) {
    on("project", move |_, project| listener(project));
}

/// `wings.tabState`: what this tab's page saved with `set_tab_state`, or null. Only a page shown as a tab has one.
pub fn tab_state() -> Answer {
    call("tabState", json!({}))
}

/// `wings.setTabState`: saves any JSON for this tab, which Wings gives back when the tab opens again.
pub fn set_tab_state(state: &Value) -> Answer {
    call("setTabState", json!({ "state": state }))
}

/// `wings.openSettings`: opens the plugin's settings page. Needs `contributes.settings`.
pub fn open_settings() -> Answer {
    call("openSettings", json!({}))
}

/// `wings.emit`: an event `name` (lowercase letters, digits and dashes, up to 64) with up to 16 KB of JSON `data` that the
/// other plugins hear with `on_event`.
pub fn emit(name: &str, data: &Value) -> Answer {
    call("emit", json!({ "name": name, "data": data }))
}

/// Hears an event another plugin `emit`ted: who sent it, its name and its data.
pub fn on_event(listener: impl Fn(&str, &str, &Value) + 'static) {
    on("pluginEvent", move |_, event| listener(event["from"].as_str().unwrap_or_default(), event["name"].as_str().unwrap_or_default(), &event["data"]));
}

/// `wings.sendToAgent`: a message of up to 8 KB typed to the agent in the pane `pane`, as a message from this plugin. Needs
/// `permissions.sendToAgents`.
pub fn send_to_agent(pane: &str, text: &str) -> Answer {
    call("sendToAgent", json!({ "paneId": pane, "text": text }))
}

/// `wings.callTool`: runs the tool `name` (`"<plugin>__<tool>"`, like `tasks__list`) of another of Wings' own plugins with
/// `arguments`. Needs `permissions.callTools` naming it.
pub fn call_tool(name: &str, arguments: &Value) -> Answer {
    call("callTool", json!({ "name": name, "arguments": arguments }))
}

/// `wings.setSectionSort`: confirms the sort `sort` the plugin's project section `section` declared as the one in use.
pub fn set_section_sort(section: &str, sort: &str) -> Answer {
    call("setSectionSort", json!({ "sectionId": section, "sort": sort }))
}

/// Hears a sort the user picked in a project section's menu: the section and the sort. Answer with `set_section_sort`.
pub fn on_section_sort(listener: impl Fn(&str, &str) + 'static) {
    on("sectionSort", move |_, picked| listener(picked["sectionId"].as_str().unwrap_or_default(), picked["sort"].as_str().unwrap_or_default()));
}

/// `wings.agentActivity`: what the OpenCode session `session` is doing now. Needs `permissions.agentActivity`.
pub fn agent_activity(session: &str) -> Answer {
    call("agentActivity", json!({ "sessionId": session }))
}

/// Hears an OpenCode session that did something: its id.
pub fn on_agent_activity(listener: impl Fn(&str) + 'static) {
    on("agentActivity", move |_, did| listener(did["sessionId"].as_str().unwrap_or_default()));
}

/// `wings.notify` with buttons and a project: `actions` is a list of up to 3 `{ label, ... }` that open one of the plugin's
/// tab views or an https link, `project_id` the project the notification's tabs open in.
pub fn notify_with(title: &str, body: &str, actions: Option<&[Value]>, project_id: Option<&str>) -> Answer {
    call("notify", json!({ "title": title, "body": body, "actions": actions, "projectId": project_id }))
}

/// `wings.post` through the CLI `program` (`gh` or `az`) for a call `permissions.post` names: `{ code, stdout, stderr }`.
/// `host` names a GitHub Enterprise host.
pub fn post(program: &str, url: &str, host: Option<&str>, fields: &Value) -> Answer {
    call("post", json!({ "program": program, "url": url, "host": host, "fields": fields }))
}

/// A terminal `wings.openPane` opens: add what it runs and where, then `open`.
#[derive(Default)]
pub struct OpenPane<'a> {
    command: Option<&'a str>,
    args: &'a [&'a str],
    cwd: Option<&'a str>,
    placement: Option<&'a str>,
    background: bool,
    account: Option<Option<&'a str>>,
}

impl<'a> OpenPane<'a> {
    /// A shell in the project on screen, in a new tab.
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `command` with `args`, which the manifest's `permissions.panes` has to allow.
    pub fn command(mut self, command: &'a str, args: &'a [&'a str]) -> Self {
        self.command = Some(command);
        self.args = args;
        self
    }

    /// In the folder `cwd`, which has to be in one of your projects or a repo an open pane is in.
    pub fn cwd(mut self, cwd: &'a str) -> Self {
        self.cwd = Some(cwd);
        self
    }

    /// `"tab"` (the default), `"right"`, `"down"` or `"replace"`, which takes the place of this page's tab.
    pub fn placement(mut self, placement: &'a str) -> Self {
        self.placement = Some(placement);
        self
    }

    /// In the project's tabs without showing it: the project, tab and keyboard on screen stay as they are. Only for a tab.
    pub fn background(mut self) -> Self {
        self.background = true;
        self
    }

    /// Under the Claude Code account `name`, or with `None` the default one. Needs `permissions.claudeAccounts`.
    pub fn claude_account(mut self, name: Option<&'a str>) -> Self {
        self.account = Some(name);
        self
    }

    fn params(&self) -> Value {
        let mut params = json!({ "command": self.command, "args": self.args, "cwd": self.cwd, "placement": self.placement.unwrap_or("tab") });
        if self.background {
            params["background"] = true.into();
        }
        if let Some(account) = self.account {
            params["claudeAccount"] = account.into();
        }
        params
    }

    /// Opens it. The call goes out now.
    pub fn open(&self) -> Answer {
        call("openPane", self.params())
    }
}

/// A setting `set_worktree_setup` leaves alone, puts back to its default, or sets.
pub enum Change<T> {
    Keep,
    Reset,
    Set(T),
}

impl<T> Change<T> {
    fn put(self, params: &mut Map<String, Value>, key: &str, set: impl FnOnce(T) -> Value) {
        match self {
            Change::Keep => {}
            Change::Reset => {
                params.insert(key.to_owned(), Value::Null);
            }
            Change::Set(value) => {
                params.insert(key.to_owned(), set(value));
            }
        }
    }
}

fn project_params(project: Option<&str>) -> Map<String, Value> {
    let mut params = Map::new();
    params.insert("projectId".to_owned(), project.into());
    params
}

/// `wings.createWorktree`: a worktree of the project `project` (the one on screen for `None`) on the new branch `branch`,
/// from `base`, listed under that project. Answers `{ projectId, path }`. Needs `permissions.worktrees`.
pub fn create_worktree(project: Option<&str>, branch: &str, base: Option<&str>) -> Answer {
    let mut params = project_params(project);
    params.insert("branch".to_owned(), branch.into());
    params.insert("base".to_owned(), base.into());
    call("createWorktree", Value::Object(params))
}

/// `wings.worktreeSetup`: the files a project's new worktrees get copied in, and the line that runs in them. Needs
/// `permissions.worktrees`.
pub fn worktree_setup(project: Option<&str>) -> Answer {
    call("worktreeSetup", Value::Object(project_params(project)))
}

/// `wings.setWorktreeSetup`: what a project's new worktrees get: the list of files to copy in, and the setup line, each left
/// as it is, put back to its default (clears the line), or set. Only Wings' own plugins may set the line.
pub fn set_worktree_setup(project: Option<&str>, include: Change<&[&str]>, setup: Change<&str>) -> Answer {
    let mut params = project_params(project);
    include.put(&mut params, "include", |lines| json!(lines));
    setup.put(&mut params, "setup", Value::from);
    call("setWorktreeSetup", Value::Object(params))
}

/// `wings.claudeAccounts`: `{ accounts, projects }`, the Claude Code accounts with their folders and the projects each
/// one is set for. Needs `permissions.claudeAccounts`; `accounts` is the quick list.
pub fn claude_accounts() -> Answer {
    call("claudeAccounts", json!({}))
}

/// `wings.setClaudeAccount`: the account the project `project` (an `id` of `projects`) starts new terminals under, or the
/// default one for `None`. Needs `permissions.claudeAccounts`.
pub fn set_claude_account(project: &str, name: Option<&str>) -> Answer {
    call("setClaudeAccount", json!({ "projectId": project, "name": name }))
}

/// `wings.shareClaudeSetup`: whether the named account `name` uses the default account's setup (settings, plugins and
/// the like). Needs `permissions.claudeAccounts`.
pub fn share_claude_setup(name: &str, on: bool) -> Answer {
    call("shareClaudeSetup", json!({ "name": name, "on": on }))
}

/// `wings.resizeWindow`: sizes the window of a page that `open_view` opened `as: "window"` to `width` by `height`
/// points, kept on the screen and no smaller than 320 by 240; answers `{ width, height }`, the size it got. Wings 0.2.113.
pub fn resize_window(width: f64, height: f64) -> Answer {
    call("resizeWindow", json!({ "width": width, "height": height }))
}

/// `wings.webData`: clears the cookies and site data the plugin's web views kept. Needs `permissions.webView`.
pub fn web_data_clear() -> Answer {
    call("webData", json!({ "action": "clear" }))
}

/// `wings.webExtensions` `list`: `{ supported, extensions: [{ id, name, version, description, enabled, icon, permissions,
/// hosts, error }] }`. `supported` is false before macOS 15.4, and `icon` is an image id an `ui::image` draws. Needs
/// `permissions.webView`.
pub fn web_extensions() -> Answer {
    call("webExtensions", json!({ "action": "list" }))
}

/// `wings.webExtensions` `installed`: the extensions already installed in the user's other browsers, as `[{ source,
/// browser, profile, name, description, version, icon, permissions, hosts }]`.
pub fn web_extensions_installed() -> Answer {
    call("webExtensions", json!({ "action": "installed" }))
}

/// Where `web_extension_stage` gets an extension from.
pub enum Stage<'a> {
    /// A folder or ZIP the user picks: only right after a click, and the answer is null when they cancel.
    Pick,
    /// One of `web_extensions_installed`'s `source`s.
    Browser(&'a str),
    /// A Chrome Web Store or addons.mozilla.org page.
    Store(&'a str),
}

/// `wings.webExtensions` `stage`: copies, downloads and unpacks an extension without installing it, and answers `{ stage,
/// name, version, description, icon, permissions, hosts }` (null when a picker was cancelled). Show the name and what it
/// asks for, then `web_extension_add` or `web_extension_discard` the `stage`.
pub fn web_extension_stage(from: Stage) -> Answer {
    call(
        "webExtensions",
        match from {
            Stage::Pick => json!({ "action": "stage", "from": "pick" }),
            Stage::Browser(source) => json!({ "action": "stage", "from": "browser", "source": source }),
            Stage::Store(url) => json!({ "action": "stage", "from": "store", "url": url }),
        },
    )
}

/// `wings.webExtensions` `add`: installs a staged extension, only right after a click. Answers the new entry.
pub fn web_extension_add(stage: &str) -> Answer {
    call("webExtensions", json!({ "action": "add", "stage": stage }))
}

/// `wings.webExtensions` `discard`: drops a staged extension.
pub fn web_extension_discard(stage: &str) -> Answer {
    call("webExtensions", json!({ "action": "discard", "stage": stage }))
}

/// `wings.webExtensions` `setEnabled`: turns the extension `id` on or off.
pub fn web_extension_set_enabled(id: &str, enabled: bool) -> Answer {
    call("webExtensions", json!({ "action": "setEnabled", "id": id, "enabled": enabled }))
}

/// `wings.webExtensions` `remove`: removes the extension `id`.
pub fn web_extension_remove(id: &str) -> Answer {
    call("webExtensions", json!({ "action": "remove", "id": id }))
}

/// `wings.webExtensions` `storePage`: whether an address is a Chrome Web Store or addons.mozilla.org extension page,
/// as `{ store: "chrome" | "firefox", id }`, or null.
pub fn web_extension_store_page(url: &str) -> Answer {
    call("webExtensions", json!({ "action": "storePage", "url": url }))
}

/// `wings.browserCookies` `sources`: the other browsers' profiles whose cookies can be imported, `[{ id, browser,
/// profile }]`, with no prompt. Needs `permissions.browserCookies`.
pub fn browser_cookie_sources() -> Answer {
    call("browserCookies", json!({ "action": "sources" }))
}

/// `wings.browserCookies` `import`: copies a source's cookies into the plugin's web views, as `{ imported, skipped }`.
/// Only right after a click. It can take a while and may show a Keychain prompt; an error is a message for the user
/// (like needing Full Disk Access). `keep_data` is whether the cookies are kept after Wings quits.
pub fn browser_cookies_import(source: &str, keep_data: bool) -> Answer {
    call("browserCookies", json!({ "action": "import", "source": source, "keepData": keep_data }))
}

#[cfg(test)]
mod tests {
    use super::super::{testing, tests::record};
    use super::*;
    use crate::task::run_ready;
    use std::{cell::RefCell, rc::Rc};

    /// What each function sends, against what the app's `calls.rs` reads (`paneId`, `text`, `id`, `state`, `name`, `data`,
    /// `arguments`, `sectionId`, `sort`, `sessionId`, `projectId`, `on`...).
    #[test]
    fn each_call_sends_the_params_the_app_reads() {
        let calls = record(&[]);
        drop(focus_pane("7"));
        drop(copy_text("hi"));
        drop(close_view("board"));
        drop(project());
        drop(tab_state());
        drop(set_tab_state(&json!({ "folded": [1] })));
        drop(open_settings());
        drop(emit("open-changes", &json!({ "cwd": "/r" })));
        drop(send_to_agent("7", "go"));
        drop(call_tool("tasks__list", &json!({ "cwd": "/r" })));
        drop(set_section_sort("issues", "date"));
        drop(agent_activity("ses_1"));
        drop(notify_with("T", "B", Some(&[json!({ "label": "Open" })][..]), Some("/r")));
        drop(post("az", "https://x", None, &json!({ "a": 1 })));
        drop(claude_accounts());
        drop(set_claude_account("/r", None));
        drop(share_claude_setup("work", true));
        drop(web_data_clear());
        drop(web_extensions());
        drop(web_extensions_installed());
        drop(web_extension_stage(Stage::Pick));
        drop(web_extension_stage(Stage::Browser("chrome:Default")));
        drop(web_extension_stage(Stage::Store("https://x")));
        drop(web_extension_add("s1"));
        drop(web_extension_discard("s1"));
        drop(web_extension_set_enabled("e", false));
        drop(web_extension_remove("e"));
        drop(web_extension_store_page("https://x"));
        drop(browser_cookie_sources());
        drop(browser_cookies_import("c", true));
        assert_eq!(
            *calls.borrow(),
            [
                ("focusPane", json!({ "paneId": "7" })),
                ("copyText", json!({ "text": "hi" })),
                ("closeView", json!({ "id": "board" })),
                ("project", json!({})),
                ("tabState", json!({})),
                ("setTabState", json!({ "state": { "folded": [1] } })),
                ("openSettings", json!({})),
                ("emit", json!({ "name": "open-changes", "data": { "cwd": "/r" } })),
                ("sendToAgent", json!({ "paneId": "7", "text": "go" })),
                ("callTool", json!({ "name": "tasks__list", "arguments": { "cwd": "/r" } })),
                ("setSectionSort", json!({ "sectionId": "issues", "sort": "date" })),
                ("agentActivity", json!({ "sessionId": "ses_1" })),
                ("notify", json!({ "title": "T", "body": "B", "actions": [{ "label": "Open" }], "projectId": "/r" })),
                ("post", json!({ "program": "az", "url": "https://x", "host": null, "fields": { "a": 1 } })),
                ("claudeAccounts", json!({})),
                ("setClaudeAccount", json!({ "projectId": "/r", "name": null })),
                ("shareClaudeSetup", json!({ "name": "work", "on": true })),
                ("webData", json!({ "action": "clear" })),
                ("webExtensions", json!({ "action": "list" })),
                ("webExtensions", json!({ "action": "installed" })),
                ("webExtensions", json!({ "action": "stage", "from": "pick" })),
                ("webExtensions", json!({ "action": "stage", "from": "browser", "source": "chrome:Default" })),
                ("webExtensions", json!({ "action": "stage", "from": "store", "url": "https://x" })),
                ("webExtensions", json!({ "action": "add", "stage": "s1" })),
                ("webExtensions", json!({ "action": "discard", "stage": "s1" })),
                ("webExtensions", json!({ "action": "setEnabled", "id": "e", "enabled": false })),
                ("webExtensions", json!({ "action": "remove", "id": "e" })),
                ("webExtensions", json!({ "action": "storePage", "url": "https://x" })),
                ("browserCookies", json!({ "action": "sources" })),
                ("browserCookies", json!({ "action": "import", "source": "c", "keepData": true })),
            ]
            .map(|(method, params)| (method.to_owned(), params))
        );
    }

    #[test]
    fn a_pane_opens_with_only_what_was_asked() {
        let calls = record(&[]);
        drop(OpenPane::new().open());
        drop(OpenPane::new().command("claude", &["fix it"]).cwd("/r").background().open());
        drop(OpenPane::new().command("claude", &[]).placement("right").claude_account(None).open());
        drop(OpenPane::new().claude_account(Some("work")).open());
        let sent: Vec<Value> = calls.borrow().iter().map(|(_, params)| params.clone()).collect();
        assert_eq!(
            sent,
            [
                json!({ "command": null, "args": [], "cwd": null, "placement": "tab" }),
                json!({ "command": "claude", "args": ["fix it"], "cwd": "/r", "placement": "tab", "background": true }),
                json!({ "command": "claude", "args": [], "cwd": null, "placement": "right", "claudeAccount": null }),
                json!({ "command": null, "args": [], "cwd": null, "placement": "tab", "claudeAccount": "work" }),
            ]
        );
    }

    /// `setWorktreeSetup` tells absent, `null` and a value apart (`calls.rs`), so each setting is left out, null or set.
    #[test]
    fn worktrees_tell_leaving_a_setting_from_resetting_it() {
        let calls = record(&[]);
        drop(create_worktree(Some("/r"), "fix", Some("main")));
        drop(create_worktree(None, "fix", None));
        drop(worktree_setup(None));
        drop(set_worktree_setup(Some("/r"), Change::Keep, Change::Keep));
        drop(set_worktree_setup(Some("/r"), Change::Reset, Change::Reset));
        drop(set_worktree_setup(None, Change::Set(&[".env", "a/b"][..]), Change::Set("pnpm i")));
        let sent: Vec<(String, Value)> = calls.borrow().clone();
        assert_eq!(
            sent,
            [
                ("createWorktree", json!({ "projectId": "/r", "branch": "fix", "base": "main" })),
                ("createWorktree", json!({ "projectId": null, "branch": "fix", "base": null })),
                ("worktreeSetup", json!({ "projectId": null })),
                ("setWorktreeSetup", json!({ "projectId": "/r" })),
                ("setWorktreeSetup", json!({ "projectId": "/r", "include": null, "setup": null })),
                ("setWorktreeSetup", json!({ "projectId": null, "include": [".env", "a/b"], "setup": "pnpm i" })),
            ]
            .map(|(method, params)| (method.to_owned(), params))
        );
    }

    #[test]
    fn the_messages_reach_their_listeners_with_the_fields_the_app_sends() {
        testing::answer(|_, _| Ok(Value::Null));
        let heard = Rc::new(RefCell::new(Vec::<String>::new()));
        let note = |heard: &Rc<RefCell<Vec<String>>>| {
            let heard = heard.clone();
            move |text: String| heard.borrow_mut().push(text)
        };
        let seen = note(&heard);
        on_project(move |project| seen(format!("project {project}")));
        let seen = note(&heard);
        on_event(move |from, name, data| seen(format!("event {from} {name} {data}")));
        let seen = note(&heard);
        on_section_sort(move |section, sort| seen(format!("sort {section} {sort}")));
        let seen = note(&heard);
        on_agent_activity(move |session| seen(format!("activity {session}")));
        super::super::deliver("project", 0, r#"{"id":"/r"}"#.into());
        super::super::deliver("pluginEvent", 0, r#"{"from":"changes","name":"open-changes","data":{"a":1}}"#.into());
        super::super::deliver("sectionSort", 0, r#"{"sectionId":"issues","sort":"date"}"#.into());
        super::super::deliver("agentActivity", 0, r#"{"sessionId":"ses_1"}"#.into());
        run_ready();
        assert_eq!(*heard.borrow(), [r#"project {"id":"/r"}"#, r#"event changes open-changes {"a":1}"#, "sort issues date", "activity ses_1"]);
    }
}
