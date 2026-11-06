//! What a page's text can hold beyond a plain string: spans inside a line (a bold word, a code span, a link), avatars
//! and the blocks that stand in for content that is loading.

use std::ops::Range;

use gpui::{
    AnyElement, Context, ElementId, FontWeight, HighlightStyle, InteractiveText, IntoElement,
    SharedString, StyledText, UnderlineStyle, div, hsla, prelude::*, px, relative, rgb, rgba,
};

use super::{
    PageView, tone_color,
    tree::{Avatar, Length, Node, Skeleton, Tone, Weight},
};
use crate::{terminal_view::DEFAULT_FONT_FAMILY, theme};

/// How a run of text looks, and what a click on it does.
#[derive(Clone, Debug, Default, PartialEq)]
struct Style {
    tone: Option<Tone>,
    weight: Weight,
    mono: bool,
    click: Option<String>,
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
    let tone = style
        .tone
        .or_else(|| style.click.is_some().then_some(Tone::Info));
    HighlightStyle {
        color: tone.map(|tone| rgb(tone_color(tone)).into()),
        font_weight: match style.weight {
            Weight::Normal => None,
            Weight::Medium => Some(FontWeight::MEDIUM),
            Weight::Strong => Some(FontWeight::SEMIBOLD),
        },
        background_color: style.mono.then(|| rgba(0xffffff14).into()),
        underline: style.click.is_some().then(|| UnderlineStyle {
            thickness: px(1.0),
            color: None,
            wavy: false,
        }),
        ..Default::default()
    }
}

impl PageView {
    /// `nodes` as one wrapping line of text, when they are strings and `Text` spans; None for anything else. Spans
    /// that are clicked send the page their `onClick`.
    pub(super) fn rich_text(&self, nodes: &[Node], cx: &mut Context<Self>) -> Option<AnyElement> {
        let (mut text, mut spans, mut first) = (String::new(), Vec::new(), None);
        if !collect(nodes, &Style::default(), &mut text, &mut spans, &mut first) {
            return None;
        }
        let id = first?;
        let mut styled = StyledText::new(text).with_highlights(
            spans
                .iter()
                .map(|span| (span.range.clone(), highlight(&span.style))),
        );
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
        if ranges.is_empty() {
            return Some(styled.into_any_element());
        }
        let view = cx.weak_entity();
        Some(
            InteractiveText::new(ElementId::NamedInteger("page-rich".into(), id), styled)
                .on_click(ranges, move |index, _, cx| {
                    if let Some(handler) = handlers.get(index) {
                        let _ = view.update(cx, |view, _| view.send(handler, None));
                    }
                })
                .into_any_element(),
        )
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
        .text_color(rgb(theme::FOREGROUND))
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
