//! The `Code` a page shows: its lines in a card, as one text, coloured with the diff viewer's highlighter when the page
//! names a language it has a grammar for. Highlighting never runs on the window's thread: a block is coloured once on a
//! background thread and kept by what it says, so a page that renders again with the same code costs a lookup. A block
//! that is still being coloured, or that grew by a token since, shows the last colours it had where its lines still match,
//! so code streaming in doesn't flash plain on every token.
use std::{
    collections::{HashMap, VecDeque},
    hash::{DefaultHasher, Hash, Hasher},
    ops::Range,
    sync::{Arc, Mutex},
};

use gpui::{FontStyle, HighlightStyle, StyledText};

use super::*;
use crate::diff::highlight::{self, FOREGROUND, Span};

/// What is kept: blocks by what they say, and the bytes of their text. The oldest go first.
const KEPT_BLOCKS: usize = 256;
const KEPT_BYTES: usize = 16 * 1024 * 1024;
/// The most blocks whose last colours are remembered for the next render of the same node, across pages.
const KEPT_SHOWN: usize = 1024;

/// A block's text and the colours of its lines: none for a language with no grammar.
pub(super) struct Highlighted {
    text: String,
    lines: Vec<Vec<Span>>,
}

#[derive(Default)]
struct Cache {
    done: HashMap<u64, Arc<Highlighted>>,
    order: VecDeque<u64>,
    bytes: usize,
    /// The newest colours a node of a page got, by the page and the node: what it shows while newer ones are made.
    shown: HashMap<(u64, u64), Arc<Highlighted>>,
}

impl Cache {
    fn put(&mut self, key: u64, page: (u64, u64), found: Arc<Highlighted>) {
        self.bytes += found.text.len();
        if self.done.insert(key, found.clone()).is_some() {
            self.bytes -= found.text.len();
        } else {
            self.order.push_back(key);
        }
        while self.order.len() > KEPT_BLOCKS || self.bytes > KEPT_BYTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if oldest != key
                && let Some(gone) = self.done.remove(&oldest)
            {
                self.bytes -= gone.text.len();
            }
        }
        if self.shown.len() >= KEPT_SHOWN {
            self.shown.clear();
        }
        self.shown.insert(page, found);
    }
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn with_cache<T>(f: impl FnOnce(&mut Cache) -> T) -> T {
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(cache.get_or_insert_with(Cache::default))
}

fn key(language: &str, text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    language.hash(&mut hasher);
    text.hash(&mut hasher);
    hasher.finish()
}

/// `text` coloured as `language`. Slow on a long block.
fn colour(language: &str, text: &str) -> Highlighted {
    let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    Highlighted {
        text: text.to_owned(),
        lines: highlight::language_colors(language, &lines),
    }
}

/// The stretches of `text` that have a colour of their own, as (bytes, colour, italic), from `found`: its colours for each
/// line that still reads as it did when they were made. Text in the plain colour is left out.
fn runs(text: &str, found: &Highlighted) -> Vec<(Range<usize>, u32, bool)> {
    let mut out = Vec::new();
    let mut before = found.text.split('\n');
    let mut start = 0;
    for (at, line) in text.split('\n').enumerate() {
        let same = before.next() == Some(line);
        if let (true, Some(spans)) = (same, found.lines.get(at)) {
            let mut from = 0;
            for span in spans {
                let end = span.end.min(line.len());
                if (span.color != FOREGROUND || span.italic) && end > from {
                    out.push((start + from..start + end, span.color, span.italic));
                }
                from = end.max(from);
            }
        }
        start += line.len() + 1;
    }
    out
}

impl PageView {
    pub(super) fn code(&self, code: &tree::Code, cx: &mut Context<Self>) -> AnyElement {
        let found = code
            .language
            .as_deref()
            .and_then(|language| self.coloured(code.id, language, &code.text, cx));
        // An empty block still takes its line.
        let text = if code.text.is_empty() {
            " "
        } else {
            code.text.as_str()
        };
        let colours: Vec<(Range<usize>, HighlightStyle)> = found.map_or_else(Vec::new, |found| {
            runs(text, &found)
                .into_iter()
                .map(|(range, color, italic)| {
                    let style = HighlightStyle {
                        color: Some(rgb(color).into()),
                        font_style: italic.then_some(FontStyle::Italic),
                        ..Default::default()
                    };
                    (range, style)
                })
                .collect()
        });
        let id = code.id;
        let size = px(scaled(12.0));
        let system = cx.text_system();
        let column = system.ch_advance(system.resolve_font(&gpui::font(DEFAULT_FONT_FAMILY)), size).unwrap_or(size * 0.6);
        // Each source line on its own, its indent drawn as a margin, so a line that wraps goes on under its first
        // character and not at the block's left edge. Each line selects as a line of its own, and a drag runs over them.
        let mut start = 0;
        let mut rows = Vec::new();
        for (at, line) in text.split('\n').enumerate() {
            let lead = line.len() - line.trim_start_matches([' ', '\t']).len();
            let columns: usize = line[..lead].chars().map(|c| if c == '\t' { 4 } else { 1 }).sum();
            let from = start + lead;
            let shown = if lead == line.len() { " " } else { &line[lead..] };
            let colours = colours
                .iter()
                .filter_map(|(range, style)| {
                    let (a, b) = (range.start.max(from), range.end.min(start + line.len()));
                    (a < b).then(|| (a - from..b - from, *style))
                })
                .collect();
            let line_id = super::selection_view::line_id(id, at);
            let marks = super::selection_view::with_selection(colours, self.selection_in(line_id, shown));
            let styled = StyledText::new(shown.to_owned()).with_highlights(marks);
            let layout = styled.layout().clone();
            let row = self.selectable_after(line_id, shown.to_owned(), line[..lead].to_owned(), layout, styled.into_any_element(), cx);
            rows.push(div().pl(column * columns as f32).child(row).into_any_element());
            start += line.len() + 1;
        }
        let lines = div()
            .min_w_0()
            .font_family(DEFAULT_FONT_FAMILY)
            .text_size(size)
            .children(rows);
        let card = match code.tone {
            // A run of a diff's lines: a tinted band with a bar in the tone's colour, for the card round the diff to hold.
            Some(tone) => {
                let color = tone_color(tone);
                lines
                    .px(px(8.0))
                    .py(px(2.0))
                    .border_l_2()
                    .border_color(rgb(color))
                    .bg(rgba(color << 8 | 0x1a))
            }
            // Unchanged lines between them: the same inset with no tint, so the text lines up with the bands'.
            None if code.band => lines.px(px(8.0)).py(px(2.0)).border_l_2().border_color(gpui::transparent_black()),
            None => ui::plain_card([lines.into_any_element()]),
        };
        // A right press tells the `Stack` round it with an `onContextMenu`, which asks after this, what it hit.
        card.on_mouse_down(
            gpui::MouseButton::Right,
            cx.listener(move |view, event: &gpui::MouseDownEvent, _, _| view.record_code_hit(id, event.position)),
        )
        .into_any_element()
    }

    /// The colours for `text` as `language`, or the ones this node last had while new ones are made on a thread of
    /// their own, and the page draws again when they are.
    fn coloured(
        &self,
        id: u64,
        language: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Option<Arc<Highlighted>> {
        let page = (cx.entity_id().as_u64(), id);
        let key = key(language, text);
        let found = with_cache(|cache| match cache.done.get(&key).cloned() {
            Some(found) => {
                cache.shown.insert(page, found.clone());
                Ok(found)
            }
            None => Err(cache.shown.get(&page).cloned()),
        });
        let stale = match found {
            Ok(found) => return Some(found),
            Err(stale) => stale,
        };
        // A node's ids are the page's own, so the set that waits on pictures can say a block is waiting too. One job runs
        // for a node at a time, and the next render after it starts another for what the text has become.
        if self.images_waiting.borrow_mut().insert(id) {
            let (language, text) = (language.to_owned(), text.to_owned());
            cx.spawn(async move |view, cx| {
                let found = cx
                    .background_executor()
                    .spawn(async move { colour(&language, &text) })
                    .await;
                with_cache(|cache| cache.put(key, page, Arc::new(found)));
                let _ = view.update(cx, |view, cx| {
                    view.images_waiting.borrow_mut().remove(&id);
                    cx.notify();
                });
            })
            .detach();
        }
        stale
    }

    /// The colour of each stretch of the first line of `text` as `language` once it has been coloured, for the tests.
    #[cfg(all(test, feature = "test-support"))]
    pub(in crate::page) fn first_line_colours(language: &str, text: &str) -> Option<Vec<u32>> {
        let found = with_cache(|cache| cache.done.get(&key(language, text)).cloned())?;
        Some(
            found
                .lines
                .first()
                .map(|spans| spans.iter().map(|span| span.color).collect())
                .unwrap_or_default(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Highlighted {
        colour("rust", text)
    }

    #[test]
    fn rust_is_coloured_by_stretch_and_the_plain_colour_is_left_out() {
        let text = "let a = 1;";
        let runs = runs(text, &found(text));
        assert!(!runs.is_empty());
        // The ranges are in order, inside the text, and none is plain.
        let mut end = 0;
        for (range, color, _) in &runs {
            assert!(range.start >= end && range.end <= text.len(), "{range:?}");
            assert_ne!(*color, FOREGROUND);
            end = range.end;
        }
        // The keyword and the number differ.
        let at = |word: &str| {
            runs.iter()
                .find(|(range, ..)| &text[range.clone()] == word)
                .map(|run| run.1)
        };
        assert!(at("let").is_some() && at("1").is_some() && at("let") != at("1"));
    }

    #[test]
    fn a_language_without_a_grammar_has_no_colours() {
        let plain = colour("no-such-language", "some words");
        assert!(plain.lines.is_empty());
        assert!(runs("some words", &plain).is_empty());
    }

    #[test]
    fn a_block_that_grew_keeps_the_colours_of_the_lines_it_still_has() {
        let old = found("let a = 1;\nlet b = 2;");
        let grown = "let a = 1;\nlet b = 2;\nlet c = 3;";
        let kept = runs(grown, &old);
        // The first two lines have their colours, the new line has none yet.
        assert!(
            kept.iter()
                .all(|(range, ..)| range.end <= "let a = 1;\nlet b = 2;".len())
        );
        assert!(
            kept.iter()
                .any(|(range, ..)| range.start > "let a = 1;\n".len())
        );
        // A line that was edited loses its colours rather than keeping wrong ones.
        let edited = runs("let a = 9;\nlet b = 2;", &old);
        assert!(
            edited
                .iter()
                .all(|(range, ..)| range.start >= "let a = 9;\n".len())
        );
    }

    #[test]
    fn the_cache_forgets_the_oldest_block_and_keeps_the_newest() {
        let mut cache = Cache::default();
        for n in 0..(KEPT_BLOCKS as u64 + 5) {
            cache.put(
                n,
                (1, n),
                Arc::new(Highlighted {
                    text: "x".into(),
                    lines: Vec::new(),
                }),
            );
        }
        assert_eq!(cache.done.len(), KEPT_BLOCKS);
        assert!(!cache.done.contains_key(&0) && cache.done.contains_key(&(KEPT_BLOCKS as u64 + 4)));
        assert_eq!(cache.bytes, KEPT_BLOCKS);
    }
}
