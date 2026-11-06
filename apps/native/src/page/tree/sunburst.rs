//! The `Sunburst` a disk map draws: wedges in rings round a centre that says what the pointer is on. The page lays the
//! wedges out and sends their angles, so the app only draws them and says which one the pointer is on.

use std::f32::consts::TAU;

use serde_json::{Map, Value};

use super::{TreeError, cut, number, str_prop, valid_handler};

/// The most wedges and rings a page may send.
pub const MAX_WEDGES: usize = 8_000;
pub const MAX_RINGS: usize = 12;

/// The room round the map, so nothing touches the frame.
const MARGIN: f32 = 12.0;
/// The circle in the middle, as a share of the map's radius.
const CENTER: f32 = 0.24;

/// A wedge: from `from` to `to` radians clockwise from 12 o'clock, in `ring` (0 is the first round the centre).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wedge {
    pub from: f32,
    pub to: f32,
    pub ring: usize,
    /// `0xRRGGBB`.
    pub color: u32,
    pub alpha: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sunburst {
    pub id: u64,
    /// How many rings the map has, which sets how wide each one is.
    pub rings: usize,
    /// By ring, and in each ring by `from`.
    pub wedges: Vec<Wedge>,
    /// The wedge drawn outlined.
    pub selected: Option<usize>,
    /// The two lines in the middle.
    pub title: String,
    pub detail: String,
    /// A click on the middle goes up, so it lights up under the pointer.
    pub up: bool,
    /// Gets `{ index }` when the pointer moves to another wedge: the wedge's index, -1 for none, -2 for the middle.
    pub on_point: Option<String>,
    /// Gets `{ index, meta }` on a click.
    pub on_select: Option<String>,
}

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Wedge(usize),
    Center,
}

impl Hit {
    /// The index a page is told: the wedge's, -2 for the middle, -1 for nothing.
    pub fn index(hit: Option<Hit>) -> i64 {
        match hit {
            Some(Hit::Wedge(index)) => index as i64,
            Some(Hit::Center) => -2,
            None => -1,
        }
    }
}

/// Where the map is drawn in a box: the centre, the radius of the circle in the middle, and how wide each ring is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub cx: f32,
    pub cy: f32,
    pub inner: f32,
    pub ring_width: f32,
}

impl Geometry {
    /// The map in a `width` by `height` box: square, in the middle, `rings` rings of the same width out to the edge.
    /// `None` when the box is too small to draw anything in.
    pub fn of(width: f32, height: f32, rings: usize) -> Option<Self> {
        let outer = (width.min(height) / 2.0 - MARGIN).max(0.0);
        if outer < 8.0 {
            return None;
        }
        let inner = outer * CENTER;
        Some(Self {
            cx: width / 2.0,
            cy: height / 2.0,
            inner,
            ring_width: (outer - inner) / rings.max(1) as f32,
        })
    }

    /// What is under the point (`x`, `y`) of the box.
    pub fn hit(&self, rings: usize, wedges: &[Wedge], x: f32, y: f32) -> Option<Hit> {
        let (dx, dy) = (x - self.cx, y - self.cy);
        let radius = dx.hypot(dy);
        if radius < self.inner {
            return Some(Hit::Center);
        }
        let ring = ((radius - self.inner) / self.ring_width).floor();
        if !(ring >= 0.0 && (ring as usize) < rings) {
            return None;
        }
        let ring = ring as usize;
        let mut angle = dx.atan2(-dy);
        if angle < 0.0 {
            angle += TAU;
        }
        wedges
            .iter()
            .position(|w| w.ring == ring && w.from <= angle && angle < w.to)
            .map(Hit::Wedge)
    }
}

pub(super) fn parse(id: u64, props: &Map<String, Value>) -> Result<Sunburst, TreeError> {
    let invalid = || {
        TreeError::Invalid(format!(
            "invalid sunburst ({MAX_WEDGES} wedges in {MAX_RINGS} rings, [from, to, ring, rgb, alpha] each)"
        ))
    };
    let rings = number(props, "rings").ok_or_else(invalid)?;
    if !((1.0..=MAX_RINGS as f64).contains(&rings) && rings.fract() == 0.0) {
        return Err(invalid());
    }
    let rings = rings as usize;
    let list = props
        .get("wedges")
        .and_then(Value::as_array)
        .filter(|list| list.len() <= MAX_WEDGES)
        .ok_or_else(invalid)?;
    let wedges = list
        .iter()
        .map(|wedge| {
            let [from, to, ring, color, alpha] = wedge.as_array()?.as_slice() else {
                return None;
            };
            let (from, to, alpha) = (from.as_f64()?, to.as_f64()?, alpha.as_f64()?);
            let (ring, color) = (ring.as_u64()? as usize, color.as_u64()?);
            let sane = (0.0..=TAU as f64 + 0.001).contains(&from)
                && (from..=TAU as f64 + 0.001).contains(&to)
                && ring < rings
                && color <= 0xff_ff_ff
                && (0.0..=1.0).contains(&alpha);
            sane.then_some(Wedge {
                from: from as f32,
                to: to as f32,
                ring,
                color: color as u32,
                alpha: alpha as f32,
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(invalid)?;
    let handler = |name| {
        str_prop(props, name)
            .filter(|s| valid_handler(s))
            .map(str::to_owned)
    };
    Ok(Sunburst {
        id,
        rings,
        selected: number(props, "selected")
            .filter(|index| *index >= 0.0 && (*index as usize) < wedges.len())
            .map(|index| index as usize),
        wedges,
        title: cut(str_prop(props, "title").unwrap_or_default().to_owned()),
        detail: cut(str_prop(props, "detail").unwrap_or_default().to_owned()),
        up: props.get("up") == Some(&Value::Bool(true)),
        on_point: handler("onPoint"),
        on_select: handler("onSelect"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn props(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn parses_wedges_and_refuses_ones_that_are_not() {
        let good = parse(
            7,
            &props(json!({ "rings": 2, "wedges": [[0, 3.0, 0, 255, 1], [3.0, 6.2, 0, 0, 0.3]], "selected": 1, "title": "Code", "onSelect": "7:onSelect" })),
        )
        .unwrap();
        assert_eq!(good.wedges.len(), 2);
        assert_eq!(good.wedges[0].color, 255);
        assert_eq!(good.selected, Some(1));
        assert_eq!(good.on_select.as_deref(), Some("7:onSelect"));
        assert_eq!(good.on_point, None);
        for wedges in [
            json!([[0, 1, 2, 0, 1]]),
            json!([[1, 0, 0, 0, 1]]),
            json!([[0, 1, 0, 16777216, 1]]),
            json!([[0, 1, 0, 0, 2]]),
            json!([[0, 1, 0, 0]]),
            json!([[0, 1, 0, "x", 1]]),
        ] {
            assert!(
                parse(1, &props(json!({ "rings": 2, "wedges": wedges }))).is_err(),
                "{wedges}"
            );
        }
        assert!(parse(1, &props(json!({ "rings": 99, "wedges": [] }))).is_err());
        let many = vec![json!([0, 1, 0, 0, 1]); MAX_WEDGES + 1];
        assert!(parse(1, &props(json!({ "rings": 1, "wedges": many }))).is_err());
    }

    #[test]
    fn finds_the_wedge_under_a_point() {
        let wedges = [
            Wedge {
                from: 0.0,
                to: std::f32::consts::PI,
                ring: 0,
                color: 0,
                alpha: 1.0,
            },
            Wedge {
                from: std::f32::consts::PI,
                to: TAU,
                ring: 0,
                color: 0,
                alpha: 1.0,
            },
            Wedge {
                from: 0.0,
                to: 1.0,
                ring: 1,
                color: 0,
                alpha: 1.0,
            },
        ];
        let geometry = Geometry::of(400.0, 400.0, 2).unwrap();
        let (cx, cy) = (geometry.cx, geometry.cy);
        let first = geometry.inner + geometry.ring_width / 2.0;
        // Right of the centre is 3 o'clock, in the first half of the first ring.
        assert_eq!(
            geometry.hit(2, &wedges, cx + first, cy),
            Some(Hit::Wedge(0))
        );
        assert_eq!(
            geometry.hit(2, &wedges, cx - first, cy),
            Some(Hit::Wedge(1))
        );
        // Straight up in the second ring is inside the third wedge; to its left there is none.
        let second = geometry.inner + geometry.ring_width * 1.5;
        assert_eq!(
            geometry.hit(2, &wedges, cx, cy - second),
            Some(Hit::Wedge(2))
        );
        assert_eq!(geometry.hit(2, &wedges, cx - second, cy), None);
        assert_eq!(
            geometry.hit(2, &wedges, cx + 3.0, cy - 3.0),
            Some(Hit::Center)
        );
        assert_eq!(geometry.hit(2, &wedges, 1.0, 1.0), None);
        assert_eq!(Geometry::of(20.0, 400.0, 2), None);
        assert_eq!(Hit::index(Some(Hit::Wedge(4))), 4);
        assert_eq!(Hit::index(Some(Hit::Center)), -2);
        assert_eq!(Hit::index(None), -1);
    }
}
