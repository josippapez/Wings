//! One node of a page's tree, drawn by its kind.
use super::*;

impl PageView {
    pub(super) fn node(&self, node: &Node, cx: &mut Context<Self>) -> AnyElement {
        #[cfg(feature = "test-support")]
        if self.hidden_for_bench(node) {
            return div().into_any_element();
        }
        match node {
            Node::Str { text, .. } => div().child(text.clone()).into_any_element(),
            // Only a component that names its parts reads them; one out of place just shows what it holds.
            Node::Slot(slot) => div()
                .children(self.nodes(&slot.children, cx))
                .into_any_element(),
            Node::Page(page) => {
                let body = div()
                    .id(ElementId::NamedInteger("page-body".into(), page.id))
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .when(page.flush, |body| body.overflow_hidden())
                    .when(!page.flush, |body| {
                        body.gap(px(10.0)).p(px(14.0)).overflow_y_scroll()
                    });
                let nodes = page.body.iter().map(|node| match node {
                    Node::Board(board) if page.flush => self.board(board, true, cx),
                    node => self.node(node, cx),
                });
                // A `column` page sets its body in one centred column, the same one its toolbar lines up with.
                let body = match page.column.filter(|_| !page.flush) {
                    Some(width) => body.child(
                        div()
                            .w_full()
                            .max_w(px(width))
                            .mx_auto()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(10.0))
                            .children(nodes),
                    ),
                    None => body.children(nodes),
                };
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .children(page.toolbar.iter().map(|node| match node {
                        Node::Toolbar(toolbar) => self.toolbar(toolbar, page.column, cx),
                        node => self.node(node, cx),
                    }))
                    .child(body)
                    .into_any_element()
            }
            Node::Toolbar(toolbar) => self.toolbar(toolbar, None, cx),
            Node::Stack(stack) => self.stack(stack, None, cx).into_any_element(),
            Node::Row(row) => self.row(row, cx),
            Node::Text(text) => self.text_element(text, None, cx),
            Node::Button(button) => self.button(button, 0.0, cx),
            Node::Notice(notice) => div()
                .id(ElementId::NamedInteger("page-notice".into(), notice.id))
                .role(if matches!(notice.tone, Some(Tone::Warn | Tone::Danger)) {
                    Role::Alert
                } else {
                    Role::Status
                })
                .rounded(px(8.0))
                .border_1()
                // The tone is 0xRRGGBB: alpha goes below it, as the ground's does.
                .border_color(rgba(notice.tone.map_or(theme::palette().info, tone_color) << 8 | 0x66))
                .bg(rgba(
                    notice.tone.map_or(theme::palette().info, tone_color) << 8 | 0x20,
                ))
                .px(px(10.0))
                .py(px(7.0))
                .text_size(px(12.0))
                .text_color(rgb(notice.tone.map_or(theme::palette().info, tone_color)))
                .child(self.inline(&notice.children, cx))
                .into_any_element(),
            Node::Spinner(spinner) => div()
                .id(ElementId::NamedInteger("page-spinner".into(), spinner.id))
                .role(Role::Status)
                .when(!spinner.label.is_empty(), |el| {
                    el.aria_label(spinner.label.clone())
                })
                .size(px(if spinner.large { 18.0 } else { 14.0 }))
                .flex_none()
                .map(|ring| {
                    if self.spin_in_layer.get() {
                        return ring.child(ui::view_spinner_slot(format!("spinner-{}", spinner.id))).into_any_element();
                    }
                    ring.with_animation(
                        ElementId::NamedInteger("page-spinner-animation".into(), spinner.id),
                        Animation::new(Duration::from_secs(1)).repeat().with_max_fps(30.0),
                        |ring, turn| ring.child(ui::working_spinner(turn * std::f32::consts::TAU)),
                    )
                    .into_any_element()
                }),
            Node::Checkbox(checkbox) => self.checkbox(checkbox, cx),
            Node::Confirm(confirm) => ui::plain_card(vec![
                div()
                    .px(px(12.0))
                    .pt(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.inline(&confirm.title, cx))
                    .into_any_element(),
                div()
                    .px(px(12.0))
                    .py(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .children(self.nodes(&confirm.body, cx))
                    .into_any_element(),
                div()
                    .px(px(12.0))
                    .pb(px(12.0))
                    .flex()
                    .flex_wrap()
                    .gap(px(4.0))
                    .children(self.nodes(&confirm.actions, cx))
                    .into_any_element(),
            ])
            .into_any_element(),
            Node::Badge(badge) => {
                // A pill is never wider than its row: its words are cut short with an ellipsis, and a `title` says all.
                let pill = ui::badge(
                    variant(badge.variant),
                    badge.tone.map(tone_color),
                    badge.small,
                    self.contents(&badge.children, 4.0, cx),
                )
                .flex_shrink_1()
                .min_w_0()
                .max_w_full()
                .text_ellipsis();
                let pill = pill.id(ElementId::NamedInteger("page-badge".into(), badge.id)).when_some(badge.title.clone(), |pill, title| {
                    pill.aria_description(title.clone()).tooltip(ui::tooltip(title, None))
                });
                let Some(handler) = badge.on_click.clone() else {
                    return pill.into_any_element();
                };
                // A summary chip that opens something, like a pull request's checks: the same pill, a button that
                // lights up under the pointer, as plugin-ui.css's `button.w-summary-chip` does. GPUI clicks a focused
                // one on Enter and Space.
                pill.role(Role::Button)
                    .aria_label(plain_text(&badge.children))
                    .focusable()
                    .tab_stop(true)
                    .cursor_pointer()
                    .hover(|style| style.bg(rgba(theme::hover())))
                    .focus_visible(|style| style.border_color(rgb(theme::palette().foreground)))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        cx.stop_propagation();
                        view.act(&handler, None);
                    }))
                    .into_any_element()
            }
            Node::Empty(empty) => self.empty(empty, cx),
            Node::Loading(loading) => div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_color(rgb(theme::palette().muted_foreground))
                .child(if loading.children.is_empty() {
                    "Loading…".into_any_element()
                } else {
                    self.inline(&loading.children, cx)
                })
                .into_any_element(),
            // A card that scrolls shrinks to the room it has and scrolls what it holds, so its edge stays drawn: a
            // floating panel taller than the page was cut off at its foot, border and corners with it.
            Node::Card(card) if card.scrolls => {
                let (children, reveals) = self.card_children(card, cx);
                let body = div()
                    .id(ElementId::NamedInteger("page-card-scroll".into(), card.id))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(if card.raised { 6.0 } else { 10.0 }))
                    .track_scroll(&self.watched_scroll(card.id).handle)
                    .children(children);
                let shell = ui::plain_card([body.into_any_element()]).min_h_0().overflow_hidden().children(ui::spot(|| format!("page-card-{}", card.id)));
                let shell = match card.raised {
                    true => shell
                        .px(px(12.0))
                        .py(px(10.0))
                        .rounded(px(14.0))
                        .border_color(rgba(theme::hairline_strong()))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0.0, 0.0, 0.0, 0.28),
                            offset: gpui::point(px(0.0), px(6.0)),
                            blur_radius: px(18.0),
                            spread_radius: px(0.0),
                            inset: false,
                        }]),
                    false => shell,
                };
                self.card_hovering(card.id, shell, reveals, cx)
            }
            Node::Card(card) if card.raised => {
                let (children, reveals) = self.card_children(card, cx);
                let shell = ui::plain_card(children)
                .gap(px(6.0))
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(14.0))
                .border_color(rgba(theme::hairline_strong()))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0.0, 0.0, 0.0, 0.28),
                    offset: gpui::point(px(0.0), px(6.0)),
                    blur_radius: px(18.0),
                    spread_radius: px(0.0),
                    inset: false,
                }]);
                self.card_hovering(card.id, shell, reveals, cx)
            }
            // Rows split by hairlines, as Wings' Settings groups its own: each row keeps the card's inset.
            Node::Card(card) if card.rows => {
                let (children, reveals) = self.card_children(card, cx);
                let shell = ui::plain_card(
                children.into_iter().enumerate().map(|(at, row)| {
                    div()
                        .px(px(12.0))
                        .py(px(10.0))
                        .when(at > 0, |row| row.border_t_1().border_color(rgba(ui::HAIRLINE)))
                        .child(row)
                        .into_any_element()
                }),
            )
            .gap(px(0.0))
            .px(px(0.0))
            .py(px(0.0));
                self.card_hovering(card.id, shell, reveals, cx)
            }
            Node::Card(card) => {
                let (children, reveals) = self.card_children(card, cx);
                self.card_hovering(card.id, ui::plain_card(children), reveals, cx)
            }
            Node::Heading(heading) => ui::heading(self.inline(&heading.children, cx))
                .when(heading.section, |el| {
                    el.text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::palette().foreground))
                })
                .into_any_element(),
            Node::Dot(dot) => match dot.state {
                // Turning where the page's spinners turn: in the window's layer for them, or in place.
                Some(state) if self.spin_in_layer.get() => {
                    ui::view_status_dot_slot(state, format!("dot-{}", dot.id))
                }
                Some(state) => ui::status_dot(state).into_any_element(),
                None => ui::dot(
                    dot.color
                        .unwrap_or_else(|| dot.tone.map_or(theme::palette().done, tone_color)),
                )
                .into_any_element(),
            },
            Node::Scroll(scroll) if scroll.max_height.is_some() => self.scroll_within(scroll, cx),
            // A scroll keeps 8 points either side and 12 below, as a list under a header does.
            Node::Scroll(scroll) => {
                let watches = scroll.scroll_to.is_some() || scroll.on_track.is_some();
                let column = match &scroll.children[..] {
                    [Node::Stack(column)] => Some(column),
                    _ => None,
                };
                // A long column is drawn a window at a time, which needs to know where the scroll is: so does one that was,
                // whose place would otherwise be lost as it gets shorter.
                let windowed = column.filter(|column| column_window_view::windowed(column));
                let state = match (scroll.follow_end, watches) {
                    (true, _) => Some(self.follow_end(scroll.id, scroll.on_reach_end.is_none())),
                    (false, true) => Some(self.watched_scroll(scroll.id)),
                    (false, false) if windowed.is_some() || self.follow_scrolls.borrow().contains_key(&scroll.id) => {
                        Some(self.watched_scroll(scroll.id))
                    }
                    (false, false) => None,
                };
                // A following scroll with one column keeps what is in view in place by the column's children, as children
                // come and go at either end; another keeps it by how much grew above.
                let edges = column.filter(|_| scroll.follow_end).map(|column| Edges {
                    start: scroll.on_reach_start.clone(),
                    end: scroll.on_reach_end.clone(),
                    ids: element_ids(column),
                    tree: self.trees,
                });
                let reach = state.as_ref().filter(|_| scroll.follow_end && edges.is_none()).map(|state| Self::reach_start(state.clone(), scroll.on_reach_start.clone(), cx));
                let mut base = div();
                if let Some(reach) = reach {
                    base = base.on_children_prepainted(reach);
                }
                // A scale sizes the plain text the scroll holds from here, which the other sizes follow with `scaled`.
                let before = TEXT_SCALE.with(Cell::get);
                let scale = scroll.text_scale.unwrap_or(before);
                TEXT_SCALE.with(|cell| cell.set(scale));
                // Following, or with a place to scroll to or children to track, its one column reports where its children are.
                let reports = watches || edges.is_some();
                let children = match (column, state.as_ref()) {
                    (Some(column), Some(state)) if windowed.is_some() || reports => {
                        let watch: Option<popup_menu_view::ChildrenWatch> = match reports {
                            true => Some(Box::new(Self::scroll_watch(state.clone(), edges, scroll.scroll_to, scroll.track.clone(), scroll.on_track.clone(), cx))),
                            false => None,
                        };
                        let element = match windowed {
                            Some(_) => self.windowed_column(column, state, scroll.follow_end && state.stuck.get(), watch, cx),
                            None => self.stack(column, watch, cx),
                        };
                        vec![element.into_any_element()]
                    }
                    _ => self.nodes(&scroll.children, cx),
                };
                TEXT_SCALE.with(|cell| cell.set(before));
                let area = base
                    .id(ElementId::NamedInteger("page-scroll".into(), scroll.id))
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    // As far in as a Bar's content, so a page's text lines up with its header and footer in a narrow pane.
                    .px(px(14.0))
                    .pb(px(12.0))
                    .overflow_y_scroll()
                    .when(scale != 1.0, |el| el.text_size(px(13.0 * scale)))
                    .children(children);
                match state {
                    Some(state) => area.track_scroll(&state.handle).into_any_element(),
                    None => area.into_any_element(),
                }
            }
            Node::Table(table) => self.table(table, cx),
            Node::Bar(bar) => div()
                .flex_none()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(gap(bar.gap)))
                .px(px(14.0))
                .py(px(8.0))
                // A terminal pane's header: 30 points tall with its 28 point buttons, and closer to its right edge.
                .when(bar.slim, |el| el.py(px(1.0)).pr(px(4.0)))
                .when(bar.edge == tree::Edge::Top, |el| {
                    el.border_t_1().border_color(rgba(ui::HAIRLINE))
                })
                .when(bar.edge == tree::Edge::Bottom, |el| {
                    el.border_b_1().border_color(rgba(ui::HAIRLINE))
                })
                .children(self.nodes(&bar.children, cx))
                .into_any_element(),
            Node::Split(split) => self.split(split, cx),
            Node::Gauge(gauge) => ui::gauge(
                ElementId::NamedInteger("page-gauge".into(), gauge.id),
                gauge.label.clone(),
                gauge.percent,
                level_color(gauge.level()),
                (!gauge.note.is_empty()).then(|| self.inline(&gauge.note, cx)),
            )
            .into_any_element(),
            Node::Meter(meter) => board_view::meter(meter),
            Node::Board(board) => self.board(board, false, cx),
            Node::BoardRow(row) => self.board_row(&row.children, cx),
            Node::BoardColumn(column) => self.board_column(column, cx),
            Node::List(list) => ui::list(self.nodes(&list.children, cx)).into_any_element(),
            Node::ListItem(item) => self.list_item(item, cx),
            Node::Disclosure(disclosure) => self.disclosure(disclosure, cx),
            Node::HoverCard(card) => self.hover_card(card, cx),
            Node::PopupMenu(menu) => self.popup_menu(menu, cx),
            Node::Select(select) => self.select(select, cx),
            Node::Combobox(combobox) => self.combobox(combobox, cx),
            Node::Menu(menu) => self.menu(menu, cx),
            Node::Chips(select) => self.chips(select, cx),
            Node::ListButton(item) => self.list_button(item, None, cx),
            Node::SearchField(field) => self.search_field(field, cx),
            Node::Input(input) => self.input(input, cx),
            Node::Icon(icon) => crate::icons::icon(
                match icon.name {
                    tree::IconName::Check => crate::icons::Icon::Check,
                    tree::IconName::ChevronDown => crate::icons::Icon::ChevronDown,
                    tree::IconName::Copy => crate::icons::Icon::Copy,
                    tree::IconName::Trash => crate::icons::Icon::Trash,
                    tree::IconName::Claude => crate::icons::Icon::Claude,
                },
                icon.size,
                // The Claude spark is drawn in its own orange, as Wings draws it beside a Claude session.
                match icon.name {
                    tree::IconName::Claude => theme::claude(),
                    _ => theme::palette().foreground,
                },
            )
            .into_any_element(),
            // GPUI draws an svg as a mask in its text colour, which is the drawing's `currentColor`.
            Node::Svg(drawing) => {
                let image = gpui::svg()
                    .data(drawing.markup.as_bytes())
                    .size(px(drawing.size))
                    .flex_none()
                    .text_color(rgb(drawing.tone.map_or(theme::palette().foreground, tone_color)));
                match &drawing.label {
                    Some(label) => div()
                        .id(ElementId::NamedInteger("page-svg".into(), drawing.id))
                        .role(Role::Image)
                        .aria_label(label.clone())
                        .flex_none()
                        .child(image)
                        .into_any_element(),
                    None => image.into_any_element(),
                }
            }
            Node::Image(picture) => self.picture(picture, cx),
            Node::Graph(graph) => self.graph(graph, cx),
            Node::Avatar(avatar) => rich_view::avatar(avatar),
            Node::Skeleton(skeleton) => rich_view::skeleton(skeleton),
            Node::Sunburst(map) => self.sunburst(map, cx),
            Node::PromptRail(rail) => self.prompt_rail(rail, cx),
            Node::Code(code) => self.code(code, cx),
            // Drawn over the whole page by `render`, not where it sits.
            Node::Dialog(_) => div().into_any_element(),
            Node::Sheet(sheet) => self.sheet(sheet, cx),
            Node::Terminal(terminal) => self.terminal(terminal),
            Node::WebView(web) => self.web_view(web, cx),
            // At the end of the line whether it is in a column (a dialog) or a row (a confirm's actions).
            Node::Actions(actions) => div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_wrap()
                .justify_end()
                .items_center()
                .gap(px(8.0))
                .children(self.nodes(&actions.children, cx))
                .into_any_element(),
        }
    }
}
