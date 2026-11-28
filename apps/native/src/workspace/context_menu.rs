//! The desktop's right-click menus on tabs, pane headers, project rows and agent rows (`title-bar.tsx`, `pane-grid.tsx`,
//! `sidebar.tsx`), with the items whose actions the native app has.

use crate::workspace::NotifyWorkspace;
use std::path::PathBuf;

use gpui::{
    AnyElement, ClipboardItem, Context, ElementId, Pixels, Point, Role, SharedString, Toggled,
    Window, div, prelude::*, px, relative,
};

use crate::{
    agents::AgentKind,
    icons::Icon,
    layout::{Dir, Layout},
    ui,
};

use super::{Active, TerminalTab, Workspace, history::fork_command, pane_card::PaneAction};

/// What a menu was opened on.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum MenuTarget {
    Tab(Active),
    /// A pane, by its header.
    Pane(usize),
    /// A pane, by the "…" of a header too narrow for all its buttons: those it moved here come first in the menu.
    PaneMore(usize, Vec<PaneAction>),
    Project(PathBuf),
    /// An agent in the sidebar, by its pane.
    Agent(usize),
    /// A plugin's sidebar page, by its title bar button or its header in the right sidebar.
    Sidebar(super::right_stack::Key),
}

pub(super) struct Menu {
    at: Point<Pixels>,
    pub(super) target: MenuTarget,
    /// The pane menu's "Move to tab" list is open beside it.
    moving: bool,
    /// The item the keys are on.
    keys: ui::behaviors::MenuKeys,
}

fn target_path(target: &MenuTarget) -> PathBuf {
    match target {
        MenuTarget::Project(path) => path.clone(),
        _ => PathBuf::new(),
    }
}

type Run = Box<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// One item of a menu: its icon, label, whether it destroys something, and what it does, or `None` while it can't.
struct Item(Icon, SharedString, bool, Option<Run>);

fn run(
    action: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) -> Option<Run> {
    Some(Box::new(action))
}

impl Workspace {
    pub(super) fn open_menu(
        &mut self,
        target: MenuTarget,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.menu = Some(Menu {
            at,
            target,
            moving: false,
            keys: Default::default(),
        });
        cx.stop_propagation();
        cx.notify_workspace();
    }

    pub(super) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.take() {
            self.leave("menu", menu, crate::ui::motion::POPUP_IN, cx);
        }
    }

    /// The open menu's keys: Up, Down, Home, End and letters move the highlight, Enter picks. True when the menu took
    /// the key.
    pub(super) fn menu_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(target) = self.menu.as_ref().map(|menu| menu.target.clone()) else {
            return false;
        };
        let mut entries: Vec<(SharedString, Option<Run>)> = Vec::new();
        let mut groups = match &target {
            MenuTarget::Tab(tab) => self.tab_items(*tab),
            MenuTarget::Pane(pane) => self.pane_items(*pane, &[]),
            MenuTarget::PaneMore(pane, moved) => self.pane_items(*pane, moved),
            MenuTarget::Project(path) => self.project_items(path),
            MenuTarget::Agent(pane) => self.agent_items(*pane),
            MenuTarget::Sidebar(key) => self.sidebar_items(key),
        };
        let checks = match &target {
            MenuTarget::Project(path) if !self.projects.iter().any(|entry| entry.path == *path && entry.child) => {
                self.section_choices(path)
            }
            _ => Vec::new(),
        };
        let last = groups.len().saturating_sub(1);
        for (index, group) in groups.drain(..).enumerate() {
            if index == last && index > 0 {
                for (key, title, on) in &checks {
                    let (project, key, on) = (target_path(&target), key.clone(), *on);
                    entries.push((
                        title.clone().into(),
                        run(move |this, _, cx| {
                            this.menu = None;
                            this.set_section_off(&project, &key, on, cx);
                        }),
                    ));
                }
            }
            entries.extend(group.into_iter().map(|Item(_, label, _, action)| (label, action)));
        }
        let labels: Vec<Option<String>> =
            entries.iter().map(|(label, run)| run.as_ref().map(|_| label.to_string())).collect();
        let Some(menu) = self.menu.as_mut() else {
            return false;
        };
        match menu.keys.press(event, &labels) {
            ui::behaviors::MenuKey::Ignored => return false,
            ui::behaviors::MenuKey::Moved => {}
            ui::behaviors::MenuKey::Pick(at) => {
                if let Some((_, Some(action))) = entries.get(at) {
                    self.menu = None;
                    action(self, window, cx);
                }
            }
        }
        cx.notify_workspace();
        true
    }

    /// The open menu, above everything else, where it was opened.
    pub(super) fn menu_layer(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let groups = match &menu.target {
            MenuTarget::Tab(tab) => self.tab_items(*tab),
            MenuTarget::Pane(pane) => self.pane_items(*pane, &[]),
            MenuTarget::PaneMore(pane, moved) => self.pane_items(*pane, moved),
            MenuTarget::Project(path) => self.project_items(path),
            MenuTarget::Agent(pane) => self.agent_items(*pane),
            MenuTarget::Sidebar(key) => self.sidebar_items(key),
        };
        let mut children = Vec::new();
        let mut checks = match &menu.target {
            MenuTarget::Project(path) => self.section_checks(path, menu.keys.highlight, cx),
            _ => Vec::new(),
        };
        // The items in the order the keys go through them: the sections' checks come before the last group.
        let mut place = 0;
        let last = groups.len().saturating_sub(1);
        for (index, group) in groups.into_iter().enumerate() {
            if index > 0 {
                children.push(ui::menu_separator());
            }
            // The sections' checks go above the last group, "Remove from sidebar".
            if index == last && index > 0 && !checks.is_empty() {
                place += checks.len();
                children.append(&mut checks);
                children.push(ui::menu_separator());
            }
            for item in group {
                children.push(self.menu_item(item, menu.keys.highlight == Some(place), cx));
                place += 1;
            }
        }
        if let MenuTarget::Pane(pane) | MenuTarget::PaneMore(pane, _) = &menu.target {
            children.push(self.move_to_tab(*pane, menu.moving, cx));
        }
        // The most it may be tall: the window less the padding `popup_at` keeps, so it shifts up to fit whole.
        let room = window.viewport_size().height - px(16.0) - window.client_inset().unwrap_or_default() * 2.0;
        Some(ui::context_menu(
            menu.at,
            ui::menu("context-menu", "Menu")
                // A menu taller than the window scrolls; a shorter one shifts up to fit. A pane's is short, and its
                // "Move to tab" list opens outside its box, which scrolling would clip.
                .when(!matches!(menu.target, MenuTarget::Pane(_) | MenuTarget::PaneMore(..)), |menu| {
                    menu.max_h(room).overflow_y_scroll()
                })
                .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_menu(cx)))
                .children(ui::spot(|| "context-menu".into()))
                .children(children),
        ))
    }

    /// A project's plugin sections, a check on each it shows; a click turns one off or on.
    fn section_checks(
        &self,
        project: &std::path::Path,
        highlight: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if self.projects.iter().any(|entry| entry.path == project && entry.child) {
            return Vec::new();
        }
        self.section_choices(project)
            .into_iter()
            .enumerate()
            .map(|(at, (key, title, on))| {
                let project = project.to_path_buf();
                // The check stands where the other items have their icon, so all the labels line up.
                let glyph = match on {
                    true => crate::icons::icon(Icon::Check, 16.0, crate::theme::muted_foreground()).into_any_element(),
                    false => div().size(px(16.0)).flex_none().into_any_element(),
                };
                ui::menu_highlight(
                    ui::menu_item_with(ElementId::Name(format!("menu-section-{key}").into()), glyph, title, false, false),
                    highlight == Some(at),
                )
                    .role(Role::MenuItemCheckBox)
                    .aria_toggled(if on { Toggled::True } else { Toggled::False })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu = None;
                        this.set_section_off(&project, &key, on, cx);
                    }))
                    .into_any_element()
            })
            .collect()
    }

    fn menu_item(
        &self,
        Item(glyph, label, destructive, action): Item,
        highlighted: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = ElementId::Name(format!("menu-{label}").into());
        let disabled = action.is_none();
        ui::menu_highlight(ui::menu_item(id, glyph, label, destructive, disabled), highlighted)
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if let Some(menu) = this.menu.as_mut().filter(|menu| *over && menu.moving) {
                    menu.moving = false;
                    cx.notify_workspace();
                }
            }))
            .when_some(action, |item, action| {
                item.on_click(cx.listener(move |this, _, window, cx| {
                    this.menu = None;
                    action(this, window, cx);
                    cx.notify_workspace();
                }))
            })
            .into_any_element()
    }

    /// A tab's menu: move it within its kind (terminals among terminals, pages among pages, as Cmd-Shift-[ does), and
    /// close it.
    fn tab_items(&self, tab: Active) -> Vec<Vec<Item>> {
        let (left, right) = self.tab_moves(tab);
        vec![
            vec![
                Item(
                    Icon::ArrowLeft,
                    "Move left".into(),
                    false,
                    left.then(|| run(move |this, _, cx| this.move_tab(tab, false, cx)))
                        .flatten(),
                ),
                Item(
                    Icon::ArrowRight,
                    "Move right".into(),
                    false,
                    right
                        .then(|| run(move |this, _, cx| this.move_tab(tab, true, cx)))
                        .flatten(),
                ),
            ],
            vec![Item(
                Icon::X,
                "Close tab".into(),
                true,
                run(move |this, window, cx| this.close_tab_of(tab, window, cx)),
            )],
        ]
    }

    /// A sidebar page's menu: show it alone or as a split below the open ones, or close its split.
    fn sidebar_items(&self, key: &super::right_stack::Key) -> Vec<Vec<Item>> {
        let (plugin, page) = (key.0.clone(), key.1.clone());
        let alone = {
            let (plugin, page) = (plugin.clone(), page.clone());
            run(move |this, _, cx| this.show_sidebar(&plugin, &page, cx))
        };
        if !self.right.contains(key) {
            return vec![vec![
                Item(
                    Icon::Rows,
                    "Open as a split".into(),
                    false,
                    run(move |this, _, cx| this.split_sidebar(&plugin, &page, cx)),
                ),
                Item(Icon::PanelRight, "Show alone".into(), false, alone),
            ]];
        }
        let key = key.clone();
        let many = self.right.len() > 1;
        let mut items = Vec::new();
        if many {
            items.push(Item(Icon::PanelRight, "Show alone".into(), false, alone));
        }
        items.push(Item(
            Icon::X,
            if many { "Close split" } else { "Close sidebar" }.into(),
            true,
            run(move |this, _, cx| this.close_split(&key, cx)),
        ));
        vec![items]
    }

    /// A pane header's menu: rename it, or give it back its program's name. "Move to tab" follows, with its list.
    fn pane_items(&self, pane: usize, moved: &[PaneAction]) -> Vec<Vec<Item>> {
        let named = self.panes.get(&pane).is_some_and(|p| p.name.is_some());
        let mut items = vec![Item(
            Icon::Pencil,
            "Rename".into(),
            false,
            run(move |this, window, cx| this.start_rename(pane, window, cx)),
        )];
        if named {
            items.push(Item(
                Icon::SquareTerminal,
                "Use automatic name".into(),
                false,
                run(move |this, _, _| {
                    if let Some(pane) = this.panes.get_mut(&pane) {
                        pane.name = None;
                    }
                }),
            ));
        }
        // The empty group puts the hairline above "Move to tab", which `menu_layer` adds after.
        let mut groups = vec![items, Vec::new()];
        if !moved.is_empty() {
            groups.insert(0, self.moved_items(pane, moved));
        }
        groups
    }

    /// The buttons a narrow pane's header moved into its menu, as menu items.
    fn moved_items(&self, pane: usize, moved: &[PaneAction]) -> Vec<Item> {
        let owner = self.opened_by(pane);
        let zoomed = self.terminals.iter().any(|tab| tab.zoomed == Some(pane));
        moved
            .iter()
            .filter_map(|action| {
                Some(match action {
                    PaneAction::SplitRight => Item(
                        Icon::Columns,
                        "Split right".into(),
                        false,
                        run(move |this, window, cx| this.split_pane(pane, Dir::Row, window, cx)),
                    ),
                    PaneAction::SplitDown => Item(
                        Icon::Rows,
                        "Split down".into(),
                        false,
                        run(move |this, window, cx| this.split_pane(pane, Dir::Column, window, cx)),
                    ),
                    PaneAction::Zoom => Item(
                        if zoomed { Icon::Minimize } else { Icon::Maximize },
                        if zoomed { "Restore" } else { "Zoom" }.into(),
                        false,
                        run(move |this, window, cx| this.toggle_zoom(pane, window, cx)),
                    ),
                    PaneAction::Close => Item(
                        Icon::X,
                        "Close pane".into(),
                        true,
                        run(move |this, window, cx| this.close_pane(pane, window, cx)),
                    ),
                    PaneAction::Hide => {
                        let owner = owner?;
                        Item(
                            Icon::EyeOff,
                            "Hide".into(),
                            false,
                            run(move |this, window, cx| this.toggle_zoom(owner, window, cx)),
                        )
                    }
                    PaneAction::Move => {
                        let owner = owner?;
                        let (glyph, label, to) = self.opened_move(owner);
                        Item(
                            glyph,
                            label.into(),
                            false,
                            run(move |this, window, cx| this.agent_view(owner, to, window, cx)),
                        )
                    }
                    PaneAction::Float => {
                        let owner = owner?;
                        Item(
                            Icon::PictureInPicture,
                            "Float over the agent".into(),
                            false,
                            run(move |this, window, cx| {
                                this.agent_view(owner, super::agent_terminals::View::Float, window, cx)
                            }),
                        )
                    }
                })
            })
            .collect()
    }

    /// "Move to tab" and, while it's pointed at, the project's other terminal tabs and a new one beside it.
    fn move_to_tab(&self, pane: usize, open: bool, cx: &mut Context<Self>) -> AnyElement {
        let home = self
            .terminals
            .iter()
            .find(|tab| tab.layout.panes().contains(&pane))
            .map(|tab| tab.id);
        let others: Vec<Item> = self
            .terminals
            .iter()
            .filter(|tab| Some(tab.id) != home)
            .map(|tab| {
                let id = tab.id;
                Item(
                    Icon::SquareTerminal,
                    self.label(id, cx),
                    false,
                    run(move |this, window, cx| this.move_pane_to(pane, Some(id), window, cx)),
                )
            })
            .collect();
        let mut list = Vec::new();
        let some = !others.is_empty();
        list.extend(others.into_iter().map(|item| self.menu_item(item, false, cx)));
        if some {
            list.push(ui::menu_separator());
        }
        list.push(self.menu_item(
            Item(
                Icon::SquarePlus,
                "New tab".into(),
                false,
                run(move |this, window, cx| this.move_pane_to(pane, None, window, cx)),
            ),
            false,
            cx,
        ));
        div()
            .relative()
            .child(
                ui::menu_item(
                    "menu-move-to-tab",
                    Icon::SquareTerminal,
                    "Move to tab",
                    false,
                    false,
                )
                .aria_expanded(open)
                .when(open, |item| item.bg(gpui::rgba(0xffffff14)))
                .child(crate::icons::icon(
                    Icon::ChevronRight,
                    16.0,
                    crate::theme::muted_foreground(),
                ))
                .on_hover(cx.listener(|this, over: &bool, _, cx| {
                    if let Some(menu) = this.menu.as_mut().filter(|_| *over) {
                        menu.moving = true;
                        cx.notify_workspace();
                    }
                }))
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(menu) = this.menu.as_mut() {
                        menu.moving = !menu.moving;
                        cx.notify_workspace();
                    }
                })),
            )
            .when(open, |row| {
                row.child(
                    div()
                        .absolute()
                        .top(px(-5.0))
                        .left(relative(1.0))
                        .pl(px(8.0))
                        .child(ui::menu("move-to-tab", "Move to tab").children(list)),
                )
            })
            .into_any_element()
    }

    /// A project row's menu, as the desktop's without the worktree and editor items.
    fn project_items(&self, path: &std::path::Path) -> Vec<Vec<Item>> {
        let Some(entry) = self.projects.iter().find(|entry| entry.path == path) else {
            return Vec::new();
        };
        let path = path.to_path_buf();
        let at =
            move |action: fn(&mut Workspace, PathBuf, &mut Window, &mut Context<Workspace>)| {
                let path = path.clone();
                run(move |this, window, cx| action(this, path.clone(), window, cx))
            };
        let checking = self.side.checking_git();
        let mut groups = vec![vec![
            Item(
                Icon::SquareTerminal,
                "New tab".into(),
                false,
                at(|this, path, window, cx| {
                    this.switch_project(path, window, cx);
                    this.new_terminal(window, cx);
                }),
            ),
            Item(
                Icon::FileText,
                "Project page".into(),
                false,
                at(|this, path, window, cx| this.open_project_page(path, window, cx)),
            ),
            Item(
                Icon::Wrench,
                "Worktree setup…".into(),
                false,
                at(|this, path, window, cx| this.open_worktree_setup(path, window, cx)),
            ),
            Item(
                Icon::FolderOpen,
                reveal_label().into(),
                false,
                at(|_, path, _, _| reveal(&path)),
            ),
            Item(
                Icon::Copy,
                "Copy path".into(),
                false,
                at(|_, path, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        path.to_string_lossy().into_owned(),
                    ))
                }),
            ),
            Item(
                Icon::RefreshCw,
                "Refresh git status".into(),
                false,
                (!checking)
                    .then(|| at(|this, path, _, cx| this.refresh_git(Some(vec![path]), true, cx)))
                    .flatten(),
            ),
        ]];
        if !entry.child {
            let pinned = entry.pinned;
            let mut items = vec![Item(
                if pinned { Icon::PinOff } else { Icon::Pin },
                if pinned { "Unpin" } else { "Pin to top" }.into(),
                false,
                if pinned {
                    at(|this, path, _, cx| this.pin_project(&path, false, cx))
                } else {
                    at(|this, path, _, cx| this.pin_project(&path, true, cx))
                },
            )];
            // In the Manual sort, a listed project moves one place, as far as the others of its kind go.
            if let Some((up, down)) = self.project_steps(&entry.path) {
                let step = |slot: Option<PathBuf>| {
                    let project = entry.path.clone();
                    slot.and_then(|slot| {
                        run(move |this, _, cx| this.move_project(&project, &slot, cx))
                    })
                };
                items.push(Item(Icon::ArrowUp, "Move up".into(), false, step(up)));
                items.push(Item(Icon::ArrowDown, "Move down".into(), false, step(down)));
            }
            groups.push(items);
        }
        groups.push(vec![Item(
            Icon::Trash,
            "Remove from sidebar".into(),
            true,
            at(|this, path, window, cx| this.request_remove(path, window, cx)),
        )]);
        groups
    }

    /// An agent row's menu: go to it, fork its session in a new tab, copy its session id, or close its pane.
    fn agent_items(&self, pane: usize) -> Vec<Vec<Item>> {
        let Some(agent) = self.agents.get(&pane) else {
            return Vec::new();
        };
        let project = PathBuf::from(&agent.space_id);
        let session = agent.session_id.clone();
        let fork = session.as_deref().and_then(|id| {
            (agent.kind == AgentKind::Claude)
                .then(|| fork_command(wings_agents::history::Agent::Claude, id))
                .flatten()
        });
        let go = project.clone();
        vec![
            vec![
                Item(
                    Icon::SquareTerminal,
                    "Go to pane".into(),
                    false,
                    run(move |this, window, cx| this.go_to_pane(go.clone(), pane, window, cx)),
                ),
                Item(
                    Icon::GitFork,
                    "Fork in new tab".into(),
                    false,
                    fork.and_then(|line| {
                        let project = project.clone();
                        run(move |this, window, cx| {
                            this.switch_project(project.clone(), window, cx);
                            this.new_terminal(window, cx);
                            if let Some(pane) = this.focused_pane() {
                                this.type_into(pane, &format!("{line}\r"), window, cx);
                            }
                        })
                    }),
                ),
                Item(
                    Icon::Copy,
                    "Copy session id".into(),
                    false,
                    session.and_then(|id| {
                        run(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(id.clone()))
                        })
                    }),
                ),
            ],
            vec![Item(
                Icon::X,
                "Close pane".into(),
                true,
                run(move |this, window, cx| {
                    // A pane of a project out of sight is closed there.
                    if !this
                        .terminals
                        .iter()
                        .any(|tab| tab.layout.panes().contains(&pane))
                    {
                        let project = this.agents.get(&pane).map(|a| PathBuf::from(&a.space_id));
                        if let Some(project) = project {
                            this.switch_project(project, window, cx);
                        }
                    }
                    this.close_pane(pane, window, cx);
                }),
            )],
        ]
    }

    /// Takes the pane out of its tab, which closes when it was the last, and puts it beside the focused pane of the tab
    /// `to`, or in a tab of its own, which is then shown, as the desktop's `movePane`.
    pub(super) fn move_pane_to(
        &mut self,
        pane: usize,
        to: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self
            .terminals
            .iter()
            .position(|tab| tab.layout.panes().contains(&pane))
        else {
            return;
        };
        if to == Some(self.terminals[from].id) {
            return;
        }
        let tab = &mut self.terminals[from];
        if tab.zoomed == Some(pane) {
            tab.zoomed = None;
        }
        match std::mem::replace(&mut tab.layout, Layout::Pane(pane)).remove(pane) {
            Some(rest) => {
                if tab.focused == pane {
                    tab.focused = rest.panes()[0];
                }
                tab.layout = rest;
            }
            None => {
                self.terminals.remove(from);
            }
        }
        let id = match to.and_then(|id| self.terminals.iter_mut().find(|tab| tab.id == id)) {
            Some(tab) => {
                tab.layout.split(tab.focused, Dir::Row, pane);
                tab.focused = pane;
                tab.zoomed = None;
                tab.id
            }
            None => {
                let id = self.next_tab;
                self.next_tab += 1;
                self.terminals.push(TerminalTab {
                    id,
                    layout: Layout::Pane(pane),
                    focused: pane,
                    zoomed: None,
                    focus: cx.focus_handle().tab_stop(true),
                });
                id
            }
        };
        self.show(Active::Terminal(id), window, cx);
    }
}

/// What the desktop calls showing a folder in the system's file manager.
fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else {
        "Show in file manager"
    }
}

/// Shows the folder in Finder, selected, or opens it in the file manager elsewhere.
fn reveal(path: &std::path::Path) {
    let mut command = if cfg!(target_os = "macos") {
        let mut open = std::process::Command::new("open");
        open.arg("-R");
        open
    } else {
        std::process::Command::new("xdg-open")
    };
    command.arg(path);
    let path = path.to_path_buf();
    // Waited for off the window's thread, so it leaves no zombie and holds nothing up.
    std::thread::spawn(move || {
        if let Err(error) = command.status() {
            eprintln!("Couldn't show {}: {error}", path.display());
        }
    });
}
