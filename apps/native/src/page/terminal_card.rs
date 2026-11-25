//! How a terminal a tab's agent opened shows in the tab's page, flush in a docked column: a header with what it was opened to run, whether it still
//! runs and buttons that open it as a tab of its own and close it, over the live terminal. Wings draws it in `TERMINALS`,
//! over the place the page's `Terminal` leaves.

use gpui::{
    AnyElement, App, Entity, FontWeight, Role, StyleRefinement, Window, div, prelude::*, px, rgb,
    rgba,
};

use crate::{
    icons::{Icon, icon},
    terminal_view::TerminalView,
    theme,
    ui::{self, ButtonSize},
};

pub fn terminal_card(
    pane: usize,
    title: String,
    running: bool,
    view: Entity<TerminalView>,
    pop: impl Fn(&mut Window, &mut App) + 'static,
    close: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let header = div()
        .flex_none()
        .h(px(32.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .pl(px(10.0))
        .pr(px(4.0))
        .border_b_1()
        .border_color(rgba(ui::HAIRLINE))
        .text_size(px(12.0))
        .child(icon(Icon::SquareTerminal, 14.0, theme::palette().muted_foreground))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(title.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_color(rgb(if running {
                    theme::palette().working
                } else {
                    theme::palette().muted_foreground
                }))
                .child(if running { "Running" } else { "Idle" }),
        )
        .child(
            ui::icon_button(("page-terminal-pop", pane), Icon::Maximize, "Open in a tab", ButtonSize::Sm)
                .on_click(move |_, window, cx| pop(window, cx)),
        )
        .child(
            ui::icon_button(("page-terminal-close", pane), Icon::X, format!("Close {title}"), ButtonSize::Sm)
                .on_click(move |_, window, cx| close(window, cx)),
        );
    div()
        .size_full()
        .flex()
        .flex_col()
        // Flush, as the app's panes are: no rounded card of its own. The column it is docked in draws the hairline beside the
        // page, and a hairline over each terminal divides those stacked in it.
        .border_t_1()
        .border_color(rgba(ui::HAIRLINE))
        .bg(rgb(theme::palette().surface))
        .overflow_hidden()
        .child(header)
        .child(
            div()
                .id(("terminal-pane", pane))
                .flex_1()
                .min_h_0()
                // Its text ran against the card's right border.
                .pr(px(8.0))
                .role(Role::Group)
                .aria_label(title)
                .child(view.cached(StyleRefinement::default().size_full())),
        )
        .into_any_element()
}
