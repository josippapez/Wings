//! Settings, Plugins and the dialog that asks before a plugin runs. The page is a header with Add plugin (a GitHub link
//! or a file), a filter bar (Installed, Browse, Updates, and a search) and a list of rows. A row says what the plugin
//! does, with one status word, and clicking it opens the plugin's own page (`plugin_detail_ui.rs`) in place of the list.

use crate::workspace::NotifyWorkspace;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gpui::{
    AnimationExt, AnyElement, Bounds, ClipboardItem, Context, ElementId, ExternalPaths, FontWeight, KeyDownEvent,
    MouseButton, Pixels, Role, SharedString, deferred, div, prelude::*, px, rgb, rgba,
};
use wings_plugins::{
    manifest::Manifest,
    store::{self, Available, PluginView, Source},
};

use crate::{
    icons::{Icon, icon, plugin_glyph},
    theme,
    ui::{
        self, ButtonSize, Variant,
        behaviors::{DismissReason, EscapeExt, TrapTabExt, bind_layer},
    },
};

use super::Workspace;

/// The desktop's names for a plugin's `ui` parts.
fn ui_name(kind: &str) -> String {
    match kind {
        "badges" => "Pane badges".into(),
        "diff" => "Diff viewer".into(),
        "terminals" => "The terminals its agent opens, in its page".into(),
        other => other.into(),
    }
}

/// One of the desktop's `size="xs"` ghost buttons over the installed list: its icon, or a turning ring in its place
/// while it `works`, when it is dimmed and doesn't answer a click. Add the click handler to the result.
fn header_button(
    id: &'static str,
    label: SharedString,
    glyph: Icon,
    works: bool,
) -> gpui::Stateful<gpui::Div> {
    let mark = if works {
        div()
            .id(SharedString::from(format!("{id}-busy")))
            .size(px(16.0))
            .flex_none()
            .with_animation(
                SharedString::from(format!("{id}-turn")),
                gpui::Animation::new(std::time::Duration::from_secs(1))
                    .repeat()
                    .with_max_fps(30.0),
                |ring, turn| ring.child(ui::working_spinner(turn * std::f32::consts::TAU)),
            )
            .into_any_element()
    } else {
        icon(glyph, 12.0, theme::foreground()).into_any_element()
    };
    ui::button(
        id,
        label.clone(),
        Variant::Outline,
        works,
        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(mark)
            .child(label),
    )
    .h(px(24.0))
    .px(px(8.0))
    .text_size(px(12.0))
}

/// The host of an https address, as the desktop's `new URL(url).host`.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .to_owned()
}

/// What turning a plugin on lets it do, in plain words, as the desktop's `Permissions` lists it: each group's title
/// and its items. A yes or no permission that is on is a group without items.
pub(super) fn access_groups(manifest: &Manifest) -> Vec<(&'static str, Vec<String>)> {
    let p = &manifest.permissions;
    let c = &manifest.contributes;
    let shows: Vec<String> =
        c.ui.iter()
            .map(|kind| ui_name(kind))
            .chain(c.sidebars.iter().map(|s| {
                format!(
                    "{} sidebar{}",
                    s.title,
                    if s.popover.is_some() {
                        " and popover"
                    } else {
                        ""
                    }
                )
            }))
            .chain(
                c.views
                    .iter()
                    .map(|v| format!("{} {}", v.title, v.opens.join(" or "))),
            )
            .chain(c.settings.as_ref().map(|_| "Settings page".to_owned()))
            .chain(c.links.iter().map(|link| format!("{} links in messages", link.title)))
            .collect();
    let on = |yes: bool| if yes { Some(Vec::new()) } else { None };
    let list = |items: &[String]| (!items.is_empty()).then(|| items.to_vec());
    // Loading lets an http entry reach only this computer, and binds no token to one.
    let (local, web): (Vec<String>, Vec<String>) = p.fetch.iter().cloned().partition(|entry| {
        entry
            .split_once(' ')
            .map_or(entry.as_str(), |(_, url)| url)
            .starts_with("http://")
    });
    [
        (SHOWS, list(&shows)),
        ("Call these web addresses, with tokens you give it", list(&web)),
        (
            "Call the programs on this computer that listen at these addresses",
            list(&local),
        ),
        (
            "Keep these tokens in your keychain, each sent only to its own address",
            list(&p.secrets.iter().map(|(name, prefix)| format!("{name} → {prefix}")).collect::<Vec<_>>()),
        ),
        ("Open these sites in a panel from the title bar", list(&c.panels.iter().map(|panel| host_of(&panel.url)).collect::<Vec<_>>())),
        ("Offer Claude these tools over MCP", list(&c.mcp_tools.iter().map(|tool| tool.name.clone()).collect::<Vec<_>>())),
        ("Answer these Claude Code hooks: add context to a session, or refuse a Bash command", list(&c.hooks)),
        ("Run these commands as you, with your own sign-ins", list(&p.exec)),
        (
            "Keep these programs running as you and talk to them. An agent like claude run this way does what the plugin tells it, with your permissions",
            list(&p.processes),
        ),
        ("Post to these addresses as you, like replying to a review comment", list(&p.post)),
        ("Open and close terminals in your projects, and move your typing to any pane", on(p.panes.is_some())),
        ("Start these commands in a new terminal, as you", p.panes.as_deref().and_then(list)),
        ("Show desktop notifications, up to 20 a minute", on(p.notify)),
        ("Type messages into the prompts of the agents in your terminals", on(p.send_to_agents)),
        ("Make git worktrees of your projects, on new branches beside the repo", on(p.worktrees)),
        (
            "Choose which Claude Code account each project's new terminals use, and see the email each one is signed in with",
            on(p.claude_accounts),
        ),
        ("Use these Wings plugins' tools and data", list(&p.call_tools)),
        ("Open these views of other Wings plugins, like a chat that starts with its prompt", list(&p.open_views)),
        ("Hear these events from other Wings plugins", list(&p.events)),
        ("Read these parts of your Claude Code sessions", list(&p.transcript)),
        ("Read your Claude usage limits and each session's context and cache, from Claude Code's status line", on(p.statusline)),
        ("Read what your OpenCode sessions are doing: their latest prompts, tool calls and token use", on(p.agent_activity)),
        ("See which local ports the programs in your terminals listen on", on(p.ports)),
        (PANE_OUTPUT, on(p.pane_output)),
        ("Read the names and sizes of the files and folders on your disks", on(p.disk_usage)),
        ("Open links that start with", list(&p.open_url)),
        (WEB_VIEW, on(p.web_view)),
        (BROWSER_COOKIES, on(p.browser_cookies)),
        (WEB_SCRIPTS, on(p.web_scripts)),
    ]
    .into_iter()
    .filter_map(|(title, items)| Some((title, items?)))
    .collect()
}

/// The group of what a plugin adds to Wings: its items are plain words ("Pane badges"), not commands or addresses.
const SHOWS: &str = "Show these in Wings";

/// Whether a group's items are plain words, which read as text, not as code.
fn plain_items(title: &str) -> bool {
    title == SHOWS
}

/// How the approval names `paneOutput`. Terminal output is where programs print tokens and passwords, so it says so.
const PANE_OUTPUT: &str = "Read what your terminals show, their recent output. That can include passwords, tokens and other secrets a program printed";

/// How the approval names `webView`. The pages it shows are any site, with a cookie store of the plugin's own.
const WEB_VIEW: &str = "Show web pages in its tabs, keep their cookies and site data, and add browser extensions you choose";
/// How the approval names `browserCookies`. An import signs the plugin's pages in wherever that browser is signed in.
const BROWSER_COOKIES: &str = "Copy the cookies of another browser on this Mac when you start an import, which signs its pages in where that browser is";

/// How the approval names `webScripts`: a script in a page reads and changes what you are signed in to there.
const WEB_SCRIPTS: &str = "Run its own scripts in the web pages it shows, which can read and change what you are signed in to there";

/// What a plugin built for JavaScript says in its page in Settings.
pub(super) const BUILT_FOR_JAVASCRIPT: &str = "Wings runs plugins built to WebAssembly. This version is written in JavaScript, so it doesn't run. Install an update if one is offered, or ask its author for a version written in Rust.";

pub(super) fn muted(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(12.0))
        .text_color(rgb(theme::muted_foreground()))
        .child(text.into())
}

/// The groups of `access_groups`, each a title over its items as small code chips.
pub(super) fn access_list(manifest: &Manifest) -> AnyElement {
    let groups = access_groups(manifest);
    if groups.is_empty() {
        return muted("It doesn't ask for any access beyond showing things in Wings.")
            .into_any_element();
    }
    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .children(groups.into_iter().map(|(title, items)| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(muted(title).font_weight(FontWeight::MEDIUM))
                .when(!items.is_empty(), |group| {
                    group.child(div().flex().flex_wrap().gap(px(6.0)).children(
                        items.into_iter().map(|item| {
                            // The desktop's `max-w-full break-all`: a long command wraps inside the page.
                            div()
                                .max_w_full()
                                .px(px(6.0))
                                .py(px(2.0))
                                .rounded(px(6.0))
                                .bg(rgba(0xffffff0f))
                                .when(!plain_items(title), |chip| chip.font_family("Menlo"))
                                .text_size(px(12.0))
                                .child(item)
                        }),
                    ))
                })
        }))
        .into_any_element()
}

/// The plugin's own SVG (`own`), else a puzzle piece, dimmed for a plugin that is off. It has no colour of its own:
/// amber is the page's one accent, for what needs you.
pub(super) fn plugin_icon(on: bool, own: Option<&str>, side: f32) -> gpui::Div {
    div()
        .size(px(side))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.0))
        .bg(rgba(0xffffff0f))
        .child(plugin_glyph(
            "",
            own,
            side / 2.0,
            if on {
                theme::foreground()
            } else {
                theme::muted_foreground()
            },
        ))
}

/// Which list the Plugins page shows under its header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PluginsTab {
    #[default]
    Installed,
    Browse,
    Updates,
}

/// The menu that is open on the Plugins page: the header's Add plugin, or one plugin's more actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PluginMenu {
    Add,
    Row(String),
}

/// The width of the action slot at a row's end (Review, Update or Install), the same in every row so a state change
/// never moves the switch beside it.
const ACTION_W: f32 = 84.0;
/// A row's height: its name line, and its description at two lines.
const ROW_H: f32 = 76.0;

/// The one word a plugin's row says about it, and its tone. What needs you comes before what is fine.
pub(super) fn status_word(
    approved: bool,
    enabled: bool,
    runs: bool,
    update: Option<&str>,
) -> (String, u32) {
    if !approved {
        ("Needs approval".into(), theme::working())
    } else if enabled && !runs {
        ("Couldn't start".into(), theme::danger())
    } else if let Some(version) = update {
        (format!("Update {version}"), theme::info())
    } else if enabled {
        ("On".into(), theme::done())
    } else {
        ("Off".into(), theme::muted_foreground())
    }
}

/// Whether a plugin matches what was typed in the search: every word of it is in the name or the description.
pub(super) fn matches_query(query: &str, name: &str, description: &str) -> bool {
    let haystack = format!("{name} {description}").to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

/// The installed plugins a tab lists, those matching `query`: all of them, or with Updates only those with a newer
/// version found.
pub(super) fn listed(
    views: &[PluginView],
    updates: &HashMap<String, String>,
    tab: PluginsTab,
    query: &str,
) -> Vec<PluginView> {
    views
        .iter()
        .filter(|view| tab != PluginsTab::Updates || updates.contains_key(&view.manifest.id))
        .filter(|view| matches_query(query, &view.manifest.name, &view.manifest.description))
        .cloned()
        .collect()
}

/// What Browse lists: the official plugins not installed yet, those matching `query`.
pub(super) fn browse(available: &[Available], views: &[PluginView], query: &str) -> Vec<Available> {
    available
        .iter()
        .filter(|plugin| !views.iter().any(|view| view.manifest.id == plugin.id))
        .filter(|plugin| matches_query(query, &plugin.name, &plugin.description))
        .cloned()
        .collect()
}

/// Where a plugin came from, as its page's byline says it.
pub(super) fn source_label(view: &PluginView) -> String {
    if view.dev {
        return "Wings plugin".into();
    }
    if view.folder.is_some() {
        return "Linked folder".into();
    }
    match &view.source {
        Some(Source::Github { repo, folder: Some(folder) })
            if store::is_official_repo(repo) && folder.starts_with("plugins/") =>
        {
            "Wings plugin".into()
        }
        Some(Source::Github { repo, .. }) => format!("GitHub · {repo}"),
        _ => "From a file".into(),
    }
}

/// Where a plugin's page opens from its "Adds to Wings" list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Open {
    /// A sidebar or a view, by id.
    Page(String),
    /// Its page in Settings.
    Settings,
}

/// One thing a plugin adds to Wings, with what opens it where something does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Adds {
    pub(super) kind: &'static str,
    pub(super) name: String,
    pub(super) open: Option<Open>,
}

/// What the manifest says a plugin adds to Wings, in the order the page lists it.
pub(super) fn adds_to_wings(manifest: &Manifest) -> Vec<Adds> {
    let c = &manifest.contributes;
    let item = |kind, name: String, open| Adds { kind, name, open };
    c.sidebars
        .iter()
        .map(|s| item("Sidebar", s.title.clone(), Some(Open::Page(s.id.clone()))))
        .chain(c.views.iter().map(|v| item("View", v.title.clone(), Some(Open::Page(v.id.clone())))))
        .chain(c.project_sections.iter().map(|s| item("Project section", s.title.clone(), None)))
        .chain(c.blocks.iter().map(|b| item("Project block", b.title.clone(), None)))
        .chain(c.ui.iter().map(|kind| item("Pane", ui_name(kind), None)))
        .chain(c.mcp_tools.iter().map(|t| item("MCP tool", t.name.clone(), None)))
        .chain(c.hooks.iter().map(|h| item("Claude Code hook", h.clone(), None)))
        .chain(c.links.iter().map(|l| item("Link in messages", l.title.clone(), None)))
        .chain(c.settings.iter().map(|_| item("Settings", "Settings page".into(), Some(Open::Settings))))
        .collect()
}

/// One plain line over the access list: what turning the plugin on lets it do, in a few words.
pub(super) fn access_summary(manifest: &Manifest) -> String {
    let p = &manifest.permissions;
    let phrases: Vec<&str> = [
        (!p.exec.is_empty() || !p.processes.is_empty(), "run commands as you"),
        (!p.fetch.is_empty() || !p.post.is_empty() || !p.secrets.is_empty(), "call web addresses"),
        (p.panes.is_some() || p.send_to_agents || p.worktrees, "open terminals and type into your agents"),
        (
            !p.transcript.is_empty() || p.pane_output || p.statusline || p.agent_activity || p.disk_usage || p.ports || p.claude_accounts,
            "read what your terminals, sessions or disks hold",
        ),
        (p.web_view, "show web pages"),
        (p.browser_cookies, "import another browser's sign-ins"),
        (p.web_scripts, "run scripts in the web pages it shows"),
        (p.notify, "show notifications"),
        (!p.call_tools.is_empty() || !p.open_views.is_empty() || !p.events.is_empty(), "use other plugins"),
    ]
    .into_iter()
    .filter_map(|(yes, phrase)| yes.then_some(phrase))
    .collect();
    match phrases.as_slice() {
        [] => "It doesn't ask for any access beyond showing things in Wings.".into(),
        [one] => format!("Turned on, it can {one}."),
        [init @ .., last] => format!("Turned on, it can {} and {last}.", init.join(", ")),
    }
}

/// A button as small as the page's rows want: 28 tall, 13 text.
fn small(button: gpui::Stateful<gpui::Div>) -> gpui::Stateful<gpui::Div> {
    button.h(px(28.0)).px(px(10.0)).text_size(px(13.0))
}

/// An element id of a control in `scope`'s rows, such as `plugin-row` or `plugin-detail`.
fn control_id(scope: &str, name: &str, index: usize) -> ElementId {
    ElementId::NamedInteger(format!("{scope}-{name}").into(), index as u64)
}

/// Draws nothing but notes where it is each draw, under `key`, for a menu that opens under its button.
fn track(map: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>, key: String) -> impl IntoElement {
    gpui::canvas(
        move |bounds, _, _| {
            map.borrow_mut().insert(key.clone(), bounds);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

impl Workspace {
    /// The status under a plugin's name: the desktop's, and for one that is on whether it runs here.
    pub(super) fn plugin_status(&self, view: &PluginView) -> String {
        let state = if !view.approved {
            "Waiting for approval"
        } else if view.enabled {
            "On"
        } else {
            "Off"
        };
        match (
            view.enabled && view.approved,
            self.plugin_runs(&view.manifest.id),
        ) {
            (true, true) => format!("{state} · Runs here"),
            (true, false) if self.built_for_javascript(view) => {
                format!("{state} · Built for JavaScript")
            }
            (true, false) => format!("{state} · No native build"),
            _ => state.to_owned(),
        }
    }

    /// What a row without a description says: the part of `plugin_status` after the state, which the switch and the word
    /// beside the name already say.
    pub(super) fn plugin_note(&self, view: &PluginView) -> String {
        self.plugin_status(view)
            .split_once(" · ")
            .map(|(_, note)| note.to_owned())
            .unwrap_or_default()
    }

    pub(super) fn plugins_section(&self, content_w: f32, cx: &mut Context<Self>) -> AnyElement {
        if self.settings.is_none() {
            return div().into_any_element();
        }
        let views = self.plugin_views();
        let open = self
            .manage
            .selected
            .as_deref()
            .and_then(|id| views.iter().find(|view| view.manifest.id == id))
            .cloned();
        let menu = self.plugins_menu(&views, cx);
        let page = match open {
            Some(view) => self.plugin_detail(view, content_w, cx),
            None => self.plugins_list(views, cx),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(page)
            .children(menu)
            .into_any_element()
    }

    /// The page header: its title strong, with `actions` at the right end.
    fn page_header(left: impl IntoElement, actions: impl IntoElement) -> gpui::Div {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .child(left)
            .child(actions)
    }

    /// The list of plugins: its header with Add plugin, the filter bar, and the rows of a tab.
    fn plugins_list(&self, views: Vec<PluginView>, cx: &mut Context<Self>) -> AnyElement {
        let Some(dialog) = &self.settings else {
            return div().into_any_element();
        };
        let manage = &self.manage;
        let installing = manage.installing.is_some();
        let typed = !dialog.link.read(cx).text.trim().is_empty();
        let query = dialog.search.read(cx).text.clone();
        let updatable: Vec<String> = views
            .iter()
            .filter(|view| manage.updates.contains_key(&view.manifest.id))
            .map(|view| view.manifest.id.clone())
            .collect();
        let updating = updatable.iter().any(|id| manage.busy.contains(id));
        let tab = manage.tab;

        let add = small(ui::button(
            "plugin-add",
            "Add plugin",
            Variant::Outline,
            installing,
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(icon(Icon::Plus, 14.0, theme::foreground()))
                .child(match manage.installing {
                    Some(_) => "Installing…",
                    None => "Add plugin",
                }),
        ))
        .aria_expanded(manage.menu == Some(PluginMenu::Add))
        .children(ui::spot(|| "plugin-add".into()))
        .child(track(manage.menu_bounds.clone(), "add".into()))
        .when(!installing, |button| {
            button.on_click(cx.listener(|this, _, _, cx| {
                this.manage.menu = (this.manage.menu != Some(PluginMenu::Add)).then_some(PluginMenu::Add);
                cx.notify_workspace();
            }))
        });
        let actions = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.0))
            .child(
                header_button(
                    "plugin-check-updates",
                    "Check for updates".into(),
                    Icon::RefreshCw,
                    manage.checking,
                )
                .children(ui::spot(|| "plugin-check-updates".into()))
                .when(!manage.checking, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| this.check_plugin_updates(true, cx)))
                }),
            )
            .when(tab != PluginsTab::Browse && !updatable.is_empty(), |row| {
                let ids = updatable.clone();
                row.child(
                    header_button(
                        "plugin-update-all",
                        format!("Update all ({})", updatable.len()).into(),
                        Icon::CircleArrowUp,
                        updating,
                    )
                    .children(ui::spot(|| "plugin-update-all".into()))
                    .when(!updating, |button| {
                        button.on_click(cx.listener(move |this, _, window, cx| {
                            this.update_plugins(ids.clone(), window, cx)
                        }))
                    }),
                )
            })
            .child(add);
        let header = Self::page_header(
            div()
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Plugins"),
            actions,
        );

        let link_card = manage.adding_link.then(|| {
            ui::card([div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .px(px(16.0))
                .py(px(12.0))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(div().flex_1().min_w(px(180.0)).child(ui::input(dialog.link.clone())))
                        .child(
                            ui::button(
                                "plugin-install-link",
                                "Install",
                                Variant::Default,
                                !typed || installing,
                                if manage.installing == Some("link") { "Installing…" } else { "Install" },
                            )
                            .when(typed && !installing, |button| {
                                button.on_click(cx.listener(|this, _, window, cx| this.install_from_link(window, cx)))
                            }),
                        )
                        .child(
                            ui::button("plugin-install-cancel", "Cancel", Variant::Ghost, false, "Cancel")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.manage.adding_link = false;
                                    cx.notify_workspace();
                                })),
                        ),
                )
                .child(muted("Paste a GitHub repo or plugin folder link."))
                .into_any_element()])
        });
        let error = manage.error.clone().map(|error| {
            div().text_size(px(12.0)).text_color(rgb(theme::danger())).child(error)
        });

        let tab_button = |id: &'static str, label: String, to: PluginsTab, cx: &mut Context<Self>| {
            ui::segment(id, label, tab == to).on_click(cx.listener(move |this, _, _, cx| {
                this.manage.tab = to;
                this.manage.menu = None;
                cx.notify_workspace();
            }))
        };
        let updates_label = if updatable.is_empty() {
            "Updates".to_owned()
        } else {
            format!("Updates {}", updatable.len())
        };
        let bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.0))
            .child(ui::segmented([
                tab_button("plugins-tab-installed", "Installed".into(), PluginsTab::Installed, cx),
                tab_button("plugins-tab-browse", "Browse".into(), PluginsTab::Browse, cx),
                tab_button("plugins-tab-updates", updates_label, PluginsTab::Updates, cx),
            ]))
            .child(
                div()
                    .w(px(220.0))
                    .flex_none()
                    .child(ui::search_box(dialog.search.clone(), None)),
            );

        let rows: Vec<AnyElement> = if tab == PluginsTab::Browse {
            browse(&manage.available, &views, &query)
                .into_iter()
                .enumerate()
                .map(|(index, plugin)| self.browse_row(index, plugin, cx))
                .collect()
        } else {
            listed(&views, &manage.updates, tab, &query)
                .into_iter()
                .enumerate()
                .map(|(index, view)| self.plugin_row(index, view, cx))
                .collect()
        };
        let body = if rows.is_empty() {
            let searching = !query.trim().is_empty();
            let words = match (tab, searching) {
                (_, true) => "No plugin matches that search.",
                (PluginsTab::Installed, _) => "No plugins yet. Browse the official ones, or use Add plugin for a GitHub link or a file.",
                (PluginsTab::Browse, _) if manage.checking => "Checking the official list…",
                (PluginsTab::Browse, _) => "Nothing to install from the official list right now. Check for updates to load it again.",
                (PluginsTab::Updates, _) => "Every plugin is up to date.",
            };
            ui::card([muted(words).px(px(16.0)).py(px(16.0)).into_any_element()])
        } else {
            ui::card(rows)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(header)
            .child(muted("Plugins add features to Wings. Each one runs only after you approve what it can do."))
            .children(link_card)
            .children(error)
            .child(bar)
            .child(body)
            .into_any_element()
    }

    /// The name and version of a plugin, its one status word, and its description at a fixed two lines (or what went
    /// wrong, in its place), as a row and a page's header say them.
    fn row_words(
        name: String,
        version: String,
        word: Option<(String, u32)>,
        about: impl IntoElement,
    ) -> gpui::Div {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child(name),
                    )
                    .child(muted(version).text_size(px(11.0)).flex_none())
                    .children(word.map(|(word, tone)| ui::badge(Variant::Secondary, Some(tone), true, word))),
            )
            .child(about)
    }

    /// The fixed-height description box of a row: two lines, cut after them.
    fn clamped(text: String, color: u32) -> gpui::Div {
        div()
            .h(px(32.0))
            .overflow_hidden()
            .line_height(px(16.0))
            .line_clamp(2)
            .text_size(px(12.0))
            .text_color(rgb(color))
            .child(text)
    }

    /// The slot at a row's end: Review while its access waits for you, Update when a newer version is found, else empty.
    /// Always `ACTION_W` wide.
    pub(super) fn action_slot(
        &self,
        scope: &'static str,
        index: usize,
        view: &PluginView,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let id = view.manifest.id.clone();
        let busy = self.manage.busy.contains(&id);
        let slot = div().w(px(ACTION_W)).flex_none().flex().justify_end();
        if !view.approved {
            let review = view.clone();
            return slot.child(
                small(ui::button(
                    control_id(scope, "review", index),
                    format!("Review access for {}", view.manifest.name),
                    Variant::Outline,
                    false,
                    div().text_color(rgb(theme::working())).child("Review"),
                ))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.queue_reviews([review.clone()], cx);
                    cx.notify_workspace();
                })),
            );
        }
        match self.manage.updates.get(&id) {
            Some(version) => slot.child(
                small(ui::button(
                    control_id(scope, "update", index),
                    format!("Update {} to {version}", view.manifest.name),
                    Variant::Secondary,
                    busy,
                    "Update",
                ))
                .when(!busy, |button| {
                    let ids = vec![id.clone()];
                    button.on_click(cx.listener(move |this, _, window, cx| {
                        this.update_plugins(ids.clone(), window, cx)
                    }))
                }),
            ),
            None => slot,
        }
    }

    /// A plugin's switch, which turns it on or off without opening its page.
    pub(super) fn plugin_switch(
        &self,
        scope: &'static str,
        index: usize,
        view: &PluginView,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let on = view.enabled && view.approved;
        let busy = self.manage.busy.contains(&view.manifest.id);
        let toggled = view.clone();
        ui::switch(control_id(scope, "on", index), on)
            .aria_label(format!("{} {}", view.manifest.name, if on { "on" } else { "off" }))
            .when(busy, |switch| switch.opacity(0.5))
            .when(!busy, |switch| {
                switch
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle_plugin(toggled.clone(), !on, window, cx)
                    }))
            })
            .into_any_element()
    }

    /// The ⋯ button at a row's end, which opens the plugin's menu under it.
    pub(super) fn more_button(&self, scope: &'static str, index: usize, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let open = self.manage.menu == Some(PluginMenu::Row(id.to_owned()));
        let which = PluginMenu::Row(id.to_owned());
        ui::icon_button(control_id(scope, "more", index), Icon::Ellipsis, "More actions", ButtonSize::Sm)
            .aria_expanded(open)
            .child(track(self.manage.menu_bounds.clone(), id.to_owned()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.manage.menu = (this.manage.menu.as_ref() != Some(&which)).then(|| which.clone());
                cx.notify_workspace();
            }))
            .into_any_element()
    }

    /// One installed plugin: its icon, name, version and one status word, a two-line description, and at the end the
    /// action slot, the switch and the more menu. Its click opens the plugin's page.
    fn plugin_row(&self, index: usize, view: PluginView, cx: &mut Context<Self>) -> AnyElement {
        let manage = &self.manage;
        let id = view.manifest.id.clone();
        let on = view.enabled && view.approved;
        let runs = self.plugin_runs(&id);
        let (word, tone) = status_word(view.approved, view.enabled, runs, manage.updates.get(&id).map(String::as_str));
        let about = match view.manifest.description.trim() {
            "" => self.plugin_note(&view),
            description => description.to_owned(),
        };
        // What went wrong with its switch shows in place of the description, where the row doesn't change height.
        let about = match manage.row_error.get(&id).cloned() {
            Some(error) => Self::clamped(error, theme::danger())
                .relative()
                .children(ui::spot(move || format!("plugin-row-error-{index}")))
                .into_any_element(),
            None => Self::clamped(about, theme::muted_foreground()).into_any_element(),
        };
        let select = id.clone();
        div()
            .id(("plugin-row", index))
            .role(Role::Button)
            .aria_label(view.manifest.name.clone())
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(16.0))
            .h(px(ROW_H))
            .cursor_pointer()
            .hover(|style| style.bg(rgba(theme::hover())))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.manage.selected = Some(select.clone());
                this.manage.removing = None;
                this.manage.menu = None;
                cx.notify_workspace();
            }))
            .children(ui::spot(move || format!("plugin-row-{index}")))
            .child(plugin_icon(on, view.icon_svg.as_deref(), 36.0))
            .child(Self::row_words(
                view.manifest.name.clone(),
                view.manifest.version.clone(),
                // The switch beside it says on or off.
                (!matches!(word.as_str(), "On" | "Off")).then_some((word, tone)),
                about,
            ))
            .child(self.action_slot("plugin-row", index, &view, cx))
            .child(self.plugin_switch("plugin-row", index, &view, cx))
            .child(self.more_button("plugin-row", index, &id, cx))
            .into_any_element()
    }

    /// One official plugin that isn't installed: the same row, with Install in its action slot.
    fn browse_row(&self, index: usize, plugin: Available, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.manage.installing.is_some();
        let link = plugin.link.clone();
        div()
            .id(("plugin-available", index))
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(16.0))
            .h(px(ROW_H))
            .child(plugin_icon(false, None, 36.0))
            .child(Self::row_words(
                plugin.name.clone(),
                plugin.version,
                None,
                Self::clamped(plugin.description, theme::muted_foreground()),
            ))
            .child(div().w(px(ACTION_W)).flex_none().flex().justify_end().child(
                small(ui::button(
                    ("plugin-available-install", index),
                    format!("Install {}", plugin.name),
                    Variant::Secondary,
                    busy,
                    "Install",
                ))
                .when(!busy, |button| {
                    button.on_click(cx.listener(move |this, _, window, cx| {
                        this.install_available(link.clone(), window, cx)
                    }))
                }),
            ))
            .into_any_element()
    }

    /// The open menu, when one is: Add plugin's, or a plugin's more actions.
    fn plugins_menu(&self, views: &[PluginView], cx: &mut Context<Self>) -> Option<AnyElement> {
        let which = self.manage.menu.clone()?;
        let key = match &which {
            PluginMenu::Add => "add".to_owned(),
            PluginMenu::Row(id) => id.clone(),
        };
        let trigger = self.manage.menu_bounds.borrow().get(&key).copied()?;
        let close = |this: &mut Self, cx: &mut Context<Self>| {
            this.manage.menu = None;
            cx.notify_workspace();
        };
        let menu = match which {
            PluginMenu::Add => ui::menu("plugin-add-menu", "Add plugin")
                .child(
                    ui::menu_item("plugin-add-link", Icon::Globe, "From a GitHub link…", false, false)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.manage.adding_link = true;
                            close(this, cx);
                        })),
                )
                .child(
                    ui::menu_item("plugin-add-file", Icon::FileText, "Choose a file…", false, false)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            close(this, cx);
                            this.install_from_file(window, cx);
                        })),
                ),
            PluginMenu::Row(id) => {
                let view = views.iter().find(|view| view.manifest.id == id)?;
                let github = matches!(view.source, Some(Source::Github { .. })) && !view.dev;
                let busy = self.manage.busy.contains(&id);
                let (details, copy, reinstall, remove) = (id.clone(), id.clone(), id.clone(), id.clone());
                ui::menu("plugin-row-menu", "Plugin actions")
                    .child(
                        ui::menu_item("plugin-menu-details", Icon::FileText, "Details", false, false)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.manage.selected = Some(details.clone());
                                close(this, cx);
                            })),
                    )
                    .child(
                        ui::menu_item("plugin-menu-check", Icon::RefreshCw, "Check for update", false, self.manage.checking)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                close(this, cx);
                                this.check_plugin_updates(true, cx);
                            })),
                    )
                    .child(
                        ui::menu_item("plugin-menu-copy", Icon::Copy, "Copy ID", false, false)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                close(this, cx);
                            })),
                    )
                    .when(github, |menu| {
                        menu.child(
                            ui::menu_item("plugin-menu-reinstall", Icon::RotateCcwClock, "Reinstall", false, busy)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    close(this, cx);
                                    this.update_plugins(vec![reinstall.clone()], window, cx);
                                })),
                        )
                    })
                    .when(!view.dev, |menu| {
                        menu.child(ui::menu_separator()).child(
                            ui::menu_item("plugin-menu-remove", Icon::Trash, "Remove…", true, busy)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    // The page asks once more, with the plugin's name over the button.
                                    this.manage.selected = Some(remove.clone());
                                    this.manage.removing = Some(remove.clone());
                                    close(this, cx);
                                })),
                        )
                    })
            }
        };
        let menu = bind_layer(
            menu,
            cx.listener(move |this, reason: &DismissReason, _, cx| {
                // A press on the button that opened it is its own toggle.
                if let DismissReason::OutsidePress(at) = reason
                    && trigger.contains(at)
                {
                    return;
                }
                this.manage.menu = None;
                cx.notify_workspace();
            }),
        );
        Some(ui::dropdown_menu(trigger, true, menu))
    }

    /// Over everything, where a `.wings-plugin` file can be dropped anywhere on the window, as on the desktop's. It
    /// shows only while one is dragged over the window, and lets every click and hover through.
    pub(super) fn plugin_drop_layer(&self, cx: &mut Context<Self>) -> AnyElement {
        let shown = |paths: &ExternalPaths| super::plugin_manage::carries_package(paths);
        let layer = div()
            .id("plugin-drop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS))
            .border_2()
            .border_color(gpui::transparent_black())
            .drag_over::<ExternalPaths>(move |style, paths, _, _| {
                if shown(paths) {
                    style.bg(rgba(0x0f0e1299)).border_color(rgba(0xe9b65fb3))
                } else {
                    style
                }
            })
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.drop_package(paths, window, cx)
            }))
            .child(
                div()
                    .id("plugin-drop-label")
                    .px(px(14.0))
                    .py(px(8.0))
                    .rounded(px(10.0))
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .bg(rgba(theme::popover()))
                    .text_color(rgb(theme::foreground()))
                    // A drag over only refines what paints, not the text's color, so the label fades in whole.
                    .opacity(0.0)
                    .drag_over::<ExternalPaths>(move |style, paths, _, _| {
                        if shown(paths) {
                            style.opacity(1.0)
                        } else {
                            style
                        }
                    })
                    .child("Drop to install the plugin"),
            );
        // Over the spinners turning over the window's drawing only while something is dragged, when it may tint the
        // window: it is always there, over all of it.
        let layer = if cx.has_active_drag() {
            ui::cover(layer).into_any_element()
        } else {
            layer.into_any_element()
        };
        deferred(layer).with_priority(5).into_any_element()
    }

    /// "Turn on <plugin>?": what the first plugin waiting for review will be able to do, over everything else.
    pub(super) fn plugin_review_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.manage.reviews.first()?;
        let more = self.manage.reviews.len() > 1;
        // Turn on answers only once the review has been up its moment.
        let busy = self.manage.busy.contains(&shown.manifest.id) || !self.manage.review_armed;
        let focus = &self.review_focus;
        let (backdrop, panel) = ui::dialog(
            "plugin-review",
            format!("Turn on {}?", shown.manifest.name),
            focus,
            448.0,
            None,
        );
        let changed = !shown.approved && shown.enabled;
        let description = muted(if changed {
            "This version asks for different access than you approved before."
        } else {
            "Here's what it will be able to do."
        });
        Some(
            backdrop
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.skip_review(cx)),
                )
                .child(ui::dialog_panel(
                    panel
                        .trap_tab(focus)
                        .on_escape(cx.listener(|this, _: &KeyDownEvent, _, cx| this.skip_review(cx)))
                        .child(
                            ui::dialog_body()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.0))
                                        .child(
                                            div()
                                                .flex()
                                                .items_baseline()
                                                .gap(px(12.0))
                                                .child(
                                                    div()
                                                        .text_size(px(16.0))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .child(format!(
                                                            "Turn on {}?",
                                                            shown.manifest.name
                                                        )),
                                                )
                                                .when(more, |title| {
                                                    title.child(muted(format!(
                                                        "1 of {}",
                                                        self.manage.reviews.len()
                                                    )))
                                                }),
                                        )
                                        .child(description),
                                )
                                .child(
                                    div()
                                        .id("plugin-review-access")
                                        .max_h(px(320.0))
                                        .overflow_y_scroll()
                                        .child(access_list(&shown.manifest)),
                                )
                                .child(muted(
                                    "Commands run as you, so only turn on plugins you trust.",
                                ))
                                .child(
                                    ui::dialog_footer()
                                        .child(
                                            ui::button(
                                                "plugin-review-cancel",
                                                if more { "Not now" } else { "Cancel" },
                                                Variant::Ghost,
                                                false,
                                                if more { "Not now" } else { "Cancel" },
                                            )
                                            .on_click(
                                                cx.listener(|this, _, _, cx| this.skip_review(cx)),
                                            ),
                                        )
                                        .child(
                                            ui::button(
                                                "plugin-review-approve",
                                                "Turn on",
                                                Variant::Default,
                                                busy,
                                                "Turn on",
                                            )
                                            .when(
                                                !busy,
                                                |button| {
                                                    button.on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.approve_review(window, cx)
                                                        },
                                                    ))
                                                },
                                            ),
                                        ),
                                ),
                        ),
                ))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_plugin_shows_reads_as_words_and_commands_as_code() {
        assert!(plain_items("Show these in Wings"));
        assert!(!plain_items("Run these commands as you, with your own sign-ins"));
    }

    #[test]
    fn the_review_lists_what_the_manifest_asks_for_in_the_desktops_words() {
        let manifest: Manifest = serde_json::from_value(serde_json::json!({
            "id": "demo", "name": "Demo", "version": "1.0.0", "api": 1, "main": "main.js",
            "permissions": { "exec": ["gh pr view"], "notify": true, "panes": [] },
            "contributes": { "ui": ["badges"], "panels": [{ "id": "p", "title": "P", "icon": "clock", "url": "https://example.com/a?b" }] },
        }))
        .unwrap();
        assert_eq!(
            access_groups(&manifest),
            [
                ("Show these in Wings", vec!["Pane badges".to_owned()]),
                (
                    "Open these sites in a panel from the title bar",
                    vec!["example.com".to_owned()]
                ),
                (
                    "Run these commands as you, with your own sign-ins",
                    vec!["gh pr view".to_owned()]
                ),
                (
                    "Open and close terminals in your projects, and move your typing to any pane",
                    vec![]
                ),
                ("Show desktop notifications, up to 20 a minute", vec![]),
            ]
        );
    }

    #[test]
    fn the_review_warns_that_reading_terminal_output_can_reveal_secrets() {
        let manifest: Manifest = serde_json::from_value(serde_json::json!({
            "id": "demo", "name": "Demo", "version": "1.0.0", "api": 1, "main": "main.js",
            "permissions": { "paneOutput": true },
        }))
        .unwrap();
        assert_eq!(access_groups(&manifest), [(PANE_OUTPUT, vec![])]);
        assert!(PANE_OUTPUT.starts_with("Read what your terminals show") && PANE_OUTPUT.contains("secrets"));
    }

    #[test]
    fn the_review_names_web_pages_and_a_cookie_import_on_lines_of_their_own() {
        let manifest: Manifest = serde_json::from_value(serde_json::json!({
            "id": "browser", "name": "Browser", "version": "1.0.0", "api": 1, "main": "main.js",
            "permissions": { "webView": true, "browserCookies": true, "webScripts": true },
        }))
        .unwrap();
        assert_eq!(access_groups(&manifest), [(WEB_VIEW, vec![]), (BROWSER_COOKIES, vec![]), (WEB_SCRIPTS, vec![])]);
        assert_eq!(
            access_summary(&manifest),
            "Turned on, it can show web pages, import another browser's sign-ins and run scripts in the web pages it shows."
        );
    }

    #[test]
    fn the_review_lists_calls_to_this_computer_apart_from_web_addresses() {
        let manifest: Manifest = serde_json::from_value(serde_json::json!({
            "id": "demo", "name": "Demo", "version": "1.0.0", "api": 1, "main": "main.js",
            "permissions": { "fetch": [
                "https://api.example.com/",
                "http://127.0.0.1:4096/",
                "POST http://localhost:4096/session/*/message",
            ] },
        }))
        .unwrap();
        assert_eq!(
            access_groups(&manifest),
            [
                (
                    "Call these web addresses, with tokens you give it",
                    vec!["https://api.example.com/".to_owned()]
                ),
                (
                    "Call the programs on this computer that listen at these addresses",
                    vec![
                        "http://127.0.0.1:4096/".to_owned(),
                        "POST http://localhost:4096/session/*/message".to_owned()
                    ]
                ),
            ]
        );
    }

    fn view(name: &str, description: &str) -> PluginView {
        let manifest: Manifest = serde_json::from_value(serde_json::json!({
            "id": name.to_lowercase(), "name": name, "version": "1.0.0", "description": description, "api": 1,
            "main": "main.wasm",
        }))
        .unwrap();
        PluginView { manifest, enabled: true, approved: true, source: None, icon_svg: None, dev: false, folder: None }
    }

    #[test]
    fn a_row_says_one_word_and_what_needs_you_comes_first() {
        let word = |approved, enabled, runs, update| status_word(approved, enabled, runs, update).0;
        assert_eq!(word(false, true, true, Some("2.0.0")), "Needs approval");
        assert_eq!(word(true, true, false, Some("2.0.0")), "Couldn't start");
        assert_eq!(word(true, true, true, Some("2.0.0")), "Update 2.0.0");
        assert_eq!(word(true, true, true, None), "On");
        assert_eq!(word(true, false, false, None), "Off");
        assert_eq!(status_word(true, true, true, None).1, theme::done());
        assert_eq!(status_word(false, false, false, None).1, theme::working());
        assert_eq!(status_word(true, true, false, None).1, theme::danger());
    }

    #[test]
    fn the_search_matches_every_word_in_the_name_or_the_description() {
        assert!(matches_query("", "Notes", ""));
        assert!(matches_query("  NOTES ", "Notes", "Keeps notes beside each project."));
        assert!(matches_query("keeps project", "Notes", "Keeps notes beside each project."));
        assert!(!matches_query("keeps calendar", "Notes", "Keeps notes beside each project."));
    }

    #[test]
    fn the_tabs_list_what_they_name_and_browse_leaves_out_what_is_installed() {
        let views = [view("Notes", "Keeps notes."), view("Board", "A board.")];
        let updates = HashMap::from([("board".to_owned(), "2.0.0".to_owned())]);
        let names = |list: Vec<PluginView>| list.into_iter().map(|v| v.manifest.name).collect::<Vec<_>>();
        assert_eq!(names(listed(&views, &updates, PluginsTab::Installed, "")), ["Notes", "Board"]);
        assert_eq!(names(listed(&views, &updates, PluginsTab::Updates, "")), ["Board"]);
        assert_eq!(names(listed(&views, &updates, PluginsTab::Installed, "keeps")), ["Notes"]);
        let entry = |id: &str, description: &str| Available {
            id: id.into(),
            name: id.into(),
            description: description.into(),
            version: "1.0.0".into(),
            link: String::new(),
        };
        let available = [entry("notes", "Notes"), entry("disk", "Disk usage"), entry("hud", "A status line")];
        let ids = |list: Vec<Available>| list.into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(browse(&available, &views, "")), ["disk", "hud"]);
        assert_eq!(ids(browse(&available, &views, "disk")), ["disk"]);
    }

    #[test]
    fn the_byline_names_where_a_plugin_came_from() {
        let mut plugin = view("Notes", "");
        plugin.source = Some(Source::Github { repo: "josippapez/Wings".into(), folder: Some("plugins/notes".into()) });
        assert_eq!(source_label(&plugin), "Wings plugin");
        plugin.source = Some(Source::Github { repo: "owner/repo".into(), folder: None });
        assert_eq!(source_label(&plugin), "GitHub · owner/repo");
        plugin.source = Some(Source::File);
        assert_eq!(source_label(&plugin), "From a file");
        plugin.dev = true;
        assert_eq!(source_label(&plugin), "Wings plugin");
    }

    #[test]
    fn the_page_lists_what_the_manifest_adds_and_what_opens_it() {
        let manifest: Manifest = serde_json::from_value(serde_json::json!({
            "id": "demo", "name": "Demo", "version": "1.0.0", "api": 1, "main": "main.wasm",
            "permissions": { "exec": ["gh pr view"], "notify": true },
            "contributes": {
                "ui": ["badges"],
                "mcpTools": [{ "name": "pr_status" }],
                "sidebars": [{ "id": "side", "title": "Board", "page": "dist/side.html" }],
                "settings": { "page": "dist/settings.html" },
            },
        }))
        .unwrap();
        let adds: Vec<_> = adds_to_wings(&manifest).into_iter().map(|a| (a.kind, a.name, a.open)).collect();
        assert_eq!(
            adds,
            [
                ("Sidebar", "Board".to_owned(), Some(Open::Page("side".into()))),
                ("Pane", "Pane badges".to_owned(), None),
                ("MCP tool", "pr_status".to_owned(), None),
                ("Settings", "Settings page".to_owned(), Some(Open::Settings)),
            ]
        );
        assert_eq!(access_summary(&manifest), "Turned on, it can run commands as you and show notifications.");
    }
}
