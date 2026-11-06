//! The Settings dialog, laid out like the desktop's: sections on the left, and each setting a row
//! with what it does and its control. It lists only the settings the native app honours.

use gpui::{
    AnyElement, Context, Entity, FocusHandle, FontWeight, KeyDownEvent, MouseButton, Pixels, Role,
    SharedString, Subscription, Window, div, prelude::*, px, rgb, rgba,
};

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

use super::{
    HAIRLINE, Workspace, shortcut_label,
    sound::{self, SoundId, SoundSettings},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Section {
    General,
    Text,
    Cursor,
    History,
    Keyboard,
    Editor,
    Agents,
    Notifications,
    Plugins,
}

impl Section {
    const ALL: [Section; 9] = [
        Section::General,
        Section::Text,
        Section::Cursor,
        Section::History,
        Section::Keyboard,
        Section::Editor,
        Section::Agents,
        Section::Notifications,
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
            Section::Agents => "Agents",
            Section::Notifications => "Notifications",
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
            Section::Agents => Icon::Bot,
            Section::Notifications => Icon::Bell,
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
    /// "Let Claude run commands in new tabs": `open_pane` presses Enter after the command it types.
    pub(super) agents_run_commands: bool,
    /// "Let agents message each other": a message for an agent is submitted with Enter.
    pub(super) agents_message: bool,
    /// The sound for new notifications.
    pub(super) notify_sound: SoundSettings,
    /// The diff viewer wraps long lines.
    pub(super) diff_wrap: bool,
    /// The diff viewer lists files as a folder tree.
    pub(super) diff_tree: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            quick_terminal: true,
            editor: None,
            new_tab: "terminal".into(),
            new_project_page: false,
            agents_run_commands: false,
            agents_message: false,
            notify_sound: SoundSettings::default(),
            diff_wrap: false,
            diff_tree: false,
        }
    }
}

pub(super) struct SettingsDialog {
    pub(super) section: Section,
    pub(super) focus: FocusHandle,
    pub(super) family: Entity<TextField>,
    pub(super) family_error: Option<SharedString>,
    _family_events: Subscription,
    /// The select whose menu is open.
    menu: Option<&'static str>,
    /// The Plugins section's GitHub link field.
    pub(super) link: Entity<TextField>,
    _link_events: Subscription,
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
                "JetBrains Mono",
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
        let link = cx.new(|cx| {
            let mut field = TextField::new(
                Role::TextInput,
                "GitHub repo link",
                "github.com/owner/plugin",
                px(176.0),
                cx,
            );
            field.fill = true;
            field
        });
        let link_events =
            cx.subscribe_in(&link, window, |this, _, event, window, cx| match event {
                FieldEvent::Submit { .. } => this.install_from_link(window, cx),
                FieldEvent::Cancel => this.close_settings(window, cx),
                FieldEvent::Changed => cx.notify(),
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
            link,
            _link_events: link_events,
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
        self.select_of_width(id, label, choices, px(176.0), cx)
    }

    fn select_of_width(
        &self,
        id: &'static str,
        label: &'static str,
        choices: Vec<Choice>,
        width: Pixels,
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
            .child(
                ui::select_trigger(id, label, value)
                    .w(width)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(dialog) = &mut this.settings {
                            dialog.menu = (dialog.menu != Some(id)).then_some(id);
                        }
                        cx.notify();
                    })),
            )
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
                    ("ask".into(), "Choose each time".into()),
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
                            "What the + button does: open a terminal or the Project page, choose each time from the \
                             menu, or open a plugin page. Plugin pages that declare \"newTab\": true in their view show \
                             here and in the menu next to the button. {} always opens a terminal.",
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
                        "Tried before the default monospace fonts. Leave empty for the default. Enter applies it.",
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
                self.switch_row(
                    "cursor-blink",
                    "Blink",
                    "The cursor turns off and on in the pane you're typing in.",
                    current.cursor_blink,
                    |this, on, cx| this.set_look(|look| look.cursor_blink = on, cx),
                    cx,
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
            Section::Agents => self.agents_cards(cx),
            Section::Notifications => self.notification_cards(cx),
            Section::Plugins => Vec::new(),
        }
    }

    /// Claude Code's connection, as the desktop's Agents chip shows it: whether its sessions get Wings' tools, and its
    /// status line and hooks go through Wings, with Connect, Reconnect and Disconnect for every account at once, then
    /// each account's own state.
    /// The desktop's Notifications section: whether a sound plays, which one, and whether it also plays for the pane
    /// on screen.
    fn notification_cards(&self, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        let current = self.prefs.notify_sound;
        let choices = SoundId::ALL
            .into_iter()
            .map(|id| {
                let choose: Choose = Box::new(move |this, cx| {
                    this.set_prefs(|prefs| prefs.notify_sound.sound = id, cx)
                });
                (id.label().into(), id == current.sound, choose)
            })
            .collect();
        let preview = ui::button(
            "notify-sound-preview",
            "Preview the sound",
            ui::Variant::Outline,
            false,
            "Preview",
        )
        .px(px(10.0))
        .on_click(move |_, _, _| sound::play(current.sound));
        let watching = ui::switch("notify-sound-watching", current.watching).map(|switch| {
            if current.enabled {
                switch.on_click(cx.listener(move |this, _, _, cx| {
                    this.set_prefs(|prefs| prefs.notify_sound.watching = !current.watching, cx)
                }))
            } else {
                switch
                    .opacity(0.5)
                    .cursor_default()
                    .a11y_synthetic_children(|builder| {
                        builder.parent_node().set_disabled();
                    })
            }
        });
        vec![vec![
            self.switch_row(
                "notify-sound",
                "Play a sound",
                "When an agent needs you or finishes, or a plugin notifies. Several at once play one sound.",
                current.enabled,
                |this, on, cx| this.set_prefs(|prefs| prefs.notify_sound.enabled = on, cx),
                cx,
            ),
            ui::setting_row(
                "Sound",
                "",
                None,
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(preview)
                    // The names are short, and a full-width select would leave the label no room at 640px.
                    .child(self.select_of_width("notify-sound-pick", "Sound", choices, px(112.0), cx)),
            ),
            ui::setting_row(
                "Also for the pane on screen",
                "Play it when the agent you're looking at needs you or finishes.",
                None,
                watching,
            ),
        ]]
    }

    fn agents_cards(&self, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        use super::claude::Change;
        let link = &self.claude;
        let default = link
            .accounts
            .as_ref()
            .and_then(|accounts| accounts.first())
            .map(|account| account.status.clone());
        let state = match &default {
            None if link.busy => "Checking…",
            None => "Not checked",
            Some(s) if !s.claude => "Not installed",
            Some(s) if s.connected && s.statusline => "Connected",
            Some(s) if s.connected || s.statusline => "Partly connected",
            Some(_) => "Not connected",
        };
        let installed = default.as_ref().is_some_and(|s| s.claude);
        let off = state == "Not connected";
        let summary = match state {
            "Not installed" => "Install Claude Code to give it plugin tools.",
            "Connected" => {
                "Its sessions get Wings' and plugin tools, and plugins get its usage numbers. Your own status line \
                 still shows."
            }
            _ => {
                "Connect it so its sessions get Wings' and plugin tools and plugins get its usage numbers. Your own \
                 status line keeps showing, and Disconnect puts your settings back as they were."
            }
        };
        let busy = link.busy;
        let mut buttons = div().flex().items_center().gap(px(8.0));
        if installed && !off {
            buttons = buttons.child(
                ui::button(
                    "claude-disconnect",
                    "Disconnect Claude Code",
                    ui::Variant::Ghost,
                    busy,
                    "Disconnect",
                )
                .when(!busy, |b| {
                    b.on_click(
                        cx.listener(|this, _, _, cx| this.change_claude(Change::Disconnect, cx)),
                    )
                }),
            );
        }
        if installed {
            let (label, variant) = if state == "Connected" {
                ("Reconnect", ui::Variant::Outline)
            } else {
                ("Connect", ui::Variant::Secondary)
            };
            buttons = buttons.child(
                ui::button(
                    "claude-connect",
                    format!("{label} Claude Code"),
                    variant,
                    busy,
                    label,
                )
                .when(!busy, |b| {
                    b.on_click(
                        cx.listener(|this, _, _, cx| this.change_claude(Change::Connect(None), cx)),
                    )
                }),
            );
        }
        let error = link.error.clone().map(SharedString::from);
        // The buttons go under the text, so a narrow window doesn't squeeze it into a column of single words.
        let mut main = vec![
            div()
                .flex()
                .flex_col()
                .child(ui::setting_row(
                    format!("Claude Code · {state}"),
                    format!(
                        "{summary} Connect and Disconnect apply to every account a project uses."
                    ),
                    error,
                    div(),
                ))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .px(px(16.0))
                        .pb(px(12.0))
                        .child(buttons),
                )
                .into_any_element(),
        ];
        if let Some(status) = default.as_ref().filter(|s| s.connected) {
            let on = status.always_load;
            main.push(ui::setting_row(
                "Load Wings tools at session start",
                "Claude sees every plugin tool from the first message, instead of only when something tells it to look. \
                 Takes more of its context. New sessions only.",
                None,
                ui::switch("claude-always-load", on).when(!busy, |s| {
                    s.on_click(cx.listener(move |this, _, _, cx| this.change_claude(Change::Connect(Some(!on)), cx)))
                }),
            ));
        }
        let accounts: Vec<AnyElement> = link
            .accounts
            .iter()
            .flatten()
            .map(|account| {
                let s = &account.status;
                let state = match (s.connected, s.statusline) {
                    (true, true) => "Connected",
                    (true, false) => "Tools only: no status line or hooks",
                    (false, true) => "Status line and hooks only: no tools",
                    (false, false) => "Not connected",
                };
                let (name, folder) = match &account.name {
                    None => ("Default account".to_owned(), "~/.claude".to_owned()),
                    Some(name) => (name.clone(), format!("~/.claude-{name}")),
                };
                ui::setting_row(
                    name,
                    folder,
                    None,
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child(state),
                )
            })
            .collect();
        let note = div()
            .px(px(16.0))
            .py(px(12.0))
            .text_size(px(12.0))
            .text_color(rgb(theme::MUTED_FOREGROUND))
            .child("Not here yet: OpenCode is connected from the desktop app.")
            .into_any_element();
        let enter = vec![
            self.switch_row(
                "agents-run-commands",
                "Let Claude run commands in new tabs",
                "When Claude opens a tab for a command, like a dev server, Wings presses Enter for it. Claude Code still \
                 asks you before it opens the tab, unless you allowed open_pane without asking. Off: the command waits \
                 for you to press Enter.",
                self.prefs.agents_run_commands,
                |this, on, cx| this.set_prefs(|prefs| prefs.agents_run_commands = on, cx),
                cx,
            ),
            self.switch_row(
                "agents-message",
                "Let agents message each other",
                "When an agent, or a plugin like the PR tracker, sends a message to another agent, Wings presses Enter \
                 for it, so that agent starts on it. Off: the message waits in its prompt for you to press Enter.",
                self.prefs.agents_message,
                |this, on, cx| this.set_prefs(|prefs| prefs.agents_message = on, cx),
                cx,
            ),
        ];
        let mut cards = vec![main];
        if !accounts.is_empty() {
            cards.push(accounts);
        }
        cards.push(enter);
        cards.push(vec![note]);
        cards
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
                // Claude Code's state is read each time the section opens, as the desktop's does.
                if section == Section::Agents {
                    this.load_claude_status(cx);
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
                    content.child(self.plugins_section(cx))
                } else {
                    content.children(self.section_cards(section, cx).into_iter().map(ui::card))
                }
            });
        // The desktop's `nav w-52 gap-3 p-3 pt-4 bg-black/15`: the title, the sections, then the version at the foot.
        let sidebar = div()
            .w(px(208.0))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(12.0))
            .pt(px(16.0))
            .bg(rgba(0x00000026))
            .border_r_1()
            .border_color(rgba(HAIRLINE))
            .child(
                div()
                    .px(px(8.0))
                    .child(
                        div()
                            .text_size(px(16.0))
                            .line_height(px(16.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Settings"),
                    )
                    .child(
                        div()
                            .mt(px(4.0))
                            .text_size(px(12.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child("These apply to every terminal right away."),
                    ),
            )
            .child(div().flex().flex_col().gap(px(2.0)).children(nav).when(
                !plugin_nav.is_empty(),
                |list| {
                    list.child(
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
                },
            ))
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
                // `w-[min(820px,calc(100%-4rem))]` and the same for its height: 32px clear of each edge.
                .p(px(32.0))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                )
                .child(
                    panel
                        .max_w_full()
                        .max_h_full()
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                this.close_settings(window, cx);
                            }
                        }))
                        .child(sidebar)
                        .child(content)
                        .child(
                            div().absolute().top(px(8.0)).right(px(8.0)).child(
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

    /// The desktop sidebar's last row: Settings, with its shortcut, as its `SidebarNavRow`, muted until pointed at.
    pub(super) fn settings_row(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("open-settings")
            .role(Role::Button)
            .aria_label("Settings")
            .h(px(28.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(8.0))
            .rounded(px(theme::RADIUS))
            .cursor_pointer()
            .text_size(px(13.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(theme::MUTED_FOREGROUND))
            .hover(|style| {
                style
                    .bg(rgba(theme::HOVER))
                    .text_color(rgb(theme::FOREGROUND))
            })
            .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx)))
            .child(icon(Icon::Settings, 16.0, theme::MUTED_FOREGROUND))
            .child(div().flex_1().child("Settings"))
            // The desktop's `RowHint`: plain text at 60%, not a keycap.
            .child(
                div()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .opacity(0.6)
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
        .gap(px(8.0))
        .px(px(8.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(rgba(0))
        .cursor_pointer()
        .text_size(px(13.0))
        .font_weight(FontWeight::MEDIUM)
        // The desktop's section trigger: muted, and bright on `bg-white/[0.07]` when chosen.
        .when(selected, |item| {
            item.bg(rgba(0xffffff12)).text_color(rgb(theme::FOREGROUND))
        })
        .when(!selected, |item| {
            item.text_color(rgb(theme::MUTED_FOREGROUND))
                .hover(|style| style.text_color(rgb(theme::FOREGROUND)))
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
