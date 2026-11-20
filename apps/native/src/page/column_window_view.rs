//! A long column in a `Scroll`, as a chat's conversation is, drawn a window at a time. Every render of the page laid out
//! and wrapped every message, so a key typed in a long chat's message box, a streamed reply and any change of the page
//! cost a frame in proportion to the whole conversation. Only the children in view and half a view either side of it are
//! drawn now; the others are spacers as tall as each child was when last drawn, or, for one never drawn, as tall as the
//! drawn ones are on average.
//!
//! The first child in view (the pivot) is laid out at the place the kept heights give it, the children after it down
//! from it and the ones before it up from it, so a child above it that turns out taller or shorter than kept (one never
//! drawn, or drawn at another width or text size) moves nothing in view. The next render finds the child at the top of
//! the view in the last frame's places and scrolls so it stays where it was, which moves the content and the view
//! together.
//!
//! What reads where the column's children are (`scrollTo`, `onTrack`, keeping the view in place as children come and go,
//! a right-click's child) hears every child's place: one not drawn where its kept height puts it. The children that hold
//! a selection's ends, the field with the keyboard or an open menu are drawn wherever they are, with all between them and
//! the view, so a selection copies whole and a field keeps the keys typed in it.

use super::*;
use gpui::{Bounds, size};

/// A `Scroll`'s column with more children than this is drawn a window at a time.
const WINDOW_FROM: usize = 40;

/// How tall a child is taken to be before any child of its column has been drawn: about a line and its padding. Low, so a
/// first window draws more children than it needs rather than leave part of the view empty.
const FIRST_GUESS: f32 = 40.0;

/// What a column drawn a window at a time keeps from one render to the next.
#[derive(Default)]
pub(super) struct Windowing {
    /// Each child's height by node id: as last drawn (`true`), or guessed for one never drawn.
    heights: HashMap<u64, (Pixels, bool)>,
    /// Where each child was in the last frame, from the column's top: its top and bottom, in the column's order.
    placed: Vec<(Pixels, Pixels)>,
    /// The tree drawn in that frame, and how far the column's top was below the top of what scrolls.
    placed_tree: u64,
    lead: Pixels,
    /// When each child was first in the tree: one that came before its rise was over doesn't rise as it is scrolled to.
    seen: HashMap<u64, Instant>,
    /// The tree `heights` and `seen` were last kept to.
    tree: u64,
    /// The children the last render drew, of how many.
    pub(super) drawn: (std::ops::Range<usize>, usize),
    /// Each child's top in the window in the last frame, and the top of the view.
    #[cfg(all(test, feature = "test-support"))]
    screen: (Vec<f32>, f32),
}

/// Whether `column`, the one child of a `Scroll`, is drawn a window at a time: it is long, and each child is one element
/// with an id, as a chat's messages are. A run of plain strings is one element of several nodes.
pub(super) fn windowed(column: &tree::Stack) -> bool {
    column.children.len() > WINDOW_FROM
        && !column.inset
        && column.children.iter().all(|child| !matches!(child, Node::Str { .. }) && child.id().is_some())
}

impl PageView {
    /// `stack`, the column of the `Scroll` that `state` keeps, with only the children in view and half a view either side
    /// drawn. `stuck` says the scroll follows its end in this render. `watch` hears every child's place.
    pub(super) fn windowed_column(
        &self,
        stack: &tree::Stack,
        state: &Rc<FollowScroll>,
        stuck: bool,
        watch: Option<popup_menu_view::ChildrenWatch>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let count = stack.children.len();
        let ids: Vec<u64> = stack.children.iter().filter_map(Node::id).collect();
        let space = px(gap(stack.gap));
        let now = cx.background_executor().now();
        let handle = &state.handle;
        let mut kept = state.windowing.borrow_mut();
        if kept.tree != self.trees {
            kept.tree = self.trees;
            let here: HashSet<u64> = ids.iter().copied().collect();
            kept.heights.retain(|id, _| here.contains(id));
            kept.seen.retain(|id, _| here.contains(id));
        }
        for id in &ids {
            kept.seen.entry(*id).or_insert(now);
        }
        // A child never drawn is as tall as the drawn ones are on average.
        let (sum, drawn) = ids
            .iter()
            .filter_map(|id| kept.heights.get(id).filter(|(_, drawn)| *drawn))
            .fold((0.0, 0), |(sum, drawn), (height, _)| (sum + height.as_f32(), drawn + 1));
        let guess = px(if drawn > 0 { sum / drawn as f32 } else { FIRST_GUESS });
        let used: Vec<Pixels> = ids
            .iter()
            .map(|id| {
                let (height, drawn) = kept.heights.entry(*id).or_insert((guess, false));
                if !*drawn {
                    *height = guess;
                }
                *height
            })
            .collect();
        let mut tops = Vec::with_capacity(count);
        let mut y = px(0.0);
        for height in &used {
            tops.push(y);
            y += *height + space;
        }
        let total = (y - space).max(px(0.0));
        let bottoms: Vec<Pixels> = tops.iter().zip(&used).map(|(top, height)| *top + *height).collect();
        // Not laid out yet, the view is taken to be as tall as the window.
        let view = match handle.bounds().size.height {
            height if height > px(0.0) => height,
            _ => px(self.viewport.get().1),
        };
        let mut offset = handle.offset();
        // The child at the view's top in the last frame stays where it was, now that the children drawn then have their
        // heights: scrolled by what they changed above it. A new tree is kept in place by `keep_in_place`, which reads
        // how far it moved what was in view; following the end, the scroll goes there.
        if !stuck && kept.placed_tree == self.trees && kept.placed.len() == count {
            let seen_top = -offset.y - kept.lead;
            let at = kept.placed.partition_point(|(_, bottom)| *bottom <= seen_top).min(count - 1);
            let wanted = tops[at] + (seen_top - kept.placed[at].0);
            offset.y = (-(wanted + kept.lead)).min(px(0.0));
            if (offset.y - handle.offset().y).abs() > px(0.01) {
                handle.set_offset(offset);
            }
        }
        let last_top = (total - view).max(px(0.0));
        let top = if stuck { last_top } else { (-offset.y - kept.lead).clamp(px(0.0), last_top) };
        let margin = view * 0.5;
        let pivot = bottoms.partition_point(|bottom| *bottom <= top).min(count - 1);
        let mut start = bottoms.partition_point(|bottom| *bottom <= top - margin).min(pivot);
        let mut end = tops.partition_point(|at| *at < top + view + margin).max(pivot + 1);
        if let Some((first, last)) = self.pinned(stack) {
            start = start.min(first);
            end = end.max(last + 1);
        }
        let rises: Vec<bool> = (start..end)
            .map(|at| stack.enter && kept.seen.get(&ids[at]).is_some_and(|seen| now.saturating_duration_since(*seen) < ui::motion::RISE))
            .collect();
        kept.drawn = (start..end, count);
        drop(kept);

        let before = LINKED.with(Cell::get);
        LINKED.with(|cell| cell.set(before || stack.links));
        let above: Vec<AnyElement> =
            (start..pivot).map(|at| self.column_child(&stack.children[at], stack.enter, rises[at - start], cx)).collect();
        let below: Vec<AnyElement> =
            (pivot..end).map(|at| self.column_child(&stack.children[at], stack.enter, rises[at - start], cx)).collect();
        LINKED.with(|cell| cell.set(before));

        // The children before the pivot hang from its top, in a box as tall as their kept heights and the ones not drawn,
        // which they overflow upwards when they are taller.
        let drawn_above: Rc<RefCell<Vec<Bounds<Pixels>>>> = Rc::default();
        let hanging = (pivot > 0).then(|| {
            let drawn_above = drawn_above.clone();
            div().flex_none().relative().h(tops[pivot] - space).child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .flex()
                    .flex_col()
                    .gap(space)
                    .children(above)
                    .on_children_prepainted(move |bounds, _, _| *drawn_above.borrow_mut() = bounds),
            )
        });
        let after = (end < count).then(|| div().flex_none().h(total - tops[end]));
        let element = Self::stack_box(stack).children(hanging).children(below).children(after);

        let state = state.clone();
        let tree = self.trees;
        let (hangs, trails) = (usize::from(pivot > 0), usize::from(end < count));
        let places = move |bounds: Vec<Bounds<Pixels>>, window: &mut Window, _: &mut gpui::App| {
            if bounds.len() != hangs + (end - pivot) + trails {
                return bounds;
            }
            let mut drawn = drawn_above.borrow().clone();
            drawn.extend_from_slice(&bounds[hangs..bounds.len() - trails]);
            if drawn.len() != end - start {
                return bounds;
            }
            let (first, last) = (drawn[0], drawn[drawn.len() - 1]);
            let mut places = vec![Bounds::default(); count];
            places[start..end].copy_from_slice(&drawn);
            let mut y = first.top();
            for at in (0..start).rev() {
                y -= space + used[at];
                places[at] = Bounds::new(point(first.left(), y), size(first.size.width, used[at]));
            }
            let mut y = last.bottom();
            for at in end..count {
                y += space;
                places[at] = Bounds::new(point(last.left(), y), size(last.size.width, used[at]));
                y += used[at];
            }
            let view = state.handle.bounds();
            // Where the column's top is with the pivot where its kept heights put it. The layout rounds where it puts the
            // pivot, and a place read from the column's own top would scroll by that rounding on every render.
            let column = places[pivot].top() - tops[pivot];
            let mut kept = state.windowing.borrow_mut();
            for (at, place) in (start..end).zip(&drawn) {
                kept.heights.insert(ids[at], (place.size.height, true));
            }
            kept.placed = places.iter().map(|place| (place.top() - column, place.bottom() - column)).collect();
            kept.placed_tree = tree;
            kept.lead = column - view.top() - state.handle.offset().y;
            #[cfg(all(test, feature = "test-support"))]
            {
                kept.screen = (places.iter().map(|place| place.top().as_f32()).collect(), view.top().as_f32());
            }
            drop(kept);
            // Children drawn shorter than their guesses, or a jump past the margin, left part of the view empty: the next
            // frame draws the window where the view is now.
            let short = (start > 0 && first.top() > view.top()) || (end < count && last.bottom() < view.bottom());
            if view.size.height > px(0.0) && short {
                window.request_animation_frame();
            }
            places
        };
        self.with_context_menu(element, stack, watch, Some(Box::new(places)), cx)
    }

    /// A child of a windowed column. In a column whose children rise, one that was there before its rise ended is drawn
    /// as it was, under the same element id as its rise, so what is inside it keeps its state.
    fn column_child(&self, node: &Node, enter: bool, rise: bool, cx: &mut Context<Self>) -> AnyElement {
        match (enter, rise, node.id()) {
            (false, _, _) => self.node(node, cx),
            (true, false, Some(id)) => div()
                .id(ElementId::NamedInteger("page-rise".into(), id))
                .min_w_0()
                .flex()
                .flex_col()
                .child(self.node(node, cx))
                .into_any_element(),
            _ => self.rising(node, cx),
        }
    }

    /// The first and last children of `stack` that hold an end of the selection, the field with the keyboard or the open
    /// menu: they are drawn wherever they are.
    fn pinned(&self, stack: &tree::Stack) -> Option<(usize, usize)> {
        let mut held: Vec<u64> = self.selection_ends().map_or_else(Vec::new, |(anchor, head)| vec![anchor, head]);
        held.extend(self.focused);
        held.extend(self.menu);
        if held.is_empty() {
            return None;
        }
        let mut found = stack
            .children
            .iter()
            .enumerate()
            .filter(|(_, child)| held.iter().any(|id| selection_view::has_node(std::slice::from_ref(*child), *id)))
            .map(|(at, _)| at);
        let first = found.next()?;
        Some((first, found.last().unwrap_or(first)))
    }

    /// The children the windowed column of the `Scroll` `id` drew in the last render, of how many, and where each child's
    /// top was below the view's top in the last frame, in points.
    #[cfg(all(test, feature = "test-support"))]
    pub(in crate::page) fn windowed_drawn(&self, id: u64) -> Option<(std::ops::Range<usize>, usize, Vec<f32>)> {
        let scrolls = self.follow_scrolls.borrow();
        let kept = scrolls.get(&id)?.windowing.borrow();
        let (tops, view) = &kept.screen;
        Some((kept.drawn.0.clone(), kept.drawn.1, tops.iter().map(|top| top - view).collect()))
    }

    /// Where the view of the `Scroll` `id` was in the window in the last frame.
    #[cfg(all(test, feature = "test-support"))]
    pub(in crate::page) fn scroll_view_top(&self, id: u64) -> Option<f32> {
        Some(self.follow_scrolls.borrow().get(&id)?.windowing.borrow().screen.1)
    }
}
