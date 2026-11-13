//! The lists plugins fill under each project in the sidebar with `wings.setProjectSection`: one collapsible section per
//! list, a row for each item, as `apps/desktop/src/components/project-sections.tsx` and `lib/project-sections.ts` draw
//! them. A click on a row opens the view it names, as a tab of that project.
//!
//! Not kept from one launch to the next: which sections are collapsed (the desktop saves that in its workspace file).

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, Role, SharedString, Window, div,
    prelude::*, px, rgb, rgba,
};

use crate::{
    icons::{Icon, icon},
    page::plugin::{SectionIcon, SectionRow, ui::Tone},
    theme, ui,
};

use super::{Active, Workspace, plugin_ui::palette};

/// Rows a section lists before "Show more".
const ROWS_SHOWN: usize = 5;
/// A row draws at most this many levels in.
const DEPTH_MAX: usize = 3;
/// `px-1.5`, the indent a level of nesting adds, and half an icon's width: a child's guide line runs down its ancestors'
/// icon centres.
const ROW_PAD: f32 = 6.0;
const INDENT: f32 = 14.0;
const ICON_CENTER: f32 = 7.0;

/// A section's identity in a project: `plugin:section`.
pub(super) fn section_key(plugin: &str, section: &str) -> String {
    format!("{plugin}:{section}")
}

/// What plugins put under projects, and how the user has the sections.
#[derive(Default)]
pub(super) struct Sections {
    /// By project folder, then `plugin:section`. A section with no rows isn't kept.
    rows: HashMap<(PathBuf, String), Vec<SectionRow>>,
    collapsed: HashSet<(PathBuf, String)>,
    /// Sections whose whole list is showing.
    expanded: HashSet<(PathBuf, String)>,
    /// The sort each section's plugin last confirmed, by `plugin:section`, for every project.
    sorts: HashMap<String, String>,
    /// The sort menu that is open: its project and section.
    menu: Option<(PathBuf, String)>,
}

impl Sections {
    /// Drops what a plugin that stopped listed, so a new start begins with nothing.
    pub(super) fn forget_plugin(&mut self, plugin: &str) {
        let prefix = format!("{plugin}:");
        self.rows.retain(|(_, key), _| !key.starts_with(&prefix));
    }

    /// Sets or, with no rows, removes one section. Whether anything changed.
    pub(super) fn set(&mut self, project: &Path, key: &str, rows: Option<Vec<SectionRow>>) -> bool {
        let at = (project.to_path_buf(), key.to_owned());
        match rows.filter(|rows| !rows.is_empty()) {
            Some(rows) if self.rows.get(&at) == Some(&rows) => false,
            Some(rows) => {
                self.rows.insert(at, rows);
                true
            }
            None => self.rows.remove(&at).is_some(),
        }
    }

    pub(super) fn set_sort(&mut self, key: &str, sort: &str) {
        if self.sorts.get(key).map(String::as_str) != Some(sort) {
            self.sorts.insert(key.to_owned(), sort.to_owned());
        }
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn rows(&self, project: &Path, key: &str) -> Option<&Vec<SectionRow>> {
        self.rows.get(&(project.to_path_buf(), key.to_owned()))
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn sort(&self, key: &str) -> Option<&str> {
        self.sorts.get(key).map(String::as_str)
    }
}

/// The rows in the order they draw, each followed by the rows that name it as their parent and theirs, siblings in the
/// order the plugin sent them: the index of each row and its depth. A row whose parent isn't listed is a top-level row.
/// Rows in or under a loop of parents start at the top instead, so every row appears exactly once.
pub(super) fn nest(rows: &[SectionRow]) -> Vec<(usize, usize)> {
    let ids: HashSet<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    let mut below: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut top = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        match row.parent.as_deref() {
            Some(parent) if parent != row.id && ids.contains(parent) => {
                below.entry(parent).or_default().push(index)
            }
            _ => top.push(index),
        }
    }
    fn walk(
        rows: &[SectionRow],
        below: &HashMap<&str, Vec<usize>>,
        index: usize,
        depth: usize,
        seen: &mut HashSet<usize>,
        out: &mut Vec<(usize, usize)>,
    ) {
        if !seen.insert(index) {
            return;
        }
        out.push((index, depth.min(DEPTH_MAX)));
        for child in below.get(rows[index].id.as_str()).into_iter().flatten() {
            walk(rows, below, *child, depth + 1, seen, out);
        }
    }
    let (mut out, mut seen) = (Vec::new(), HashSet::new());
    for index in top {
        walk(rows, &below, index, 0, &mut seen, &mut out);
    }
    // What no top-level row reaches is in or under a loop of parents.
    for index in 0..rows.len() {
        walk(rows, &below, index, 0, &mut seen, &mut out);
    }
    out
}

/// A row's short reference and what is left of its detail: `#6399 · needs your review` is `#6399` and `needs your
/// review`. The reference is the first `#123` or `!123` word, wherever it is.
pub(super) fn reference(detail: &str) -> (Option<&str>, String) {
    let word = detail.split_whitespace().find(|word| {
        word.strip_prefix(['#', '!'])
            .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
    });
    let Some(word) = word else {
        return (None, detail.to_owned());
    };
    let at = word.as_ptr() as usize - detail.as_ptr() as usize;
    let without = format!("{}{}", &detail[..at], &detail[at + word.len()..]);
    let rest: Vec<&str> = without
        .split(" · ")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    (Some(word), rest.join(" · "))
}

/// A row's title without its leading `[tag]` groups; the original when nothing else is left.
pub(super) fn title(label: &str) -> &str {
    let mut rest = label;
    loop {
        let trimmed = rest.trim_start();
        let Some(inner) = trimmed.strip_prefix('[') else {
            break;
        };
        let Some(end) = inner.find(']') else {
            break;
        };
        rest = &inner[end + 1..];
    }
    let rest = rest.trim_start();
    if rest.is_empty() { label } else { rest }
}

/// What a screen reader says for a row, and the tooltip's text: `label, status, detail`.
fn name(row: &SectionRow) -> String {
    [
        Some(row.label.as_str()),
        row.badge.as_ref().map(|(text, _)| text.as_str()),
        row.detail.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ")
}

fn glyph(icon: SectionIcon) -> Icon {
    match icon {
        SectionIcon::PrOpen => Icon::GitPullRequest,
        SectionIcon::PrMerged => Icon::GitMerge,
        SectionIcon::PrClosed => Icon::GitPullRequestClosed,
        SectionIcon::PrDraft => Icon::GitPullRequestDraft,
        SectionIcon::Todo => Icon::Circle,
        SectionIcon::InProgress => Icon::CircleDotDashed,
        SectionIcon::InReview => Icon::CircleDot,
        SectionIcon::Done => Icon::CircleCheck,
        SectionIcon::Canceled => Icon::CircleX,
    }
}

/// One element of the project list: a project's row, or the sections after a project's last row.
pub(super) enum ListItem<'a> {
    Row(usize),
    Sections(&'a Path, Vec<Group>),
}

/// A section as the sidebar draws it.
pub(super) struct Group {
    key: String,
    plugin: String,
    section: String,
    title: String,
    all: Option<String>,
    sorts: Vec<(String, String)>,
    rows: Vec<SectionRow>,
}

impl Workspace {
    /// The project rows `order` lists (indexes into the projects, in the order the sidebar shows them), with each
    /// project's sections after it and its worktrees: issues, pull requests and schedules belong to the repo, so they
    /// come once after its worktrees.
    pub(super) fn sidebar_rows(&self, order: &[usize], cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.sidebar_slots(order)
            .into_iter()
            .map(|slot| match slot {
                ListItem::Row(index) => self.sidebar_row(index, cx).into_any_element(),
                ListItem::Sections(project, groups) => self.project_sections(project, groups, cx),
            })
            .collect()
    }

    /// What `sidebar_rows` draws for `order`, one slot per element.
    pub(super) fn sidebar_slots(&self, order: &[usize]) -> Vec<ListItem<'_>> {
        let mut slots = Vec::new();
        let mut project: Option<&Path> = None;
        for (at, &index) in order.iter().enumerate() {
            let entry = &self.projects[index];
            if !entry.child {
                project = Some(&entry.path);
            }
            slots.push(ListItem::Row(index));
            let last = !order
                .get(at + 1)
                .is_some_and(|&next| self.projects[next].child);
            if let (true, Some(project)) = (last, project) {
                let groups = self.section_groups(project);
                if !groups.is_empty() {
                    slots.push(ListItem::Sections(project, groups));
                }
            }
        }
        slots
    }

    fn section_groups(&self, project: &Path) -> Vec<Group> {
        self.plugins
            .iter()
            .flat_map(|installed| {
                let plugin = &installed.plugin.manifest;
                plugin
                    .contributes
                    .project_sections
                    .iter()
                    .filter_map(move |section| {
                        let key = section_key(&plugin.id, &section.id);
                        if self.side.section_off(project, &key) {
                            return None;
                        }
                        let rows = self
                            .sections
                            .rows
                            .get(&(project.to_path_buf(), key.clone()))?;
                        Some(Group {
                            key,
                            plugin: plugin.id.clone(),
                            section: section.id.clone(),
                            title: section.title.clone(),
                            all: Some(section.all.clone()).filter(|all| !all.is_empty()),
                            sorts: section
                                .sorts
                                .iter()
                                .map(|sort| (sort.id.clone(), sort.label.clone()))
                                .collect(),
                            rows: rows.clone(),
                        })
                    })
            })
            .collect()
    }

    /// Every section the plugins declare, with whether `project` shows it: what the project's menu turns on and off.
    pub(super) fn section_choices(&self, project: &Path) -> Vec<(String, String, bool)> {
        self.plugins
            .iter()
            .flat_map(|installed| {
                let plugin = &installed.plugin.manifest;
                plugin
                    .contributes
                    .project_sections
                    .iter()
                    .map(move |section| (section_key(&plugin.id, &section.id), section.title.clone()))
            })
            .map(|(key, title)| {
                let on = !self.side.section_off(project, &key);
                (key, title, on)
            })
            .collect()
    }

    /// Turns a plugin's section off under `project`, or back on. Off drops its rows, and the plugins hear it with the
    /// project, so the one that fills the section stops working for it.
    pub(super) fn set_section_off(
        &mut self,
        project: &Path,
        key: &str,
        off: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.side.set_section_off(project, key, off) {
            return;
        }
        if off {
            self.sections.set(project, key, None);
        }
        self.save(cx);
        cx.notify();
    }

    /// The project's sections, or nothing while no plugin has put anything under it.
    fn project_sections(
        &self,
        project: &Path,
        groups: Vec<Group>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hover = SharedString::from(format!("sections-{}", project.display()));
        div()
            .id(SharedString::from(format!(
                "project-sections-{}",
                project.display()
            )))
            .group(hover.clone())
            .flex()
            .flex_col()
            .pt(px(1.0))
            .pb(px(4.0))
            .pl(px(20.0))
            .children(
                groups
                    .into_iter()
                    .map(|group| self.section_group(project, group, &hover, cx)),
            )
            .into_any_element()
    }

    fn section_group(
        &self,
        project: &Path,
        group: Group,
        hover: &SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let at = (project.to_path_buf(), group.key.clone());
        let open = !self.sections.collapsed.contains(&at);
        let all = self.sections.expanded.contains(&at);
        let nested = nest(&group.rows);
        let shown = if all {
            nested.len()
        } else {
            nested.len().min(ROWS_SHOWN)
        };
        let hidden = nested.len() - shown;
        let id = SharedString::from(format!("section-{}-{}", project.display(), group.key));
        let count = group.rows.len();
        let label = format!(
            "{}, {count} {}",
            group.title,
            if count == 1 { "item" } else { "items" }
        );
        let toggle = {
            let at = at.clone();
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                if !this.sections.collapsed.remove(&at) {
                    this.sections.collapsed.insert(at.clone());
                }
                cx.notify();
            })
        };
        let chevron = div()
            .flex_none()
            .when(open, |chevron| {
                chevron
                    .opacity(0.0)
                    .group_hover(hover.clone(), |style| style.opacity(1.0))
            })
            .child(icon(
                if open {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                },
                12.0,
                theme::MUTED_FOREGROUND,
            ));
        let header = div()
            .flex()
            .items_center()
            .child(
                div()
                    .id(SharedString::from(format!("{id}-toggle")))
                    .role(Role::Button)
                    .aria_label(label.clone())
                    .aria_expanded(open)
                    .h(px(24.0))
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(6.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .text_size(px(11.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .hover(|style| style.bg(rgba(theme::HOVER)))
                    .focusable()
                    .focus_visible(|style| style.bg(rgba(theme::HOVER)))
                    .on_click(toggle)
                    .child(div().truncate().child(group.title.clone()))
                    .child(chevron),
            )
            .when(!group.sorts.is_empty(), |header| {
                header.child(self.sort_button(project, &group, &id, cx))
            })
            .when_some(group.all.clone(), |header, view| {
                let (plugin, target, lower) = (
                    group.plugin.clone(),
                    project.to_path_buf(),
                    group.title.to_lowercase(),
                );
                header.child(
                    div()
                        .id(SharedString::from(format!("{id}-all")))
                        .role(Role::Button)
                        .aria_label(format!("All {lower}"))
                        .h(px(24.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .px(px(6.0))
                        .rounded(px(6.0))
                        .cursor_pointer()
                        .text_size(px(11.0))
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .hover(|style| {
                            style
                                .bg(rgba(theme::HOVER))
                                .text_color(rgb(theme::FOREGROUND))
                        })
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            let new_tab = event.modifiers().secondary(cx);
                            this.open_section_view(
                                target.clone(),
                                &plugin,
                                &view,
                                None,
                                new_tab,
                                window,
                                cx,
                            );
                        }))
                        .child("All"),
                )
            });
        let rows = open.then(|| {
            let mut list = div().flex().flex_col();
            for &(index, depth) in &nested[..shown] {
                list = list.child(self.section_row(project, &group, index, depth, &id, cx));
            }
            if hidden > 0 || (all && nested.len() > ROWS_SHOWN) {
                let at = at.clone();
                list = list.child(
                    div()
                        .id(SharedString::from(format!("{id}-more")))
                        .role(Role::Button)
                        .h(px(28.0))
                        .flex()
                        .items_center()
                        .px(px(6.0))
                        .pl(px(18.0))
                        .rounded(px(6.0))
                        .cursor_pointer()
                        .text_size(px(12.5))
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .hover(|style| {
                            style
                                .bg(rgba(theme::HOVER))
                                .text_color(rgb(theme::FOREGROUND))
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if !this.sections.expanded.remove(&at) {
                                this.sections.expanded.insert(at.clone());
                            }
                            cx.notify();
                        }))
                        .child(if all {
                            "Show less".to_owned()
                        } else {
                            format!("Show {hidden} more")
                        }),
                );
            }
            list
        });
        div()
            .id(id)
            .role(Role::Group)
            .aria_label(group.title.clone())
            .child(header)
            .children(rows)
            .into_any_element()
    }

    fn sort_button(
        &self,
        project: &Path,
        group: &Group,
        id: &SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let at = (project.to_path_buf(), group.key.clone());
        let open = self.sections.menu.as_ref() == Some(&at);
        let current = self.sections.sorts.get(&group.key);
        let lower = group.title.to_lowercase();
        let button = ui::icon_button(
            SharedString::from(format!("{id}-sort")),
            Icon::ArrowUpDown,
            format!("Sort {lower}"),
            ui::ButtonSize::Xs,
        )
        .on_click({
            let at = at.clone();
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.sections.menu = (this.sections.menu.as_ref() != Some(&at)).then(|| at.clone());
                cx.notify();
            })
        });
        let items: Vec<AnyElement> = group
            .sorts
            .iter()
            .enumerate()
            .map(|(index, (sort, label))| {
                let (plugin, section, sort) =
                    (group.plugin.clone(), group.section.clone(), sort.clone());
                ui::select_item(
                    ElementId::NamedInteger(format!("{id}-sort-item").into(), index as u64),
                    label.clone(),
                    current == Some(&sort),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.pick_section_sort(&plugin, &section, &sort, cx)
                }))
                .into_any_element()
            })
            .collect();
        div()
            .flex_none()
            .child(button)
            .children(
                self.popover_phase(format!("{id}-sort-menu"), open, cx)
                    .map(|phase| {
                        ui::motion::with_phase(phase, || {
                            ui::popover(ui::select_menu(items).mt(px(26.0)).on_mouse_down_out(
                                cx.listener(|this, _, _, cx| {
                                    this.sections.menu = None;
                                    cx.notify();
                                }),
                            ))
                        })
                    }),
            )
            .into_any_element()
    }

    fn section_row(
        &self,
        project: &Path,
        group: &Group,
        index: usize,
        depth: usize,
        id: &SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = &group.rows[index];
        let tone = row.badge.as_ref().map_or(Tone::Neutral, |(_, tone)| *tone);
        let color = palette(tone).text;
        let filled = matches!(tone, Tone::Success | Tone::Danger | Tone::Merged);
        let mark = match row.icon {
            Some(row_icon) => icon(glyph(row_icon), 14.0, color).into_any_element(),
            None => div()
                .size(px(6.0))
                .flex_none()
                .rounded_full()
                .border_1()
                .border_color(rgb(color))
                .when(filled, |dot| dot.bg(rgb(color)))
                .into_any_element(),
        };
        let reference = row
            .detail
            .as_deref()
            .and_then(|detail| reference(detail).0.map(str::to_owned));
        let (target, plugin, view, args) = (
            project.to_path_buf(),
            group.plugin.clone(),
            row.view.clone(),
            row.args.clone(),
        );
        let label = name(row);
        let line = SharedString::from(format!("{id}-row-{}", row.id));
        let hover = SharedString::from(format!("{line}-hover"));
        let guide = ROW_PAD + (depth as f32) * INDENT;
        div()
            .relative()
            .children((0..depth).map(|level| {
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(ROW_PAD + level as f32 * INDENT + ICON_CENTER))
                    .w(px(1.0))
                    .bg(rgba(theme::HAIRLINE_STRONG))
            }))
            .child(
                div()
                    .id(line)
                    .group(hover.clone())
                    .role(Role::Button)
                    .aria_label(label)
                    .h(px(28.0))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(ROW_PAD))
                    .when(depth > 0, |row| row.pl(px(guide)))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgba(theme::HOVER)))
                    .focusable()
                    .focus_visible(|style| style.bg(rgba(theme::HOVER)))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        let new_tab = event.modifiers().secondary(cx);
                        this.open_section_view(
                            target.clone(),
                            &plugin,
                            &view,
                            args.clone(),
                            new_tab,
                            window,
                            cx,
                        );
                    }))
                    .child(mark)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(12.5))
                            .text_color(rgb(theme::FOREGROUND))
                            .child(title(&row.label).to_owned()),
                    )
                    .when_some(reference, |line, reference| {
                        line.child(
                            div()
                                .flex_none()
                                .opacity(0.0)
                                .group_hover(hover, |style| style.opacity(1.0))
                                .text_size(px(10.0))
                                .font_weight(FontWeight::NORMAL)
                                .text_color(rgb(theme::MUTED_FOREGROUND))
                                .child(reference),
                        )
                    }),
            )
            .into_any_element()
    }

    /// Opens the view a row names as a tab of `project`, showing that project first. `new_tab` opens another tab even
    /// where the view has one.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_section_view(
        &mut self,
        project: PathBuf,
        plugin: &str,
        view: &str,
        args: Option<serde_json::Value>,
        new_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.switch_project(project, window, cx);
        let opened = if new_tab {
            self.new_plugin_tab(plugin, view, args, cx)
                .map(|id| {
                    self.show(Active::Plugin(id), window, cx);
                })
                .is_some()
        } else {
            self.open_plugin_view(plugin, view, args, window, cx)
        };
        if !opened {
            self.show_notice(
                "Wings".into(),
                format!("{plugin} has no native page {view} yet."),
                cx,
            );
        }
        cx.notify();
    }

    /// The user picked a sort: the section shows it at once, and the plugin sorts its rows and confirms.
    pub(super) fn pick_section_sort(
        &mut self,
        plugin: &str,
        section: &str,
        sort: &str,
        cx: &mut Context<Self>,
    ) {
        self.sections.menu = None;
        self.sections.set_sort(&section_key(plugin, section), sort);
        if let Some(installed) = self.plugins.iter().find(|installed| installed.id == plugin) {
            installed.host.hear_section_sort(section, sort);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, parent: Option<&str>) -> SectionRow {
        SectionRow {
            id: id.into(),
            label: id.into(),
            detail: None,
            badge: None,
            icon: None,
            parent: parent.map(Into::into),
            view: "board".into(),
            args: None,
        }
    }

    #[test]
    fn rows_nest_under_their_parent_in_the_order_sent_and_a_loop_or_a_missing_parent_still_shows_every_row()
     {
        let rows = [
            row("a", None),
            row("b", Some("a")),
            row("c", Some("b")),
            row("d", Some("a")),
            row("e", Some("nowhere")),
            row("x", Some("y")),
            row("y", Some("x")),
        ];
        let nested = nest(&rows);
        let drawn: Vec<(&str, usize)> = nested
            .iter()
            .map(|&(i, depth)| (rows[i].id.as_str(), depth))
            .collect();
        assert_eq!(
            drawn,
            [
                ("a", 0),
                ("b", 1),
                ("c", 2),
                ("d", 1),
                ("e", 0),
                ("x", 0),
                ("y", 1)
            ]
        );
    }

    #[test]
    fn a_row_draws_at_most_three_levels_in() {
        let rows = [
            row("a", None),
            row("b", Some("a")),
            row("c", Some("b")),
            row("d", Some("c")),
            row("e", Some("d")),
        ];
        let depths: Vec<usize> = nest(&rows).iter().map(|&(_, depth)| depth).collect();
        assert_eq!(depths, [0, 1, 2, 3, 3]);
    }

    #[test]
    fn a_reference_is_the_first_number_word_and_the_rest_is_what_is_left_of_the_detail() {
        assert_eq!(
            reference("#6399 · needs your review"),
            (Some("#6399"), "needs your review".into())
        );
        assert_eq!(
            reference("Defect #185638"),
            (Some("#185638"), "Defect".into())
        );
        assert_eq!(reference("!42"), (Some("!42"), String::new()));
        assert_eq!(reference("in #7a and 12"), (None, "in #7a and 12".into()));
    }

    #[test]
    fn a_title_loses_its_leading_tags_unless_nothing_else_is_left() {
        assert_eq!(title("[ui] [wip] First issue"), "First issue");
        assert_eq!(title("Fix [x] later"), "Fix [x] later");
        assert_eq!(title("[only]"), "[only]");
    }

    #[test]
    fn an_empty_section_is_taken_away_and_a_repeat_changes_nothing() {
        let mut sections = Sections::default();
        let project = Path::new("/work");
        assert!(sections.set(project, "p:s", Some(vec![row("a", None)])));
        assert!(!sections.set(project, "p:s", Some(vec![row("a", None)])));
        assert!(sections.set(project, "p:s", Some(Vec::new())));
        assert!(!sections.set(project, "p:s", None));
    }
}
