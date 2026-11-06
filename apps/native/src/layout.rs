//! A tab's panes: a binary tree of splits with terminals at the leaves, like the desktop app's
//! `src/lib/layout.ts`.

use gpui::{Bounds, Pixels, point, px, size};

/// The draggable gap between two panes.
pub const DIVIDER: f32 = 8.0;
/// A drag keeps each side between 15% and 85%, as in the desktop app.
const MIN_RATIO: f32 = 0.15;
const MAX_RATIO: f32 = 0.85;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Side by side.
    Row,
    /// One above the other.
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toward {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    Pane(usize),
    Split {
        dir: Dir,
        /// The first side's share of the split's length.
        ratio: f32,
        a: Box<Layout>,
        b: Box<Layout>,
    },
}

/// Where everything sits: each pane's bounds, and each split's bounds with its path from the root
/// (`false` for the first side, `true` for the second).
#[derive(Default)]
pub struct Placed {
    pub panes: Vec<(usize, Bounds<Pixels>)>,
    pub splits: Vec<(Vec<bool>, Dir, Bounds<Pixels>)>,
}

impl Layout {
    pub fn panes(&self) -> Vec<usize> {
        match self {
            Layout::Pane(id) => vec![*id],
            Layout::Split { a, b, .. } => [a.panes(), b.panes()].concat(),
        }
    }

    /// Puts `new` beside `target`: to the right for a row, below for a column.
    pub fn split(&mut self, target: usize, dir: Dir, new: usize) {
        match self {
            Layout::Pane(id) if *id == target => {
                *self = Layout::Split {
                    dir,
                    ratio: 0.5,
                    a: Box::new(Layout::Pane(target)),
                    b: Box::new(Layout::Pane(new)),
                }
            }
            Layout::Pane(_) => {}
            Layout::Split { a, b, .. } => {
                a.split(target, dir, new);
                b.split(target, dir, new);
            }
        }
    }

    /// Removes a pane, and its sibling takes the parent's place. `None` when the last pane goes.
    pub fn remove(self, target: usize) -> Option<Layout> {
        match self {
            Layout::Pane(id) => (id != target).then_some(Layout::Pane(id)),
            Layout::Split { dir, ratio, a, b } => match (a.remove(target), b.remove(target)) {
                (Some(a), Some(b)) => Some(Layout::Split {
                    dir,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (one, other) => one.or(other),
            },
        }
    }

    pub fn set_ratio(&mut self, path: &[bool], ratio: f32) {
        match (self, path.split_first()) {
            (Layout::Split { ratio: current, .. }, None) => {
                *current = ratio.clamp(MIN_RATIO, MAX_RATIO)
            }
            (Layout::Split { a, b, .. }, Some((second, rest))) => {
                if *second { b } else { a }.set_ratio(rest, ratio)
            }
            _ => {}
        }
    }

    /// The same sizes the rendered flex rows give: the first side takes `ratio` of the length,
    /// then the divider, then the rest.
    pub fn place(&self, bounds: Bounds<Pixels>) -> Placed {
        let mut placed = Placed::default();
        self.place_into(bounds, &mut Vec::new(), &mut placed);
        placed
    }

    fn place_into(&self, bounds: Bounds<Pixels>, path: &mut Vec<bool>, placed: &mut Placed) {
        let Layout::Split { dir, ratio, a, b } = self else {
            if let Layout::Pane(id) = self {
                placed.panes.push((*id, bounds));
            }
            return;
        };
        placed.splits.push((path.clone(), *dir, bounds));
        let gap = px(DIVIDER);
        let (first, second) = match dir {
            Dir::Row => {
                let width = bounds.size.width * *ratio;
                (
                    Bounds::new(bounds.origin, size(width, bounds.size.height)),
                    Bounds::new(
                        point(bounds.origin.x + width + gap, bounds.origin.y),
                        size(bounds.size.width - width - gap, bounds.size.height),
                    ),
                )
            }
            Dir::Column => {
                let height = bounds.size.height * *ratio;
                (
                    Bounds::new(bounds.origin, size(bounds.size.width, height)),
                    Bounds::new(
                        point(bounds.origin.x, bounds.origin.y + height + gap),
                        size(bounds.size.width, bounds.size.height - height - gap),
                    ),
                )
            }
        };
        for (second_side, child, child_bounds) in [(false, a, first), (true, b, second)] {
            path.push(second_side);
            child.place_into(child_bounds, path, placed);
            path.pop();
        }
    }
}

/// The pane next to `from` on one side, by on-screen position, scored like the desktop app's
/// `neighborPane`: the gap to it plus a quarter of how far it sits across.
pub fn neighbor(panes: &[(usize, Bounds<Pixels>)], from: usize, toward: Toward) -> Option<usize> {
    let (_, from_bounds) = panes.iter().find(|(id, _)| *id == from)?;
    let f = from_bounds;
    panes
        .iter()
        .filter(|(id, _)| *id != from)
        .filter_map(|(id, r)| {
            let gap = match toward {
                Toward::Left => f.left() - r.right(),
                Toward::Right => r.left() - f.right(),
                Toward::Up => f.top() - r.bottom(),
                Toward::Down => r.top() - f.bottom(),
            }
            .as_f32();
            let across = match toward {
                Toward::Left | Toward::Right => r.top() - f.top(),
                Toward::Up | Toward::Down => r.left() - f.left(),
            }
            .as_f32()
            .abs();
            (gap >= -1.0).then_some((*id, gap + across / 4.0))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Bounds<Pixels> {
        Bounds::new(point(px(0.0), px(0.0)), size(px(808.0), px(408.0)))
    }

    /// 1 on the left, 2 above 3 on the right.
    fn three() -> Layout {
        let mut layout = Layout::Pane(1);
        layout.split(1, Dir::Row, 2);
        layout.split(2, Dir::Column, 3);
        layout
    }

    #[test]
    fn splitting_and_removing_keep_the_desktop_tree_shape() {
        let layout = three();
        assert_eq!(layout.panes(), vec![1, 2, 3]);
        let layout = layout.remove(2).unwrap();
        assert_eq!(layout.panes(), vec![1, 3]);
        assert!(matches!(&layout, Layout::Split { dir: Dir::Row, .. }));
        let layout = layout.remove(1).unwrap();
        assert_eq!(layout, Layout::Pane(3));
        assert_eq!(layout.remove(3), None);
    }

    #[test]
    fn placing_matches_the_rendered_flex_sizes_and_finds_neighbours() {
        let placed = three().place(area());
        let at = |id| placed.panes.iter().find(|(pane, _)| *pane == id).unwrap().1;
        assert_eq!((at(1).size.width, at(2).size.width), (px(404.0), px(396.0)));
        assert_eq!(at(2).origin.x, px(412.0));
        assert_eq!((at(2).size.height, at(3).origin.y), (px(204.0), px(212.0)));
        assert_eq!(placed.splits.len(), 2);
        assert_eq!(placed.splits[1].0, vec![true]);

        assert_eq!(neighbor(&placed.panes, 1, Toward::Right), Some(2));
        assert_eq!(neighbor(&placed.panes, 3, Toward::Left), Some(1));
        assert_eq!(neighbor(&placed.panes, 2, Toward::Down), Some(3));
        assert_eq!(neighbor(&placed.panes, 3, Toward::Up), Some(2));
        assert_eq!(neighbor(&placed.panes, 1, Toward::Left), None);
    }

    #[test]
    fn dragging_a_divider_stays_between_15_and_85_percent() {
        let mut layout = three();
        layout.set_ratio(&[true], 0.99);
        layout.set_ratio(&[], 0.01);
        let Layout::Split { ratio, b, .. } = &layout else {
            panic!("a split")
        };
        assert_eq!(*ratio, 0.15);
        assert!(matches!(**b, Layout::Split { ratio, .. } if ratio == 0.85));
    }
}
