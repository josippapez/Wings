//! A `Split`: its panes, edge and floating card.
use super::*;

impl PageView {
    /// Each pane scrolls on its own, as plugin-ui.css's `.w-split-pane { overflow: auto }`: Disk usage's list of
    /// files ran past the bottom of its tab. With `side: "end"` the fixed pane is on the right, borderless with 12
    /// points round it, for floating cards, and the other pane doesn't scroll: it is a page that fills its frame and
    /// scrolls what it holds itself. That fixed pane isn't drawn while the split is narrower than `collapseBelow`
    /// (unless pinned), or when it holds nothing.
    pub(super) fn split(&self, split: &tree::Split, cx: &mut Context<Self>) -> AnyElement {
        let id = split.id;
        // Measures the split's own width, which the fixed pane doesn't change, and draws again only when it crosses
        // the threshold. A drag that crosses it shows or hides the pane a frame late, which nobody sees as wrong, and
        // it saves sizing the split from what it holds.
        let (collapse_below, inset_below) = (split.collapse_below, split.inset_below.filter(|_| split.float));
        let (inset_column, pane) = (split.inset_column, self.split_size(split));
        let resizable = split.dock && split.on_resize.is_some();
        let measure = (collapse_below.is_some() || inset_below.is_some() || resizable).then(|| {
            let entity = cx.entity();
            let on_narrow = split.on_narrow.clone();
            canvas(
                move |bounds, _, cx| {
                    let width = bounds.size.width.as_f32();
                    let narrow = collapse_below.is_some_and(|below| width < below);
                    let inset = inset_below.is_some_and(|below| width >= below);
                    let by = if inset { inset_by(width, pane, inset_column) } else { 0.0 };
                    entity.update(cx, |view, cx| {
                        view.split_widths.insert(id, width);
                        let moved = view.split_narrow.insert(id, narrow) != Some(narrow);
                        let was = if inset_below.is_some() { view.split_inset.insert(id, (inset, by)) } else { None };
                        // The page hears when the split crosses a threshold; a step of how far it is set in only draws this view.
                        let crossed = moved || (inset_below.is_some() && was.map(|(was, _)| was) != Some(inset));
                        // Only a split that sets its start in has a step to draw; any other, measured once per frame, drew its page
                        // again on every frame it was drawn.
                        if crossed || (inset_below.is_some() && was.map(|(_, was)| was) != Some(by)) {
                            cx.notify();
                        }
                        if crossed {
                            if let Some(handler) = &on_narrow {
                                let mut payload = serde_json::json!({ "narrow": narrow });
                                if inset_below.is_some() {
                                    payload["inset"] = serde_json::json!(inset);
                                }
                                view.send(handler, Some(&payload));
                            }
                        }
                    })
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        });
        let narrow = self.split_narrow.get(&id).copied().unwrap_or(false);
        let fixed = if split.fixed_end { &split.end } else { &split.start };
        let drawn = !fixed.is_empty() && (split.pinned || !narrow);
        let fixed_pane = |name: &'static str| {
            div()
                .id(ElementId::NamedInteger(name.into(), id))
                .overflow_y_scroll()
                .w(px(split.size))
                .max_w(relative(0.5))
                .flex_none()
                .min_h_0()
                .flex()
                .flex_col()
        };
        let container = div().relative().flex_1().min_h_0().min_w_0().flex().children(measure);
        if split.fixed_end && split.float {
            return self.floating_split(split, container, cx);
        }
        if split.fixed_end && split.dock {
            // A column docked beside `start`: flush, full height, a hairline between, and what it holds shares its height.
            // With `onResize` its inner edge is dragged: the width is the app's while it moves (`split_size`).
            let column = div()
                .id(ElementId::NamedInteger("page-split-dock".into(), id))
                .relative()
                .w(px(self.split_size(split)))
                .when(!resizable, |column| column.max_w(relative(0.5)))
                .flex_none()
                .min_h_0()
                .flex()
                .flex_col()
                .border_l_1()
                .border_color(rgba(ui::HAIRLINE))
                .children(self.nodes(&split.end, cx))
                .when(resizable, |column| column.child(self.split_edge(split, cx)));
            return container
                .child(div().flex_1().min_w_0().min_h_0().flex().flex_col().overflow_hidden().children(self.nodes(&split.start, cx)))
                .when(drawn, |el| el.child(column))
                .into_any_element();
        }
        if split.fixed_end {
            return container
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .children(self.nodes(&split.start, cx)),
                )
                .when(drawn, |el| {
                    el.child(fixed_pane("page-split-end-fixed").p(px(12.0)).gap(px(8.0)).children(self.nodes(&split.end, cx)))
                })
                .into_any_element();
        }
        let gutter = px(gap(split.gap));
        container
            .when(drawn, |el| {
                el.child(
                    fixed_pane("page-split-start")
                        .pr(gutter)
                        .border_r_1()
                        .border_color(rgba(ui::HAIRLINE))
                        .children(self.nodes(&split.start, cx)),
                )
            })
            .child(
                div()
                    .id(ElementId::NamedInteger("page-split-end".into(), id))
                    .overflow_y_scroll()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .when(drawn, |el| el.pl(gutter))
                    .children(self.nodes(&split.end, cx)),
            )
            .into_any_element()
    }

    /// The inner edge of a docked column with `onResize`: the grab area on the column's left, its hairline lit while the
    /// pointer is on it or drags it, and a double press that sends the page's `resetSize`.
    pub(super) fn split_edge(&self, split: &tree::Split, cx: &mut Context<Self>) -> AnyElement {
        let (start, reset) = (split.clone(), split.clone());
        ui::resize_edge(
            "page-split-dock-edge",
            "Resize column",
            ui::Side::Right,
            self.split_dragging(split.id),
            cx,
            move |this, x, _, cx| this.start_split_drag(&start, x, cx),
            |this, x, _, cx| this.split_drag_to(x, cx),
            Self::end_split_drag,
        )
        .left(px(-ui::RESIZE_EDGE))
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                if event.click_count == 2 {
                    this.reset_split(&reset, cx);
                }
            }),
        )
        .into_any_element()
    }

    /// A split with `float: true`: `start` takes the whole width and the fixed `end` pane floats over its right edge,
    /// as tall as what it holds, so the main pane never moves for it. The page decides when it shows (it hears
    /// `onNarrow`). With `onClose` the pane is open over the content: it has the keyboard, Escape in it and a press
    /// outside it send `onClose`.
    pub(super) fn floating_split(&self, split: &tree::Split, container: gpui::Div, cx: &mut Context<Self>) -> AnyElement {
        let id = split.id;
        // Set in by the pane's width where the split has room, so nothing of the main pane goes under the pane.
        let inset = match self.split_inset.get(&id) {
            Some((true, by)) if !split.end.is_empty() => *by,
            _ => 0.0,
        };
        let main = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .when(inset > 0.0, |main| main.pr(px(inset)))
            .children(self.nodes(&split.start, cx));
        if split.end.is_empty() {
            return container.child(main).into_any_element();
        }
        // A field of the start's under the pane, being typed in, draws its text under the pane, in place.
        let over = Rc::new(Cell::new(gpui::Bounds::default()));
        let container = container.child(ui::under(over.clone(), main));
        // A card that scrolls what it holds (`scroll: true`) shrinks to the pane, which then neither scrolls nor clips: its
        // clip cut off the card's foot and its shadow. Any other pane scrolls as a whole.
        let card_scrolls = split.end.iter().any(|node| matches!(node, Node::Card(card) if card.scrolls));
        let mut pane = div()
            .id(ElementId::NamedInteger("page-split-float".into(), id))
            .absolute()
            .top_0()
            .right_0()
            .w(px(split.size))
            .max_w(relative(0.5))
            .max_h(relative(1.0))
            .when(!card_scrolls, |pane| pane.overflow_y_scroll())
            // The pane, not its cards, keeps the pointer from the page under it: a card that did was hit first and hid
            // the pane from the wheel, so a panel taller than the page didn't scroll.
            .occlude()
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(8.0));
        let mut container = container;
        if let Some(close) = split.on_close.clone() {
            let focus = self.sheet_focus.borrow_mut().entry(id).or_insert_with(|| cx.focus_handle()).clone();
            self.sheets_drawn.borrow_mut().push((id, focus.clone()));
            let on_press = close.clone();
            container = container.child(
                div().absolute().top_0().left_0().size_full().on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |view, _, _, _| view.act(&on_press, None)),
                ),
            );
            let page = cx.entity();
            pane = pane.track_focus(&focus).capture_escape(move |_, window, cx| {
                page.update(cx, |view, _| {
                    if view.shows_menu() {
                        return false;
                    }
                    view.send(&close, None);
                    window.prevent_default();
                    true
                })
            });
        }
        // The cards' surface is see-through, which only shows once the pane floats over the page's text: an opaque layer
        // under them, at the raised card's radius, keeps that text out, and the pointer with it. It is the dialogs' and
        // menus' lighter fill with their deep soft shadow, so the pane stands off the page as they do: on the page's own
        // colour only its light edge told it apart, which read as an outline.
        let under = div()
            .id(ElementId::NamedInteger("page-split-float-under".into(), id))
            .flex()
            .flex_col()
            .when(card_scrolls, |under| under.min_h_0())
            .gap(px(8.0))
            .rounded(px(14.0))
            .bg(rgb(theme::palette().dialog))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0.0, 0.0, 0.0, 0.45),
                offset: gpui::point(px(0.0), px(10.0)),
                blur_radius: px(32.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .child(ui::bounds_into(over))
            .children(self.nodes(&split.end, cx));
        container.child(pane.child(under)).into_any_element()
    }
}
