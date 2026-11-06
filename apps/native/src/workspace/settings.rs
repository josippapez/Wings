//! The Settings dialog, laid out like the desktop's: sections on the left, and each setting a row
//! with what it does and its control. It lists only the settings the native app honours.

use gpui::{
    AnyElement, Context, Entity, FocusHandle, FontWeight, KeyDownEvent, MouseButton, Role,
    SharedString, Subscription, Window, div, prelude::*, px, rgb, rgba,
};

use crate::{
    icons::{Icon, icon},
    terminal_view::{CURSOR_STYLES, CursorShape, DEFAULT_FONT_FAMILY, look},
    text_field::{FieldEvent, TextField},
    theme,
    ui::{self, ButtonSize},
};

use super::{HAIRLINE, Workspace, project::SIDEBAR_ACCENT, shortcut_label};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Section {
    Text,
    Cursor,
    Keyboard,
}

impl Section {
    const ALL: [Section; 3] = [Section::Text, Section::Cursor, Section::Keyboard];

    fn label(self) -> &'static str {
        match self {
            Section::Text => "Text",
            Section::Cursor => "Cursor",
            Section::Keyboard => "Keyboard",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Section::Text => Icon::Type,
            Section::Cursor => Icon::TextCursor,
            Section::Keyboard => Icon::Keyboard,
        }
    }
}

pub(super) struct SettingsDialog {
    pub(super) section: Section,
    focus: FocusHandle,
    pub(super) family: Entity<TextField>,
    pub(super) family_error: Option<SharedString>,
    _family_events: Subscription,
    /// The cursor style menu is open.
    cursor_menu: bool,
}

impl Workspace {
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = &self.settings {
            dialog.focus.focus(window, cx);
            return;
        }
        let family = cx.new(|cx| {
            let mut field = TextField::new(
                Role::TextInput,
                "Font family",
                DEFAULT_FONT_FAMILY,
                px(176.0),
                cx,
            );
            let current = look(cx).family;
            if current != DEFAULT_FONT_FAMILY {
                field.text = current.to_string();
            }
            field
        });
        let events = cx.subscribe_in(&family, window, |this, _, event, window, cx| match event {
            FieldEvent::Submit { .. } => this.apply_family(cx),
            FieldEvent::Cancel => this.close_settings(window, cx),
            FieldEvent::Changed => {}
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.settings = Some(SettingsDialog {
            section: Section::Text,
            focus,
            family,
            family_error: None,
            _family_events: events,
            cursor_menu: false,
        });
        cx.notify();
    }

    /// Keeps what was typed in the font field, then gives the keys back to the terminal.
    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_family(cx);
        self.settings = None;
        if let Some(pane) = self.focused_pane() {
            self.focus_pane(pane, window, cx);
        }
        cx.notify();
    }

    /// An installed family, or empty for the default. Anything else stays in the field with a note.
    pub(super) fn apply_family(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.settings else {
            return;
        };
        let typed = dialog.family.read(cx).text.trim().to_string();
        let installed = typed.is_empty() || cx.text_system().all_font_names().contains(&typed);
        dialog.family_error = (!installed).then(|| format!("{typed} isn't installed.").into());
        let family: SharedString = if typed.is_empty() {
            DEFAULT_FONT_FAMILY.into()
        } else {
            typed.into()
        };
        let mut current = look(cx);
        if installed && current.family != family {
            current.family = family;
            cx.set_global(current);
            self.save(cx);
            cx.refresh_windows();
        }
        cx.notify();
    }

    pub(super) fn set_cursor(&mut self, shape: CursorShape, cx: &mut Context<Self>) {
        let mut current = look(cx);
        current.cursor = shape;
        cx.set_global(current);
        if let Some(dialog) = &mut self.settings {
            dialog.cursor_menu = false;
        }
        self.save(cx);
        cx.notify();
    }

    pub(super) fn set_shift_return_meta(&mut self, on: bool, cx: &mut Context<Self>) {
        let mut current = look(cx);
        current.shift_return_meta = on;
        cx.set_global(current);
        self.save(cx);
        cx.notify();
    }

    pub(super) fn settings_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.settings.as_ref()?;
        let nav = Section::ALL.map(|section| {
            let selected = dialog.section == section;
            div()
                .id(SharedString::from(format!("settings-{}", section.label())))
                .role(Role::Tab)
                .aria_label(section.label())
                .aria_selected(selected)
                .h(px(32.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .px(px(8.0))
                .rounded(px(8.0))
                .cursor_pointer()
                .text_size(px(13.0))
                .when(selected, |item| item.bg(rgba(SIDEBAR_ACCENT)))
                .when(!selected, |item| {
                    item.hover(|style| style.bg(rgba(theme::HOVER)))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(dialog) = &mut this.settings {
                        dialog.section = section;
                    }
                    cx.notify();
                }))
                .child(icon(section.icon(), 16.0, theme::MUTED_FOREGROUND))
                .child(section.label())
        });
        let rows = match dialog.section {
            Section::Text => vec![ui::setting_row(
                "Font family",
                format!("Leave empty for the default, {DEFAULT_FONT_FAMILY}. Enter applies it."),
                dialog.family_error.clone(),
                ui::input(dialog.family.clone()),
            )],
            Section::Cursor => {
                let current = look(cx).cursor;
                let label = CURSOR_STYLES
                    .iter()
                    .find(|(shape, ..)| *shape == current)
                    .map_or("Block", |(_, _, label)| *label);
                let items = CURSOR_STYLES.map(|(shape, name, label)| {
                    ui::select_item(
                        SharedString::from(format!("cursor-{name}")),
                        label,
                        shape == current,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set_cursor(shape, cx)))
                    .into_any_element()
                });
                let open = dialog.cursor_menu;
                vec![ui::setting_row(
                    "Cursor style",
                    "The cursor programs get until they set their own.",
                    None,
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            ui::select_trigger("cursor-style", "Cursor style", label).on_click(
                                cx.listener(|this, _, _, cx| {
                                    if let Some(dialog) = &mut this.settings {
                                        dialog.cursor_menu = !dialog.cursor_menu;
                                    }
                                    cx.notify();
                                }),
                            ),
                        )
                        .when(open, |column| {
                            column.child(ui::popover(ui::select_menu(items).on_mouse_down_out(
                                cx.listener(|this, _, _, cx| {
                                    if let Some(dialog) = &mut this.settings {
                                        dialog.cursor_menu = false;
                                    }
                                    cx.notify();
                                }),
                            )))
                        }),
                )]
            }
            Section::Keyboard => {
                let on = look(cx).shift_return_meta;
                vec![ui::setting_row(
                    "Shift-Return sends Meta Return",
                    "Adds a new line in Claude Code instead of sending the prompt.",
                    None,
                    ui::switch("shift-return-meta", on).on_click(
                        cx.listener(move |this, _, _, cx| this.set_shift_return_meta(!on, cx)),
                    ),
                )]
            }
        };
        let content = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(24.0))
            .child(
                div()
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(dialog.section.label()),
            )
            .child(ui::card(rows));
        let sidebar = div()
            .w(px(208.0))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .p(px(16.0))
            .bg(rgba(0x0000001f))
            .border_r_1()
            .border_color(rgba(HAIRLINE))
            .child(
                div()
                    .px(px(4.0))
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Settings"),
            )
            .child(
                div()
                    .px(px(4.0))
                    .pb(px(16.0))
                    .text_size(px(12.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child("These apply to every terminal right away."),
            )
            .children(nav);
        let (backdrop, panel) = ui::dialog("settings", "Settings", &dialog.focus, 820.0, 576.0);
        Some(
            backdrop
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                )
                .child(
                    panel
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                this.close_settings(window, cx);
                            }
                        }))
                        .child(sidebar)
                        .child(content)
                        .child(
                            div().absolute().top(px(12.0)).right(px(12.0)).child(
                                ui::icon_button(
                                    "close-settings",
                                    Icon::X,
                                    "Close settings",
                                    ButtonSize::Sm,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| this.close_settings(window, cx),
                                )),
                            ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The desktop sidebar's last row: Settings, with its shortcut.
    pub(super) fn settings_row(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("open-settings")
            .role(Role::Button)
            .aria_label("Settings")
            .h(px(32.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(8.0))
            .rounded(px(8.0))
            .cursor_pointer()
            .text_size(px(13.0))
            .hover(|style| style.bg(rgba(theme::HOVER)))
            .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx)))
            .child(icon(Icon::Settings, 16.0, theme::MUTED_FOREGROUND))
            .child(div().flex_1().child("Settings"))
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(shortcut_label(",", false)),
            )
            .into_any_element()
    }
}
