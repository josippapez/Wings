//! A bounded window of graph data over a virtual list of the complete history.
use super::*;
use gpui::{PathBuilder, ScrollStrategy, UniformListScrollHandle, canvas, point, uniform_list};
use std::ops::Range;

#[derive(Default)]
pub(super) struct GraphState {
    scroll: UniformListScrollHandle,
    wanted: Range<usize>,
    sent: Option<Range<usize>>,
    pending: bool,
    cursor: usize,
    focus: Option<gpui::FocusHandle>,
    pub(super) columns: Columns,
}

/// Which of a row's fixed columns fit the graph's width. A sidebar is 250 to 640 points wide, and the author's 110 and
/// the date left the subject next to nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Columns {
    #[default]
    All,
    NoAuthor,
    NoAuthorOrDate,
}

impl Columns {
    fn fit(width: f32) -> Self {
        if width >= 520.0 {
            Self::All
        } else if width >= 380.0 {
            Self::NoAuthor
        } else {
            Self::NoAuthorOrDate
        }
    }
}

fn window_range(visible: Range<usize>, total: usize) -> Range<usize> {
    let start = visible.start.saturating_sub(12);
    start..visible.end.saturating_add(12).min(total).min(start + 256)
}

fn navigate(key: &str, current: usize, total: usize, page: usize) -> Option<usize> {
    if total == 0 {
        return None;
    }
    Some(match key {
        "down" => (current + 1).min(total - 1),
        "up" => current.saturating_sub(1),
        "pagedown" => current.saturating_add(page).min(total - 1),
        "pageup" => current.saturating_sub(page),
        "home" => 0,
        "end" => total - 1,
        _ => return None,
    })
}

impl PageView {
    pub(super) fn graph(&self, graph: &tree::Graph, cx: &mut Context<Self>) -> AnyElement {
        let id = graph.id;
        let (handle, focus) = {
            let mut graphs = self.graphs.borrow_mut();
            let state = graphs.entry(id).or_default();
            (
                state.scroll.clone(),
                state
                    .focus
                    .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
                    .clone(),
            )
        };
        let list_focus = focus.clone();
        let graph = graph.clone();
        let keyboard_graph = graph.clone();
        let entity = cx.entity();
        let list = uniform_list(ElementId::NamedInteger("page-graph-list".into(), id), graph.total, move |range, window, cx| {
            entity.update(cx, |view, cx| {
                let mut graphs = view.graphs.borrow_mut();
                let state = graphs.entry(id).or_default();
                state.wanted = window_range(range.clone(), graph.total);
                if !state.pending {
                    state.pending = true;
                    let handler = graph.on_range.clone();
                    // uniform_list measures one row before requesting the actual viewport. Send only the
                    // last range of the layout pass, or that measurement would request row zero on every scroll.
                    let entity = cx.entity();
                    cx.defer(move |cx| entity.update(cx, |view, _| {
                        let request = {
                            let mut graphs = view.graphs.borrow_mut();
                            let state = graphs.get_mut(&id).unwrap();
                            state.pending = false;
                            if state.sent.as_ref() == Some(&state.wanted) { None } else {
                                state.sent = Some(state.wanted.clone());
                                Some(state.wanted.clone())
                            }
                        };
                        if let (Some(handler), Some(range)) = (handler, request) {
                            view.send(&handler, Some(&serde_json::json!({ "start": range.start, "end": range.end })));
                        }
                    }));
                }
                drop(graphs);
                range.map(|index| view.graph_row(&graph, index, list_focus.is_focused(window), cx)).collect::<Vec<_>>()
            })
        })
        .track_scroll(&handle)
        .flex_1().min_h_0().w_full();
        // Draws again only when the width crosses into other columns, not on every frame of a sidebar drag.
        let entity = cx.entity();
        let measure = canvas(
            move |bounds, _, cx| {
                let fit = Columns::fit(bounds.size.width.as_f32());
                entity.update(cx, |view, cx| {
                    let changed = {
                        let mut graphs = view.graphs.borrow_mut();
                        let state = graphs.entry(id).or_default();
                        std::mem::replace(&mut state.columns, fit) != fit
                    };
                    if changed {
                        cx.notify();
                    }
                })
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div()
            .id(ElementId::NamedInteger("page-graph".into(), id))
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .child(measure)
            .child(list)
            .role(Role::ListBox)
            .aria_label("Commits")
            .track_focus(&focus)
            .tab_stop(true)
            .border_1()
            .border_color(rgba(0))
            .focus_visible(|s| s.border_color(rgb(theme::FOREGROUND)))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                let mut graphs = view.graphs.borrow_mut();
                let state = graphs.entry(id).or_default();
                let current = state.cursor.min(keyboard_graph.total.saturating_sub(1));
                let key = event.keystroke.key.as_str();
                if let Some(index) = navigate(
                    key,
                    current,
                    keyboard_graph.total,
                    state.wanted.len().saturating_sub(24).max(1),
                ) {
                    state.cursor = index;
                    state.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
                    drop(graphs);
                    cx.notify();
                } else if matches!(key, "enter" | "space") {
                    drop(graphs);
                    if let Some(handler) = &keyboard_graph.on_select {
                        view.act(handler, Some(&serde_json::json!({ "index": current })));
                    }
                } else {
                    return;
                }
                window.prevent_default();
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    fn graph_row(
        &self,
        graph: &tree::Graph,
        index: usize,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (cursor, columns) = self
            .graphs
            .borrow()
            .get(&graph.id)
            .map(|s| (s.cursor, s.columns))
            .unwrap_or_default();
        let row = index
            .checked_sub(graph.offset)
            .and_then(|i| graph.rows.get(i));
        let element = div()
            .id(ElementId::NamedInteger(
                format!("graph-{}-row", graph.id).into(),
                index as u64,
            ))
            .role(Role::ListBoxOption)
            .aria_selected(index as i64 == graph.selected)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(graph.total)
            .when(index == cursor, |row| row.aria_active_descendant())
            .h(px(26.0))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(8.0))
            .pr(px(12.0))
            .text_size(px(12.0))
            .overflow_hidden()
            .when(
                (focused && index == cursor) || index as i64 == graph.selected,
                |el| el.bg(rgba(theme::HOVER_STRONG)),
            )
            .hover(|s| s.bg(rgba(theme::HOVER)));
        let Some(row) = row else {
            return element.child("Loading…").into_any_element();
        };
        let graph_width = 20.0 + graph.lanes as f32 * 14.0;
        let geometry = row.clone();
        let selected = index as i64 == graph.selected;
        let lanes = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let at = |lane, y| {
                    point(
                        bounds.left() + px(16.0 + lane as f32 * 14.0),
                        bounds.top() + px(y),
                    )
                };
                for line in geometry.lines.as_chunks::<4>().0 {
                    let [a, b, part, color] = [line[0], line[1], line[2], line[3]];
                    if a >= 24 && b >= 24 {
                        continue;
                    }
                    let (ya, yb) = match part {
                        0 => (0.0, 13.0),
                        1 => (13.0, 26.0),
                        _ => (0.0, 26.0),
                    };
                    let mut path = PathBuilder::stroke(px(1.75));
                    path.move_to(at(a, ya));
                    if a == b {
                        path.line_to(at(b, yb));
                    } else {
                        path.cubic_bezier_to(
                            at(b, yb),
                            at(a, (ya + yb) / 2.0),
                            at(b, (ya + yb) / 2.0),
                        );
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, rgb(palette(color)));
                    }
                }
                if geometry.lane < 24 {
                    let center = at(geometry.lane, 13.0);
                    let outer = if selected { 5.0 } else { 4.0 };
                    let radius = if geometry.merge {
                        (outer + 2.0) / 2.0
                    } else {
                        outer
                    };
                    let mut path = if geometry.merge {
                        PathBuilder::stroke(px(outer - 2.0))
                    } else {
                        PathBuilder::fill()
                    };
                    for step in 0..=32 {
                        let angle = step as f32 / 32.0 * std::f32::consts::TAU;
                        let point = point(
                            center.x + px(radius * angle.cos()),
                            center.y + px(radius * angle.sin()),
                        );
                        if step == 0 {
                            path.move_to(point);
                        } else {
                            path.line_to(point);
                        }
                    }
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(path, rgb(palette(geometry.color)));
                    }
                }
            },
        )
        .w(px(graph_width))
        .h(px(26.0))
        .flex_none();
        let refs = row
            .refs
            .iter()
            .map(|value| {
                let color = match value.chars().next() {
                    Some('H') => Some(theme::INFO),
                    Some('b') => Some(theme::DONE),
                    Some('t') => Some(theme::WORKING),
                    _ => None,
                };
                let name = value.get(2..).unwrap_or(value);
                // The changes plugin's `describeRef`: a tag's name after a diamond.
                let label = if value.starts_with('t') {
                    format!("◆ {name}")
                } else {
                    name.to_owned()
                };
                ui::badge(ui::Variant::Outline, color, true, label).into_any_element()
            })
            .collect::<Vec<_>>();
        let handler = graph.on_select.clone();
        let id = graph.id;
        element
            .aria_label(format!("{}, {}, {}", row.subject, row.hash, row.author))
            .cursor_pointer()
            .child(lanes)
            .children(refs)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(row.subject.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(11.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(row.hash.chars().take(7).collect::<String>()),
            )
            .when(columns == Columns::All, |element| {
                element.child(
                    div()
                        .w(px(110.0))
                        .flex_none()
                        .truncate()
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child(row.author.clone()),
                )
            })
            .when(columns != Columns::NoAuthorOrDate, |element| {
                element.child(
                    div()
                        .flex_none()
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child(row.date.clone()),
                )
            })
            .on_click(cx.listener(move |view, _, window, cx| {
                let focus = {
                    let mut graphs = view.graphs.borrow_mut();
                    let state = graphs.entry(id).or_default();
                    state.cursor = index;
                    state.focus.clone()
                };
                if let Some(focus) = focus {
                    focus.focus(window, cx);
                }
                if let Some(handler) = &handler {
                    view.act(handler, Some(&serde_json::json!({ "index": index })));
                }
                cx.notify();
            }))
            .into_any_element()
    }
}

fn palette(index: usize) -> u32 {
    [
        theme::INFO,
        theme::DONE,
        theme::WORKING,
        theme::DANGER,
        0xc4a7ff,
        0x6fd3d0,
        0xf0a3d0,
        0xb9cf6e,
    ][index % 8]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ten_thousand_rows_request_only_a_bounded_viewport() {
        assert_eq!(window_range(5_000..5_030, 10_000), 4_988..5_042);
        assert_eq!(window_range(0..10_000, 10_000).len(), 256);
        assert_eq!(window_range(9_990..10_000, 10_000), 9_978..10_000);
    }
    #[test]
    fn navigation_clamps_and_pages() {
        assert_eq!(navigate("end", 0, 10_000, 20), Some(9_999));
        assert_eq!(navigate("down", 9_999, 10_000, 20), Some(9_999));
        assert_eq!(navigate("pageup", 12, 10_000, 20), Some(0));
        assert_eq!(navigate("pagedown", 12, 10_000, 20), Some(32));
        assert_eq!(navigate("home", 500, 10_000, 20), Some(0));
    }
    #[test]
    fn a_sidebar_dragged_from_widest_to_narrowest_changes_columns_twice() {
        let mut seen = vec![Columns::fit(640.0)];
        for width in (250..640).rev() {
            let columns = Columns::fit(width as f32);
            if seen.last() != Some(&columns) {
                seen.push(columns);
            }
        }
        assert_eq!(seen, [Columns::All, Columns::NoAuthor, Columns::NoAuthorOrDate]);
    }
}
