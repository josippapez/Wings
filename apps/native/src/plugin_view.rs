use gpui::{
    App, Context, Div, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Role, SharedString,
    Stateful, UniformListScrollHandle, Window, div, prelude::*, px, rgb, rgba, uniform_list,
};

use crate::{
    plugin::protocol::{Action, Screen, ScreenAction, ScreenState, Status, TaskType},
    theme,
};

const ROW_HEIGHT: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssuesEvent {
    Refresh,
}

enum ViewState {
    Setup,
    Loading,
    Screen(Screen),
    Error(String),
}

/// A host-owned view of an already validated worker screen.
pub struct IssuesView {
    state: ViewState,
    title: SharedString,
    refresh: Option<ScreenAction>,
    focus: FocusHandle,
    refresh_focus: FocusHandle,
    list_focus: FocusHandle,
    scroll: UniformListScrollHandle,
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
            state: ViewState::Loading,
            title: "Issues".into(),
            refresh: None,
            focus: cx.focus_handle(),
            refresh_focus: cx.focus_handle(),
            list_focus: cx.focus_handle().tab_stop(true),
            scroll: UniformListScrollHandle::new(),
        }
    }

    pub fn set_screen(&mut self, screen: Screen, cx: &mut Context<Self>) {
        self.title = screen.title.clone().into();
        self.refresh = screen
            .actions
            .iter()
            .find(|action| action.id == Action::Refresh)
            .cloned();
        self.state = ViewState::Screen(screen);
        self.state_changed(cx);
    }

    pub fn set_loading(&mut self, cx: &mut Context<Self>) {
        self.state = ViewState::Loading;
        self.state_changed(cx);
    }

    pub fn set_setup(&mut self, cx: &mut Context<Self>) {
        self.refresh = None;
        self.state = ViewState::Setup;
        self.state_changed(cx);
    }

    pub fn set_error(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.state = ViewState::Error(message.into());
        self.state_changed(cx);
    }

    fn state_changed(&mut self, cx: &mut Context<Self>) {
        self.refresh_focus = self.refresh_focus.clone().tab_stop(self.can_refresh());
        cx.notify();
    }

    fn can_refresh(&self) -> bool {
        self.refresh.is_some() && !matches!(self.state, ViewState::Loading)
    }

    fn request_refresh(&mut self, cx: &mut Context<Self>) {
        if self.can_refresh() {
            self.set_loading(cx);
            cx.emit(IssuesEvent::Refresh);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let stroke = &event.keystroke;
        let modifiers = stroke.modifiers;
        if stroke.key == "tab" && modifiers.number_of_modifiers() == u8::from(modifiers.shift) {
            if modifiers.shift {
                window.focus_prev(cx);
            } else {
                window.focus_next(cx);
            }
        } else if self.list_focus.is_focused(window) && !modifiers.modified() {
            let scroll = self.scroll.0.borrow().base_handle.clone();
            let mut offset = scroll.offset();
            let page = scroll.bounds().size.height;
            offset.y = match stroke.key.as_str() {
                "up" => offset.y + px(ROW_HEIGHT),
                "down" => offset.y - px(ROW_HEIGHT),
                "pageup" => offset.y + page,
                "pagedown" => offset.y - page,
                "home" => px(0.0),
                "end" => -scroll.max_offset().y,
                _ => return,
            }
            .clamp(-scroll.max_offset().y, px(0.0));
            scroll.set_offset(offset);
            cx.notify();
        } else {
            return;
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    fn rows(&self, range: std::ops::Range<usize>) -> Vec<Stateful<Div>> {
        let ViewState::Screen(screen) = &self.state else {
            return Vec::new();
        };
        range
            .map(|index| {
                let row = &screen.rows[index];
                issue_row(
                    index,
                    screen.rows.len(),
                    &row.id,
                    &row.title,
                    match row.status {
                        Status::Todo => "To do",
                        Status::InProgress => "In progress",
                        Status::InReview => "In review",
                        Status::Done => "Done",
                        Status::Canceled => "Canceled",
                    },
                    match row.task_type {
                        TaskType::Task => "Task",
                        TaskType::Bug => "Bug",
                        TaskType::Feature => "Feature",
                        TaskType::Epic => "Epic",
                    },
                )
            })
            .collect()
    }
}

impl Render for IssuesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.can_refresh();
        let refresh_label = self
            .refresh
            .as_ref()
            .map_or("Refresh", |action| action.label.as_str());
        let refresh = div()
            .id("issues-refresh")
            .role(Role::Button)
            .aria_label(refresh_label.to_owned())
            .track_focus(&self.refresh_focus)
            .h(px(32.0))
            .max_w(px(180.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
            .border_2()
            .border_color(rgba(theme::BORDER))
            .rounded(px(theme::RADIUS))
            .focus_visible(|style| style.border_color(rgb(theme::FOREGROUND)))
            .when(enabled, |el| {
                el.cursor_pointer()
                    .hover(|style| style.bg(rgba(theme::HOVER)))
                    .on_click(cx.listener(|this, _, _, cx| this.request_refresh(cx)))
            })
            .when(!enabled, |el| {
                el.opacity(0.5).a11y_synthetic_children(|builder| {
                    builder.parent_node().set_disabled();
                })
            })
            .child(div().min_w_0().truncate().child(refresh_label.to_owned()));

        let body = match &self.state {
            ViewState::Setup => state_panel(
                "Connect Issues storage",
                "Start with --issues-storage-root PATH and --issues-scope SCOPE to read an existing Issues installation.",
                false,
            )
            .into_any_element(),
            ViewState::Loading => state_panel("Loading issues", "", false).into_any_element(),
            ViewState::Error(error) => {
                state_panel("Couldn't load issues", error, true).into_any_element()
            }
            ViewState::Screen(screen) if screen.state == ScreenState::Error => state_panel(
                "Couldn't load issues",
                screen
                    .error
                    .as_deref()
                    .unwrap_or("The plugin returned an error."),
                true,
            )
            .into_any_element(),
            ViewState::Screen(screen)
                if screen.state == ScreenState::Empty || screen.rows.is_empty() =>
            {
                state_panel("No issues yet", "This project has no issues.", false)
                    .into_any_element()
            }
            ViewState::Screen(screen) => div()
                .id("issues-list")
                .role(Role::List)
                .aria_label("Issues")
                .aria_description("Use arrow keys, Page Up, Page Down, Home or End to scroll.")
                .track_focus(&self.list_focus)
                .size_full()
                .min_h_0()
                .flex()
                .flex_col()
                .overflow_hidden()
                .border_2()
                .border_color(rgba(0))
                .rounded(px(6.0))
                .focus_visible(|style| style.border_color(rgb(theme::FOREGROUND)))
                .child(
                    div()
                        .h(px(32.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .px(px(8.0))
                        .text_size(px(12.0))
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child(row_content("ID", "Title", "Status", "Type")),
                )
                .child(
                    uniform_list(
                        "issues-rows",
                        screen.rows.len(),
                        cx.processor(|this, range, _, _| this.rows(range)),
                    )
                    .track_scroll(&self.scroll)
                    .flex_1()
                    .min_h_0(),
                )
                .into_any_element(),
        };

        div()
            .id("native-issues")
            .role(Role::Region)
            .aria_label(self.title.clone())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(rgb(theme::BACKGROUND))
            .text_color(rgb(theme::FOREGROUND))
            .text_size(px(13.0))
            .child(
                div()
                    .id("issues-toolbar")
                    .role(Role::Toolbar)
                    .aria_label("Issues actions")
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(rgba(theme::BORDER))
                    .child(
                        div()
                            .id("issues-title")
                            .role(Role::Heading)
                            .aria_level(1)
                            .aria_label(self.title.clone())
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.title.clone()),
                    )
                    .child(refresh),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .p(px(14.0))
                    .overflow_hidden()
                    .child(body),
            )
    }
}

fn fixed_cell(value: &str, width: f32) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .truncate()
        .text_color(rgb(theme::MUTED_FOREGROUND))
        .child(value.to_owned())
}

fn row_content(id: &str, title: &str, status: &str, issue_type: &str) -> Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(12.0))
        .child(fixed_cell(id, 80.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(single_line_title(title)),
        )
        .child(fixed_cell(status, 108.0))
        .child(fixed_cell(issue_type, 72.0))
}

fn single_line_title(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn issue_row(
    index: usize,
    total: usize,
    id: &str,
    title: &str,
    status: &str,
    issue_type: &str,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.to_owned()))
        .role(Role::ListItem)
        .aria_label(format!(
            "{id}, {title}. Status: {status}. Type: {issue_type}."
        ))
        .aria_position_in_set(index + 1)
        .aria_size_of_set(total)
        .w_full()
        .h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .px(px(8.0))
        .border_b_1()
        .border_color(rgba(theme::BORDER))
        .hover(|style| style.bg(rgba(theme::HOVER)))
        .child(row_content(id, title, status, issue_type))
}

fn state_panel(title: &str, description: &str, error: bool) -> Stateful<Div> {
    div()
        .id("issues-state")
        .role(if error { Role::Alert } else { Role::Status })
        .aria_label(format!("{title}. {description}"))
        .a11y_synthetic_children(move |builder| {
            builder.parent_node().set_live(if error {
                gpui::accesskit::Live::Assertive
            } else {
                gpui::accesskit::Live::Polite
            });
        })
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.0))
        .p(px(24.0))
        .overflow_y_scroll()
        .child(
            div()
                .max_w(px(384.0))
                .text_size(px(14.0))
                .when(error, |el| el.text_color(rgb(theme::DANGER)))
                .child(title.to_owned()),
        )
        .child(
            div()
                .max_w(px(384.0))
                .text_size(px(14.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(description.to_owned()),
        )
}

#[cfg(test)]
mod tests {
    #[test]
    fn multiline_titles_have_a_single_visual_line_without_changing_the_source() {
        let title = String::from("First line\n\tsecond\r\n  line");
        assert_eq!(super::single_line_title(&title), "First line second line");
        assert_eq!(title, "First line\n\tsecond\r\n  line");
    }
}
