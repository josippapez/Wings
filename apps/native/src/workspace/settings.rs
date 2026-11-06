//! The Settings dialog, laid out like the desktop's: sections on the left, and each setting a row
//! with what it does and its control. It lists only the settings the native app honours.

use gpui::{
    AnyElement, Context, Entity, FocusHandle, FontWeight, KeyDownEvent, MouseButton, Role,
    SharedString, Subscription, Window, div, prelude::*, px, rgb, rgba,
};
use wings_plugins::store::Store;

use crate::{
    icons::{Icon, icon},
    installed::WINGS_VERSION,
    terminal_view::{
        CONTRASTS, CURSOR_STYLES, DEFAULT_FONT_FAMILY, INACTIVE_CURSORS, LINE_HEIGHTS, SCROLLBACKS,
        TerminalLook, look,
    },
    text_field::{FieldEvent, TextField},
    theme,
    ui::{self, ButtonSize},
};

use super::{HAIRLINE, Workspace, project::SIDEBAR_ACCENT, shortcut_label};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Section {
    General,
    Text,
    Cursor,
    History,
    Keyboard,
    Editor,
    Plugins,
}

impl Section {
    const ALL: [Section; 7] = [
        Section::General,
        Section::Text,
        Section::Cursor,
        Section::History,
        Section::Keyboard,
        Section::Editor,
        Section::Plugins,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Text => "Text",
            Section::Cursor => "Cursor",
            Section::History => "History and selection",
            Section::Keyboard => "Keyboard",
            Section::Editor => "Editor",
            Section::Plugins => "Plugins",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Section::General => Icon::SlidersHorizontal,
            Section::Text => Icon::Type,
            Section::Cursor => Icon::TextCursor,
            Section::History => Icon::ScrollText,
            Section::Keyboard => Icon::Keyboard,
            Section::Editor => Icon::Code,
            Section::Plugins => Icon::Puzzle,
        }
    }
}

/// What the app keeps besides how terminals look, read at launch and saved with the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Prefs {
    /// Ctrl-` shows and hides the quick terminal.
    pub(super) quick_terminal: bool,
    /// The editor chosen in Settings, else the first one found.
    pub(super) editor: Option<String>,
    /// What the + button opens: `terminal`, `project` or a plugin page as `plugin:view`.
    pub(super) new_tab: String,
    /// A project you add opens its Project page instead of a terminal.
    pub(super) new_project_page: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            quick_terminal: true,
            editor: None,
            new_tab: "terminal".into(),
            new_project_page: false,
        }
    }
}

/// An installed plugin as the Plugins section lists it.
pub(super) struct PluginRow {
    pub(super) name: String,
    version: String,
    description: String,
    /// The desktop's "On", "Off" or "Waiting for approval", and whether it runs here.
    pub(super) status: String,
    pub(super) on: bool,
}

pub(super) struct SettingsDialog {
    pub(super) section: Section,
    focus: FocusHandle,
    pub(super) family: Entity<TextField>,
    pub(super) family_error: Option<SharedString>,
    _family_events: Subscription,
    /// The select whose menu is open.
    menu: Option<&'static str>,
    pub(super) plugins: Vec<PluginRow>,
}

/// What choosing a select's item does.
type Choose = Box<dyn Fn(&mut Workspace, &mut Context<Workspace>)>;
/// A select's choice: its label, whether it is the current one, and what choosing it does.
type Choice = (SharedString, bool, Choose);

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
            menu: None,
            plugins: self.plugin_rows(),
        });
        cx.notify();
    }

    /// The desktop's installed plugins with their state, read from its files and never written: turning one on or off
    /// and approving it happen in the desktop app.
    fn plugin_rows(&self) -> Vec<PluginRow> {
        let Some(paths) = &self.config.plugins else {
            return Vec::new();
        };
        let store = Store::load(paths.dir.clone(), paths.state.clone(), paths.dev.as_deref());
        store
            .list()
            .into_iter()
            .map(|view| {
                let on = view.enabled && view.approved;
                let runs = self
                    .plugins
                    .iter()
                    .any(|installed| installed.id == view.manifest.id);
                let state = if !view.approved {
                    "Waiting for approval"
                } else if view.enabled {
                    "On"
                } else {
                    "Off"
                };
                let status = match (on, runs) {
                    (true, true) => format!("{state} · Runs here"),
                    (true, false) => format!("{state} · No native build yet"),
                    _ => state.to_owned(),
                };
                PluginRow {
                    name: view.manifest.name,
                    version: view.manifest.version,
                    description: view.manifest.description,
                    status,
                    on,
                }
            })
            .collect()
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
        if installed && look(cx).family != family {
            self.set_look(|look| look.family = family, cx);
        }
        cx.notify();
    }

    /// Changes how every terminal looks or types, saves it, and redraws them.
    pub(super) fn set_look(
        &mut self,
        change: impl FnOnce(&mut TerminalLook),
        cx: &mut Context<Self>,
    ) {
        let mut current = look(cx);
        change(&mut current);
        cx.set_global(current);
        self.save(cx);
        cx.refresh_windows();
    }

    pub(super) fn set_shift_return_meta(&mut self, on: bool, cx: &mut Context<Self>) {
        self.set_look(|look| look.shift_return_meta = on, cx);
    }

    pub(super) fn set_prefs(&mut self, change: impl FnOnce(&mut Prefs), cx: &mut Context<Self>) {
        change(&mut self.prefs);
        self.save(cx);
        cx.notify();
    }

    /// A select: its trigger, and its menu while open. A choice closes the menu.
    fn select(
        &self,
        id: &'static str,
        label: &'static str,
        choices: Vec<Choice>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.settings.as_ref().is_some_and(|d| d.menu == Some(id));
        let value = choices
            .iter()
            .find(|(_, selected, _)| *selected)
            .map_or_else(SharedString::default, |(label, ..)| label.clone());
        let items: Vec<AnyElement> = choices
            .into_iter()
            .enumerate()
            .map(|(index, (label, selected, choose))| {
                ui::select_item(SharedString::from(format!("{id}-{index}")), label, selected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(dialog) = &mut this.settings {
                            dialog.menu = None;
                        }
                        choose(this, cx);
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .child(ui::select_trigger(id, label, value).on_click(cx.listener(
                move |this, _, _, cx| {
                    if let Some(dialog) = &mut this.settings {
                        dialog.menu = (dialog.menu != Some(id)).then_some(id);
                    }
                    cx.notify();
                },
            )))
            .when(open, |column| {
                column.child(ui::popover(ui::select_menu(items).on_mouse_down_out(
                    cx.listener(|this, _, _, cx| {
                        if let Some(dialog) = &mut this.settings {
                            dialog.menu = None;
                        }
                        cx.notify();
                    }),
                )))
            })
            .into_any_element()
    }

    /// The choices of a terminal look setting: each value with its label, and the field it sets.
    fn look_choices<T: Copy + PartialEq + 'static>(
        values: impl IntoIterator<Item = (T, String)>,
        current: T,
        set: fn(&mut TerminalLook, T),
    ) -> Vec<Choice> {
        values
            .into_iter()
            .map(|(value, label)| {
                let choose: Choose =
                    Box::new(move |this, cx| this.set_look(|look| set(look, value), cx));
                (label.into(), value == current, choose)
            })
            .collect()
    }

    fn switch_row(
        &self,
        id: &'static str,
        label: &'static str,
        hint: &'static str,
        on: bool,
        toggle: impl Fn(&mut Workspace, bool, &mut Context<Workspace>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        ui::setting_row(
            label,
            hint,
            None,
            ui::switch(id, on).on_click(cx.listener(move |this, _, _, cx| toggle(this, !on, cx))),
        )
    }

    fn section_cards(&self, section: Section, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        let current = look(cx);
        match section {
            Section::General => {
                let mut tabs: Vec<(String, String)> = vec![
                    ("terminal".into(), "Terminal".into()),
                    ("project".into(), "Project page".into()),
                ];
                for installed in &self.plugins {
                    for page in installed.pages.iter().filter(|page| page.new_tab) {
                        tabs.push((format!("{}:{}", installed.id, page.id), page.title.clone()));
                    }
                }
                let chosen = if tabs.iter().any(|(key, _)| *key == self.prefs.new_tab) {
                    self.prefs.new_tab.clone()
                } else {
                    "terminal".into()
                };
                let new_tab = tabs
                    .into_iter()
                    .map(|(key, label)| {
                        let selected = key == chosen;
                        let choose: Choose =
                            Box::new(move |this, cx| {
                                let key = key.clone();
                                this.set_prefs(|prefs| prefs.new_tab = key, cx)
                            });
                        (label.into(), selected, choose)
                    })
                    .collect();
                let page = self.prefs.new_project_page;
                let opens = [(false, "Terminal"), (true, "Project page")]
                    .map(|(value, label)| {
                        let choose: Choose =
                            Box::new(move |this, cx| {
                                this.set_prefs(|prefs| prefs.new_project_page = value, cx)
                            });
                        (label.into(), value == page, choose)
                    })
                    .into();
                vec![vec![
                    ui::setting_row(
                        "New tab button opens",
                        format!(
                            "What the + button does: open a terminal or the Project page, or open a plugin page. \
                             Plugin pages that declare \"newTab\": true in their view show here. {} always opens a \
                             terminal.",
                            shortcut_label("T", false)
                        ),
                        None,
                        self.select("new-tab-page", "New tab button opens", new_tab, cx),
                    ),
                    ui::setting_row(
                        "New projects open with",
                        "The first tab of a project you add to Wings.",
                        None,
                        self.select("new-project-opens", "New projects open with", opens, cx),
                    ),
                ]]
            }
            Section::Text => {
                let family = self.settings.as_ref().map(|d| (d.family.clone(), d.family_error.clone()));
                let (field, error) = family.expect("the dialog is open");
                vec![vec![
                    ui::setting_row(
                        "Font family",
                        format!("Leave empty for the default, {DEFAULT_FONT_FAMILY}. Enter applies it."),
                        error,
                        ui::input(field),
                    ),
                    ui::setting_row(
                        "Line height",
                        "Spacing between lines, as a multiple of the font size.",
                        None,
                        self.select(
                            "line-height",
                            "Line height",
                            Self::look_choices(
                                LINE_HEIGHTS.map(|value| (value, format!("{value:.1}"))),
                                current.line_height,
                                |look, value| look.line_height = value,
                            ),
                            cx,
                        ),
                    ),
                    ui::setting_row(
                        "Minimum contrast",
                        "Lightens or darkens text that is hard to read on its background.",
                        None,
                        self.select(
                            "min-contrast",
                            "Minimum contrast",
                            Self::look_choices(
                                CONTRASTS.map(|(value, label)| (value, label.to_owned())),
                                current.min_contrast,
                                |look, value| look.min_contrast = value,
                            ),
                            cx,
                        ),
                    ),
                ]]
            }
            Section::Cursor => vec![vec![
                ui::setting_row(
                    "Cursor style",
                    "The cursor programs get until they set their own.",
                    None,
                    self.select(
                        "cursor-style",
                        "Cursor style",
                        Self::look_choices(
                            CURSOR_STYLES.map(|(shape, _, label)| (shape, label.to_owned())),
                            current.cursor,
                            |look, shape| look.cursor = shape,
                        ),
                        cx,
                    ),
                ),
                ui::setting_row(
                    "When the pane isn't focused",
                    "How the cursor looks in the panes you aren't typing in.",
                    None,
                    self.select(
                        "cursor-inactive",
                        "When the pane isn't focused",
                        Self::look_choices(
                            INACTIVE_CURSORS.map(|(style, _, label)| (style, label.to_owned())),
                            current.inactive_cursor,
                            |look, style| look.inactive_cursor = style,
                        ),
                        cx,
                    ),
                ),
            ]],
            Section::History => vec![
                vec![
                    ui::setting_row(
                        "Scrollback",
                        "Lines kept per pane. A smaller number drops older history.",
                        None,
                        self.select(
                            "scrollback",
                            "Scrollback",
                            Self::look_choices(
                                SCROLLBACKS.map(|lines| (lines, format!("{} lines", thousands(lines)))),
                                current.scrollback,
                                |look, lines| look.scrollback = lines,
                            ),
                            cx,
                        ),
                    ),
                    self.switch_row(
                        "copy-on-select",
                        "Copy on select",
                        "Copies the selection to the clipboard when you let go of the mouse.",
                        current.copy_on_select,
                        |this, on, cx| this.set_look(|look| look.copy_on_select = on, cx),
                        cx,
                    ),
                ],
                vec![self.switch_row(
                    "clipboard-write",
                    "Let programs copy to the clipboard",
                    "Lets a program in a pane, such as tmux or a remote shell, put text on your clipboard (OSC 52). \
                     Programs can never read it.",
                    current.clipboard_write,
                    |this, on, cx| this.set_look(|look| look.clipboard_write = on, cx),
                    cx,
                )],
            ],
            Section::Keyboard => vec![vec![
                self.switch_row(
                    "shift-return-meta",
                    "Shift-Return sends Meta Return",
                    "Adds a new line in Claude Code instead of sending the prompt.",
                    current.shift_return_meta,
                    |this, on, cx| this.set_shift_return_meta(on, cx),
                    cx,
                ),
                ui::setting_row(
                    "Quick terminal",
                    "Shows a terminal in your home folder over whatever you're looking at, and hides it again. Its \
                     shell keeps running while hidden.",
                    None,
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .child(
                            div()
                                .h(px(32.0))
                                .flex()
                                .items_center()
                                .px(px(12.0))
                                .rounded(px(6.0))
                                .border_1()
                                .border_color(rgba(HAIRLINE))
                                .bg(rgba(0xffffff0a))
                                .text_size(px(13.0))
                                .when(!self.prefs.quick_terminal, |key| key.opacity(0.5))
                                .child(super::quick::SHORTCUT),
                        )
                        .child(ui::switch("quick-terminal-key", self.prefs.quick_terminal).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.set_prefs(|prefs| prefs.quick_terminal = !prefs.quick_terminal, cx)
                            }),
                        )),
                ),
            ]],
            Section::Editor => {
                let current = self.default_editor().cloned();
                let control = current.as_ref().map(|current| {
                    let choices = self
                        .editors
                        .iter()
                        .map(|editor| {
                            let id = editor.id.clone();
                            let choose: Choose =
                                Box::new(move |this, cx| {
                                    let id = id.clone();
                                    this.set_prefs(|prefs| prefs.editor = Some(id), cx)
                                });
                            (editor.name.clone().into(), editor.id == current.id, choose)
                        })
                        .collect();
                    self.select("default-editor", "Open projects in", choices, cx)
                });
                vec![vec![ui::setting_row(
                    "Open projects in",
                    if current.is_some() {
                        "The first item of a project's Open in menu. The others are one click further."
                    } else {
                        "No editor found. Install VS Code, Cursor or Zed, or an editor app in /Applications."
                    },
                    None,
                    div().children(control),
                )]]
            }
            Section::Plugins => Vec::new(),
        }
    }

    /// The desktop's plugin list, read only: what is installed and whether it is on, and whether it runs here.
    fn plugins_section(&self) -> AnyElement {
        let rows: Vec<AnyElement> = self
            .settings
            .as_ref()
            .map_or(&[][..], |dialog| &dialog.plugins)
            .iter()
            .enumerate()
            .map(|(index, row)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .px(px(16.0))
                    .py(px(10.0))
                    .child(
                        div()
                            .size(px(32.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(8.0))
                            .bg(rgba(if row.on { 0xb392f026 } else { 0xffffff0f }))
                            .child(icon(
                                Icon::Puzzle,
                                16.0,
                                if row.on {
                                    0xc9b2f7
                                } else {
                                    theme::MUTED_FOREGROUND
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(13.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(row.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(11.0))
                                            .text_color(rgb(theme::MUTED_FOREGROUND))
                                            .child(row.version.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme::MUTED_FOREGROUND))
                                    .child(row.status.clone()),
                            )
                            .when(!row.description.is_empty(), |text| {
                                text.child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(rgb(theme::MUTED_FOREGROUND))
                                        .line_clamp(2)
                                        .child(row.description.clone()),
                                )
                            }),
                    )
                    // Read only: the desktop app owns which plugins are on.
                    .child(
                        ui::switch(("plugin-on", index), row.on)
                            .aria_label(format!(
                                "{} {}",
                                row.name,
                                if row.on { "on" } else { "off" }
                            ))
                            .cursor_default()
                            .opacity(0.6),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(
                        "Plugins add features to Wings. Install, approve and turn them on or off in the desktop app; \
                         this app runs the ones that are on and have a native build.",
                    ),
            )
            .child(if rows.is_empty() {
                ui::card([div()
                    .px(px(16.0))
                    .py(px(16.0))
                    .text_size(px(12.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child("No plugins yet. Add them in the desktop app's Settings, Plugins.")
                    .into_any_element()])
            } else {
                ui::card(rows)
            })
            .into_any_element()
    }

    pub(super) fn settings_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.settings.as_ref()?;
        let section = dialog.section;
        let nav = Section::ALL.map(|section| {
            let selected = dialog.section == section;
            nav_item(
                SharedString::from(format!("settings-{}", section.label())),
                section.label(),
                section.icon(),
                selected,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(dialog) = &mut this.settings {
                    dialog.section = section;
                    dialog.menu = None;
                }
                cx.notify();
            }))
        });
        // Plugins that are on and have a settings page, by name. Each opens its page in a window, as
        // `wings.openSettings` does.
        let mut plugin_settings: Vec<_> = self
            .plugins
            .iter()
            .filter(|installed| installed.settings.is_some())
            .map(|installed| (installed.id.clone(), installed.name.clone()))
            .collect();
        plugin_settings.sort_by(|a, b| a.1.cmp(&b.1));
        let plugin_nav = plugin_settings.into_iter().map(|(id, name)| {
            nav_item(
                SharedString::from(format!("plugin-settings-{id}")),
                name,
                Icon::Puzzle,
                false,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_plugin_settings(&id, cx)))
        });
        let plugin_nav: Vec<_> = plugin_nav.collect();
        let content = div()
            .id("settings-content")
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(24.0))
            .overflow_y_scroll()
            .child(
                div()
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(section.label()),
            )
            .map(|content| {
                if section == Section::Plugins {
                    content.child(self.plugins_section())
                } else {
                    content.children(self.section_cards(section, cx).into_iter().map(ui::card))
                }
            });
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
            .children(nav)
            .when(!plugin_nav.is_empty(), |sidebar| {
                sidebar
                    .child(
                        div()
                            .px(px(8.0))
                            .pt(px(12.0))
                            .pb(px(4.0))
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child("Plugin settings"),
                    )
                    .children(plugin_nav)
            })
            .child(
                div()
                    .mt_auto()
                    .px(px(8.0))
                    .text_size(px(12.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(format!("Wings {WINGS_VERSION}")),
            );
        let (backdrop, panel) = ui::dialog("settings", "Settings", &dialog.focus, 820.0, 600.0);
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

/// A row of the section list.
fn nav_item(
    id: SharedString,
    label: impl Into<SharedString>,
    glyph: Icon,
    selected: bool,
) -> gpui::Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::Tab)
        .aria_label(label.clone())
        .aria_selected(selected)
        .h(px(32.0))
        .flex_none()
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
        .child(icon(glyph, 16.0, theme::MUTED_FOREGROUND))
        .child(div().min_w_0().truncate().child(label))
}

/// `10,000`, as the desktop writes line counts.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}
