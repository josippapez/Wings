//! The title bar, drawn by the app like the desktop's: the sidebar button beside the traffic lights,
//! then the tabs and the new tab button. Its empty parts move the window.

use gpui::{AnyElement, Context, MouseButton, MouseDownEvent, Role, div, prelude::*, px};

use crate::{
    actions::ToggleSidebar,
    icons::Icon,
    ui::{ButtonSize, icon_button},
};

use super::{Workspace, shortcut_label};

/// The desktop's `h-10`.
pub(super) const TITLE_BAR_HEIGHT: f32 = 40.0;
/// Room for the traffic lights, as the desktop's `pl-[84px]`.
const TRAFFIC_LIGHTS: f32 = 84.0;

impl Workspace {
    pub(super) fn title_bar(&self, tabs: Vec<AnyElement>, cx: &mut Context<Self>) -> AnyElement {
        let sidebar = self.sidebar;
        // The plugins' sidebar buttons, at the right end like the desktop's.
        let plugin_buttons = self.sidebar_buttons(cx);
        div()
            .id("title-bar")
            .h(px(TITLE_BAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .pr(px(10.0))
            .on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
                if event.click_count == 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            })
            .child(
                // Open, it is as wide as the sidebar plus the project bar's padding, so the tabs
                // start where the project name does.
                div()
                    .h_full()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .pl(px(TRAFFIC_LIGHTS))
                    .when(sidebar, |left| left.w(px(self.side.width + 12.0)))
                    .when(!sidebar, |left| left.pr(px(4.0)))
                    .child(
                        icon_button(
                            "toggle-sidebar",
                            Icon::PanelLeft,
                            format!(
                                "{} ({})",
                                if sidebar {
                                    "Hide sidebar"
                                } else {
                                    "Show sidebar"
                                },
                                shortcut_label("B", false)
                            ),
                            ButtonSize::Sm,
                        )
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(ToggleSidebar), cx)
                        }),
                    ),
            )
            .child(
                div()
                    .id("tabs")
                    .role(Role::TabList)
                    .aria_label("Tabs")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .overflow_x_scroll()
                    .track_scroll(&self.tab_scroll)
                    .children(tabs),
            )
            .child(
                icon_button(
                    "new-terminal",
                    Icon::Plus,
                    format!("New tab ({})", shortcut_label("T", false)),
                    ButtonSize::Sm,
                )
                .on_click(cx.listener(|this, _, window, cx| this.new_terminal(window, cx)))
                .ml(px(4.0))
                .track_focus(&self.new_focus),
            )
            .when(!plugin_buttons.is_empty(), |bar| {
                bar.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(2.0))
                        .ml(px(8.0))
                        .children(plugin_buttons),
                )
            })
            .into_any_element()
    }
}
