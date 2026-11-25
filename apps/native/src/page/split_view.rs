//! A docked `Split`'s column you drag the width of (`onResize`). The app owns the width while the pointer moves, so the
//! page hears nothing until the button comes up: then it hears the width once and keeps it.

use gpui::{Context, Pixels};

use super::{PageView, tree};

/// A drag of a split's inner edge, from where the pointer was pressed.
pub(super) struct SplitDrag {
    id: u64,
    from: f32,
    size: f32,
    /// What the column was when the drag began, to tell whether it moved.
    began: f32,
    min: f32,
    min_rest: f32,
    handler: String,
}

impl PageView {
    /// How wide the docked column of `split` is now: the width being dragged or just dragged, until the page gives
    /// another `defaultSize`, clamped to what the split's own width leaves.
    pub(super) fn split_size(&self, split: &tree::Split) -> f32 {
        if split.on_resize.is_none() {
            return split.size;
        }
        let kept = match self.split_sizes.get(&split.id) {
            Some((set, base)) if *base == split.size => *set,
            _ => split.size,
        };
        let max = self.split_widths.get(&split.id).map_or(f32::MAX, |width| width - split.min_rest);
        kept.clamp(split.min_size, max.max(split.min_size))
    }

    pub(super) fn split_dragging(&self, id: u64) -> bool {
        self.split_drag.as_ref().is_some_and(|drag| drag.id == id)
    }

    pub(super) fn start_split_drag(&mut self, split: &tree::Split, x: Pixels, cx: &mut Context<Self>) {
        let Some(handler) = split.on_resize.clone() else { return };
        let size = self.split_size(split);
        self.split_drag = Some(SplitDrag {
            id: split.id,
            from: x.as_f32(),
            size,
            began: size,
            min: split.min_size,
            min_rest: split.min_rest,
            handler,
        });
        cx.notify();
    }

    /// The pointer is at `x`: the column is as wide as the pointer's move to the left makes it, within its bounds.
    pub(super) fn split_drag_to(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(drag) = &self.split_drag else { return };
        let max = self.split_widths.get(&drag.id).map_or(f32::MAX, |width| width - drag.min_rest).max(drag.min);
        let size = (drag.size + drag.from - x.as_f32()).clamp(drag.min, max).round();
        let base = self.split_sizes.get(&drag.id).map_or(drag.began, |(_, base)| *base);
        if self.split_sizes.get(&drag.id).map(|(set, _)| *set) != Some(size) {
            self.split_sizes.insert(drag.id, (size, base));
            cx.notify();
        }
    }

    /// The button came up: the page hears the width once, when it moved.
    pub(super) fn end_split_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.split_drag.take() else { return };
        if let Some((size, _)) = self.split_sizes.get(&drag.id).copied().filter(|(size, _)| *size != drag.began) {
            self.act(&drag.handler, Some(&serde_json::json!({ "size": size })));
        }
        cx.notify();
    }

    /// A double press on the edge: back to the width the page named, told to the page at once.
    pub(super) fn reset_split(&mut self, split: &tree::Split, cx: &mut Context<Self>) {
        let (Some(handler), Some(reset)) = (&split.on_resize, split.reset_size) else { return };
        self.split_drag = None;
        let size = self.split_size(split);
        self.split_sizes.insert(split.id, (reset, split.size));
        if size != reset {
            self.act(handler, Some(&serde_json::json!({ "size": reset })));
        }
        cx.notify();
    }
}
