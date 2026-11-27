//! A browser tab: back, forward and reload, an address bar with suggestions from history, and the page under them, which
//! Wings draws with WebKit. Everything the tab keeps (history, settings) is in storage, so each tab reads it itself.

use crate::{
    address::resolve,
    agent_page,
    extension::{Staged, dialog_for},
    history::{self, Entry},
    settings,
    start_page::{self, RECENT},
};
use serde_json::Value;
use std::cell::RefCell;
use wings_plugin::{
    host::{self, Stage},
    task::spawn,
    ui::{
        Choice, Combobox, ExtensionButton, Node, WebState, actions as dialog_actions, badge, button, combobox_submit, dialog, icon_button, icons, image, menu, muted, notice, page_with, row, slot, stack,
        text, toast, web_command, web_extension_action, web_view,
    },
};

const SUGGESTIONS: usize = 6;
/// `set_tab_title` takes one line of at most 80 characters.
const TITLE_MAX: usize = 80;
/// The width of the start page's column, and of the toolbar over it.
const START_COLUMN: u32 = 720;

#[derive(Default)]
pub(crate) struct Page {
    /// Where the view starts, once.
    pub start: String,
    /// A new tab that has no address yet: the start page is shown in place of the view until one is chosen.
    blank: bool,
    /// What the start page draws from: the terminals and the ports they listen on, the project, and the pages visited.
    panes: Vec<Value>,
    ports: Value,
    project: Value,
    visited: Vec<Entry>,
    pub state: WebState,
    /// The commands sent lately, oldest first, which the view runs once each by `token`: two sent before the next tree
    /// (an agent's step and a click) both run.
    pub commands: Vec<Value>,
    token: u64,
    /// What is in the address field while it is being edited, in place of the page's address.
    typed: Option<String>,
    /// History as of when typing began, for the suggestions.
    history: Vec<Entry>,
    /// The page is a Chrome Web Store or Firefox add-ons page, so its extension can be added.
    store: bool,
    staged: Option<Staged>,
    busy: bool,
    problem: Option<String>,
    added: Option<String>,
    /// The page last put in history, and the title last given the tab, so a repeat of either does nothing.
    recorded: (String, String),
    titled: String,
    /// The token of the last click of an extension's button, whose refusal shows in the red line.
    opening: Option<u64>,
    /// The address "Clear data for" was chosen on, while its dialog asks.
    clearing_site: Option<String>,
    /// What clearing a site did, shown over the foot of the page: its tone and words.
    cleared: Option<(&'static str, String)>,
}

/// What a change of the view's state asks the tab to do.
#[derive(Debug, Default, PartialEq)]
struct Effects {
    /// A new title for the tab: none for the view's own.
    title: Option<Option<String>>,
    /// A finished page for history.
    record: Option<(String, String)>,
    /// A new address to ask whether it is a store page.
    check_store: Option<String>,
}

/// The most commands a page keeps sending: more than a step and a click between two draws.
const COMMANDS_KEPT: usize = 16;

impl Page {
    pub fn send(&mut self, action: &str, url: Option<&str>) {
        // The view isn't there yet: an address starts it there, and nothing else has a page to act on.
        if self.blank {
            if let ("load", Some(url)) = (action, url) {
                self.blank = false;
                self.start = url.to_owned();
            }
            return;
        }
        self.run(|token| web_command(action, url, token));
    }

    /// Sends the view a command made from a new token, which the view runs once, and gives the token.
    pub fn run(&mut self, make: impl FnOnce(u64) -> Value) -> u64 {
        self.token += 1;
        self.commands.push(make(self.token));
        // The view has run all but the newest few by the time the page draws again.
        let extra = self.commands.len().saturating_sub(COMMANDS_KEPT);
        self.commands.drain(..extra);
        self.token
    }

    fn apply(&mut self, new: WebState) -> Effects {
        let mut effects = Effects::default();
        let moved = self.state.url != new.url;
        if moved {
            self.typed = None;
            self.store = false;
            self.added = None;
            effects.check_store = (new.url.starts_with("http://") || new.url.starts_with("https://")).then(|| new.url.clone());
        }
        if new.title != self.titled {
            self.titled.clone_from(&new.title);
            effects.title = Some((!new.title.is_empty()).then(|| new.title.chars().take(TITLE_MAX).collect()));
        }
        let finished = !new.loading && new.error.is_none() && !new.url.is_empty();
        if finished && self.recorded != (new.url.clone(), new.title.clone()) {
            self.recorded = (new.url.clone(), new.title.clone());
            effects.record = Some(self.recorded.clone());
        }
        self.state = new;
        effects
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

/// The part of a web page's address that names its site, as the person reads it (`localhost:4300`): none for the start
/// page and anything that isn't an `http` or `https` page, which keep nothing to clear.
fn site_name(url: &str) -> Option<String> {
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let name = authority.rsplit_once('@').map_or(authority, |(_, after)| after);
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}

/// Removes the cookies and site data of the site of `url`, then loads the page again, which shows it signed out. The host
/// only does it right after a click, so this runs in the click of the dialog's button.
fn clear_site(url: String) {
    let name = site_name(&url).unwrap_or_default();
    with(|page| {
        page.clearing_site = None;
        page.cleared = None;
    });
    spawn(async move {
        let result = host::web_data_clear_site(&url).await;
        with(|page| match result {
            Ok(_) => {
                page.send("reload", None);
                page.cleared = Some(("success", format!("Cleared the data of {name}.")));
            }
            Err(error) => page.cleared = Some(("danger", error)),
        });
    });
}

fn site_menu(url: &str) -> Option<Node> {
    let name = site_name(url)?;
    let asked = url.to_owned();
    let trigger = Node::new("Button").prop("variant", "ghost").prop("size", "icon-sm").prop("label", "More").prop("title", "More").child(wings_plugin::ui::icon(24, 2.0, &icons::ELLIPSIS).prop("size", 16));
    let item = Node::new("Button").key("clear-site").on("onClick", move |_| with(|page| page.clearing_site = Some(asked.clone()))).text(format!("Clear data for {name}"));
    Some(menu(trigger, [item], true))
}

fn clear_site_dialog(url: &str) -> Node {
    let name = site_name(url).unwrap_or_default();
    let go = url.to_owned();
    dialog(&format!("Clear data for {name}?"), 420, || with(|page| page.clearing_site = None))
        .child(muted("This signs you out of it and removes what it stored. Other sites keep theirs."))
        .child(dialog_actions(Some(button("Cancel", "ghost", false, || with(|page| page.clearing_site = None))), [button("Clear", "destructive", false, move || clear_site(go.clone()))]))
}

fn command(action: &'static str) {
    with(|page| page.send(action, None));
}

/// Loads what is typed, or reloads the page when nothing was changed.
fn go() {
    let engine = settings::engine().get();
    with(|page| {
        let typed = page.typed.take().unwrap_or_else(|| page.state.url.clone());
        if let Some(url) = resolve(&typed, engine) {
            page.send("load", Some(&url));
        }
    });
}

fn typed(text: String) {
    with(|page| {
        if page.typed.is_none() {
            page.history = history::load();
        }
        page.typed = Some(text);
    });
}

fn state(new: WebState) {
    let effects = with(|page| page.apply(new));
    agent_page::changed();
    if let Some(title) = effects.title {
        drop(host::set_tab_title(title.as_deref()));
    }
    if let Some((url, title)) = effects.record {
        drop(history::add(&url, &title, host::now()));
    }
    if let Some(url) = effects.check_store {
        spawn(async move {
            let found = host::web_extension_store_page(&url).await.is_ok_and(|answer| !answer.is_null());
            with(|page| page.store = found && page.state.url == url);
        });
    }
}

/// Stages the extension of the store page the tab shows. Called from the click, since the host only starts one right after.
fn stage_store() {
    let Some(url) = with(|page| (!page.busy).then(|| page.state.url.clone())) else {
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
                Ok(staged) => page.staged = Staged::from_json(&staged),
                Err(error) => page.problem = Some(error),
            }
        });
    });
}

fn add() {
    let Some(stage) = with(|page| if page.busy { None } else { page.staged.as_ref().map(|staged| staged.stage.clone()) }) else {
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
                Ok(_) => page.added = page.staged.take().map(|staged| format!("Added {}. It is on for the pages you open now.", staged.name)),
                Err(error) => page.problem = Some(error),
            }
        });
    });
}

fn cancel() {
    if let Some(staged) = with(|page| {
        page.problem = None;
        page.staged.take()
    }) {
        drop(host::web_extension_discard(&staged.stage));
    }
}

/// Clicks an extension's button for the page: its popup opens under the button. From the click, which Wings needs.
fn open_extension(id: &str) {
    with(|page| {
        page.problem = None;
        page.opening = Some(page.run(|token| web_extension_action(id, token)));
    });
}

/// What a command came to: a refused extension button says why in the red line, and the rest are the agent's.
fn result_event(event: &Value) {
    let refused = with(|page| {
        let mine = page.opening.is_some() && event["token"].as_u64() == page.opening;
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

fn address(page: &Page) -> Node {
    let options = page
        .typed
        .as_deref()
        .map(|typed| {
            history::suggest(&page.history, typed, SUGGESTIONS)
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
        value: page.typed.clone().unwrap_or_else(|| page.state.url.clone()),
        query: None,
        options,
        // Only while something is typed: after Enter, or with the page's own address in the field, there is nothing to submit.
        empty: if page.typed.is_some() { "Press Enter to search or go" } else { "" },
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
            page.typed = None;
            page.send("load", Some(url));
        }),
        |key| match key {
            "Enter" => go(),
            "Escape" => with(|page| page.typed = None),
            _ => {}
        },
    )
}

/// What the start page shows now, and a look at the ports again: a server started since the tab opened comes in.
fn look_again() {
    if !with(|page| page.blank) {
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
    with(|page| page.send("load", Some(url)));
}

fn view() -> Vec<Node> {
    let keep = settings::keep_data().get();
    PAGE.with(|page| {
        let page = page.borrow();
        let state = &page.state;
        // In the toolbar's children, which take the width the title would leave, so the address field grows with the tab.
        let nav = row()
            .prop("grow", true)
            .prop("gap", "xs")
            .prop("align", "center")
            .child(icon_button("Back", &icons::CHEVRON_LEFT, || command("back")).disabled(!state.can_go_back))
            .child(icon_button("Forward", &icons::CHEVRON_RIGHT, || command("forward")).disabled(!state.can_go_forward))
            .child(if state.loading { icon_button("Stop", &icons::CLOSE, || command("stop")) } else { icon_button("Reload", &icons::REFRESH_CW, || command("reload")) })
            .child(stack().prop("grow", true).child(address(&page)));
        let actions = row()
            .prop("gap", "s")
            .prop("align", "center")
            .maybe(page.store.then(|| button("Add to Wings", "secondary", page.busy, stage_store).size("sm")))
            .children(state.extensions.iter().map(extension_button))
            .child(if agent_page::popped() {
                icon_button("Put back in a tab", &icons::MINIMIZE_2, agent_page::pop_button_action)
            } else {
                icon_button("Pop out into a window", &icons::EXTERNAL_LINK, agent_page::pop_button_action)
            })
            .maybe(site_menu(&state.url))
            .child(icon_button("Browser settings", &icons::SETTINGS, || drop(host::open_settings())));
        let problem = if page.staged.is_none() { page.problem.as_deref() } else { None };
        let top = [
            Some(Node::new("Toolbar").child(slot("children", nav)).child(slot("actions", actions))),
            state.error.as_deref().map(|error| notice("danger", error).key("error")),
            problem.map(|problem| notice("danger", problem).key("problem")),
            page.added.as_deref().map(|added| notice("success", added).key("added")),
            agent_page::handoff_banner(),
        ];
        let toasts = page.cleared.as_ref().map(|(tone, words)| toast(tone, words.as_str(), || with(|page| page.cleared = None)).key("cleared"));
        let ask = page.clearing_site.as_deref().map(clear_site_dialog);
        if page.blank {
            let servers = start_page::servers(&page.ports, &page.panes, &page.project);
            let recent = start_page::recent(&page.visited, &servers, RECENT);
            let body = start_page::view(&servers, &recent, open_from_start);
            return vec![page_with(top.into_iter().flatten(), [body]).prop("column", START_COLUMN).toasts(toasts)];
        }
        let mut web = web_view("page").prop("fill", true).prop("url", page.start.as_str()).prop("keepData", keep).on("onState", state_event).on("onResult", result_event);
        if !page.commands.is_empty() {
            web = web.prop("command", Value::Array(page.commands.clone()));
        }
        let dialog = page.staged.as_ref().map(|staged| dialog_for(staged, page.busy, page.problem.as_deref(), cancel, add));
        vec![page_with(top.into_iter().flatten(), std::iter::once(web).chain(dialog).chain(ask)).toasts(toasts)]
    })
}

fn state_event(event: &Value) {
    state(WebState::from_event(event));
}

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    let engine = settings::engine().get();
    let args = host::view_args();
    let asked = args.as_ref().and_then(|args| args["url"].as_str().and_then(|url| resolve(url, engine)));
    let blank = asked.is_none();
    let start = asked.unwrap_or_else(|| engine.home().to_owned());
    with(|page| {
        page.start.clone_from(&start);
        page.blank = blank;
    });
    if blank {
        host::on_panes(|list| with(|page| page.panes = list.to_vec()));
        host::on_focus(|focused| {
            if focused {
                look_again();
            }
        });
        look_again();
    }
    agent_page::start(args.as_ref(), &start);
    wings_plugin::run_filled(view);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(url: &str, title: &str) -> WebState {
        WebState { url: url.into(), title: title.into(), ..WebState::default() }
    }

    /// The hint under the address field is for text being typed: with the loaded page's address in the field, and after
    /// Enter has submitted what was typed, it isn't there.
    #[test]
    fn the_address_hint_shows_while_typing_and_not_once_the_page_is_loaded() {
        let hint = |page: &Page| wings_plugin::ui::tree_json(address(page)).to_string().contains("Press Enter to search or go");
        let mut page = Page::default();
        page.apply(loaded("https://a.com/", "A"));
        assert!(!hint(&page), "a loaded page's address is not a search");
        page.typed = Some("zzz-no-history-match".into());
        assert!(hint(&page), "typing something nothing matches");
        page.typed = None;
        assert!(!hint(&page), "after Enter or Escape");
    }

    fn drawn() -> String {
        wings_plugin::ui::export(view()).0.to_string()
    }

    #[test]
    fn a_web_page_has_a_clear_data_menu_item_that_asks_first_and_the_start_page_has_none() {
        with(|page| *page = Page { start: "http://localhost:4300/".into(), ..Page::default() });
        with(|page| page.state = loaded("http://localhost:4300/app", "App"));
        let now = drawn();
        assert!(now.contains("\"Menu\"") && now.contains("Clear data for localhost:4300"), "{now}");
        assert!(!now.contains("Clear data for localhost:4300?"), "it asks only once chosen: {now}");
        with(|page| page.clearing_site = Some("http://localhost:4300/app".into()));
        let now = drawn();
        for shown in ["\"Dialog\"", "Clear data for localhost:4300?", "This signs you out of it and removes what it stored. Other sites keep theirs.", "\"Cancel\"", "\"destructive\""] {
            assert!(now.contains(shown), "{shown} is not in {now}");
        }
        with(|page| *page = Page { start: "https://duckduckgo.com/".into(), blank: true, ..Page::default() });
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
        with(|page| *page = Page { start: "https://app.example.com/".into(), ..Page::default() });
        with(|page| page.state = loaded("https://app.example.com/inbox?x=1", "Inbox"));
        clear_site("https://app.example.com/inbox?x=1".into());
        wings_plugin::host::testing::settle();
        assert_eq!(*asked.borrow(), [("webData".to_owned(), serde_json::json!({ "action": "clearSite", "url": "https://app.example.com/inbox?x=1" }))]);
        assert!(with(|page| page.commands.iter().any(|command| command["action"] == "reload")));
        let now = drawn();
        assert!(now.contains("\"Toast\"") && now.contains("Cleared the data of app.example.com.") && now.contains("success"), "{now}");
    }

    #[test]
    fn a_failed_clear_shows_the_error_in_a_toast_and_does_not_reload() {
        wings_plugin::host::testing::answer(|_, _| Err("WebKit didn't answer".into()));
        with(|page| *page = Page { start: "https://a.com/".into(), ..Page::default() });
        clear_site("https://a.com/".into());
        wings_plugin::host::testing::settle();
        assert!(with(|page| page.commands.is_empty()));
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
        with(|page| *page = Page { start: "https://duckduckgo.com/".into(), blank: true, ..Page::default() });
        let drawn = wings_plugin::ui::export(view()).0.to_string();
        assert!(!drawn.contains("\"WebView\""), "{drawn}");
        with(|page| page.send("reload", None));
        assert!(with(|page| page.blank && page.commands.is_empty()), "nothing to reload yet");
        with(|page| page.send("load", Some("http://localhost:5173/")));
        let drawn = wings_plugin::ui::export(view()).0.to_string();
        assert!(drawn.contains("\"WebView\"") && drawn.contains("\"url\":\"http://localhost:5173/\""), "{drawn}");
        assert!(with(|page| page.commands.is_empty()), "the view loads its start address itself");
    }

    #[test]
    fn each_command_gets_a_new_token_so_the_view_runs_it_again() {
        let mut page = Page::default();
        page.send("reload", None);
        page.send("reload", None);
        page.send("load", Some("https://a.com"));
        assert_eq!(page.commands.last(), Some(&web_command("load", Some("https://a.com"), 3)));
        assert_eq!(page.commands.len(), 3, "the earlier two are still sent, so none is dropped before the view runs it");
        for _ in 0..20 {
            page.send("reload", None);
        }
        assert_eq!(page.commands.len(), COMMANDS_KEPT);
    }

    #[test]
    fn a_page_is_recorded_once_when_it_has_finished_without_an_error() {
        let mut page = Page::default();
        let loading = WebState { loading: true, ..loaded("https://a.com/", "") };
        assert_eq!(page.apply(loading.clone()).record, None);
        assert_eq!(page.apply(loaded("https://a.com/", "A")).record, Some(("https://a.com/".into(), "A".into())));
        assert_eq!(page.apply(WebState { can_go_back: true, ..loaded("https://a.com/", "A") }).record, None, "the same page again");
        let failed = WebState { error: Some("offline".into()), ..loaded("https://b.com/", "B") };
        assert_eq!(page.apply(failed).record, None);
    }

    #[test]
    fn the_tab_title_follows_the_page_title_and_is_cut_to_80_characters() {
        let mut page = Page::default();
        assert_eq!(page.apply(loaded("https://a.com/", "A")).title, Some(Some("A".into())));
        assert_eq!(page.apply(loaded("https://a.com/", "A")).title, None);
        assert_eq!(page.apply(loaded("https://a.com/", &"x".repeat(100))).title, Some(Some("x".repeat(80))));
        assert_eq!(page.apply(loaded("https://a.com/", "")).title, Some(None), "back to the view's own title");
    }

    #[test]
    fn going_to_another_address_ends_an_edit_and_asks_about_the_store_for_web_pages_only() {
        let mut page = Page { typed: Some("half".into()), store: true, ..Page::default() };
        let effects = page.apply(loaded("https://chromewebstore.google.com/detail/x/y", ""));
        assert_eq!((page.typed.clone(), page.store, effects.check_store), (None, false, Some("https://chromewebstore.google.com/detail/x/y".into())));
        assert_eq!(page.apply(loaded("about:blank", "")).check_store, None);
        assert_eq!(page.apply(loaded("about:blank", "")).check_store, None);
    }

    #[test]
    fn typing_is_kept_until_the_address_changes() {
        let mut page = Page { typed: Some("half".into()), ..Page::default() };
        page.apply(WebState { loading: true, ..loaded("", "") });
        assert_eq!(page.typed.as_deref(), Some("half"));
    }

    /// The first button in `tree` read out as `label`.
    fn button_named<'a>(tree: &'a Value, label: &str) -> Option<&'a Value> {
        if tree["type"] == "Button" && tree["props"]["label"] == label {
            return Some(tree);
        }
        let children = tree.as_array().or_else(|| tree["children"].as_array())?;
        children.iter().find_map(|child| button_named(child, label))
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
        with(|page| *page = Page { start: "https://github.com/".into(), state, ..Page::default() });
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
        let sent = with(|page| (page.commands.last().cloned(), page.opening));
        assert_eq!(sent, (Some(web_extension_action("e1", 1)), Some(1)));
        // Wings refuses one that came without your click: the page says why.
        result_event(&serde_json::json!({ "token": 1, "error": "only right after you click" }));
        assert_eq!(with(|page| page.problem.clone()), Some("only right after you click".into()));
        // A page with no extensions has no buttons for them.
        with(|page| page.state.extensions.clear());
        assert!(button_named(&wings_plugin::ui::export(view()).0, "Bitwarden").is_none());
    }

    #[test]
    fn the_page_is_a_toolbar_over_a_web_view_that_fills_and_offers_the_store_button_only_on_a_store_page() {
        with(|page| *page = Page { start: "https://duckduckgo.com/".into(), ..Page::default() });
        let drawn = wings_plugin::ui::export(view()).0.to_string();
        assert!(drawn.contains("\"WebView\"") && drawn.contains("\"fill\":true") && drawn.contains("\"url\":\"https://duckduckgo.com/\""), "{drawn}");
        assert!(!drawn.contains("Add to Wings"));
        with(|page| page.store = true);
        assert!(wings_plugin::ui::export(view()).0.to_string().contains("Add to Wings"));
    }
}
