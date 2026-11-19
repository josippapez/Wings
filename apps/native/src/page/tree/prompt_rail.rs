//! The `PromptRail` a chat draws beside its conversation: a column of short ticks, one for each prompt, with the one in
//! view long and its neighbours stepping down. The page sends the words of each prompt and which one is current; this
//! lays the ticks out and says which one the pointer is on, so hovering never reaches the page.

use serde_json::{Map, Value};

use super::{number, str_prop, valid_handler};

/// The most marks a page may send; the rest are dropped.
pub const MAX_MARKS: usize = 2_000;
/// The longest title and preview kept, in characters.
pub const TITLE_CHARS: usize = 200;
pub const PREVIEW_CHARS: usize = 400;

/// How far apart ticks are when there is room, and the closest they get before the rail scrolls.
pub const PITCH_MAX: f32 = 20.0;
pub const PITCH_MIN: f32 = 12.0;
/// Room above the first tick and below the last.
pub const PAD_Y: f32 = 8.0;
/// Room left of the ticks.
pub const PAD_X: f32 = 8.0;
pub const TICK_HEIGHT: f32 = 2.0;
/// A tick's length by its distance from the one in view (or under the pointer): 0, 1, 2 and the rest.
pub const LENGTHS: [f32; 4] = [50.0, 40.0, 24.0, 12.0];
/// The rows kept in view above and below the current tick when the rail scrolls.
const KEEP: f32 = 2.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    /// The prompt's first line.
    pub title: String,
    /// The start of Claude's reply to it.
    pub preview: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PromptRail {
    pub id: u64,
    pub marks: Vec<Mark>,
    /// The mark of the prompt in view.
    pub current: usize,
    /// Gets `{ index }` on a click on a tick or the card.
    pub on_select: Option<String>,
    /// `beside`: the width of the centred column the rail sits against, at its left, over the page beside it rather than
    /// in a pane of its own. Left out where the page has no room for it there.
    pub beside: Option<f32>,
}

fn clip(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

pub(super) fn parse(id: u64, props: &Map<String, Value>) -> PromptRail {
    let marks: Vec<Mark> = props
        .get("marks")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .take(MAX_MARKS)
                .map(|mark| {
                    let words = |name, chars| {
                        mark.as_object()
                            .and_then(|mark| str_prop(mark, name))
                            .map(|text| clip(text, chars))
                            .unwrap_or_default()
                    };
                    Mark {
                        title: words("title", TITLE_CHARS),
                        preview: words("preview", PREVIEW_CHARS),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let current = number(props, "current")
        .filter(|at| *at >= 0.0)
        .map_or(0, |at| at as usize)
        .min(marks.len().saturating_sub(1));
    PromptRail {
        id,
        marks,
        current,
        on_select: str_prop(props, "onSelect")
            .filter(|handler| valid_handler(handler))
            .map(str::to_owned),
        beside: number(props, "beside").filter(|width| width.is_finite() && *width > 0.0).map(|width| width.min(4000.0) as f32),
    }
}

/// How far apart the ticks are in a rail `height` tall holding `count`: 20 points when they fit, closer down to 12 when
/// they don't, and then the rail scrolls rather than cram them in.
pub fn pitch(count: usize, height: f32) -> f32 {
    if count == 0 {
        return PITCH_MAX;
    }
    ((height - 2.0 * PAD_Y) / count as f32).clamp(PITCH_MIN, PITCH_MAX)
}

/// How tall the ticks make the rail's content.
pub fn content_height(count: usize, pitch: f32) -> f32 {
    2.0 * PAD_Y + count as f32 * pitch
}

/// How far the content can be scrolled.
pub fn max_scroll(count: usize, pitch: f32, height: f32) -> f32 {
    (content_height(count, pitch) - height).max(0.0)
}

/// The top of tick `index`'s row, in the content.
pub fn row_top(index: usize, pitch: f32) -> f32 {
    PAD_Y + index as f32 * pitch
}

/// The tick under `y` (from the rail's top) with the content scrolled by `scroll`.
pub fn hit(y: f32, scroll: f32, count: usize, pitch: f32) -> Option<usize> {
    let row = ((y + scroll - PAD_Y) / pitch).floor();
    (row >= 0.0 && (row as usize) < count).then_some(row as usize)
}

/// A tick's length: by its distance from the current one or the hovered one, whichever is nearer.
pub fn length(index: usize, current: usize, hover: Option<usize>) -> f32 {
    let near = |at: usize| index.abs_diff(at).min(LENGTHS.len() - 1);
    let distance = hover.map_or(near(current), |hover| near(current).min(near(hover)));
    LENGTHS[distance]
}

/// `length` for a current tick that is between marks (`focus`, easing from one to the next): the lengths blend by the
/// fractional distance, so a tick lengthens and shortens smoothly as the long one passes.
pub fn length_near(index: usize, focus: f32, hover: Option<usize>) -> f32 {
    let top = (LENGTHS.len() - 1) as f32;
    let near = |at: f32| (index as f32 - at).abs().min(top);
    let distance = hover.map_or(near(focus), |hover| near(focus).min(near(hover as f32)));
    let low = distance.floor() as usize;
    let high = (low + 1).min(LENGTHS.len() - 1);
    LENGTHS[low] + (LENGTHS[high] - LENGTHS[low]) * (distance - low as f32)
}

/// How much tick `index` is the current one while `focus` passes: 1 on it, 0 a mark or more away.
pub fn weight(index: usize, focus: f32) -> f32 {
    (1.0 - (index as f32 - focus).abs()).clamp(0.0, 1.0)
}

/// The scroll that keeps `current` in view, a couple of rows from the edge, starting from `scroll`.
pub fn keep_in_view(scroll: f32, current: usize, count: usize, pitch: f32, height: f32) -> f32 {
    let (top, bottom) = (
        row_top(current, pitch) - KEEP * pitch,
        row_top(current, pitch) + (1.0 + KEEP) * pitch,
    );
    let scroll = if top < scroll { top } else { scroll };
    let scroll = if bottom > scroll + height {
        bottom - height
    } else {
        scroll
    };
    scroll.clamp(0.0, max_scroll(count, pitch, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn props(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn marks_are_parsed_cut_and_capped() {
        let many: Vec<Value> = (0..MAX_MARKS + 50).map(|n| json!({ "title": format!("P{n}"), "preview": "r" })).collect();
        let rail = parse(3, &props(json!({ "marks": many, "current": 5, "onSelect": "3:onSelect" })));
        assert_eq!((rail.marks.len(), rail.current), (MAX_MARKS, 5));
        assert_eq!(rail.on_select.as_deref(), Some("3:onSelect"));
        let long = parse(3, &props(json!({ "marks": [{ "title": "t".repeat(900), "preview": "p".repeat(900) }, 7], "current": 99 })));
        assert_eq!((long.marks[0].title.len(), long.marks[0].preview.len()), (TITLE_CHARS, PREVIEW_CHARS));
        assert_eq!(long.marks[1], Mark { title: String::new(), preview: String::new() });
        assert_eq!(long.current, 1, "a current past the last is the last");
        assert_eq!(parse(3, &props(json!({ "onSelect": "bad handler" }))).on_select, None);
        assert_eq!(parse(3, &props(json!({ "beside": 760 }))).beside, Some(760.0));
        assert_eq!(parse(3, &props(json!({ "beside": -1 }))).beside, None);
        assert_eq!(parse(3, &props(json!({}))).beside, None);
        assert!(parse(3, &props(json!({}))).marks.is_empty());
    }

    #[test]
    fn the_current_tick_is_long_and_its_neighbours_step_down() {
        let lengths: Vec<f32> = (0..6).map(|at| length(at, 1, None)).collect();
        assert_eq!(lengths, [40.0, 50.0, 40.0, 24.0, 12.0, 12.0]);
        // A hovered tick lengthens the same way, and the current one stays long.
        assert_eq!(length(5, 1, Some(5)), 50.0);
        assert_eq!(length(4, 1, Some(5)), 40.0);
        assert_eq!(length(1, 1, Some(5)), 50.0);
    }

    #[test]
    fn the_long_tick_blends_between_marks_and_matches_length_on_them() {
        for at in 0..6 {
            for focus in 0..6 {
                assert_eq!(length_near(at, focus as f32, None), length(at, focus, None));
                assert_eq!(length_near(at, focus as f32, Some(5)), length(at, focus, Some(5)));
            }
        }
        // Half way from mark 1 to 2: both are between long and step-down, so neither jumps.
        assert_eq!(length_near(1, 1.5, None), 45.0);
        assert_eq!(length_near(2, 1.5, None), 45.0);
        assert_eq!((weight(1, 1.0), weight(1, 1.5), weight(1, 2.5)), (1.0, 0.5, 0.0));
    }

    #[test]
    fn ticks_sit_20_points_apart_and_close_up_to_6_before_the_rail_scrolls() {
        assert_eq!(pitch(10, 600.0), 20.0);
        assert_eq!(pitch(40, 600.0), (600.0 - 2.0 * PAD_Y) / 40.0);
        assert_eq!(pitch(500, 600.0), 12.0);
        assert_eq!(max_scroll(10, 20.0, 600.0), 0.0);
        assert_eq!(max_scroll(500, 6.0, 600.0), 2.0 * PAD_Y + 3000.0 - 600.0);
    }

    #[test]
    fn a_point_is_on_the_tick_whose_row_holds_it() {
        assert_eq!(hit(PAD_Y + 1.0, 0.0, 10, 20.0), Some(0));
        assert_eq!(hit(PAD_Y + 41.0, 0.0, 10, 20.0), Some(2));
        assert_eq!(hit(2.0, 0.0, 10, 20.0), None);
        assert_eq!(hit(PAD_Y + 10.0 * 20.0 + 1.0, 0.0, 10, 20.0), None);
        assert_eq!(hit(PAD_Y + 1.0, 100.0, 40, 6.0), Some(16));
    }

    #[test]
    fn the_rail_scrolls_to_keep_the_current_tick_in_view() {
        let (count, pitch, height) = (500, 6.0, 400.0);
        let at = keep_in_view(0.0, 300, count, pitch, height);
        let top = row_top(300, pitch);
        assert!(top >= at && top + pitch <= at + height, "{at}");
        // Already in view: it stays where it is.
        assert_eq!(keep_in_view(at, 300, count, pitch, height), at);
        // Back near the start: scrolled to the top, never past it.
        assert_eq!(keep_in_view(at, 0, count, pitch, height), 0.0);
        assert!(keep_in_view(0.0, 499, count, pitch, height) <= max_scroll(count, pitch, height));
    }
}
