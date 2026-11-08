//! The `Svg` a page draws: a small drawing of its own, like an icon. The page sends its shapes as data, only the
//! elements, attributes and values below get through, and the app writes the markup from them itself. So nothing the
//! page wrote reaches the SVG renderer as text: no `<image>` or `href` to load a file with, no `<style>`, no `<text>`
//! that looks up fonts, no entity.

use std::fmt::Write;

use serde_json::{Map, Value};

use super::{Choice, Tone, cut, number, str_prop};

/// The most shapes an `Svg` holds, its `g`s included, and the longest markup they make.
pub const MAX_SHAPES: usize = 32;
pub const MAX_MARKUP_BYTES: usize = 4 * 1024;
/// How deep `g`s go in one another.
const MAX_GROUPS: usize = 4;
/// The largest coordinate or length, either way.
const MAX_NUMBER: f64 = 10_000.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Svg {
    pub id: u64,
    /// Points square.
    pub size: f32,
    /// The colour it's drawn in; the text's when there is none.
    pub tone: Option<Tone>,
    /// What a screen reader says; without one it's decoration.
    pub label: Option<String>,
    /// `<svg …>…</svg>`, written here from what was checked.
    pub markup: String,
}

/// The props the svg and each shape can paint with, by their names in JSX and in SVG.
const PAINT: [(&str, &str); 7] = [
    ("fill", "fill"),
    ("stroke", "stroke"),
    ("strokeWidth", "stroke-width"),
    ("strokeLinecap", "stroke-linecap"),
    ("strokeLinejoin", "stroke-linejoin"),
    ("fillRule", "fill-rule"),
    ("opacity", "opacity"),
];

/// The `Svg` in `props`, or `None` when it holds anything not drawn here or is over the limits.
pub fn parse(id: u64, props: &Map<String, Value>) -> Option<Svg> {
    let mut markup = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{}""#,
        view_box(str_prop(props, "viewBox")?)?
    );
    for (prop, name) in PAINT {
        if let Some(value) = props.get(prop) {
            attribute(&mut markup, name, value)?;
        }
    }
    markup.push('>');
    let mut count = 0;
    shapes(props.get("shapes")?.as_array()?, 0, &mut count, &mut markup)?;
    markup.push_str("</svg>");
    if markup.len() > MAX_MARKUP_BYTES {
        return None;
    }
    Some(Svg {
        id,
        size: number(props, "size").unwrap_or(16.0).clamp(1.0, 64.0) as f32,
        tone: str_prop(props, "tone").and_then(Tone::choose),
        label: str_prop(props, "label").map(|label| cut(label.to_owned())),
        markup,
    })
}

/// `minX minY width height`, square: the app draws an `Svg` in a square, as the browser fits it in one.
fn view_box(text: &str) -> Option<String> {
    let parts: Vec<f64> = text
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| coordinate(part.parse().ok()?))
        .collect::<Option<_>>()?;
    match parts.as_slice() {
        [x, y, width, height] if *width > 0.0 && width == height => {
            Some(format!("{x} {y} {width} {height}"))
        }
        _ => None,
    }
}

/// Writes each of `list`'s shapes, `[tag, props]` or a `g`'s `[tag, props, shapes]`, into `markup`.
fn shapes(list: &[Value], groups: usize, count: &mut usize, markup: &mut String) -> Option<()> {
    for shape in list {
        *count += 1;
        if *count > MAX_SHAPES {
            return None;
        }
        let (tag, props, inner) = match shape.as_array()?.as_slice() {
            [tag, props] => (tag.as_str()?, props.as_object()?, None),
            [tag, props, inner] => (tag.as_str()?, props.as_object()?, Some(inner.as_array()?)),
            _ => return None,
        };
        let geometry: &[&str] = match tag {
            "g" => &[],
            "path" => &["d"],
            "rect" => &["x", "y", "width", "height", "rx", "ry"],
            "circle" => &["cx", "cy", "r"],
            "ellipse" => &["cx", "cy", "rx", "ry"],
            "line" => &["x1", "y1", "x2", "y2"],
            "polyline" | "polygon" => &["points"],
            _ => return None,
        };
        markup.push('<');
        markup.push_str(tag);
        for (prop, value) in props {
            let name = match geometry.iter().copied().find(|name| *name == prop.as_str()) {
                Some(name) => name,
                None => PAINT.iter().find(|(jsx, _)| *jsx == prop.as_str())?.1,
            };
            attribute(markup, name, value)?;
        }
        match inner {
            None => markup.push_str("/>"),
            Some(inner) if tag == "g" && groups < MAX_GROUPS => {
                markup.push('>');
                shapes(inner, groups + 1, count, markup)?;
                markup.push_str("</g>");
            }
            Some(_) => return None,
        }
        if markup.len() > MAX_MARKUP_BYTES {
            return None;
        }
    }
    Some(())
}

/// Writes ` name="value"` into `markup`, the value as SVG reads it: `None` for one it doesn't take here.
fn attribute(markup: &mut String, name: &str, value: &Value) -> Option<()> {
    let one_of = |choices: &[&'static str]| {
        let text = value.as_str()?;
        choices
            .iter()
            .find(|choice| **choice == text)
            .map(|choice| choice.to_string())
    };
    let text = match name {
        // Path data and points are numbers, separators and a path's commands: nothing that can end the attribute.
        "d" => data(value, "MmLlHhVvCcSsQqTtAaZz")?,
        "points" => data(value, "")?,
        "fill" | "stroke" => one_of(&["none", "currentColor"])?,
        "stroke-linecap" => one_of(&["butt", "round", "square"])?,
        "stroke-linejoin" => one_of(&["miter", "round", "bevel"])?,
        "fill-rule" => one_of(&["nonzero", "evenodd"])?,
        "opacity" => Some(number_of(value)?)
            .filter(|opacity| (0.0..=1.0).contains(opacity))?
            .to_string(),
        "x" | "y" | "cx" | "cy" | "x1" | "y1" | "x2" | "y2" => number_of(value)?.to_string(),
        // Widths, heights, radii and the stroke's width.
        _ => Some(number_of(value)?)
            .filter(|length| *length >= 0.0)?
            .to_string(),
    };
    write!(markup, r#" {name}="{text}""#).ok()
}

/// A number, from JSON or from a string of one without units, as SVG reads it.
fn number_of(value: &Value) -> Option<f64> {
    coordinate(match value {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse().ok()?,
        _ => return None,
    })
}

fn coordinate(value: f64) -> Option<f64> {
    (value.is_finite() && value.abs() <= MAX_NUMBER).then_some(value)
}

/// Path data or points: digits, separators and the letters in `commands`.
fn data(value: &Value, commands: &str) -> Option<String> {
    let text = value.as_str()?;
    text.chars()
        .all(|c| {
            c.is_ascii_digit()
                || matches!(
                    c,
                    '.' | ',' | '+' | '-' | 'e' | 'E' | ' ' | '\t' | '\n' | '\r'
                )
                || commands.contains(c)
        })
        .then(|| text.to_owned())
}
