use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Role, Window, div,
    prelude::*, px, rgb,
};

use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssuesEvent {
    OpenPlugins,
}

/// What the Issues tab shows with no Issues plugin: the plugin's board replaces the tab when it runs.
pub struct IssuesView {
    focus: FocusHandle,
}

impl EventEmitter<IssuesEvent> for IssuesView {}

impl Focusable for IssuesView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl IssuesView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle().tab_stop(true),
        }
    }

    fn open_plugins(&mut self, cx: &mut Context<Self>) {
        cx.emit(IssuesEvent::OpenPlugins);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if matches!(key, "enter" | "space") && self.focus.is_focused(window) {
            self.open_plugins(cx);
            cx.stop_propagation();
        }
    }
}

impl Render for IssuesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = "Issues is a plugin";
        let description =
            "Install Issues from Settings \u{2192} Plugins to track this project's issues here.";
        div()
            .id("issues-state")
            .role(Role::Status)
            .aria_label(format!("{title}. {description}"))
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            // Centred in the card like the app's other empty states, not hung from its top.
            .justify_center()
            .gap(px(8.0))
            .p(px(24.0))
            .overflow_y_scroll()
            .child(
                div()
                    .max_w(px(384.0))
                    .text_center()
                    .text_size(px(14.0))
                    .child(title),
            )
            .child(
                div()
                    .max_w(px(384.0))
                    .text_center()
                    .text_size(px(14.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(description),
            )
            .child(
                // The app's own button, not a 2 point border of its own: the page's other buttons are 1 point and
                // only show a ring while the keyboard has focus on them.
                crate::ui::button(
                    "issues-open-plugins",
                    "Open Plugins",
                    crate::ui::Variant::Secondary,
                    false,
                    "Open Plugins",
                )
                .track_focus(&self.focus)
                .mt(px(8.0))
                .on_click(cx.listener(|this, _, _, cx| this.open_plugins(cx)))
                .children(crate::ui::spot(|| "issues-open-plugins".into())),
            )
    }
}
