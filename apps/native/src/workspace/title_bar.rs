//! The title bar, drawn by the app like the desktop's: the sidebar, Back and Forward buttons beside the traffic
//! lights, then the tabs, the new tab button and its menu. Its empty parts move the window.

use crate::workspace::NotifyWorkspace;
use gpui::{
    AnyElement, Context, MouseButton, MouseDownEvent, Role, SharedString, Window, div, prelude::*,
    px,
};

use crate::{
    actions::{GoBack, GoForward, ToggleSidebar},
    icons::{Icon, icon, plugin_glyph},
    theme,
    ui::{self, ButtonSize, motion, shortcut_icon_button},
};

use super::{Workspace, shortcut_label};

/// The desktop's `h-10`.
pub(super) const TITLE_BAR_HEIGHT: f32 = 40.0;

/// The width of the shade at a cut end of the tab strip, and how far from its end the strip counts as scrolled.
const TAB_EDGE: f32 = 20.0;
const TAB_EDGE_SLACK: f32 = 1.0;

/// One entry of the new tab menu: a plugin page that offers itself as a new tab.
struct NewPage {
    plugin: String,
    view: String,
    title: String,
    icon: String,
    own: Option<String>,
}

impl Workspace {
    pub(super) fn title_bar(&self, tabs: Vec<AnyElement>, cx: &mut Context<Self>) -> AnyElement {
        let (sidebar, width) = (self.sidebar, self.side.width);
        let (back, forward) = (self.can_navigate(true), self.can_navigate(false));
        let (offset, max) = (self.tab_scroll.offset(), self.tab_scroll.max_offset());
        div()
            .id("title-bar")
            // As wide as the window, not as its tabs: wider, the tabs pushed the + button off the window instead of
            // scrolling.
            .w_full()
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
                self.follow_slide(
                    "title-bar-slide",
                    // Open, it is as wide as the sidebar plus the project bar's padding, so the tabs
                    // start where the project name does; sliding, the tabs move with the sidebar's edge.
                    div()
                        .h_full()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .pl(px(ui::TRAFFIC_LIGHTS))
                        .when(sidebar, |left| left.w(px(width + 12.0)))
                        .when(!sidebar, |left| left.pr(px(4.0)))
                        .child(
                            shortcut_icon_button(
                                "toggle-sidebar",
                                Icon::PanelLeft,
                                if sidebar {
                                    "Hide sidebar"
                                } else {
                                    "Show sidebar"
                                },
                                Some(shortcut_label("B", false).into()),
                                ButtonSize::Sm,
                            )
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(ToggleSidebar), cx)
                            }),
                        )
                        .child(nav_button("go-back", Icon::ArrowLeft, "Back", "[", back))
                        .child(nav_button(
                            "go-forward",
                            Icon::ArrowRight,
                            "Forward",
                            "]",
                            forward,
                        )),
                    move |left, share, _| left.w_auto().min_w(px((width + 12.0) * share)),
                ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        div()
                            .id("tabs")
                            .role(Role::TabList)
                            .aria_label("Tabs")
                            .size_full()
                            .flex()
                            .items_center()
                            .gap(px(2.0))
                            .overflow_x_scroll()
                            .track_scroll(&self.tab_scroll)
                            .children(tabs)
                            .children(crate::ui::cutoff::clips_on_purpose()),
                    )
                    .children(self.tab_edge(90.0, offset.x < px(-TAB_EDGE_SLACK)))
                    .children(self.tab_edge(270.0, offset.x > -max.x + px(TAB_EDGE_SLACK))),
            )
            // With no project there is nothing to open a tab in.
            .when(!self.welcome, |bar| bar.child(self.new_tab_buttons(cx)))
            .children(self.tab_drag_listeners(cx))
            .into_any_element()
    }

    /// A shade over one end of the tab strip where tabs run on past it, so a cut tab reads as continuing. `angle` is the
    /// gradient's, 90 for the start and 270 for the end; it is black, as the bar's own ground may be see-through.
    fn tab_edge(&self, angle: f32, more: bool) -> Option<gpui::Div> {
        let shade = gpui::linear_gradient(
            angle,
            gpui::linear_color_stop(gpui::rgba(0x0000005c), 0.0),
            gpui::linear_color_stop(gpui::rgba(0x00000000), 1.0),
        );
        more.then(|| {
            // No id and no handlers, so a click or a drag on the tab under the shade still reaches the tab.
            let edge = div().absolute().top_0().h_full().w(px(TAB_EDGE)).bg(shade);
            if angle < 180.0 {
                edge.left_0().children(ui::spot(|| "tabs-fade-start".into()))
            } else {
                edge.right_0().children(ui::spot(|| "tabs-fade-end".into()))
            }
        })
    }

    /// The + button, and the chevron beside it that opens the menu of what a new tab can be. With "Choose each time"
    /// in Settings, + opens the menu too.
    fn new_tab_buttons(&self, cx: &mut Context<Self>) -> AnyElement {
        let ask = self.prefs.new_tab == "ask";
        let pages: Vec<NewPage> = self
            .plugins
            .iter()
            .flat_map(|installed| {
                installed
                    .pages
                    .iter()
                    .filter(|page| page.new_tab)
                    .map(move |page| NewPage {
                        plugin: installed.id.clone(),
                        view: page.id.clone(),
                        title: page.title.clone(),
                        icon: installed
                            .plugin
                            .manifest
                            .contributes
                            .views
                            .iter()
                            .find(|view| view.id == page.id)
                            .map(|view| view.icon.clone())
                            .unwrap_or_default(),
                        own: installed.plugin.icon_svg.clone(),
                    })
            })
            .collect();
        let has_page = self.project_info.is_some();
        let mut items = vec![
            ui::menu_item("new-tab-terminal", Icon::Terminal, "Terminal", false, false)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.new_tab_menu = false;
                    this.new_terminal(window, cx);
                }))
                .into_any_element(),
            ui::menu_item(
                "new-tab-project-page",
                Icon::FileText,
                "Project page",
                false,
                !has_page,
            )
            .when(has_page, |item| {
                item.on_click(cx.listener(|this, _, window, cx| {
                    this.new_tab_menu = false;
                    this.open_project_page(this.config.project.clone(), window, cx);
                }))
            })
            .into_any_element(),
        ];
        items.extend(pages.into_iter().map(|page| {
            let NewPage {
                plugin,
                view,
                title,
                icon,
                own,
            } = page;
            ui::menu_item_with(
                SharedString::from(format!("new-tab-{plugin}-{view}")),
                plugin_glyph(&icon, own.as_deref(), 16.0, theme::muted_foreground()),
                title,
                false,
                false,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.new_tab_menu = false;
                this.open_plugin_view(&plugin, &view, None, window, cx);
            }))
            .into_any_element()
        }));
        let menu_open = self.new_tab_menu;
        let toggle_menu =
            move |this: &mut Self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                this.new_tab_menu = !ui::behaviors::was_open("new-tab-menu", menu_open, cx);
                cx.notify_workspace();
            };
        div()
            .flex_none()
            .flex()
            .flex_col()
            .ml(px(4.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        shortcut_icon_button(
                            "new-terminal",
                            Icon::Plus,
                            "New tab",
                            Some(shortcut_label("T", false).into()),
                            ButtonSize::Sm,
                        )
                        .map(|button| {
                            if ask {
                                ui::behaviors::note_press(button, "new-tab-menu", menu_open)
                                    .on_click(cx.listener(toggle_menu))
                            } else {
                                button.on_click(cx.listener(|this, _, window, cx| {
                                    this.new_tab_button(window, cx)
                                }))
                            }
                        })
                        .track_focus(&self.new_focus),
                    )
                    // The desktop's `-ml-1 w-4` chevron, a narrow ghost button.
                    .child(
                        ui::behaviors::note_press(
                            ui::ghost_button(
                                "new-tab-menu",
                                "New tab menu",
                                None,
                                ButtonSize::Sm,
                                None,
                                icon(Icon::ChevronDown, 12.0, theme::muted_foreground()),
                            ),
                            "new-tab-menu",
                            menu_open,
                        )
                        .w(px(16.0))
                        .ml(px(-4.0))
                        .on_click(cx.listener(toggle_menu)),
                    ),
            )
            .children(
                self.popover_phase("new-tab-menu", self.new_tab_menu, cx)
                    .map(|phase| {
                        motion::with_phase(phase, || {
                            ui::popover(
                                ui::menu("new-tab-items", "New tab")
                                    // A popover starts where its column does, so it clears the buttons itself.
                                    .mt(px(32.0))
                                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                        this.new_tab_menu = false;
                                        cx.notify_workspace();
                                    }))
                                    .children(items),
                            )
                        })
                    }),
            )
            .into_any_element()
    }
}

/// Back or Forward: the desktop's `IconButton`, dimmed and inert while there is nowhere to go.
fn nav_button(
    id: &'static str,
    glyph: Icon,
    label: &'static str,
    key: &str,
    enabled: bool,
) -> AnyElement {
    // The desktop's `alt` commands: Alt on top of the usual modifier.
    let shortcut = if cfg!(target_os = "macos") {
        format!("⌥⌘{key}")
    } else {
        format!("Ctrl+Shift+Alt+{key}")
    };
    if !enabled {
        return div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .size(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .opacity(0.5)
            .a11y_synthetic_children(|builder| {
                builder.parent_node().set_disabled();
            })
            .child(icon(glyph, 16.0, theme::muted_foreground()))
            .into_any_element();
    }
    let action: Box<dyn gpui::Action> = if id == "go-back" {
        Box::new(GoBack)
    } else {
        Box::new(GoForward)
    };
    shortcut_icon_button(id, glyph, label, Some(shortcut.into()), ButtonSize::Sm)
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        .into_any_element()
}
