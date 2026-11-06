//! The find bar: the view's find methods, the bar itself and its buttons.

use gpui::{
    BoxShadow, Context, FocusHandle, Role, Window, div, hsla, point, prelude::*, px, rgb, rgba,
};

use crate::{
    icons::{Icon, icon},
    text_field::{FieldEvent, TextField},
    theme,
    ui::{self, ButtonSize},
};

use super::{
    FindBar, TerminalView,
    find::{FIND_LIMIT, FIND_SETTLE, FindMove, FindQuery},
    worker::Command,
};

impl TerminalView {
    /// Replaced on every stale frame, so it fires only after output stops.
    pub(super) fn settle_find(&mut self, cx: &mut Context<Self>) {
        let commands = self.runtime.commands.clone();
        self.find_settle = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(FIND_SETTLE).await;
            let _ = commands.send(Command::Refresh).await;
        }));
    }

    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            let field = cx.new(|cx| {
                TextField::new(Role::SearchInput, "Find in terminal", "Find", px(176.0), cx)
            });
            let events =
                cx.subscribe_in(&field, window, |view, _, event, window, cx| match event {
                    FieldEvent::Changed => view.search(FindMove::Stay, cx),
                    FieldEvent::Submit { back: false } => view.search(FindMove::Next, cx),
                    FieldEvent::Submit { back: true } => view.search(FindMove::Previous, cx),
                    FieldEvent::Cancel => view.close_find(window, cx),
                });
            self.find = Some(FindBar {
                open: false,
                field,
                case_sensitive: false,
                regex: false,
                buttons: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
                _events: events,
            });
        }
        if let Some(find) = &mut self.find {
            find.open = true;
            find.field
                .update(cx, |field, cx| field.select_all(window, cx));
        }
        cx.notify();
    }

    fn search(&mut self, movement: FindMove, cx: &mut Context<Self>) {
        let Some(find) = &self.find else {
            return;
        };
        let query = FindQuery {
            term: find.field.read(cx).text.clone(),
            case_sensitive: find.case_sensitive,
            regex: find.regex,
        };
        self.send(Command::Find(query, movement), cx);
    }

    /// Escape or the close button. The highlights go, the term stays for next time.
    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(find) = &mut self.find {
            find.open = false;
            self.send(Command::FindClose, cx);
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn toggle_find_option(&mut self, regex: bool, cx: &mut Context<Self>) {
        if let Some(find) = &mut self.find {
            if regex {
                find.regex = !find.regex;
            } else {
                find.case_sensitive = !find.case_sensitive;
            }
        }
        self.search(FindMove::Next, cx);
        cx.notify();
    }

    pub(super) fn find_bar(&self, find: &FindBar, cx: &mut Context<Self>) -> impl IntoElement {
        let label = match (find.field.read(cx).text.is_empty(), self.snapshot.find) {
            (true, _) => String::new(),
            (false, Some(results)) if results.count > 0 => format!(
                "{} of {}",
                results.index.map_or(0, |index| index + 1),
                if results.count >= FIND_LIMIT {
                    "1000+".to_string()
                } else {
                    results.count.to_string()
                }
            ),
            _ => "No results".to_string(),
        };
        let [case, regex, previous, next, close] = &find.buttons;
        div()
            .id("find-bar")
            .role(Role::Search)
            .aria_label("Find")
            .absolute()
            // The desktop's `top-10` from the card's top is 10px under the pane header.
            .top(px(10.0))
            .right(px(12.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .p(px(4.0))
            .rounded(px(14.0))
            .border_1()
            .border_color(rgba(theme::HAIRLINE_STRONG))
            .bg(rgba(theme::POPOVER))
            .shadow(vec![BoxShadow {
                color: hsla(0.0, 0.0, 0.0, 0.35),
                offset: point(px(0.0), px(8.0)),
                blur_radius: px(24.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .occlude()
            .cursor_default()
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            // The desktop's `Input`, borderless with its `dark:bg-input/30` fill.
            .child(
                div()
                    .flex_none()
                    .rounded(px(theme::RADIUS))
                    .bg(rgba(INPUT_FILL))
                    .child(find.field.clone()),
            )
            .child(
                div()
                    .id("find-results")
                    .role(Role::Status)
                    .min_w(px(56.0))
                    .text_right()
                    .text_size(px(12.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(label),
            )
            .child(find_button(
                case,
                "Match case",
                Icon::CaseSensitive,
                Some(find.case_sensitive),
                cx,
                |view, _, cx| view.toggle_find_option(false, cx),
            ))
            .child(find_button(
                regex,
                "Regular expression",
                Icon::Regex,
                Some(find.regex),
                cx,
                |view, _, cx| view.toggle_find_option(true, cx),
            ))
            .child(find_button(
                previous,
                "Previous match",
                Icon::ArrowUp,
                None,
                cx,
                |view, _, cx| view.search(FindMove::Previous, cx),
            ))
            .child(find_button(
                next,
                "Next match",
                Icon::ArrowDown,
                None,
                cx,
                |view, _, cx| view.search(FindMove::Next, cx),
            ))
            .child(find_button(
                close,
                "Close find",
                Icon::X,
                None,
                cx,
                Self::close_find,
            ))
    }
}

/// `--input` at 30%.
const INPUT_FILL: u32 = 0xffffff08;

/// The kit's 28px ghost button with a 16px icon, or a toggle when `toggled` is set, like the desktop find bar's: its
/// toggles are shadcn's small `Toggle`, a 14px icon in the text colour.
fn find_button(
    focus: &FocusHandle,
    label: &'static str,
    glyph: Icon,
    toggled: Option<bool>,
    cx: &mut Context<TerminalView>,
    action: fn(&mut TerminalView, &mut Window, &mut Context<TerminalView>),
) -> impl IntoElement {
    // The keys the desktop's find bar shows in these buttons' tooltips.
    let shortcut = match label {
        "Previous match" => Some("⇧↩".into()),
        "Next match" => Some("↩".into()),
        "Close find" => Some("Esc".into()),
        _ => None,
    };
    let glyph = match toggled {
        Some(_) => icon(glyph, 14.0, theme::FOREGROUND),
        None => icon(glyph, 16.0, theme::MUTED_FOREGROUND),
    };
    ui::ghost_button(label, label, shortcut, ButtonSize::Sm, toggled, glyph)
        .track_focus(focus)
        .on_click(cx.listener(move |view, _, window, cx| action(view, window, cx)))
}
