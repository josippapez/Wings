//! A project's page: its facts, then sections and plugin blocks in the order you put them, with "+ Add" between, as
//! `apps/desktop/src/components/project-page/` draws it. What it shows is the desktop's `projects/<id>/info.json`, which
//! agents write through Wings' MCP tools and which both apps read and write with the same code (`wings-spaces`): every
//! change is an `Op` applied to what is on disk at that moment, so an agent's write and the page's never lose each other.
//! The page looks at the file's time once a second while it is shown, and reads it again when it changed.
//!
//! The desktop also stores images pasted into a section and draws them; here an image shows as its alt text.

mod facts;
mod items;
mod merge;
mod tab;
#[cfg(all(test, feature = "test-support"))]
mod tests;

use std::{
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, Render, Role, ScrollHandle, SharedString,
    Subscription, Task, Window, div, prelude::*, px, rgb, rgba,
};
use wings_spaces::project_info::{
    Info, Item, MAX_BLOCKS, MAX_SECTIONS, Op, Place, Store, dir_name,
};

use crate::{
    icons::{Icon, icon},
    installed::BlockPage,
    page::PageView,
    text_field::{FieldEvent, TextField},
    theme, ui,
};

use self::{
    facts::FactState,
    items::SectionEdit,
    merge::{Match, clamp_match, find_matches, fresh_title, step_match},
};

/// How often the file is looked at while the page is shown.
const POLL: Duration = Duration::from_secs(1);
/// How long typing in the search field waits before the page is searched.
const SEARCH_PAUSE: Duration = Duration::from_millis(150);

/// Where the project pages are kept, which the window opens them from.
#[derive(Clone)]
pub struct InfoStore {
    pub store: Arc<Store>,
    /// `<data>/projects`.
    pub root: PathBuf,
}

impl InfoStore {
    /// The store in the desktop's data folder `data`.
    pub fn open(data: &std::path::Path) -> Self {
        let root = data.join("projects");
        Self {
            store: Arc::new(Store::new(root.clone(), data.join("plugin-storage"))),
            root,
        }
    }
}

pub struct ProjectPage {
    /// The project's id: its folder.
    id: String,
    name: SharedString,
    path: SharedString,
    branch: Option<SharedString>,
    info: InfoStore,
    data: Option<Arc<Info>>,
    unreadable: Option<SharedString>,
    /// The file's modification time when it was last read, to tell a change made by someone else.
    seen: Option<SystemTime>,
    /// Every read or change takes a number, and an answer older than the newest one shown is dropped.
    ticket: u64,
    landed: u64,
    visible: bool,
    blocks: Vec<BlockPage>,
    scroll: ScrollHandle,
    /// A match to scroll to at the next frame: the index of the page's child that holds it.
    scroll_to: Option<Match>,
    search: Entity<TextField>,
    applied: String,
    at: usize,
    typing: Option<Task<()>>,
    facts: FactState,
    /// The section editors open, by item id.
    editors: HashMap<String, SectionEdit>,
    /// The Markdown of each section, parsed once per text.
    markdown: HashMap<String, (String, Rc<Vec<crate::markdown::Block>>)>,
    /// The section just added, which opens for writing.
    fresh: Option<String>,
    /// The item waiting for "Delete" to be confirmed.
    removing: Option<String>,
    /// Where the "+ Add" menu is open.
    menu: Option<Place>,
    block_views: HashMap<String, Entity<PageView>>,
    error: Option<SharedString>,
    ticks: u32,
    _subs: Vec<Subscription>,
    _poll: Task<()>,
}

/// The project's folder with the home folder as `~`.
fn tilde(path: &str) -> String {
    match std::env::home_dir() {
        Some(home) => match std::path::Path::new(path).strip_prefix(&home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.to_owned(),
        },
        None => path.to_owned(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A search match's tint: any match is tinted, the one Enter is on more strongly.
fn mark_style<T: gpui::Styled>(element: T, mark: Option<bool>) -> T {
    match mark {
        Some(true) => element.bg(rgba(0xece9e438)),
        Some(false) => element.bg(rgba(0xece9e41a)),
        None => element,
    }
}

impl ProjectPage {
    pub fn new(
        project: &std::path::Path,
        name: &str,
        branch: Option<&str>,
        info: InfoStore,
        blocks: Vec<BlockPage>,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            let mut field = TextField::new(
                Role::SearchInput,
                "Search this page",
                "Search",
                px(120.0),
                cx,
            );
            field.max_chars = Some(200);
            field
        });
        let subs = vec![cx.subscribe(&search, |this, field, event, cx| match event {
            FieldEvent::Changed => {
                this.typing = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(SEARCH_PAUSE).await;
                    this.update(cx, |this, cx| this.search_now(cx)).ok();
                }));
            }
            FieldEvent::Submit { back } => {
                let typed = field.read(cx).text.clone();
                if typed != this.applied {
                    this.search_now(cx);
                } else {
                    let count = this.matches().len();
                    this.at = step_match(this.at, !*back, count);
                    this.scroll_to = this.matches().get(this.at).cloned();
                    cx.notify();
                }
            }
            FieldEvent::Cancel => {
                field.update(cx, |field, cx| {
                    field.text.clear();
                    cx.notify();
                });
                this.search_now(cx);
            }
        })];
        let id = project.to_string_lossy().into_owned();
        let mut page = Self {
            id,
            name: name.to_owned().into(),
            path: tilde(&project.to_string_lossy()).into(),
            branch: branch.map(|b| b.to_owned().into()),
            info,
            data: None,
            unreadable: None,
            seen: None,
            ticket: 0,
            landed: 0,
            visible: false,
            blocks,
            scroll: ScrollHandle::new(),
            scroll_to: None,
            search,
            applied: String::new(),
            at: 0,
            typing: None,
            facts: FactState::new(cx),
            editors: HashMap::new(),
            markdown: HashMap::new(),
            fresh: None,
            removing: None,
            menu: None,
            block_views: HashMap::new(),
            error: None,
            ticks: 0,
            _subs: subs,
            _poll: Task::ready(()),
        };
        page.facts.listen(&mut page._subs, cx);
        page.reload(cx);
        page._poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let watch = this
                    .update(cx, |this, cx| {
                        // The "2 minutes ago" under each section ticks with the minute.
                        this.ticks += 1;
                        if this.visible && this.ticks % 60 == 0 {
                            cx.notify();
                        }
                        this.visible.then(|| (this.file(), this.seen))
                    })
                    .ok();
                // The page is gone: stop. Not shown: look again next time.
                let Some(watch) = watch else { break };
                let Some((file, seen)) = watch else { continue };
                let modified = cx
                    .background_executor()
                    .spawn(async move { std::fs::metadata(file).and_then(|m| m.modified()).ok() })
                    .await;
                if modified.is_some()
                    && modified != seen
                    && this.update(cx, |this, cx| this.reload(cx)).is_err()
                {
                    break;
                }
            }
        });
        page
    }

    fn file(&self) -> PathBuf {
        self.info.root.join(dir_name(&self.id)).join("info.json")
    }

    /// Told by the window: whether this page is on screen. Only a page on screen looks for changes.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible != visible {
            self.visible = visible;
            for view in self.block_views.values() {
                view.read(cx).set_visible(visible);
            }
            if visible {
                self.reload(cx);
            }
            cx.notify();
        }
    }

    /// Reads the file again, off the window's thread.
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.ticket += 1;
        let ticket = self.ticket;
        let (store, id, file) = (self.info.store.clone(), self.id.clone(), self.file());
        let work = cx.background_executor().spawn(async move {
            let modified = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
            (store.get(&id), modified)
        });
        cx.spawn(async move |this, cx| {
            let (result, modified) = work.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(info) => {
                        // A read made after the file changed under it is dropped by the next poll's comparison.
                        this.seen = modified.or(this.seen);
                        this.unreadable = None;
                        this.take(ticket, info, cx);
                    }
                    Err(error) => this.unreadable = Some(error.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Shows `info` unless a newer answer is already shown.
    fn take(&mut self, ticket: u64, info: Info, cx: &mut Context<Self>) {
        if ticket < self.landed {
            return;
        }
        self.landed = ticket;
        self.data = Some(Arc::new(info));
        self.sections_changed(cx);
    }

    /// Applies one change to what is stored and shows the answer; `done` hears how it went.
    fn apply(
        &mut self,
        op: Op,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<Info, String>, &mut Context<Self>) + 'static,
    ) {
        self.ticket += 1;
        let ticket = self.ticket;
        let (store, id, file) = (self.info.store.clone(), self.id.clone(), self.file());
        let work = cx.background_executor().spawn(async move {
            let result = store.apply(&id, op);
            (
                result,
                std::fs::metadata(&file).and_then(|m| m.modified()).ok(),
            )
        });
        cx.spawn(async move |this, cx| {
            let (result, modified) = work.await;
            this.update(cx, |this, cx| {
                if let Ok(info) = &result {
                    this.seen = modified.or(this.seen);
                    this.take(ticket, info.clone(), cx);
                }
                done(this, result, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// An op the page started that has nothing to do after it but show a refusal.
    fn act(&mut self, op: Op, cx: &mut Context<Self>) {
        self.apply(op, cx, |this, result, _| {
            this.error = result.err().map(Into::into);
        });
    }

    fn matches(&self) -> Vec<Match> {
        self.data
            .as_ref()
            .map_or_else(Vec::new, |info| find_matches(info, &self.applied))
    }

    fn search_now(&mut self, cx: &mut Context<Self>) {
        self.typing = None;
        self.applied = self.search.read(cx).text.clone();
        self.at = 0;
        self.scroll_to = self.matches().first().cloned();
        cx.notify();
    }

    /// How a match shows: the one Enter is on more strongly than the others.
    fn mark(&self, wanted: &Match, matches: &[Match]) -> Option<bool> {
        let index = matches.iter().position(|m| m == wanted)?;
        Some(index == clamp_match(self.at, matches.len()))
    }

    /// Adds a section, a fact or a block where the "+ Add" row was.
    fn add_at(&mut self, at: Place, what: Added, cx: &mut Context<Self>) {
        self.menu = None;
        let Some(info) = self.data.clone() else {
            return;
        };
        match what {
            Added::Section => {
                let titles = info.items.iter().filter_map(|i| match i {
                    Item::Section { title, .. } => Some(title.as_str()),
                    Item::Block { .. } => None,
                });
                let title = fresh_title(titles);
                let known: Vec<String> = info.items.iter().map(|i| i.id().to_owned()).collect();
                self.apply(
                    Op::AddSection { title, at },
                    cx,
                    move |this, result, _| match result {
                        Ok(next) => {
                            this.fresh = next
                                .items
                                .iter()
                                .find(|i| {
                                    !known.iter().any(|k| k == i.id())
                                        && matches!(i, Item::Section { .. })
                                })
                                .map(|i| i.id().to_owned());
                            this.error = None;
                        }
                        Err(error) => this.error = Some(error.into()),
                    },
                );
            }
            Added::Block(plugin, block) => self.act(
                Op::AddBlock {
                    plugin_id: plugin,
                    block_id: block,
                    at,
                },
                cx,
            ),
        }
    }

    // ---- Drawing ----

    fn header(&self, data: &Info) -> AnyElement {
        let sections = data
            .items
            .iter()
            .filter(|i| matches!(i, Item::Section { .. }))
            .count();
        let blocks = data.items.len() - sections;
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let mut counts = format!(
            "{} \u{b7} {}",
            plural(data.facts.len(), "fact", "facts"),
            plural(sections, "section", "sections")
        );
        if blocks > 0 {
            counts.push_str(&format!(" \u{b7} {}", plural(blocks, "block", "blocks")));
        }
        let chip = |glyph: Icon, text: SharedString| {
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(4.0))
                .px(px(6.0))
                .py(px(2.0))
                .rounded(px(6.0))
                .bg(rgba(theme::HOVER))
                .child(icon(glyph, 14.0, theme::MUTED_FOREGROUND))
                .child(div().truncate().child(text))
        };
        let matches = self.matches();
        let status = (!self.applied.trim().is_empty()).then(|| {
            if matches.is_empty() {
                "No matches".to_owned()
            } else {
                let n = matches.len();
                format!(
                    "{} of {n} {}",
                    clamp_match(self.at, n) + 1,
                    if n == 1 { "match" } else { "matches" }
                )
            }
        });
        div()
            .w_full()
            .flex()
            .items_start()
            .gap(px(12.0))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(28.0))
                            .line_height(px(36.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.name.clone()),
                    )
                    .child(
                        div()
                            .mt(px(4.0))
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_x(px(6.0))
                            .gap_y(px(4.0))
                            .text_size(px(12.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(chip(Icon::Folder, self.path.clone()))
                            .children(self.branch.clone().map(|b| chip(Icon::GitBranch, b)))
                            .child(div().px(px(4.0)).child(counts)),
                    ),
            )
            .child(
                div()
                    .id("project-page-search")
                    .role(Role::Search)
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .pt(px(6.0))
                    .child(
                        div()
                            .h(px(28.0))
                            .flex()
                            .items_center()
                            .pl(px(8.0))
                            .rounded(px(8.0))
                            .bg(rgba(theme::HOVER))
                            .child(icon(Icon::Search, 14.0, theme::MUTED_FOREGROUND))
                            .child(self.search.clone()),
                    )
                    .children(status.map(|text| {
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .whitespace_nowrap()
                            .child(text)
                    })),
            )
            .into_any_element()
    }

    /// The "+ Add" row between two items: a hairline with a pill that shows when you point at the row, and offers a
    /// section, a fact or a plugin's block.
    fn add_row(
        &self,
        at: Place,
        label: String,
        always: bool,
        (sections, blocks): (usize, usize),
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.menu.as_ref() == Some(&at);
        let group = SharedString::from(format!("add-{index}"));
        let pill = div()
            .id(SharedString::from(format!("add-pill-{index}")))
            .role(Role::Button)
            .aria_label(label)
            .aria_expanded(open)
            .relative()
            .h(px(24.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(10.0))
            .rounded_full()
            .border_1()
            .border_color(rgba(if always {
                theme::HAIRLINE_STRONG
            } else {
                ui::HAIRLINE
            }))
            .bg(rgb(theme::SURFACE))
            .text_size(px(11.0))
            .text_color(rgb(theme::MUTED_FOREGROUND))
            .cursor_pointer()
            .focusable()
            .when(!always && !open, |pill| {
                pill.opacity(0.0)
                    .group_hover(group.clone(), |style| style.opacity(1.0))
            })
            .hover(|style| style.text_color(rgb(theme::FOREGROUND)))
            .on_click({
                let at = at.clone();
                cx.listener(move |this, _, _, cx| {
                    this.menu = (this.menu.as_ref() != Some(&at)).then(|| at.clone());
                    cx.notify();
                })
            })
            .child(icon(Icon::Plus, 12.0, theme::MUTED_FOREGROUND))
            .child("Add");
        let can_block = blocks < MAX_BLOCKS;
        let mut items: Vec<AnyElement> = vec![
            self.menu_item(
                format!("add-{index}-section"),
                "Section",
                sections < MAX_SECTIONS,
                {
                    let at = at.clone();
                    cx.listener(move |this, _, _, cx| this.add_at(at.clone(), Added::Section, cx))
                },
            ),
            self.menu_item(
                format!("add-{index}-fact"),
                "Fact",
                self.facts_has_room(),
                {
                    cx.listener(|this, _, window, cx| {
                        this.menu = None;
                        this.focus_fact_field(window, cx);
                    })
                },
            ),
        ];
        for (n, block) in self.blocks.iter().enumerate() {
            let (plugin, id, at) = (block.plugin.clone(), block.id.clone(), at.clone());
            items.push(self.menu_item(
                format!("add-{index}-block-{n}"),
                &block.title,
                can_block,
                cx.listener(move |this, _, _, cx| {
                    this.add_at(at.clone(), Added::Block(plugin.clone(), id.clone()), cx)
                }),
            ));
        }
        div()
            .group(group)
            .h(px(if always { 36.0 } else { 28.0 }))
            .w_full()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .when(!always, |row| {
                row.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(13.0))
                        .h(px(1.0))
                        .bg(rgba(ui::HAIRLINE))
                        .opacity(0.0)
                        .group_hover(SharedString::from(format!("add-{index}")), |style| {
                            style.opacity(1.0)
                        }),
                )
            })
            .child(pill)
            .when(open, |row| {
                row.child(ui::popover(
                    ui::select_menu(items)
                        .w(px(190.0))
                        .mt(px(2.0))
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu = None;
                            cx.notify();
                        })),
                ))
            })
            .into_any_element()
    }

    fn menu_item(
        &self,
        id: String,
        label: &str,
        enabled: bool,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    ) -> AnyElement {
        let item = ui::select_item(SharedString::from(id), label.to_owned(), false);
        if enabled {
            item.on_click(on_click).into_any_element()
        } else {
            item.opacity(0.5).cursor_default().into_any_element()
        }
    }
}

enum Added {
    Section,
    Block(String, String),
}

impl Render for ProjectPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = div()
            .id("project-page")
            .role(Role::Main)
            .aria_label(format!("{}, project page", self.name))
            .size_full()
            .rounded(px(14.0))
            .border_1()
            .border_color(rgba(ui::HAIRLINE))
            .bg(rgb(theme::SURFACE))
            .overflow_hidden();
        let outer = div().size_full().px(px(8.0)).pb(px(8.0));
        let Some(data) = self.data.clone() else {
            let note = match &self.unreadable {
                Some(error) => format!("The page for {} can't be read: {error}", self.name),
                None => "Loading the project page".to_owned(),
            };
            return outer
                .child(
                    panel
                        .p(px(24.0))
                        .text_size(px(13.0))
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child(note),
                )
                .into_any_element();
        };
        let matches = self.matches();
        let sections = data
            .items
            .iter()
            .filter(|i| matches!(i, Item::Section { .. }))
            .count();
        let blocks = data.items.len() - sections;
        let mut children: Vec<AnyElement> = Vec::new();
        children.push(
            div()
                .w_full()
                .max_w(px(768.0))
                .mb(px(20.0))
                .child(self.header(&data))
                .into_any_element(),
        );
        let fact_marks: HashMap<String, bool> = data
            .facts
            .iter()
            .filter_map(|f| {
                Some((
                    f.key.clone(),
                    self.mark(&Match::Fact(f.key.clone()), &matches)?,
                ))
            })
            .collect();
        children.push(
            div()
                .w_full()
                .max_w(px(768.0))
                .mb(px(20.0))
                .child(self.facts_section(&data, &fact_marks, window, cx))
                .into_any_element(),
        );
        let mut at_item: HashMap<String, usize> = HashMap::new();
        if data.items.is_empty() {
            children.push(
                div()
                    .w_full()
                    .max_w(px(768.0))
                    .mb(px(20.0))
                    .py(px(40.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(rgba(ui::HAIRLINE))
                    .child(icon(Icon::StickyNote, 20.0, theme::MUTED_FOREGROUND))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("Nothing written about {} yet", self.name)),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child("Add a section or a plugin's block below, or ask Claude to keep what it learns here."),
                    )
                    .into_any_element(),
            );
        }
        let row = |at: Place,
                   label: String,
                   always: bool,
                   index: usize,
                   this: &Self,
                   cx: &mut Context<Self>| {
            div()
                .w_full()
                .max_w(px(768.0))
                .child(this.add_row(at, label, always, (sections, blocks), index, cx))
                .into_any_element()
        };
        let first = if data.items.is_empty() {
            "Add"
        } else {
            "Add at the top"
        };
        children.push(row(
            Place::Top,
            first.to_owned(),
            data.items.is_empty(),
            0,
            self,
            cx,
        ));
        for (i, item) in data.items.iter().enumerate() {
            at_item.insert(item.id().to_owned(), children.len());
            let mark = self.mark(&Match::Item(item.id().to_owned()), &matches);
            let card = match item {
                Item::Section { .. } => {
                    self.section_card(item, i == 0, i + 1 == data.items.len(), mark, window, cx)
                }
                Item::Block { .. } => {
                    self.block_item(item, i == 0, i + 1 == data.items.len(), mark, cx)
                }
            };
            children.push(
                div()
                    .w_full()
                    .max_w(px(768.0))
                    .child(card)
                    .into_any_element(),
            );
            let label = match item {
                Item::Section { title, .. } => format!("Add after {title}"),
                Item::Block { block_id, .. } => format!("Add after {block_id}"),
            };
            children.push(row(
                Place::After(item.id().to_owned()),
                label,
                i + 1 == data.items.len(),
                i + 1,
                self,
                cx,
            ));
        }
        if let Some(error) = self.error.clone() {
            children.push(
                div()
                    .w_full()
                    .max_w(px(768.0))
                    .text_size(px(12.0))
                    .text_color(rgb(theme::DANGER))
                    .child(error)
                    .into_any_element(),
            );
        }
        if let Some(target) = self.scroll_to.take() {
            let index = match target {
                Match::Fact(_) => Some(1),
                Match::Item(id) => at_item.get(&id).copied(),
            };
            if let Some(index) = index {
                self.scroll.scroll_to_item(index);
            }
        }
        outer
            .child(
                panel.child(
                    div()
                        .id("project-page-scroll")
                        .size_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .px(px(24.0))
                        .pt(px(40.0))
                        .pb(px(64.0))
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .children(children),
                ),
            )
            .into_any_element()
    }
}
