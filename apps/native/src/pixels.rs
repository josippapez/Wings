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

/// Which two colours a pattern's warm and cool shades take (`ui::backdrop` names them in the page's palette): the
/// Claude orange and the info blue, or other pairs of the palette's, or the text's own colour for black and white.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    /// The Claude orange and the info blue.
    Claude,
    /// The text colour for both: light on a dark ground, dark on a light one.
    Mono,
    /// The info blue and the done green.
    Ocean,
    /// The done green and the working amber.
    Meadow,
    /// The danger coral and a lavender between it and the info blue.
    Dusk,
    /// A ramp of six colours from deep blue through violet and pink to orange, which a pattern of `ramp` steps uses
    /// in place of the shades' usual roles (`RAMP`).
    Aurora,
    /// The same kind of ramp from teal through blue to violet.
    Lagoon,
}

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
    /// `mono`: the drift in black and white, light cells on a dark ground and the other way round on a light one.
    Mono,
    /// `ocean`: the drift in blues and sea greens.
    Ocean,
    /// `meadow`: the drift in greens and amber.
    Meadow,
    /// `dusk`: the drift in coral and lavender.
    Dusk,
    /// `blocks`: the drift in big square blocks.
    Blocks,
    /// `bayer`: slow waves of light and dark cut by an ordered dither, so a soft ramp is a fine screen of cells.
    Bayer,
    /// `grain`: a slow colour gradient from deep blue to orange in big pixels, with an ordered dither where one band
    /// meets the next, so it reads as a pixelated, grainy gradient. It covers the whole ground, with no cell left plain.
    Grain,
    /// `grain-lagoon`: the same from teal to violet.
    GrainLagoon,
}

impl Pattern {
    pub const ALL: [Pattern; 12] = [
        Pattern::Drift,
        Pattern::Rain,
        Pattern::Checker,
        Pattern::Embers,
        Pattern::Mono,
        Pattern::Ocean,
        Pattern::Meadow,
        Pattern::Dusk,
        Pattern::Blocks,
        Pattern::Bayer,
        Pattern::Grain,
        Pattern::GrainLagoon,
    ];

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
            Pattern::Mono => "mono",
            Pattern::Ocean => "ocean",
            Pattern::Meadow => "meadow",
            Pattern::Dusk => "dusk",
            Pattern::Blocks => "blocks",
            Pattern::Bayer => "bayer",
            Pattern::Grain => "grain",
            Pattern::GrainLagoon => "grain-lagoon",
        }
    }

    /// The colours of its warm and cool shades.
    pub fn tone(self) -> Tone {
        match self {
            Pattern::Mono | Pattern::Bayer => Tone::Mono,
            Pattern::Ocean => Tone::Ocean,
            Pattern::Grain => Tone::Aurora,
            Pattern::GrainLagoon => Tone::Lagoon,
            Pattern::Meadow => Tone::Meadow,
            Pattern::Dusk => Tone::Dusk,
            Pattern::Drift | Pattern::Rain | Pattern::Checker | Pattern::Embers | Pattern::Blocks => Tone::Claude,
        }
    }

    /// How wide and tall a cell is, in points.
    pub fn cell(self) -> f32 {
        match self {
            Pattern::Drift | Pattern::Mono | Pattern::Ocean | Pattern::Meadow | Pattern::Dusk => 12.0,
            Pattern::Blocks => 30.0,
            Pattern::Bayer => 8.0,
            Pattern::Grain | Pattern::GrainLagoon => 12.0,
            Pattern::Rain | Pattern::Embers => 10.0,
            Pattern::Checker => 24.0,
        }
    }

    /// The shade of the cell `col` across and `row` down from the slot's top left corner, `t` seconds in.
    pub fn shade(self, col: u32, row: u32, t: f32) -> u8 {
        match self {
            Pattern::Drift | Pattern::Mono | Pattern::Ocean | Pattern::Meadow | Pattern::Dusk | Pattern::Blocks => drift(col, row, t),
            Pattern::Bayer => bayer(col, row, t),
            Pattern::Grain | Pattern::GrainLagoon => grain(col, row, t),
            Pattern::Rain => rain(col, row, t),
            Pattern::Checker => checker(col, row, t),
            Pattern::Embers => embers(col, row, t),
        }
    }
}

impl Pattern {
    /// The shades of a whole grid, `cols` by `rows`, row by row, `t` seconds in: what `shade` gives for each cell, with what
    /// a column, a row or a diagonal of cells shares worked out once, not for each cell (a sine for every cell was most of
    /// a step's cost).
    pub fn shades(self, cols: u32, rows: u32, t: f32) -> Vec<u8> {
        let mut out = Vec::with_capacity(cols as usize * rows as usize);
        match self {
            Pattern::Drift | Pattern::Mono | Pattern::Ocean | Pattern::Meadow | Pattern::Dusk | Pattern::Blocks | Pattern::Bayer => {
                let dithered = self == Pattern::Bayer;
                let by_col: Vec<f32> = (0..cols).map(|col| (col as f32 * 0.067 + t * 0.07).sin()).collect();
                let by_row: Vec<f32> = (0..rows).map(|row| (row as f32 * 0.089 - t * 0.05).sin()).collect();
                // Indexed by `col + rows - row`.
                let by_diagonal: Vec<f32> =
                    (0..cols + rows).map(|k| ((k as f32 - rows as f32) * 0.047 + t * 0.04).sin()).collect();
                let (cx, cy) = (34.0 + 22.0 * (t * 0.029).sin(), 22.0 + 14.0 * (t * 0.037).cos());
                for row in 0..rows {
                    let y = row as f32;
                    for col in 0..cols {
                        let x = col as f32;
                        let waves = by_col[col as usize]
                            + by_row[row as usize]
                            + by_diagonal[(col + rows - row) as usize]
                            + ((x - cx).hypot(y - cy) * 0.16 - t * 0.12).sin();
                        out.push(if dithered { bayer_band(col, row, waves) } else { drift_band(col, row, x, y, t, waves) });
                    }
                }
            }
            Pattern::Grain | Pattern::GrainLagoon => {
                let across: Vec<[Angle; 2]> = (0..cols).map(|col| grain_across(col, t)).collect();
                for row in 0..rows {
                    let down = grain_down(row);
                    for col in 0..cols {
                        out.push(grain_step(col, row, across[col as usize], down));
                    }
                }
            }
            Pattern::Rain => {
                let heads: Vec<(f32, f32)> = (0..cols).map(|col| drop_head(col, t, (1.5, 4.0), (36, 72))).collect();
                for row in 0..rows {
                    for &(head, period) in &heads {
                        out.push(rain_shade((head - row as f32).rem_euclid(period) as u32));
                    }
                }
            }
            Pattern::Checker => out.extend((0..rows).flat_map(|row| (0..cols).map(move |col| checker(col, row, t)))),
            Pattern::Embers => {
                let heads: Vec<(f32, f32)> = (0..cols).map(|col| drop_head(col, t, (0.8, 2.4), (40, 90))).collect();
                let by_col: Vec<f32> = (0..cols).map(|col| (col as f32 * 0.05 + t * 0.03).sin()).collect();
                for row in 0..rows {
                    let by_row = (row as f32 * 0.07 + t * 0.11).sin();
                    for col in 0..cols {
                        let (head, period) = heads[col as usize];
                        let behind = (head - -(row as f32)).rem_euclid(period) as u32;
                        out.push(ember_shade(behind, by_col[col as usize] * by_row + dither(col, row) * 0.1));
                    }
                }
            }
        }
        out
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
    drift_band(col, row, x, y, t, waves)
}

/// The shades of a ramp's six steps, deepest to brightest: a `Tone::Aurora` or `Tone::Lagoon` gives each its own
/// colour, and none is the ground's.
pub const RAMP: [u8; 6] = [DARK, DIM, WARM_FAINT, WARM, COOL_FAINT, COOL];

/// A slow colour gradient across the window, a broad diagonal wave bent by a second: cut into six steps with the ordered
/// dither deciding which side of a step each cell falls on, so the bands meet in a screen of cells.
fn grain(col: u32, row: u32, t: f32) -> u8 {
    grain_step(col, row, grain_across(col, t), grain_down(row))
}

/// What a column and a row each give the two waves, as the sine and cosine of their angles, so a cell's wave is
/// sin(a + b) = sin a cos b + cos a sin b without a sine for each cell.
type Angle = (f32, f32);

fn grain_across(col: u32, t: f32) -> [Angle; 2] {
    let angle = |v: f32| (v.sin(), v.cos());
    [angle(col as f32 * 0.021 + t * 0.06 - 1.9), angle(col as f32 * 0.03 - t * 0.04)]
}

fn grain_down(row: u32) -> [Angle; 2] {
    let angle = |v: f32| (v.sin(), v.cos());
    [angle(row as f32 * 0.03), angle(row as f32 * 0.02)]
}

fn grain_step(col: u32, row: u32, across: [Angle; 2], down: [Angle; 2]) -> u8 {
    let diagonal = across[0].0 * down[0].1 + across[0].1 * down[0].0;
    let bend = across[1].0 * down[1].1 - across[1].1 * down[1].0;
    let level = (0.5 + diagonal * 0.46 + bend * 0.1) * 5.0 + dither(col, row);
    RAMP[level.round().clamp(0.0, 5.0) as usize]
}

/// An ordered dither of the same waves: the sum is cut into dark, ground and light by a threshold that moves with each
/// cell's place in the 4x4 dither, so a soft rise from dark to light is a screen of cells that thickens, not a band.
fn bayer(col: u32, row: u32, t: f32) -> u8 {
    let (x, y) = (col as f32, row as f32);
    let (cx, cy) = (34.0 + 22.0 * (t * 0.029).sin(), 22.0 + 14.0 * (t * 0.037).cos());
    let waves = (x * 0.067 + t * 0.07).sin()
        + (y * 0.089 - t * 0.05).sin()
        + ((x - y) * 0.047 + t * 0.04).sin()
        + ((x - cx).hypot(y - cy) * 0.16 - t * 0.12).sin();
    bayer_band(col, row, waves)
}

/// The shade of a dithered cell from its sum of waves: the dark shade below a third of the way up the ramp, the light
/// one above two thirds, the ground between, each cell's edge moved by its place in the dither.
fn bayer_band(col: u32, row: u32, waves: f32) -> u8 {
    let level = waves / 8.0 + 0.5 + dither(col, row) * 0.3;
    if level < 0.4 {
        DIM
    } else if level > 0.6 {
        WARM_FAINT
    } else {
        BASE
    }
}

/// The shade of a drift cell from its sum of waves.
fn drift_band(col: u32, row: u32, x: f32, y: f32, t: f32, waves: f32) -> u8 {
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
    let (head, period) = drop_head(col, t, speeds, periods);
    (head - row).rem_euclid(period) as u32
}

/// Where a column's drop is along its travel `t` seconds in, and how many cells it runs before the next one.
fn drop_head(col: u32, t: f32, speeds: (f32, f32), periods: (u32, u32)) -> (f32, f32) {
    let hash = mix(col);
    let speed = speeds.0 + (hash & 0x3ff) as f32 / 1023.0 * (speeds.1 - speeds.0);
    let period = (periods.0 + (hash >> 10) % (periods.1 - periods.0)) as f32;
    let start = ((hash >> 20) % 1024) as f32;
    ((t * speed + start).rem_euclid(period), period)
}

/// Drops of the blue falling 1.5 to 4 cells a second down each column, one every 36 to 72 cells, each with a short
/// fading trail.
fn rain(col: u32, row: u32, t: f32) -> u8 {
    rain_shade(behind(col, row as f32, t, (1.5, 4.0), (36, 72)))
}

fn rain_shade(behind: u32) -> u8 {
    match behind {
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
    let (x, y) = (col as f32, row as f32);
    let glow = (x * 0.05 + t * 0.03).sin() * (y * 0.07 + t * 0.11).sin() + dither(col, row) * 0.1;
    ember_shade(behind(col, -(row as f32), t, (0.8, 2.4), (40, 90)), glow)
}

/// The shade of an embers cell: a spark if it is within a spark's reach (`behind`), else the glow's.
fn ember_shade(behind: u32, glow: f32) -> u8 {
    match behind {
        0 => return WARM,
        1..=2 => return WARM_FAINT,
        _ => {}
    }
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
    fn a_grid_is_what_each_cell_alone_gives() {
        for pattern in Pattern::ALL {
            for (cols, rows, t) in [(100, 80, 12.5), (7, 3, 0.0), (213, 129, 301.2)] {
                let each: Vec<u8> = (0..rows).flat_map(|row| (0..cols).map(move |col| pattern.shade(col, row, t))).collect();
                assert_eq!(pattern.shades(cols, rows, t), each, "{pattern:?} {cols}x{rows} at {t}");
            }
        }
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
            let covers = matches!(pattern, Pattern::Grain | Pattern::GrainLagoon);
            assert!(off * 100 >= cells.len() * 3 && (covers || off * 5 <= cells.len() * 4), "{pattern:?}: {off} of {} off the ground", cells.len());
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
                    std::hint::black_box(pattern.shades(cols, rows, t));
                }
                let ms = start.elapsed().as_secs_f64() * 1000.0 / 15.0;
                eprintln!("[measure] {pattern:?} {width}x{height}: {} cells, {ms:.3} ms a step", cols * rows);
            }
        }
    }

    #[test]
    fn each_new_variant_looks_unlike_the_others_by_its_colours_its_cells_or_its_dither() {
        // What tells two patterns apart: the colours of their shades, the size of their cells, or which cells they pick.
        let looks: Vec<(Tone, u32, Vec<u8>)> =
            Pattern::ALL.iter().map(|pattern| (pattern.tone(), pattern.cell() as u32, grid(*pattern, 12.5))).collect();
        for (at, a) in looks.iter().enumerate() {
            for (other, b) in looks.iter().enumerate().skip(at + 1) {
                assert!(a != b, "{:?} and {:?} look the same", Pattern::ALL[at], Pattern::ALL[other]);
            }
        }
        assert!(Pattern::Blocks.cell() >= 2.0 * Pattern::Drift.cell(), "blocks are big");
        assert!(Pattern::Bayer.cell() < Pattern::Drift.cell(), "a dither is fine");
        // The ordered dither has no dither-free bands: its three shades meet in screens, so beside a cell of one shade
        // there are many cells of another.
        let cells = grid(Pattern::Bayer, 12.5);
        let edges = cells.windows(2).filter(|pair| pair[0] != pair[1]).count();
        let drift = grid(Pattern::Drift, 12.5);
        let drift_edges = drift.windows(2).filter(|pair| pair[0] != pair[1]).count();
        assert!(edges > drift_edges * 3 / 2, "bayer {edges} edges against the drift's {drift_edges}");
        let seen: HashSet<u8> = cells.iter().copied().collect();
        assert_eq!(seen, HashSet::from([DIM, BASE, WARM_FAINT]));
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
