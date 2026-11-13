//! The disk map's geometry: which wedge each file and folder gets and its colour. Angles are in radians, clockwise from 12
//! o'clock. Ring 0 is the first ring around the centre.

use crate::entry::{Entry, Kind, child_path};
use std::collections::HashMap;
use std::f64::consts::TAU;

/// Rings drawn at most.
pub const RINGS: usize = 6;
/// Wedges narrower than this fold into their folder's smaller objects: 1.15 degrees, 2 pixels where the first ring starts in a map 900 pixels tall.
pub const MIN_ANGLE: f64 = 0.02;
/// How opaque a collected wedge is.
pub const FADED: f64 = 0.3;

/// What a wedge stands for: a file or folder of the tree, a folder's smaller objects, or the hidden space.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Entry(String),
    Smaller(String),
    Hidden,
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub target: Target,
    pub name: String,
    pub size: u64,
    /// What a folder holds, or what the smaller objects add up to. 0 for a file and the hidden space.
    pub items: u64,
    /// Whether it is a folder, whose wedge says how many items it holds.
    pub is_dir: bool,
    pub ring: usize,
    pub a0: f64,
    pub a1: f64,
    /// `0xRRGGBB`, and how opaque.
    pub color: u32,
    pub alpha: f64,
}

pub struct Layout {
    /// By ring, and in each ring by `a0`.
    pub segments: Vec<Segment>,
    /// The `entry` segments' places in `segments`, by path, for the list's colour swatches.
    pub by_path: HashMap<String, usize>,
    /// How many rings have segments.
    pub rings: usize,
}

const SMALLER: &str = "Smaller objects";
const HIDDEN: &str = "Hidden space";
/// How opaque a folder's smaller objects are, next to the folder itself.
const SMALLER_ALPHA: f64 = 0.3;
/// How far round the hue wheel the first ring goes, in degrees. It stops short of a full turn so the first wedge and the last differ.
const HUE_SPAN: f64 = 320.0;
/// How much of a wedge's own place round the wheel a folder under the first ring takes on, next to its first-ring folder's hue.
const HUE_DRIFT: f64 = 0.3;

/// A hue round the wheel for an angle of the circle, clockwise from 12 o'clock: red, yellow, green, blue, then pink.
fn hue_at(angle: f64) -> f64 {
    20.0 + angle / TAU * HUE_SPAN
}

/// The colour of OKLCH `l` (0 to 1), chroma `c` and hue `h` in degrees, cut to what a screen can show. OKLCH keeps every hue as bright as the others.
fn oklch(l: f64, c: f64, h: f64) -> u32 {
    let (a, b) = (c * (h * std::f64::consts::PI / 180.0).cos(), c * (h * std::f64::consts::PI / 180.0).sin());
    let lms = [l + 0.3963377774 * a + 0.2158037573 * b, l - 0.1055613458 * a - 0.0638541728 * b, l - 0.0894841775 * a - 1.291485548 * b].map(|v| v.powi(3));
    let linear = [
        4.0767416621 * lms[0] - 3.3077115913 * lms[1] + 0.2309699292 * lms[2],
        -1.2684380046 * lms[0] + 2.6097574011 * lms[1] - 0.3413193965 * lms[2],
        -0.0041960863 * lms[0] - 0.7034186147 * lms[1] + 1.707614701 * lms[2],
    ];
    let [r, g, blue] = linear.map(|v| {
        let x = v.clamp(0.0, 1.0);
        ((if x <= 0.0031308 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }) * 255.0).round() as u32
    });
    r << 16 | g << 8 | blue
}

/// A folder of `hue` in `ring`: lighter and greyer the further out it is.
fn folder_color(hue: f64, ring: usize) -> u32 {
    oklch((0.7 + ring as f64 * 0.034).min(0.88), (0.135 - ring as f64 * 0.011).max(0.05), hue)
}

fn file_color(ring: usize) -> u32 {
    oklch((0.7 + ring as f64 * 0.015).min(0.84), 0.0, 0.0)
}

/// A folder whose children are about to get their wedges.
struct Parent<'a> {
    entry: &'a Entry,
    path: String,
    a0: f64,
    a1: f64,
    /// The first-ring folder it is in or is, whose hue its children keep, and the middle of its wedge. None for the focus.
    family: Option<(f64, f64)>,
    /// Its own hue, which its smaller objects take. None for the focus.
    hue: Option<f64>,
}

/// Lays out `top`, the folder at `focus`, as rings round the centre. `hidden` is the bytes of hidden space to give a wedge in the first ring.
pub fn layout(top: &Entry, focus: &str, rings: usize, hidden: Option<u64>, min_angle: f64) -> Layout {
    let mut out = Layout { segments: Vec::new(), by_path: HashMap::new(), rings: 0 };
    let extra = hidden.unwrap_or(0) as f64;
    let total = top.size as f64 + extra;
    if total <= 0.0 {
        return out;
    }
    let own = TAU * (top.size as f64 / total);
    let mut parents: Vec<Parent> = if top.size > 0 { vec![Parent { entry: top, path: focus.to_owned(), a0: 0.0, a1: own, family: None, hue: None }] } else { Vec::new() };
    let mut ring = 0;
    while ring < rings && (!parents.is_empty() || (ring == 0 && extra > 0.0)) {
        let mut next = Vec::new();
        for parent in &parents {
            place(&mut out, parent, ring, ring + 1 < rings, min_angle, &mut next);
        }
        if ring == 0 && extra > 0.0 {
            out.segments.push(Segment { target: Target::Hidden, name: HIDDEN.into(), size: extra as u64, items: 0, is_dir: false, ring: 0, a0: own, a1: TAU, color: oklch(0.36, 0.0, 0.0), alpha: 1.0 });
            out.rings = out.rings.max(1);
        }
        parents = next;
        ring += 1;
    }
    out
}

/// Gives the children of `parent` their wedges in `ring`, and the smaller objects theirs at the end. The folders to go on with are pushed to `next`.
fn place<'a>(out: &mut Layout, parent: &Parent<'a>, ring: usize, deeper: bool, min_angle: f64, next: &mut Vec<Parent<'a>>) {
    let Parent { entry, path, family, .. } = parent;
    if entry.size == 0 {
        return;
    }
    let per_byte = (parent.a1 - parent.a0) / entry.size as f64;
    let mut cursor = parent.a0;
    let mut folded_size = entry.smaller.map_or(0, |s| s.size);
    let mut folded_items = entry.smaller.map_or(0, |s| s.items);

    for child in &entry.children {
        // A folder Wings couldn't open has no size to show.
        if child.restricted || child.size == 0 {
            continue;
        }
        let width = child.size as f64 * per_byte;
        if width < min_angle {
            folded_size += child.size;
            folded_items += child.items + 1;
            continue;
        }
        let (a0, a1) = (cursor, cursor + width);
        cursor = a1;
        let mid = (a0 + a1) / 2.0;
        let child_path_name = child_path(path, &child.name);
        let (color, child_family, hue) = if child.kind != Kind::Dir {
            (file_color(ring), *family, 0.0)
        } else if let Some((family_hue, family_mid)) = *family {
            // Deeper, a folder keeps its family's hue and moves along the wheel with how far it sits from the family's middle.
            let hue = family_hue + (mid - family_mid) / TAU * HUE_SPAN * HUE_DRIFT;
            (folder_color(hue, ring), *family, hue)
        } else {
            // In the first ring a folder's hue comes from where it sits round the wheel.
            let hue = hue_at(mid);
            (folder_color(hue, ring), Some((hue, mid)), hue)
        };
        let segment = Segment { target: Target::Entry(child_path_name.clone()), name: child.name.clone(), size: child.size, items: child.items, is_dir: child.kind == Kind::Dir, ring, a0, a1, color, alpha: 1.0 };
        out.by_path.insert(child_path_name.clone(), out.segments.len());
        out.segments.push(segment);
        out.rings = out.rings.max(ring + 1);
        if deeper && child.kind == Kind::Dir && (!child.children.is_empty() || child.smaller.is_some()) {
            next.push(Parent { entry: child, path: child_path_name, a0, a1, family: child_family, hue: Some(hue) });
        }
    }

    if folded_size > 0 {
        let a1 = parent.a1.min(cursor + folded_size as f64 * per_byte);
        // The focus has no colour of its own, so its smaller objects take the one a folder in their place would have.
        // They are drawn from two rings further out, because at this opacity a colour of the folder's own lightness turns to mud on a dark page.
        let hue = parent.hue.unwrap_or_else(|| hue_at((cursor + a1) / 2.0));
        out.segments.push(Segment {
            target: Target::Smaller(path.clone()),
            name: SMALLER.into(),
            size: folded_size,
            items: folded_items,
            is_dir: false,
            ring,
            a0: cursor,
            a1,
            color: folder_color(hue, ring + 2),
            alpha: SMALLER_ALPHA,
        });
        out.rings = out.rings.max(ring + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Smaller;
    use crate::entry::parent_path;

    fn near(actual: f64, expected: f64, what: &str) {
        assert!((actual - expected).abs() < 1e-9, "{what}: {actual} is not {expected}");
    }

    fn file(name: &str, size: u64) -> Entry {
        Entry { name: name.into(), size, kind: Kind::File, items: 0, children: vec![], smaller: None, restricted: false }
    }

    fn dir(name: &str, children: Vec<Entry>, smaller: Option<Smaller>) -> Entry {
        let size = children.iter().map(|c| c.size).sum::<u64>() + smaller.map_or(0, |s| s.size);
        Entry { name: name.into(), size, kind: Kind::Dir, items: children.len() as u64, children, smaller, restricted: false }
    }

    fn mail() -> Entry {
        Entry { name: "Mail".into(), size: 0, kind: Kind::Dir, items: 0, children: vec![], smaller: None, restricted: true }
    }

    /// A made-up home folder with folders three deep, smaller objects and a folder that couldn't be opened.
    fn home() -> Entry {
        let projects: Vec<Entry> = (0..6u64).map(|i| dir(&format!("project-{i}"), vec![dir("node_modules", vec![dir("react", vec![file("index.js", 900_000)], None)], Some(Smaller { size: 600_000, items: 90 })), file("bundle.zip", 700_000 / (i + 1))], None)).collect();
        dir(
            "/Users/ada",
            vec![dir("Code", projects, None), dir("Movies", vec![file("holiday.mov", 3_000_000), file("clip.mov", 1_000_000)], Some(Smaller { size: 40_000, items: 5 })), dir("Library", vec![mail(), dir("Caches", vec![], Some(Smaller { size: 800_000, items: 300 }))], None), file("notes.txt", 20_000)],
            Some(Smaller { size: 100_000, items: 40 }),
        )
    }

    const HOME: &str = "/Users/ada";
    const SPAN: f64 = TAU;

    fn ring_zero(map: &Layout) -> Vec<&Segment> {
        map.segments.iter().filter(|s| s.ring == 0).collect()
    }

    fn span(s: &Segment) -> f64 {
        s.a1 - s.a0
    }

    #[test]
    fn the_first_ring_adds_up_to_a_full_circle_with_the_hidden_space_and_without_it() {
        let tree = home();
        let plain = layout(&tree, HOME, 6, None, 0.012);
        near(ring_zero(&plain).iter().map(|s| span(s)).sum::<f64>(), SPAN, "plain");
        let with_hidden = layout(&tree, HOME, 6, Some(5_000_000), 0.012);
        let first = ring_zero(&with_hidden);
        near(first.iter().map(|s| span(s)).sum::<f64>(), SPAN, "with hidden space");
        // The hidden space is the last wedge, and its share of everything in the ring.
        let wedge = first[first.len() - 1];
        assert_eq!(wedge.target, Target::Hidden);
        near(span(wedge), SPAN * 5_000_000.0 / (ring_zero(&plain).iter().map(|s| s.size).sum::<u64>() as f64 + 5_000_000.0), "hidden share");
    }

    #[test]
    fn children_stay_inside_their_parents_span_and_a_folder_that_couldnt_be_opened_gets_no_wedge() {
        let map = layout(&home(), HOME, 6, Some(2_000_000), 0.012);
        assert!(map.rings >= 4);
        assert!(!map.by_path.contains_key("/Users/ada/Library/Mail"));
        let mut inside = 0;
        for (i, s) in map.segments.iter().enumerate() {
            // Ordered by ring, then by angle, and no wedge starts before the one before it ends.
            if let Some(before) = i.checked_sub(1).map(|i| &map.segments[i]) {
                if before.ring == s.ring {
                    assert!(before.a1 <= s.a0 + 1e-9, "{} starts before {} ends", s.name, before.name);
                } else {
                    assert_eq!(before.ring + 1, s.ring);
                }
            }
            assert!(s.a0 >= -1e-9 && s.a1 <= TAU + 1e-9 && s.a1 > s.a0, "{}", s.name);
            if s.ring == 0 {
                continue;
            }
            // A folder's children and its smaller objects are in the ring after it.
            let folder = match &s.target {
                Target::Smaller(folder) => folder.clone(),
                Target::Entry(path) => parent_path(path).unwrap(),
                Target::Hidden => unreachable!(),
            };
            let parent = &map.segments[map.by_path[&folder]];
            assert_eq!(parent.ring, s.ring - 1);
            assert!(s.a0 >= parent.a0 - 1e-9 && s.a1 <= parent.a1 + 1e-9, "{} is outside {}", s.name, parent.name);
            inside += 1;
        }
        assert!(inside > 10, "{inside}");
    }

    #[test]
    fn a_child_narrower_than_min_angle_is_folded_into_its_folders_smaller_wedge_and_what_is_under_it_goes_too() {
        let logs = dir("logs", vec![file("big.log", 600), file("mid.log", 300), dir("old", vec![file("a.log", 2), file("b.log", 2)], None)], Some(Smaller { size: 96, items: 7 }));
        let top = dir("/data", vec![logs, file("rest.bin", 1000)], None);
        let all = layout(&top, "/data", 3, None, 0.0);
        assert!(all.by_path.contains_key("/data/logs/old") && all.by_path.contains_key("/data/logs/old/a.log"));

        // "old" is 4 of the 1000 bytes in "logs", which is half the circle: about 0.0126 rad.
        let map = layout(&top, "/data", 3, None, 0.02);
        assert!(!map.by_path.contains_key("/data/logs/old") && !map.by_path.contains_key("/data/logs/old/a.log"));
        let smaller = map.segments.iter().find(|s| matches!(s.target, Target::Smaller(_))).unwrap();
        assert_eq!(smaller.target, Target::Smaller("/data/logs".into()));
        // Its own 96 bytes in 7 items, and the 4 bytes of "old" and the 3 items it is, are counted there.
        assert_eq!((smaller.size, smaller.items), (100, 7 + 3));
        let folder = &map.segments[map.by_path["/data/logs"]];
        near(span(smaller), span(folder) * 100.0 / 1000.0, "smaller span");
        // It is the last wedge of the folder's span, and the wedges in that ring add up to the folder.
        near(smaller.a1, folder.a1, "smaller end");
        let ring: Vec<&Segment> = map.segments.iter().filter(|s| s.ring == 1).collect();
        assert!(std::ptr::eq(ring[ring.len() - 1], smaller));
        assert_eq!(ring.iter().map(|s| s.size).sum::<u64>(), folder.size);
        assert_eq!(smaller.alpha, SMALLER_ALPHA);
    }

    #[test]
    fn folders_take_their_hue_from_where_they_sit_and_files_are_grey() {
        let map = layout(&home(), HOME, 2, None, 0.0);
        let (code, notes) = (&map.segments[map.by_path["/Users/ada/Code"]], &map.segments[map.by_path["/Users/ada/notes.txt"]]);
        assert_ne!(code.color, notes.color);
        let (r, g, b) = (notes.color >> 16, notes.color >> 8 & 0xff, notes.color & 0xff);
        assert!(r == g && g == b, "{:06x}", notes.color);
        // A colour is the same however many times it is worked out.
        assert_eq!(layout(&home(), HOME, 2, None, 0.0).segments[0].color, map.segments[0].color);
    }
}
