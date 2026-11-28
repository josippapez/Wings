//! A browser page in a Wings tab: browser tabs with the agents' tab groups, and for the shown one back, forward and reload, an
//! address bar with suggestions from history, and the web page under them, which Wings draws with WebKit. Everything the page
//! keeps (history, settings) is in storage, so each page reads it itself.

use crate::{
    address::resolve,
    agent_page,
    downloads::{self, Download, Downloads},
    extension::{Staged, dialog_for},
    history::{self, Entry},
    settings,
    start_page::{self, RECENT},
    tabs::{Tab, Tabs},
};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use wings_plugin::{
    host::{self, Stage},
    task::spawn,
    ui::{
        Choice, Combobox, ExtensionButton, Node, WebDownload, WebPopup, WebState, actions as dialog_actions, badge, bar, button, combobox_submit, dialog, icon, icon_button, icons, image, menu,
        meter, muted, notice, page_with, row, section_label, slot, small, stack, tab as strip_tab, tab_group, text, toast, web_download, web_extension_action, web_view,
    },
};

const SUGGESTIONS: usize = 6;
/// `set_tab_title` takes one line of at most 80 characters.
const TITLE_MAX: usize = 80;
/// The width of the start page's column, and of the toolbar over it.
const START_COLUMN: u32 = 720;

#[derive(Default)]
pub(crate) struct Page {
    pub tabs: Tabs,
    /// What the start page draws from: the terminals and the ports they listen on, the project, and the pages visited.
    panes: Vec<Value>,
    ports: Value,
    project: Value,
    visited: Vec<Entry>,
    busy: bool,
    problem: Option<String>,
    /// What clearing a site did, shown over the foot of the page: its tone and words.
    cleared: Option<(&'static str, String)>,
    /// What the tabs downloaded, and the tokens of the commands sent to a download (show it, open it, stop it), whose
    /// refusal shows in the red line.
    pub downloads: Downloads,
    download_tokens: Vec<u64>,
    /// A window the shown tab's page opened by itself, which wasn't opened: what the toast says of it.
    blocked: Option<String>,
    /// The title last given the Wings tab, so a repeat does nothing.
    titled: String,
    /// What was last kept with the Wings tab, and whether the saved tabs have been read yet (before that nothing is kept, so
    /// they aren't written over).
    saved: Option<Value>,
    restored: bool,
}

/// What a change of a tab's view state asks the page to do.
#[derive(Debug, Default, PartialEq)]
struct Effects {
    /// A new title for the Wings tab: none for the view's own.
    title: Option<Option<String>>,
    /// A finished page for history.
    record: Option<(String, String)>,
    /// A new address to ask whether it is a store page.
    check_store: Option<String>,
}

impl Page {
    /// A page whose first tab is at `url` (blank for none), in `group` when an agent's.
    fn fresh(url: Option<&str>, group: Option<(&str, &str)>) -> Self {
        Self { tabs: Tabs::new(url, group), ..Self::default() }
    }

    pub fn active(&self) -> &Tab {
        self.tabs.active_tab()
    }

    pub fn active_id(&self) -> String {
        self.tabs.active.clone()
    }

    /// Sends the shown tab's view a command.
    pub fn send_active(&mut self, action: &str, url: Option<&str>) {
        let id = self.active_id();
        self.tabs.send(&id, action, url);
    }

    fn apply(&mut self, id: &str, new: WebState) -> Effects {
        let mut effects = Effects::default();
        let shown = self.tabs.active == id;
        let Some(tab) = self.tabs.get_mut(id) else { return effects };
        tab.updates += 1;
        let moved = tab.state.url != new.url;
        if moved {
            tab.typed = None;
            tab.store = false;
            tab.added = None;
            tab.blocked = None;
            effects.check_store = (new.url.starts_with("http://") || new.url.starts_with("https://")).then(|| new.url.clone());
        }
        let finished = !new.loading && new.error.is_none() && !new.url.is_empty();
        if finished && tab.recorded != (new.url.clone(), new.title.clone()) {
            tab.recorded = (new.url.clone(), new.title.clone());
            effects.record = Some(tab.recorded.clone());
        }
        tab.state = new;
        if shown {
            effects.title = self.title_effect();
        }
        effects
    }

    /// The Wings tab is named after the shown tab: a new title when it differs from the last, none for the view's own.
    fn title_effect(&mut self) -> Option<Option<String>> {
        let wanted: String = self.active().state.title.chars().take(TITLE_MAX).collect();
        (wanted != self.titled).then(|| {
            self.titled.clone_from(&wanted);
            (!wanted.is_empty()).then_some(wanted)
        })
    }

    /// What is to be kept with the Wings tab now: none when it is what was kept, or the saved tabs haven't been read yet.
    fn to_save(&mut self) -> Option<Value> {
        if !self.restored {
            return None;
        }
        let now = self.tabs.snapshot();
        (self.saved.as_ref() != Some(&now)).then(|| {
            self.saved = Some(now.clone());
            now
        })
    }

    /// Opens what the page was opened with (`url` and an agent's `group`) as a tab, unless it did so already: a page that is
    /// opened again at launch gets the same arguments.
    fn open_args(&mut self, args: &OpenArgs) {
        let sig = args.signature();
        if self.tabs.args_seen == sig {
            return;
        }
        self.tabs.args_seen = sig;
        if args.url.is_some() || args.group.is_some() {
            let group = args.group.as_ref().map(|(key, name)| (key.as_str(), name.as_str()));
            let id = self.tabs.open(group, args.url.as_deref());
            if group.is_none() {
                self.tabs.activate(&id);
            }
        }
    }
}

/// What a page is opened with: an address, and the group of the agent it is opened for.
#[derive(Default)]
struct OpenArgs {
    url: Option<String>,
    group: Option<(String, String)>,
}

impl OpenArgs {
    fn from(args: Option<&Value>) -> Self {
        let engine = settings::engine().get();
        let Some(args) = args else { return Self::default() };
        let key = args["group"]["key"].as_str().filter(|key| !key.is_empty());
        Self {
            url: args["url"].as_str().and_then(|url| resolve(url, engine)),
            group: key.map(|key| (key.to_owned(), args["group"]["name"].as_str().unwrap_or("Agent").to_owned())),
        }
    }

    fn signature(&self) -> String {
        format!("{}|{}", self.group.as_ref().map(|(key, _)| key.as_str()).unwrap_or_default(), self.url.as_deref().unwrap_or_default())
    }
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::new(Page::default());
}

pub(crate) fn with<R>(change: impl FnOnce(&mut Page) -> R) -> R {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

/// Shows `error` in the page's red line, as a failed store add does.
pub(crate) fn problem(error: String) {
    with(|page| page.problem = Some(error));
}

/// How often agents' tabs are checked for being left unused.
const IDLE_CHECK_MS: u32 = 60_000;

thread_local! {
    /// Whether the idle check's timer runs: it does only while an agent has a tab here.
    static WATCHING: Cell<bool> = const { Cell::new(false) };
}

/// Closes the agents' tabs nobody has used for as long as `settings::idle_minutes` says, never the tab shown. Gives how many went.
pub(crate) fn close_idle() -> usize {
    close_idle_after(settings::idle_minutes())
}

/// `close_idle` for a limit in minutes, 0 for never.
fn close_idle_after(minutes: u32) -> usize {
    // Never still counts the shown tab, so a later choice of a limit starts from now and not from a long time ago.
    let limit = if minutes == 0 { f64::INFINITY } else { f64::from(minutes) * 60_000.0 };
    let idle = with(|page| page.tabs.idle(host::now(), limit));
    for id in &idle {
        close_tab(id);
    }
    idle.len()
}

/// Starts the once-a-minute idle check when an agent has a tab and it isn't running; it stops itself when none is left.
fn watch_idle() {
    if WATCHING.with(Cell::get) || !with(|page| page.tabs.has_agent_tabs()) {
        return;
    }
    WATCHING.with(|watching| watching.set(true));
    spawn(async {
        loop {
            host::sleep(IDLE_CHECK_MS).await;
            close_idle();
            if !with(|page| page.tabs.has_agent_tabs()) {
                WATCHING.with(|watching| watching.set(false));
                return;
            }
        }
    });
}

/// The tabs changed: tell main what groups the page has, name the Wings tab after the tab shown, and keep the tabs.
pub(crate) fn tabs_changed() {
    watch_idle();
    agent_page::announce(false);
    if let Some(title) = with(Page::title_effect) {
        drop(host::set_tab_title(title.as_deref()));
    }
    if let Some(saved) = with(Page::to_save) {
        drop(host::set_tab_state(&saved));
    }
}

/// Closes a tab, and tells whoever waits for what it was asked that it won't answer.
pub(crate) fn close_tab(id: &str) {
    let tokens = with(|page| page.tabs.close(id));
    agent_page::closed(&tokens);
    tabs_changed();
}


fn select_tab(id: &str) {
    with(|page| {
        page.blocked = None;
        page.tabs.activate(id);
        page.tabs.touch(id, host::now());
    });
    tabs_changed();
    look_again();
}

fn new_tab() {
    with(|page| page.tabs.open(None, None));
    tabs_changed();
    look_again();
}

fn toggle_group(key: &str) {
    with(|page| {
        if let Some(group) = page.tabs.groups.iter_mut().find(|group| group.key == key) {
            group.collapsed = !group.collapsed;
        }
    });
    tabs_changed();
}

/// The part of a web page's address that names its site, as the person reads it (`localhost:4300`): none for the start
/// page and anything that isn't an `http` or `https` page, which keep nothing to clear.
fn site_name(url: &str) -> Option<String> {
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let name = authority.rsplit_once('@').map_or(authority, |(_, after)| after);
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}

/// Removes the cookies and site data of the site of `url`, then loads the tab's page again, which shows it signed out. The host
/// only does it right after a click, so this runs in the click of the dialog's button.
fn clear_site(tab: String, url: String) {
    let name = site_name(&url).unwrap_or_default();
    with(|page| {
        if let Some(tab) = page.tabs.get_mut(&tab) {
            tab.clearing_site = None;
        }
        page.cleared = None;
    });
    spawn(async move {
        let result = host::web_data_clear_site(&url).await;
        with(|page| match result {
            Ok(_) => {
                page.tabs.send(&tab, "reload", None);
                page.cleared = Some(("success", format!("Cleared the data of {name}.")));
            }
            Err(error) => page.cleared = Some(("danger", error)),
        });
    });
}

fn site_menu(tab: &Tab) -> Option<Node> {
    let name = site_name(&tab.state.url)?;
    let (asked, id) = (tab.state.url.clone(), tab.id.clone());
    let trigger = Node::new("Button").prop("variant", "ghost").prop("size", "icon-sm").prop("label", "More").prop("title", "More").child(wings_plugin::ui::icon(24, 2.0, &icons::ELLIPSIS).prop("size", 16));
    let item = Node::new("Button").key("clear-site").on("onClick", move |_| with(|page| {
        if let Some(tab) = page.tabs.get_mut(&id) {
            tab.clearing_site = Some(asked.clone());
        }
    })).text(format!("Clear data for {name}"));
    Some(menu(trigger, [item], true))
}

fn clear_site_dialog(tab: &Tab, url: &str) -> Node {
    let name = site_name(url).unwrap_or_default();
    let (go, id, close_id) = (url.to_owned(), tab.id.clone(), tab.id.clone());
    let cancel = move || with(|page| {
        if let Some(tab) = page.tabs.get_mut(&close_id) {
            tab.clearing_site = None;
        }
    });
    dialog(&format!("Clear data for {name}?"), 420, cancel.clone())
        .child(muted("This signs you out of it and removes what it stored. Other sites keep theirs."))
        .child(dialog_actions(Some(button("Cancel", "ghost", false, cancel)), [button("Clear", "destructive", false, move || clear_site(id.clone(), go.clone()))]))
}

fn command(action: &'static str) {
    with(|page| page.send_active(action, None));
}

/// Loads what is typed, or reloads the page when nothing was changed.
fn go() {
    let engine = settings::engine().get();
    with(|page| {
        let tab = page.tabs.active_mut();
        let typed = tab.typed.take().unwrap_or_else(|| tab.state.url.clone());
        if let Some(url) = resolve(&typed, engine) {
            page.send_active("load", Some(&url));
        }
    });
}

fn typed(text: String) {
    with(|page| {
        let tab = page.tabs.active_mut();
        if tab.typed.is_none() {
            tab.history = history::load();
        }
        tab.typed = Some(text);
    });
}

fn state(id: &str, new: WebState) {
    let effects = with(|page| page.apply(id, new));
    agent_page::changed();
    if let Some(title) = effects.title {
        drop(host::set_tab_title(title.as_deref()));
    }
    if let Some(saved) = with(Page::to_save) {
        drop(host::set_tab_state(&saved));
    }
    if let Some((url, title)) = effects.record {
        drop(history::add(&url, &title, host::now()));
    }
    if let Some(url) = effects.check_store {
        let id = id.to_owned();
        spawn(async move {
            let found = host::web_extension_store_page(&url).await.is_ok_and(|answer| !answer.is_null());
            with(|page| {
                if let Some(tab) = page.tabs.get_mut(&id) {
                    tab.store = found && tab.state.url == url;
                }
            });
        });
    }
}

/// Stages the extension of the store page the shown tab has. Called from the click, since the host only starts one right after.
fn stage_store() {
    let Some((id, url)) = with(|page| (!page.busy).then(|| (page.active_id(), page.active().state.url.clone()))) else {
        return;
    };
    with(|page| {
        page.busy = true;
        page.problem = None;
    });
    spawn(async move {
        let answer = host::web_extension_stage(Stage::Store(&url)).await;
        with(|page| {
            page.busy = false;
            match answer {
                Ok(staged) => {
                    if let Some(tab) = page.tabs.get_mut(&id) {
                        tab.staged = Staged::from_json(&staged);
                    }
                }
                Err(error) => page.problem = Some(error),
            }
        });
    });
}

fn add() {
    let Some((id, stage)) = with(|page| if page.busy { None } else { page.active().staged.as_ref().map(|staged| (page.active_id(), staged.stage.clone())) }) else {
        return;
    };
    with(|page| {
        page.busy = true;
        page.problem = None;
    });
    spawn(async move {
        let answer = host::web_extension_add(&stage).await;
        with(|page| {
            page.busy = false;
            match answer {
                Ok(_) => {
                    if let Some(tab) = page.tabs.get_mut(&id) {
                        tab.added = tab.staged.take().map(|staged| format!("Added {}. It is on for the pages you open now.", staged.name));
                    }
                }
                Err(error) => page.problem = Some(error),
            }
        });
    });
}

fn cancel() {
    if let Some(staged) = with(|page| {
        page.problem = None;
        page.tabs.active_mut().staged.take()
    }) {
        drop(host::web_extension_discard(&staged.stage));
    }
}

/// Clicks an extension's button for the shown tab: its popup opens under the button. From the click, which Wings needs.
fn open_extension(id: &str) {
    with(|page| {
        page.problem = None;
        let tab = page.active_id();
        let token = page.tabs.run(&tab, |token| web_extension_action(id, token));
        page.tabs.active_mut().opening = token;
    });
}

/// What a tab's command came to: a refused extension button, or a download that couldn't be shown or opened, says why in
/// the red line, and the rest are the agent's.
fn result_event(tab: &str, event: &Value) {
    let token = event["token"].as_u64();
    let download = with(|page| {
        let at = page.download_tokens.iter().position(|sent| Some(*sent) == token)?;
        page.download_tokens.remove(at);
        if let Some(error) = event["error"].as_str() {
            page.problem = Some(error.to_owned());
        }
        Some(())
    });
    if download.is_some() {
        return;
    }
    let refused = with(|page| {
        let mine = page.tabs.get(tab).is_some_and(|tab| tab.opening.is_some() && event["token"].as_u64() == tab.opening);
        let error = event["error"].as_str().filter(|_| mine).map(str::to_owned);
        if error.is_some() {
            page.problem = error;
        }
        mine
    });
    if !refused {
        agent_page::result(event);
    }
}

/// The page of `opener` opened a window: a tab of its own beside it, or for one it opened by itself, which wasn't opened,
/// a toast when the person is looking at that page.
fn popup_opened(opener: &str, popup: WebPopup) {
    if popup.blocked {
        with(|page| {
            let site = page.tabs.get(opener).and_then(|tab| site_name(&tab.state.url));
            if let Some(tab) = page.tabs.get_mut(opener) {
                tab.blocked = Some(popup.url.clone());
            }
            if page.tabs.active == opener {
                let from = site.map_or_else(String::new, |site| format!(" from {site}"));
                page.blocked = Some(format!("Blocked a pop-up{from} that opened without a click. Click its link or button to open it."));
            }
        });
        agent_page::changed();
        return;
    }
    if with(|page| page.tabs.open_from(opener, &popup.url, popup.popup, popup.background)).is_some() {
        tabs_changed();
    }
}

/// A view of `tab` told how a download goes: a new one the person started opens the list under the page.
fn downloaded(tab: &str, event: WebDownload) {
    with(|page| {
        let group = page.tabs.get(tab).and_then(|tab| tab.group.clone());
        if page.downloads.heard(tab, group.as_deref(), event) && group.is_none() {
            page.downloads.open = true;
        }
    });
}

/// Shows a download in Finder, opens it or stops it, by a command to its tab's view or, with that tab gone, another's:
/// Wings acts on it whichever view carries the command.
fn act_on_download(tab: &str, id: u64, action: &'static str) {
    with(|page| {
        let has_view = |tab: &Tab| !tab.blank;
        let via = page.tabs.get(tab).filter(|tab| has_view(tab)).or_else(|| page.tabs.list.iter().find(|tab| has_view(tab))).map(|tab| tab.id.clone());
        let Some(via) = via else {
            page.problem = Some("Open a page in a tab first: the download is acted on through it.".into());
            return;
        };
        page.problem = None;
        if let Some(token) = page.tabs.run(&via, |token| web_download(action, id, token)) {
            page.download_tokens.push(token);
        }
    });
}

/// The toolbar's button that shows and hides the downloads, with how many are under way: none before the page downloads.
fn downloads_button(page: &Page) -> Option<Node> {
    if page.downloads.list.is_empty() {
        return None;
    }
    let running = page.downloads.running();
    let label = match running {
        0 => "Downloads".to_owned(),
        n => format!("Downloads, {n} under way"),
    };
    let glyph = icon(24, 2.0, &icons::DOWNLOAD).prop("size", 16);
    let button = Node::new("Button")
        .prop("variant", "ghost")
        .prop("label", label.as_str())
        .prop("title", if page.downloads.open { "Hide downloads" } else { "Show downloads" })
        .on("onClick", |_| with(|page| page.downloads.open = !page.downloads.open))
        .key("downloads");
    Some(match running {
        0 => button.prop("size", "icon-sm").child(glyph),
        n => button.prop("size", "sm").child(row().prop("gap", "s").prop("align", "center").child(glyph).child(badge(n.to_string(), Some("info")))),
    })
}

/// How many downloads the list under the page shows, newest first.
const DOWNLOADS_SHOWN: usize = 4;

/// The list of downloads under the page, while it is open: each with its name, how far it is, and what can be done to it.
fn downloads_list(page: &Page) -> Option<Node> {
    if !page.downloads.open || page.downloads.list.is_empty() {
        return None;
    }
    let head = row()
        .prop("gap", "s")
        .prop("align", "center")
        .child(stack().prop("grow", true).child(section_label(format!("Downloads · {}", page.downloads.list.len()))))
        .child(button("Clear", "ghost", false, || with(|page| page.downloads.clear())).size("sm").tooltip("Take the finished ones off the list"))
        .child(icon_button("Hide downloads", &icons::CLOSE, || with(|page| page.downloads.open = false)));
    let more = page.downloads.list.len().saturating_sub(DOWNLOADS_SHOWN);
    let list = stack()
        .prop("gap", "s")
        .child(head)
        .children(page.downloads.list.iter().take(DOWNLOADS_SHOWN).map(download_row))
        .maybe((more > 0).then(|| muted(format!("{more} more, in ~/Downloads")).prop("size", "small")));
    Some(bar("top", [list]).key("downloads"))
}

fn download_row(download: &Download) -> Node {
    let event = &download.event;
    let (id, tab) = (event.id, download.tab.clone());
    let name = if event.name.is_empty() { event.url.clone() } else { event.name.clone() };
    let tone = if matches!(event.state.as_str(), "failed" | "blocked") { "danger" } else { "muted" };
    let words = stack()
        .prop("grow", true)
        .prop("gap", "xs")
        .child(text(name.as_str()).prop("weight", "medium").prop("truncate", true).prop("title", if event.path.is_empty() { event.url.as_str() } else { event.path.as_str() }))
        .child(small(downloads::status(event)).prop("tone", tone));
    let progress = match (event.state.as_str(), event.total) {
        ("progress", Some(total)) if total > 0 => Some(stack().prop("width", 120).child(meter(&format!("{name} downloading"), (event.received * 100 / total) as f64, Some("info")))),
        _ => None,
    };
    let act = move |action: &'static str| {
        let tab = tab.clone();
        move || act_on_download(&tab, id, action)
    };
    let buttons: Vec<Node> = match event.state.as_str() {
        "progress" => vec![icon_button("Stop downloading", &icons::CLOSE, act("cancel"))],
        "done" => vec![button("Open", "ghost", false, act("open")).size("sm").tooltip("Open it in its app"), icon_button("Show in Finder", &icons::FOLDER_OPEN, act("reveal"))],
        _ => Vec::new(),
    };
    row().prop("gap", "m").prop("align", "center").key(format!("download-{id}")).child(words).maybe(progress).children(buttons)
}

/// An extension's button in the toolbar: its icon, with its badge beside it as a count is on a header button, and its
/// label as the tooltip.
fn extension_button(extension: &ExtensionButton) -> Node {
    let label = if extension.label.is_empty() { &extension.name } else { &extension.label };
    let glyph = match &extension.icon {
        Some(icon) => image(icon, &extension.name, 16, 16).prop("radius", 0),
        // An extension with no picture at all: the first letter of its name.
        None => text(extension.name.chars().next().map(|first| first.to_uppercase().to_string()).unwrap_or_default()).prop("weight", "medium"),
    };
    let id = extension.id.clone();
    let button = Node::new("Button")
        .prop("variant", "ghost")
        .prop("label", label.as_str())
        .prop("title", label.as_str())
        .on("onClick", move |_| open_extension(&id))
        .disabled(!extension.enabled)
        .key(format!("extension-{}", extension.id));
    match extension.badge.as_str() {
        "" => button.prop("size", "icon-sm").child(glyph),
        said => button.prop("size", "sm").child(row().prop("gap", "s").prop("align", "center").child(glyph).child(badge(said, Some("info")))),
    }
}

fn address(tab: &Tab) -> Node {
    let options = tab
        .typed
        .as_deref()
        .map(|typed| {
            history::suggest(&tab.history, typed, SUGGESTIONS)
                .into_iter()
                .map(|entry| match entry.title.is_empty() {
                    true => Choice { value: entry.url.clone(), label: entry.url.clone(), detail: None },
                    false => Choice { value: entry.url.clone(), label: entry.title.clone(), detail: Some(entry.url.clone()) },
                })
                .collect()
        })
        .unwrap_or_default();
    let field = Combobox {
        label: "Address",
        placeholder: "Search or enter an address",
        value: tab.typed.clone().unwrap_or_else(|| tab.state.url.clone()),
        query: None,
        options,
        // Only while something is typed: after Enter, or with the page's own address in the field, there is nothing to submit.
        empty: if tab.typed.is_some() { "Press Enter to search or go" } else { "" },
        auto_focus: false,
    };
    combobox_submit(
        field,
        |text| {
            if let Some(text) = text {
                typed(text);
            }
        },
        |url| with(|page| {
            page.tabs.active_mut().typed = None;
            page.send_active("load", Some(url));
        }),
        |key| match key {
            "Enter" => go(),
            "Escape" => with(|page| page.tabs.active_mut().typed = None),
            _ => {}
        },
    )
}

/// What the start page shows now, and a look at the ports again: a server started since the tab opened comes in.
fn look_again() {
    if !with(|page| page.active().blank) {
        return;
    }
    spawn(async {
        let ports = host::ports().await.unwrap_or(Value::Null);
        let project = host::project().await.unwrap_or(Value::Null);
        with(|page| {
            page.ports = ports;
            page.project = project;
            page.visited = history::load();
        });
    });
}

fn open_from_start(url: &str) {
    with(|page| page.send_active("load", Some(url)));
}

/// The strip above the toolbar, on one line: each agent's group, its label in a tone of its own and then its tabs, which a
/// click on the label folds, then the person's own tabs, then New tab.
fn tab_strip(page: &Page) -> Node {
    const TONES: [&str; 4] = ["info", "accent", "success", "warning"];
    let tab = |tab: &Tab| strip_tab(&tab.id, &tab.label(), &tab.state.url, tab.id == page.tabs.active, tab.state.loading, tab.group.as_deref());
    let mut children = Vec::new();
    for (index, group) in page.tabs.groups.iter().enumerate() {
        let tabs = page.tabs.of(&group.key);
        let name = if group.name.is_empty() { "Agent" } else { group.name.as_str() };
        children.push(tab_group(&group.key, name, TONES[index % TONES.len()], group.collapsed, tabs.len()));
        children.extend(tabs.into_iter().map(tab));
    }
    children.extend(page.tabs.of("").into_iter().map(tab));
    wings_plugin::ui::tab_strip("Tabs", children, select_tab, close_tab, new_tab, toggle_group)
}

/// A tab's web view: every tab keeps its own, the shown one drawn and the others kept made and hidden, so a background tab goes
/// on loading and an agent's tab, kept live too, can be read and shot. The key is the view's name in the page.
fn web(tab: &Tab, shown: bool, keep: bool) -> Node {
    let (state_id, result_id, popup_id, close_id, download_id) = (tab.id.clone(), tab.id.clone(), tab.id.clone(), tab.id.clone(), tab.id.clone());
    let mut web = web_view(&format!("tab-{}", tab.id))
        .key(format!("web-{}", tab.id))
        .prop("fill", true)
        .prop("url", tab.start.as_str())
        .prop("keepData", keep)
        .on("onState", move |event| state(&state_id, WebState::from_event(event)))
        .on("onResult", move |event| result_event(&result_id, event))
        .on("onPopup", move |event| popup_opened(&popup_id, WebPopup::from_event(event)))
        .on("onClose", move |_| close_tab(&close_id))
        .on("onDownload", move |event| downloaded(&download_id, WebDownload::from_event(event)));
    if let Some(popup) = tab.popup {
        web = web.prop("popup", popup);
    }
    if !shown {
        web = web.prop("hidden", true);
    }
    if tab.group.is_some() {
        web = web.prop("live", true);
    }
    if !tab.commands.is_empty() {
        web = web.prop("command", Value::Array(tab.commands.clone()));
    }
    web
}

fn view() -> Vec<Node> {
    let keep = settings::keep_data().get();
    PAGE.with(|page| {
        let page = page.borrow();
        let tab = page.active();
        let state = &tab.state;
        // In the toolbar's children, which take the width the title would leave, so the address field grows with the tab.
        let nav = row()
            .prop("grow", true)
            .prop("gap", "xs")
            .prop("align", "center")
            .child(icon_button("Back", &icons::CHEVRON_LEFT, || command("back")).disabled(!state.can_go_back))
            .child(icon_button("Forward", &icons::CHEVRON_RIGHT, || command("forward")).disabled(!state.can_go_forward))
            .child(if state.loading { icon_button("Stop", &icons::CLOSE, || command("stop")) } else { icon_button("Reload", &icons::REFRESH_CW, || command("reload")) })
            .child(stack().prop("grow", true).child(address(tab)));
        let actions = row()
            .prop("gap", "s")
            .prop("align", "center")
            .maybe(tab.store.then(|| button("Add to Wings", "secondary", page.busy, stage_store).size("sm")))
            .children(state.extensions.iter().map(extension_button))
            .maybe(downloads_button(&page))
            .child(if agent_page::popped() {
                icon_button("Put back in a tab", &icons::MINIMIZE_2, agent_page::pop_button_action)
            } else {
                icon_button("Pop out into a window", &icons::EXTERNAL_LINK, agent_page::pop_button_action)
            })
            .maybe(site_menu(tab))
            .child(icon_button("Browser settings", &icons::SETTINGS, || drop(host::open_settings())));
        let problem = if tab.staged.is_none() { page.problem.as_deref() } else { None };
        let top = [
            Some(tab_strip(&page)),
            Some(Node::new("Toolbar").child(slot("children", nav)).child(slot("actions", actions))),
            state.error.as_deref().map(|error| notice("danger", error).key("error")),
            problem.map(|problem| notice("danger", problem).key("problem")),
            tab.added.as_deref().map(|added| notice("success", added).key("added")),
            agent_page::handoff_banner(),
        ];
        let cleared = page.cleared.as_ref().map(|(tone, words)| toast(tone, words.as_str(), || with(|page| page.cleared = None)).key("cleared"));
        let blocked = page.blocked.as_ref().map(|words| toast("warn", words.as_str(), || with(|page| page.blocked = None)).key("blocked"));
        let toasts = cleared.into_iter().chain(blocked);
        // Every tab with a page keeps its view, whichever is shown.
        let webs = page.tabs.list.iter().filter(|other| !other.blank).map(|other| web(other, other.id == tab.id, keep));
        let list = downloads_list(&page);
        if tab.blank {
            let servers = start_page::servers(&page.ports, &page.panes, &page.project);
            let recent = start_page::recent(&page.visited, &servers, RECENT);
            let body = start_page::view(&servers, &recent, open_from_start);
            return vec![page_with(top.into_iter().flatten(), std::iter::once(body).chain(webs).chain(list)).prop("column", START_COLUMN).toasts(toasts)];
        }
        let dialog = tab.staged.as_ref().map(|staged| dialog_for(staged, page.busy, page.problem.as_deref(), cancel, add));
        let ask = tab.clearing_site.as_deref().map(|url| clear_site_dialog(tab, url));
        vec![page_with(top.into_iter().flatten(), webs.chain(list).chain(dialog).chain(ask)).toasts(toasts)]
    })
}

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    let args = host::view_args();
    let opened = OpenArgs::from(args.as_ref());
    let group = opened.group.as_ref().map(|(key, name)| (key.as_str(), name.as_str()));
    with(|page| {
        *page = Page::fresh(opened.url.as_deref(), group);
        page.tabs.args_seen = opened.signature();
    });
    host::on_panes(|list| with(|page| page.panes = list.to_vec()));
    host::on_focus(|focused| {
        if focused {
            look_again();
        }
    });
    look_again();
    agent_page::start(args.as_ref());
    // The tabs and groups kept with the Wings tab win over the address it was opened with, which opens as one more tab unless
    // this is that tab being opened again.
    spawn(async move {
        let saved = host::tab_state().await.unwrap_or(Value::Null);
        with(|page| {
            if page.tabs.restore(&saved) {
                page.open_args(&opened);
            }
            page.restored = true;
        });
        tabs_changed();
    });
    wings_plugin::run_filled(view);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wings_plugin::ui::web_command;

    fn loaded(url: &str, title: &str) -> WebState {
        WebState { url: url.into(), title: title.into(), ..WebState::default() }
    }

    /// A page with one tab of the person's own at `url` (blank for none), which has loaded `url` when it has one.
    fn only(url: Option<&str>) {
        with(|page| *page = Page::fresh(url, None));
        if let Some(url) = url {
            with(|page| page.tabs.active_mut().state = loaded(url, ""));
        }
    }

    fn shown<R>(read: impl FnOnce(&Tab) -> R) -> R {
        with(|page| read(page.active()))
    }

    /// The hint under the address field is for text being typed: with the loaded page's address in the field, and after
    /// Enter has submitted what was typed, it isn't there.
    #[test]
    fn the_address_hint_shows_while_typing_and_not_once_the_page_is_loaded() {
        let hint = |tab: &Tab| wings_plugin::ui::tree_json(address(tab)).to_string().contains("Press Enter to search or go");
        let mut page = Page::default();
        page.apply("t1", loaded("https://a.com/", "A"));
        assert!(!hint(page.active()), "a loaded page's address is not a search");
        page.tabs.active_mut().typed = Some("zzz-no-history-match".into());
        assert!(hint(page.active()), "typing something nothing matches");
        page.tabs.active_mut().typed = None;
        assert!(!hint(page.active()), "after Enter or Escape");
    }

    fn drawn() -> String {
        wings_plugin::ui::export(view()).0.to_string()
    }

    #[test]
    fn a_web_page_has_a_clear_data_menu_item_that_asks_first_and_the_start_page_has_none() {
        only(Some("http://localhost:4300/app"));
        let now = drawn();
        assert!(now.contains("\"Menu\"") && now.contains("Clear data for localhost:4300"), "{now}");
        assert!(!now.contains("Clear data for localhost:4300?"), "it asks only once chosen: {now}");
        with(|page| page.tabs.active_mut().clearing_site = Some("http://localhost:4300/app".into()));
        let now = drawn();
        for shown in ["\"Dialog\"", "Clear data for localhost:4300?", "This signs you out of it and removes what it stored. Other sites keep theirs.", "\"Cancel\"", "\"destructive\""] {
            assert!(now.contains(shown), "{shown} is not in {now}");
        }
        only(None);
        let now = drawn();
        assert!(!now.contains("Clear data for") && !now.contains("\"Menu\""), "{now}");
    }

    #[test]
    fn confirming_clears_the_page_site_only_then_reloads_and_says_so_in_a_toast() {
        let asked = std::rc::Rc::new(RefCell::new(Vec::new()));
        let seen = asked.clone();
        wings_plugin::host::testing::answer(move |method, params| {
            seen.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(Value::Null)
        });
        only(Some("https://app.example.com/inbox?x=1"));
        clear_site("t1".into(), "https://app.example.com/inbox?x=1".into());
        wings_plugin::host::testing::settle();
        assert_eq!(*asked.borrow(), [("webData".to_owned(), serde_json::json!({ "action": "clearSite", "url": "https://app.example.com/inbox?x=1" }))]);
        assert!(shown(|tab| tab.commands.iter().any(|command| command["action"] == "reload")));
        let now = drawn();
        assert!(now.contains("\"Toast\"") && now.contains("Cleared the data of app.example.com.") && now.contains("success"), "{now}");
    }

    #[test]
    fn a_failed_clear_shows_the_error_in_a_toast_and_does_not_reload() {
        wings_plugin::host::testing::answer(|_, _| Err("WebKit didn't answer".into()));
        only(Some("https://a.com/"));
        clear_site("t1".into(), "https://a.com/".into());
        wings_plugin::host::testing::settle();
        assert!(shown(|tab| tab.commands.is_empty()));
        let now = drawn();
        assert!(now.contains("WebKit didn't answer") && now.contains("danger"), "{now}");
    }

    #[test]
    fn only_web_pages_have_a_site_named_as_the_address_bar_shows_it() {
        assert_eq!(site_name("http://Localhost:4300/a?b#c").as_deref(), Some("localhost:4300"));
        assert_eq!(site_name("https://me:pw@x.com/").as_deref(), Some("x.com"));
        for other in ["", "about:blank", "file:///a", "https:///a"] {
            assert_eq!(site_name(other), None, "{other}");
        }
    }

    #[test]
    fn a_new_tab_shows_the_start_page_until_an_address_is_sent_and_then_starts_the_view_there() {
        only(None);
        let drawn = wings_plugin::ui::export(view()).0.to_string();
        assert!(!drawn.contains("\"WebView\""), "{drawn}");
        with(|page| page.send_active("reload", None));
        assert!(shown(|tab| tab.blank && tab.commands.is_empty()), "nothing to reload yet");
        with(|page| page.send_active("load", Some("http://localhost:5173/")));
        let drawn = wings_plugin::ui::export(view()).0.to_string();
        assert!(drawn.contains("\"WebView\"") && drawn.contains("\"url\":\"http://localhost:5173/\""), "{drawn}");
        assert!(shown(|tab| tab.commands.is_empty()), "the view loads its start address itself");
    }

    #[test]
    fn each_command_gets_a_new_token_so_the_view_runs_it_again() {
        let mut page = Page::fresh(Some("https://a.com/"), None);
        page.send_active("reload", None);
        page.send_active("reload", None);
        page.send_active("load", Some("https://a.com"));
        assert_eq!(page.active().commands.last(), Some(&web_command("load", Some("https://a.com"), 3)));
        assert_eq!(page.active().commands.len(), 3, "the earlier two are still sent, so none is dropped before the view runs it");
        for _ in 0..20 {
            page.send_active("reload", None);
        }
        assert_eq!(page.active().commands.len(), crate::tabs::COMMANDS_KEPT);
    }

    #[test]
    fn an_agents_tab_nobody_uses_is_closed_after_the_limit_with_its_group_but_never_the_tab_shown_or_the_persons_own() {
        const HALF_HOUR: f64 = 30.0 * 60_000.0;
        let close_idle = || close_idle_after(30);
        busy();
        with(|page| page.tabs.activate("t2"));
        wings_plugin::host::testing::set_now(1000.0);
        assert_eq!(close_idle(), 0, "the first check starts the counts");
        // t2 is shown; t3 sits unused; t1 is the person's own.
        wings_plugin::host::testing::set_now(1000.0 + HALF_HOUR + 1.0);
        assert_eq!(close_idle(), 1);
        assert_eq!(with(|page| page.tabs.list.iter().map(|tab| tab.id.clone()).collect::<Vec<_>>()), ["t1", "t2", "t4"]);
        assert!(with(|page| page.tabs.group("ses-b").is_none() && page.tabs.group("ses-a").is_some()), "a group goes with its last tab");
        // The person looks away; the tab starts its count then, and is closed one limit later.
        with(|page| page.tabs.activate("t4"));
        wings_plugin::host::testing::set_now(2000.0 + HALF_HOUR);
        assert_eq!(close_idle(), 0);
        wings_plugin::host::testing::set_now(2000.0 + 2.0 * HALF_HOUR + 1.0);
        assert_eq!(close_idle(), 1);
        assert_eq!(with(|page| page.tabs.list.iter().map(|tab| tab.id.clone()).collect::<Vec<_>>()), ["t1", "t4"]);
    }

    #[test]
    fn never_closes_nothing_and_the_default_is_ten_minutes() {
        busy();
        with(|page| page.tabs.activate("t2"));
        wings_plugin::host::testing::set_now(1000.0);
        assert_eq!(close_idle_after(0), 0);
        wings_plugin::host::testing::set_now(1000.0 + 100.0 * 3_600_000.0);
        assert_eq!(close_idle_after(0), 0, "never closes anything, however long");
        assert_eq!(settings::idle_minutes(), 10);
        assert_eq!(close_idle(), 1, "the default limit closes the one nobody used");
    }

    #[test]
    fn the_idle_check_runs_once_a_minute_only_while_an_agent_has_a_tab() {
        only(Some("https://a.com/"));
        WATCHING.with(|watching| watching.set(false));
        watch_idle();
        assert!(!WATCHING.with(Cell::get), "no agent tab, no timer");
        let agent = with(|page| page.tabs.open(Some(("ses-a", "A")), Some("https://b.com/")));
        wings_plugin::host::testing::set_now(0.0);
        watch_idle();
        watch_idle();
        wings_plugin::host::testing::settle();
        assert!(WATCHING.with(Cell::get));
        wings_plugin::host::testing::set_now(40.0 * 60_000.0);
        wings_plugin::host::testing::fire_timers();
        wings_plugin::host::testing::settle();
        // The first check started its count: nothing closed yet. The next, a limit later, closes it and the timer ends with it.
        assert!(with(|page| page.tabs.get(&agent).is_some()));
        wings_plugin::host::testing::set_now(80.0 * 60_000.0);
        wings_plugin::host::testing::fire_timers();
        wings_plugin::host::testing::settle();
        assert!(with(|page| page.tabs.get(&agent).is_none()));
        assert!(!WATCHING.with(Cell::get));
    }

    #[test]
    fn a_page_is_recorded_once_when_it_has_finished_without_an_error() {
        let mut page = Page::default();
        let loading = WebState { loading: true, ..loaded("https://a.com/", "") };
        assert_eq!(page.apply("t1", loading.clone()).record, None);
        assert_eq!(page.apply("t1", loaded("https://a.com/", "A")).record, Some(("https://a.com/".into(), "A".into())));
        assert_eq!(page.apply("t1", WebState { can_go_back: true, ..loaded("https://a.com/", "A") }).record, None, "the same page again");
        let failed = WebState { error: Some("offline".into()), ..loaded("https://b.com/", "B") };
        assert_eq!(page.apply("t1", failed).record, None);
    }

    #[test]
    fn the_tab_title_follows_the_page_title_and_is_cut_to_80_characters() {
        let mut page = Page::default();
        assert_eq!(page.apply("t1", loaded("https://a.com/", "A")).title, Some(Some("A".into())));
        assert_eq!(page.apply("t1", loaded("https://a.com/", "A")).title, None);
        assert_eq!(page.apply("t1", loaded("https://a.com/", &"x".repeat(100))).title, Some(Some("x".repeat(80))));
        assert_eq!(page.apply("t1", loaded("https://a.com/", "")).title, Some(None), "back to the view's own title");
    }

    #[test]
    fn the_wings_tab_is_named_after_the_tab_shown_and_a_background_tab_does_not_rename_it() {
        let mut page = Page::fresh(Some("https://a.com/"), None);
        page.apply("t1", loaded("https://a.com/", "Mine"));
        let agent = page.tabs.open(Some(("ses-a", "Fix login bug")), Some("https://b.com/"));
        assert_eq!(page.apply(&agent, loaded("https://b.com/", "Agent page")).title, None, "the person is not looking at it");
        assert_eq!(page.title_effect(), None);
        page.tabs.activate(&agent);
        assert_eq!(page.title_effect(), Some(Some("Agent page".into())), "choosing the tab names the Wings tab after it");
        let blank = page.tabs.open(None, None);
        assert_eq!(page.tabs.active, blank);
        assert_eq!(page.title_effect(), Some(None), "a blank tab has the view's own title");
    }

    #[test]
    fn going_to_another_address_ends_an_edit_and_asks_about_the_store_for_web_pages_only() {
        let mut page = Page::default();
        page.tabs.active_mut().typed = Some("half".into());
        page.tabs.active_mut().store = true;
        let effects = page.apply("t1", loaded("https://chromewebstore.google.com/detail/x/y", ""));
        assert_eq!((page.active().typed.clone(), page.active().store, effects.check_store), (None, false, Some("https://chromewebstore.google.com/detail/x/y".into())));
        assert_eq!(page.apply("t1", loaded("about:blank", "")).check_store, None);
        assert_eq!(page.apply("t1", loaded("about:blank", "")).check_store, None);
    }

    #[test]
    fn typing_is_kept_until_the_address_changes() {
        let mut page = Page::default();
        page.tabs.active_mut().typed = Some("half".into());
        page.apply("t1", WebState { loading: true, ..loaded("", "") });
        assert_eq!(page.active().typed.as_deref(), Some("half"));
    }

    /// The first button in `tree` read out as `label`.
    fn button_named<'a>(tree: &'a Value, label: &str) -> Option<&'a Value> {
        if tree["type"] == "Button" && tree["props"]["label"] == label {
            return Some(tree);
        }
        let children = tree.as_array().or_else(|| tree["children"].as_array())?;
        children.iter().find_map(|child| button_named(child, label))
    }

    /// Every node of `kind` in `tree`.
    fn all<'a>(tree: &'a Value, kind: &str, out: &mut Vec<&'a Value>) {
        if tree["type"] == kind {
            out.push(tree);
        }
        for child in tree.as_array().or_else(|| tree["children"].as_array()).into_iter().flatten() {
            all(child, kind, out);
        }
    }

    fn views(tree: &Value) -> Vec<&Value> {
        let mut found = Vec::new();
        all(tree, "WebView", &mut found);
        found
    }

    #[test]
    fn the_toolbar_has_a_button_for_each_extension_with_its_icon_and_badge_and_a_click_names_it() {
        let extension = |id: &str, name: &str, icon: Option<&str>, badge: &str, enabled: bool| ExtensionButton {
            id: id.into(), name: name.into(), label: name.into(), icon: icon.map(str::to_owned), badge: badge.into(), enabled,
        };
        let state = WebState {
            extensions: vec![extension("e1", "Bitwarden", Some("ext-btn-1"), "3", true), extension("e2", "React Developer Tools", None, "", false)],
            ..loaded("https://github.com/", "GitHub")
        };
        only(Some("https://github.com/"));
        with(|page| page.tabs.active_mut().state = state);
        let (tree, handlers) = wings_plugin::ui::export(view());
        let bitwarden = button_named(&tree, "Bitwarden").expect("Bitwarden's button");
        assert_eq!((bitwarden["props"]["title"].as_str(), bitwarden["props"]["size"].as_str()), (Some("Bitwarden"), Some("sm")));
        let drawn = bitwarden.to_string();
        assert!(drawn.contains(r#""src":"ext-btn-1""#) && drawn.contains(r#""type":"Badge""#) && drawn.contains(r#""text":"3""#), "{drawn}");
        // No picture: the first letter of its name. Off: greyed.
        let react = button_named(&tree, "React Developer Tools").expect("React's button");
        assert_eq!((react["props"]["size"].as_str(), react["props"]["disabled"].as_bool()), (Some("icon-sm"), Some(true)));
        assert!(react.to_string().contains(r#""text":"R""#), "{react}");
        // A click sends the command that names it, which the view runs once.
        assert!(handlers.call(bitwarden["props"]["onClick"].as_str().unwrap(), &Value::Null));
        let sent = shown(|tab| (tab.commands.last().cloned(), tab.opening));
        assert_eq!(sent, (Some(web_extension_action("e1", 1)), Some(1)));
        // Wings refuses one that came without your click: the page says why.
        result_event("t1", &serde_json::json!({ "token": 1, "error": "only right after you click" }));
        assert_eq!(with(|page| page.problem.clone()), Some("only right after you click".into()));
        // A page with no extensions has no buttons for them.
        with(|page| page.tabs.active_mut().state.extensions.clear());
        assert!(button_named(&wings_plugin::ui::export(view()).0, "Bitwarden").is_none());
    }

    #[test]
    fn a_refused_extension_click_is_the_page_of_the_tab_that_sent_it() {
        only(Some("https://a.com/"));
        let other = with(|page| page.tabs.open(Some(("ses-a", "Agent")), Some("https://b.com/")));
        // The click came from the person's tab (t1); the same token from the agent's tab is the agent's, not a red line.
        with(|page| page.tabs.active_mut().opening = page.tabs.run("t1", |token| web_extension_action("e1", token)));
        result_event(&other, &serde_json::json!({ "token": 1, "error": "no" }));
        assert_eq!(with(|page| page.problem.clone()), None);
        result_event("t1", &serde_json::json!({ "token": 1, "error": "no" }));
        assert_eq!(with(|page| page.problem.clone()), Some("no".into()));
    }

    /// The handler `name` of the web view of tab `tab` in what is drawn now, and the handlers to call it with.
    fn view_handler(tab: &str, name: &str) -> (String, wings_plugin::ui::Handlers) {
        let (tree, handlers) = wings_plugin::ui::export(view());
        let found = views(&tree).into_iter().find(|view| view["props"]["name"] == format!("tab-{tab}")).unwrap_or_else(|| panic!("no view of {tab}: {tree}"));
        (found["props"][name].as_str().unwrap_or_else(|| panic!("no {name}")).to_owned(), handlers)
    }

    /// A window a page opens with your click is a tab beside it showing the view Wings made for it, which keeps its opener;
    /// its `window.close()` closes the tab.
    #[test]
    fn a_window_the_page_opens_is_a_tab_beside_it_on_the_view_wings_made_and_closing_itself_closes_it() {
        only(Some("https://app.example/"));
        with(|page| page.tabs.open(None, Some("https://other.example/")));
        with(|page| page.tabs.activate("t1"));
        let (on_popup, handlers) = view_handler("t1", "onPopup");
        assert!(handlers.call(&on_popup, &json!({ "url": "https://login.example/authorize", "background": false, "popup": 3 })));
        assert_eq!(with(|page| (page.tabs.order(), page.tabs.active.clone())), (vec!["t1".to_owned(), "t3".into(), "t2".into()], "t3".to_owned()));
        let tree = wings_plugin::ui::export(view()).0;
        let popup = views(&tree).into_iter().find(|view| view["props"]["name"] == "tab-t3").cloned().expect("the window's view");
        assert_eq!((popup["props"]["popup"].as_u64(), popup["props"]["url"].as_str()), (Some(3), Some("https://login.example/authorize")));
        assert!(popup["props"].get("hidden").is_none(), "it shows");
        let (on_close, handlers) = view_handler("t3", "onClose");
        assert!(handlers.call(&on_close, &Value::Null));
        assert_eq!(with(|page| (page.tabs.order(), page.tabs.active.clone())), (vec!["t1".to_owned(), "t2".into()], "t1".to_owned()), "it closed itself, back to its opener");
        // A Cmd-click opens a plain tab behind.
        let (on_popup, handlers) = view_handler("t1", "onPopup");
        assert!(handlers.call(&on_popup, &json!({ "url": "https://docs.example/", "background": true, "popup": null })));
        assert_eq!(with(|page| (page.tabs.active.clone(), page.tabs.get("t4").map(|tab| (tab.popup, tab.start.clone())))), ("t1".into(), Some((None, "https://docs.example/".into()))));
    }

    /// A window a page opens by itself isn't opened: the person sees a toast while they look at that page, and nothing else
    /// changes.
    #[test]
    fn a_window_opened_without_a_click_is_blocked_with_a_toast() {
        only(Some("https://app.example/"));
        let (on_popup, handlers) = view_handler("t1", "onPopup");
        assert!(handlers.call(&on_popup, &json!({ "url": "https://ads.example/", "blocked": true })));
        assert_eq!(with(|page| page.tabs.list.len()), 1);
        let now = drawn();
        assert!(now.contains("\"Toast\"") && now.contains("Blocked a pop-up from app.example that opened without a click."), "{now}");
        assert_eq!(with(|page| page.tabs.get("t1").unwrap().blocked.clone()), Some("https://ads.example/".into()));
        // The next page it goes to starts with none.
        state("t1", loaded("https://app.example/next", "Next"));
        assert_eq!(with(|page| page.tabs.get("t1").unwrap().blocked.clone()), None);
    }

    /// A download shows under the page with how far it is, and the toolbar has a button for the list; it is stopped, shown in
    /// Finder or opened by a command naming it, whose refusal shows in the red line.
    #[test]
    fn a_download_shows_under_the_page_and_is_stopped_shown_or_opened_by_its_id() {
        only(Some("https://app.example/"));
        let (on_download, handlers) = view_handler("t1", "onDownload");
        let event = |state: &str, received: u64| json!({ "id": 7, "url": "https://app.example/report.pdf", "name": "report.pdf", "path": "/Users/me/Downloads/report.pdf", "state": state, "received": received, "total": 2_600_000, "error": null });
        assert!(handlers.call(&on_download, &event("progress", 1_200_000)));
        let (tree, handlers) = wings_plugin::ui::export(view());
        let now = tree.to_string();
        for shown in ["report.pdf", "46% · 1.2 MB of 2.6 MB", "\"Meter\"", "Downloads · 1"] {
            assert!(now.contains(shown), "{shown} is not in {now}");
        }
        assert!(button_named(&tree, "Downloads, 1 under way").is_some(), "the toolbar's button counts it");
        let stop = button_named(&tree, "Stop downloading").expect("a stop button");
        assert!(handlers.call(stop["props"]["onClick"].as_str().unwrap(), &Value::Null));
        assert_eq!(shown(|tab| tab.commands.last().cloned()), Some(json!({ "action": "download", "do": "cancel", "id": 7, "token": 1 })));
        assert!(handlers.call(&on_download, &event("done", 2_600_000)));
        let (tree, handlers) = wings_plugin::ui::export(view());
        assert!(tree.to_string().contains("Done · 2.6 MB") && button_named(&tree, "Downloads").is_some());
        let reveal = button_named(&tree, "Show in Finder").expect("Show in Finder");
        assert!(handlers.call(reveal["props"]["onClick"].as_str().unwrap(), &Value::Null));
        assert_eq!(shown(|tab| tab.commands.last().cloned()), Some(json!({ "action": "download", "do": "reveal", "id": 7, "token": 2 })));
        result_event("t1", &json!({ "token": 2, "error": "The file isn't in Downloads any more" }));
        assert_eq!(with(|page| page.problem.clone()), Some("The file isn't in Downloads any more".into()));
        // Hidden, the list goes and the button stays.
        let hide = button_named(&tree, "Hide downloads").expect("a hide button");
        assert!(handlers.call(hide["props"]["onClick"].as_str().unwrap(), &Value::Null));
        let (tree, _) = wings_plugin::ui::export(view());
        assert!(!tree.to_string().contains("Done · 2.6 MB") && button_named(&tree, "Downloads").is_some());
    }

    /// An agent's tab downloading doesn't put the list in front of the person.
    #[test]
    fn an_agents_download_does_not_open_the_list() {
        only(Some("https://app.example/"));
        let agent = with(|page| page.tabs.open(Some(("ses-a", "A")), Some("https://a.example/")));
        let (on_download, handlers) = view_handler(&agent, "onDownload");
        assert!(handlers.call(&on_download, &json!({ "id": 1, "url": "https://a.example/x.zip", "name": "x.zip", "path": "/Users/me/Downloads/x.zip", "state": "progress", "received": 1, "total": null })));
        assert!(with(|page| page.downloads.list.len() == 1 && !page.downloads.open));
        assert_eq!(with(|page| page.downloads.of("ses-a").len()), 1);
    }

    #[test]
    fn the_page_is_a_toolbar_over_a_web_view_that_fills_and_offers_the_store_button_only_on_a_store_page() {
        only(Some("https://duckduckgo.com/"));
        let drawn = wings_plugin::ui::export(view()).0.to_string();
        assert!(drawn.contains("\"WebView\"") && drawn.contains("\"fill\":true") && drawn.contains("\"url\":\"https://duckduckgo.com/\""), "{drawn}");
        assert!(!drawn.contains("Add to Wings"));
        with(|page| page.tabs.active_mut().store = true);
        assert!(wings_plugin::ui::export(view()).0.to_string().contains("Add to Wings"));
    }

    /// The person's tab t1 at a.com, an agent's group "Fix login bug" with a tab at b.com, another agent's "Write docs" with one
    /// at c.com, and the person's blank tab t4 (shown).
    fn busy() {
        only(Some("https://a.com/"));
        with(|page| {
            page.tabs.open(Some(("ses-a", "Fix login bug")), Some("https://b.com/"));
            page.tabs.open(Some(("ses-b", "Write docs")), Some("https://c.com/"));
            page.tabs.open(None, None);
        });
    }

    #[test]
    fn every_tab_with_a_page_draws_its_own_view_hidden_unless_shown_and_live_for_an_agents_group() {
        busy();
        let tree = wings_plugin::ui::export(view()).0;
        let found = views(&tree);
        let names: Vec<_> = found.iter().map(|view| view["props"]["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["tab-t1", "tab-t2", "tab-t3"], "the blank tab t4 has no view: {tree}");
        let flags = |view: &Value| (view["props"]["hidden"].as_bool().unwrap_or(false), view["props"]["live"].as_bool().unwrap_or(false), view["props"]["fill"].as_bool());
        assert_eq!(found.iter().map(|view| flags(view)).collect::<Vec<_>>(), [(true, false, Some(true)), (true, true, Some(true)), (true, true, Some(true))]);
        // Showing an agent's tab draws that view and hides the rest, and its start page gives way.
        with(|page| page.tabs.activate("t2"));
        let tree = wings_plugin::ui::export(view()).0;
        assert_eq!(views(&tree).iter().map(|view| flags(view)).collect::<Vec<_>>(), [(true, false, Some(true)), (false, true, Some(true)), (true, true, Some(true))]);
        // A tab of the person's own shown is not hidden and not live.
        with(|page| page.tabs.activate("t1"));
        let tree = wings_plugin::ui::export(view()).0;
        assert_eq!(views(&tree).iter().map(|view| flags(view)).collect::<Vec<_>>(), [(false, false, Some(true)), (true, true, Some(true)), (true, true, Some(true))]);
    }

    #[test]
    fn a_tab_views_events_name_its_tab_so_a_background_tab_keeps_its_own_state() {
        busy();
        let (tree, handlers) = wings_plugin::ui::export(view());
        let found = views(&tree);
        let on_state = |at: usize| found[at]["props"]["onState"].as_str().unwrap().to_owned();
        assert!(handlers.call(&on_state(1), &json!({ "url": "https://b.com/next", "title": "B next", "loading": false })));
        assert_eq!(with(|page| page.tabs.get("t2").map(|tab| tab.state.url.clone())), Some("https://b.com/next".into()));
        assert_eq!(with(|page| page.tabs.get("t1").map(|tab| tab.state.url.clone())), Some("https://a.com/".into()), "the person's tab is as it was");
    }

    /// The tab strip drawn now, and its handlers.
    fn strip() -> (Value, wings_plugin::ui::Handlers) {
        let (tree, handlers) = wings_plugin::ui::export(view());
        let mut strips = Vec::new();
        all(&tree, "TabStrip", &mut strips);
        (strips.first().cloned().cloned().expect("a tab strip"), handlers)
    }

    /// What each item of the strip is, in order: a group's id, or a tab's id with whether it is the one shown.
    fn items(strip: &Value) -> Vec<String> {
        strip["children"].as_array().unwrap().iter().map(|item| match item["type"].as_str() {
            Some("TabGroup") => format!("group {}", item["props"]["id"].as_str().unwrap()),
            _ => format!("{}{}", item["props"]["id"].as_str().unwrap(), if item["props"]["active"] == true { " (shown)" } else { "" }),
        }).collect()
    }

    #[test]
    fn the_strip_has_each_group_with_its_name_and_tabs_then_the_persons_tabs() {
        busy();
        with(|page| page.tabs.get_mut("t1").unwrap().state.title = "A page with a rather long title indeed".into());
        let (drawn, _) = strip();
        assert_eq!(items(&drawn), ["group ses-a", "t2", "group ses-b", "t3", "t1", "t4 (shown)"]);
        let children = drawn["children"].as_array().unwrap();
        // Each group has its name and a tone of its own, and its tabs say whose they are; a tab with no title is named by
        // its address, and the strip cuts a long one itself.
        assert_eq!((children[0]["props"]["name"].as_str(), children[2]["props"]["name"].as_str()), (Some("Fix login bug"), Some("Write docs")));
        assert_ne!(children[0]["props"]["tone"], children[2]["props"]["tone"]);
        assert_eq!((children[1]["props"]["group"].as_str(), children[1]["props"]["title"].as_str()), (Some("ses-a"), Some("https://b.com/")));
        assert_eq!(children[4]["props"]["title"], "A page with a rather long title indeed");
        assert!(children[4]["props"].get("group").is_none());
    }

    #[test]
    fn a_click_on_a_groups_label_folds_it_and_another_unfolds_it() {
        busy();
        with(|page| page.tabs.open(Some(("ses-a", "Fix login bug")), Some("https://b.com/2")));
        let (drawn, handlers) = strip();
        let fold = drawn["props"]["onGroup"].as_str().unwrap().to_owned();
        assert!(handlers.call(&fold, &json!({ "id": "ses-a" })));
        let (drawn, _) = strip();
        let group = &drawn["children"][0]["props"];
        assert_eq!((group["collapsed"].as_bool(), group["count"].as_u64()), (Some(true), Some(2)));
        assert!(with(|page| page.tabs.group("ses-a").unwrap().collapsed));
        assert!(handlers.call(&fold, &json!({ "id": "ses-a" })));
        assert!(!with(|page| page.tabs.group("ses-a").unwrap().collapsed));
    }

    #[test]
    fn the_strip_makes_a_tab_picks_one_and_closes_one() {
        only(Some("https://a.com/"));
        let (drawn, handlers) = strip();
        let on = |name: &str| drawn["props"][name].as_str().unwrap().to_owned();
        assert!(handlers.call(&on("onNew"), &Value::Null));
        assert_eq!((with(|page| page.tabs.list.len()), shown(|tab| tab.blank)), (2, true), "New tab opens a blank tab and shows it");
        assert!(handlers.call(&on("onSelect"), &json!({ "id": "t1" })));
        assert_eq!(with(|page| page.tabs.active.clone()), "t1");
        // Closing the shown tab shows its neighbour, and the last one left is blank, not gone.
        assert!(handlers.call(&on("onClose"), &json!({ "id": "t1" })));
        assert_eq!(with(|page| (page.tabs.list.len(), page.tabs.active.clone())), (1, "t2".into()));
        assert!(handlers.call(&on("onClose"), &json!({ "id": "t2" })));
        assert_eq!(with(|page| (page.tabs.list.len(), page.active().blank)), (1, true));
    }

    #[test]
    fn the_tabs_are_kept_with_the_wings_tab_and_come_back_with_their_groups_and_the_one_shown() {
        busy();
        with(|page| {
            page.tabs.groups[0].collapsed = true;
            page.tabs.activate("t3");
            page.restored = true;
        });
        let kept = with(Page::to_save).expect("something to keep");
        assert_eq!(with(Page::to_save), None, "nothing changed since");
        with(|page| *page = Page::fresh(Some("https://other.com/"), None));
        with(|page| assert!(page.tabs.restore(&kept)));
        assert_eq!(with(|page| page.tabs.snapshot()), kept);
        let (drawn, _) = strip();
        assert_eq!(items(&drawn), ["group ses-a", "t2", "group ses-b", "t3 (shown)", "t1", "t4"]);
        assert_eq!(drawn["children"][0]["props"]["collapsed"], true);
        // Nothing is kept before the saved tabs have been read, which would write over them.
        with(|page| *page = Page::default());
        assert_eq!(with(Page::to_save), None);
    }

    #[test]
    fn a_restored_page_opens_a_new_address_as_one_more_tab_in_its_group_but_not_the_one_it_opened_with_before() {
        let args = |url: &str, group: Option<(&str, &str)>| OpenArgs { url: Some(url.into()), group: group.map(|(key, name)| (key.to_owned(), name.to_owned())) };
        let first = args("https://a.com/", None);
        let mut page = Page::fresh(first.url.as_deref(), None);
        page.tabs.args_seen = first.signature();
        page.tabs.open(Some(("ses-a", "Fix login bug")), Some("https://b.com/"));
        let kept = page.tabs.snapshot();
        // Launched again with the same arguments: no duplicate.
        let mut again = Page::fresh(first.url.as_deref(), None);
        assert!(again.tabs.restore(&kept));
        again.open_args(&first);
        assert_eq!(again.tabs.list.len(), 2);
        // Opened with an address for an agent: a tab in its group, which is made when it isn't there.
        again.open_args(&args("https://new.com/", Some(("ses-c", "Check CI"))));
        assert_eq!(again.tabs.list.len(), 3);
        let target = again.tabs.target("ses-c").unwrap();
        assert_eq!((again.tabs.get(&target).unwrap().start.as_str(), again.tabs.group("ses-c").map(|group| group.name.as_str())), ("https://new.com/", Some("Check CI")));
        // The saved tabs win over the address: the tab that was shown still is.
        assert_eq!(again.tabs.active, "t1");
    }
}
