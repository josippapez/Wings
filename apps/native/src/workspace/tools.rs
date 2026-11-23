//! Settings, Tools: the command-line programs Wings and its plugins run (`crate::tools`), each one's version and the
//! newest, with Update for the ones Homebrew or Claude's installer put here, and what's new in Claude Code since the
//! version you last marked read. A check runs off the window's thread at launch, every six hours while Wings is in
//! front and on Check now; Settings only draws what the last one found.

use std::{path::PathBuf, sync::Arc, time::Duration};

use gpui::{
    AnyElement, Context, FontWeight, Role, SharedString, Subscription, Task, Window, div,
    prelude::*, px, rgb,
};

use crate::{
    icons::{Icon, icon},
    markdown,
    page::plugin::OpenPane,
    terminal_view::TerminalEvent,
    theme,
    tools::{self, Entry, Lookup, Source, ToolStatus},
    ui::{self, Variant},
    workspace::NotifyWorkspace,
};

use super::Workspace;

/// The newest versions of the changelog Settings lists, and the notes under each.
const SHOWN_VERSIONS: usize = 8;
const SHOWN_NOTES: usize = 5;
/// The changelog as people read it.
const CHANGELOG_PAGE: &str = "https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// The first check waits, so it doesn't compete with the launch.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(8);

/// What the last check found, and what is running to find it again.
pub(super) struct Tools {
    /// `None` until the first check is back.
    list: Option<Vec<ToolStatus>>,
    /// Claude Code's newest changelog entries, newest first. Empty until read.
    changelog: Vec<Entry>,
    /// The Claude Code version whose notes you have read: What's new lists what came after it. Saved with the window.
    pub(super) read: Option<String>,
    /// A project that holds the Claude chat plugin's source, which "Check what Claude chat needs" opens a chat in.
    repo: Option<PathBuf>,
    checking: bool,
    error: Option<SharedString>,
    lookup: Arc<dyn Lookup + Send>,
    task: Option<Task<()>>,
    timer: Option<Task<()>>,
    /// The tabs an Update runs in, each rechecked when its shell ends.
    updating: Vec<Subscription>,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            list: None,
            changelog: Vec::new(),
            read: None,
            repo: None,
            checking: false,
            error: None,
            lookup: Arc::new(tools::System),
            task: None,
            timer: None,
            updating: Vec::new(),
        }
    }
}

impl Tools {
    /// How many programs have an update, for the count beside Tools in the list of sections.
    pub(super) fn is_unchecked(&self) -> bool {
        self.list.is_none() && !self.checking
    }

    pub(super) fn updates(&self) -> usize {
        self.list.as_deref().map_or(0, tools::updates)
    }
}

/// What a check reads: the tools, Claude Code's changelog when it is installed, and the project with Claude chat's source.
struct Found {
    list: Vec<ToolStatus>,
    changelog: Option<Vec<Entry>>,
    repo: Option<PathBuf>,
}

impl Workspace {
    /// Checks at launch, then every six hours while Wings is in front. Only an app that serves the socket runs your
    /// programs, as with Agents' checks, so tests and screenshots run none.
    pub(super) fn start_tool_checks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.config.socket.is_none() {
            return;
        }
        self.tools.timer = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(FIRST_CHECK_AFTER).await;
            if this.update(cx, |this, cx| this.check_tools(cx)).is_err() {
                return;
            }
            loop {
                cx.background_executor().timer(CHECK_EVERY).await;
                let active = this.update(cx, |this, cx| {
                    if this.window_active {
                        this.check_tools(cx);
                    }
                });
                if active.is_err() {
                    break;
                }
            }
        }));
    }

    /// The programs the turned-on plugins run, by plugin, for `tools::wants`.
    fn plugin_programs(&self) -> Vec<(String, Vec<String>)> {
        self.plugins
            .iter()
            .map(|installed| {
                let permissions = &installed.plugin.manifest.permissions;
                let entries = permissions
                    .exec
                    .iter()
                    .chain(&permissions.processes)
                    .chain(permissions.panes.iter().flatten())
                    .cloned()
                    .collect();
                (installed.name.clone(), entries)
            })
            .collect()
    }

    /// Looks at every tool again, off the window's thread.
    pub(super) fn check_tools(&mut self, cx: &mut Context<Self>) {
        if self.tools.checking {
            return;
        }
        let wants = tools::wants(&self.plugin_programs());
        let projects: Vec<PathBuf> = self.projects.iter().map(|p| p.path.clone()).collect();
        let lookup = self.tools.lookup.clone();
        self.tools.checking = true;
        self.tools.error = None;
        self.tools_changed(true, cx);
        let work = cx.background_executor().spawn(async move {
            let list = tools::check(&*lookup, &wants);
            let claude = list.iter().any(|tool| tool.program == "claude" && tool.found);
            let changelog = claude
                .then(|| lookup.get(tools::CHANGELOG, tools::MAX_CHANGELOG))
                .flatten()
                .map(|text| tools::parse_changelog(&text));
            let repo = tools::wings_repo(&projects);
            Found {
                list,
                changelog,
                repo,
            }
        });
        self.tools.task = Some(cx.spawn(async move |this, cx| {
            let found = work.await;
            let _ = this.update(cx, |this, cx| this.checked_tools(found, cx));
        }));
    }

    fn checked_tools(&mut self, found: Found, cx: &mut Context<Self>) {
        let tools = &mut self.tools;
        tools.checking = false;
        let mut changed = tools.list.as_ref() != Some(&found.list) || tools.repo != found.repo;
        tools.list = Some(found.list);
        tools.repo = found.repo;
        // A failed fetch of the changelog keeps the last one.
        if let Some(entries) = found.changelog.filter(|entries| *entries != tools.changelog) {
            tools.changelog = entries;
            changed = true;
        }
        // The first time, what's new starts from the version you have.
        let installed = tools
            .list
            .iter()
            .flatten()
            .find(|tool| tool.program == "claude")
            .and_then(|tool| tool.installed.clone());
        if tools.read.is_none()
            && let Some(version) = installed
        {
            tools.read = Some(version);
            changed = true;
            self.save(cx);
        }
        self.tools_changed(changed, cx);
    }

    /// Draws Settings again when it is open and something it shows changed: nobody sees a check while it is closed.
    fn tools_changed(&mut self, changed: bool, cx: &mut Context<Self>) {
        if changed && self.settings.is_some() {
            cx.notify_workspace();
        }
    }

    /// Shows these tools instead of looking, for screenshots.
    #[cfg(feature = "test-support")]
    pub fn show_tools(
        &mut self,
        list: Vec<ToolStatus>,
        changelog: Vec<Entry>,
        read: &str,
        repo: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.tools.list = Some(list);
        self.tools.changelog = changelog;
        self.tools.read = Some(read.into());
        self.tools.repo = repo;
        cx.notify_workspace();
    }

    /// Runs `command` in a new tab of the project on screen, so you see what it prints and can answer a password prompt,
    /// and looks at the tools again when its shell ends. Settings closes to show the tab. The shell ends by itself only
    /// when the command worked: a failure stays on screen to read.
    fn update_tool(&mut self, command: String, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.config.project.clone();
        let plan = OpenPane {
            cwd: project.clone(),
            project,
            input: Some(format!("{command} && exit\r")),
            placement: "tab".into(),
            claude_account: None,
            replace: None,
        };
        match self.place_plugin_pane(plan, window, cx) {
            Ok((pane, tab)) => {
                self.close_settings(window, cx);
                self.show(tab, window, cx);
                self.sync_plugins(cx);
                if let Some(view) = self.terminal(pane) {
                    self.tools.updating.push(cx.subscribe(
                        &view,
                        |this, _, _: &TerminalEvent, cx| this.check_tools(cx),
                    ));
                }
            }
            Err(error) => {
                self.tools.error = Some(format!("Couldn't open a tab: {error}").into());
                cx.notify_workspace();
            }
        }
    }

    /// What's been marked read moves to the newest version of the changelog.
    fn mark_changelog_read(&mut self, cx: &mut Context<Self>) {
        if let Some(newest) = self.tools.changelog.first() {
            self.tools.read = Some(newest.version.clone());
            self.save(cx);
            cx.notify_workspace();
        }
    }

    /// A Claude chat in the project that holds its source, asked to compare the changelog with what it supports.
    fn ask_claude_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(repo), Some(read)) = (self.tools.repo.clone(), self.tools.read.clone()) else {
            return;
        };
        let prompt = tools::compatibility_prompt(&tools::unread(&self.tools.changelog, &read));
        self.switch_project(repo.clone(), window, cx);
        let args = serde_json::json!({ "cwd": repo.to_string_lossy(), "prompt": prompt });
        if self.open_plugin_view("claude-chat", "chat", Some(args), window, cx) {
            self.close_settings(window, cx);
        } else {
            self.tools.error = Some("Claude chat isn't turned on, so it can't open a chat.".into());
            cx.notify_workspace();
        }
    }

    /// The cards of the section: the tools, then Claude Code's news.
    pub(super) fn tools_cards(&self, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        let tools = &self.tools;
        let check = ui::button(
            "tools-check",
            "Check the tools again",
            Variant::Ghost,
            tools.checking,
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(icon(Icon::RefreshCw, 14.0, theme::MUTED_FOREGROUND))
                .child(if tools.checking {
                    "Checking…"
                } else {
                    "Check now"
                }),
        )
        .px(px(8.0))
        .when(!tools.checking, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.check_tools(cx)))
        });
        let mut rows = vec![ui::setting_row(
            "Command-line tools",
            "What Wings and its plugins run. Checked when Wings starts and every six hours.",
            tools.error.clone(),
            check,
        )];
        match &tools.list {
            None => rows.push(div().px(px(16.0)).py(px(12.0)).text_size(px(12.0)).text_color(rgb(theme::MUTED_FOREGROUND)).child(if tools.checking { "Looking for your tools…" } else { "Not checked yet." }).into_any_element()),
            Some(list) => rows.extend(list.iter().map(|tool| tool_row(tool, cx))),
        }
        let mut cards = vec![rows];
        if let Some(news) = self.whats_new(cx) {
            cards.push(news);
        }
        cards
    }

    /// Claude Code's changelog since the version you last marked read, when Claude Code is installed and it was read.
    fn whats_new(&self, cx: &mut Context<Self>) -> Option<Vec<AnyElement>> {
        let tools = &self.tools;
        let read = tools.read.as_deref()?;
        let installed = tools
            .list
            .iter()
            .flatten()
            .any(|tool| tool.program == "claude" && tool.found);
        if !installed || tools.changelog.is_empty() {
            return None;
        }
        let unread = tools::unread(&tools.changelog, read);
        let muted = |text: SharedString| {
            div()
                .text_size(px(12.0))
                .line_height(px(16.5))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(text)
        };
        let full = ui::button(
            "tools-changelog",
            "Open Claude Code's changelog",
            Variant::Ghost,
            false,
            "Full changelog",
        )
        .on_click(|_, _, cx| cx.open_url(CHANGELOG_PAGE));
        if unread.is_empty() {
            return Some(vec![ui::setting_row(
                "What's new in Claude Code",
                format!("Nothing new since {read}."),
                None,
                full,
            )]);
        }
        let mut rows = vec![ui::setting_row(
            "What's new in Claude Code",
            format!(
                "{} since {read}.",
                match unread.len() {
                    1 => "1 new version".to_owned(),
                    n => format!("{n} new versions"),
                }
            ),
            None,
            div(),
        )];
        rows.extend(unread.iter().take(SHOWN_VERSIONS).map(|entry| {
            let more = entry.notes.len().saturating_sub(SHOWN_NOTES);
            div()
                .id(SharedString::from(format!("tools-news-{}", entry.version)))
                .role(Role::Group)
                .aria_label(format!("Claude Code {}", entry.version))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .px(px(16.0))
                .py(px(12.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child(entry.version.clone()),
                )
                .children(
                    entry
                        .notes
                        .iter()
                        .take(SHOWN_NOTES)
                        .enumerate()
                        // The notes are markdown: `/rewind` is code, not backticks around words.
                        .map(|(at, note)| {
                            div()
                                .text_size(px(12.0))
                                .line_height(px(16.5))
                                .text_color(rgb(theme::MUTED_FOREGROUND))
                                .child(markdown::inline_element(
                                    format!("tools-note-{}-{at}", entry.version),
                                    &markdown::inline(&format!("• {note}")),
                                ))
                        }),
                )
                .when(more > 0, |list| {
                    list.child(muted(format!("and {more} more in the full changelog").into()))
                })
                .into_any_element()
        }));
        if unread.len() > SHOWN_VERSIONS {
            rows.push(
                div()
                    .px(px(16.0))
                    .py(px(12.0))
                    .child(muted(
                        format!("{} older versions are in the full changelog.", unread.len() - SHOWN_VERSIONS).into(),
                    ))
                    .into_any_element(),
            );
        }
        let mark = ui::button(
            "tools-mark-read",
            "Mark Claude Code's changes as read",
            Variant::Secondary,
            false,
            "Mark as read",
        )
        .on_click(cx.listener(|this, _, _, cx| this.mark_changelog_read(cx)));
        let ask = tools.repo.is_some().then(|| {
            ui::button(
                "tools-ask-chat",
                "Open a Claude chat that checks what Claude chat doesn't support yet",
                Variant::Outline,
                false,
                "Check what Claude chat needs",
            )
            .on_click(cx.listener(|this, _, window, cx| this.ask_claude_chat(window, cx)))
        });
        rows.push(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.0))
                .px(px(16.0))
                .py(px(12.0))
                .child(mark)
                .children(ask)
                .child(full)
                .into_any_element(),
        );
        Some(rows)
    }
}

/// A tool's row: its name and whether it is current, what uses it, its versions and where it came from, and Update.
fn tool_row(tool: &ToolStatus, cx: &mut Context<Workspace>) -> AnyElement {
    let status = if !tool.found {
        Some((theme::IDLE, "Not installed"))
    } else if tool.has_update() {
        Some((theme::WORKING, "Update available"))
    } else if tool.latest.is_some() {
        Some((theme::DONE, "Up to date"))
    } else {
        None
    };
    let muted = |text: String| {
        div()
            .text_size(px(12.0))
            .line_height(px(16.5))
            .text_color(rgb(theme::MUTED_FOREGROUND))
            .child(text)
    };
    let details = if !tool.found {
        format!(
            "{}Not found on this Mac.",
            if tool.uses.is_empty() {
                String::new()
            } else {
                format!("Needed by {}. ", tool.uses)
            }
        )
    } else {
        let installed = tool
            .installed
            .as_deref()
            .map_or("version unknown".to_owned(), |v| format!("Installed {v}"));
        match (&tool.latest, tool.source == Source::Unknown) {
            (Some(latest), _) => format!("{installed} · Newest {latest} · {}", tool.source.label()),
            (None, true) => installed,
            (None, false) => format!("{installed} · {}: couldn't look up the newest", tool.source.label()),
        }
    };
    let command = tool.command();
    let name = tool.name.clone();
    let update = command.clone().map(|command| {
        ui::button(
            SharedString::from(format!("tools-update-{}", tool.program)),
            format!("Update {name}: runs {command} in a new tab"),
            Variant::Secondary,
            false,
            "Update",
        )
        .h(px(28.0))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.update_tool(command.clone(), window, cx)
        }))
    });
    div()
        .id(SharedString::from(format!("tools-{}", tool.program)))
        .role(Role::Group)
        .aria_label(format!("{}: {details}", tool.name))
        .flex()
        .items_center()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(12.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .child(tool.name.clone()),
                        )
                        .children(status.map(|(tone, label)| {
                            ui::badge(Variant::Secondary, Some(tone), true, label)
                        })),
                )
                .when(!tool.uses.is_empty() && tool.found, |col| {
                    col.child(muted(tool.uses.clone()))
                })
                .child(muted(details))
                .when(tool.found && tool.source == Source::Unknown, |col| {
                    col.child(muted(
                        "Update it the way you installed it.".to_owned(),
                    ))
                }),
        )
        .children(update)
        .into_any_element()
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
