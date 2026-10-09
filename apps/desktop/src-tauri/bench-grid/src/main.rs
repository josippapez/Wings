//! `wings-bench-grid <recording> <frames-out>`: runs a PTY recording through the grid encoder with one frame per
//! 16 ms of recording time, as the app would, and writes `u32 ms, u32 len, frame` records. The browser benchmark
//! replays that file, since Chromium has no Rust behind it. Prints how long parsing and encoding took.
use std::time::Instant;

use wings_bench_grid::{GridPane, Recording};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rec = Recording::parse(&std::fs::read(&args[1]).expect("read recording")).expect("parse recording");
    let started = Instant::now();
    let mut pane = GridPane::new(rec.cols, rec.rows);
    let (mut out, mut frame, mut frames) = (Vec::new(), Vec::new(), 0);
    let mut flush = |pane: &mut GridPane, t: u32, out: &mut Vec<u8>| {
        frame.clear();
        if pane.frame(false, &mut frame) {
            out.extend_from_slice(&t.to_le_bytes());
            out.extend_from_slice(&(frame.len() as u32).to_le_bytes());
            out.extend_from_slice(&frame);
            frames += 1;
        }
    };
    let mut next_frame = 0;
    for (t, bytes) in &rec.chunks {
        if *t >= next_frame {
            flush(&mut pane, *t, &mut out);
            next_frame = t + 16;
        }
        pane.feed(bytes);
    }
    flush(&mut pane, rec.chunks.last().map_or(0, |c| c.0), &mut out);
    let input: usize = rec.chunks.iter().map(|c| c.1.len()).sum();
    println!(
        "{} bytes in, {} frames, {} bytes out, parse+encode {:.1} ms",
        input,
        frames,
        out.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    std::fs::write(&args[2], out).expect("write frames");
}
