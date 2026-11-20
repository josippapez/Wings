//! The gradient backdrops a page can ask for behind it (`backdrop: "gradient:<preset>"` on its `Page`): slow moving
//! meshes of three colours, which a Metal shader of Wings' own draws (`ui/gradient.metal`). Here are the presets, the
//! values each frame hands the shader, and the bounds that keep the page's muted text readable on every pixel of it.
//!
//! The presets take their colours, and the kind of mesh each is drawn on, from shadergradient's presets
//! (https://github.com/ruucm/shadergradient, `packages/shadergradient/src/presets.ts`, MIT © ruucm and stone-skipper).
//! None of its code is used: its look is a three.js mesh displaced by noise and lit, and the shader here recreates that
//! look on a flat picture.

use crate::theme::{self, Palette};

/// In a preset's colours: the page's own text colour, so a line is light on a dark page and dark on a light one.
const FOREGROUND_INK: u32 = u32::MAX;

/// A gradient backdrop, by the name a page asks for it with after `gradient:`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    Halo,
    Pensive,
    Mint,
    Interstella,
    NightyNight,
    Topo,
    Flow,
    Grid,
    Dots,
    Silk,
    Cells,
    Moire,
}

/// The kind of mesh a preset is drawn on, as shadergradient's `type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// `plane`: broad soft folds lit from the top left, one colour across, the third on their crests.
    Plane,
    /// `sphere`: a round bloom that drifts and twists, two colours top to bottom inside, the third on its rim.
    Sphere,
    /// `waterPlane`: finer ripples crossing each other, the third colour on their crests.
    Water,
    /// Contour lines of a drifting hill field.
    Topo,
    /// Parallel lines that bend with a drifting field.
    Flow,
    /// A floor of lines in perspective with a pulse of light travelling along it.
    Grid,
    /// A matrix of dots that swell and shrink.
    Dots,
    /// Warped bands of two colours with a bright line on each crest.
    Silk,
    /// Voronoi cells with thin walls.
    Cells,
    /// Two sets of lines turning against each other.
    Moire,
}

/// What a preset looks like: its colours, mesh and how it moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub name: &'static str,
    pub label: &'static str,
    pub shape: Shape,
    /// shadergradient's `color1`, `color2` and `color3`, as 0xRRGGBB.
    pub colors: [u32; 3],
    /// How fast it moves, shadergradient's `uSpeed`.
    pub speed: f32,
    /// How close together its folds or ripples are, shadergradient's `uDensity`.
    pub density: f32,
    /// How much of the third colour the crests take, 0 to 1 (from shadergradient's `uStrength`).
    pub strength: f32,
    /// How far a sphere twists, 0 to 1 (from shadergradient's `uAmplitude` over 7, its strongest preset's); how much a
    /// topography's field or a flow's lines swing, for those.
    pub twist: f32,
    /// Film grain, shadergradient's `grain`.
    pub grain: bool,
}

impl Preset {
    pub const ALL: [Preset; 12] = [
        Preset::Halo,
        Preset::Pensive,
        Preset::Mint,
        Preset::Interstella,
        Preset::NightyNight,
        Preset::Topo,
        Preset::Flow,
        Preset::Grid,
        Preset::Dots,
        Preset::Silk,
        Preset::Cells,
        Preset::Moire,
    ];

    /// The preset a page names, as in `"gradient:halo"` less its `gradient:`.
    pub fn named(name: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|preset| preset.look().name == name)
    }

    pub fn look(self) -> Look {
        match self {
            Preset::Halo => Look {
                name: "halo",
                label: "Halo",
                shape: Shape::Plane,
                colors: [0xff5005, 0xdbba95, 0xd0bce1],
                speed: 0.4,
                density: 1.3,
                strength: 0.8,
                twist: 0.0,
                grain: true,
            },
            Preset::Pensive => Look {
                name: "pensive",
                label: "Pensive",
                shape: Shape::Sphere,
                colors: [0x809bd6, 0x910aff, 0xaf38ff],
                speed: 0.3,
                density: 0.8,
                strength: 0.5,
                twist: 1.0,
                grain: true,
            },
            Preset::Mint => Look {
                name: "mint",
                label: "Mint",
                shape: Shape::Water,
                colors: [0x94ffd1, 0x6bf5ff, 0xffffff],
                speed: 0.2,
                density: 1.2,
                strength: 0.85,
                twist: 0.0,
                grain: false,
            },
            Preset::Interstella => Look {
                name: "interstella",
                label: "Interstella",
                shape: Shape::Sphere,
                colors: [0x73bfc4, 0xff810a, 0x8da0ce],
                speed: 0.3,
                density: 0.8,
                strength: 0.45,
                twist: 0.46,
                grain: true,
            },
            Preset::NightyNight => Look {
                name: "nighty-night",
                label: "Nighty night",
                shape: Shape::Water,
                colors: [0x606080, 0x8d7dca, 0x212121],
                speed: 0.3,
                density: 1.5,
                strength: 0.6,
                twist: 0.0,
                grain: true,
            },
            Preset::Topo => Look {
                name: "topo",
                label: "Topography",
                shape: Shape::Topo,
                colors: [theme::INFO, theme::CLAUDE, FOREGROUND_INK],
                speed: 0.5,
                density: 1.0,
                strength: 0.9,
                twist: 1.0,
                grain: false,
            },
            Preset::Flow => Look {
                name: "flow",
                label: "Flow lines",
                shape: Shape::Flow,
                colors: [theme::INFO, theme::CLAUDE, FOREGROUND_INK],
                speed: 0.6,
                density: 1.0,
                strength: 0.9,
                twist: 1.0,
                grain: false,
            },
            Preset::Grid => Look {
                name: "grid",
                label: "Grid",
                shape: Shape::Grid,
                colors: [theme::INFO, theme::CLAUDE, FOREGROUND_INK],
                speed: 1.5,
                density: 1.0,
                strength: 0.9,
                twist: 0.0,
                grain: false,
            },
            Preset::Dots => Look {
                name: "dots",
                label: "Dot matrix",
                shape: Shape::Dots,
                colors: [theme::INFO, theme::CLAUDE, FOREGROUND_INK],
                speed: 0.8,
                density: 1.0,
                strength: 0.9,
                twist: 0.0,
                grain: false,
            },
            Preset::Silk => Look {
                name: "silk",
                label: "Silk",
                shape: Shape::Silk,
                colors: [theme::INFO, theme::CLAUDE, FOREGROUND_INK],
                speed: 0.8,
                density: 1.0,
                strength: 0.9,
                twist: 0.0,
                grain: false,
            },
            Preset::Cells => Look {
                name: "cells",
                label: "Cells",
                shape: Shape::Cells,
                colors: [theme::INFO, theme::CLAUDE, FOREGROUND_INK],
                speed: 0.8,
                density: 1.0,
                strength: 0.9,
                twist: 0.0,
                grain: false,
            },
            Preset::Moire => Look {
                name: "moire",
                label: "Moire",
                shape: Shape::Moire,
                colors: [theme::INFO, theme::CLAUDE, FOREGROUND_INK],
                speed: 0.8,
                density: 1.0,
                strength: 0.8,
                twist: 0.0,
                grain: false,
            },
        }
    }
}

/// The contrast muted text keeps on the brightest (or, on a light page, the darkest) pixel. WCAG's 4.5 for text, and a
/// margin for the 8-bit planes the frame is handed over in, their dither and the grain.
pub const CONTRAST: f32 = 4.8;

/// Shader time units a second at a `speed` of 1: slow enough to sit behind text.
const TIME_SCALE: f64 = 0.35;

/// How many shader time units the motion takes to come back to where it started, so it loops without a jump however
/// long Wings runs. The shader's slow waves turn a whole number of times in it (`gradient.metal`, `LOOP`).
pub const LOOP: f64 = 64.0;

/// How far each colour may move the page's ground towards it at most, before the contrast bound.
const STRENGTH: f32 = 0.6;

/// What a frame hands the shader, laid out as `Gradient` in `gradient.metal` (128 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Uniforms {
    /// The three colours as moved towards the ground, then the ground, as sRGB 0 to 1.
    pub inks: [[f32; 4]; 3],
    pub ground: [f32; 4],
    /// The frame's size in its pixels, and the size of the plane being drawn now (the chroma plane is half of it).
    pub size: [f32; 2],
    pub target: [f32; 2],
    /// Shader time, 0 up to `LOOP`.
    pub time: f32,
    /// 0 plane, 1 sphere, 2 water, 3 topo, 4 flow, 5 grid, 6 dots, 7 silk, 8 cells, 9 moire.
    pub shape: f32,
    pub density: f32,
    pub strength: f32,
    pub twist: f32,
    /// How strong the grain is, 0 without.
    pub grain: f32,
    /// How round the corners of what holds it are, in the frame's pixels.
    pub corner: f32,
    /// The relative luminance every pixel stays within, so muted text keeps `CONTRAST` on it.
    pub luma_max: f32,
    pub luma_min: f32,
    pub ground_luma: f32,
    pub _pad: [f32; 2],
}

/// WCAG's relative luminance of a 0xRRGGBB colour.
pub fn luminance(color: u32) -> f32 {
    luminance_of(rgb(color))
}

fn luminance_of([r, g, b]: [f32; 3]) -> f32 {
    let linear = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

/// WCAG's contrast between two relative luminances.
pub fn contrast(a: f32, b: f32) -> f32 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn rgb(color: u32) -> [f32; 3] {
    [(color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff].map(|c| c as f32 / 255.0)
}

/// The luminances a pixel may have, lowest to highest, for `muted` text to keep `CONTRAST` on it: on a dark page up to
/// some bound above none, on a light one down to one below full white.
pub fn luma_bounds(palette: &Palette) -> (f32, f32) {
    let (muted, ground) = (luminance(palette.muted_foreground), luminance(palette.surface));
    if muted >= ground {
        (0.0, (muted + 0.05) / CONTRAST - 0.05)
    } else {
        ((muted + 0.05) * CONTRAST - 0.05, 1.0)
    }
}

/// `color` moved from the page's ground towards it as far as `STRENGTH`, or less where that would leave the bounds.
fn ink(ground: [f32; 3], color: u32, (low, high): (f32, f32)) -> [f32; 3] {
    let color = rgb(color);
    let at = |s: f32| [0, 1, 2].map(|i| ground[i] + (color[i] - ground[i]) * s);
    let within = |s: f32| {
        let luma = luminance_of(at(s));
        luma >= low && luma <= high
    };
    if within(STRENGTH) {
        return at(STRENGTH);
    }
    // The ground itself is within on any palette whose plain page is readable; past it, nothing helps.
    let (mut inside, mut outside) = (0.0, STRENGTH);
    for _ in 0..20 {
        let mid = (inside + outside) / 2.0;
        if within(mid) { inside = mid } else { outside = mid }
    }
    at(inside)
}

/// The values for a frame of `preset` behind a page in `palette`, `seconds` into its motion, `size` pixels big, in what
/// holds it whose corners are `corner` pixels round.
pub fn uniforms(preset: Preset, palette: &Palette, seconds: f64, size: (u32, u32), corner: f32) -> Uniforms {
    let look = preset.look();
    let own = |color: u32| if color == FOREGROUND_INK { palette.foreground } else { color };
    let bounds = luma_bounds(palette);
    let ground = rgb(palette.surface);
    let four = |[r, g, b]: [f32; 3]| [r, g, b, 1.0];
    let size = [size.0 as f32, size.1 as f32];
    Uniforms {
        inks: look.colors.map(|color| four(ink(ground, own(color), bounds))),
        ground: four(ground),
        size,
        target: size,
        time: (seconds * f64::from(look.speed) * TIME_SCALE).rem_euclid(LOOP) as f32,
        shape: match look.shape {
            Shape::Plane => 0.0,
            Shape::Sphere => 1.0,
            Shape::Water => 2.0,
            Shape::Topo => 3.0,
            Shape::Flow => 4.0,
            Shape::Grid => 5.0,
            Shape::Dots => 6.0,
            Shape::Silk => 7.0,
            Shape::Cells => 8.0,
            Shape::Moire => 9.0,
        },
        density: look.density,
        strength: look.strength,
        twist: look.twist,
        grain: if look.grain { 1.0 } else { 0.0 },
        corner,
        luma_min: bounds.0,
        luma_max: bounds.1,
        ground_luma: luminance_of(ground),
        _pad: [0.0; 2],
    }
}

/// A few things made once and used in turn, as a frame's buffers are: the oldest is drawn into again once there are
/// `N`, so what was handed over most recently is never the one being drawn into.
pub struct Ring<T, const N: usize> {
    items: Vec<T>,
    next: usize,
}

impl<T, const N: usize> Default for Ring<T, N> {
    fn default() -> Self {
        Self { items: Vec::with_capacity(N), next: 0 }
    }
}

impl<T, const N: usize> Ring<T, N> {
    /// The next one to use: a new one from `make` until there are `N`, then the oldest again.
    pub fn advance<E>(&mut self, make: impl FnOnce() -> Result<T, E>) -> Result<&mut T, E> {
        let at = self.next;
        if self.items.len() <= at {
            self.items.push(make()?);
        }
        self.next = (at + 1) % N;
        Ok(&mut self.items[at])
    }

    /// The ones made, the most recently used first.
    pub fn newest_first(&self) -> impl Iterator<Item = &T> {
        let newest = (self.next + N - 1) % N;
        (0..N).map(move |back| (newest + N - back) % N).filter_map(|at| self.items.get(at))
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_names_a_preset_by_its_name_and_any_other_name_is_none() {
        for preset in Preset::ALL {
            assert_eq!(Preset::named(preset.look().name), Some(preset));
        }
        assert_eq!(Preset::named("nighty-night"), Some(Preset::NightyNight));
        assert_eq!(Preset::named("Halo"), None);
        assert_eq!(Preset::named(""), None);
        assert_eq!(Preset::named("aurora"), None);
    }

    #[test]
    fn the_uniforms_are_laid_out_as_the_shader_reads_them() {
        assert_eq!(std::mem::size_of::<Uniforms>(), 128);
        let u = uniforms(Preset::Halo, &Palette::APP, 0.0, (500, 380), 6.5);
        let base = &u as *const Uniforms as usize;
        let offset = |field: &f32| field as *const f32 as usize - base;
        assert_eq!(offset(&u.size[0]), 64);
        assert_eq!(offset(&u.time), 80);
        assert_eq!(offset(&u.corner), 104);
        assert_eq!(offset(&u.ground_luma), 116);
    }

    #[test]
    fn a_frame_is_the_same_each_time_it_is_asked_and_moves_on_with_time() {
        let at = |seconds| uniforms(Preset::Pensive, &Palette::APP, seconds, (500, 380), 6.5);
        assert_eq!(at(12.5), at(12.5));
        assert_ne!(at(12.5).time, at(12.5 + 1.0 / 15.0).time);
        // Hours in, the time is still within the loop, so the shader's noise keeps its precision.
        let late = at(36.0 * 3600.0).time;
        assert!((0.0..LOOP as f32).contains(&late), "{late}");
        // Each preset's mesh.
        assert_eq!(uniforms(Preset::Halo, &Palette::APP, 0.0, (2, 2), 0.0).shape, 0.0);
        assert_eq!(uniforms(Preset::Interstella, &Palette::APP, 0.0, (2, 2), 0.0).shape, 1.0);
        assert_eq!(uniforms(Preset::Mint, &Palette::APP, 0.0, (2, 2), 0.0).shape, 2.0);
        assert_eq!(uniforms(Preset::Mint, &Palette::APP, 0.0, (2, 2), 0.0).grain, 0.0);
        for (at, preset) in Preset::ALL[5..].iter().enumerate() {
            assert_eq!(uniforms(*preset, &Palette::APP, 0.0, (2, 2), 0.0).shape, at as f32 + 3.0);
        }
    }

    /// The colours of each preset as moved towards the ground, and the bound the shader holds every pixel to: muted text
    /// keeps at least `CONTRAST` on all of them, on the app's dark page and on a light one.
    #[test]
    fn muted_text_stays_readable_on_every_colour_and_the_bound_the_shader_holds_pixels_to() {
        let light = Palette { surface: 0xf7f5f2, muted_foreground: 0x6b665f, ..Palette::APP };
        for palette in [Palette::APP, light] {
            let muted = luminance(palette.muted_foreground);
            let (low, high) = luma_bounds(&palette);
            assert!(contrast(muted, high) >= CONTRAST - 0.001 || high >= 1.0);
            assert!(contrast(muted, low) >= CONTRAST - 0.001 || low <= 0.0);
            assert!(CONTRAST > 4.5);
            for preset in Preset::ALL {
                let u = uniforms(preset, &palette, 0.0, (2, 2), 0.0);
                for ink in u.inks {
                    let luma = luminance_of([ink[0], ink[1], ink[2]]);
                    assert!(luma <= high + 1e-4 && luma >= low - 1e-4, "{preset:?} {ink:?} at {luma}");
                    assert!(contrast(muted, luma) >= CONTRAST - 0.01, "{preset:?}: {}", contrast(muted, luma));
                }
                if palette == Palette::APP {
                    // Not the flat ground: each preset tints it.
                    let tint = |ink: &[f32; 4]| (0..3).map(|i| (ink[i] - u.ground[i]).abs()).sum::<f32>();
                    assert!(u.inks.iter().any(|ink| tint(ink) > 0.04), "{preset:?} {:?}", u.inks);
                    let worst = u.inks.iter().map(|ink| contrast(muted, luminance_of([ink[0], ink[1], ink[2]]))).fold(f32::MAX, f32::min);
                    eprintln!("[measure] {preset:?}: muted text on its brightest colour {worst:.2}:1, on any pixel at least {:.2}:1", contrast(muted, high));
                }
            }
        }
    }

    #[test]
    fn a_ring_makes_n_and_then_uses_the_oldest_again() {
        let mut ring: Ring<usize, 3> = Ring::default();
        let mut made = 0;
        for step in 0..100 {
            let item = *ring.advance(|| -> Result<usize, ()> {
                made += 1;
                Ok(made - 1)
            })
            .unwrap();
            assert_eq!(item, step % 3);
            assert_eq!(ring.newest_first().next().copied(), Some(step % 3));
        }
        assert_eq!((made, ring.len()), (3, 3), "no more than three made in 100 steps");
        assert_eq!(ring.newest_first().copied().collect::<Vec<_>>(), [0, 2, 1]);
        // One that can't be made is not counted, and the next try makes it.
        let mut ring: Ring<usize, 2> = Ring::default();
        assert!(ring.advance(|| Err::<usize, _>("no device")).is_err());
        assert_eq!(ring.len(), 0);
        assert_eq!(*ring.advance(|| Ok::<_, ()>(7)).unwrap(), 7);
    }
}
