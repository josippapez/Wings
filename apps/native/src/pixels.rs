//! The patterns of pixels a page can ask for behind it (`backdrop: "pixels"`, or `"pixels:<pattern>"` on its `Page`):
//! grids of square cells, each one of a few shades near the page's ground (`SHADES`), worked out from where it is and
//! when. `ui::backdrop` paints them, in the window or in a layer under it. Every pattern uses the same shades, which keep
//! close to the ground, so text on any of them reads as on the plain ground.

/// How many shades a cell can be: two darker than the ground, the ground itself (`BASE`), then the orange and the blue,
/// each faint and fainter.
pub const SHADES: usize = 7;

/// The window's darker ground, strong and light.
pub const DARK: u8 = 0;
pub const DIM: u8 = 1;
/// The ground's shade, which the window paints once under the cells and no cell paints again.
pub const BASE: u8 = 2;
/// The Claude orange, faint and less faint.
pub const WARM_FAINT: u8 = 3;
pub const WARM: u8 = 4;
/// The info blue, faint and less faint.
pub const COOL_FAINT: u8 = 5;
pub const COOL: u8 = 6;

/// The 4x4 ordered dither: where in a band's edge a cell falls, so two bands meet in a checker of cells, not a line.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Where the drift's waves change band, darkest to brightest, on their 0 to 1.
const EDGES: [f32; 4] = [0.30, 0.42, 0.58, 0.70];

/// A pattern of pixels, by the name a page asks for it with after `pixels:`. `"pixels"` alone is `Drift`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pattern {
    /// `drift`: slow waves cut into bands, warm or cool by a broader wave.
    Drift,
    /// `rain`: drops of the blue falling slowly down columns of small cells, each with a short trail.
    Rain,
    /// `checker`: a coarse checker a shade darker than the ground, with a faint band of light sweeping across it.
    Checker,
    /// `embers`: sparks of the Claude orange rising slowly, over a dim warm glow that breathes.
    Embers,
}

impl Pattern {
    pub const ALL: [Pattern; 4] = [Pattern::Drift, Pattern::Rain, Pattern::Checker, Pattern::Embers];

    /// The pattern a page names, as in `"pixels:rain"` less its `pixels:`.
    pub fn named(name: &str) -> Option<Pattern> {
        Pattern::ALL.into_iter().find(|pattern| pattern.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            Pattern::Drift => "drift",
            Pattern::Rain => "rain",
            Pattern::Checker => "checker",
            Pattern::Embers => "embers",
        }
    }

    /// How wide and tall a cell is, in points.
    pub fn cell(self) -> f32 {
        match self {
            Pattern::Drift => 12.0,
            Pattern::Rain | Pattern::Embers => 10.0,
            Pattern::Checker => 24.0,
        }
    }

    /// The shade of the cell `col` across and `row` down from the slot's top left corner, `t` seconds in.
    pub fn shade(self, col: u32, row: u32, t: f32) -> u8 {
        match self {
            Pattern::Drift => drift(col, row, t),
            Pattern::Rain => rain(col, row, t),
            Pattern::Checker => checker(col, row, t),
            Pattern::Embers => embers(col, row, t),
        }
    }
}

/// Where in the 4x4 dither a cell falls, from -0.5 to 0.5.
fn dither(col: u32, row: u32) -> f32 {
    (f32::from(BAYER[(row % 4) as usize][(col % 4) as usize]) + 0.5) / 16.0 - 0.5
}

/// A column's own number, spread so neighbouring columns differ.
fn mix(value: u32) -> u32 {
    let mut x = value.wrapping_mul(0x9e37_79b1);
    x ^= x >> 15;
    x = x.wrapping_mul(0x85eb_ca77);
    x ^ (x >> 13)
}

/// A sum of slow waves cut into five bands, dithered at their edges. The two brightest bands are warm or cool by a
/// broader, slower wave.
fn drift(col: u32, row: u32, t: f32) -> u8 {
    let (x, y) = (col as f32, row as f32);
    // Rings out of a centre that wanders over the first 70 by 45 cells or so.
    let (cx, cy) = (34.0 + 22.0 * (t * 0.029).sin(), 22.0 + 14.0 * (t * 0.037).cos());
    let waves = (x * 0.067 + t * 0.07).sin()
        + (y * 0.089 - t * 0.05).sin()
        + ((x - y) * 0.047 + t * 0.04).sin()
        + ((x - cx).hypot(y - cy) * 0.16 - t * 0.12).sin();
    let value = waves / 8.0 + 0.5 + dither(col, row) * 0.06;
    let band = EDGES.iter().filter(|edge| value >= **edge).count() as u8;
    if band <= BASE {
        return band;
    }
    let warm = (x * 0.019 + y * 0.023 + t * 0.017).sin() > 0.0;
    if warm { band } else { band + 2 }
}

/// Where in its column a drop is: each column runs a drop every `period` cells, at its own speed and from its own start.
/// How many cells the cell `row` is behind the drop's head along the column's travel, 0 at the head.
fn behind(col: u32, row: f32, t: f32, speeds: (f32, f32), periods: (u32, u32)) -> u32 {
    let hash = mix(col);
    let speed = speeds.0 + (hash & 0x3ff) as f32 / 1023.0 * (speeds.1 - speeds.0);
    let period = (periods.0 + (hash >> 10) % (periods.1 - periods.0)) as f32;
    let start = ((hash >> 20) % 1024) as f32;
    let head = (t * speed + start).rem_euclid(period);
    (head - row).rem_euclid(period) as u32
}

/// Drops of the blue falling 1.5 to 4 cells a second down each column, one every 36 to 72 cells, each with a short
/// fading trail.
fn rain(col: u32, row: u32, t: f32) -> u8 {
    match behind(col, row as f32, t, (1.5, 4.0), (36, 72)) {
        0 => COOL,
        1..=2 => COOL_FAINT,
        3..=5 => DIM,
        _ => BASE,
    }
}

/// A coarse checker a shade darker than the ground, crossed by a broad diagonal band of light that sweeps by slowly and
/// lifts the cells it passes to a faint warm or cool, dithered at its edges.
fn checker(col: u32, row: u32, t: f32) -> u8 {
    let dark = (col + row) % 2 == 0;
    let along = col as f32 + row as f32 * 0.6;
    let band = (along * 0.17 - t * 0.55).sin() + dither(col, row) * 0.12;
    if band > 0.86 {
        return if dark { WARM_FAINT } else { COOL_FAINT };
    }
    if dark { DIM } else { BASE }
}

/// Sparks of the orange rising 0.8 to 2.4 cells a second up each column, one every 40 to 90 cells, with a short trail
/// below them, over a dim warm glow that breathes slowly.
fn embers(col: u32, row: u32, t: f32) -> u8 {
    // A rising spark: its head moves up the rows as time goes on, its trail below it.
    match behind(col, -(row as f32), t, (0.8, 2.4), (40, 90)) {
        0 => return WARM,
        1..=2 => return WARM_FAINT,
        _ => {}
    }
    let (x, y) = (col as f32, row as f32);
    let glow = (x * 0.05 + t * 0.03).sin() * (y * 0.07 + t * 0.11).sin() + dither(col, row) * 0.1;
    if glow > 0.62 {
        WARM_FAINT
    } else if glow < -0.7 {
        DIM
    } else {
        BASE
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// The step of the backdrops, 15 a second (`ui::backdrop`).
    const STEP: f32 = 1.0 / 15.0;

    /// The shades of a grid of cells `t` seconds in, row by row.
    fn grid(pattern: Pattern, t: f32) -> Vec<u8> {
        (0..80).flat_map(|row| (0..100).map(move |col| pattern.shade(col, row, t))).collect()
    }

    #[test]
    fn a_page_names_a_pattern_by_its_name_and_any_other_name_is_none() {
        for pattern in Pattern::ALL {
            assert_eq!(Pattern::named(pattern.name()), Some(pattern));
        }
        assert_eq!(Pattern::named("Rain"), None);
        assert_eq!(Pattern::named(""), None);
        assert_eq!(Pattern::named("snow"), None);
    }

    #[test]
    fn each_pattern_is_a_few_shades_the_same_each_time_it_is_asked_and_not_flat() {
        for pattern in Pattern::ALL {
            let cells = grid(pattern, 12.5);
            assert_eq!(cells, grid(pattern, 12.5), "{pattern:?}");
            assert!(cells.iter().all(|shade| usize::from(*shade) < SHADES), "{pattern:?}");
            let seen: HashSet<u8> = cells.iter().copied().collect();
            assert!(seen.len() >= 3, "{pattern:?} is a pattern, not a flat ground: {seen:?}");
            // Some cells off the ground, and some on it.
            let off = cells.iter().filter(|shade| **shade != BASE).count();
            assert!(off * 100 >= cells.len() * 3 && off * 5 <= cells.len() * 4, "{pattern:?}: {off} of {} off the ground", cells.len());
        }
    }

    #[test]
    fn a_step_of_any_pattern_changes_few_cells_and_some_time_changes_more() {
        for pattern in Pattern::ALL {
            let (now, next) = (grid(pattern, 30.0), grid(pattern, 30.0 + STEP));
            let moved = now.iter().zip(&next).filter(|(a, b)| a != b).count();
            assert!(moved * 10 < now.len(), "{pattern:?}: {moved} of {} cells changed in one step", now.len());
            let later = grid(pattern, 40.0);
            assert_ne!(now, later, "{pattern:?} moves");
        }
    }

    /// What working out a step of each pattern costs on the CPU, for a tab and for a full 2560 by 1440 window. Run with
    /// `--release` for a number to quote.
    #[test]
    fn measure_a_step_of_each_pattern() {
        for pattern in Pattern::ALL {
            for (width, height) in [(1000.0f32, 760.0f32), (2560.0, 1440.0)] {
                let (cols, rows) = ((width / pattern.cell()).ceil() as u32, (height / pattern.cell()).ceil() as u32);
                let start = std::time::Instant::now();
                for step in 0..15 {
                    let t = step as f32 * STEP;
                    let shades: Vec<u8> = (0..rows).flat_map(|row| (0..cols).map(move |col| pattern.shade(col, row, t))).collect();
                    std::hint::black_box(shades);
                }
                let ms = start.elapsed().as_secs_f64() * 1000.0 / 15.0;
                eprintln!("[measure] {pattern:?} {width}x{height}: {} cells, {ms:.3} ms a step", cols * rows);
            }
        }
    }

    #[test]
    fn rain_falls_and_embers_rise() {
        // The brightest cell of a column a few steps on is further down for rain and further up for embers.
        let head = |pattern: Pattern, bright: u8, col: u32, t: f32| (0..200).find(|row| pattern.shade(col, *row, t) == bright);
        let (mut down, mut up) = (0, 0);
        for col in 0..40 {
            if let (Some(a), Some(b)) = (head(Pattern::Rain, COOL, col, 10.0), head(Pattern::Rain, COOL, col, 10.5)) {
                down += i32::from(b > a) - i32::from(b < a);
            }
            if let (Some(a), Some(b)) = (head(Pattern::Embers, WARM, col, 10.0), head(Pattern::Embers, WARM, col, 11.0)) {
                up += i32::from(b < a) - i32::from(b > a);
            }
        }
        assert!(down > 0, "rain falls: {down}");
        assert!(up > 0, "embers rise: {up}");
    }
}
