//! The window: terminal tabs whose panes split like the desktop app's, pages of the desktop's plugins as tabs, and the
//! Issues tab, with the desktop app's shortcuts.

mod agent_list;
mod diff;
#[cfg(all(test, feature = "test-support"))]
mod finder_tests;
mod history;
mod palette;
mod pane_card;
mod panes;
mod persist;
mod plugin_env;
#[cfg(all(test, feature = "test-support"))]
mod plugin_tests;
mod plugin_ui;
mod plugin_windows;
mod plugins;
mod project;
mod project_page;
mod project_sections;
mod rank;
mod settings;
mod sidebar;
mod status;
mod tabs;
#[cfg(all(test, feature = "test-support"))]
mod tests;
mod title_bar;

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use gpui::{
    AnyElement, Bounds, Context, DispatchPhase, Entity, FocusHandle, Focusable, MouseMoveEvent,
    MouseUpEvent, Pixels, SharedString, Subscription, Task, Window, canvas, div, prelude::*, px,
    rgb, rgba,
};

use crate::{
    actions::{
        ActivateTab, ClosePane, CloseTab, CloseWindow, DecreaseFontSize, FocusDown, FocusLeft,
        FocusRight, FocusUp, IncreaseFontSize, Minimize, MoveTabLeft, MoveTabRight, NewTab,
        NextTab, OpenHistory, OpenSettings, PreviousTab, ResetFontSize, SplitDown, SplitRight,
        ToggleFullScreen, TogglePalette, ToggleSidebar, Zoom, ZoomPane,
    },
    agents::{Agent, PaneInfo},
    host::Config,
    installed::{self, Installed},
    layout::{Dir, Layout, Toward},
    page::{PageView, plugin::Env},
    plugin,
    plugin_view::{IssuesEvent, IssuesView},
    projects::{ProjectEntry, Projects},
    saved,
    terminal_view::{
        CURSOR_STYLES, CursorShape, DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE, MAX_FONT_SIZE,
        MIN_FONT_SIZE, TerminalFont, TerminalLook, TerminalView, font_size,
    },
    text_field::TextField,
    theme,
};

/// Around panes that don't have focus.
use crate::ui::HAIRLINE;

struct Pane {
    /// A shell that couldn't start keeps its pane, showing why, until it's closed.
    view: Result<Entity<TerminalView>, SharedString>,
    focus: FocusHandle,
    /// Names the output saved for this pane on quit.
    history: String,
    /// A name you gave the pane, shown instead of its program's title, as in the desktop app.
    name: Option<SharedString>,
    /// The name field while you edit it, in place of the title.
    renaming: Option<(Entity<TextField>, Subscription)>,
}

struct TerminalTab {
    id: usize,
    layout: Layout,
    focused: usize,
    /// A pane filling the tab while the others keep running out of sight, as in the desktop app.
    zoomed: Option<usize>,
    focus: FocusHandle,
}

/// What a project that isn't shown keeps: its tabs, with their shells and plugin pages still running.
struct Parked {
    terminals: Vec<TerminalTab>,
    plugin_tabs: Vec<plugins::PluginTab>,
    project_page: Option<Entity<project_page::ProjectPage>>,
    active: Active,
}

/// A divider being dragged: which split, and where that split sits.
struct Drag {
    tab: usize,
    path: Vec<bool>,
    dir: Dir,
    bounds: Bounds<Pixels>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    project_page_focus: FocusHandle,
    /// The badge whose card is open.
    badge_card: Option<plugin_ui::BadgeCard>,
    /// The running plugins that may read Claude's transcripts, for sending them each entry.
    transcript_plugins: Vec<wings_plugins::manifest::Plugin>,
    /// The plugin sidebar shown in the panel on the right: its plugin's id and its own.
    right: Option<(String, String)>,
    /// The sidebar pages started so far, which keep running while the panel is closed or shows another.
    sidebar_pages: HashMap<(String, String), Entity<PageView>>,
    panes: HashMap<usize, Pane>,
    /// The next id for a terminal tab or a plugin tab: the two never share one.
    next_tab: usize,
    next_pane: usize,
    issues: Entity<IssuesView>,
    issues_focus: FocusHandle,
    new_focus: FocusHandle,
    active: Active,
    config: Config,
    in_flight: bool,
    load_task: Option<Task<()>>,
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
    /// The sidebar's width, order, git counts and peek.
    side: sidebar::State,
    /// What each pane's agent is doing, from the last detection, by pane.
    agents: HashMap<usize, Agent>,
    /// What each pane runs now, from the last detection.
    pane_info: HashMap<usize, PaneInfo>,
    window_active: bool,
    settings: Option<settings::SettingsDialog>,
    /// The command palette and the history sheet, one at a time, and the past sessions both search.
    palette: Option<palette::Palette>,
    sheet: Option<history::Sheet>,
    sessions: history::Sessions,
    _detection: Option<Task<()>>,
    _plugin_events: Task<()>,
    _activation: Subscription,
    _quit: Subscription,
}

/// How a shortcut reads in a button's name. The keys themselves are bound in `actions`.
fn shortcut_label(key: &str, extra: bool) -> String {
    if cfg!(target_os = "macos") {
        format!("{}⌘{key}", if extra { "⇧" } else { "" })
    } else {
        format!("Ctrl+Shift{}+{key}", if extra { "+Alt" } else { "" })
    }
}

impl Workspace {
    pub fn new(config: Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Registered before any terminal's, so it runs first and reads each shell's folder while it's alive.
        let quit = cx.on_app_quit(|this, cx| {
            this.save(cx);
            this.save_histories(cx);
            async {}
        });
        let issues = cx.new(IssuesView::new);
        cx.subscribe(&issues, |this, _, event: &IssuesEvent, cx| match event {
            IssuesEvent::Refresh => this.load(Some(plugin::Action::Refresh), cx),
        })
        .detach();
        let (plugin_events, requested) = async_channel::unbounded();
        let spaces = Arc::new(Projects::open(
            config.projects.clone(),
            config.project.clone(),
        ));
        let plugin_env = Arc::new(plugin_env::AppEnv::new(
            plugin_events,
            config.agents.clone().unwrap_or_default(),
            spaces.clone(),
        ));
        let sessions = history::Sessions::new(config.sessions.clone());
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
            right: None,
            sidebar_pages: HashMap::new(),
            panes: HashMap::new(),
            next_tab: 0,
            next_pane: 0,
            issues,
            issues_focus: cx.focus_handle().tab_stop(true),
            new_focus: cx.focus_handle().tab_stop(true),
            active: Active::Issues,
            config,
            in_flight: false,
            load_task: None,
            area: None,
            drag: None,
            projects: spaces.entries(),
            spaces,
            parked: HashMap::new(),
            sidebar: true,
            side: Default::default(),
            agents: HashMap::new(),
            pane_info: HashMap::new(),
            window_active: window.is_window_active(),
            settings: None,
            palette: None,
            sheet: None,
            sessions,
            _detection: None,
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
            _activation: cx.observe_window_activation(window, |this, window, _| {
                this.window_active = window.is_window_active();
                for installed in &this.plugins {
                    installed.host.publish_focus(this.window_active);
                }
            }),
            _quit: quit,
        };
        let env: Arc<dyn Env> = this.plugin_env.clone();
        this.plugins = this
            .config
            .plugins
            .as_ref()
            .map(|paths| installed::start(installed::discover(paths), paths, &env))
            .unwrap_or_default();
        this.plugin_env.set_plugins(&this.plugins);
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
        let settings = this
            .config
            .state
            .as_deref()
            .map(saved::load_settings)
            .unwrap_or_default();
        if let Some(size) = settings.font_size {
            cx.set_global(TerminalFont(size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)));
        }
        this.sidebar = settings.sidebar_open.unwrap_or(true);
        this.start_sidebar(&settings, window, cx);
        let installed = |family: &String| cx.text_system().all_font_names().contains(family);
        cx.set_global(TerminalLook {
            family: settings
                .font_family
                .filter(installed)
                .map_or(DEFAULT_FONT_FAMILY.into(), Into::into),
            shift_return_meta: settings.shift_return_meta.unwrap_or(true),
            cursor: CURSOR_STYLES
                .iter()
                .find(|(_, name, _)| settings.cursor_style.as_deref() == Some(*name))
                .map_or(CursorShape::Block, |(shape, ..)| *shape),
        });
        this.enter_project(window, cx);
        this
    }

    /// Every terminal picks the new size up on its next frame and refits its shell.
    fn set_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        cx.set_global(TerminalFont(size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)));
        self.save(cx);
        cx.refresh_windows();
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_plugins(cx);
        if let Some((viewer, _)) = &self.plugin_ui.diff {
            return div().size_full().child(viewer.clone()).into_any_element();
        }
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
        let body: AnyElement = match self.active {
            Active::Terminal(id) => match self.terminals.iter().find(|tab| tab.id == id) {
                Some(tab) => {
                    let entity = cx.entity();
                    let measure = canvas(
                        move |bounds, _, cx| entity.update(cx, |this, _| this.area = Some(bounds)),
                        {
                            let entity = cx.entity();
                            move |_, _, window, _| {
                                let moved = entity.clone();
                                window.on_mouse_event(
                                    move |event: &MouseMoveEvent, phase, _, cx| {
                                        if phase == DispatchPhase::Bubble && event.dragging() {
                                            moved.update(cx, |this, cx| {
                                                this.drag_to(event.position, cx)
                                            });
                                        }
                                    },
                                );
                                let released = entity.clone();
                                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        released.update(cx, |this, cx| {
                                            if this.drag.take().is_some() {
                                                this.save(cx);
                                            }
                                        });
                                    }
                                });
                            }
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full();
                    let layout = match tab.zoomed {
                        Some(pane) => self.pane_card(pane, true, true, cx),
                        None => self.layout_element(
                            tab.id,
                            &tab.layout,
                            &mut Vec::new(),
                            tab.focused,
                            cx,
                        ),
                    };
                    div()
                        .size_full()
                        .p(px(8.0))
                        .child(div().relative().size_full().child(layout).child(measure))
                        .into_any_element()
                }
                None => div().into_any_element(),
            },
            Active::Plugin(id) => match self.plugin_tabs.iter().find(|tab| tab.id == id) {
                Some(tab) => tab.page.clone().into_any_element(),
                None => div().into_any_element(),
            },
            Active::Project => match &self.project_page {
                Some(page) => page.clone().into_any_element(),
                None => div().into_any_element(),
            },
            Active::Issues => self.issues.clone().into_any_element(),
        };
        let mut tabs: Vec<AnyElement> = self
            .terminals
            .iter()
            .map(|tab| {
                self.tab(
                    Active::Terminal(tab.id),
                    self.label(tab.id, cx),
                    self.tab_state(tab.id),
                    &tab.focus,
                    cx,
                )
                .into_any_element()
            })
            .collect();
        tabs.extend(self.plugin_tabs.iter().map(|tab| {
            self.tab(
                Active::Plugin(tab.id),
                tab.title.clone(),
                None,
                &tab.focus,
                cx,
            )
            .into_any_element()
        }));
        if self.project_page.is_some() {
            tabs.push(
                self.tab(
                    Active::Project,
                    "Project page".into(),
                    None,
                    &self.project_page_focus,
                    cx,
                )
                .into_any_element(),
            );
        }
        tabs.push(
            self.tab(
                Active::Issues,
                "Issues".into(),
                None,
                &self.issues_focus,
                cx,
            )
            .into_any_element(),
        );
        let title_bar = self.title_bar(tabs, cx);
        let sidebar = self.sidebar.then(|| self.sidebar_panel(cx));
        let right = self.right_panel(cx);
        let notices = self.notices_layer(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(rgba(theme::GLASS))
            .on_action(
                cx.listener(|this, _: &OpenSettings, window, cx| this.open_settings(window, cx)),
            )
            .text_color(rgb(theme::FOREGROUND))
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
                if let Some(pane) = this.focused_pane() {
                    this.close_pane(pane, window, cx);
                } else if matches!(this.active, Active::Plugin(_) | Active::Project) {
                    // A plugin page is a tab of one pane.
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
            .on_action(cx.listener(|this, _: &IncreaseFontSize, _, cx| {
                this.set_font_size(font_size(cx) + 1.0, cx)
            }))
            .on_action(cx.listener(|this, _: &DecreaseFontSize, _, cx| {
                this.set_font_size(font_size(cx) - 1.0, cx)
            }))
            .on_action(cx.listener(|this, _: &ResetFontSize, _, cx| {
                this.set_font_size(DEFAULT_FONT_SIZE, cx)
            }))
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
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
            .child(title_bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .relative()
                    .children(sidebar)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(self.project_bar())
                            .child(div().flex_1().min_h_0().child(body)),
                    )
                    .children(right)
                    .children(self.sidebar_peek(cx)),
            )
            .children(self.settings_dialog(cx))
            .children(self.sheet_layer(cx))
            .children(self.palette_layer(window, cx))
            .children(self.remove_dialog(cx))
            .children(notices)
            .into_any_element()
    }
}
