//! Renderer benchmark (`WINGS_BENCH=replay`, run by `scripts/bench-renderers.sh`): replays a recorded Claude Code
//! session into many panes, as raw bytes for xterm.js or, with `--features bench-canvas`, as rows parsed by
//! `alacritty_terminal` for a canvas, and samples the CPU time and memory of Wings and its WebKit processes.
use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{
    ipc::{Channel, InvokeResponseBody, JavaScriptChannelId},
    Webview,
};

#[derive(Serialize)]
pub struct BenchConfig {
    /// `synthetic` (the original `WINGS_BENCH=1` benchmark), `replay` or `empty`.
    suite: String,
    /// `dom`, `webgl`, `canvas` (one canvas per pane) or `canvas-shared` (one for all panes).
    renderer: String,
}

#[tauri::command]
pub fn bench_config() -> BenchConfig {
    let suite = std::env::var("WINGS_BENCH").unwrap_or_default();
    BenchConfig {
        suite: if suite == "replay" || suite == "empty" { suite } else { "synthetic".into() },
        renderer: std::env::var("WINGS_BENCH_RENDERER").unwrap_or_else(|_| "webgl".into()),
    }
}

/// A PTY recording: `u16 cols, u16 rows`, then chunks of `u32 ms since start, u32 len, bytes`.
struct Recording {
    cols: usize,
    rows: usize,
    chunks: Vec<(u32, Vec<u8>)>,
}

fn recording() -> Result<Recording, String> {
    let path = std::env::var_os("WINGS_BENCH_REPLAY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/claude-fullscreen.rec").into());
    let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let u16_at = |i: usize| data.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |i: usize| data.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let bad = || "bad recording".to_string();
    let (cols, rows) = (u16_at(0).ok_or_else(bad)?, u16_at(2).ok_or_else(bad)?);
    let mut chunks = Vec::new();
    let mut i = 4;
    while i < data.len() {
        let (t, n) = (u32_at(i).ok_or_else(bad)?, u32_at(i + 4).ok_or_else(bad)? as usize);
        chunks.push((t, data.get(i + 8..i + 8 + n).ok_or_else(bad)?.to_vec()));
        i += 8 + n;
    }
    Ok(Recording { cols, rows, chunks })
}

#[derive(Serialize)]
pub struct RecordingInfo {
    cols: usize,
    rows: usize,
    bytes: usize,
    ms: u32,
}

#[tauri::command]
pub fn bench_recording() -> Result<RecordingInfo, String> {
    let rec = recording()?;
    Ok(RecordingInfo {
        cols: rec.cols,
        rows: rec.rows,
        bytes: rec.chunks.iter().map(|c| c.1.len()).sum(),
        ms: rec.chunks.last().map_or(0, |c| c.0),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayPlan {
    /// Send rows parsed in Rust instead of raw bytes.
    grid: bool,
    /// Per pane. A hidden grid pane is parsed but sends no rows, the way it would in the app.
    visible: Vec<bool>,
    /// Per pane, where in the recording to start: everything before arrives at once, as after a tab switch.
    from_ms: Vec<u32>,
    /// Play this long at the recorded pace; without it, send the whole recording as fast as possible.
    play_ms: Option<u32>,
}

#[derive(Serialize, Default)]
pub struct ReplayStats {
    bytes: u64,
    messages: u64,
    ms: f64,
}

/// One frame of changed rows per display refresh at most, like a renderer that draws once per frame.
const FRAME: Duration = Duration::from_millis(16);
/// What `pty.rs` reads at once when a lot of output is waiting.
const READ: usize = 64 * 1024;

trait Sink {
    fn feed(&mut self, bytes: &[u8]);
    /// Sends what's pending. `full` sends every row, not only the changed ones.
    fn flush(&mut self, full: bool);
    fn dirty(&self) -> bool;
}

struct Raw<'a> {
    channel: &'a Channel<InvokeResponseBody>,
    pending: Vec<u8>,
    stats: &'a mut ReplayStats,
}

impl Sink for Raw<'_> {
    fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() >= READ {
            self.flush(false);
        }
    }
    fn flush(&mut self, _full: bool) {
        if self.pending.is_empty() {
            return;
        }
        self.stats.bytes += self.pending.len() as u64;
        self.stats.messages += 1;
        let _ = self.channel.send(InvokeResponseBody::Raw(std::mem::take(&mut self.pending)));
    }
    fn dirty(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[cfg(feature = "bench-canvas")]
struct Grid<'a> {
    channel: &'a Channel<InvokeResponseBody>,
    pane: wings_bench_grid::GridPane,
    visible: bool,
    dirty: bool,
    stats: &'a mut ReplayStats,
}

#[cfg(feature = "bench-canvas")]
impl Sink for Grid<'_> {
    fn feed(&mut self, bytes: &[u8]) {
        self.pane.feed(bytes);
        self.dirty = true;
    }
    fn flush(&mut self, full: bool) {
        self.dirty = false;
        let mut frame = Vec::new();
        if self.visible && self.pane.frame(full, &mut frame) {
            self.stats.bytes += frame.len() as u64;
            self.stats.messages += 1;
            let _ = self.channel.send(InvokeResponseBody::Raw(frame));
        }
    }
    fn dirty(&self) -> bool {
        self.dirty
    }
}

/// Plays one pane's part of the recording into `sink`. Raw bytes go out as they were read (paced) or in reads of up
/// to 64 KB (drain); grid rows go out at most once per frame.
fn play(rec: &Recording, sink: &mut dyn Sink, grid: bool, from_ms: u32, play_ms: Option<u32>) {
    let (before, after): (Vec<_>, Vec<_>) = rec.chunks.iter().partition(|c| c.0 < from_ms);
    for (_, bytes) in &before {
        sink.feed(bytes);
    }
    sink.flush(true);
    let start = Instant::now();
    let mut last_frame = Instant::now();
    let Some(play_ms) = play_ms else {
        for (_, bytes) in &after {
            sink.feed(bytes);
            if grid && last_frame.elapsed() >= FRAME {
                sink.flush(false);
                last_frame = Instant::now();
            }
        }
        sink.flush(false);
        return;
    };
    for (t, bytes) in after.iter().take_while(|c| c.0 < from_ms + play_ms) {
        let due = start + Duration::from_millis((t - from_ms) as u64);
        // A grid pane sends its rows a frame after they changed, even while no output arrives.
        while grid && sink.dirty() && last_frame + FRAME < due {
            thread::sleep((last_frame + FRAME).saturating_duration_since(Instant::now()));
            sink.flush(false);
            last_frame = Instant::now();
        }
        thread::sleep(due.saturating_duration_since(Instant::now()));
        sink.feed(bytes);
        if !grid || last_frame.elapsed() >= FRAME {
            sink.flush(false);
            last_frame = Instant::now();
        }
    }
    sink.flush(false);
    thread::sleep((start + Duration::from_millis(play_ms as u64)).saturating_duration_since(Instant::now()));
}

/// Replays the recording into every pane at once, one thread per pane like `pty.rs`. Each pane's channel gets an
/// empty message when its part is done.
#[tauri::command]
pub async fn bench_replay(webview: Webview, channels: Vec<JavaScriptChannelId>, plan: ReplayPlan) -> Result<ReplayStats, String> {
    if plan.grid && !cfg!(feature = "bench-canvas") {
        return Err("the canvas renderer needs a build with --features bench-canvas".into());
    }
    let rec = std::sync::Arc::new(recording()?);
    let started = Instant::now();
    let threads: Vec<_> = channels
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            let channel: Channel<InvokeResponseBody> = id.channel_on(webview.clone());
            let rec = rec.clone();
            let (visible, from_ms, play_ms, grid) = (plan.visible[i], plan.from_ms[i], plan.play_ms, plan.grid);
            thread::spawn(move || {
                let mut stats = ReplayStats::default();
                if grid {
                    #[cfg(feature = "bench-canvas")]
                    {
                        let pane = wings_bench_grid::GridPane::new(rec.cols, rec.rows);
                        play(&rec, &mut Grid { channel: &channel, pane, visible, dirty: false, stats: &mut stats }, true, from_ms, play_ms);
                    }
                } else {
                    let _ = visible;
                    play(&rec, &mut Raw { channel: &channel, pending: Vec::new(), stats: &mut stats }, false, from_ms, play_ms);
                }
                let _ = channel.send(InvokeResponseBody::Raw(Vec::new()));
                stats
            })
        })
        .collect();
    let mut total = ReplayStats::default();
    for t in threads {
        let s = t.join().map_err(|_| "replay thread panicked")?;
        total.bytes += s.bytes;
        total.messages += s.messages;
    }
    total.ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(total)
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSample {
    pid: u32,
    cpu_ms: u64,
    /// From `vmmap --summary`, in MB.
    footprint_mb: Option<f64>,
    /// "owned unmapped (graphics)": resident and swapped, the IOSurfaces WebKit draws layers and canvases into.
    owned_graphics_mb: Option<f64>,
    owned_graphics_swapped_mb: Option<f64>,
    owned_graphics_regions: Option<u32>,
    /// Every region type tagged "(graphics)", resident plus swapped.
    all_graphics_mb: Option<f64>,
}

fn mb(size: &str) -> Option<f64> {
    let (num, unit) = size.split_at(size.len().checked_sub(1)?);
    let n: f64 = num.parse().ok()?;
    Some(match unit {
        "K" => n / 1024.0,
        "M" => n,
        "G" => n * 1024.0,
        "B" => n / 1024.0 / 1024.0,
        _ => return None,
    })
}

fn vmmap(sample: &mut ProcessSample) {
    let Ok(out) = Command::new("/usr/bin/vmmap").args(["--summary", &sample.pid.to_string()]).output() else { return };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut all = 0.0;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Physical footprint:") {
            sample.footprint_mb = mb(rest.trim());
        }
        let Some(at) = line.find("(graphics)") else { continue };
        // Columns after the name: virtual, resident, dirty, swapped, volatile, nonvolatile, empty, region count.
        let cols: Vec<&str> = line[at + "(graphics)".len()..].split_whitespace().collect();
        let (Some(resident), Some(swapped)) = (cols.get(1).and_then(|s| mb(s)), cols.get(3).and_then(|s| mb(s))) else { continue };
        all += resident + swapped;
        if line.starts_with("owned unmapped (graphics)") {
            sample.owned_graphics_mb = Some(resident);
            sample.owned_graphics_swapped_mb = Some(swapped);
            sample.owned_graphics_regions = cols.get(7).and_then(|s| s.parse().ok());
        }
    }
    sample.all_graphics_mb = Some(all);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    wings: ProcessSample,
    web_content: Option<ProcessSample>,
    gpu: Option<ProcessSample>,
}

/// CPU time of Wings and the WebKit WebContent and GPU processes it started, and with `memory` their footprints.
/// WebKit's processes are launchd's children, so they're found as the first of each kind that started after Wings.
#[tauri::command]
pub async fn bench_sample(memory: bool) -> Sample {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let own = Pid::from_u32(std::process::id());
    let since = system.process(own).map_or(0, |p| p.start_time());
    let take = |pid: Pid| {
        let p = system.process(pid)?;
        Some(ProcessSample { pid: pid.as_u32(), cpu_ms: p.accumulated_cpu_time(), ..ProcessSample::default() })
    };
    let first = |prefix: &str| {
        system
            .processes()
            .values()
            .filter(|p| p.name().to_string_lossy().starts_with(prefix) && p.start_time() >= since)
            .min_by_key(|p| p.start_time())
            .map(|p| p.pid())
    };
    let mut wings = take(own).unwrap_or_default();
    let mut web_content = first("com.apple.WebKit.WebContent").and_then(take);
    let mut gpu = first("com.apple.WebKit.GPU").and_then(take);
    if memory {
        vmmap(&mut wings);
        web_content.iter_mut().chain(gpu.iter_mut()).for_each(vmmap);
    }
    Sample { wings, web_content, gpu }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn recorder() -> (Channel<InvokeResponseBody>, Arc<Mutex<Vec<Vec<u8>>>>) {
        let got = Arc::new(Mutex::new(Vec::new()));
        let sink = got.clone();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                sink.lock().unwrap().push(bytes);
            }
            Ok(())
        });
        (channel, got)
    }

    fn rec() -> Recording {
        Recording { cols: 20, rows: 4, chunks: vec![(0, b"a".to_vec()), (5, b"b".to_vec()), (30, b"c".to_vec()), (60, b"d".to_vec())] }
    }

    #[test]
    fn paced_raw_replay_sends_the_fast_forward_at_once_then_each_read() {
        let (channel, got) = recorder();
        let mut stats = ReplayStats::default();
        play(&rec(), &mut Raw { channel: &channel, pending: Vec::new(), stats: &mut stats }, false, 10, Some(40));
        assert_eq!(*got.lock().unwrap(), vec![b"ab".to_vec(), b"c".to_vec()]);
        assert_eq!((stats.bytes, stats.messages), (3, 2));
    }

    #[test]
    fn drained_raw_replay_coalesces_reads() {
        let (channel, got) = recorder();
        let mut stats = ReplayStats::default();
        play(&rec(), &mut Raw { channel: &channel, pending: Vec::new(), stats: &mut stats }, false, 0, None);
        assert_eq!(*got.lock().unwrap(), vec![b"abcd".to_vec()]);
    }

    #[cfg(feature = "bench-canvas")]
    #[test]
    fn hidden_grid_panes_send_nothing() {
        for visible in [false, true] {
            let (channel, got) = recorder();
            let mut stats = ReplayStats::default();
            let pane = wings_bench_grid::GridPane::new(20, 4);
            play(&rec(), &mut Grid { channel: &channel, pane, visible, dirty: false, stats: &mut stats }, true, 0, None);
            assert_eq!(got.lock().unwrap().is_empty(), !visible);
        }
    }

    #[test]
    fn samples_its_own_footprint_with_vmmap() {
        let s = tauri::async_runtime::block_on(bench_sample(true));
        assert!(s.wings.footprint_mb.is_some_and(|mb| mb > 1.0), "no footprint");
        assert_eq!(mb("273.6M"), Some(273.6));
        assert_eq!(mb("1.5G"), Some(1536.0));
    }
}
