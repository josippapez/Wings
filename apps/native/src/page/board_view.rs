//! Boards share a minimum column width across their headers and lane cells.

use super::{PageView, tone_color, tree};
use crate::{theme, ui};
use gpui::{
    AnyElement, Context, ElementId, FontWeight, IntoElement, Role, div, prelude::*, px, relative,
    rgb, rgba,
};

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
                .bg(rgb(meter.tone.map_or(theme::INFO, tone_color))),
        )
        .into_any_element()
}

impl PageView {
    pub(super) fn board(&self, board: &tree::Board, cx: &mut Context<Self>) -> AnyElement {
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
            .gap(px(12.0))
            .when(board.lanes, |el| {
                el.flex_col().children(self.nodes(&board.header, cx)).child(
                    div()
                        .id(ElementId::NamedInteger("page-board-lanes".into(), board.id))
                        .min_h_0()
                        .flex_1()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .children(self.nodes(&board.body, cx)),
                )
            })
            .when(!board.lanes, |el| el.children(self.nodes(&board.body, cx)))
            .into_any_element()
    }

    pub(super) fn board_row(&self, children: &[tree::Node], cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_none()
            .gap(px(12.0))
            .children(self.nodes(children, cx))
            .into_any_element()
    }

    pub(super) fn board_column(
        &self,
        column: &tree::BoardColumn,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
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
            .flex()
            .flex_col()
            .gap(px(8.0))
            .when(!column.cell, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
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
                        .children(self.nodes(&column.actions, cx)),
                )
            })
            .child(
                div()
                    .id(ElementId::NamedInteger(
                        "page-board-cards".into(),
                        column.id,
                    ))
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .overflow_y_scroll()
                    .children(self.nodes(&column.body, cx)),
            )
            .into_any_element()
    }
}
