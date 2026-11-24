//! Boards share a minimum column width across their headers and lane cells.

use super::{FollowScroll, PageView, column_window_view, tone_color, tree};
use std::rc::Rc;
use crate::{theme, ui};
use gpui::{
    AnyElement, Context, ElementId, FontWeight, IntoElement, Role, div, prelude::*, px, relative,
    rgb, rgba,
};

/// A lanes board with more lanes than this draws them a window at a time: each lane is a row of cells of cards.
const WINDOW_LANES: usize = 12;

/// A board's column with more cards than this draws them a window at a time.
const WINDOW_CARDS: usize = 40;

pub(super) fn meter(meter: &tree::Meter) -> AnyElement {
    div()
        .id(ElementId::NamedInteger("page-meter".into(), meter.id))
        .role(Role::ProgressIndicator)
        .aria_label(meter.label.clone())
        .aria_min_numeric_value(0.0)
        .aria_max_numeric_value(100.0)
        .aria_numeric_value(meter.percent)
        .w_full()
        .h(px(4.0))
        .rounded(px(2.0))
        .overflow_hidden()
        .bg(rgba(0xffffff14))
        .child(
            div()
                .h_full()
                .w(relative((meter.percent / 100.0) as f32))
                .rounded(px(2.0))
                .bg(rgb(meter.tone.map_or(theme::palette().info, tone_color))),
        )
        .into_any_element()
}

impl PageView {
    /// plugin-ui.css's `.w-board`: columns 10px apart and, when it fills a page's `flush` body, 14px in from the page's
    /// edges, as Issues' board is; a board of lanes is then 14px in at its sides and bottom, under the columns' names
    /// (`.w-board-lanes`). A board in a padded page or block, like Work item's sprint, sits in that padding, as its own
    /// CSS takes the board's away.
    pub(super) fn board(
        &self,
        board: &tree::Board,
        padded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(ElementId::NamedInteger("page-board".into(), board.id))
            .role(Role::Region)
            .aria_label(board.label.clone())
            .focusable()
            .tab_stop(true)
            .min_w_0()
            .min_h_0()
            .flex_1()
            .overflow_x_scroll()
            .flex()
            .gap(px(10.0))
            .when(board.lanes, |el| {
                el.flex_col()
                    .when(padded, |el| el.px(px(14.0)))
                    .children(board.header.iter().map(|node| self.lane_heads(node, cx)))
                    .child({
                        let lanes = div()
                            .id(ElementId::NamedInteger("page-board-lanes".into(), board.id))
                            .min_h_0()
                            .flex_1()
                            .when(padded, |el| el.pb(px(14.0)))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(10.0));
                        // Many lanes: the ones in view are drawn, as a long column of a `Scroll` is.
                        match self.window_of(board.id, &board.body, WINDOW_LANES, 10.0, cx) {
                            Some((state, inner)) => lanes.track_scroll(&state.handle).child(inner),
                            None => lanes.children(self.nodes(&board.body, cx)),
                        }
                    })
            })
            .when(!board.lanes, |el| {
                el.when(padded, |el| el.p(px(14.0)))
                    .children(self.nodes(&board.body, cx))
            })
            .children(crate::ui::cutoff::clips_on_purpose())
            .into_any_element()
    }

    /// The names of a board of lanes' columns over them, as `.w-board-lanes-column`: no box of their own.
    fn lane_heads(&self, node: &tree::Node, cx: &mut Context<Self>) -> AnyElement {
        let tree::Node::BoardRow(row) = node else {
            return self.node(node, cx);
        };
        div()
            .flex()
            .flex_none()
            .gap(px(10.0))
            .children(row.children.iter().map(|node| {
                match node {
                    tree::Node::BoardColumn(column) => self
                        .column_head(column, cx)
                        .min_w(px(220.0))
                        .flex_1()
                        .px(px(9.0))
                        .pt(px(12.0))
                        .pb(px(8.0))
                        .into_any_element(),
                    other => self.node(other, cx),
                }
            }))
            .into_any_element()
    }

    /// A column's name in 12px semibold, cut to one line, with its count and actions after it.
    fn column_head(&self, column: &tree::BoardColumn, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.inline(&column.title, cx)),
            )
            .when_some(column.count, |el, count| {
                el.child(ui::badge(
                    ui::Variant::Secondary,
                    None,
                    true,
                    count.to_string(),
                ))
            })
            .children(self.nodes(&column.actions, cx))
    }

    /// `children` drawn a window at a time in a box of their own, for the scroll `id` of a board's column or lanes to put
    /// round them (as a `Scroll`'s one column is, which is where the children's places are read), and the scroll's state:
    /// none when there are not enough of them, or they are not each an element with an id.
    fn window_of(
        &self,
        id: u64,
        children: &[tree::Node],
        from: usize,
        space: f32,
        cx: &mut Context<Self>,
    ) -> Option<(Rc<FollowScroll>, gpui::Div)> {
        if !column_window_view::windowed_nodes(children, from) {
            return None;
        }
        let state = self.watched_scroll(id);
        let (parts, places) = self.windowed_parts(children, px(space), false, false, &state, false, cx);
        let inner = div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(space))
            .children(parts)
            .on_children_prepainted(move |bounds, window, cx| {
                places(bounds, window, cx);
            });
        Some((state, inner))
    }

    pub(super) fn board_row(&self, children: &[tree::Node], cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_none()
            .gap(px(10.0))
            .children(self.nodes(children, cx))
            .into_any_element()
    }

    /// `.w-board-column`, or a lane's `.w-board-cell`: a box of cards 8px apart, the column's name over them.
    pub(super) fn board_column(
        &self,
        column: &tree::BoardColumn,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        ui::board_box()
            .id(ElementId::NamedInteger(
                "page-board-column".into(),
                column.id,
            ))
            .role(Role::Group)
            .aria_label(if column.label.is_empty() {
                tree::plain_text(&column.title)
            } else {
                column.label.clone()
            })
            .min_w(px(220.0))
            .flex_1()
            .min_h_0()
            .when(column.cell, |el| el.min_h(px(52.0)))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .when(!column.cell, |el| {
                el.child(self.column_head(column, cx).px(px(4.0)).pt(px(2.0)))
            })
            .child({
                let cards = div()
                    .id(ElementId::NamedInteger(
                        "page-board-cards".into(),
                        column.id,
                    ))
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .overflow_y_scroll();
                // A column of a board, not a lane's cell (which grows with its cards and scrolls in its lane), draws the
                // cards in view and half a view either side, as a long column of a `Scroll` does.
                match Some(column).filter(|column| !column.cell).and_then(|column| {
                    self.window_of(column.id, &column.body, WINDOW_CARDS, 8.0, cx)
                }) {
                    Some((state, inner)) => cards.track_scroll(&state.handle).child(inner),
                    None => cards.children(self.nodes(&column.body, cx)),
                }
            })
            .into_any_element()
    }
}
