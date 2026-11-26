//! A `Scroll`: what it follows, keeps in place and reports back.
use super::*;

impl PageView {
    /// The handle of a `Scroll` that follows its end, which keeps it at its end through this draw while it is there, as
    /// a chat stays on its newest message. Scrolled up, it stays where it is, and so it does while the page shows an older
    /// part of its list (`newest` false) or newer items it asked for are on their way.
    pub(super) fn follow_end(&self, id: u64, newest: bool) -> Rc<FollowScroll> {
        let state = self.follow_scrolls.borrow_mut().entry(id).or_default().clone();
        let handle = &state.handle;
        // Its offset grows negative as it scrolls down, to minus the most it can scroll. Not laid out yet, both are 0.
        let stuck = newest && !state.asked_end.get() && -handle.offset().y >= handle.max_offset().y - px(FOLLOW_SLACK);
        state.stuck.set(stuck);
        if stuck {
            handle.scroll_to_bottom();
        }
        state
    }

    /// A `Scroll` with a `maxHeight`: as tall as its children up to that, then it scrolls inside itself, with a thin thumb
    /// at its right edge while it can. GPUI hands a wheel event to every scroll under the pointer, so a list inside a
    /// scrolling card would move the card too: a wheel step the list took stops there, and one it couldn't (it is at that
    /// end, or short) goes on to the parent. The thumb is painted from the last layout, so a step never builds the page.
    pub(super) fn scroll_within(&self, scroll: &tree::Scroll, cx: &mut Context<Self>) -> AnyElement {
        let handle = self.watched_scroll(scroll.id).handle.clone();
        let wheel = handle.clone();
        let thumb = handle.clone();
        let area = div()
            .id(ElementId::NamedInteger("page-scroll-within".into(), scroll.id))
            .max_h(px(scroll.max_height.unwrap_or_default()))
            .min_w_0()
            .flex()
            .flex_col()
            .pr(px(6.0))
            .overflow_y_scroll()
            .track_scroll(&handle)
            .on_scroll(move |event, window, cx| {
                // The scroll's own listener ran first and moved the offset without clamping it, so what it was before the
                // step is the offset less the step.
                let step = event.delta.pixel_delta(window.line_height()).y;
                let offset = wheel.offset();
                let most = wheel.max_offset().y;
                let before = offset.y - step;
                // Compared with half a point to spare: `Pixels` orders by `total_cmp`, where 0 is above -0, so a list with
                // nothing to scroll (`most` 0, an offset of 0) counted as one that could still go down and took the step.
                let slack = px(0.5);
                let moved = (step > px(0.0) && before < -slack) || (step < px(0.0) && before > -most + slack);
                wheel.set_offset(point(offset.x, offset.y.clamp(-most, px(0.0))));
                if moved {
                    cx.stop_propagation();
                }
            })
            .children(self.nodes(&scroll.children, cx));
        let thumb = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let most = thumb.max_offset().y.as_f32();
                if most <= 0.0 {
                    return;
                }
                let height = bounds.size.height.as_f32();
                let long = height + most;
                let size = (height / long * height).max(16.0).min(height);
                let top = (-thumb.offset().y.as_f32() / most).clamp(0.0, 1.0) * (height - size);
                window.paint_quad(
                    gpui::fill(
                        gpui::Bounds::new(
                            point(bounds.right() - px(4.0), bounds.top() + px(top)),
                            gpui::size(px(4.0), px(size)),
                        ),
                        rgba(0xffffff40),
                    )
                    .corner_radii(px(2.0)),
                );
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div().relative().flex_none().min_w_0().child(area).child(thumb).into_any_element()
    }

    /// The handle of a `Scroll` that doesn't follow its end but is told where to scroll or which children to watch.
    pub(super) fn watched_scroll(&self, id: u64) -> Rc<FollowScroll> {
        self.follow_scrolls.borrow_mut().entry(id).or_default().clone()
    }

    /// Runs once the column of a `Scroll` with `scrollTo` or `track` is laid out, with the bounds of its children: scrolls to
    /// the child `scrollTo` names, once for each token, and tells the page, with `onTrack`, which tracked child the view has
    /// scrolled to when that changes. Only the children's tops are read, so it never lays anything out again.
    /// `edges`, for a following scroll, first keeps its view in place and asks for more at its ends (`keep_in_place`).
    pub(super) fn scroll_watch(
        state: Rc<FollowScroll>,
        edges: Option<Edges>,
        to: Option<(usize, u64)>,
        track: Vec<usize>,
        on_track: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl Fn(Vec<gpui::Bounds<Pixels>>, &mut Window, &mut gpui::App) + 'static {
        let entity = cx.entity();
        move |children, window, cx| {
            let handle = &state.handle;
            let view = handle.bounds();
            if view.size.height <= px(0.0) {
                // Not laid out yet: look again next frame.
                window.refresh();
                return;
            }
            if let Some(edges) = &edges
                && Self::keep_in_place(&state, edges, &children, window, &entity, cx)
            {
                return;
            }
            if let Some((index, token)) = to.filter(|(_, token)| state.jumped.get() != *token)
                && let Some(child) = children.get(index)
            {
                state.jumped.set(token);
                let (max, offset) = (handle.max_offset().y, handle.offset());
                // The offset is how far the content moved up, so it is minus what the child is below the view's top.
                let wanted = offset.y + view.top() - child.top();
                handle.set_offset(point(offset.x, wanted.max(-max).min(px(0.0))));
                // Drawn again from the page, which draws the children at the new place in a windowed column.
                window.request_animation_frame();
                return;
            }
            let Some(handler) = on_track.as_ref() else {
                return;
            };
            // The last tracked child whose top is above a line a third of the way down the view, as a reader's eye is; at the
            // end of the scroll, the last one in view, since a short last child never reaches that line. A point of slack
            // for rounding.
            let at_end = handle.offset().y <= -handle.max_offset().y + px(1.0);
            let line = if at_end { view.bottom() } else { view.top() + view.size.height / 3.0 } + px(1.0);
            let reached = track.partition_point(|index| children.get(*index).is_some_and(|child| child.top() <= line));
            let at = reached.checked_sub(1);
            if state.tracked.get() == Some(at) {
                return;
            }
            state.tracked.set(Some(at));
            let handler = handler.clone();
            let entity = entity.clone();
            cx.defer(move |cx| entity.update(cx, |view, _| view.send(&handler, Some(&serde_json::json!({ "at": at })))));
        }
    }

    /// For a following scroll's column, with the bounds of its children: when a new tree adds or takes away children
    /// above or below what was in view, moves the view so the first of those children still there is where it was, as a
    /// list too long to send whole is moved along. Then asks the page for older items within a screen of the top
    /// (`onReachStart`) and newer ones within a screen of the end (`onReachEnd`). Says whether it moved the view, which
    /// this frame then drew in the wrong place.
    pub(super) fn keep_in_place(state: &FollowScroll, edges: &Edges, children: &[gpui::Bounds<Pixels>], window: &mut Window, entity: &gpui::Entity<Self>, cx: &mut gpui::App) -> bool {
        let handle = &state.handle;
        let view = handle.bounds();
        let (max, offset) = (handle.max_offset().y, handle.offset());
        let before = state.max.replace(max);
        let mut anchor = state.anchor.borrow_mut();
        if anchor.tree != edges.tree {
            anchor.tree = edges.tree;
            // Newer items on their way aren't followed to (`follow_end`), so the view is kept on what it showed.
            let following = edges.end.is_none() && !state.asked_end.get() && -offset.y >= max - px(FOLLOW_SLACK);
            let seen = std::mem::take(&mut anchor.seen);
            // How far down the first child still here has gone: by what was added above it, less what was taken away.
            let moved = seen.iter().find_map(|(id, below)| {
                let at = edges.ids.iter().position(|el| *el == Some(*id))?;
                Some(children.get(at)?.top() - view.top() - *below)
            });
            let Some(moved) = moved else {
                // Nothing of what was in view is left to keep in place, so what was asked for has come.
                state.asked.set(false);
                state.asked_end.set(false);
                return false;
            };
            if moved > px(0.0) {
                state.asked.set(false);
            }
            if max - before - moved > px(0.0) {
                state.asked_end.set(false);
            }
            // At the newest end the scroll follows it instead.
            if !following && moved.abs() > px(0.5) {
                handle.set_offset(point(offset.x, offset.y - moved));
                // `refresh` does nothing while the window draws. Older items that came in a windowed column aren't drawn,
                // so no rise of theirs asks for the next frame either.
                window.request_animation_frame();
                return true;
            }
        }
        anchor.seen = children
            .iter()
            .zip(&edges.ids)
            // An empty child, like a column's first one that leaves a gap above the messages, stays where it is as older
            // ones come in above what is in view: kept in place by it, the view never moved.
            .filter(|(child, _)| child.size.height > px(0.0))
            .skip_while(|(child, _)| child.bottom() <= view.top())
            .take_while(|(child, _)| child.top() < view.bottom())
            .filter_map(|(child, id)| Some(((*id)?, child.top() - view.top())))
            .take(8)
            .collect();
        drop(anchor);
        let height = view.size.height;
        let mut ask = |handler: &String| {
            let (handler, entity) = (handler.clone(), entity.clone());
            cx.defer(move |cx| entity.update(cx, |view, _| view.send(&handler, None)));
        };
        match &edges.start {
            Some(handler) if !state.asked.get() && -offset.y <= height => {
                state.asked.set(true);
                ask(handler);
            }
            Some(_) => {}
            None => state.asked.set(false),
        }
        match &edges.end {
            Some(handler) if !state.asked_end.get() && max + offset.y <= height => {
                state.asked_end.set(true);
                ask(handler);
            }
            Some(_) => {}
            None => state.asked_end.set(false),
        }
        false
    }

    /// Asks the page for older items when `area` is scrolled within a screen of its top, and keeps the view on what
    /// it showed when they arrive. It runs once the area is laid out, so it sees how far it scrolls now.
    /// `handler` is `None` once there is nothing older to ask for, which is also how the last older items arrive.
    pub(super) fn reach_start(state: Rc<FollowScroll>, handler: Option<String>, cx: &mut Context<Self>) -> impl Fn(Vec<gpui::Bounds<Pixels>>, &mut Window, &mut gpui::App) + 'static {
        let entity = cx.entity();
        move |_, window, cx| {
            let handle = &state.handle;
            let (max, offset) = (handle.max_offset().y, handle.offset());
            let grew = max - state.max.replace(max);
            let at_end = -offset.y >= max - px(FOLLOW_SLACK);
            // Only a request brings older items, and what a request brings is above the view: moving the offset by
            // what it added keeps the same lines under the pointer. At its end the scroll follows on its own.
            if grew > px(0.0) && state.asked.get() {
                state.asked.set(false);
                if !at_end {
                    handle.set_offset(point(offset.x, offset.y - grew));
                    // This frame drew the content shifted, so draw once more with it back in place.
                    window.refresh();
                }
                return;
            }
            let Some(handler) = handler.as_ref() else {
                // Nothing older to ask for now, so the next time there is, it is asked for afresh.
                state.asked.set(false);
                return;
            };
            let height = handle.bounds().size.height;
            if state.asked.get() || height <= px(0.0) || -offset.y > height {
                return;
            }
            state.asked.set(true);
            let handler = handler.clone();
            let entity = entity.clone();
            cx.defer(move |cx| entity.update(cx, |view, _| view.send(&handler, None)));
        }
    }
}
