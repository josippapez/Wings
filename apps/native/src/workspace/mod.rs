//! The window: terminal tabs whose panes split like the desktop app's, pages of the desktop's plugins as tabs, and the
//! Issues tab, with the desktop app's shortcuts.

mod agent_list;
mod agent_terminals;
#[cfg(all(test, feature = "test-support"))]
mod agent_terminals_tests;
mod claude;
pub use claude::AccountStatus;
mod context_menu;
#[cfg(all(test, feature = "test-support"))]
mod desktop_parity_tests;
#[cfg(all(test, feature = "test-support"))]
mod terminal_background_tests;
mod diff;
mod disk_access;
mod first_launch_plugins;
mod drag_slot;
mod editor;
mod exits;
#[cfg(all(test, feature = "test-support"))]
mod finder_tests;
mod floating;
mod global_hotkey;
mod harnesses;
mod tools;
mod history;
#[cfg(all(test, feature = "test-support"))]
mod hotkey_tests;
mod inbox;
mod journal;
mod links;
#[cfg(all(test, feature = "test-support"))]
mod manage_tests;
mod nav;
#[cfg(all(test, feature = "test-support"))]
mod narrow_tests;
#[cfg(all(test, feature = "test-support"))]
mod nav_tests;
#[cfg(all(test, feature = "test-support"))]
mod panes_look_tests;
#[cfg(unix)]
mod older_servers;
mod opencode;
mod palette;
mod pane_card;
mod panes;
mod parts;
mod persist;
mod plugin_env;
mod plugin_manage;
mod plugin_detail_ui;
mod plugin_manage_ui;
mod plugin_modal;
mod plugin_tab_place;
#[cfg(all(test, feature = "test-support"))]
mod plugin_tests;
#[cfg(all(test, feature = "test-support"))]
mod web_tests;
#[cfg(all(test, feature = "test-support"))]
mod performance_tests;
#[cfg(all(test, feature = "test-support"))]
mod page_keys_tests;
mod plugin_ui;
mod plugin_updates;
mod plugin_windows;
mod plugins;
mod project;
mod project_page;
mod project_sections;
mod quick;
mod rank;
mod right_edge;
mod right_stack;
mod semantic;
mod settings;
mod setup_approval;
mod sidebar;
mod sleep;
mod sound;
mod status;
mod summaries;
mod tab_drag;
mod tabs;
#[cfg(all(test, feature = "test-support"))]
mod tests;
mod title_bar;
mod updater;
mod usage;
mod welcome;
mod worktree_setup;

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use gpui::{
    AnyElement, Bounds, Context, Entity, FocusHandle, Focusable, MouseButton, MouseDownEvent,
    MouseUpEvent, NavigationDirection, Pixels, ScrollHandle, SharedString, StyleRefinement,
    Subscription, Task, Window, canvas, deferred, div, prelude::*, px, rgb, rgba,
};

use crate::ui::behaviors::{EscapeExt, Reveal, RevealExt};
use crate::{
    actions::{
        ActivateTab, ClosePane, CloseTab, CloseWindow, DecreaseFontSize, FocusDown, FocusLeft,
        FocusRight, FocusUp, GoBack, GoForward, IncreaseFontSize, Minimize, MoveTabLeft,
        MoveTabRight, NewTab, NextTab, OpenHistory, OpenProjectInEditor, OpenSettings, PreviousTab,
        ResetFontSize, ShowNotifications, SplitDown, SplitRight, ToggleFullScreen, TogglePalette,
        ToggleQuickTerminal, TogglePerformanceReadout, ToggleSidebar, Zoom, ZoomPane,
    },
    agents::{Agent, PaneInfo},
    host::Config,
    installed::{self, Installed},
    layout::{Dir, Layout, Toward},
    page::{PageView, plugin::Env},
    plugin_view::{IssuesEvent, IssuesView},
    projects::{ProjectEntry, Projects},
    saved,
    terminal_view::{
        CONTRASTS, CURSOR_STYLES, CursorShape, DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE,
        INACTIVE_CURSORS, LINE_HEIGHTS, MAX_FONT_SIZE, MIN_FONT_SIZE, OPTION_AS_META, SCROLLBACKS,
        TerminalFont, TerminalLook, TerminalView, font_size,
    },
    text_field::TextField,
    theme,
};

/// Around panes that don't have focus.
use crate::ui::{self, HAIRLINE};

struct Pane {
    /// A shell that couldn't start keeps its pane, showing why, until it's closed.
    view: Result<Entity<TerminalView>, SharedString>,
    focus: FocusHandle,
    /// Names the output saved for this pane on quit.
    history: String,
    /// A name you gave the pane, shown instead of its program's title, as in the desktop app.
    name: Option<SharedString>,
    /// The Claude Code account its shell runs under when a plugin chose one apart from the project's.
    claude_account: Option<wings_spaces::claude_accounts::AccountPick>,
    /// The agent session it ran, saved so a restore that can't attach to its program resumes it (`sync_sessions`).
    session: Option<saved::SavedSession>,
    /// Its agent was seen this launch, so the agent leaving ends the session. A restored session waits for its agent.
    agent_seen: bool,
    /// When Wings typed the line that brings its session back, until its agent shows.
    resuming: Option<std::time::Instant>,
    /// Wings ended its idle agent to free memory: `session` resumes when the pane is shown (`sleep.rs`).
    asleep: bool,
    /// Its folder is on a drive that isn't mounted yet: it starts once the drive is there (`panes::start_mounted`).
    mount: Option<panes::MountWait>,
    /// The name field while you edit it, in place of the title.
    renaming: Option<(Entity<TextField>, Subscription)>,
    /// Closes the pane when its shell exits.
    _exit: Option<Subscription>,
}

struct TerminalTab {
    id: usize,
    layout: Layout,
    focused: usize,
    /// A pane filling the tab while the others keep running out of sight, as in the desktop app.
    zoomed: Option<usize>,
    focus: FocusHandle,
}

impl TerminalTab {
    /// Takes `pane` out of the tab, and the pane before it takes focus: false when it was the last, and the tab closes.
    fn take_out(&mut self, pane: usize) -> bool {
        if self.zoomed == Some(pane) {
            self.zoomed = None;
        }
        let order = self.layout.panes();
        let at = order.iter().position(|id| *id == pane).unwrap_or(0);
        match std::mem::replace(&mut self.layout, Layout::Pane(pane)).remove(pane) {
            Some(layout) => {
                if self.focused == pane {
                    self.focused = if at == 0 { order[1] } else { order[at - 1] };
                }
                self.layout = layout;
                true
            }
            None => false,
        }
    }
}

/// What a project that isn't shown keeps: its tabs, with their shells and plugin pages still running.
struct Parked {
    terminals: Vec<TerminalTab>,
    plugin_tabs: Vec<plugins::PluginTab>,
    project_page: Option<Entity<project_page::ProjectPage>>,
    issues_open: bool,
    active: Active,
    /// The right sidebar's pages, when the project kept any, for when it comes back.
    right: Option<right_stack::Stack>,
    /// Restored at launch without being shown (`restore_hidden`): what it restores when it's first shown.
    waiting: Option<persist::Waiting>,
}

/// A divider being dragged: which split, and where that split sits.
struct Drag {
    tab: usize,
    path: Vec<bool>,
    dir: Dir,
    bounds: Bounds<Pixels>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Active {
    Terminal(usize),
    /// A page of an installed plugin, by tab id.
    Plugin(usize),
    /// The project page of the shown project.
    Project,
    Issues,
}

pub struct Workspace {
    terminals: Vec<TerminalTab>,
    /// The plugin pages open in the shown project, after the terminals in the tab strip.
    plugin_tabs: Vec<plugins::PluginTab>,
    /// The desktop's plugins that have a native build, each with its main running.
    plugins: Vec<Installed>,
    /// What the plugins ask of the window, and what it tells them.
    plugin_env: Arc<plugin_env::AppEnv>,
    /// The panes, badges, status strips and notices of plugins, as the window shows them.
    plugin_ui: plugin_env::PluginUi,
    /// What plugins list under the projects in the sidebar.
    sections: project_sections::Sections,
    /// Where the project pages are kept, if the desktop's data folder is known.
    project_info: Option<project_page::InfoStore>,
    /// The shown project's page, once opened: a tab after the plugin pages that stays until it is closed.
    project_page: Option<Entity<project_page::ProjectPage>>,
    /// The Issues tab is in the strip. Closing it hides it; opening Issues again brings it back.
    issues_open: bool,
    project_page_focus: FocusHandle,
    /// The badge whose card is open.
    badge_card: Option<plugin_ui::BadgeCard>,
    /// The running plugins that may read Claude's transcripts, for sending them each entry.
    transcript_plugins: Vec<wings_plugins::manifest::Plugin>,
    /// The plugin sidebars shown in the panel on the right, stacked top to bottom.
    right: right_stack::Stack,
    /// The sidebar pages started so far, which keep running while the panel is closed or shows another.
    sidebar_pages: HashMap<(String, String), Entity<PageView>>,
    panes: HashMap<usize, Pane>,
    /// The next id for a terminal tab or a plugin tab: the two never share one.
    next_tab: usize,
    next_pane: usize,
    issues: Entity<IssuesView>,
    /// The cached pieces of the window, from just after the workspace is made (see `parts`).
    parts: Option<parts::Parts>,
    issues_focus: FocusHandle,
    new_focus: FocusHandle,
    /// Keeps the selected tab in the part of the strip that is not covered by controls.
    tab_scroll: ScrollHandle,
    /// A tab being dragged along the strip (`tab_drag`).
    tab_drags: tab_drag::TabDrags,
    /// The inputs that change the tab strip's available width.
    tab_layout: Option<(Pixels, bool, Option<f32>, bool, usize)>,
    active: Active,
    config: Config,
    window_bounds: Option<saved::SavedWindowBounds>,
    /// Where the active tab's panes go, measured every frame.
    area: Option<Bounds<Pixels>>,
    drag: Option<Drag>,
    /// The native app's own projects, which plugins change.
    spaces: Arc<Projects>,
    /// The projects as the sidebar shows them, from `spaces`.
    projects: Vec<ProjectEntry>,
    /// Other projects' tabs, with their shells still running, while another project is shown.
    parked: HashMap<PathBuf, Parked>,
    sidebar: bool,
    /// The window is too narrow for the docked sidebar, the panes' 320 and the right panel's 250: for the layout the
    /// sidebar steps aside, as when collapsed, and its peek still shows it. Never saved: `sidebar` is the user's choice.
    aside: bool,
    /// The sidebar's width, order, git counts and peek.
    side: sidebar::State,
    /// Layers and popovers that closed, drawn on their way out.
    exits: exits::Exits,
    /// What each pane's agent is doing, from the last detection, by pane.
    agents: HashMap<usize, Agent>,
    /// The same for the sidebar's one-off terminals, by their key.
    loose_agents: HashMap<usize, Agent>,
    /// The account each pane's Claude runs under, by name, when it isn't the default one.
    agent_accounts: HashMap<usize, SharedString>,
    /// What each pane runs now, from the last detection.
    pane_info: HashMap<usize, PaneInfo>,
    /// The terminals agents opened with `open_pane`, and which float over their agent.
    agent_terminals: agent_terminals::AgentTerminals,
    window_active: bool,
    settings: Option<settings::SettingsDialog>,
    updates: updater::Updates,
    /// What the app keeps besides how terminals look: the quick terminal key, the editor, what new tabs open.
    prefs: settings::Prefs,
    /// The editors found on this computer, looked up once.
    editors: Vec<wings_editors::EditorView>,
    /// The project bar's menu of the other editors is open.
    editor_menu: bool,
    /// The menu beside the new tab button is open.
    new_tab_menu: bool,
    /// Back and forward between the places shown.
    nav: nav::Nav,
    /// The sidebar's terminals, which belong to no project, in the order they were opened.
    loose: Vec<quick::QuickTerminal>,
    /// The one Ctrl-` shows and hides, the desktop's `loose.last`.
    loose_last: Option<usize>,
    next_loose: usize,
    /// The last project was removed: the window shows what to do first, with no tabs.
    welcome: bool,
    /// The project being entered was just added, so its first tab is what Settings, General says.
    just_added: bool,
    /// The command palette and the history sheet, one at a time, and the past sessions both search.
    palette: Option<palette::Palette>,
    sheet: Option<history::Sheet>,
    sessions: history::Sessions,
    /// Makes session names and agent summaries, when the app has the desktop's `ai/` folder.
    ai: Option<Arc<wings_agents::ai::Ai>>,
    ai_view: summaries::View,
    /// Semantic search of the past sessions: its indexer, and its progress the sidebar shows.
    search: semantic::Search,
    /// What agents and plugins said, for the sidebar's Inbox.
    inbox: inbox::Inbox,
    /// Claude Code's connection, as Settings, Agents shows it.
    claude: claude::Connection,
    hotkey: global_hotkey::HotkeyState,
    opencode: opencode::Link,
    /// The agent CLIs Settings, Agents lists.
    harnesses: harnesses::Checks,
    /// The command-line tools Settings, Tools lists.
    tools: tools::Tools,
    /// The right-click menu that is open.
    menu: Option<context_menu::Menu>,
    /// What the Plugins section is doing: installs, reviews waiting, updates found.
    manage: plugin_manage::Manage,
    /// The plugin review and the setup approval, which show one at a time over everything.
    review_focus: FocusHandle,
    /// What had the keyboard before the review or the approval took it.
    review_recovery: crate::ui::behaviors::FocusRecovery,
    /// Setup commands agents asked to set, waiting for you.
    setup_asks: setup_approval::SetupAsks,
    worktree_setup: Option<worktree_setup::WorktreeSetupDialog>,
    /// A check for drives mounting runs while a pane waits for one.
    watching_mounts: bool,
    /// Whether the terminals can read other apps' data, and the dialog that asks for it.
    disk_access: disk_access::DiskAccess,
    first_launch: first_launch_plugins::FirstLaunch,
    /// Settings' list of terminal servers, its Stop dialog, and the panes moving off an older server.
    #[cfg(unix)]
    older: older_servers::OlderServers,
    _detection: Option<Task<()>>,
    /// Keeps the `settings.json` of the default Claude account and of the accounts that share its setup the same as they
    /// change (`claude_accounts::SettingsWatcher`), on a thread of its own.
    _settings_sync: Option<wings_spaces::claude_accounts::SettingsWatcher>,
    /// How long each idle agent out of sight has been so, and the timer for the next one due (`sleep.rs`).
    sleep: sleep::Sleep,
    _plugin_events: Task<()>,
    _activation: Subscription,
    _visibility: Subscription,
    _quit: Subscription,
    /// Hands the keys a plugin page takes for itself to it before Wings' own actions (`page_claims`).
    _page_claims: Subscription,
}

/// A page tab's body, as the desktop's `PluginTab` and `ProjectPageTab`: a card like a pane's, in the same `p-2 pt-0`.
/// The project bar draws no line over it (`project_bar`), which ran past the card's corner.
fn page_card(page: impl IntoElement) -> AnyElement {
    page_card_over(page, None)
}

/// `page_card`, with the slot of the backdrop the page asks for (`ui::backdrop_slot`) under the page in place of the
/// card's ground.
fn page_card_over(page: impl IntoElement, backdrop: Option<AnyElement>) -> AnyElement {
    div()
        .size_full()
        .flex()
        .p(px(8.0))
        .pt_0()
        .child(
            div()
                .relative()
                .children(ui::spot(|| "page-card".into()))
                .flex_1()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .rounded(px(14.0))
                .border_1()
                .border_color(rgba(HAIRLINE))
                .when(backdrop.is_none(), |card| card.bg(rgb(theme::surface())))
                .children(backdrop)
                .child(page),
        )
        .into_any_element()
}

fn shortcut_label(key: &str, extra: bool) -> String {
    if cfg!(target_os = "macos") {
        format!("{}⌘{key}", if extra { "⇧" } else { "" })
    } else {
        format!("Ctrl+Shift{}+{key}", if extra { "+Alt" } else { "" })
    }
}

impl Workspace {
    pub fn new(config: Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let window_handle = window.window_handle();
        // Registered before any terminal's, so it runs first and reads each shell's folder while it's alive.
        let quit = cx.on_app_quit(move |this, cx| {
            // A chat's Claude Code in the terminal server runs on for the next app, as the server's panes do.
            for installed in &this.plugins {
                installed.host.app_quits();
            }
            if let Ok(saved) = window_handle.update(cx, |_, window, app| {
                crate::window_state::save(window.window_bounds(), window.display(app).as_deref())
            }) {
                this.window_bounds = Some(saved);
            }
            this.save(cx);
            this.save_histories(cx);
            // Nothing saves the sidebar's terminals, so their shells end rather than run on in the terminal server.
            for quick in &this.loose {
                quick.view().read(cx).end();
            }
            async {}
        });
        let this = cx.weak_entity();
        let page_claims = cx.intercept_keystrokes(move |event, window, cx| {
            let keystroke = event.keystroke.clone();
            if this.update(cx, |this, cx| this.page_claims(&keystroke, window, cx)).unwrap_or(false) {
                cx.stop_propagation();
            }
        });
        // Closing the window quits, as Cmd+Q does, with the window still open: closed first, it would drop its
        // terminals, which ends their programs, before the quit that leaves the terminal server's panes running.
        window.on_window_should_close(cx, |_, cx| {
            cx.quit();
            false
        });
        let issues = cx.new(IssuesView::new);
        cx.subscribe_in(&issues, window, |this, _, event: &IssuesEvent, window, cx| match event {
            IssuesEvent::OpenPlugins => {
                this.open_settings(window, cx);
                if let Some(dialog) = &mut this.settings {
                    dialog.section = settings::Section::Plugins;
                }
                cx.notify_workspace();
            }
        })
        .detach();
        let (plugin_events, requested) = async_channel::unbounded();
        let spaces = Arc::new(Projects::open(
            config.projects.clone(),
            config.project.clone(),
        ));
        let ai = config
            .ai
            .clone()
            .map(|dir| Arc::new(wings_agents::ai::Ai::new(dir)));
        let sessions = history::Sessions::new(config.sessions.clone(), ai.clone());
        let plugin_env = Arc::new(plugin_env::AppEnv::new(
            plugin_events,
            config.agents.clone().unwrap_or_default(),
            spaces.clone(),
            config.plugins.as_ref().map(installed::load_store),
            config.socket.clone(),
            config.state.clone(),
            sessions.clone(),
        ));
        #[cfg(unix)]
        plugin_env.use_servers(config.terminal_server.clone());
        if let Some(ai) = &ai {
            summaries::start(ai.clone(), sessions.clone());
        }
        let mut this = Self {
            terminals: Vec::new(),
            plugin_tabs: Vec::new(),
            plugins: Vec::new(),
            plugin_env,
            plugin_ui: Default::default(),
            sections: Default::default(),
            project_info: config
                .project_info
                .as_deref()
                .map(project_page::InfoStore::open),
            project_page: None,
            project_page_focus: cx.focus_handle().tab_stop(true),
            badge_card: None,
            transcript_plugins: Vec::new(),
            right: right_stack::Stack::default(),
            sidebar_pages: HashMap::new(),
            panes: HashMap::new(),
            next_tab: 0,
            next_pane: 0,
            issues,
            parts: None,
            issues_focus: cx.focus_handle().tab_stop(true),
            new_focus: cx.focus_handle().tab_stop(true),
            tab_scroll: ScrollHandle::new(),
            tab_drags: Default::default(),
            tab_layout: None,
            aside: false,
            active: Active::Issues,
            issues_open: true,
            config,
            window_bounds: None,
            area: None,
            drag: None,
            projects: spaces.entries(),
            spaces,
            parked: HashMap::new(),
            sidebar: true,
            side: Default::default(),
            exits: Default::default(),
            agents: HashMap::new(),
            loose_agents: HashMap::new(),
            agent_accounts: HashMap::new(),
            pane_info: HashMap::new(),
            agent_terminals: Default::default(),
            window_active: window.is_window_active(),
            settings: None,
            updates: updater::Updates::new(cx),
            prefs: settings::Prefs::default(),
            editors: Vec::new(),
            editor_menu: false,
            new_tab_menu: false,
            nav: Default::default(),
            loose: Vec::new(),
            loose_last: None,
            next_loose: 0,
            welcome: false,
            just_added: false,
            palette: None,
            sheet: None,
            sessions,
            ai,
            ai_view: Default::default(),
            search: Default::default(),
            inbox: Default::default(),
            menu: None,
            claude: Default::default(),
            hotkey: Default::default(),
            opencode: Default::default(),
            harnesses: Default::default(),
            tools: Default::default(),
            manage: Default::default(),
            review_focus: cx.focus_handle(),
            review_recovery: Default::default(),
            setup_asks: Default::default(),
            worktree_setup: None,
            watching_mounts: false,
            disk_access: Default::default(),
            first_launch: Default::default(),
            #[cfg(unix)]
            older: Default::default(),
            _detection: None,
            _settings_sync: None,
            sleep: Default::default(),
            _plugin_events: cx.spawn_in(window, async move |this, cx| {
                while let Ok(event) = requested.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            this.apply_plugin_event(event, window, cx)
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }),
            _visibility: cx.observe_window_visibility(window, |this, visibility, _, cx| {
                let terminals = this
                    .panes
                    .values()
                    .filter_map(|pane| pane.view.as_ref().ok().cloned())
                    .chain(this.loose.iter().map(|quick| quick.view().clone()))
                    .collect::<Vec<_>>();
                for terminal in terminals {
                    terminal.update(cx, |view, cx| {
                        view.set_window_visible(visibility.is_visible(), cx)
                    });
                }
            }),
            _activation: cx.observe_window_activation(window, |this, window, cx| {
                this.window_active = window.is_window_active();
                // Reduce motion turned on or off in System Settings takes effect when you come back.
                crate::ui::motion::follow_reduce_motion(cx);
                for installed in &this.plugins {
                    installed.host.publish_focus(this.window_active);
                }
                // Back from System Settings, where you may have just given the terminals Full Disk Access.
                if this.window_active && this.disk_access.granting {
                    this.check_disk_access(cx);
                }
                if this.window_active && this.manage.check_due {
                    this.manage.check_due = false;
                    this.check_plugin_updates(false, cx);
                }
            }),
            _quit: quit,
            _page_claims: page_claims,
        };
        let env: Arc<dyn Env> = this.plugin_env.clone();
        this.plugins = match (&this.config.plugins, &this.plugin_env.store) {
            (Some(paths), Some(store)) => {
                let retired = store.lock().unwrap().remove_retired();
                for result in retired {
                    match result {
                        Ok((id, Some(old))) => this.forget_plugin_data(&id, &old),
                        Ok(_) => {}
                        Err(error) => eprintln!("Couldn't remove a retired plugin: {error}"),
                    }
                }
                if let Err(error) = store.lock().unwrap().adopt_moved_in() {
                    eprintln!("Couldn't point a plugin at its new place in the Wings repo: {error}");
                }
                let found = installed::discover_in(&store.lock().unwrap());
                installed::start(found, paths, &env)
            }
            _ => Vec::new(),
        };
        this.plugin_env.set_plugins(&this.plugins);
        // Nothing has drawn yet, so nothing draws again: a second draw as the window opened took the keyboard from the
        // restored tab's pane to the last pane drawn.
        this.set_page_links(cx);
        // Claude Code reaches this app through the socket, as it reaches the desktop's.
        if let Some(socket) = this.config.socket.clone() {
            let info = this.project_info.as_ref().map(|info| info.store.clone());
            let sessions = this.sessions.clone();
            if let Err(error) = claude::start(&socket, this.plugin_env.clone(), sessions, info) {
                eprintln!("Couldn't listen on {}: {error}", socket.display());
            }
            claude::connect_on_start(
                socket.clone(),
                this.config.state.clone(),
                this.spaces.spaces(),
            );
            opencode::connect_on_start(socket, this.config.state.clone());
            journal::sync_on_start(this.config.state.clone());
            this.start_usage(wings_connect::usage::Usage::claude(), cx);
        }
        this.start_semantic(cx);
        this.transcript_plugins = this
            .plugins
            .iter()
            .filter(|installed| !installed.plugin.manifest.permissions.transcript.is_empty())
            .map(|installed| installed.plugin.clone())
            .collect();
        this._detection = this
            .config
            .agents
            .clone()
            .map(|dir| Self::detect(dir, !this.transcript_plugins.is_empty(), cx));
        this._settings_sync = this.config.agents.clone().map(|dir| {
            wings_spaces::claude_accounts::SettingsWatcher::start(dir, |error| {
                eprintln!("[claude] {error}")
            })
        });
        let settings = this
            .config
            .state
            .as_deref()
            .map(saved::load_settings)
            .unwrap_or_default();
        this.window_bounds = settings.window_bounds.clone();
        if let Some(size) = settings.font_size {
            cx.set_global(TerminalFont(size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)));
        }
        this.sidebar = settings.sidebar_open.unwrap_or(true);
        this.start_sidebar(&settings, window, cx);
        this.plugin_ui.right_size = right_edge::Size::load(&settings);
        this.plugin_ui.diff_dock = diff::Dock::load(&settings);
        // The desktop showed the projects in its own saved order, not the list's. Once, the list takes that order.
        if settings.desktop_order.is_none()
            && let Some(state) = this.config.state.as_deref()
            && let Some(desktop) =
                crate::data_dir::desktop::read(&state.with_file_name("workspace.json"))
            && !desktop.space_order.is_empty()
        {
            this.spaces.sort_by_order(&desktop.space_order);
            this.projects_changed(cx);
        }
        // A replacement that moved in before pins were read takes them from the desktop's file beside its own.
        this.plugin_ui.pinned = settings
            .pinned_buttons
            .clone()
            .or_else(|| {
                let state = this.config.state.as_deref()?;
                crate::data_dir::desktop::read(&state.with_file_name("workspace.json"))?
                    .pinned_buttons
            })
            .unwrap_or_default();
        let installed = |family: &String| cx.text_system().all_font_names().contains(family);
        let defaults = TerminalLook::default();
        // Only the desktop's choices, so a hand-edited file can't ask for a million lines.
        let offered = |value: Option<f32>, choices: &[f32], default: f32| {
            value
                .filter(|value| choices.contains(value))
                .unwrap_or(default)
        };
        cx.set_global(TerminalLook {
            family: settings
                .font_family
                .clone()
                .filter(installed)
                .map_or(DEFAULT_FONT_FAMILY.into(), Into::into),
            shift_return_meta: settings.shift_return_meta.unwrap_or(true),
            option_as_meta: OPTION_AS_META
                .iter()
                .find(|(_, name, _)| settings.option_as_meta.as_deref() == Some(*name))
                .map_or(defaults.option_as_meta, |(choice, ..)| *choice),
            cursor: CURSOR_STYLES
                .iter()
                .find(|(_, name, _)| settings.cursor_style.as_deref() == Some(*name))
                .map_or(CursorShape::Block, |(shape, ..)| *shape),
            cursor_blink: settings.cursor_blink.unwrap_or(defaults.cursor_blink),
            inactive_cursor: INACTIVE_CURSORS
                .iter()
                .find(|(_, name, _)| settings.cursor_inactive_style.as_deref() == Some(*name))
                .map_or(defaults.inactive_cursor, |(style, ..)| *style),
            line_height: offered(settings.line_height, &LINE_HEIGHTS, defaults.line_height),
            min_contrast: offered(
                settings.minimum_contrast_ratio,
                &CONTRASTS.map(|(ratio, _)| ratio),
                defaults.min_contrast,
            ),
            scrollback: settings
                .scrollback
                .filter(|lines| SCROLLBACKS.contains(lines))
                .unwrap_or(defaults.scrollback),
            copy_on_select: settings.copy_on_select.unwrap_or(defaults.copy_on_select),
            clipboard_write: settings.clipboard_write.unwrap_or(defaults.clipboard_write),
            images: settings.images.unwrap_or(defaults.images),
            background: settings.terminal_background.as_deref().and_then(crate::terminal_view::parse_hex),
        });
        this.tools.read = settings.tools_read.clone();
        let prefs = settings::Prefs::default();
        this.prefs = settings::Prefs {
            quick_terminal: settings.quick_terminal.unwrap_or(prefs.quick_terminal),
            editor: settings.editor.clone(),
            new_tab: settings.new_tab.clone().unwrap_or(prefs.new_tab),
            new_project_page: settings.new_project_opens.as_deref() == Some("project"),
            // Off unless turned on, as the desktop's `saved.agentsRunCommands === true`.
            agents_run_commands: settings.agents_run_commands == Some(true),
            agents_message: settings.agents_message == Some(true),
            agent_terminals_open: agent_terminals::Opens::parse(
                settings.agent_terminals_open.as_deref(),
            ),
            notify_sound: sound::SoundSettings::from_saved(settings.notify_sound.as_ref()),
            sleep: sleep::SleepSettings::from_saved(settings.sleep.as_ref()),
            seen_plugins: settings.seen_plugins,
            diff_wrap: settings.diff_wrap == Some(true),
            diff_tree: settings.diff_tree == Some(true),
            diff_dock_files: settings.diff_dock_files != Some(false),
            global_hotkey: settings.global_hotkey.clone().unwrap_or_default(),
            terminal_server: settings.terminal_server,
            app_data_asked: settings.app_data_asked == Some(true),
            official_plugins_asked: settings.official_plugins_asked == Some(true),
            theme: theme::Theme::from_key(settings.theme.as_deref()),
            transparent: settings.transparent != Some(false),
        };
        theme::set(this.prefs.theme, this.prefs.transparent);
        this.find_editors(cx);
        this.start_global_hotkey(window, cx);
        this.listen_for_notification_clicks(window, cx);
        this.enter_project(window, cx);
        #[cfg(unix)]
        this.restore_hidden(cx);
        this.check_disk_access(cx);
        this.start_update_checks(window, cx);
        this.start_plugin_update_checks(window, cx);
        this.start_tool_checks(window, cx);
        this.offer_official_plugins(cx);
        // The cached pieces need the workspace's entity, which exists once this returns (see `parts`).
        let workspace = cx.weak_entity();
        cx.defer(move |cx| {
            if let Some(workspace) = workspace.upgrade() {
                let parts = parts::Parts::new(&workspace, cx);
                workspace.update(cx, |this, _| this.parts = Some(parts));
            }
        });
        this
    }

    /// The page of the plugin tab whose tab itself has the keyboard, as it does once the tab opens. The page isn't on
    /// the keyboard's way then, so its shortcuts and font keys come here; with the keyboard in the page it takes them.
    fn page_on_tab(&self, window: &Window) -> Option<Entity<PageView>> {
        let Active::Plugin(id) = self.active else {
            return None;
        };
        let tab = self.plugin_tabs.iter().find(|tab| tab.id == id)?;
        tab.focus.is_focused(window).then(|| tab.page.clone())
    }

    /// Gives the key to the page of the plugin tab in front when it takes that key for itself (`Page::claims`) and has the
    /// keyboard, in its fields or on its tab: before Wings' own actions, so a Browser's Cmd-T opens a browser tab and not
    /// a terminal. Says whether the page took it.
    fn page_claims(&self, keystroke: &gpui::Keystroke, window: &Window, cx: &gpui::App) -> bool {
        // A key from the page's own web page is the page's to take even when Wings' focus was left elsewhere, as on a
        // browser tab since closed.
        let from_page = crate::web::take_key_from();
        let Active::Plugin(id) = self.active else {
            return false;
        };
        let Some(tab) = self.plugin_tabs.iter().find(|tab| tab.id == id) else {
            return false;
        };
        let page = tab.page.read(cx);
        (page.has_keyboard(window, cx) || from_page == Some(tab.page.entity_id())) && page.claim_shortcut(keystroke)
    }

    fn page_scales_text(&self, step: &str, window: &Window, cx: &gpui::App) -> bool {
        self.page_on_tab(window).is_some_and(|page| page.read(cx).scale_text(step))
    }

    /// Every terminal picks the new size up on its next frame and refits its shell.
    fn set_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        cx.set_global(TerminalFont(size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)));
        self.save(cx);
        cx.refresh_windows();
    }
}

impl Workspace {
    /// The title bar with the tabs, drawn as a piece of its own (`parts`).
    fn title_bar_part(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mut tabs: Vec<(Active, AnyElement)> = self
            .terminals
            .iter()
            .map(|tab| {
                (
                    Active::Terminal(tab.id),
                    self.tab(
                        Active::Terminal(tab.id),
                        self.label(tab.id, cx),
                        self.tab_state(tab.id),
                        &tab.focus,
                        cx,
                    )
                    .into_any_element(),
                )
            })
            .collect();
        tabs.extend(self.plugin_tabs.iter().map(|tab| {
            (
                Active::Plugin(tab.id),
                self.tab(
                    Active::Plugin(tab.id),
                    tab.label(),
                    None,
                    &tab.focus,
                    cx,
                )
                .into_any_element(),
            )
        }));
        if self.project_page.is_some() {
            tabs.push((
                Active::Project,
                self.tab(
                    Active::Project,
                    "Project page".into(),
                    None,
                    &self.project_page_focus,
                    cx,
                )
                .into_any_element(),
            ));
        }
        // With the Issues plugin's own board, that is the Issues tab, among the plugin pages.
        if self.issues_open && !self.has_tasks_board() {
            tabs.push((
                Active::Issues,
                self.tab(
                    Active::Issues,
                    "Issues".into(),
                    None,
                    &self.issues_focus,
                    cx,
                )
                .into_any_element(),
            ));
        }
        if self.welcome {
            tabs.clear();
        }
        let tabs = self.tab_strip(tabs);
        self.title_bar(tabs, cx)
    }

    /// The project bar and the active tab's content under it, drawn as a piece of its own (`parts`).
    fn main_part(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let viewport = window.viewport_size().width;
        let main_width = self.main_width(viewport);
        // The panes' share: the main column less the diff docked beside the tab.
        let docked = match self.diff_shown() {
            Some(diff::Shown::Docked) => self.diff_width(viewport),
            _ => 0.0,
        };
        let tab_width = (main_width - docked).max(0.0);
        let body: AnyElement = if self.welcome {
            self.welcome(cx)
        } else {
            match self.active {
                Active::Terminal(id) => match self.terminals.iter().find(|tab| tab.id == id) {
                    Some(tab) => {
                        let entity = cx.entity();
                        let measure = canvas(
                            move |bounds, _, cx| {
                                entity.update(cx, |this, _| this.area = Some(bounds))
                            },
                            {
                                let entity = cx.entity();
                                move |_, _, window, _| {
                                    ui::window_drag(
                                        window,
                                        &entity,
                                        |this, event, _, cx| {
                                            if event.dragging() {
                                                this.drag_to(event.position, cx)
                                            }
                                        },
                                        |this, _, cx| {
                                            if this.drag.take().is_some() {
                                                this.save(cx);
                                            }
                                        },
                                    );
                                }
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full();
                        let layout = match tab.zoomed {
                            Some(pane) => self.pane_card(pane, true, true, tab_width, cx),
                            None => self.layout_element(
                                tab.id,
                                &tab.layout,
                                &mut Vec::new(),
                                tab.focused,
                                &self.floating_in(&tab.layout),
                                tab_width,
                                cx,
                            ),
                        };
                        // The panes fill the area, flush with the project bar above and sharing their lines.
                        div()
                            .size_full()
                            .child(div().relative().size_full().child(layout).child(measure))
                            .into_any_element()
                    }
                    None => div().into_any_element(),
                },
                Active::Plugin(id) => match self.plugin_tabs.iter().find(|tab| tab.id == id) {
                    // Kept as last drawn until the page changes, so a busy terminal doesn't rebuild it every frame. Over the
                    // backdrop the page asks for once the window has the layer that draws it (`parts`), at the card's
                    // radius less its border.
                    Some(tab) => {
                        let backdrop = tab.page.read(cx).backdrop().filter(|_| self.parts.is_some());
                        // On screen unless a terminal fills the area in its place: its spinners turn in the window's
                        // layer for them then (`render`), and a step of them no longer draws the whole page again.
                        if self.loose_page().is_none() {
                            ui::place_view(tab.page.entity_id(), window.window_handle().window_id());
                        }
                        page_card_over(
                            tab.page
                                .clone()
                                .cached(StyleRefinement::default().size_full()),
                            backdrop.map(|(kind, palette)| {
                                ui::backdrop_slot(format!("tab-{}", tab.id), kind, palette, 13.0).into_any_element()
                            }),
                        )
                    }
                    None => div().into_any_element(),
                },
                Active::Project => match &self.project_page {
                    Some(page) => page_card(page.clone()),
                    None => div().into_any_element(),
                },
                Active::Issues => page_card(self.issues.clone()),
            }
        };
        let dock = self.diff_dock(window.viewport_size().width, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .when(!self.welcome, |column| column.child(self.project_bar(main_width, cx)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            // The terminal filling the area stands in for the tab, so the tab is out of sight, not just
                            // under it: laid out at its size, but not painted or clickable.
                            .child(
                                div()
                                    .size_full()
                                    .when(self.loose_page().is_some(), |tab| tab.invisible())
                                    .child(body),
                            )
                            // The terminals floating over the tab draw at this slot in the window's quick layer, over
                            // the panes' terminals (`render`).
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .size_full()
                                    .child(ui::slot(QUICK, QUICK, None)),
                            ),
                    )
                    // Beside the tab, as the desktop's `diff` panel in the same group as its tabs.
                    .children(dock),
            )
            .into_any_element()
    }

    /// The terminals of the tab on screen, each at the slot its pane's card leaves in the main column
    /// (`pane_card::TERMINALS`). Each is drawn again only when it changes, which no longer draws the main column again.
    /// None while a terminal fills the area in place of the tab, as the tab isn't painted then.
    fn tab_terminals(&self, cx: &gpui::App) -> Vec<(SharedString, AnyElement)> {
        let Active::Terminal(id) = self.active else {
            return Vec::new();
        };
        let Some(tab) = self.terminals.iter().find(|tab| tab.id == id) else {
            return Vec::new();
        };
        if self.welcome || self.loose_page().is_some() {
            return Vec::new();
        }
        self.card_panes(tab)
            .into_iter()
            .filter_map(|pane| {
                let view = self.panes.get(&pane)?.view.as_ref().ok()?;
                // Named after its pane, as the card around it is: drawn in its own layer, the terminal is no longer
                // inside the card's group, and a screen reader would lose which pane it is.
                let terminal = div()
                    .id(("terminal-pane", pane))
                    .size_full()
                    .role(gpui::Role::Group)
                    .aria_label(self.pane_title(pane, cx))
                    .child(view.clone().cached(StyleRefinement::default().size_full()))
                    .into_any_element();
                Some((pane_card::terminal_slot(pane), terminal))
            })
            .collect()
    }
}

/// The layer of the window that draws the terminals floating over the tab, and its one slot.
const QUICK: &str = "quick";

impl Workspace {
    /// Turns the performance readout (`ui::readout`) on or off. The workspace draws again only for this, to put the card
    /// in the window or take it out; its once-a-second update draws the card alone.
    pub(super) fn toggle_readout(&mut self, cx: &mut Context<Self>) {
        if let Some(parts) = &self.parts {
            parts.readout.update(cx, |readout, cx| readout.set_on(!readout.is_on(), cx));
            cx.notify_workspace();
        }
    }

    /// What counts the window's draws while the readout is on or a measurement runs, and the readout's card, kept as last
    /// drawn, while it is on; nothing otherwise.
    fn readout_layer(&self, cx: &gpui::App) -> Vec<gpui::AnyElement> {
        let Some(parts) = self.parts.as_ref().filter(|parts| parts.readout.read(cx).counting()) else {
            return Vec::new();
        };
        let mut layer = vec![ui::readout::frame_counter().into_any_element()];
        if parts.readout.read(cx).is_on() {
            layer.push(parts.readout.clone().cached(ui::readout::placement()).into_any_element());
        }
        layer
    }

    /// The `performance` tool's three steps on the window's thread (`claude::measure_performance`): count, start the
    /// window once the counter is in, and read the counts and put the readout as it was. Each draws the workspace once, to
    /// put the frame counter in or take it out, outside the measured window.
    pub(super) fn measure_begin(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let parts = self.parts.as_ref().ok_or("The window isn't ready yet")?;
        parts.readout.update(cx, |readout, _| readout.begin_measure())?;
        cx.notify_workspace();
        Ok(())
    }

    pub(super) fn measure_restart(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let parts = self.parts.as_ref().ok_or("The window isn't ready yet")?;
        parts.readout.update(cx, |readout, _| readout.restart_measure())
    }

    pub(super) fn measure_end(&mut self, cx: &mut Context<Self>) -> Result<ui::readout::Measured, String> {
        let parts = self.parts.as_ref().ok_or("The window isn't ready yet")?;
        let measured = parts.readout.update(cx, |readout, _| readout.end_measure());
        cx.notify_workspace();
        measured
    }
}

/// `cx.notify_workspace()` for the workspace, which redraws all its pieces. It tells the performance measurement where it was
/// called from (`causes` of `wings performance`), and `src/workspace` is checked for a plain `cx.notify_workspace()` on the
/// workspace (`ui/live/tests.rs`).
pub(crate) trait NotifyWorkspace {
    fn notify_workspace(&mut self);
}

impl NotifyWorkspace for Context<'_, Workspace> {
    #[track_caller]
    fn notify_workspace(&mut self) {
        ui::readout::notified(std::panic::Location::caller());
        self.notify();
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::readout::render("workspace");
        self.sync_plugins(cx);
        self.exits.popovers.render();
        // Whatever showed another project or tab, it arrives here as the place on screen, as the desktop's history.
        let place = self.place();
        self.nav.visit(place);
        self.close_diff_without_tab();
        self.sync_aside(window.viewport_size().width);
        self.sync_diff_room(window.viewport_size().width, cx);
        self.sync_diff_width(window.viewport_size().width, cx);
        if self.diff_shown() == Some(diff::Shown::Full)
            && let Some((viewer, _)) = &self.plugin_ui.diff
        {
            // No spinner layers in this draw: the spinners turning over the window's drawing go too. A draw of the
            // window still, for the overlays' places (`ui::cover`).
            ui::start_draw(window.window_handle().window_id());
            return div()
                .size_full()
                .child(viewer.clone())
                .child(ui::spinner_sync(window))
                .child(crate::web::sync(window))
                .into_any_element();
        }
        // Before anything in this draw puts a page on screen (`ui::place_view`): the main column's tab page and the right
        // panel's pages turn their spinners in the window's layer for them only when placed in this draw.
        ui::start_draw(window.window_handle().window_id());
        // A click in a terminal focuses it directly, so the tab learns which pane has focus here.
        if let Active::Terminal(id) = self.active
            && let Some(index) = self.terminals.iter().position(|tab| tab.id == id)
        {
            let clicked = self.terminals[index]
                .layout
                .panes()
                .into_iter()
                .find(|pane| {
                    self.panes.get(pane).is_some_and(|p| match &p.view {
                        Ok(view) => view.focus_handle(cx).contains_focused(window, cx),
                        Err(_) => p.focus.is_focused(window),
                    })
                });
            if let Some(pane) = clicked {
                self.terminals[index].focused = pane;
            }
        }
        let tab_layout = (
            window.viewport_size().width,
            self.docked(),
            self.docked().then_some(self.side.width),
            !self.right.is_empty(),
            self.plugins
                .iter()
                .map(|plugin| plugin.sidebars.len())
                .sum(),
        );
        if self.tab_layout != Some(tab_layout) {
            if let Some(index) = self.tabs().iter().position(|tab| *tab == self.active) {
                self.tab_scroll.reveal(index, Reveal::Nearest);
            }
            self.tab_layout = Some(tab_layout);
        }
        // Each kept as last drawn until it, or the workspace, changes: a terminal's output redraws the main column, and a
        // spinner step only the layer that turns the spinners, not the whole window. Drawn in place until the pieces
        // exist, right after the first draw.
        let (title_bar, main) = match &self.parts {
            Some(parts) => (
                parts
                    .title_bar
                    .clone()
                    .cached(
                        StyleRefinement::default()
                            .w_full()
                            .h(px(title_bar::TITLE_BAR_HEIGHT))
                            .flex_none(),
                    )
                    .into_any_element(),
                parts
                    .main
                    .clone()
                    .cached(StyleRefinement::default().flex_1().min_w_0().h_full())
                    .into_any_element(),
            ),
            None => (
                self.title_bar_part(window, cx),
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.main_part(window, cx))
                    .into_any_element(),
            ),
        };
        let sidebar = self.sidebar_panel(cx);
        if sidebar.is_none() {
            ui::forget_piece(window.window_handle().window_id(), "sidebar");
        }
        let right = self.right_panel(window.viewport_size().width, window.window_handle().window_id(), cx);
        let notices = self.notices_layer(cx);
        let (under, over) = self.layers(window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .on_action(
                cx.listener(|this, _: &OpenSettings, window, cx| this.open_settings(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &crate::actions::CheckForUpdates, window, cx| {
                    this.check_updates(true, window, cx)
                }),
            )
            .text_color(rgb(theme::foreground()))
            .text_size(px(13.0))
            .on_action(cx.listener(|this, _: &NewTab, window, cx| this.new_terminal(window, cx)))
            .on_action(
                cx.listener(|this, _: &CloseTab, window, cx| this.close_active_tab(window, cx)),
            )
            .on_action(cx.listener(|this, _: &MoveTabLeft, _, cx| this.move_active_tab(false, cx)))
            .on_action(cx.listener(|this, _: &MoveTabRight, _, cx| this.move_active_tab(true, cx)))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.cycle(true, window, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousTab, window, cx| this.cycle(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, ActivateTab(index): &ActivateTab, window, cx| {
                    if let Some(tab) = this.tabs().get(*index) {
                        this.show(*tab, window, cx);
                    }
                }),
            )
            .on_action(
                cx.listener(|this, _: &SplitRight, window, cx| this.split(Dir::Row, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SplitDown, window, cx| this.split(Dir::Column, window, cx)),
            )
            .on_action(cx.listener(|this, _: &ZoomPane, window, cx| {
                if let Some(pane) = this.focused_pane() {
                    this.toggle_zoom(pane, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ClosePane, window, cx| {
                // The terminal on screen in place of the tab, or a floating one being typed in, closes before the tab
                // does, as the desktop's `closeFocused`.
                if let Some(key) = this.loose_page().or_else(|| this.focused_loose(window, cx)) {
                    this.close_quick(key, window, cx);
                } else if let Some(pane) = this
                    .focused_pane()
                    .or_else(|| this.focused_page_terminal(window, cx))
                {
                    this.close_pane(pane, window, cx);
                } else {
                    // A page tab (a plugin's, the project page, Issues) is a tab of one pane.
                    this.close_active_tab(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &FocusLeft, window, cx| {
                this.focus_toward(Toward::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusRight, window, cx| {
                this.focus_toward(Toward::Right, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusUp, window, cx| {
                this.focus_toward(Toward::Up, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusDown, window, cx| {
                this.focus_toward(Toward::Down, window, cx)
            }))
            .on_action(cx.listener(|this, _: &IncreaseFontSize, window, cx| {
                if !this.page_scales_text("bigger", window, cx) {
                    this.set_font_size(font_size(cx) + 1.0, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &DecreaseFontSize, window, cx| {
                if !this.page_scales_text("smaller", window, cx) {
                    this.set_font_size(font_size(cx) - 1.0, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &ResetFontSize, window, cx| {
                if !this.page_scales_text("reset", window, cx) {
                    this.set_font_size(DEFAULT_FONT_SIZE, cx)
                }
            }))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if this.page_on_tab(window).is_some_and(|page| page.read(cx).press_shortcut(&event.keystroke)) {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            }))
            .on_action(|_: &CloseWindow, _, cx| cx.quit())
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &Zoom, window, _| window.zoom_window())
            .on_action(|_: &ToggleFullScreen, window, _| window.toggle_fullscreen())
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_docked(cx)))
            .on_action(
                cx.listener(|this, _: &TogglePalette, window, cx| this.toggle_palette(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenHistory, window, cx| this.open_history(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleQuickTerminal, window, cx| {
                if this.prefs.quick_terminal {
                    this.toggle_quick(window, cx);
                } else {
                    // Turned off, the key goes on to the terminal, as in the desktop app.
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &OpenProjectInEditor, _, cx| {
                if let Some(editor) = this.default_editor().map(|editor| editor.id.clone()) {
                    this.open_in_editor(&editor, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ShowNotifications, _, cx| this.open_inbox(cx)))
            .on_action(cx.listener(|this, _: &TogglePerformanceReadout, _, cx| this.toggle_readout(cx)))
            .on_action(cx.listener(|this, _: &GoBack, window, cx| this.navigate(true, window, cx)))
            .on_action(
                cx.listener(|this, _: &GoForward, window, cx| this.navigate(false, window, cx)),
            )
            // The mouse's back and forward buttons, before a terminal can take them, as in the desktop app.
            .capture_any_mouse_down(|event: &MouseDownEvent, _, cx| {
                if matches!(event.button, MouseButton::Navigate(_)) {
                    cx.stop_propagation();
                }
            })
            .capture_any_mouse_up(cx.listener(|this, event: &MouseUpEvent, window, cx| {
                if let MouseButton::Navigate(direction) = event.button {
                    cx.stop_propagation();
                    this.navigate(direction == NavigationDirection::Back, window, cx);
                }
            }))
            // Escape closes a menu or the Inbox before the terminal under it gets the key.
            .when(
                self.menu.is_some() || self.inbox.open.is_some() || self.new_tab_menu,
                |root| {
                    let workspace = cx.entity();
                    root.capture_escape(move |_, _, cx| {
                        workspace.update(cx, |this, cx| {
                            this.close_menu(cx);
                            this.close_inbox(cx);
                            this.new_tab_menu = false;
                            cx.notify_workspace();
                        });
                        true
                    })
                    .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        if this.menu_key(event, window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                },
            )
            // The glass everything lies on, with a hole where a tab's backdrop shows in the layer under the window.
            .child(ui::window_glass(rgba(theme::glass())))
            .child(title_bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .relative()
                    .children(sidebar)
                    // Under the main column: the backdrop a tab's page asks for, in the place its card leaves for it.
                    .children(self.parts.as_ref().map(|parts| parts.backdrop.clone()))
                    .child(main)
                    // Beside the main column; what follows is laid over the main column alone, so draws over neither.
                    .children(right)
                    // What moves in the pages of the tab and the right panel (spinners, working dots, pulsing and
                    // counting lines), as if in them: over both, under what the window draws over the main column.
                    .children(self.parts.iter().flat_map(|parts| parts.view_layers.clone()))
                    // The text of the field being typed in, in the main column or the right panel, as if in its place.
                    .children(self.parts.as_ref().map(|parts| parts.fields.clone()))
                    // Over the main column, as if in it: what draws over the terminals in it follows.
                    .child(ui::slot_layer(pane_card::TERMINALS, self.tab_terminals(cx)))
                    // The terminals an agent opened floating over its pane, over its terminal.
                    // Each of these over the window's spinners, where those turning over its drawing give way
                    // (`ui::cover`), as do the layers over the window below.
                    .child(ui::slot_layer(
                        agent_terminals::FLOATS,
                        self.float_layer(cx)
                            .into_iter()
                            .map(|(name, float)| (name, ui::cover(float).into_any_element()))
                            .collect(),
                    ))
                    .child(ui::slot_layer(
                        QUICK,
                        self.quick_layer(cx)
                            .map(|quick| {
                                let area = div().relative().size_full().child(ui::cover(quick));
                                (QUICK.into(), area.into_any_element())
                            })
                            .into_iter()
                            .collect(),
                    ))
                    // The terminals a chat's agent opened, where its session sidebar shows them: after the sidebar, so
                    // they draw over its card and find the places it left in this draw.
                    .child(ui::slot_layer(
                        crate::page::TERMINALS,
                        self.page_terminals_layer(cx),
                    ))
                    .children(self.sidebar_diff(window.viewport_size().width, cx).map(ui::cover))
                    .children(self.sidebar_peek(cx).into_iter().map(ui::cover)),
            )
            // The sidebar's spinners, which turn here so the sidebar keeps its last drawing (see `parts`).
            .children(self.parts.as_ref().map(|parts| parts.spinners.clone()))
            .children(self.readout_layer(cx))
            .children(under.into_iter().map(ui::cover))
            .children(notices.map(ui::cover))
            .children(over.into_iter().map(ui::cover))
            .child(self.plugin_drop_layer(cx))
            .child(deferred(crate::ui::tooltip_exit()).with_priority(10))
            // Last: hands the spinners this draw shows to what turns them over the window's drawing, once every
            // overlay above has its place.
            .child(ui::spinner_sync(window))
            // The web pages the pages show, over the window's drawing where their places are and nothing covers them.
            .child(crate::web::sync(window))
            .into_any_element()
    }
}
