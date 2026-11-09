//! Past sessions: the sheet that opens from the View menu or the palette (the desktop's `history-sheet.tsx`), and the
//! search it shares with the palette. It lists the Claude Code and OpenCode sessions in this project or in every
//! project, narrowed by words, last activity and branch. Resuming one opens it where it started, in a new tab, as the
//! desktop's `launchSession` does: in the project that holds its folder, or in a project made of that folder.

use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use gpui::{
    Anchor, AnyElement, BoxShadow, ClipboardItem, Context, Div, Entity, FocusHandle, FontWeight,
    HighlightStyle, KeyDownEvent, MouseButton, Role, SharedString, Stateful, StyledText,
    Subscription, Task, Toggled, Window, anchored, deferred, div, hsla, point, prelude::*, px,
    relative, rgb, rgba,
};
use wings_agents::history::{
    Agent, Filter, History, Hit, Results, Role as Speaker, Snippet, fold, terms,
};
use wings_spaces::claude_accounts;

use crate::{
    agents::AgentState,
    host::SessionSource,
    icons::{Icon, icon},
    projects::shell_quote,
    text_field::{FieldEvent, TextField},
    theme,
    ui::{self, ButtonSize, HAIRLINE, Variant, motion},
};

use super::Workspace;

/// Sessions listed at first, and added by each "Show more".
const PAGE: usize = 100;
/// The sheet's width: the desktop's `data-[side=right]:w-[480px]`.
const WIDTH: f32 = 480.0;
/// Typing waits this long before searching, so a word typed at speed searches once.
const TYPING: Duration = Duration::from_millis(150);
/// OpenCode's sessions load in the background, so the sheet asks again this often while they do.
const OPENCODE_POLL: Duration = Duration::from_secs(1);
/// How long "Copied" stays on the menu item.
const COPIED: Duration = Duration::from_millis(1500);
const DAY: u64 = 86_400_000;
/// Programs that sit at a prompt, which a typed command reaches (the desktop's `isShellCommand`).
const SHELLS: [&str; 11] = [
    "sh", "bash", "zsh", "fish", "dash", "ksh", "tcsh", "csh", "nu", "elvish", "xonsh",
];

/// A session to continue, and where it started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SessionRef {
    pub id: String,
    pub cwd: String,
    pub agent: Agent,
    /// A Claude session's transcript, in the folder of the account it ran under.
    pub transcript: Option<PathBuf>,
}

/// The history behind the sheet and the palette, kept between searches so each one reads only what was added.
#[derive(Clone)]
pub(super) struct Sessions {
    history: Arc<History>,
    source: Option<SessionSource>,
}

impl Sessions {
    pub fn new(source: Option<SessionSource>) -> Self {
        Self {
            history: Arc::default(),
            source,
        }
    }

    /// Reads transcripts, which takes a while the first time: call it off the main thread.
    pub fn search(&self, query: &str, filter: &Filter, limit: usize) -> Results {
        let folders = match &self.source {
            None => {
                return Results {
                    sessions: Vec::new(),
                    total: 0,
                    branches: Vec::new(),
                    opencode_loading: false,
                };
            }
            Some(SessionSource::Folders(folders)) => folders.clone(),
            Some(SessionSource::Accounts(default)) => {
                // OpenCode's sessions load in the background, so this search answers with what is there.
                if self.history.begin_opencode() {
                    let history = self.history.clone();
                    std::thread::spawn(move || history.run_opencode());
                }
                claude_accounts::dirs_with_default(default.clone())
            }
        };
        self.history.search(&folders, query, filter, limit)
    }

    /// Runs Claude Code's `search_history` or `read_session`, or `None` for another tool. OpenCode's sessions are loaded
    /// first, since the caller can't ask again later. Reads transcripts: call it off the main thread.
    pub fn call_tool(
        &self,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Option<Result<String, String>> {
        let folders = match &self.source {
            None => Vec::new(),
            Some(SessionSource::Folders(folders)) => folders.clone(),
            Some(SessionSource::Accounts(default)) => {
                if matches!(name, "search_history" | "read_session") {
                    self.history.sync_opencode();
                }
                claude_accounts::dirs_with_default(default.clone())
            }
        };
        wings_agents::history::call_tool(&self.history, &folders, name, arguments)
    }

    /// The default account's folder, where a terminal with no account of its own runs Claude Code.
    pub fn default_account(&self) -> Option<PathBuf> {
        match &self.source {
            Some(SessionSource::Accounts(default)) => Some(default.clone()),
            Some(SessionSource::Folders(folders)) => folders.first().cloned(),
            None => None,
        }
    }

    /// The account a hit ran under, by name, when it isn't the default one.
    fn account_label(&self, hit: &Hit) -> Option<SharedString> {
        let account = account_of(hit.transcript.as_deref()?)?;
        claude_accounts::label(account, &self.default_account()?).map(SharedString::new)
    }
}

/// The account folder a transcript is in: `<account>/projects/<folder>/<id>.jsonl`.
fn account_of(transcript: &Path) -> Option<&Path> {
    transcript.parent()?.parent()?.parent()
}

/// Makes a Claude session resumable under the account in folder `target`. Claude Code looks for a session only in its
/// own account's `projects/`, so a transcript from another account is copied into the same project folder there, with
/// the folder beside it that holds its subagents and tool results. A transcript already there is kept: it may hold the
/// session's continuation.
pub(super) fn bring_transcript(transcript: &Path, target: &Path) -> std::io::Result<()> {
    let (Some(account), Some(folder), Some(name), Some(id)) = (
        account_of(transcript),
        transcript.parent().and_then(Path::file_name),
        transcript.file_name(),
        transcript.file_stem(),
    ) else {
        return Ok(());
    };
    let into = target.join("projects").join(folder);
    if account == target || into.join(name).exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&into)?;
    let extras = transcript.with_file_name(id);
    if extras.is_dir() {
        copy_missing(&extras, &into.join(id))?;
    }
    // Written aside and renamed, so a copy cut short is never taken for the transcript, which is never overwritten.
    let partial = into.join(format!(".{}.partial", name.to_string_lossy()));
    std::fs::copy(transcript, &partial)?;
    std::fs::rename(&partial, into.join(name))
}

/// Copies the files of folder `from` that `to` doesn't have yet.
fn copy_missing(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let into = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_missing(&entry.path(), &into)?;
        } else if !into.exists() {
            std::fs::copy(entry.path(), &into)?;
        }
    }
    Ok(())
}

/// The line that continues a session, without its Enter. `None` when `id` isn't one of the agent's: it gets typed into a
/// shell, so nothing else goes through.
pub(super) fn resume_command(agent: Agent, id: &str) -> Option<String> {
    let argv = match agent {
        Agent::Claude if claude_id(id) => ["claude", "--resume", id],
        Agent::OpenCode if opencode_id(id) => ["opencode", "--session", id],
        _ => return None,
    };
    Some(argv.map(shell_quote).join(" "))
}

/// The line a restored pane types to continue its saved session, Enter included, as the desktop's `resumeLine`: Claude
/// Code's with the flags it was started with, unless one holds a control character, which would act as a key press.
pub(super) fn resume_line(session: &crate::saved::SavedSession) -> Option<String> {
    use crate::saved::SessionKind;
    let agent = match session.kind {
        SessionKind::Claude => Agent::Claude,
        SessionKind::Opencode => Agent::OpenCode,
    };
    let mut line = resume_command(agent, &session.id)?;
    if agent == Agent::Claude {
        push_flags(&mut line, &session.args);
    }
    line.push('\r');
    Some(line)
}

/// What a restored pane types to bring its agent back: `resume_line`, except for a Claude Code session with no
/// transcript in the account at `claude_dir`, which `--resume` can't find. Claude Code writes the transcript with the
/// first message, so that is an agent you started and never wrote to, and a new one starts with the same flags.
pub(super) fn restore_line(
    session: &crate::saved::SavedSession,
    claude_dir: Option<&Path>,
) -> Option<String> {
    let resume = resume_line(session)?;
    match claude_dir {
        Some(dir)
            if session.kind == crate::saved::SessionKind::Claude
                && !has_transcript(dir, &session.id) =>
        {
            let mut line = String::from("claude");
            push_flags(&mut line, &session.args);
            line.push('\r');
            Some(line)
        }
        _ => Some(resume),
    }
}

/// Whether the account at `claude_dir` has the Claude Code session `id`: `projects/<folder>/<id>.jsonl`.
fn has_transcript(claude_dir: &Path, id: &str) -> bool {
    let file = format!("{id}.jsonl");
    std::fs::read_dir(claude_dir.join("projects"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|folder| folder.path().join(&file).is_file())
}

/// The flags a Claude Code session started with, unless one holds a control character, which would act as a key press.
fn push_flags(line: &mut String, args: &[String]) {
    if args.iter().all(|arg| typeable(arg)) {
        for arg in args {
            line.push(' ');
            line.push_str(&shell_quote(arg));
        }
    }
}

/// The line that starts a copy of a session. Only Claude Code can fork.
pub(super) fn fork_command(agent: Agent, id: &str) -> Option<String> {
    (agent == Agent::Claude && claude_id(id)).then(|| {
        ["claude", "--resume", id, "--fork-session"]
            .map(shell_quote)
            .join(" ")
    })
}

/// A session id as Claude Code makes them: a UUID.
fn claude_id(id: &str) -> bool {
    id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// A session id as OpenCode makes them: `ses_` and letters and digits.
fn opencode_id(id: &str) -> bool {
    id.strip_prefix("ses_")
        .is_some_and(|rest| rest.len() >= 8 && rest.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// What is typed into a terminal reaches a shell, where a control character acts as a key press.
fn typeable(text: &str) -> bool {
    !text.chars().any(|c| c.is_control())
}

/// Text on one line, as a web page shows it: runs of white space are a single space.
pub(super) fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "5 minutes ago", "yesterday": how `Intl.RelativeTimeFormat` says it in English with `numeric: "auto"`.
fn ago(now: u64, then: u64) -> String {
    let round = |x: f64| (x + 0.5).floor() as i64;
    let minutes = round((then as f64 - now as f64) / 60_000.0);
    if minutes.abs() < 60 {
        return relative_time(minutes, "minute");
    }
    let hours = round(minutes as f64 / 60.0);
    if hours.abs() < 24 {
        return relative_time(hours, "hour");
    }
    relative_time(round(hours as f64 / 24.0), "day")
}

fn relative_time(count: i64, unit: &str) -> String {
    match (count, unit) {
        (0, "minute") => "this minute".into(),
        (0, "hour") => "this hour".into(),
        (0, _) => "today".into(),
        (-1, "day") => "yesterday".into(),
        (1, "day") => "tomorrow".into(),
        (count, _) => {
            let (n, s) = (count.abs(), if count.abs() == 1 { "" } else { "s" });
            if count < 0 {
                format!("{n} {unit}{s} ago")
            } else {
                format!("in {n} {unit}{s}")
            }
        }
    }
}

/// The line under a session's title: when it was last active, on which branch, in which folder unless it is the
/// project's own (by its path from the project, or by its last part over every project or outside this one), and how
/// many messages matched when more than one did. `project` is this project's folder.
fn session_meta(hit: &Hit, now: u64, project: &str, all: bool) -> String {
    let place = hit.cwd.as_deref().and_then(|cwd| {
        if cwd == project {
            return None;
        }
        match cwd.strip_prefix(&format!("{project}/")) {
            Some(inside) if !all => Some(inside.to_owned()),
            _ => cwd
                .rsplit('/')
                .find(|part| !part.is_empty())
                .map(str::to_owned),
        }
    });
    format!(
        "{}{}{}{}",
        ago(now, hit.last_active_ms),
        hit.git_branch
            .as_ref()
            .map_or(String::new(), |b| format!(" on {b}")),
        place.map_or(String::new(), |p| format!(" in {p}")),
        if hit.matches > 1 {
            format!(" · {} matching messages", hit.matches)
        } else {
            String::new()
        },
    )
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Where the matches of `terms` (as `terms` folds them) are in `text`, left to right, each word at its first place.
fn highlights(text: &str, terms: &[String]) -> Vec<Range<usize>> {
    let folded = fold(text);
    let mut found = Vec::new();
    let mut from = 0;
    while let Some((start, len)) = terms
        .iter()
        .filter(|term| !term.is_empty())
        .filter_map(|term| Some((from + folded[from..].find(term.as_str())?, term.len())))
        .min_by_key(|(start, _)| *start)
    {
        found.push(start..start + len);
        from = start + len;
    }
    found
}

/// Last activity, as the sheet's second row of buttons offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Since {
    Any,
    Today,
    Week,
    Month,
}

impl Since {
    const ALL: [Since; 4] = [Since::Any, Since::Today, Since::Week, Since::Month];

    fn label(self) -> &'static str {
        match self {
            Since::Any => "Any time",
            Since::Today => "Today",
            Since::Week => "7 days",
            Since::Month => "30 days",
        }
    }

    /// The earliest last activity that counts, in ms since 1970.
    fn ms(self) -> Option<u64> {
        match self {
            Since::Any => None,
            Since::Today => {
                let start = chrono::Local::now()
                    .date_naive()
                    .and_hms_opt(0, 0, 0)?
                    .and_local_timezone(chrono::Local)
                    .earliest()?;
                u64::try_from(start.timestamp_millis()).ok()
            }
            Since::Week => Some(now_ms().saturating_sub(7 * DAY)),
            Since::Month => Some(now_ms().saturating_sub(30 * DAY)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Menu {
    Branch,
    /// A session's "more" menu, by its id.
    Session(String),
}

pub(super) struct Sheet {
    field: Entity<TextField>,
    focus: FocusHandle,
    /// Every project's sessions, not just this one's.
    pub(super) all: bool,
    since: Since,
    pub(super) branch: Option<String>,
    limit: usize,
    pub(super) results: Option<Results>,
    /// The account each of `results.sessions` ran under, by name, when it isn't the default one.
    pub(super) accounts: Vec<Option<SharedString>>,
    searching: bool,
    menu: Option<Menu>,
    /// The session whose resume command was copied a moment ago.
    copied: Option<String>,
    /// The search under way; a newer one replaces it.
    load: Option<Task<()>>,
    reset_copied: Option<Task<()>>,
    _events: Subscription,
}

impl Workspace {
    /// Opens the sheet, or takes the keys back to its search field.
    pub(super) fn open_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(sheet) = &self.sheet {
            sheet
                .field
                .update(cx, |field, cx| field.select_all(window, cx));
            return;
        }
        // Settings is a dialog of its own, which the sheet would take the keys from.
        if self.settings.is_some() {
            return;
        }
        self.palette = None;
        let field = cx.new(|cx| {
            TextField::new(
                Role::SearchInput,
                "Search past sessions",
                "Search prompts, replies, titles, files",
                px(388.0),
                cx,
            )
        });
        let events = cx.subscribe_in(&field, window, |this, _, event, window, cx| match event {
            FieldEvent::Changed => {
                if let Some(sheet) = &mut this.sheet {
                    sheet.limit = PAGE;
                }
                this.search_history(cx);
            }
            FieldEvent::Submit { .. } => {}
            FieldEvent::Cancel => this.escape_history(window, cx),
        });
        field.update(cx, |field, cx| field.select_all(window, cx));
        self.sheet = Some(Sheet {
            field,
            focus: cx.focus_handle(),
            all: false,
            since: Since::Any,
            branch: None,
            limit: PAGE,
            results: None,
            accounts: Vec::new(),
            searching: false,
            menu: None,
            copied: None,
            load: None,
            reset_copied: None,
            _events: events,
        });
        self.search_history(cx);
        cx.notify();
    }

    /// Escape: closes an open menu first, then the sheet.
    fn escape_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.sheet.as_mut() {
            Some(Sheet {
                menu: menu @ Some(_),
                ..
            }) => {
                *menu = None;
                cx.notify();
            }
            _ => self.close_history(window, cx),
        }
    }

    /// Closes the sheet and gives the keys back to what had them: the pane you were in.
    pub(super) fn close_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(sheet) = self.sheet.take() {
            self.leave("sheet", sheet, motion::SHEET_IN, cx);
            self.show(self.active, window, cx);
        }
    }

    /// Searches again with what the sheet asks for, and shows the answer when it comes.
    fn search_history(&mut self, cx: &mut Context<Self>) {
        let Some(sheet) = &mut self.sheet else {
            return;
        };
        let query = sheet.field.read(cx).text.clone();
        let filter = Filter {
            project: (!sheet.all).then(|| self.config.project.to_string_lossy().into_owned()),
            branch: sheet.branch.clone(),
            since_ms: sheet.since.ms(),
        };
        let limit = sheet.limit;
        let sessions = self.sessions.clone();
        sheet.searching = true;
        sheet.load = Some(cx.spawn(async move |this, cx| {
            if !query.trim().is_empty() {
                cx.background_executor().timer(TYPING).await;
            }
            let (found, accounts) = cx
                .background_executor()
                .spawn(async move {
                    let found = sessions.search(&query, &filter, limit);
                    let accounts = found
                        .sessions
                        .iter()
                        .map(|hit| sessions.account_label(hit))
                        .collect();
                    (found, accounts)
                })
                .await;
            let loading = found.opencode_loading;
            this.update(cx, |this, cx| {
                if let Some(sheet) = &mut this.sheet {
                    sheet.results = Some(found);
                    sheet.accounts = accounts;
                    sheet.searching = false;
                    cx.notify();
                }
            })
            .ok();
            if loading {
                cx.background_executor().timer(OPENCODE_POLL).await;
                this.update(cx, |this, cx| this.search_history(cx)).ok();
            }
        }));
        cx.notify();
    }

    /// Changes what the sheet asks for, and searches again from the first page.
    pub(super) fn refine_history(
        &mut self,
        change: impl FnOnce(&mut Sheet),
        cx: &mut Context<Self>,
    ) {
        if let Some(sheet) = &mut self.sheet {
            change(sheet);
            sheet.limit = PAGE;
            sheet.menu = None;
        }
        self.search_history(cx);
    }

    /// The session behind a hit, if there's a folder to continue it in: its own, else this project's when only this
    /// project's sessions are listed.
    pub(super) fn session_of(&self, hit: &Hit) -> Option<SessionRef> {
        let all = self.sheet.as_ref().is_some_and(|sheet| sheet.all);
        let cwd = hit
            .cwd
            .clone()
            .or_else(|| (!all).then(|| self.config.project.to_string_lossy().into_owned()))?;
        Some(SessionRef {
            id: hit.id.clone(),
            cwd,
            agent: hit.agent,
            transcript: hit.transcript.clone(),
        })
    }

    /// The project a session opens in, and the line that continues it there: the project that holds the session's
    /// folder, else a new project made of the folder. A fork starts a copy of the session.
    pub(super) fn launch_plan(
        &mut self,
        session: &SessionRef,
        fork: bool,
    ) -> Result<(PathBuf, String), String> {
        let command = if fork {
            fork_command(session.agent, &session.id)
        } else {
            resume_command(session.agent, &session.id)
        }
        .ok_or("It has no id the agent can continue.")?;
        if !typeable(&session.cwd) {
            return Err("Its folder has a name that can't be typed.".into());
        }
        let cwd = Path::new(&session.cwd);
        let holder = self
            .projects
            .iter()
            .filter(|project| cwd.starts_with(&project.path))
            .max_by_key(|project| project.path.as_os_str().len())
            .map(|project| project.path.clone());
        let project = match holder {
            Some(path) => path,
            None if cwd.is_dir() => {
                let added = self.spaces.add(&session.cwd);
                self.projects = self.spaces.entries();
                added.path.into()
            }
            None => return Err(format!("{} isn't there any more.", session.cwd)),
        };
        let cd = if cwd == project {
            String::new()
        } else {
            format!("cd {} && ", shell_quote(&session.cwd))
        };
        Ok((project, format!("{cd}{command}\r")))
    }

    /// Opens a session in a new tab where it started (see `launch_plan`), under the account of the project there.
    pub(super) fn launch_session(
        &mut self,
        session: &SessionRef,
        fork: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (project, input) = self.launch_plan(session, fork)?;
        self.bring_over(session, self.spaces.claude_config_dir(&project))?;
        self.switch_project(project, window, cx);
        self.new_terminal(window, cx);
        if let Some(pane) = self.focused_pane() {
            self.type_into(pane, &input, window, cx);
        }
        Ok(())
    }

    /// Copies a session's transcript into the account in folder `account`, `None` being the default one, when it ran
    /// under another (see `bring_transcript`).
    fn bring_over(&self, session: &SessionRef, account: Option<PathBuf>) -> Result<(), String> {
        let (Some(transcript), Some(target)) = (
            &session.transcript,
            account.or_else(|| self.sessions.default_account()),
        ) else {
            return Ok(());
        };
        bring_transcript(transcript, &target).map_err(|error| {
            format!(
                "Its transcript couldn't be copied to the account in {}: {error}",
                target.display()
            )
        })
    }

    /// Types `text` into a pane's shell, as the keys would.
    pub(super) fn type_into(
        &mut self,
        pane: usize,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui::EntityInputHandler;
        if let Some(view) = self.terminal(pane) {
            view.update(cx, |view, cx| {
                view.replace_text_in_range(None, text, window, cx)
            });
        }
    }

    /// The focused pane, if it sits at a shell prompt, which a typed command reaches; else why not.
    fn prompt_pane(&self) -> Result<usize, &'static str> {
        let pane = self.focused_pane().ok_or("No tab is open")?;
        let idle = self
            .pane_info
            .get(&pane)
            .is_some_and(|info| SHELLS.contains(&info.command.as_str()));
        if idle {
            Ok(pane)
        } else {
            Err("The focused pane is running a program")
        }
    }

    /// Types the resume command into the focused pane, after a `cd` if it isn't in the session's folder.
    pub(super) fn resume_here(
        &mut self,
        session: &SessionRef,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Ok(pane), Some(command)) = (
            self.prompt_pane(),
            resume_command(session.agent, &session.id),
        ) else {
            return;
        };
        if !typeable(&session.cwd) {
            return;
        }
        let here = self
            .pane_info
            .get(&pane)
            .and_then(|info| info.cwd.as_deref());
        let cd = if here == Some(session.cwd.as_str()) {
            String::new()
        } else {
            format!("cd {} && ", shell_quote(&session.cwd))
        };
        let account = self
            .panes
            .get(&pane)
            .and_then(|p| p.claude_account.as_ref());
        let brought = self.bring_over(session, self.account_dir(account));
        self.close_history(window, cx);
        match brought {
            Ok(()) => self.type_into(pane, &format!("{cd}{command}\r"), window, cx),
            Err(error) => self.show_notice("Couldn't resume the session".into(), error, cx),
        }
    }

    pub(super) fn copy_resume_command(&mut self, session: &SessionRef, cx: &mut Context<Self>) {
        let Some(command) = resume_command(session.agent, &session.id) else {
            return;
        };
        let line = format!("cd {} && {command}", shell_quote(&session.cwd));
        cx.write_to_clipboard(ClipboardItem::new_string(line));
        let id = session.id.clone();
        let Some(sheet) = &mut self.sheet else {
            return;
        };
        sheet.copied = Some(id.clone());
        sheet.reset_copied = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED).await;
            this.update(cx, |this, cx| {
                if let Some(sheet) = &mut this.sheet
                    && sheet.copied.as_ref() == Some(&id)
                {
                    sheet.copied = None;
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    /// Resumes or forks a session in a new tab, and closes the sheet.
    pub(super) fn start_session(
        &mut self,
        session: &SessionRef,
        fork: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_history(window, cx);
        if let Err(error) = self.launch_session(session, fork, window, cx) {
            let title = if fork {
                "Couldn't fork the session"
            } else {
                "Couldn't resume the session"
            };
            self.show_notice(title.into(), error, cx);
        }
    }

    /// The sheet over the right side of the window, or nothing while it is closed.
    pub(super) fn sheet_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sheet = self.sheet.as_ref()?;
        let query = sheet.field.read(cx).text.clone();
        let filtered =
            !query.trim().is_empty() || sheet.branch.is_some() || sheet.since != Since::Any;
        let header = self.history_header(sheet, cx);
        let body: AnyElement = match &sheet.results {
            None => skeleton(),
            Some(results) if results.sessions.is_empty() => empty(
                if filtered { "No session matches" } else { "No past sessions yet" },
                if filtered {
                    "Every word has to appear in the session. Try fewer words, another branch or a longer time."
                } else if sheet.all {
                    "Sessions you run with Claude Code or OpenCode show up here."
                } else {
                    "Sessions you run in this project show up here."
                }
                .to_owned(),
            ),
            Some(results) => self.session_list(sheet, results, &terms(&query), cx),
        };
        let (backdrop, panel) = ui::dialog("history", "Past sessions", &sheet.focus, WIDTH, 0.0);
        // The backdrop and the sheet each fade on their own, as the desktop's: the backdrop over 150ms, the sheet over
        // 200ms as it slides.
        let phase = motion::phase();
        let sheet_panel = |panel| motion::sheet_panel(panel, phase);
        Some(motion::sheet_backdrop(
            backdrop
                .items_stretch()
                .justify_end()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_history(window, cx)),
                )
                .child(sheet_panel(
                    panel
                        .h_full()
                        .max_h(relative(1.0))
                        .max_w(relative(0.92))
                        .rounded(px(0.0))
                        .border_0()
                        .border_l_1()
                        .border_color(rgba(theme::HAIRLINE_STRONG))
                        .shadow(vec![BoxShadow {
                            color: hsla(0.0, 0.0, 0.0, 0.5),
                            offset: point(px(0.0), px(24.0)),
                            blur_radius: px(64.0),
                            spread_radius: px(0.0),
                            inset: false,
                        }])
                        .flex_col()
                        .bg(rgba(theme::POPOVER))
                        // Escape from a button the sheet's field doesn't have the keys for.
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                this.escape_history(window, cx);
                            }
                        }))
                        .child(header)
                        .child(
                            div()
                                .id("history-list")
                                .min_h_0()
                                .flex_1()
                                .overflow_y_scroll()
                                .child(body),
                        )
                        .child(
                            div().absolute().top(px(12.0)).right(px(12.0)).child(
                                ui::icon_button("close-history", Icon::X, "Close", ButtonSize::Sm)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_history(window, cx)
                                    })),
                            ),
                        ),
                )),
            ui::backdrop_color(),
            phase,
        ))
    }

    /// The sheet's top: its heading, this project or every one, the search field, and the time and branch filters.
    fn history_header(&self, sheet: &Sheet, cx: &mut Context<Self>) -> Div {
        let project = self
            .projects
            .iter()
            .find(|project| project.path == self.config.project);
        let project_name = project.map_or("this project", |project| project.name.as_str());
        let loading = sheet.results.as_ref().is_some_and(|r| r.opencode_loading);
        let heading = div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                div()
                    .text_size(px(16.0))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Past sessions"),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(if sheet.all {
                        "Claude Code and OpenCode sessions in every project.".to_owned()
                    } else {
                        format!("Claude Code and OpenCode sessions in {project_name}.")
                    }),
            );
        let scope = segmented([
            segment("history-project", "This project", !sheet.all).on_click(cx.listener(
                |this, _, _, cx| {
                    this.refine_history(|sheet| (sheet.all, sheet.branch) = (false, None), cx)
                },
            )),
            segment("history-all", "All projects", sheet.all).on_click(cx.listener(
                |this, _, _, cx| {
                    this.refine_history(|sheet| (sheet.all, sheet.branch) = (true, None), cx)
                },
            )),
        ]);
        let search = div()
            .h(px(32.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(10.0))
            .pr(px(8.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(rgba(0xffffff1a))
            .bg(rgba(theme::HOVER))
            .child(icon(Icon::Search, 16.0, theme::MUTED_FOREGROUND))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .child(sheet.field.clone()),
            )
            .child(
                div()
                    .size(px(16.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(
                        (sheet.searching || loading) && sheet.results.is_some(),
                        |slot| slot.child(ui::status_dot(AgentState::Working)),
                    ),
            );
        let since = segmented(Since::ALL.map(|since| {
            segment(
                SharedString::from(format!("history-since-{since:?}")),
                since.label(),
                sheet.since == since,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.refine_history(|sheet| sheet.since = since, cx)
            }))
        }));
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(16.0))
            .border_b_1()
            .border_color(rgba(HAIRLINE))
            .child(heading)
            .child(scope)
            .child(search)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .child(since)
                    .child(self.branch_picker(sheet, cx)),
            )
    }

    /// The branch filter: its button, and the branches of the listed sessions while its menu is open.
    fn branch_picker(&self, sheet: &Sheet, cx: &mut Context<Self>) -> Div {
        let branches = sheet
            .results
            .as_ref()
            .map(|r| r.branches.as_slice())
            .unwrap_or(&[]);
        div()
            .relative()
            .min_w_0()
            .child(
                div()
                    .id("history-branch")
                    .role(Role::ComboBox)
                    .aria_label("Branch")
                    .h(px(28.0))
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .rounded(px(8.0))
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .hover(|style| {
                        style
                            .bg(rgba(theme::HOVER))
                            .text_color(rgb(theme::FOREGROUND))
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(sheet) = &mut this.sheet {
                            sheet.menu = (sheet.menu != Some(Menu::Branch)).then_some(Menu::Branch);
                        }
                        cx.notify();
                    }))
                    .child(icon(Icon::GitBranch, 14.0, theme::MUTED_FOREGROUND))
                    .child(
                        div().min_w_0().max_w(px(200.0)).truncate().child(
                            sheet
                                .branch
                                .clone()
                                .unwrap_or_else(|| "All branches".into()),
                        ),
                    )
                    .child(icon(Icon::ChevronDown, 14.0, theme::MUTED_FOREGROUND)),
            )
            .when(sheet.menu == Some(Menu::Branch), |slot| {
                let choose =
                    |id: SharedString, label: String, chosen: Option<String>, selected: bool| {
                        ui::select_item(id, label, selected).on_click(cx.listener(
                            move |this, _, _, cx| {
                                let chosen = chosen.clone();
                                this.refine_history(move |sheet| sheet.branch = chosen, cx)
                            },
                        ))
                    };
                let mut items = vec![
                    choose(
                        "history-branch-all".into(),
                        "All branches".into(),
                        None,
                        sheet.branch.is_none(),
                    )
                    .into_any_element(),
                ];
                items.extend(branches.iter().enumerate().map(|(index, name)| {
                    choose(
                        SharedString::from(format!("history-branch-{index}")),
                        name.clone(),
                        Some(name.clone()),
                        sheet.branch.as_ref() == Some(name),
                    )
                    .into_any_element()
                }));
                slot.child(menu_below(
                    32.0,
                    ui::select_menu(items)
                        .mt(px(0.0))
                        .max_h(px(320.0))
                        .overflow_y_hidden()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            if let Some(sheet) = &mut this.sheet {
                                sheet.menu = None;
                            }
                            cx.notify();
                        })),
                ))
            })
    }

    /// The sessions found, then how many of how many are shown and "Show more" when there are more.
    fn session_list(
        &self,
        sheet: &Sheet,
        results: &Results,
        words: &[String],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = now_ms();
        let live: Vec<&str> = self
            .agents
            .values()
            .filter_map(|agent| agent.session_id.as_deref())
            .collect();
        let mut rows: Vec<AnyElement> = results
            .sessions
            .iter()
            .enumerate()
            .map(|(at, hit)| {
                let running = live.contains(&hit.id.as_str());
                let account = sheet.accounts.get(at).cloned().flatten();
                self.session_row(sheet, at, hit, account, now, words, running, cx)
            })
            .collect();
        if results.total > results.sessions.len() {
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .px(px(10.0))
                    .py(px(8.0))
                    .text_size(px(12.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(format!(
                        "Showing {} of {}",
                        results.sessions.len(),
                        results.total
                    ))
                    .child(
                        ui::button(
                            "history-more-sessions",
                            "Show more",
                            Variant::Ghost,
                            false,
                            "Show more",
                        )
                        .h(px(28.0))
                        .text_size(px(12.0))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(sheet) = &mut this.sheet {
                                sheet.limit += PAGE;
                            }
                            this.search_history(cx);
                        })),
                    )
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .p(px(8.0))
            .children(rows)
            .into_any_element()
    }

    /// One session: its agent and title, the account it ran under unless that's the default one, what matched, when and
    /// where it ran, and Resume and its menu, which show while the pointer is over it or the menu is open.
    #[allow(clippy::too_many_arguments)]
    fn session_row(
        &self,
        sheet: &Sheet,
        at: usize,
        hit: &Hit,
        account: Option<SharedString>,
        now: u64,
        words: &[String],
        running: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let session = self.session_of(hit);
        let launchable = session.is_some();
        let title = one_line(
            hit.title
                .as_deref()
                .or(hit.first_prompt.as_deref())
                .unwrap_or("Untitled session"),
        );
        let detail: Option<AnyElement> = match hit.snippets.first() {
            Some(snippet) => Some(snippet_text(snippet, hit.agent, words)),
            None => hit
                .title
                .as_ref()
                .zip(hit.first_prompt.as_ref())
                .map(|(_, prompt)| muted_lines(one_line(prompt)).into_any_element()),
        };
        let meta = session_meta(hit, now, &self.config.project.to_string_lossy(), sheet.all);
        let menu_open = sheet.menu == Some(Menu::Session(hit.id.clone()));
        let copied = sheet.copied.as_ref() == Some(&hit.id);
        let group = SharedString::from(format!("session-{at}"));
        div()
            .id(("history-session", at))
            .group(group.clone())
            .flex()
            .items_start()
            .gap(px(8.0))
            .p(px(10.0))
            .rounded(px(10.0))
            .hover(|style| style.bg(rgba(theme::HOVER)))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child(agent_mark(hit.agent))
                            .child(div().min_w_0().truncate().child(title))
                            .children(account.map(super::agent_list::account_badge)),
                    )
                    .children(detail)
                    .child(
                        div()
                            .mt(px(2.0))
                            .truncate()
                            .text_size(px(12.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(meta),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .opacity(if menu_open { 1.0 } else { 0.0 })
                    .group_hover(group, |style| style.opacity(1.0))
                    .child({
                        let session = session.clone();
                        let disabled = running || !launchable;
                        let button = ui::button(
                            ("history-resume", at),
                            if running { "Running" } else { "Resume" },
                            if running {
                                Variant::Ghost
                            } else {
                                Variant::Secondary
                            },
                            disabled,
                            if running { "Running" } else { "Resume" },
                        )
                        .h(px(28.0))
                        .px(px(12.0))
                        .rounded_full()
                        .text_size(px(12.0));
                        if let (false, Some(session)) = (disabled, session) {
                            button.on_click(cx.listener(move |this, _, window, cx| {
                                this.start_session(&session, false, window, cx)
                            }))
                        } else {
                            button
                        }
                    })
                    .child(
                        ui::icon_button(
                            ("history-more", at),
                            Icon::Ellipsis,
                            "More for this session",
                            ButtonSize::Sm,
                        )
                        .on_click({
                            let id = hit.id.clone();
                            cx.listener(move |this, _, _, cx| {
                                if let Some(sheet) = &mut this.sheet {
                                    let menu = Menu::Session(id.clone());
                                    sheet.menu =
                                        (sheet.menu.as_ref() != Some(&menu)).then_some(menu);
                                }
                                cx.notify();
                            })
                        }),
                    )
                    .when(menu_open, |slot| {
                        slot.child(self.session_menu(at, session, copied, self.prompt_pane(), cx))
                    }),
            )
            .into_any_element()
    }

    /// A session's "more" menu: resume in the focused pane, fork, copy the command.
    fn session_menu(
        &self,
        at: usize,
        session: Option<SessionRef>,
        copied: bool,
        prompt: Result<usize, &'static str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let can_fork = session
            .as_ref()
            .is_some_and(|s| fork_command(s.agent, &s.id).is_some());
        let here = session.clone().filter(|_| prompt.is_ok());
        let mut items = vec![
            menu_item(
                ("history-here", at),
                Icon::SquareTerminal,
                "Resume in this tab",
                prompt.err().filter(|_| session.is_some()),
                here.is_none(),
            )
            .when_some(here, |item, session| {
                item.on_click(
                    cx.listener(move |this, _, window, cx| this.resume_here(&session, window, cx)),
                )
            })
            .into_any_element(),
        ];
        if can_fork {
            items.push(
                menu_item(
                    ("history-fork", at),
                    Icon::GitFork,
                    "Fork into a new session",
                    None,
                    false,
                )
                .when_some(session.clone(), |item, session| {
                    item.on_click(cx.listener(move |this, _, window, cx| {
                        this.start_session(&session, true, window, cx)
                    }))
                })
                .into_any_element(),
            );
        }
        items.push(
            menu_item(
                ("history-copy", at),
                if copied { Icon::Check } else { Icon::Copy },
                if copied {
                    "Copied"
                } else {
                    "Copy resume command"
                },
                None,
                session.is_none(),
            )
            .when_some(session, |item, session| {
                item.on_click(
                    cx.listener(move |this, _, _, cx| this.copy_resume_command(&session, cx)),
                )
            })
            .into_any_element(),
        );
        menu_below(
            32.0,
            ui::select_menu(items)
                .mt(px(0.0))
                .w(px(220.0))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    if let Some(sheet) = &mut this.sheet {
                        sheet.menu = None;
                    }
                    cx.notify();
                })),
        )
    }
}

/// A menu drawn above everything, under `top` points down the box it sits in and with its right edge on that box's.
fn menu_below(top: f32, menu: impl IntoElement) -> AnyElement {
    div()
        .absolute()
        .top(px(top))
        .right_0()
        .w_0()
        .h_0()
        .child(deferred(anchored().anchor(Anchor::TopRight).child(menu)))
        .into_any_element()
}

/// One line of a session's menu. `note` says why it is off.
fn menu_item(
    id: impl Into<gpui::ElementId>,
    glyph: Icon,
    label: &'static str,
    note: Option<&'static str>,
    disabled: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::MenuItem)
        .aria_label(label)
        .min_h(px(32.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .text_size(px(13.0))
        .when(disabled, |item| item.opacity(0.5))
        .when(!disabled, |item| {
            item.cursor_pointer()
                .hover(|style| style.bg(rgba(theme::HOVER_STRONG)))
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(icon(glyph, 16.0, theme::MUTED_FOREGROUND))
        .child(div().flex_none().child(label))
        .children(note.map(|note| {
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_size(px(11.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(note)
        }))
}

/// The desktop's `ToggleGroup`: buttons side by side on a faint track.
fn segmented(items: impl IntoIterator<Item = Stateful<Div>>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(2.0))
        .p(px(2.0))
        .rounded(px(8.0))
        .bg(rgba(0xffffff0d))
        .children(items)
}

fn segment(id: impl Into<gpui::ElementId>, label: &'static str, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .h(px(24.0))
        .flex()
        .items_center()
        .px(px(10.0))
        .rounded(px(6.0))
        .cursor_pointer()
        .text_size(px(12.0))
        .text_color(rgb(if on {
            theme::FOREGROUND
        } else {
            theme::MUTED_FOREGROUND
        }))
        .when(on, |item| item.bg(rgba(0xffffff17)))
        .when(!on, |item| {
            item.hover(|style| style.text_color(rgb(theme::FOREGROUND)))
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(label)
}

/// The agent's mark beside a session's name: Claude's spark in its orange, OpenCode's frame.
fn agent_mark(agent: Agent) -> impl IntoElement {
    match agent {
        Agent::Claude => icon(Icon::Claude, 12.0, 0xd97757),
        Agent::OpenCode => icon(Icon::Opencode, 12.0, theme::FOREGROUND),
    }
}

fn muted_lines(content: impl IntoElement) -> Div {
    div()
        .text_size(px(12.0))
        .text_color(rgb(theme::MUTED_FOREGROUND))
        .line_clamp(2)
        .child(content)
}

/// What matched in a session: who said it, then the words, with the searched ones marked.
fn snippet_text(snippet: &Snippet, agent: Agent, words: &[String]) -> AnyElement {
    let speaker = match (snippet.role, agent) {
        (Speaker::User, _) => "You",
        (Speaker::Assistant, Agent::Claude) => "Claude",
        (Speaker::Assistant, Agent::OpenCode) => "OpenCode",
        (Speaker::File, _) => "Edited",
    };
    let prefix = format!("{speaker}: ");
    let body = one_line(&snippet.text);
    let text = format!("{prefix}{body}");
    let fade = HighlightStyle {
        color: Some(rgba(theme::FOREGROUND << 8 | 0xb3).into()),
        ..Default::default()
    };
    let mark = HighlightStyle {
        color: Some(rgb(theme::FOREGROUND).into()),
        background_color: Some(rgba(theme::WORKING << 8 | 0x40).into()),
        ..Default::default()
    };
    let marks = highlights(&body, words)
        .into_iter()
        .map(|range| (range.start + prefix.len()..range.end + prefix.len(), mark));
    let runs = std::iter::once((0..prefix.len(), fade)).chain(marks);
    muted_lines(StyledText::new(text).with_highlights(runs)).into_any_element()
}

/// The sheet's empty and error states: an icon, what happened, and what to do about it.
fn empty(title: &'static str, description: String) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.0))
        .px(px(32.0))
        .py(px(64.0))
        .text_center()
        .child(
            div()
                .size(px(36.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(10.0))
                .bg(rgba(0xffffff0d))
                .child(icon(Icon::SearchX, 18.0, theme::FOREGROUND)),
        )
        .child(
            div()
                .mt(px(6.0))
                .text_size(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(description),
        )
        .into_any_element()
}

/// Rows of grey bars while the first search reads the transcripts.
fn skeleton() -> AnyElement {
    div()
        .id("history-reading")
        .role(Role::Group)
        .aria_label("Reading transcripts")
        .flex()
        .flex_col()
        .gap(px(4.0))
        .p(px(8.0))
        .children((0..6).map(|row| {
            let bar = |height: f32, width: f32, alpha: u32| {
                div()
                    .h(px(height))
                    .w(relative(width))
                    .rounded(px(4.0))
                    .bg(rgba(0xffffff00 | alpha))
            };
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .p(px(10.0))
                .child(bar(16.0, 0.5 + ((row * 29) % 40) as f32 / 100.0, 0x0f))
                .child(bar(12.0, 0.7 + ((row * 17) % 25) as f32 / 100.0, 0x0a))
                .child(bar(12.0, 0.25, 0x0a))
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_continued_with_its_agents_own_command() {
        let claude = "0b8e7c1a-3d2f-4a5b-9c6d-7e8f9a0b1c2d";
        assert_eq!(
            resume_command(Agent::Claude, claude).as_deref(),
            Some("claude --resume 0b8e7c1a-3d2f-4a5b-9c6d-7e8f9a0b1c2d")
        );
        assert_eq!(
            fork_command(Agent::Claude, claude).as_deref(),
            Some("claude --resume 0b8e7c1a-3d2f-4a5b-9c6d-7e8f9a0b1c2d --fork-session")
        );
        assert_eq!(
            resume_command(Agent::OpenCode, "ses_01HZabcdefgh").as_deref(),
            Some("opencode --session ses_01HZabcdefgh")
        );
        assert_eq!(fork_command(Agent::OpenCode, "ses_01HZabcdefgh"), None);
    }

    /// The desktop's `resumeLine` cases in `resume.test.ts`.
    #[test]
    fn a_restored_pane_resumes_with_the_flags_its_session_started_with() {
        use crate::saved::{SavedSession, SessionKind};
        let session = |kind, id: &str, args: &[&str]| SavedSession {
            kind,
            id: id.into(),
            args: args.iter().map(ToString::to_string).collect(),
            name: None,
        };
        let claude = "0b8e7c1a-3d2f-4a5b-9c6d-7e8f9a0b1c2d";
        assert_eq!(
            resume_line(&session(
                SessionKind::Claude,
                claude,
                &[
                    "--model",
                    "opus",
                    "--append-system-prompt",
                    "say \"hi\", don't ask"
                ]
            ))
            .as_deref(),
            Some(
                "claude --resume 0b8e7c1a-3d2f-4a5b-9c6d-7e8f9a0b1c2d --model opus --append-system-prompt \
                 'say \"hi\", don'\\''t ask'\r"
            )
        );
        // A flag that would press a key keeps every flag out.
        assert_eq!(
            resume_line(&session(
                SessionKind::Claude,
                claude,
                &["--model", "opus\r"]
            ))
            .as_deref(),
            Some("claude --resume 0b8e7c1a-3d2f-4a5b-9c6d-7e8f9a0b1c2d\r")
        );
        // OpenCode takes no start flags.
        assert_eq!(
            resume_line(&session(
                SessionKind::Opencode,
                "ses_01HZabcdefgh",
                &["--x"]
            ))
            .as_deref(),
            Some("opencode --session ses_01HZabcdefgh\r")
        );
        assert_eq!(
            resume_line(&session(SessionKind::Claude, "$(id)", &[])),
            None
        );
    }

    /// A Claude Code session nobody wrote to has no transcript, and `claude --resume` says it can't find it.
    #[test]
    fn a_restored_claude_session_with_no_transcript_starts_a_new_one_with_its_flags() {
        use crate::saved::{SavedSession, SessionKind};
        let id = "0b8e7c1a-3d2f-4a5b-9c6d-7e8f9a0b1c2d";
        let session = |kind, id: &str| SavedSession {
            kind,
            id: id.into(),
            args: vec!["--model".into(), "opus".into()],
            name: None,
        };
        let account =
            std::env::temp_dir().join(format!("wings-restore-line-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&account);
        std::fs::create_dir_all(account.join("projects/-tmp-acme")).unwrap();
        let claude = session(SessionKind::Claude, id);
        assert_eq!(
            restore_line(&claude, Some(&account)).as_deref(),
            Some("claude --model opus\r")
        );
        std::fs::write(
            account.join(format!("projects/-tmp-acme/{id}.jsonl")),
            "{}\n",
        )
        .unwrap();
        assert_eq!(
            restore_line(&claude, Some(&account)),
            resume_line(&claude),
            "a session with a transcript resumes"
        );
        // Without an account to look in, and for OpenCode, which keeps no such file, it resumes as before.
        let _ = std::fs::remove_dir_all(&account);
        assert_eq!(restore_line(&claude, None), resume_line(&claude));
        let opencode = session(SessionKind::Opencode, "ses_01HZabcdefgh");
        assert_eq!(
            restore_line(&opencode, Some(&account)),
            resume_line(&opencode)
        );
        // An id a shell could run something from still types nothing.
        assert_eq!(
            restore_line(&session(SessionKind::Claude, "$(id)"), Some(&account)),
            None
        );
    }

    #[test]
    fn an_id_that_a_shell_could_run_something_from_is_refused() {
        for id in [
            "",
            "x; rm -rf ~",
            "0b8e7c1a-3d2f-4a5b-9c6d-7e8f9a0b1c2d\r",
            "$(id)",
        ] {
            assert_eq!(resume_command(Agent::Claude, id), None, "{id:?}");
            assert_eq!(resume_command(Agent::OpenCode, id), None, "{id:?}");
        }
        assert_eq!(resume_command(Agent::OpenCode, "ses_short"), None);
        assert!(!typeable("/tmp/a\nrm -rf ~"));
        assert!(typeable("/tmp/acme web"));
    }

    #[test]
    fn times_read_like_the_desktops() {
        let now = 1_000_000_000_000;
        let minutes = |n: u64| now - n * 60_000;
        assert_eq!(ago(now, now), "this minute");
        assert_eq!(ago(now, minutes(1)), "1 minute ago");
        assert_eq!(ago(now, minutes(5)), "5 minutes ago");
        assert_eq!(ago(now, minutes(60)), "1 hour ago");
        assert_eq!(ago(now, minutes(3 * 60)), "3 hours ago");
        assert_eq!(ago(now, minutes(24 * 60)), "yesterday");
        assert_eq!(ago(now, minutes(5 * 24 * 60)), "5 days ago");
    }

    #[test]
    fn a_sessions_line_says_when_where_and_how_many_messages_matched() {
        let now = 1_000_000_000_000;
        let hit = |cwd: Option<&str>, branch: Option<&str>, matches| Hit {
            id: "0b8e7c1a-3d2f-4a5b-9c6d-7e8f9a0b1c2d".into(),
            agent: Agent::Claude,
            cwd: cwd.map(Into::into),
            title: None,
            first_prompt: None,
            git_branch: branch.map(Into::into),
            last_active_ms: now - 5 * 60_000,
            last_active: None,
            messages: 4,
            matches,
            snippets: Vec::new(),
            name: None,
            transcript: None,
        };
        let project = "/work/acme-web";
        let meta =
            |cwd, branch, matches, all| session_meta(&hit(cwd, branch, matches), now, project, all);
        // The project's own folder, or none, goes unsaid; one matching message too.
        assert_eq!(
            meta(Some(project), Some("main"), 1, false),
            "5 minutes ago on main"
        );
        assert_eq!(meta(None, None, 0, true), "5 minutes ago");
        // A folder inside the project by its path from there, except over every project.
        let inside = Some("/work/acme-web/packages/ui");
        assert_eq!(meta(inside, None, 0, false), "5 minutes ago in packages/ui");
        assert_eq!(meta(inside, None, 0, true), "5 minutes ago in ui");
        // Any other folder by its last part, even one whose name starts with the project's.
        assert_eq!(
            meta(Some("/work/acme-web-login"), None, 0, false),
            "5 minutes ago in acme-web-login"
        );
        assert_eq!(
            meta(Some("/work/acme-api/"), Some("release-2"), 3, false),
            "5 minutes ago on release-2 in acme-api · 3 matching messages"
        );
    }

    #[test]
    fn the_searched_words_are_marked_wherever_they_are() {
        let words = terms("Login \"fix the\"");
        let text = "Fix the LOGIN redirect, then fix the login test";
        let marked: Vec<_> = highlights(text, &words)
            .into_iter()
            .map(|range| &text[range])
            .collect();
        assert_eq!(marked, ["Fix the", "LOGIN", "fix the", "login"]);
        assert!(highlights(text, &terms("")).is_empty());
    }
}
