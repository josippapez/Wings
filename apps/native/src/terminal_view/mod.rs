//! First native terminal slice. The worker owns Alacritty; GPUI only paints immutable snapshots.

mod find;
mod find_bar;
mod input;
mod links;
mod mouse;
mod paint;
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
    SharedString, Subscription, Task, prelude::*, px,
};

pub use wings_terminal::alacritty_terminal::vte::ansi::CursorShape;

use crate::text_field::TextField;

use self::{
    links::Link,
    mouse::Hit,
    snapshot::{Latest, Snapshot},
    worker::{Command, Geometry, Runtime, fit, start},
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

/// The desktop's default terminal font on macOS.
pub const DEFAULT_FONT_FAMILY: &str = "Menlo";

/// The Settings that change how every terminal draws and types, like the desktop's terminal look.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalLook {
    /// An installed font family, else the default.
    pub family: SharedString,
    /// Shift-Return sends Meta-Return, a new line in Claude Code.
    pub shift_return_meta: bool,
    /// The cursor programs get until they set their own.
    pub cursor: CursorShape,
}

impl Default for TerminalLook {
    fn default() -> Self {
        Self {
            family: DEFAULT_FONT_FAMILY.into(),
            shift_return_meta: true,
            cursor: CursorShape::Block,
        }
    }
}

impl Global for TerminalLook {}

pub fn look(cx: &App) -> TerminalLook {
    cx.try_global::<TerminalLook>().cloned().unwrap_or_default()
}

/// 20px lines at 14px, about what the desktop's `lineHeight: 1.2` gives in Menlo.
fn line_height(font_size: f32) -> f32 {
    (font_size * 1.4).round()
}
const RESIZE_DELAY: Duration = Duration::from_millis(80);
const REDRAW_DELAY: Duration = Duration::from_millis(16);

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
    pub fn open(cwd: PathBuf, history: Vec<u8>, cx: &mut App) -> Result<Entity<Self>, String> {
        let (runtime, latest, updates) = start(cwd, history)?;
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
                            view.snapshot = view.latest.lock().unwrap().clone();
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
        let quit = cx.on_app_quit(|view, _| {
            view.runtime.shutdown();
            async {}
        });
        let cursor = look(cx).cursor;
        if cursor != CursorShape::Block {
            let _ = runtime.commands.try_send(Command::Cursor(cursor));
        }
        let look_changed = cx.observe_global::<TerminalLook>(|view, cx| {
            let _ = view
                .runtime
                .commands
                .try_send(Command::Cursor(look(cx).cursor));
        });
        Self {
            focus: cx.focus_handle(),
            runtime,
            snapshot: latest.lock().unwrap().clone(),
            latest: latest.clone(),
            bounds: None,
            cell_width: px(8.0),
            line_height: line_height(DEFAULT_FONT_SIZE),
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
            visible: true,
            autofocus: true,
            scroll_rest: 0.0,
            drag: None,
            last_hit: None,
            link: None,
            find: None,
        }
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
