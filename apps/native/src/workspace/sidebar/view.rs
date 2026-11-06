//! Drawing the sidebar: the desktop's project list with its header buttons, rows, git counts and "Older" group, the edge
//! you drag to resize it, and the question before a project with tabs open is removed.

use std::{collections::HashSet, path::PathBuf};

use gpui::{
    AnyElement, Context, DispatchPhase, Div, ElementId, FontWeight, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Orientation, Role, SharedString, canvas, div,
    prelude::*, px, rgb, rgba,
};
use wings_spaces::{spaces::GitStatus, worktree::slug};

use crate::{
    agents,
    icons::{Icon, icon},
    theme,
    ui::{self, ButtonSize, Variant},
};

use crate::workspace::{
    Workspace,
    context_menu::MenuTarget,
    project::{SIDEBAR_ACCENT, SIDEBAR_FOREGROUND},
};

use super::{
    now_ms,
    order::{self, Context as OrderBy, Ordered, Sort},
};

/// The width of the strip at the sidebar's edge that takes the drag.
const EDGE: f32 = 8.0;

impl Workspace {
    /// The docked sidebar, as wide as it was dragged.
    pub(in crate::workspace) fn sidebar_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        self.sidebar_column(cx)
            .w(px(self.side.width))
            .child(self.resize_edge(cx))
            .into_any_element()
    }

    /// Everything the sidebar lists, for the docked sidebar and the peek: the project list scrolls, and the plugins and
    /// agents stay at the bottom, as in the desktop sidebar.
    pub(super) fn sidebar_column(&self, cx: &mut Context<Self>) -> Div {
        div()
            .relative()
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            // The desktop's `nav px-2 pb-2`, the Projects heading, the list, the sections under it, then Settings in
            // `mx-2 mt-1` above the sidebar's `pb-2`.
            .child(div().px(px(8.0)).pb(px(8.0)).child(self.inbox_row(cx)))
            .child(self.projects_header(cx))
            .child(self.project_list(cx))
            .children(self.plugin_list(cx))
            .children(self.agent_list(cx))
            .child(
                div()
                    .mx(px(8.0))
                    .mt(px(4.0))
                    .mb(px(8.0))
                    .child(self.settings_row(cx)),
            )
    }

    fn project_order(&self) -> Ordered {
        let busy: HashSet<PathBuf> = self
            .agents
            .values()
            .map(|agent| PathBuf::from(&agent.space_id))
            .collect();
        order::order(
            &self.projects,
            &OrderBy {
                sort: self.side.sort,
                last_active: &self.side.last_active,
                now: now_ms(),
                active: &self.config.project,
                busy: &busy,
            },
        )
    }

    fn project_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let Ordered { shown, older } = self.project_order();
        let tops = older
            .iter()
            .filter(|&&row| !self.projects[row].child)
            .count();
        let open = self.side.older_open;
        div()
            .id("sidebar")
            .role(Role::Navigation)
            .aria_label("Projects")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(1.0))
            .px(px(8.0))
            .pb(px(8.0))
            .overflow_y_scroll()
            .children(self.sidebar_rows(&shown, cx))
            .when(tops > 0, |list| {
                list.child(
                    div()
                        .id("older")
                        .role(Role::Button)
                        .aria_label(format!("Older, {tops}"))
                        .mt(px(4.0))
                        .h(px(24.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .px(px(8.0))
                        .rounded(px(6.0))
                        .cursor_pointer()
                        .text_size(px(11.0))
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .hover(|style| style.bg(rgba(theme::HOVER)))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_older(cx)))
                        .child(icon(
                            if open {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            },
                            12.0,
                            theme::MUTED_FOREGROUND,
                        ))
                        .child("Older")
                        .child(tops.to_string()),
                )
            })
            .when(open, |list| list.children(self.sidebar_rows(&older, cx)))
            .into_any_element()
    }

    /// "Projects" and its buttons: refresh the git counts, sort, add.
    fn projects_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let checking = self.side.git.checking;
        let sorts = Sort::ALL.map(|sort| {
            ui::select_item(
                SharedString::from(format!("sort-{}", sort.key())),
                sort.label(),
                sort == self.side.sort,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_sort(sort, cx)))
            .into_any_element()
        });
        // The desktop's `h-8 pr-2 pl-4`, above the list rather than in it, so it stays put while the list scrolls.
        div()
            .h(px(32.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .pl(px(16.0))
            .pr(px(8.0))
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(theme::MUTED_FOREGROUND))
            .child("Projects")
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        ui::icon_button(
                            "refresh-git",
                            Icon::RefreshCw,
                            if checking {
                                "Checking git status…"
                            } else {
                                "Refresh git status"
                            },
                            ButtonSize::Xs,
                        )
                        .when(checking, |button| button.opacity(0.5))
                        .when(!checking, |button| {
                            button.on_click(
                                cx.listener(|this, _, _, cx| this.refresh_git(None, true, cx)),
                            )
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                ui::icon_button(
                                    "sort-projects",
                                    Icon::ArrowUpDown,
                                    "Sort projects",
                                    ButtonSize::Xs,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.side.sort_menu = !this.side.sort_menu;
                                        cx.notify();
                                    },
                                )),
                            )
                            .when(self.side.sort_menu, |column| {
                                column.child(ui::popover(
                                    ui::select_menu(sorts).mt(px(28.0)).on_mouse_down_out(
                                        cx.listener(|this, _, _, cx| {
                                            this.side.sort_menu = false;
                                            cx.notify();
                                        }),
                                    ),
                                ))
                            }),
                    )
                    .child(
                        ui::icon_button("add-project", Icon::Plus, "Add project", ButtonSize::Xs)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.choose_project(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// A project, as the desktop's row: its name and branch, the git counts, and the agents it runs. Pointing at it swaps
    /// the agents' count or pin for the buttons to pin and remove it.
    pub(in crate::workspace) fn sidebar_row(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entry = &self.projects[index];
        let active = entry.path == self.config.project;
        let states = self.project_agents(&entry.path);
        let agents = states.len();
        let glyph = match agents::roll_up(states) {
            Some(state) => div()
                .size(px(16.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(ui::status_dot(("project-dot", index), state))
                .into_any_element(),
            None => project_icon(entry.child),
        };
        let status = self.side.git.status.get(&entry.path);
        let branch = status
            .and_then(|status| status.branch.clone())
            .or_else(|| entry.branch.clone());
        let (title, subtitle) = if entry.child {
            worktree_title(&entry.name, branch)
        } else {
            (entry.name.clone(), branch)
        };
        let counts = status.and_then(|status| git_counts(index, status));
        let (beside_title, beside_branch) = if subtitle.is_some() {
            (None, counts)
        } else {
            (counts, None)
        };
        let hovered = self.side.hovered == Some(index);
        let (shown, pinned) = (entry.path.clone(), entry.pinned);
        let (pin_it, remove_it) = (entry.path.clone(), entry.path.clone());
        let page_of = entry.path.clone();
        let menu_of = entry.path.clone();
        div()
            .id(("project", index))
            .role(Role::Button)
            .aria_label(entry.name.clone())
            .aria_selected(active)
            .relative()
            .rounded(px(theme::RADIUS))
            .cursor_pointer()
            .when(active, |row| row.bg(rgba(SIDEBAR_ACCENT)))
            .when(!active, |row| {
                row.hover(|style| style.bg(rgba(theme::HOVER)))
            })
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                let next = if *over {
                    Some(index)
                } else {
                    this.side.hovered.filter(|&at| at != index)
                };
                if this.side.hovered != next {
                    this.side.hovered = next;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.switch_project(shown.clone(), window, cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.open_menu(MenuTarget::Project(menu_of.clone()), event.position, cx)
                }),
            )
            .child(
                // The desktop's `Item`, with its transparent 1px border: 42px with a branch line.
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .border_1()
                    .border_color(rgba(0))
                    .py(px(3.0))
                    .pr(px(if hovered { 86.0 } else { 32.0 }))
                    .pl(px(if entry.child { 24.0 } else { 8.0 }))
                    .child(glyph)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .truncate()
                                            .text_size(px(13.0))
                                            .line_height(px(18.0))
                                            .font_weight(if active {
                                                FontWeight::SEMIBOLD
                                            } else {
                                                FontWeight::MEDIUM
                                            })
                                            .text_color(rgb(SIDEBAR_FOREGROUND))
                                            .child(title),
                                    )
                                    .children(beside_title),
                            )
                            .when_some(subtitle, |text, branch| {
                                text.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .flex_1()
                                                .truncate()
                                                .text_size(px(11.0))
                                                .line_height(px(16.0))
                                                .text_color(rgb(theme::MUTED_FOREGROUND))
                                                .child(branch),
                                        )
                                        .children(beside_branch),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(6.0))
                    .flex()
                    .items_center()
                    .when(!hovered, |slot| {
                        slot.child(
                            div()
                                .when(agents > 0, |slot| {
                                    slot.px(px(4.0))
                                        .text_size(px(11.0))
                                        .text_color(rgb(theme::MUTED_FOREGROUND))
                                        .child(agents.to_string())
                                })
                                .when(agents == 0 && pinned, |slot| {
                                    slot.child(
                                        icon(Icon::Pin, 12.0, theme::MUTED_FOREGROUND)
                                            .opacity(0.7)
                                            .mx(px(4.0)),
                                    )
                                }),
                        )
                    })
                    .when(hovered, |slot| {
                        slot.child(
                            ui::icon_button(
                                ("project-page", index),
                                Icon::FileText,
                                "Project page",
                                ButtonSize::Xs,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.open_project_page(page_of.clone(), window, cx)
                                },
                            )),
                        )
                        .when(!entry.child, |buttons| {
                            buttons.child(
                                ui::icon_button(
                                    ("pin-project", index),
                                    if pinned { Icon::PinOff } else { Icon::Pin },
                                    if pinned { "Unpin" } else { "Pin to top" },
                                    ButtonSize::Xs,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.pin_project(&pin_it, !pinned, cx),
                                )),
                            )
                        })
                        .child(
                            ui::icon_button(
                                ("remove-project", index),
                                Icon::X,
                                "Remove from sidebar",
                                ButtonSize::Xs,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.request_remove(remove_it.clone(), window, cx)
                                },
                            )),
                        )
                    }),
            )
            .into_any_element()
    }

    /// The edge you drag. Invisible until you point at it. The window listens for the pointer while it is down, so the
    /// drag goes on wherever the pointer is.
    fn resize_edge(&self, cx: &mut Context<Self>) -> AnyElement {
        let dragging = self.side.resize.is_some();
        let entity = cx.entity();
        div()
            .id("sidebar-edge")
            .group("sidebar-edge")
            .role(Role::Splitter)
            .aria_label("Resize sidebar")
            .aria_orientation(Orientation::Vertical)
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(EDGE))
            .cursor_col_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.start_resize(event.position.x, cx)
                }),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w(px(1.0))
                    .when(dragging, |line| line.bg(rgba(theme::HAIRLINE_STRONG)))
                    .group_hover("sidebar-edge", |style| {
                        style.bg(rgba(theme::HAIRLINE_STRONG))
                    }),
            )
            .when(dragging, |edge| {
                edge.child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, window, _| {
                            let moved = entity.clone();
                            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble {
                                    moved.update(cx, |this, cx| {
                                        if event.dragging() {
                                            this.resize_to(event.position.x, cx);
                                        } else {
                                            this.end_resize(cx);
                                        }
                                    });
                                }
                            });
                            let released = entity.clone();
                            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble {
                                    released.update(cx, |this, cx| this.end_resize(cx));
                                }
                            });
                        },
                    )
                    .absolute()
                    .size_full(),
                )
            })
            .into_any_element()
    }

    /// Asks before a project with tabs open is removed, which closes them and any agents running in them.
    pub(in crate::workspace) fn remove_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (project, focus) = self.side.removing.as_ref()?;
        let name = self
            .projects
            .iter()
            .find(|entry| &entry.path == project)
            .map_or_else(
                || project.to_string_lossy().into_owned(),
                |p| p.name.clone(),
            );
        let tabs = self.open_tabs(project);
        let (backdrop, panel) = ui::dialog("remove-project", "Remove project", focus, 400.0, 176.0);
        let project = project.clone();
        Some(
            backdrop
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.cancel_remove(window, cx)),
                )
                .child(
                    panel
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                this.cancel_remove(window, cx);
                            }
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .p(px(20.0))
                                .child(
                                    div()
                                        .text_size(px(15.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(format!("Remove {name}?")),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(13.0))
                                        .text_color(rgb(theme::MUTED_FOREGROUND))
                                        .child(format!(
                                            "Its {tabs} open tab{} will close, running agents included. The folder stays where it is.",
                                            if tabs == 1 { "" } else { "s" }
                                        )),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap(px(8.0))
                                        .child(
                                            ui::button(
                                                "cancel-remove",
                                                "Cancel",
                                                Variant::Secondary,
                                                false,
                                                "Cancel",
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.cancel_remove(window, cx)
                                            })),
                                        )
                                        .child(
                                            ui::button(
                                                "confirm-remove",
                                                "Remove",
                                                Variant::Destructive,
                                                false,
                                                "Remove",
                                            )
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.remove_project(project.clone(), window, cx)
                                            })),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// A folder for a project, a branch for its worktrees, as in the desktop sidebar.
fn project_icon(worktree: bool) -> AnyElement {
    icon(
        if worktree {
            Icon::GitBranch
        } else {
            Icon::Folder
        },
        16.0,
        theme::MUTED_FOREGROUND,
    )
    .into_any_element()
}

/// What a worktree row is titled and whether it still needs a branch line: a folder that is just the branch spelled as
/// a path says nothing more, so the branch alone is the title.
pub(super) fn worktree_title(name: &str, branch: Option<String>) -> (String, Option<String>) {
    match branch {
        Some(branch) if slug(&branch) == name => (branch, None),
        branch => (name.to_owned(), branch),
    }
}

/// The desktop's `GitCounts`, tinted: changed files, commits to push and commits to pull.
fn git_counts(index: usize, status: &GitStatus) -> Option<AnyElement> {
    let plural = |n: u32, noun: &str| format!("{n} {noun}{}", if n == 1 { "" } else { "s" });
    let chips: Vec<AnyElement> = [
        (
            Icon::Pencil,
            status.changed,
            theme::MUTED_FOREGROUND,
            plural(status.changed, "changed file"),
        ),
        (
            Icon::ArrowUp,
            status.ahead,
            theme::INFO,
            format!("{} to push", plural(status.ahead, "commit")),
        ),
        (
            Icon::ArrowDown,
            status.behind,
            theme::WORKING,
            format!(
                "{} to pull, as of the last fetch",
                plural(status.behind, "commit")
            ),
        ),
    ]
    .into_iter()
    .filter(|(_, value, ..)| *value > 0)
    .map(|(glyph, value, color, label)| {
        div()
            .id(ElementId::Name(
                format!("git-{index}-{}", glyph as usize).into(),
            ))
            .role(Role::Label)
            .aria_label(label)
            .flex()
            .items_center()
            .gap(px(2.0))
            .text_size(px(11.0))
            .line_height(px(16.0))
            .text_color(rgb(color))
            .child(icon(glyph, 12.0, color))
            .child(value.to_string())
            .into_any_element()
    })
    .collect();
    (!chips.is_empty()).then(|| {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .children(chips)
            .into_any_element()
    })
}
