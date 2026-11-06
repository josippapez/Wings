//! The desktop's right-click menus on tabs, pane headers, project rows and agent rows (`title-bar.tsx`, `pane-grid.tsx`,
//! `sidebar.tsx`), with the items whose actions the native app has.

use std::path::PathBuf;

use gpui::{
    AnyElement, ClipboardItem, Context, ElementId, Pixels, Point, SharedString, Window, div,
    prelude::*, px, relative,
};

use crate::{
    agents::AgentKind,
    icons::Icon,
    layout::{Dir, Layout},
    ui,
};

use super::{Active, TerminalTab, Workspace, history::fork_command};

/// What a menu was opened on.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum MenuTarget {
    Tab(Active),
    /// A pane, by its header.
    Pane(usize),
    Project(PathBuf),
    /// An agent in the sidebar, by its pane.
    Agent(usize),
}

pub(super) struct Menu {
    at: Point<Pixels>,
    pub(super) target: MenuTarget,
    /// The pane menu's "Move to tab" list is open beside it.
    moving: bool,
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
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// The open menu, above everything else, where it was opened.
    pub(super) fn menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let groups = match &menu.target {
            MenuTarget::Tab(tab) => self.tab_items(*tab),
            MenuTarget::Pane(pane) => self.pane_items(*pane),
            MenuTarget::Project(path) => self.project_items(path),
            MenuTarget::Agent(pane) => self.agent_items(*pane),
        };
        let mut children = Vec::new();
        for (index, group) in groups.into_iter().enumerate() {
            if index > 0 {
                children.push(ui::menu_separator());
            }
            children.extend(group.into_iter().map(|item| self.menu_item(item, cx)));
        }
        if let MenuTarget::Pane(pane) = menu.target {
            children.push(self.move_to_tab(pane, menu.moving, cx));
        }
        Some(ui::context_menu(
            menu.at,
            ui::menu("context-menu", "Menu")
                .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_menu(cx)))
                .children(children),
        ))
    }

    fn menu_item(
        &self,
        Item(glyph, label, destructive, action): Item,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = ElementId::Name(format!("menu-{label}").into());
        let disabled = action.is_none();
        ui::menu_item(id, glyph, label, destructive, disabled)
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if let Some(menu) = this.menu.as_mut().filter(|menu| *over && menu.moving) {
                    menu.moving = false;
                    cx.notify();
                }
            }))
            .when_some(action, |item, action| {
                item.on_click(cx.listener(move |this, _, window, cx| {
                    this.menu = None;
                    action(this, window, cx);
                    cx.notify();
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

    /// A pane header's menu: rename it, or give it back its program's name. "Move to tab" follows, with its list.
    fn pane_items(&self, pane: usize) -> Vec<Vec<Item>> {
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
        vec![items, Vec::new()]
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
        list.extend(others.into_iter().map(|item| self.menu_item(item, cx)));
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
                    crate::theme::MUTED_FOREGROUND,
                ))
                .on_hover(cx.listener(|this, over: &bool, _, cx| {
                    if let Some(menu) = this.menu.as_mut().filter(|_| *over) {
                        menu.moving = true;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(menu) = this.menu.as_mut() {
                        menu.moving = !menu.moving;
                        cx.notify();
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
            groups.push(vec![Item(
                if pinned { Icon::PinOff } else { Icon::Pin },
                if pinned { "Unpin" } else { "Pin to top" }.into(),
                false,
                if pinned {
                    at(|this, path, _, cx| this.pin_project(&path, false, cx))
                } else {
                    at(|this, path, _, cx| this.pin_project(&path, true, cx))
                },
            )]);
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
