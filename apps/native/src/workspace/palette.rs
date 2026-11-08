//! The command palette (Cmd-K): commands, projects, tabs, agents, plugin pages and recent sessions in one list that
//! narrows as you type, like the desktop's `command-palette.tsx`. It lists the commands the native app has. Choosing
//! one closes the palette, gives the keys back to the pane you were in, and then runs it, so a command that opens
//! something of its own (Settings, Past Sessions, Find) takes the keys from there.

use std::{path::PathBuf, rc::Rc, time::Duration};

use gpui::{
    Action, AnyElement, App, Context, Entity, FocusHandle, MouseButton, Role, ScrollHandle,
    SharedString, Subscription, Task, Window, div, prelude::*, px, rgb, rgba,
};
use wings_agents::history::{Filter, Hit};

use crate::{
    actions::{
        ClosePane, CloseTab, DecreaseFontSize, Find, FocusDown, FocusLeft, FocusRight, FocusUp,
        IncreaseFontSize, MoveTabLeft, MoveTabRight, NewTab, NextTab, OpenHistory,
        OpenProjectInEditor, OpenSettings, PaletteFirst, PaletteLast, PaletteNext, PalettePrevious,
        PreviousTab, ResetFontSize, ShowNotifications, SplitDown, SplitRight, ToggleQuickTerminal,
        ToggleSidebar, ZoomPane,
    },
    agents::{self, AgentState},
    icons::{Icon, icon},
    text_field::{FieldEvent, TextField},
    theme, ui,
};

use super::{
    Active, Workspace,
    history::{SessionRef, one_line},
    panes::tildify,
    rank::{Item, Kind, group_of, rank},
};

/// The most lines the palette lists.
const SHOWN: usize = 80;
/// How many recent sessions it asks the history for.
const SESSIONS: usize = 8;
/// Typing waits this long before asking the history for sessions, so a word typed at speed asks once.
const SESSION_DEBOUNCE: Duration = Duration::from_millis(150);
/// The desktop's `sm:max-w-xl`.
const WIDTH: f32 = 576.0;

/// What choosing a line does.
#[derive(Clone)]
pub(super) enum Run {
    /// A command: the same action its key and its menu item send.
    Action(Rc<dyn Action>),
    Project(PathBuf),
    /// A project's page, as a tab.
    ProjectPage(PathBuf),
    /// A tab of a project, which may not be the one on show.
    Tab(PathBuf, Active),
    /// The pane of an agent in a project.
    Agent(PathBuf, usize),
    /// A plugin's page, as a tab.
    Page(String, String),
    Session(SessionRef),
}

type Entry = Item<Run>;

pub(super) struct Palette {
    field: Entity<TextField>,
    /// What the panel itself has, so a click on it that misses the field still keeps the keys in the palette.
    focus: FocusHandle,
    pub(super) active: usize,
    scroll: ScrollHandle,
    /// The past sessions that match what is typed, as lines.
    pub(super) sessions: Vec<Entry>,
    /// The search that fills `sessions`; a newer one replaces it.
    load: Option<Task<()>>,
    _events: Subscription,
}

/// How the keymap writes a binding of `action`, as the desktop writes shortcuts: "⇧⌘D" on macOS, "Ctrl+Shift+Alt+D"
/// elsewhere. The first binding is the one shown.
pub(super) fn shortcut_label(action: &dyn Action, cx: &App) -> Option<String> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let binding = keymap.bindings_for_action(action).next()?;
    let stroke = binding.keystrokes().first()?;
    let (mods, key) = (stroke.modifiers(), stroke.key());
    let key = match key {
        "left" => "←".to_owned(),
        "right" => "→".to_owned(),
        "up" => "↑".to_owned(),
        "down" => "↓".to_owned(),
        key => key.to_uppercase(),
    };
    Some(if cfg!(target_os = "macos") {
        let flag = |on: bool, mark: &'static str| if on { mark } else { "" };
        format!(
            "{}{}{}{}{key}",
            flag(mods.control, "⌃"),
            flag(mods.alt, "⌥"),
            flag(mods.shift, "⇧"),
            flag(mods.platform, "⌘")
        )
    } else {
        let flag = |on: bool, name: &'static str| on.then_some(name);
        [
            flag(mods.control, "Ctrl"),
            flag(mods.shift, "Shift"),
            flag(mods.alt, "Alt"),
        ]
        .into_iter()
        .flatten()
        .chain([key.as_str()])
        .collect::<Vec<_>>()
        .join("+")
    })
}

/// A recent session as a line: its name, and where and when it was.
fn session_entry(hit: &Hit) -> Option<Entry> {
    let cwd = hit.cwd.clone()?;
    let title = one_line(
        hit.title
            .as_deref()
            .or(hit.first_prompt.as_deref())
            .unwrap_or("Untitled session"),
    );
    let mut line = Item::new(
        Kind::Session,
        title,
        Run::Session(SessionRef {
            id: hit.id.clone(),
            cwd: cwd.clone(),
            agent: hit.agent,
            transcript: hit.transcript.clone(),
        }),
    );
    let place = cwd.rsplit('/').next().map(str::to_owned);
    line.subtitle = Some(
        [place, hit.git_branch.clone(), hit.last_active.clone()]
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
    );
    line.keywords = Some(cwd);
    Some(line)
}

impl Workspace {
    /// Cmd-K: opens the palette, or closes it.
    pub(super) fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            return self.close_palette(window, cx);
        }
        // Settings is a dialog of its own, which the palette would take the keys from.
        if self.settings.is_some() {
            return;
        }
        self.sheet = None;
        let field = cx.new(|cx| {
            TextField::new(
                Role::SearchInput,
                "Search commands",
                "Search commands, projects, tabs and agents",
                px(WIDTH - 48.0),
                cx,
            )
        });
        let events = cx.subscribe_in(&field, window, |this, _, event, window, cx| match event {
            FieldEvent::Changed => this.palette_typed(cx),
            FieldEvent::Submit { .. } => {
                let at = this.palette.as_ref().map_or(0, |palette| palette.active);
                this.palette_choose(at, window, cx);
            }
            FieldEvent::Cancel => this.close_palette(window, cx),
        });
        field.update(cx, |field, cx| field.select_all(window, cx));
        self.palette = Some(Palette {
            field,
            focus: cx.focus_handle(),
            active: 0,
            scroll: ScrollHandle::new(),
            sessions: Vec::new(),
            load: None,
            _events: events,
        });
        self.load_palette_sessions(cx);
        cx.notify();
    }

    /// Closes the palette and gives the keys back to what had them: the pane you were in.
    pub(super) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(palette) = self.palette.take() {
            self.leave("palette", palette, crate::ui::motion::POPUP_IN, cx);
            self.show(self.active, window, cx);
        }
    }

    fn palette_typed(&mut self, cx: &mut Context<Self>) {
        if let Some(palette) = &mut self.palette {
            palette.active = 0;
            palette.scroll.scroll_to_item(0);
        }
        self.load_palette_sessions(cx);
        cx.notify();
    }

    /// Asks the history for the sessions that match what is typed, or the latest ones.
    fn load_palette_sessions(&mut self, cx: &mut Context<Self>) {
        let Some(palette) = &mut self.palette else {
            return;
        };
        let query = palette.field.read(cx).text.trim().to_string();
        let sessions = self.sessions.clone();
        palette.load = Some(cx.spawn(async move |this, cx| {
            if !query.is_empty() {
                cx.background_executor().timer(SESSION_DEBOUNCE).await;
            }
            let found = cx
                .background_executor()
                .spawn(async move { sessions.search(&query, &Filter::default(), SESSIONS) })
                .await;
            this.update(cx, |this, cx| {
                if let Some(palette) = &mut this.palette {
                    palette.sessions = found.sessions.iter().filter_map(session_entry).collect();
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// The up and down keys, and Home and End: `to` is where the highlight goes, among `count` lines.
    fn palette_move(&mut self, to: impl FnOnce(usize, usize) -> usize, cx: &mut Context<Self>) {
        let count = self.palette_shown(cx).len();
        let Some(palette) = &mut self.palette else {
            return;
        };
        if count > 0 {
            palette.active = to(palette.active.min(count - 1), count);
            palette.scroll.scroll_to_item(palette.active);
            cx.notify();
        }
    }

    /// Runs line `at` of the list. A line that can't run now, like Close Tab with no tab, does nothing.
    pub(super) fn palette_choose(
        &mut self,
        at: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.palette_entries(cx);
        let Some(palette) = &self.palette else {
            return;
        };
        let shown = rank(&entries, &palette.field.read(cx).text, SHOWN);
        let Some(entry) = shown
            .get(at.min(shown.len().saturating_sub(1)))
            .map(|&i| &entries[i])
        else {
            return;
        };
        if entry.disabled {
            return;
        }
        let run = entry.run.clone();
        self.close_palette(window, cx);
        match run {
            Run::Action(action) => window.dispatch_action(action.boxed_clone(), cx),
            Run::Project(path) => self.switch_project(path, window, cx),
            Run::ProjectPage(path) => self.open_project_page(path, window, cx),
            Run::Tab(project, tab) => {
                self.switch_project(project, window, cx);
                self.show(tab, window, cx);
            }
            Run::Agent(project, pane) => self.go_to_pane(project, pane, window, cx),
            Run::Page(plugin, view) => {
                self.open_plugin_view(&plugin, &view, None, window, cx);
            }
            Run::Session(session) => {
                if let Err(error) = self.launch_session(&session, false, window, cx) {
                    self.show_notice("Couldn't resume the session".into(), error, cx);
                }
            }
        }
    }

    /// The commands the native app has, in the desktop's order, with their group and whether they can run now.
    pub(super) fn palette_commands(
        &self,
    ) -> Vec<(&'static str, &'static str, Rc<dyn Action>, bool)> {
        let terminal = matches!(self.active, Active::Terminal(_));
        let movable = match self.active {
            Active::Terminal(_) => self.terminals.len() > 1,
            Active::Plugin(_) => self.plugin_tabs.len() > 1,
            Active::Project | Active::Issues => false,
        };
        let cycles = self.tabs().len() > 1;
        let mut commands: Vec<(&str, &str, Rc<dyn Action>, bool)> = vec![
            ("New Tab", "Tab", Rc::new(NewTab), true),
            ("Close Tab", "Tab", Rc::new(CloseTab), true),
            ("Next Tab", "Tab", Rc::new(NextTab), cycles),
            ("Previous Tab", "Tab", Rc::new(PreviousTab), cycles),
            ("Move Tab Left", "Tab", Rc::new(MoveTabLeft), movable),
            ("Move Tab Right", "Tab", Rc::new(MoveTabRight), movable),
            ("Split Right", "Pane", Rc::new(SplitRight), terminal),
            ("Split Down", "Pane", Rc::new(SplitDown), terminal),
            ("Close Pane", "Pane", Rc::new(ClosePane), true),
            ("Zoom Pane", "Pane", Rc::new(ZoomPane), terminal),
            ("Focus Pane Left", "Pane", Rc::new(FocusLeft), terminal),
            ("Focus Pane Right", "Pane", Rc::new(FocusRight), terminal),
            ("Focus Pane Above", "Pane", Rc::new(FocusUp), terminal),
            ("Focus Pane Below", "Pane", Rc::new(FocusDown), terminal),
            ("Toggle Sidebar", "View", Rc::new(ToggleSidebar), true),
            ("Find in Pane", "View", Rc::new(Find), terminal),
            ("Past Sessions", "View", Rc::new(OpenHistory), true),
            (
                "Quick Terminal",
                "View",
                Rc::new(ToggleQuickTerminal),
                self.prefs.quick_terminal,
            ),
            (
                "Increase Font Size",
                "View",
                Rc::new(IncreaseFontSize),
                true,
            ),
            (
                "Decrease Font Size",
                "View",
                Rc::new(DecreaseFontSize),
                true,
            ),
            ("Reset Font Size", "View", Rc::new(ResetFontSize), true),
            (
                "Show Notifications",
                "View",
                Rc::new(ShowNotifications),
                true,
            ),
            ("Settings", "App", Rc::new(OpenSettings), true),
            (
                "Check for Updates",
                "App",
                Rc::new(crate::actions::CheckForUpdates),
                true,
            ),
        ];
        // Only with an editor installed, as on the desktop.
        if self.default_editor().is_some() {
            commands.push((
                "Open Project in Editor",
                "App",
                Rc::new(OpenProjectInEditor),
                !self.welcome,
            ));
        }
        commands
    }

    /// What the palette searches besides past sessions, built whenever it is drawn or used.
    pub(super) fn palette_entries(&self, cx: &App) -> Vec<Entry> {
        let project_name = |path: &std::path::Path| {
            self.projects
                .iter()
                .find(|project| project.path == path)
                .map_or_else(
                    || {
                        path.file_name()
                            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
                    },
                    |project| project.name.clone(),
                )
        };
        let mut items = Vec::new();
        for (title, group, action, enabled) in self.palette_commands() {
            let mut item = Item::new(Kind::Command, title, Run::Action(action.clone()));
            item.keywords = Some(group.into());
            item.shortcut = shortcut_label(action.as_ref(), cx);
            item.disabled = !enabled;
            items.push(item);
        }
        for project in &self.projects {
            let mut item = Item::new(
                Kind::Command,
                format!("Project page: {}", project.name),
                Run::ProjectPage(project.path.clone()),
            );
            item.keywords = Some("notes facts info".into());
            items.push(item);
        }
        for installed in &self.plugins {
            for page in installed.pages.iter().filter(|page| page.new_tab) {
                let mut item = Item::new(
                    Kind::Command,
                    format!("New tab: {}", page.title),
                    Run::Page(installed.id.clone(), page.id.clone()),
                );
                item.keywords = Some("plugin page".into());
                items.push(item);
            }
        }
        for project in &self.projects {
            let mut item = Item::new(
                Kind::Project,
                project.name.clone(),
                Run::Project(project.path.clone()),
            );
            item.subtitle = Some(tildify(&project.path.to_string_lossy()));
            item.keywords = Some(project.path.to_string_lossy().into_owned());
            items.push(item);
        }
        // The tabs of the project on show, then those of the projects parked behind it.
        let mut parked: Vec<_> = self.parked.iter().collect();
        parked.sort_by_key(|(path, _)| *path);
        let current = (
            &self.config.project,
            self.terminals.as_slice(),
            self.plugin_tabs.as_slice(),
            self.project_page.is_some(),
        );
        let projects = std::iter::once(current).chain(parked.into_iter().map(|(path, parked)| {
            (
                path,
                parked.terminals.as_slice(),
                parked.plugin_tabs.as_slice(),
                parked.project_page.is_some(),
            )
        }));
        for (path, terminals, plugin_tabs, project_page) in projects {
            let tabs = terminals
                .iter()
                .map(|tab| {
                    let title = self.pane_title(tab.focused, cx).to_string();
                    (Active::Terminal(tab.id), title)
                })
                .chain(
                    plugin_tabs
                        .iter()
                        .map(|tab| (Active::Plugin(tab.id), tab.title.to_string())),
                )
                .chain(project_page.then(|| (Active::Project, "Project page".to_string())));
            for (tab, title) in tabs {
                let mut item = Item::new(Kind::Tab, title, Run::Tab(path.clone(), tab));
                item.subtitle = Some(project_name(path));
                items.push(item);
            }
        }
        let mut item = Item::new(
            Kind::Tab,
            "Issues",
            Run::Tab(self.config.project.clone(), Active::Issues),
        );
        item.subtitle = Some(project_name(&self.config.project));
        items.push(item);
        let mut live: Vec<_> = self.agents.iter().collect();
        live.sort_by_key(|(pane, _)| **pane);
        for (pane, agent) in live {
            let path = PathBuf::from(&agent.space_id);
            let state = if agent.state == AgentState::Blocked {
                agent
                    .waiting_for
                    .clone()
                    .unwrap_or_else(|| agents::state_label(agent.state).to_owned())
            } else {
                agents::state_label(agent.state).to_owned()
            };
            let mut item = Item::new(
                Kind::Agent,
                agent
                    .name
                    .clone()
                    .unwrap_or_else(|| agents::label(agent.kind).to_owned()),
                Run::Agent(path.clone(), *pane),
            );
            item.subtitle = Some(format!("{} · {state}", project_name(&path)));
            item.state = Some(agent.state);
            items.push(item);
        }
        if let Some(palette) = &self.palette {
            items.extend(palette.sessions.iter().cloned());
        }
        items
    }

    pub(super) fn palette_shown(&self, cx: &App) -> Vec<usize> {
        let query = self
            .palette
            .as_ref()
            .map(|palette| palette.field.read(cx).text.clone())
            .unwrap_or_default();
        rank(&self.palette_entries(cx), &query, SHOWN)
    }

    /// The palette over the window, or nothing while it is closed.
    pub(super) fn palette_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let palette = self.palette.as_ref()?;
        let entries = self.palette_entries(cx);
        let query = palette.field.read(cx).text.clone();
        let typed = !query.trim().is_empty();
        let shown = rank(&entries, &query, SHOWN);
        let active = palette.active.min(shown.len().saturating_sub(1));
        let height = window.viewport_size().height;
        let mut group = "";
        let rows: Vec<AnyElement> = shown
            .iter()
            .enumerate()
            .map(|(at, &index)| {
                let item = &entries[index];
                let heading = group_of(item);
                let new_group = !typed && heading != group;
                group = heading;
                let trailing: Option<AnyElement> = match (&item.shortcut, typed) {
                    (Some(shortcut), _) => Some(kbd(shortcut, false).into_any_element()),
                    (None, true) => Some(
                        div()
                            .flex_none()
                            .text_size(px(11.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(heading)
                            .into_any_element(),
                    ),
                    (None, false) => None,
                };
                div()
                    .children(new_group.then(|| {
                        div()
                            .px(px(10.0))
                            .pt(px(8.0))
                            .pb(px(4.0))
                            .text_size(px(11.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(heading)
                    }))
                    .child(
                        div()
                            .id(("palette-row", at))
                            .role(Role::ListBoxOption)
                            .aria_label(item.title.clone())
                            .aria_selected(at == active)
                            .h(px(36.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .px(px(10.0))
                            .rounded(px(6.0))
                            .text_size(px(13.0))
                            .when(at == active, |row| row.bg(rgba(theme::HOVER_STRONG)))
                            .when(item.disabled, |row| row.opacity(0.5))
                            .when(!item.disabled, |row| row.cursor_pointer())
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if let Some(palette) = &mut this.palette
                                    && palette.active != at
                                {
                                    palette.active = at;
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.palette_choose(at, window, cx)
                            }))
                            .children(
                                item.state
                                    .map(|state| ui::status_dot(state).into_any_element()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .max_w_full()
                                    .truncate()
                                    .child(item.title.clone()),
                            )
                            .child(match &item.subtitle {
                                Some(subtitle) => div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme::MUTED_FOREGROUND))
                                    .child(subtitle.clone()),
                                None => div().flex_1(),
                            })
                            .children(trailing),
                    )
                    .into_any_element()
            })
            .collect();
        let list = div()
            .id("palette-list")
            .role(Role::ListBox)
            .aria_label("Results")
            .max_h(px((f32::from(height) * 0.5).min(380.0)))
            .overflow_y_scroll()
            .track_scroll(&palette.scroll)
            .p(px(6.0))
            .when(rows.is_empty(), |list| {
                list.child(
                    div()
                        .px(px(12.0))
                        .py(px(24.0))
                        .flex()
                        .justify_center()
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child(format!("Nothing matches “{}”", query.trim())),
                )
            })
            .children(rows);
        let search = div()
            .h(px(44.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(12.0))
            .border_b_1()
            .border_color(rgba(theme::BORDER))
            .child(icon(Icon::Search, 16.0, theme::MUTED_FOREGROUND))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .child(palette.field.clone()),
            );
        let hints = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(12.0))
            .py(px(8.0))
            .border_t_1()
            .border_color(rgba(theme::BORDER))
            .text_size(px(11.0))
            .text_color(rgb(theme::MUTED_FOREGROUND))
            .children(
                [("↑↓", "move"), ("↵", "run"), ("esc", "close")].map(|(key, what)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(kbd(key, true))
                        .child(what)
                }),
            );
        let (backdrop, panel) =
            ui::dialog("palette", "Command palette", &palette.focus, WIDTH, 0.0);
        Some(
            backdrop
                .items_start()
                .pt(px(f32::from(height) * 0.16))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_palette(window, cx)),
                )
                .child(ui::dialog_panel(
                    panel
                        .key_context("Palette")
                        .h_auto()
                        .flex_col()
                        .on_action(cx.listener(|this, _: &PaletteNext, _, cx| {
                            this.palette_move(|at, count| (at + 1) % count, cx)
                        }))
                        .on_action(cx.listener(|this, _: &PalettePrevious, _, cx| {
                            this.palette_move(|at, count| (at + count - 1) % count, cx)
                        }))
                        .on_action(cx.listener(|this, _: &PaletteFirst, _, cx| {
                            this.palette_move(|_, _| 0, cx)
                        }))
                        .on_action(cx.listener(|this, _: &PaletteLast, _, cx| {
                            this.palette_move(|_, count| count - 1, cx)
                        }))
                        .child(search)
                        .child(list)
                        .child(hints),
                ))
                .into_any_element(),
        )
    }
}

/// The desktop's `Kbd`: a key or a shortcut in a small rounded box.
fn kbd(text: impl Into<SharedString>, small: bool) -> impl IntoElement {
    div()
        .h(px(if small { 16.0 } else { 20.0 }))
        .min_w(px(if small { 16.0 } else { 20.0 }))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .px(px(4.0))
        .rounded(px(4.0))
        .bg(rgba(0xffffff14))
        .text_size(px(if small { 10.0 } else { 11.0 }))
        .text_color(rgb(theme::MUTED_FOREGROUND))
        .child(text.into())
}
