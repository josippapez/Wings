//! First native terminal slice. The worker owns the libghostty-vt grid; GPUI only paints immutable snapshots.

mod find;
mod find_bar;
mod input;
mod links;
mod mouse;
mod paint;
#[cfg(all(test, unix))]
mod server_tests;
mod snapshot;
#[cfg(test)]
mod tests;
mod worker;

use std::{
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use async_channel::Receiver;
use gpui::{
    App, Bounds, ClipboardItem, Context, Entity, FocusHandle, Focusable, Global, Pixels,
    SharedString, Subscription, Task, Window, prelude::*, px,
};

pub use input::{OPTION_AS_META, OptionAsMeta};
pub use wings_terminal::grid::CursorShape;

use crate::text_field::TextField;

use self::{
    links::Link,
    mouse::Hit,
    snapshot::{Latest, Snapshot},
    worker::{Command, Geometry, Options, Runtime, fit, start, start_remote},
};

/// The desktop terminal's default size. Increase and Decrease Font Size go from 9 to 24.
pub const DEFAULT_FONT_SIZE: f32 = 13.0;
pub const MIN_FONT_SIZE: f32 = 9.0;
pub const MAX_FONT_SIZE: f32 = 24.0;

/// The font size every terminal uses.
pub struct TerminalFont(pub f32);

impl Global for TerminalFont {}

pub fn font_size(cx: &App) -> f32 {
    cx.try_global::<TerminalFont>()
        .map_or(DEFAULT_FONT_SIZE, |font| font.0)
}

/// The desktop's cursor choices: each shape, its saved name and its label.
pub const CURSOR_STYLES: [(CursorShape, &str, &str); 3] = [
    (CursorShape::Block, "block", "Block"),
    (CursorShape::Beam, "bar", "Bar"),
    (CursorShape::Underline, "underline", "Underline"),
];

/// The desktop's default terminal font on macOS: its stack starts with `ui-monospace`, which is the system's SF Mono.
/// macOS names it only by this hidden family; "SF Mono" isn't installed as a family of its own.
pub const DEFAULT_FONT_FAMILY: &str = ".AppleSystemUIFontMonospaced";

/// The desktop's line heights, as multiples of the font size.
pub const LINE_HEIGHTS: [f32; 7] = [1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6];
/// The desktop's minimum contrast choices: the ratio and its label. 1 is off.
pub const CONTRASTS: [(f32, &str); 3] = [(1.0, "Off"), (4.5, "Readable (4.5)"), (7.0, "High (7)")];
/// The desktop's scrollback choices, in lines per pane.
pub const SCROLLBACKS: [usize; 6] = [1_000, 5_000, 10_000, 25_000, 50_000, 100_000];

/// How the cursor of a pane without focus looks, as the desktop's `cursorInactiveStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InactiveCursor {
    Outline,
    Block,
    Bar,
    Underline,
    Hidden,
}

/// The desktop's choices: each style, its saved name and its label.
pub const INACTIVE_CURSORS: [(InactiveCursor, &str, &str); 5] = [
    (InactiveCursor::Outline, "outline", "Outline"),
    (InactiveCursor::Block, "block", "Block"),
    (InactiveCursor::Bar, "bar", "Bar"),
    (InactiveCursor::Underline, "underline", "Underline"),
    (InactiveCursor::Hidden, "none", "Hidden"),
];

/// The Settings that change how every terminal draws and types, like the desktop's terminal look.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalLook {
    /// An installed font family, else the default.
    pub family: SharedString,
    /// Shift-Return sends Meta-Return, a new line in Claude Code.
    pub shift_return_meta: bool,
    /// Which Option key types Meta.
    pub option_as_meta: OptionAsMeta,
    /// The cursor programs get until they set their own.
    pub cursor: CursorShape,
    /// The focused pane's cursor turns off and on, as xterm's `cursorBlink`.
    pub cursor_blink: bool,
    /// The cursor of a pane without focus.
    pub inactive_cursor: InactiveCursor,
    /// Spacing between lines, as a multiple of the font size.
    pub line_height: f32,
    /// Text below this contrast with its background is lightened or darkened; 1 leaves it.
    pub min_contrast: f32,
    /// Lines of history each pane keeps.
    pub scrollback: usize,
    /// A selection goes to the clipboard when the mouse button is let go.
    pub copy_on_select: bool,
    /// Programs may put text on the clipboard (OSC 52).
    pub clipboard_write: bool,
}

impl Default for TerminalLook {
    fn default() -> Self {
        Self {
            family: DEFAULT_FONT_FAMILY.into(),
            shift_return_meta: true,
            option_as_meta: OptionAsMeta::Left,
            cursor: CursorShape::Block,
            cursor_blink: true,
            inactive_cursor: InactiveCursor::Outline,
            line_height: 1.2,
            min_contrast: 1.0,
            scrollback: wings_terminal::grid::HISTORY_LINES,
            copy_on_select: false,
            clipboard_write: true,
        }
    }
}

impl Global for TerminalLook {}

pub fn look(cx: &App) -> TerminalLook {
    cx.try_global::<TerminalLook>().cloned().unwrap_or_default()
}

/// A row's height in points. xterm multiplies the font's own line height, about 7/6 of its size in Menlo, so the
/// desktop's default 1.2 gives 20px lines at 14px.
fn line_height(font_size: f32, factor: f32) -> f32 {
    (font_size * factor * 7.0 / 6.0).round()
}
const RESIZE_DELAY: Duration = Duration::from_millis(80);
const REDRAW_DELAY: Duration = Duration::from_millis(16);
/// xterm's blink: the cursor turns off or on every 600 ms, and shows again whenever it moves.
const BLINK_INTERVAL: Duration = Duration::from_millis(600);

/// A pane hidden this long keeps its scrollback as text until it's shown (the desktop's
/// `TERMINAL_UNLOAD_MS`). A full pane's 10,000 lines cost about 51 MB as cells at 213 columns, and
/// about 0.5 MB as text (measured).
const UNLOAD_AFTER: Duration = Duration::from_secs(10 * 60);

pub struct TerminalView {
    focus: FocusHandle,
    runtime: Runtime,
    latest: Latest,
    snapshot: Arc<Snapshot>,
    bounds: Option<Bounds<Pixels>>,
    cell_width: Pixels,
    line_height: f32,
    geometry: Option<Geometry>,
    resize_task: Option<Task<()>>,
    _updates: Task<()>,
    _clipboard: Task<()>,
    /// Asks the model for one more re-find once output has been quiet.
    find_settle: Option<Task<()>>,
    /// Counts down while the pane is hidden, then stores its scrollback.
    unload: Option<Task<()>>,
    _quit: Subscription,
    _look: Subscription,
    error: Option<String>,
    marked: String,
    /// Keys you typed are waiting in the prompt, not yet sent with Enter or cleared with Ctrl-C (the desktop's PaneInput).
    typed: bool,
    visible: bool,
    autofocus: bool,
    /// Wheel movement smaller than one line, kept for the next event.
    scroll_rest: f32,
    drag: Option<Drag>,
    last_hit: Option<Hit>,
    /// The link under the pointer, underlined like in the desktop terminal.
    link: Option<Link>,
    /// Made on the first Cmd-F and kept, so the term is still there when it opens again.
    find: Option<FindBar>,
    /// Turns the cursor off and on while it blinks; none while the pane isn't focused or blinking is off.
    blink: Option<Task<()>>,
    /// False in the off half of a blink.
    cursor_lit: bool,
}

struct FindBar {
    open: bool,
    field: Entity<TextField>,
    case_sensitive: bool,
    regex: bool,
    /// Match case, regular expression, previous, next and close.
    buttons: [FocusHandle; 5],
    _events: Subscription,
}

/// What a held mouse button is doing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    Select,
    /// The program asked for mouse reports, so the press went to it.
    Report(u8),
}

impl TerminalView {
    /// Starts a shell in `cwd`. Fails before creating the view when the PTY can't start.
    /// `history` is output saved from this pane's last shell, shown above the new one.
    /// `claude_config_dir` is the folder of the Claude Code account the pane's project uses, for the shell's
    /// `CLAUDE_CONFIG_DIR`; without one the shell keeps the app's own, the default account.
    pub fn open(
        cwd: PathBuf,
        history: Vec<u8>,
        claude_config_dir: Option<PathBuf>,
        cx: &mut App,
    ) -> Result<Entity<Self>, String> {
        let options = Options::of(&look(cx));
        let (runtime, latest, updates) = start(cwd, history, claude_config_dir, options)?;
        Ok(cx.new(|cx| Self::new(runtime, latest, updates, cx)))
    }

    /// Starts a shell in the terminal server at `socket`, where it outlives the app, under the saved pane's
    /// `history_id` so the next app finds it. `history` is shown above it, as in `open`, and is the start of what the
    /// server keeps of the pane.
    #[cfg(unix)]
    pub fn open_in_server(
        socket: &std::path::Path,
        space_id: String,
        history_id: String,
        cwd: PathBuf,
        history: Vec<u8>,
        claude_config_dir: Option<PathBuf>,
        cx: &mut App,
    ) -> Result<Entity<Self>, String> {
        let env = wings_terminal::shell::pane_env(claude_config_dir.as_deref())
            .into_iter()
            .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
            .collect();
        let spawn = wings_ptyd::proto::Spawn {
            space_id,
            cwd,
            cols: 80,
            rows: 24,
            history_id: Some(history_id),
            env,
        };
        let kept = wings_ptyd::output::restored(history.clone());
        let (pane, events) = wings_ptyd::client::Pane::spawn(socket, spawn, &kept)
            .map_err(|error| format!("The terminal server couldn't start a shell: {error}"))?;
        let options = Options::of(&look(cx));
        let (runtime, latest, updates) = start_remote(pane, events, (80, 24), history, options)?;
        Ok(cx.new(|cx| Self::new(runtime, latest, updates, cx)))
    }

    /// Attaches to a pane the terminal server at `socket` kept running, which shows what it printed meanwhile.
    #[cfg(unix)]
    pub fn attach_in_server(
        socket: &std::path::Path,
        info: &wings_ptyd::proto::PaneInfo,
        cx: &mut App,
    ) -> Result<Entity<Self>, String> {
        let (pane, events, attached) = wings_ptyd::client::Pane::attach(socket, info)
            .map_err(|error| format!("Couldn't reach a pane in the terminal server: {error}"))?;
        let size = match (attached.cols, attached.rows) {
            (0, _) | (_, 0) => (80, 24),
            size => size,
        };
        let options = Options::of(&look(cx));
        let (runtime, latest, updates) = start_remote(pane, events, size, Vec::new(), options)?;
        Ok(cx.new(|cx| Self::new(runtime, latest, updates, cx)))
    }

    fn new(
        runtime: Runtime,
        latest: Latest,
        updates: Receiver<()>,
        cx: &mut Context<Self>,
    ) -> Self {
        let update_task = cx.spawn(async move |view, cx| {
            while updates.recv().await.is_ok() {
                cx.background_executor().timer(REDRAW_DELAY).await;
                while updates.try_recv().is_ok() {}
                if view
                    .update(cx, |view, cx| {
                        if view.visible {
                            let cursor = view.snapshot.cursor;
                            view.snapshot = view.latest.lock().unwrap().clone();
                            if view.blink.is_some() && view.snapshot.cursor != cursor {
                                view.restart_blink(cx);
                            }
                            if view.snapshot.find_stale {
                                view.settle_find(cx);
                            }
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let clipboard = runtime.clipboard.clone();
        let clipboard_task = cx.spawn(async move |_, cx| {
            while let Ok(text) = clipboard.recv().await {
                cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text)));
            }
        });
        // A pane in the terminal server runs on for the next app.
        let quit = cx.on_app_quit(|view, _| {
            view.runtime.detach();
            async {}
        });
        // Blocking, not try_send: a dropped change could leave clipboard writes on after they were turned off.
        let look_changed = cx.observe_global::<TerminalLook>(|view, cx| {
            let _ = view
                .runtime
                .commands
                .send_blocking(Command::Options(Options::of(&look(cx))));
        });
        Self {
            focus: cx.focus_handle(),
            runtime,
            snapshot: latest.lock().unwrap().clone(),
            latest: latest.clone(),
            bounds: None,
            cell_width: px(8.0),
            line_height: line_height(DEFAULT_FONT_SIZE, 1.2),
            geometry: None,
            resize_task: None,
            _updates: update_task,
            _clipboard: clipboard_task,
            find_settle: None,
            unload: None,
            _quit: quit,
            _look: look_changed,
            error: None,
            marked: String::new(),
            typed: false,
            visible: true,
            autofocus: true,
            scroll_rest: 0.0,
            drag: None,
            last_hit: None,
            link: None,
            find: None,
            blink: None,
            cursor_lit: true,
        }
    }

    /// Blinks only in the focused pane of the active window, as xterm pauses on blur, so nothing redraws for it
    /// otherwise. Called on every render, which a change of focus, of the active window or of Settings causes.
    fn update_blink(&mut self, window: &Window, cx: &mut Context<Self>) {
        let blinking = look(cx).cursor_blink
            && self.visible
            && window.is_window_active()
            && self.focus.is_focused(window);
        if !blinking {
            self.blink = None;
            self.cursor_lit = true;
        } else if self.blink.is_none() {
            self.restart_blink(cx);
        }
    }

    /// Shows the cursor and starts the next blink a whole interval from now, as xterm does when the cursor moves.
    fn restart_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_lit = true;
        self.blink = Some(cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(BLINK_INTERVAL).await;
                if view
                    .update(cx, |view, cx| {
                        view.cursor_lit = !view.cursor_lit;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    /// The shell's process id, so its owner can check it ended.
    pub fn process_id(&self) -> Option<u32> {
        self.runtime.process.process_id()
    }

    /// The program in front, which agent detection reads first.
    pub fn foreground_pid(&self) -> Option<u32> {
        self.runtime.process.foreground_pid()
    }

    /// The pane's latest output, up to 1 MiB, to save for the next launch.
    pub fn recent_output(&self) -> Vec<u8> {
        self.runtime
            .recent
            .lock()
            .unwrap()
            .iter()
            .copied()
            .collect()
    }

    /// The folder the shell is in now.
    pub fn cwd(&self) -> Option<PathBuf> {
        self.runtime.process.cwd()
    }

    /// The title the program in the terminal set, if any.
    pub fn title(&self) -> Option<String> {
        self.runtime.title.lock().unwrap().clone()
    }

    /// Tab owners call this when unmounting/hiding a retained view. Parsing continues, painting stops.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if visible != self.visible {
            self.unload = (!visible).then(|| {
                let commands = self.runtime.commands.clone();
                cx.spawn(async move |_, cx| {
                    cx.background_executor().timer(UNLOAD_AFTER).await;
                    // A busy pane just keeps its scrollback a while longer.
                    let _ = commands.try_send(Command::Unload);
                })
            });
        }
        self.visible = visible;
        self.runtime.visible.store(visible, Ordering::Release);
        if visible {
            self.snapshot = self.latest.lock().unwrap().clone();
            self.send(Command::Refresh, cx);
            cx.notify();
        }
    }

    fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        match self.runtime.commands.try_send(command) {
            Ok(()) => self.error = None,
            Err(async_channel::TrySendError::Full(_)) => {
                self.error = Some("Terminal is busy. Input was not sent.".into())
            }
            Err(async_channel::TrySendError::Closed(_)) => {
                self.error = Some("The terminal has closed.".into())
            }
        }
        if self.error.is_some() {
            cx.notify();
        }
    }

    fn layout(
        &mut self,
        bounds: Bounds<Pixels>,
        cell_width: Pixels,
        line_height: f32,
        cx: &mut Context<Self>,
    ) {
        self.bounds = Some(bounds);
        self.cell_width = cell_width;
        self.line_height = line_height;
        let Some(geometry) = fit(
            bounds.size.width.as_f32(),
            bounds.size.height.as_f32(),
            cell_width.as_f32(),
            line_height,
        ) else {
            return;
        };
        if self.geometry == Some(geometry) {
            return;
        }
        self.geometry = Some(geometry);
        let commands = self.runtime.commands.clone();
        self.resize_task = Some(cx.spawn(async move |view, cx| {
            cx.background_executor().timer(RESIZE_DELAY).await;
            if commands.send(Command::Resize(geometry)).await.is_err() {
                let _ = view.update(cx, |view, cx| {
                    view.error = Some("The terminal has closed.".into());
                    cx.notify();
                });
            }
        }));
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
