//! The model thread: the PTY and its terminal grid, driven by commands, publishing snapshots.

use std::{
    collections::VecDeque,
    io::Read,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use async_channel::{Receiver, Sender};
use wings_ptyd::client;
use wings_terminal::{
    PtyProcess,
    grid::{CursorShape, Event, GridPane, Scroll, SelectionType, TermMode},
    history,
};

use super::{
    DEFAULT_FONT_SIZE, TerminalLook,
    find::{Find, FindMove, FindQuery, REFIND_EVERY, find_step, refind},
    line_height,
    mouse::Hit,
    snapshot::{Latest, Snapshot, snapshot},
};

const OUTPUT_CHUNK: usize = 16 * 1024;
const OUTPUT_SLOTS: usize = 8;
const QUEUE_SLOTS: usize = 32;
const CLIPBOARD_SLOTS: usize = 4;

static NEXT_PANE: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Geometry {
    pub(super) cols: u16,
    pub(super) rows: u16,
    pub(super) cell_width: u16,
    pub(super) cell_height: u16,
}

pub(super) fn fit(width: f32, height: f32, cell_width: f32, line_height: f32) -> Option<Geometry> {
    if !width.is_finite()
        || !height.is_finite()
        || !cell_width.is_finite()
        || width <= 0.0
        || height <= 0.0
        || cell_width <= 0.0
    {
        return None;
    }
    let cell_pixels = cell_width.ceil().max(1.0) as u16;
    Some(Geometry {
        cols: (width / cell_width)
            .floor()
            .clamp(2.0, (u16::MAX / cell_pixels) as f32) as u16,
        rows: (height / line_height)
            .floor()
            .clamp(1.0, (u16::MAX / line_height as u16) as f32) as u16,
        cell_width: cell_pixels,
        cell_height: line_height as u16,
    })
}

pub(super) enum Command {
    Output,
    /// Typed, pasted or mouse input. It brings the view back to the bottom first, like the desktop terminal.
    Input(Vec<u8>),
    Resize(Geometry),
    Refresh,
    Options(Options),
    Scroll(Scroll),
    SelectFrom(Hit, SelectionType),
    SelectTo(Hit),
    Copy,
    Find(FindQuery, FindMove),
    FindClose,
    /// Stores the scrollback as text if the pane is still hidden.
    Unload,
    /// Everything the terminal server kept of the pane, drawn on a fresh screen.
    Replay(Vec<u8>),
    Eof(Option<String>),
}

/// What the Settings change in the model: the cursor programs get until they set their own, the lines of history
/// kept, and whether programs may write the clipboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Options {
    pub(super) cursor: CursorShape,
    pub(super) scrollback: usize,
    pub(super) clipboard_write: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self::of(&TerminalLook::default())
    }
}

impl Options {
    pub(super) fn of(look: &TerminalLook) -> Self {
        Self {
            cursor: look.cursor,
            scrollback: look.scrollback,
            clipboard_write: look.clipboard_write,
        }
    }
}

/// Where the pane's program runs: in this process, or in the terminal server, where it outlives the app.
#[derive(Clone)]
pub(super) enum Shell {
    Local(Arc<PtyProcess>),
    Remote(Arc<client::Pane>),
}

impl Shell {
    pub(super) fn process_id(&self) -> Option<u32> {
        match self {
            Shell::Local(process) => process.process_id(),
            Shell::Remote(pane) => pane.shell_pid(),
        }
    }

    pub(super) fn foreground_pid(&self) -> Option<u32> {
        match self {
            Shell::Local(process) => process.foreground_pid(),
            Shell::Remote(pane) => pane.foreground_pid(),
        }
    }

    pub(super) fn cwd(&self) -> Option<PathBuf> {
        match self {
            Shell::Local(process) => process.cwd(),
            Shell::Remote(pane) => wings_terminal::cwd_of(pane.shell_pid()?),
        }
    }

    fn write(&self, data: &[u8]) -> std::io::Result<()> {
        match self {
            Shell::Local(process) => process.write(data).map_err(std::io::Error::other),
            Shell::Remote(pane) => pane.write(data),
        }
    }

    fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()> {
        match self {
            Shell::Local(process) => process.resize(cols, rows).map_err(std::io::Error::other),
            Shell::Remote(pane) => pane.resize(cols, rows),
        }
    }

    /// Ends the program, wherever it runs.
    fn kill(&self) {
        match self {
            Shell::Local(process) => process.kill(),
            Shell::Remote(pane) => {
                let _ = pane.kill();
                pane.detach();
            }
        }
    }

    /// At the end of the output: reaps a local shell. The server reaps its own.
    fn reap(&self) {
        if let Shell::Local(process) = self {
            process.reap();
        }
    }
}

pub(super) struct Runtime {
    pub(super) process: Shell,
    pub(super) commands: Sender<Command>,
    /// Text for the system clipboard: a copied selection or a program's OSC 52 write.
    pub(super) clipboard: Receiver<String>,
    /// The window title the program set, kept apart from snapshots so hidden tabs update it too.
    pub(super) title: Arc<Mutex<Option<String>>>,
    pub(super) recent: Recent,
    stopped: Arc<AtomicBool>,
    pub(super) visible: Arc<AtomicBool>,
    /// Set when the shell's output ends: it exited, or the terminal server it ran in went away.
    pub(super) exited: Arc<AtomicBool>,
}

impl Runtime {
    pub(super) fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        self.commands.close();
        self.process.kill();
    }

    /// On quit: a pane in the terminal server goes on running there for the next app. A local one ends.
    pub(super) fn detach(&self) {
        let Shell::Remote(pane) = &self.process else {
            return self.shutdown();
        };
        self.stopped.store(true, Ordering::Release);
        self.commands.close();
        pane.detach();
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// As much of each pane's output as the desktop app keeps to replay after a restart (`REPLAY_BYTES`).
const HISTORY_BYTES: usize = 1 << 20;

/// The pane's latest output, saved on quit and replayed into its next shell.
type Recent = Arc<Mutex<VecDeque<u8>>>;

fn keep_recent(recent: &mut VecDeque<u8>, bytes: &[u8]) {
    recent.extend(bytes);
    let over = recent.len().saturating_sub(HISTORY_BYTES);
    recent.drain(..over);
}

/// Ends output from before a restart like the desktop's `history_end`: the modes the old program left
/// on go off, so the new shell starts as in a fresh terminal.
fn history_end(mode: TermMode) -> Vec<u8> {
    let mut end = String::new();
    for (flag, number) in [
        (TermMode::APP_CURSOR, 1),
        (TermMode::ALT_SCREEN, 1049),
        (TermMode::MOUSE_REPORT_CLICK, 1000),
        (TermMode::MOUSE_DRAG, 1002),
        (TermMode::MOUSE_MOTION, 1003),
        (TermMode::FOCUS_IN_OUT, 1004),
        (TermMode::SGR_MOUSE, 1006),
        (TermMode::BRACKETED_PASTE, 2004),
    ] {
        if mode.contains(flag) {
            end.push_str(&format!("\x1b[?{number}l"));
        }
    }
    if !mode.contains(TermMode::SHOW_CURSOR) {
        end.push_str("\x1b[?25h");
    }
    end.push_str("\x1b[0m\r\n");
    end.into_bytes()
}

pub(super) fn start(
    cwd: PathBuf,
    history: Vec<u8>,
    claude_config_dir: Option<PathBuf>,
    options: Options,
) -> Result<(Runtime, Latest, Receiver<()>), String> {
    let id = format!("p{}", NEXT_PANE.fetch_add(1, Ordering::Relaxed));
    let process = Arc::new(
        PtyProcess::spawn(&cwd, &id, 80, 24, claude_config_dir.as_deref())
            .map_err(|error| error.to_string())?,
    );
    start_process(process, id, history, options)
}

pub(super) fn start_process(
    process: Arc<PtyProcess>,
    id: String,
    history: Vec<u8>,
    options: Options,
) -> Result<(Runtime, Latest, Receiver<()>), String> {
    let mut reader = process.reader().map_err(|error| error.to_string())?;
    let (runtime, latest, updates, output, commands) =
        start_model(Shell::Local(process), &id, (80, 24), history, options)?;
    thread::Builder::new()
        .name(format!("terminal-read-{id}"))
        .spawn(move || {
            let mut buf = [0; OUTPUT_CHUNK];
            let error = loop {
                match reader.read(&mut buf) {
                    Ok(0) => break None,
                    Ok(n) => {
                        if output.send_blocking(buf[..n].to_vec()).is_err() {
                            return;
                        }
                        if commands.send_blocking(Command::Output).is_err() {
                            return;
                        }
                    }
                    Err(error) => break Some(format!("PTY read failed: {error}")),
                }
            };
            let _ = commands.send_blocking(Command::Eof(error));
        })
        .map_err(|error| error.to_string())?;
    Ok((runtime, latest, updates))
}

/// A pane in the terminal server, of `size` (columns, rows) as the server last drew it. `history` is output saved from
/// an earlier shell, for a pane just started; a pane attached again gets the server's own replay instead.
pub(super) fn start_remote(
    pane: client::Pane,
    mut events: client::Events,
    size: (u16, u16),
    history: Vec<u8>,
    options: Options,
) -> Result<(Runtime, Latest, Receiver<()>), String> {
    let pane = Arc::new(pane);
    let id = format!("s{}", pane.number());
    let (runtime, latest, updates, output, commands) =
        start_model(Shell::Remote(pane.clone()), &id, size, history, options)?;
    let stopped = runtime.stopped.clone();
    thread::Builder::new()
        .name(format!("terminal-read-{id}"))
        .spawn(move || {
            let end = loop {
                match events.next(&pane) {
                    client::Event::Output(bytes) => {
                        if output.send_blocking(bytes).is_err()
                            || commands.send_blocking(Command::Output).is_err()
                        {
                            return;
                        }
                    }
                    client::Event::Replay(bytes) => {
                        if commands.send_blocking(Command::Replay(bytes)).is_err() {
                            return;
                        }
                    }
                    client::Event::Exited => break None,
                    // Let go on quit: the pane runs on in the server, and this view is going.
                    client::Event::Closed if stopped.load(Ordering::Acquire) => return,
                    client::Event::Closed => break Some("The terminal server stopped.".to_owned()),
                }
            };
            let _ = commands.send_blocking(Command::Eof(end));
        })
        .map_err(|error| error.to_string())?;
    Ok((runtime, latest, updates))
}

type Started = (
    Runtime,
    Latest,
    Receiver<()>,
    Sender<Vec<u8>>,
    Sender<Command>,
);

/// The model thread, and what feeds it: the output channel and the command channel.
fn start_model(
    process: Shell,
    id: &str,
    size: (u16, u16),
    history: Vec<u8>,
    options: Options,
) -> Result<Started, String> {
    // Output uses only eight command slots, leaving room for control keys during a flood.
    // At most 128 KiB of output is queued, plus one read chunk. Full queues stop PTY reads.
    let (commands, receiver) = async_channel::bounded(QUEUE_SLOTS);
    let (output, output_rx) = async_channel::bounded(OUTPUT_SLOTS);
    let (dirty, updates) = async_channel::bounded(1);
    let (clipboard, clipboard_rx) = async_channel::bounded(CLIPBOARD_SLOTS);
    let latest = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
    let runtime = Runtime {
        process: process.clone(),
        commands: commands.clone(),
        clipboard: clipboard_rx,
        title: Arc::new(Mutex::new(None)),
        recent: Arc::new(Mutex::new(VecDeque::new())),
        stopped: Arc::new(AtomicBool::new(false)),
        visible: Arc::new(AtomicBool::new(true)),
        exited: Arc::new(AtomicBool::new(false)),
    };
    let stopped = runtime.stopped.clone();
    let visible = runtime.visible.clone();
    let exited = runtime.exited.clone();
    let title = runtime.title.clone();
    let recent = runtime.recent.clone();
    let state = latest.clone();
    thread::Builder::new()
        .name(format!("terminal-model-{id}"))
        .spawn(move || {
            run_model(
                process,
                receiver,
                output_rx,
                Outputs {
                    dirty,
                    clipboard,
                    title,
                    recent,
                    latest: state,
                },
                size,
                history,
                options,
                stopped,
                visible,
                exited,
            );
        })
        .map_err(|error| error.to_string())?;
    Ok((runtime, latest, updates, output, commands))
}

/// Where the model thread sends its results.
struct Outputs {
    dirty: Sender<()>,
    clipboard: Sender<String>,
    title: Arc<Mutex<Option<String>>>,
    recent: Recent,
    latest: Latest,
}

#[allow(clippy::too_many_arguments)] // the model thread's whole starting state
fn run_model(
    process: Shell,
    receiver: Receiver<Command>,
    output: Receiver<Vec<u8>>,
    Outputs {
        dirty,
        clipboard,
        title,
        recent,
        latest,
    }: Outputs,
    (cols, rows): (u16, u16),
    history: Vec<u8>,
    mut options: Options,
    stopped: Arc<AtomicBool>,
    visible: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
) {
    let mut pane = GridPane::new(cols as usize, rows as usize);
    let mut geometry = Geometry {
        cols,
        rows,
        cell_width: 8,
        cell_height: line_height(DEFAULT_FONT_SIZE, 1.2) as u16,
    };
    pane.set_cell_size(geometry.cell_width, geometry.cell_height);
    // The settings apply before the first output is read, so a program can't copy before clipboard writes are off.
    if options != Options::default() {
        pane.set_options(options.cursor, options.scrollback);
    }
    let mut status = None;
    let mut find: Option<Find> = None;
    let mut last_refind = Instant::now();
    let mut closed = false;
    let mut stored: Option<Vec<u8>> = None;
    if !history.is_empty() {
        pane.feed(&history);
        let end = history_end(pane.mode());
        pane.feed(&end);
        // Replies, titles and clipboard writes belong to the old program. None reach the new shell.
        pane.take_events();
        let mut kept = recent.lock().unwrap();
        keep_recent(&mut kept, &history);
        keep_recent(&mut kept, &end);
        drop(kept);
        *latest.lock().unwrap() = Arc::new(snapshot(&mut pane, None, false, None));
    }
    while let Ok(first) = receiver.recv_blocking() {
        if stopped.load(Ordering::Acquire) {
            break;
        }
        let batch_started = Instant::now();
        let mut command = Some(first);
        let mut processed = 0;
        let mut moved = false;
        while let Some(next) = command.take() {
            moved |= matches!(
                next,
                Command::Output | Command::Resize(_) | Command::Unload | Command::Replay(_)
            );
            let result = match next {
                Command::Output => {
                    let bytes = output
                        .recv_blocking()
                        .expect("output notification follows its bytes");
                    pane.feed(&bytes);
                    keep_recent(&mut recent.lock().unwrap(), &bytes);
                    let mut result = Ok(());
                    for event in pane.take_events() {
                        match event {
                            // Like the desktop terminal, programs may write the clipboard. The grid never lets
                            // them read it.
                            Event::Clipboard(text) => {
                                if options.clipboard_write {
                                    let _ = clipboard.try_send(text);
                                }
                            }
                            Event::Title(text) => *title.lock().unwrap() = text,
                            Event::PtyWrite(reply) => {
                                if let Err(error) = process.write(&reply) {
                                    result = Err(error);
                                    break;
                                }
                            }
                        }
                    }
                    result
                }
                Command::Input(bytes) => {
                    pane.scroll(Scroll::Bottom);
                    process.write(&bytes)
                }
                Command::Scroll(scroll) => {
                    pane.scroll(scroll);
                    Ok(())
                }
                Command::SelectFrom(hit, ty) => {
                    pane.select_from(hit.row, hit.col, hit.side, ty);
                    Ok(())
                }
                Command::SelectTo(hit) => {
                    pane.select_to(hit.row, hit.col, hit.side);
                    Ok(())
                }
                Command::Find(query, movement) => {
                    let previous = find.take().map(|find| {
                        if find.stale {
                            refind(&mut pane, find)
                        } else {
                            find
                        }
                    });
                    find = find_step(&mut pane, previous, &query, movement);
                    Ok(())
                }
                Command::FindClose => {
                    find = None;
                    Ok(())
                }
                Command::Copy => {
                    if let Some(text) = pane.selection_to_string() {
                        let _ = clipboard.try_send(text);
                    }
                    Ok(())
                }
                Command::Resize(next) => {
                    if next != geometry {
                        pane.resize(next.cols as usize, next.rows as usize);
                        pane.set_cell_size(next.cell_width, next.cell_height);
                        geometry = next;
                        process.resize(next.cols, next.rows)
                    } else {
                        Ok(())
                    }
                }
                Command::Refresh => Ok(()),
                Command::Options(next) => {
                    pane.set_options(next.cursor, next.scrollback);
                    options = next;
                    Ok(())
                }
                Command::Unload => {
                    // A full-screen program's screen isn't the history, so it stays as it is.
                    if !visible.load(Ordering::Acquire)
                        && stored.is_none()
                        && !pane.mode().contains(TermMode::ALT_SCREEN)
                        && pane.history_size() >= history::MIN_LINES
                    {
                        stored = pane.store_history();
                    }
                    Ok(())
                }
                Command::Replay(bytes) => {
                    pane = GridPane::new(geometry.cols as usize, geometry.rows as usize);
                    pane.set_cell_size(geometry.cell_width, geometry.cell_height);
                    pane.set_options(options.cursor, options.scrollback);
                    pane.feed(&bytes);
                    // Replies and clipboard writes answered a terminal long gone. Only the title still holds.
                    for event in pane.take_events() {
                        if let Event::Title(text) = event {
                            *title.lock().unwrap() = text;
                        }
                    }
                    let mut kept = recent.lock().unwrap();
                    kept.clear();
                    keep_recent(&mut kept, &bytes);
                    stored = None;
                    Ok(())
                }
                Command::Eof(error) => {
                    status = Some(error.unwrap_or_else(|| "Shell output closed.".into()));
                    process.reap();
                    closed = true;
                    exited.store(true, Ordering::Release);
                    Ok(())
                }
            };
            if let Err(error) = result {
                status = Some(format!("Terminal I/O failed: {error}"));
                process.kill();
                closed = true;
            }
            processed += 1;
            if closed
                || stopped.load(Ordering::Acquire)
                || processed >= 16
                || batch_started.elapsed() >= Duration::from_millis(4)
            {
                break;
            }
            command = receiver.try_recv().ok();
        }
        if visible.load(Ordering::Acquire)
            && !pane.mode().contains(TermMode::ALT_SCREEN)
            && let Some(lines) = stored.take()
        {
            pane.restore_history(&lines);
            moved = true;
        }
        if moved && let Some(current) = &mut find {
            current.active = None;
            current.stale = true;
        }
        if find.as_ref().is_some_and(|find| find.stale)
            && last_refind.elapsed() >= REFIND_EVERY
            && let Some(current) = find.take()
        {
            find = Some(refind(&mut pane, current));
            last_refind = Instant::now();
        }
        if visible.load(Ordering::Acquire) || closed {
            *latest.lock().unwrap() =
                Arc::new(snapshot(&mut pane, status.clone(), closed, find.as_mut()));
            // The end goes out even while hidden, so the pane's owner hears its shell exited.
            if visible.load(Ordering::Acquire) || closed {
                let _ = dirty.try_send(());
            }
        }
        if closed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_uses_whole_cells_and_ignores_hidden_bounds() {
        assert_eq!(
            fit(800.0, 480.0, 8.0, 20.0),
            Some(Geometry {
                cols: 100,
                rows: 24,
                cell_width: 8,
                cell_height: 20
            })
        );
        assert_eq!(
            fit(803.0, 499.0, 8.0, 20.0).map(|size| (size.cols, size.rows)),
            Some((100, 24))
        );
        assert_eq!(fit(0.0, 480.0, 8.0, 20.0), None);
        assert_eq!(fit(800.0, 0.0, 8.0, 20.0), None);
        assert_eq!(fit(800.0, 480.0, 0.0, 20.0), None);
        assert_eq!(fit(f32::NAN, 480.0, 8.0, 20.0), None);
        assert_eq!(
            fit(1.0, 1.0, 8.0, 20.0).map(|size| (size.cols, size.rows)),
            Some((2, 1))
        );
    }

    #[test]
    fn recent_output_keeps_the_last_mebibyte() {
        let mut recent = VecDeque::new();
        keep_recent(&mut recent, &vec![b'a'; HISTORY_BYTES - 1]);
        keep_recent(&mut recent, b"bcd");
        assert_eq!(recent.len(), HISTORY_BYTES);
        assert_eq!(
            recent.iter().rev().take(3).copied().collect::<Vec<_>>(),
            b"dcb"
        );
    }
}
