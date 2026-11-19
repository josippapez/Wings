//! What a page's text can hold beyond a plain string: spans inside a line (a bold word, a code span, a link), avatars
//! and the blocks that stand in for content that is loading.

use std::ops::Range;

use gpui::{
    AnyElement, Context, ElementId, FontWeight, HighlightStyle, InteractiveText, IntoElement,
    SharedString, StyledText, UnderlineStyle, div, hsla, prelude::*, px, relative, rgb, rgba,
};

use super::{
    PageView,
    selection_view::with_selection,
    tone_color,
    tree::{Avatar, Length, Node, Skeleton, Tone, Weight},
};
use crate::{
    page::links::{LinkTarget, PageLinks},
    terminal_view::DEFAULT_FONT_FAMILY,
    theme,
};
use wings_plugins::links::Found;

/// How a run of text looks, and what a click on it does.
#[derive(Clone, Debug, Default, PartialEq)]
struct Style {
    tone: Option<Tone>,
    weight: Weight,
    mono: bool,
    click: Option<String>,
    mark: bool,
    /// One of Wings' links, by its place among the line's.
    link: Option<usize>,
    /// The address of a link the page drew, for a right-click on it.
    href: Option<String>,
}

/// A run of the line's text with a look of its own.
struct Span {
    range: Range<usize>,
    style: Style,
}

/// Joins the strings under `nodes` into one line of `text`, with the runs that look unlike the rest. Nested `Text` adds
/// to the look it is in. False when `nodes` hold anything that is not text, which has to be laid out as boxes. `first`
/// is the id of the first nested `Text`, which names the line.
fn collect(
    nodes: &[Node],
    outer: &Style,
    text: &mut String,
    spans: &mut Vec<Span>,
    first: &mut Option<u64>,
) -> bool {
    for node in nodes {
        match node {
            Node::Str { text: piece, .. } => {
                let start = text.len();
                text.push_str(piece);
                if *outer != Style::default() && !piece.is_empty() {
                    spans.push(Span {
                        range: start..text.len(),
                        style: outer.clone(),
                    });
                }
            }
            Node::Text(inner) => {
                first.get_or_insert(inner.id);
                let style = Style {
                    tone: inner.tone.or(outer.tone),
                    weight: if inner.weight == Weight::Normal {
                        outer.weight
                    } else {
                        inner.weight
                    },
                    mono: outer.mono || inner.mono,
                    click: inner.on_click.clone().or_else(|| outer.click.clone()),
                    mark: outer.mark || inner.mark,
                    link: None,
                    href: inner.href.clone().or_else(|| outer.href.clone()),
                };
                if !collect(&inner.children, &style, text, spans, first) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn highlight(style: &Style) -> HighlightStyle {
    let clicks = style.click.is_some() || style.link.is_some();
    let tone = style.tone.or_else(|| clicks.then_some(Tone::Info));
    // A word a search found reads in full colour on the search's mark, as Past sessions shows it.
    if style.mark {
        return HighlightStyle {
            color: Some(rgb(theme::palette().foreground).into()),
            background_color: Some(rgba(crate::ui::SEARCH_MARK).into()),
            ..Default::default()
        };
    }
    HighlightStyle {
        color: tone.map(|tone| rgb(tone_color(tone)).into()),
        font_weight: match style.weight {
            Weight::Normal => None,
            Weight::Medium => Some(FontWeight::MEDIUM),
            Weight::Strong => Some(FontWeight::SEMIBOLD),
        },
        background_color: style.mono.then(|| rgba(0xffffff14).into()),
        underline: clicks.then(|| UnderlineStyle {
            thickness: px(1.0),
            color: None,
            wavy: false,
        }),
        ..Default::default()
    }
}

impl PageView {
    /// `nodes` as one wrapping line of text, when they are strings and `Text` spans; None for anything else. Spans
    /// that are clicked send the page their `onClick`. With `select`, dragging over it selects words.
    pub(super) fn rich_text(&self, nodes: &[Node], select: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (mut text, mut spans, mut first) = (String::new(), Vec::new(), None);
        if !collect(nodes, &Style::default(), &mut text, &mut spans, &mut first) {
            return None;
        }
        let id = first?;
        let links: Vec<(Range<usize>, String, String)> = spans
            .iter()
            .filter_map(|span| Some((span.range.clone(), span.style.href.clone()?, text.get(span.range.clone())?.to_owned())))
            .collect();
        let marks = with_selection(
            spans.iter().map(|span| (span.range.clone(), highlight(&span.style))).collect(),
            select.then(|| self.selection_in(id, &text)).flatten(),
        );
        let mut styled = StyledText::new(text.clone()).with_highlights(marks);
        let layout = styled.layout().clone();
        let families: Vec<_> = spans
            .iter()
            .filter(|span| span.style.mono)
            .map(|span| (span.range.clone(), SharedString::from(DEFAULT_FONT_FAMILY)))
            .collect();
        if !families.is_empty() {
            styled = styled.with_font_family_overrides(families);
        }
        let (ranges, handlers): (Vec<_>, Vec<_>) = spans
            .into_iter()
            .filter_map(|span| span.style.click.map(|handler| (span.range, handler)))
            .unzip();
        let finish = |element: AnyElement, cx: &mut Context<Self>| {
            if select { self.selectable(id, text.clone(), layout.clone(), element, cx) } else { element }
        };
        if ranges.is_empty() {
            return Some(finish(styled.into_any_element(), cx));
        }
        let view = cx.weak_entity();
        // The character the pointer is on, so a right-click on a link can say which one it was. Kept by the line's id: a
        // move redraws the page, and the text says where the pointer is only when that changes.
        let pointed = self.pointed_chars.borrow_mut().entry(id).or_default().clone();
        let on = pointed.clone();
        let interactive = InteractiveText::new(ElementId::NamedInteger("page-rich".into(), id), styled)
            .on_click(ranges, move |index, _, cx| {
                if let Some(handler) = handlers.get(index) {
                    let _ = view.update(cx, |view, _| view.act(handler, None));
                }
            })
            .on_hover(move |index, _, _, _| on.set(index));
        if links.is_empty() {
            return Some(finish(interactive.into_any_element(), cx));
        }
        let linked = div()
                .child(interactive)
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(move |view, event: &gpui::MouseDownEvent, _, _| {
                        let Some(at) = pointed.get() else {
                            return;
                        };
                        if let Some((_, href, text)) = links.iter().find(|(range, ..)| range.contains(&at)) {
                            let hit = serde_json::json!({ "link": { "href": href, "text": text } });
                            *view.context_hit.borrow_mut() = Some((event.position, hit));
                        }
                    }),
                )
                .into_any_element();
        Some(finish(linked, cx))
    }
}

/// The runs of a line with its links laid over its spans, in order and none overlapping another, as `StyledText` takes
/// them: a key in a code span is both code and a link.
fn with_links(spans: &[Span], links: &[Found]) -> Vec<Span> {
    let mut cuts: Vec<usize> = spans
        .iter()
        .flat_map(|span| [span.range.start, span.range.end])
        .chain(links.iter().flat_map(|link| [link.start, link.end]))
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .filter_map(|cut| {
            let (start, end) = (cut[0], cut[1]);
            let base = spans
                .iter()
                .find(|span| span.range.start <= start && end <= span.range.end)
                .map(|span| span.style.clone())
                .unwrap_or_default();
            let link = links.iter().position(|link| link.start <= start && end <= link.end);
            let style = Style { link, ..base };
            (style != Style::default()).then_some(Span { range: start..end, style })
        })
        .collect()
}

/// What a click on a run of a linked line does.
enum Click {
    Page(String),
    Link(LinkTarget),
}

impl PageView {
    /// A `Text` with `links: true` as one line with Wings' links in it, which a click opens, or None when it holds no link
    /// or more than text. A link inside a run the page made clickable is left to the page.
    pub(super) fn linked_text(&self, nodes: &[Node], id: u64, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (rules, generation) = {
            let links = cx.try_global::<PageLinks>()?;
            (links.rules.clone(), links.generation)
        };
        if rules.is_empty() {
            return None;
        }
        let (mut text, mut spans, mut first) = (String::new(), Vec::new(), None);
        if !collect(nodes, &Style::default(), &mut text, &mut spans, &mut first) {
            return None;
        }
        let found = self.found_links(id, &text, &rules, generation);
        let links: Vec<Found> = found
            .iter()
            .filter(|link| {
                !spans
                    .iter()
                    .any(|span| span.style.click.is_some() && span.range.start < link.end && link.start < span.range.end)
            })
            .copied()
            .collect();
        if links.is_empty() {
            return None;
        }
        let targets: Vec<LinkTarget> = links
            .iter()
            .map(|link| LinkTarget {
                plugin: rules[link.rule].plugin.clone(),
                link: rules[link.rule].link.clone(),
                text: text[link.start..link.end].to_owned(),
            })
            .collect();
        let runs = with_links(&spans, &links);
        let marks = with_selection(
            runs.iter().map(|run| (run.range.clone(), highlight(&run.style))).collect(),
            self.selection_in(id, &text),
        );
        let mut styled = StyledText::new(text.clone()).with_highlights(marks);
        let layout = styled.layout().clone();
        let families: Vec<_> = runs
            .iter()
            .filter(|run| run.style.mono)
            .map(|run| (run.range.clone(), SharedString::from(DEFAULT_FONT_FAMILY)))
            .collect();
        if !families.is_empty() {
            styled = styled.with_font_family_overrides(families);
        }
        let (ranges, clicks): (Vec<_>, Vec<_>) = runs
            .into_iter()
            .filter_map(|run| match (run.style.click, run.style.link) {
                (Some(handler), _) => Some((run.range, Click::Page(handler))),
                (None, Some(link)) => Some((run.range, Click::Link(targets[link].clone()))),
                (None, None) => None,
            })
            .unzip();
        let view = cx.weak_entity();
        let linked =
            InteractiveText::new(ElementId::NamedInteger("page-linked".into(), id), styled)
                .on_click(ranges, move |index, window, cx| match clicks.get(index) {
                    Some(Click::Page(handler)) => {
                        let _ = view.update(cx, |view, _| view.act(handler, None));
                    }
                    Some(Click::Link(target)) => {
                        let open = cx.try_global::<PageLinks>().map(|links| links.open.clone());
                        if let Some(open) = open {
                            open(target.clone(), window, cx);
                        }
                    }
                    None => {}
                })
                .into_any_element();
        Some(self.selectable(id, text, layout, linked, cx))
    }
}

/// The first letters of up to two words of `name`, in capitals.
fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let letters: Vec<char> = match words.as_slice() {
        [] => vec![],
        [one] => one.chars().take(2).collect(),
        [first, .., last] => first.chars().take(1).chain(last.chars().take(1)).collect(),
    };
    letters.iter().flat_map(|c| c.to_uppercase()).collect()
}

/// A person looks the same everywhere: the colour is the name's alone.
pub(super) fn avatar(avatar: &Avatar) -> AnyElement {
    let hue = avatar.name.bytes().fold(5381u32, |hash, byte| {
        hash.wrapping_mul(33) ^ u32::from(byte)
    }) % 360;
    let side = if avatar.large { 28.0 } else { 20.0 };
    div()
        .flex_none()
        .size(px(side))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(hsla(hue as f32 / 360.0, 0.42, 0.36, 1.0))
        .text_color(rgb(theme::palette().foreground))
        .text_size(px(if avatar.large { 11.0 } else { 9.0 }))
        .font_weight(FontWeight::SEMIBOLD)
        .child(initials(&avatar.name))
        .into_any_element()
}

pub(super) fn skeleton(skeleton: &Skeleton) -> AnyElement {
    div()
        .h(px(skeleton.height))
        .rounded(px(6.0))
        .bg(rgba(0xffffff12))
        .map(|block| match skeleton.width {
            Some(Length::Points(width)) => block.w(px(width)).flex_none(),
            Some(Length::Percent(percent)) => block.w(relative(percent / 100.0)),
            None => block.w_full(),
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(range: std::ops::Range<usize>, mono: bool) -> Span {
        Span { range, style: Style { mono, ..Style::default() } }
    }

    /// "See `ABC-12` and ABC-7": a code span around the first key, and two links.
    #[test]
    fn links_are_laid_over_the_spans_of_a_line_without_overlapping_them() {
        let spans = [span(4..10, true)];
        let links = [Found { start: 4, end: 10, rule: 0 }, Found { start: 15, end: 20, rule: 0 }];
        let runs: Vec<(std::ops::Range<usize>, bool, Option<usize>)> =
            with_links(&spans, &links).into_iter().map(|run| (run.range, run.style.mono, run.style.link)).collect();
        assert_eq!(runs, [(4..10, true, Some(0)), (15..20, false, Some(1))]);
        // A link across the end of a code span is cut where the span ends, both halves the same link.
        let runs: Vec<(std::ops::Range<usize>, bool, Option<usize>)> = with_links(&[span(0..6, true)], &[Found { start: 4, end: 9, rule: 0 }])
            .into_iter()
            .map(|run| (run.range, run.style.mono, run.style.link))
            .collect();
        assert_eq!(runs, [(0..4, true, None), (4..6, true, Some(0)), (6..9, false, Some(0))]);
    }
}
