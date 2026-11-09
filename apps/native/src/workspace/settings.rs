//! The Settings dialog, laid out like the desktop's: sections on the left, and each setting a row
//! with what it does and its control. It lists only the settings the native app honours.

use std::{cell::Cell, time::Duration};

use gpui::{
    AnyElement, Context, Div, Entity, FocusHandle, FontWeight, KeyDownEvent, MouseButton, Pixels,
    Role, SharedString, StyleRefinement, Subscription, Window, div, prelude::*, px, rgb, rgba,
};

use crate::{
    icons::{Icon, icon},
    installed::WINGS_VERSION,
    page::PageView,
    terminal_view::{
        CONTRASTS, CURSOR_STYLES, DEFAULT_FONT_FAMILY, INACTIVE_CURSORS, LINE_HEIGHTS,
        OPTION_AS_META, SCROLLBACKS, TerminalLook, look,
    },
    text_field::{FieldEvent, TextField},
    theme,
    ui::{self, ButtonSize, motion},
};

use super::{
    HAIRLINE, Workspace,
    agent_terminals::Opens,
    shortcut_label,
    sound::{self, SoundId, SoundSettings},
};

#[derive(Clone, Debug, PartialEq)]
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
    /// A plugin's own settings page, the desktop's `plugin:<id>` section.
    Plugin(PluginSettings),
}

/// A plugin's settings page shown in Settings. Its frame runs while the section shows: choosing another section or
/// closing Settings drops it, which stops the frame, as closing a plugin's tab does.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PluginSettings {
    pub(super) plugin: String,
    /// "<name> settings", the page's name for assistive tech, as the desktop's frame has it for its `title`.
    title: SharedString,
    pub(super) page: Entity<PageView>,
}

/// The desktop dialog's sizes: `w-[min(820px,calc(100%-4rem))] h-[min(600px,…)]`, and 1100 x 760 for Plugins, which has
/// a list and a details pane side by side. A plugin's own page keeps the smaller one, as there.
const SIZE: (f32, f32) = (820.0, 600.0);
const LARGE_SIZE: (f32, f32) = (1100.0, 760.0);
/// The desktop's `transition-[width,height] duration-200 ease-out` between the two.
pub(super) const RESIZE: Duration = Duration::from_millis(200);
const EASE_OUT: (f32, f32, f32, f32) = (0.0, 0.0, 0.2, 1.0);

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

    fn label(&self) -> &'static str {
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
            Section::Plugin(_) => "Plugin settings",
        }
    }

    fn icon(&self) -> Icon {
        match self {
            Section::General => Icon::SlidersHorizontal,
            Section::Text => Icon::Type,
            Section::Cursor => Icon::TextCursor,
            Section::History => Icon::ScrollText,
            Section::Keyboard => Icon::Keyboard,
            Section::Editor => Icon::Code,
            Section::Agents => Icon::Bot,
            Section::Notifications => Icon::Bell,
            Section::Plugins | Section::Plugin(_) => Icon::Puzzle,
        }
    }

    /// The plugin whose settings page this is.
    pub(super) fn plugin(&self) -> Option<&str> {
        match self {
            Section::Plugin(settings) => Some(&settings.plugin),
            _ => None,
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
    /// "Agent terminals open": where the first terminal an agent opens goes.
    pub(super) agent_terminals_open: Opens,
    /// The sound for new notifications.
    pub(super) notify_sound: SoundSettings,
    /// Official plugins already announced as new.
    pub(super) seen_plugins: Vec<String>,
    /// The diff viewer wraps long lines.
    pub(super) diff_wrap: bool,
    /// The diff viewer lists files as a folder tree.
    pub(super) diff_tree: bool,
    /// The docked diff shows its file list.
    pub(super) diff_dock_files: bool,
    /// The shortcut that brings Wings forward from any app.
    pub(super) global_hotkey: crate::saved::GlobalHotkey,
    /// "Keep terminals running when Wings quits", as saved: `None` until changed, which is on. What this app does now
    /// is `Servers::is_on`.
    pub(super) terminal_server: Option<bool>,
    /// Wings asked once to turn on other apps' data for its terminals (`disk_access`).
    pub(super) app_data_asked: bool,
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
            agent_terminals_open: Default::default(),
            notify_sound: SoundSettings::default(),
            seen_plugins: Vec::new(),
            diff_wrap: false,
            diff_tree: false,
            diff_dock_files: true,
            global_hotkey: Default::default(),
            terminal_server: None,
            app_data_asked: false,
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
    /// Takes the next key press as the global shortcut, while its button waits for one.
    pub(super) recording: Option<Subscription>,
    /// How far the dialog is from its size to its larger size, 0 to 1, on its way when the section changed between
    /// them. Unset until it is first drawn, which draws it at its section's size from the start.
    large: Cell<Option<motion::Transition>>,
}

/// What choosing a select's item does.
type Choose = Box<dyn Fn(&mut Workspace, &mut Context<Workspace>)>;
/// A select's choice: its label, whether it is the current one, and what choosing it does.
type Choice = (SharedString, bool, Choose);

/// Where an agent's connection to Wings stands, as its card in Agents names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConnState {
    Checking,
    NotChecked,
    NotInstalled,
    Connected,
    /// Claude Code with its tools but not its status line and hooks, or the other way round.
    Partly,
    NotConnected,
}

impl ConnState {
    fn label(self) -> &'static str {
        match self {
            ConnState::Checking => "Checking…",
            ConnState::NotChecked => "Not checked",
            ConnState::NotInstalled => "Not installed",
            ConnState::Connected => "Connected",
            ConnState::Partly => "Partly connected",
            ConnState::NotConnected => "Not connected",
        }
    }

    fn installed(self) -> bool {
        matches!(
            self,
            ConnState::Connected | ConnState::Partly | ConnState::NotConnected
        )
    }
}

impl Workspace {
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = &self.settings {
            dialog.focus.focus(window, cx);
            return;
        }
        // Its row says whether the terminals can read other apps' data now, which may have changed in System Settings.
        self.check_disk_access(cx);
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
            recording: None,
            large: Cell::new(None),
        });
        cx.notify();
    }

    /// Shows `section` in Settings, which is open.
    pub(super) fn set_settings_section(&mut self, section: Section, cx: &mut Context<Self>) {
        let agents = section == Section::Agents;
        if let Some(dialog) = &mut self.settings {
            dialog.section = section;
            dialog.menu = None;
            dialog.recording = None;
        }
        // Claude Code's state is read each time the section opens, as the desktop's does.
        if agents {
            self.load_claude_status(cx);
            self.load_opencode_status(cx);
            self.load_harnesses(cx);
        }
        cx.notify();
    }

    /// The plugin whose settings page Settings shows.
    pub(super) fn settings_plugin(&self) -> Option<&str> {
        self.settings.as_ref()?.section.plugin()
    }

    /// Opens Settings on the plugin's settings page, as its row under Plugin settings and `wings.openSettings` do on the
    /// desktop. A plugin that doesn't run here or has no native settings page opens Plugins, as the desktop shows Plugins
    /// for a plugin that has none.
    pub(super) fn open_plugin_settings(
        &mut self,
        plugin: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings(window, cx);
        if self.settings_plugin() == Some(plugin) {
            return;
        }
        let section = self
            .plugin_settings_page(plugin, cx)
            .unwrap_or(Section::Plugins);
        self.set_settings_section(section, cx);
    }

    /// The plugin's settings page as a section, its frame started.
    pub(super) fn plugin_settings_page(
        &self,
        plugin: &str,
        cx: &mut Context<Self>,
    ) -> Option<Section> {
        let installed = self
            .plugins
            .iter()
            .find(|installed| installed.id == plugin)?;
        let settings = installed.settings.as_ref()?;
        let (source, title) = (settings.source.clone(), settings.title.clone());
        let host = installed.host.clone();
        // On the dialog's own ground, as the desktop's frame is transparent there.
        let page = cx.new(|cx| PageView::in_plugin_popover(&host, &source, cx));
        page.read(cx).set_visible(true);
        Some(Section::Plugin(PluginSettings {
            plugin: plugin.to_owned(),
            title: title.into(),
            page,
        }))
    }

    /// Keeps what was typed in the font field, then gives the keys back to the terminal.
    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_family(cx);
        if let Some(mut settings) = self.settings.take() {
            settings.recording = None;
            self.leave("settings", settings, crate::ui::motion::POPUP_IN, cx);
        }
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

    /// Whether new terminals run in the terminal server: what this app does now, else what was saved, on unless turned
    /// off.
    pub(super) fn terminal_server_on(&self) -> bool {
        #[cfg(unix)]
        if let Some(servers) = &self.config.terminal_server {
            return servers.0.is_on();
        }
        self.prefs.terminal_server.unwrap_or(true)
    }

    /// Saves the choice and applies it to the terminals opened from now on. Turned on with no server running, one starts
    /// in the background, and new terminals run in the app until it answers.
    pub(super) fn set_terminal_server(&mut self, on: bool, cx: &mut Context<Self>) {
        #[cfg(unix)]
        if let Some(servers) = self.config.terminal_server.clone() {
            servers.0.set_on(on);
            if on {
                cx.background_executor()
                    .spawn(async move { servers.0.start_if_none() })
                    .detach();
            }
        }
        self.set_prefs(|prefs| prefs.terminal_server = Some(on), cx);
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
                    .children(ui::spot(|| format!("select-item:{id}-{index}")))
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
                    .children(ui::spot(|| format!("select:{id}")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(dialog) = &mut this.settings {
                            dialog.menu = (dialog.menu != Some(id)).then_some(id);
                        }
                        cx.notify();
                    })),
            )
            .children(self.popover_phase(id, open, cx).map(|phase| {
                motion::with_phase(phase, || {
                    ui::popover(ui::select_menu(items).on_mouse_down_out(cx.listener(
                        |this, _, _, cx| {
                            if let Some(dialog) = &mut this.settings {
                                dialog.menu = None;
                            }
                            cx.notify();
                        },
                    )))
                })
            }))
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

    /// "Bring Wings forward from any app": the shortcut, which a click lets you press anew, and its switch.
    fn global_hotkey_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let hotkey = self.prefs.global_hotkey.clone();
        let recording = self
            .settings
            .as_ref()
            .is_some_and(|d| d.recording.is_some());
        let text = if recording {
            "Press a shortcut…".to_owned()
        } else {
            crate::hotkey::label(&hotkey.accelerator, cfg!(target_os = "macos"))
        };
        let enabled = hotkey.enabled;
        let key = div()
            .id("global-hotkey-key")
            .role(Role::Button)
            .aria_label("Global shortcut. Press to change it")
            .h(px(32.0))
            .min_w(px(96.0))
            .flex()
            .items_center()
            .justify_center()
            .px(px(12.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(rgba(if recording {
                theme::HAIRLINE_STRONG
            } else {
                HAIRLINE
            }))
            .bg(rgba(0xffffff0a))
            .text_size(px(13.0))
            .when(!enabled, |key| key.opacity(0.5))
            .when(enabled, |key| {
                key.cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.record_global_hotkey(cx)))
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        if let Some(dialog) = &mut this.settings
                            && dialog.recording.take().is_some()
                        {
                            cx.notify();
                        }
                    }))
            })
            .child(text);
        ui::setting_row(
            "Bring Wings forward from any app",
            "Shows Wings and goes to the agent that needs you, or stays where you were if none does.",
            self.hotkey.error.clone(),
            div().flex().items_center().gap(px(12.0)).child(key).child(
                ui::switch("global-hotkey", enabled).on_click(cx.listener(
                    move |this, _, _, cx| {
                        let mut hotkey = this.prefs.global_hotkey.clone();
                        hotkey.enabled = !enabled;
                        this.set_global_hotkey(hotkey, cx);
                    },
                )),
            ),
        )
    }

    /// Waits for a key press to become the shortcut, as the desktop's `HotkeyRecorder`: Escape cancels, Backspace puts
    /// the default back, and a key without Ctrl, Alt or Cmd is ignored. It sees the press before the app's own key
    /// bindings, so a chord Wings uses itself can be recorded instead of running.
    pub(super) fn record_global_hotkey(&mut self, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        let listening = cx.intercept_keystrokes(move |event, _, cx| {
            let keystroke = event.keystroke.clone();
            if this
                .update(cx, |this, cx| this.recorded_key(&keystroke, cx))
                .unwrap_or(false)
            {
                cx.stop_propagation();
            }
        });
        if let Some(dialog) = &mut self.settings {
            dialog.recording = Some(listening);
        }
        cx.notify();
    }

    /// One key press while recording. Returns whether it was taken, which every key press is until recording ends.
    fn recorded_key(&mut self, keystroke: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        if !self
            .settings
            .as_ref()
            .is_some_and(|d| d.recording.is_some())
        {
            return false;
        }
        let plain = !keystroke.modifiers.modified();
        let accelerator = match keystroke.key.as_str() {
            "escape" => None,
            "backspace" if plain => Some(crate::saved::DEFAULT_HOTKEY.to_owned()),
            _ => match crate::hotkey::accelerator_from(keystroke) {
                Some(accelerator) => Some(accelerator),
                None => return true,
            },
        };
        if let Some(dialog) = &mut self.settings {
            dialog.recording = None;
        }
        if let Some(accelerator) = accelerator {
            let mut hotkey = self.prefs.global_hotkey.clone();
            hotkey.accelerator = accelerator;
            self.set_global_hotkey(hotkey, cx);
        }
        cx.notify();
        true
    }

    /// A setting whose control is a select, which is named after the setting.
    fn select_row(
        &self,
        id: &'static str,
        label: &'static str,
        hint: impl Into<SharedString>,
        choices: Vec<Choice>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        ui::setting_row(label, hint, None, self.select(id, label, choices, cx))
    }

    fn section_cards(&self, section: &Section, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        match section {
            Section::General => self.general_cards(cx),
            Section::Text => self.text_cards(cx),
            Section::Cursor => self.cursor_cards(cx),
            Section::History => self.history_cards(cx),
            Section::Keyboard => self.keyboard_cards(cx),
            Section::Editor => self.editor_cards(cx),
            Section::Agents => self.agents_cards(cx),
            Section::Notifications => self.notification_cards(cx),
            Section::Plugins | Section::Plugin(_) => Vec::new(),
        }
    }

    /// What the + button opens, a new project's first tab, and whether terminals outlive Wings.
    fn general_cards(&self, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
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
                let choose: Choose = Box::new(move |this, cx| {
                    let key = key.clone();
                    this.set_prefs(|prefs| prefs.new_tab = key, cx)
                });
                (label.into(), selected, choose)
            })
            .collect();
        let page = self.prefs.new_project_page;
        let opens = [(false, "Terminal"), (true, "Project page")]
            .map(|(value, label)| {
                let choose: Choose = Box::new(move |this, cx| {
                    this.set_prefs(|prefs| prefs.new_project_page = value, cx)
                });
                (label.into(), value == page, choose)
            })
            .into();
        let mut cards = vec![vec![
            self.select_row(
                "new-tab-page",
                "New tab button opens",
                format!(
                    "What the + button does: open a terminal or the Project page, choose each time from the \
                     menu, or open a plugin page. Plugin pages that declare \"newTab\": true in their view show \
                     here and in the menu next to the button. {} always opens a terminal.",
                    shortcut_label("T", false)
                ),
                new_tab,
                cx,
            ),
            self.select_row(
                "new-project-opens",
                "New projects open with",
                "The first tab of a project you add to Wings.",
                opens,
                cx,
            ),
        ]];
        if cfg!(unix) {
            let mut card = vec![self.switch_row(
                "terminal-server",
                "Keep terminals running when Wings quits",
                "New terminals run in a background process, so what runs in them keeps going when Wings \
                 quits or updates, and is there when Wings opens again. Changing this applies to new \
                 terminals. Turned off, new terminals end when Wings quits; ones already running in the \
                 background keep running until you close them.",
                self.terminal_server_on(),
                |this, on, cx| this.set_terminal_server(on, cx),
                cx,
            )];
            card.extend(self.disk_access_row(cx));
            cards.push(card);
        }
        cards
    }

    /// The terminals' font, line height and contrast.
    fn text_cards(&self, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        let current = look(cx);
        let family = self
            .settings
            .as_ref()
            .map(|d| (d.family.clone(), d.family_error.clone()));
        let (field, error) = family.expect("the dialog is open");
        vec![vec![
            ui::setting_row(
                "Font family",
                "Tried before the default monospace fonts. Leave empty for the default. Enter applies it.",
                error,
                ui::input(field),
            ),
            self.select_row(
                "line-height",
                "Line height",
                "Spacing between lines, as a multiple of the font size.",
                Self::look_choices(
                    LINE_HEIGHTS.map(|value| (value, format!("{value:.1}"))),
                    current.line_height,
                    |look, value| look.line_height = value,
                ),
                cx,
            ),
            self.select_row(
                "min-contrast",
                "Minimum contrast",
                "Lightens or darkens text that is hard to read on its background.",
                Self::look_choices(
                    CONTRASTS.map(|(value, label)| (value, label.to_owned())),
                    current.min_contrast,
                    |look, value| look.min_contrast = value,
                ),
                cx,
            ),
        ]]
    }

    /// The cursor's style, whether it blinks, and how it looks in the panes you aren't typing in.
    fn cursor_cards(&self, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        let current = look(cx);
        vec![vec![
            self.select_row(
                "cursor-style",
                "Cursor style",
                "The cursor programs get until they set their own.",
                Self::look_choices(
                    CURSOR_STYLES.map(|(shape, _, label)| (shape, label.to_owned())),
                    current.cursor,
                    |look, shape| look.cursor = shape,
                ),
                cx,
            ),
            self.switch_row(
                "cursor-blink",
                "Blink",
                "The cursor turns off and on in the pane you're typing in.",
                current.cursor_blink,
                |this, on, cx| this.set_look(|look| look.cursor_blink = on, cx),
                cx,
            ),
            self.select_row(
                "cursor-inactive",
                "When the pane isn't focused",
                "How the cursor looks in the panes you aren't typing in.",
                Self::look_choices(
                    INACTIVE_CURSORS.map(|(style, _, label)| (style, label.to_owned())),
                    current.inactive_cursor,
                    |look, style| look.inactive_cursor = style,
                ),
                cx,
            ),
        ]]
    }

    /// History and selection: the scrollback, copy on select, images, and programs writing the clipboard.
    fn history_cards(&self, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        let current = look(cx);
        vec![
            vec![
                self.select_row(
                    "scrollback",
                    "Scrollback",
                    "Lines kept per pane. A smaller number drops older history.",
                    Self::look_choices(
                        SCROLLBACKS.map(|lines| (lines, format!("{} lines", thousands(lines)))),
                        current.scrollback,
                        |look, lines| look.scrollback = lines,
                    ),
                    cx,
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
            vec![
                self.switch_row(
                    "terminal-images",
                    "Show images in the terminal",
                    "Draws images that programs print, such as SIXEL, iTerm2 and Kitty images. Each pane keeps \
                     up to 24 MB of them.",
                    current.images,
                    |this, on, cx| this.set_look(|look| look.images = on, cx),
                    cx,
                ),
                self.switch_row(
                    "clipboard-write",
                    "Let programs copy to the clipboard",
                    "Lets a program in a pane, such as tmux or a remote shell, put text on your clipboard (OSC \
                     52). Programs can never read it.",
                    current.clipboard_write,
                    |this, on, cx| this.set_look(|look| look.clipboard_write = on, cx),
                    cx,
                ),
            ],
        ]
    }

    /// Option as Meta, Shift-Return, and the global and quick terminal shortcuts.
    fn keyboard_cards(&self, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        let current = look(cx);
        vec![vec![
            self.select_row(
                "option-as-meta",
                "Use Option as Meta",
                "The other Option key still types characters like ~ and @.",
                Self::look_choices(
                    OPTION_AS_META.map(|(choice, _, label)| (choice, label.to_owned())),
                    current.option_as_meta,
                    |look, choice| look.option_as_meta = choice,
                ),
                cx,
            ),
            self.switch_row(
                "shift-return-meta",
                "Shift-Return sends Meta Return",
                "Adds a new line in Claude Code instead of sending the prompt.",
                current.shift_return_meta,
                |this, on, cx| this.set_shift_return_meta(on, cx),
                cx,
            ),
            self.global_hotkey_row(cx),
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
                    .child(
                        ui::switch("quick-terminal-key", self.prefs.quick_terminal).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.set_prefs(
                                    |prefs| prefs.quick_terminal = !prefs.quick_terminal,
                                    cx,
                                )
                            }),
                        ),
                    ),
            ),
        ]]
    }

    /// The editor a project's Open in menu starts with.
    fn editor_cards(&self, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        let current = self.default_editor().cloned();
        let control = current.as_ref().map(|current| {
            let choices = self
                .editors
                .iter()
                .map(|editor| {
                    let id = editor.id.clone();
                    let choose: Choose = Box::new(move |this, cx| {
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

    /// Claude Code's connection, as the desktop's Agents chip shows it: whether its sessions get Wings' tools, and its
    /// status line and hooks go through Wings, with Connect, Reconnect and Disconnect for every account at once, then
    /// each account's own state.
    fn agents_cards(&self, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        use super::claude::Change;
        let link = &self.claude;
        let default = link
            .accounts
            .as_ref()
            .and_then(|accounts| accounts.first())
            .map(|account| account.status.clone());
        let state = match &default {
            None if link.busy => ConnState::Checking,
            None => ConnState::NotChecked,
            Some(s) if !s.claude => ConnState::NotInstalled,
            Some(s) if s.connected && s.statusline => ConnState::Connected,
            Some(s) if s.connected || s.statusline => ConnState::Partly,
            Some(_) => ConnState::NotConnected,
        };
        let summary = match state {
            ConnState::NotInstalled => "Install Claude Code to give it plugin tools.",
            ConnState::Connected => {
                "Its sessions get Wings' and plugin tools, and plugins get its usage numbers. Your own status line \
                 still shows."
            }
            _ => {
                "Connect it so its sessions get Wings' and plugin tools and plugins get its usage numbers. Your own \
                 status line keeps showing, and Disconnect puts your settings back as they were."
            }
        };
        let busy = link.busy;
        let buttons = Self::connection_buttons(
            "claude",
            "Claude Code",
            state,
            busy,
            |this, connect, cx| {
                let change = if connect {
                    Change::Connect(None)
                } else {
                    Change::Disconnect
                };
                this.change_claude(change, cx)
            },
            cx,
        );
        // Its buttons' row stays, empty, while it isn't installed.
        let mut main = vec![Self::connection_card(
            "Claude Code",
            state,
            format!("{summary} Connect and Disconnect apply to every account a project uses."),
            link.error.clone(),
            Some(buttons),
        )];
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
            ui::setting_row(
                "Agent terminals open",
                "Where the first terminal an agent opens for a command goes: in a split below the agent, in a split \
                 to its right, or floating over it. Its next ones follow, and the buttons on the split switch that \
                 agent afterwards.",
                None,
                self.select(
                    "agent-terminals-open",
                    "Agent terminals open",
                    Opens::CHOICES
                        .into_iter()
                        .map(|(opens, label)| {
                            let choose: Choose = Box::new(move |this, cx| {
                                this.set_prefs(|prefs| prefs.agent_terminals_open = opens, cx)
                            });
                            (label.into(), opens == self.prefs.agent_terminals_open, choose)
                        })
                        .collect(),
                    cx,
                ),
            ),
        ];
        // The CLIs first, as the desktop's Agents section starts with them.
        let mut cards = vec![self.harnesses_card(cx), main];
        if !accounts.is_empty() {
            cards.push(accounts);
        }
        cards.push(vec![self.opencode_card(cx)]);
        cards.push(enter);
        cards
    }

    /// OpenCode's connection, as the desktop's OpenCode chip: whether its global config has Wings' MCP server.
    fn opencode_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let link = &self.opencode;
        let state = match &link.status {
            None if link.busy => ConnState::Checking,
            None => ConnState::NotChecked,
            Some(s) if !s.opencode => ConnState::NotInstalled,
            Some(s) if s.connected => ConnState::Connected,
            Some(_) => ConnState::NotConnected,
        };
        let summary = match state {
            ConnState::NotInstalled => "Install OpenCode to give it plugin tools.",
            ConnState::Connected => "Its sessions get Wings' and plugin tools.",
            _ => {
                "Connect it so its sessions get Wings' and plugin tools. Wings adds itself to OpenCode's global \
                 config and keeps the rest of it as it was."
            }
        };
        let buttons = state.installed().then(|| {
            Self::connection_buttons(
                "opencode",
                "OpenCode",
                state,
                link.busy,
                |this, connect, cx| this.change_opencode(connect, cx),
                cx,
            )
        });
        Self::connection_card("OpenCode", state, summary, link.error.clone(), buttons)
    }

    /// An agent's connection card: "<name> · <state>" and what it means, then its buttons. They go under the text, so a
    /// narrow window doesn't squeeze it into a column of single words.
    fn connection_card(
        name: &str,
        state: ConnState,
        summary: impl Into<SharedString>,
        error: Option<String>,
        buttons: Option<Div>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .child(ui::setting_row(
                format!("{name} · {}", state.label()),
                summary,
                error.map(SharedString::from),
                div(),
            ))
            .children(buttons.map(|buttons| {
                div()
                    .flex()
                    .justify_end()
                    .px(px(16.0))
                    .pb(px(12.0))
                    .child(buttons)
            }))
            .into_any_element()
    }

    /// An installed agent's Connect, or Reconnect once it is connected, after Disconnect while it is connected at all.
    /// `change` connects it on `true` and disconnects it on `false`.
    fn connection_buttons(
        id: &str,
        name: &str,
        state: ConnState,
        busy: bool,
        change: fn(&mut Workspace, bool, &mut Context<Workspace>),
        cx: &mut Context<Self>,
    ) -> Div {
        let mut buttons = div().flex().items_center().gap(px(8.0));
        if matches!(state, ConnState::Connected | ConnState::Partly) {
            buttons = buttons.child(
                ui::button(
                    format!("{id}-disconnect"),
                    format!("Disconnect {name}"),
                    ui::Variant::Ghost,
                    busy,
                    "Disconnect",
                )
                .when(!busy, |b| {
                    b.on_click(cx.listener(move |this, _, _, cx| change(this, false, cx)))
                }),
            );
        }
        if state.installed() {
            let (label, variant) = if state == ConnState::Connected {
                ("Reconnect", ui::Variant::Outline)
            } else {
                ("Connect", ui::Variant::Secondary)
            };
            buttons = buttons.child(
                ui::button(
                    format!("{id}-connect"),
                    format!("{label} {name}"),
                    variant,
                    busy,
                    label,
                )
                .when(!busy, |b| {
                    b.on_click(cx.listener(move |this, _, _, cx| change(this, true, cx)))
                }),
            );
        }
        buttons
    }

    pub(super) fn settings_dialog(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.settings.as_ref()?;
        let section = &dialog.section;
        let nav = Section::ALL.map(|item| {
            nav_item(
                SharedString::from(format!("settings-{}", item.label())),
                item.label(),
                item.icon(),
                *section == item,
            )
            .on_click(
                cx.listener(move |this, _, _, cx| this.set_settings_section(item.clone(), cx)),
            )
        });
        // Plugins that are on and have a settings page, by name. Each shows its page here, in the dialog.
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
                section.plugin() == Some(id.as_str()),
            )
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_plugin_settings(&id, window, cx)),
            )
        });
        let plugin_nav: Vec<_> = plugin_nav.collect();
        let content = div()
            .id("settings-content")
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .map(|content| match section {
                // The desktop's `TabsContent className="min-h-0 flex-1"`: the page fills the area and pads and
                // scrolls itself. Kept as last drawn until the page changes, as a plugin tab is.
                Section::Plugin(settings) => content
                    .min_h_0()
                    .role(Role::Group)
                    .aria_label(settings.title.clone())
                    .child(
                        settings
                            .page
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    ),
                _ => content
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
                        if *section == Section::Plugins {
                            content.child(self.plugins_section(cx))
                        } else {
                            content
                                .children(self.section_cards(section, cx).into_iter().map(ui::card))
                        }
                    }),
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
        let (width, height) = dialog.size(*section == Section::Plugins, window, cx);
        let (backdrop, panel) = ui::dialog("settings", "Settings", &dialog.focus, width, height);
        Some(
            backdrop
                // `calc(100%-4rem)`: 32px clear of each edge.
                .p(px(32.0))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                )
                .child(ui::dialog_panel(
                    panel
                        .max_w_full()
                        .max_h_full()
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                this.close_settings(window, cx);
                            }
                        }))
                        .children(ui::spot(|| "settings-dialog".into()))
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
                ))
                .into_any_element(),
        )
    }

    /// The desktop sidebar's last row: Settings, with its shortcut, as its `SidebarNavRow`, muted until pointed at.
    pub(super) fn settings_row(&self, cx: &mut Context<Self>) -> AnyElement {
        super::sidebar::nav_row(
            "open-settings",
            Icon::Settings,
            "Settings",
            super::sidebar::Look::Plain,
        )
        .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx)))
        .child(super::sidebar::row_hint(shortcut_label(",", false)))
        .into_any_element()
    }
}

impl SettingsDialog {
    /// Its width and height now: its section's size, at most the window's less 32px on each side, as the desktop's
    /// `min(…, calc(100%-4rem))`. For `RESIZE` after the section changes between the two sizes it is on its way, as
    /// the desktop's transition interpolates those `min`s.
    fn size(&self, large: bool, window: &Window, cx: &gpui::App) -> (f32, f32) {
        let now = cx.background_executor().now();
        let to = if large { 1.0 } else { 0.0 };
        let transition = match self.large.get() {
            Some(transition) if transition.target() == to => transition,
            Some(transition) => transition.retarget(to, RESIZE, EASE_OUT, now),
            None => motion::Transition::at(to, now),
        };
        self.large.set(Some(transition));
        let mut share = transition.value(now);
        if (share - to).abs() < 1e-4 {
            share = to;
        } else {
            window.request_animation_frame();
        }
        let room = window.viewport_size();
        let side = |small: f32, big: f32, room: Pixels| {
            let (small, big) = (
                small.min(room.as_f32() - 64.0),
                big.min(room.as_f32() - 64.0),
            );
            small + (big - small) * share
        };
        (
            side(SIZE.0, LARGE_SIZE.0, room.width),
            side(SIZE.1, LARGE_SIZE.1, room.height),
        )
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
        .children(ui::spot(|| format!("settings-nav:{label}")))
        .relative()
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
