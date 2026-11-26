//! A page's boxes: stacks, rows, toolbars, empty states, cards and list items.
use super::*;

impl PageView {
    /// `card`'s children, and whether one of them is a `showOnHover` button.
    pub(super) fn card_children(&self, card: &tree::Card, cx: &mut Context<Self>) -> (Vec<AnyElement>, bool) {
        let outer = self.drawing_card.replace(Some((card.id, false)));
        let children = self.nodes(&card.children, cx);
        let reveals = self.drawing_card.replace(outer).is_some_and(|(_, reveals)| reveals);
        (children, reveals)
    }

    /// A card that holds a `showOnHover` button tells the page when the pointer comes onto it and leaves, which shows
    /// and hides the button.
    pub(super) fn card_hovering(&self, id: u64, card: gpui::Div, reveals: bool, cx: &mut Context<Self>) -> AnyElement {
        if !reveals {
            return card.into_any_element();
        }
        card.id(ElementId::NamedInteger("page-card-hover".into(), id))
            .children(ui::spot(|| format!("page-hover-card-{id}")))
            .on_hover(cx.listener(move |view, hovered: &bool, _, cx| {
                let now = match *hovered {
                    true => Some(id),
                    false if view.hovered_card == Some(id) => None,
                    false => return,
                };
                if view.hovered_card != now {
                    view.hovered_card = now;
                    cx.notify();
                }
            }))
            .into_any_element()
    }

    /// A `Stack` as a box, before it is made an element, so a `Scroll` that watches its children can ask for their bounds
    /// with `watch`.
    pub(super) fn stack(&self, stack: &tree::Stack, watch: Option<popup_menu_view::ChildrenWatch>, cx: &mut Context<Self>) -> gpui::Div {
        // The texts it holds find links when it asks, as a Scroll's take its `textScale`.
        let before = LINKED.with(Cell::get);
        LINKED.with(|cell| cell.set(before || stack.links));
        let element = Self::stack_box(stack).map(|el| match stack.enter {
            true => el.children(stack.children.iter().map(|child| self.rising(child, cx))),
            false => el.children(self.items(&stack.children, cx)),
        });
        LINKED.with(|cell| cell.set(before));
        self.with_context_menu(element, stack, watch, None, cx)
    }

    /// A `Stack`'s box, without its children.
    pub(super) fn stack_box(stack: &tree::Stack) -> gpui::Div {
        div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(gap(stack.gap)))
            .when(stack.grow, |el| el.flex_1())
            .when(stack.fill, |el| el.flex_1().min_h_0())
            .when_some(stack.width, |el, width| el.w(px(width)).flex_none())
            .when_some(stack.max_width, |el, width| el.w_full().max_w(px(width)).mx_auto())
            .when_some(stack.measure, |el, width| el.max_w(px(width)))
            .when_some(stack.pt, |el, pt| el.pt(px(pt)))
            .when_some(stack.min_share, |el, share| el.min_w(relative(share)))
            // A quote's bar: a 3pt border in the muted tone, with the text set in from it.
            .when(stack.quote, |el| {
                el.border_l(px(3.0)).border_color(rgba(theme::palette().muted_foreground << 8 | 0x66)).pl(px(10.0))
            })
            // A list button's 1pt border and its padding, so it starts where the list buttons beside it do.
            .when(stack.inset, |el| {
                el.px(px(7.0)).py(px(1.0 + controls_view::LIST_BUTTON_PADDING))
            })
    }

    pub(super) fn nodes(&self, nodes: &[Node], cx: &mut Context<Self>) -> Vec<AnyElement> {
        nodes.iter().map(|node| self.node(node, cx)).collect()
    }

    /// What `nodes` say as one piece of text, or as inline parts when they hold more than strings.
    pub(super) fn inline(&self, nodes: &[Node], cx: &mut Context<Self>) -> AnyElement {
        self.inline_of(nodes, false, cx)
    }

    /// `inline` for the words of a `Text`, which a person can select with the mouse (`select`), unlike a button's label.
    pub(super) fn inline_of(&self, nodes: &[Node], select: bool, cx: &mut Context<Self>) -> AnyElement {
        if nodes.iter().all(|node| matches!(node, Node::Str { .. })) {
            let text = plain_text(nodes);
            return match nodes.first() {
                Some(Node::Str { id, .. }) if select && !text.is_empty() => self.selectable_string(*id, text, cx),
                _ => text.into_any_element(),
            };
        }
        if let Some(rich) = self.rich_text(nodes, select, cx) {
            return rich;
        }
        div()
            .flex()
            .flex_wrap()
            .items_baseline()
            .children(self.nodes(nodes, cx))
            .into_any_element()
    }

    /// What a button, chip or badge holds, laid out as the desktop's `inline-flex` box lays out its children: each
    /// component in it is an item of its own, centred, `gap` points from the next, and a run of plain strings is one
    /// item. Only strings are one line of text. A dot drawn as a word of a line sat at the line's foot against the word
    /// after it ("•Active"), and an id and a title ran together ("WEB-10Checkout redesign").
    pub(super) fn contents(&self, nodes: &[Node], gap: f32, cx: &mut Context<Self>) -> AnyElement {
        if nodes.iter().all(|node| matches!(node, Node::Str { .. })) {
            return self.inline(nodes, cx);
        }
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(gap))
            .children(self.items(nodes, cx))
            .into_any_element()
    }

    /// The items of a flex box, as CSS makes them: each component one, and a run of plain strings one between them. A
    /// row of "Updated ", "14 h ago" and buttons spread apart put the gap of a row between the two words. A run with no
    /// text is no item, as an empty text node makes no box in CSS: an icon button's empty label took a gap beside its
    /// icon and pushed it off its middle.
    pub(super) fn items(&self, nodes: &[Node], cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut items = Vec::new();
        let mut run: Vec<Node> = Vec::new();
        let text = |run: &mut Vec<Node>| {
            let text = plain_text(&std::mem::take(run));
            (!text.is_empty()).then(|| div().child(text).into_any_element())
        };
        for node in nodes {
            if matches!(node, Node::Str { .. }) {
                run.push(node.clone());
                continue;
            }
            items.extend(text(&mut run));
            items.push(self.node(node, cx));
        }
        items.extend(text(&mut run));
        items
    }

    /// A child of a stack that rises in: it plays once, the first time a node of its id is drawn, so the items already
    /// there stay put as new ones come. A tween asks for frames only while it runs.
    pub(super) fn rising(&self, node: &Node, cx: &mut Context<Self>) -> AnyElement {
        let element = div().min_w_0().flex().flex_col().child(self.node(node, cx));
        let Some(id) = node.id() else {
            return element.into_any_element();
        };
        crate::ui::motion::tween(element, ElementId::NamedInteger("page-rise".into(), id), crate::ui::motion::RISE, |el, t, reduced| {
            let (opacity, below) = crate::ui::motion::rise_frame(t, reduced);
            el.opacity(opacity).relative().top(px(below))
        })
        .into_any_element()
    }

    /// A page's top row. With a page `column` its content sits in that column, centred, so its title starts where the
    /// body's content does.
    pub(super) fn toolbar(&self, toolbar: &tree::Toolbar, column: Option<f32>, cx: &mut Context<Self>) -> AnyElement {
        let content = div()
            .min_w_0()
            .flex()
            // Too narrow for the title and the actions, the actions go to a line of their own: on one line they ran
            // off the frame's edge, as a 250 point sidebar cut off Changes' Refresh.
            .flex_wrap()
            .items_center()
            .gap(px(10.0))
            .map(|el| match column {
                Some(width) => el.w_full().max_w(px(width)).mx_auto(),
                None => el.w_full(),
            })
            .when(!toolbar.title.is_empty(), |bar| {
                bar.child(
                    div()
                        .flex_none()
                        .max_w(relative(0.4))
                        .truncate()
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.inline(&toolbar.title, cx)),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.0))
                    .children(self.nodes(&toolbar.filters, cx)),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .children(self.nodes(&toolbar.actions, cx)),
            );
        div()
            .id(ElementId::NamedInteger("page-toolbar".into(), toolbar.id))
            .role(Role::Toolbar)
            .flex_none()
            .min_h(px(44.0))
            .px(px(14.0))
            .py(px(8.0))
            .border_b_1()
            .border_color(rgba(ui::HAIRLINE))
            .child(content)
            .into_any_element()
    }

    pub(super) fn row(&self, row: &tree::Row, cx: &mut Context<Self>) -> AnyElement {
        div()
            .min_w_0()
            .flex()
            .gap(px(gap(row.gap)))
            .map(|el| match row.align {
                Align::Center => el.items_center(),
                Align::Start => el.items_start(),
                // GPUI's text tells the layout no baseline, so a baseline row lines up the bottoms of its items'
                // first boxes: a value that starts with a dot, like Properties' "● New", sat under its label, and a
                // value that wraps put its label on its last line, as a log line's number sat by its last line.
                // The row's texts take the tallest of their lines, and each starts as far down as puts its first
                // baseline on the lowest one (`baseline_drops`).
                Align::Baseline => match line_height(&row.children) {
                    Some(height) => el.items_start().line_height(px(height)),
                    None => el.items_baseline(),
                },
            })
            .map(|el| match row.justify {
                Justify::Start => el,
                Justify::End => el.justify_end(),
                Justify::Between => el.justify_between(),
                Justify::Center => el.justify_center(),
            })
            .when(row.wrap, |el| el.flex_wrap())
            .when(row.grow, |el| el.flex_1())
            .children(match baseline_drops(row, cx) {
                Some(drops) => row
                    .children
                    .iter()
                    .zip(drops)
                    .map(|(node, drop)| match node {
                        Node::Text(text) => self.text_element(text, Some(drop), cx),
                        Node::Button(button) => self.button(button, drop, cx),
                        Node::ListButton(item) => self.list_button(item, Some(drop), cx),
                        other => self.node(other, cx),
                    })
                    .collect(),
                None => self.items(&row.children, cx),
            })
            .into_any_element()
    }

    pub(super) fn empty(&self, empty: &tree::Empty, cx: &mut Context<Self>) -> AnyElement {
        let content = self.empty_content(empty, cx);
        if !empty.fill {
            return content;
        }
        // A little above the middle, where the eye puts the middle of a page.
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .pb(px(48.0))
            .when(empty.glow, |el| {
                el.child(div().absolute().top_0().left_0().size_full().child(ui::glow()))
            })
            .child(content)
            .into_any_element()
    }

    pub(super) fn empty_content(&self, empty: &tree::Empty, cx: &mut Context<Self>) -> AnyElement {
        div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(16.0))
            .p(px(24.0))
            .text_center()
            .child(
                div()
                    .max_w(px(384.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(if empty.fill { 22.0 } else { 14.0 }))
                            .font_weight(if empty.fill { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                            .child(self.inline(&empty.title, cx)),
                    )
                    .when(!empty.description.is_empty(), |header| {
                        header.child(
                            div()
                                .text_size(px(14.0))
                                .text_color(rgb(theme::palette().muted_foreground))
                                .child(self.inline(&empty.description, cx)),
                        )
                    }),
            )
            .children(self.nodes(&empty.children, cx))
            .into_any_element()
    }

    pub(super) fn list_item(&self, item: &tree::ListItem, cx: &mut Context<Self>) -> AnyElement {
        if item
            .children
            .iter()
            .any(|node| matches!(node, Node::ListButton(_)))
        {
            // A row that opens to a card shows the card under it, as the desktop's block list item does.
            let opened = item
                .children
                .iter()
                .any(|node| matches!(node, Node::Card(_)));
            div()
                .min_w_0()
                .w_full()
                .flex()
                .when(opened, |el| el.flex_col())
                .when(!opened, |el| el.items_center())
                .children(self.nodes(&item.children, cx))
                .into_any_element()
        } else {
            // Its actions show over the row's end while the row is pointed at or one of them has the keyboard, and the
            // row's last part gives way to them, as plugin-ui.css's `.w-list-item-actions`: Cyclops' entries' buttons
            // take the place of their times.
            let has_actions = !item.actions.is_empty();
            let within = self.actions_within.contains(&item.id);
            let actions = has_actions.then(|| {
                let focus = self
                    .actions_focus
                    .borrow_mut()
                    .entry(item.id)
                    .or_insert_with(|| cx.focus_handle())
                    .clone();
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .track_focus(&focus)
                    .when(!within, |el| {
                        el.opacity(0.0)
                            .group_hover(LIST_ITEM_GROUP, |style| style.opacity(1.0))
                    })
                    .children(self.nodes(&item.actions, cx))
            });
            let last = item.children.len().saturating_sub(1);
            // A card in a row takes the row's width, as a page's own style would have it do.
            ui::list_item(
                ElementId::NamedInteger("page-list-item".into(), item.id),
                item.children.iter().enumerate().map(|(at, child)| {
                    let element = self.node(child, cx);
                    if matches!(child, Node::Card(_)) {
                        div().flex_1().min_w_0().child(element).into_any_element()
                    } else if has_actions && at == last {
                        div()
                            .min_w_0()
                            .flex()
                            .when(within, |el| el.opacity(0.0))
                            .group_hover(LIST_ITEM_GROUP, |style| style.opacity(0.0))
                            .child(element)
                            .into_any_element()
                    } else {
                        element
                    }
                }),
            )
            .when_some(actions, |row, actions| {
                row.group(LIST_ITEM_GROUP).relative().child(actions)
            })
            .into_any_element()
        }
    }
}
