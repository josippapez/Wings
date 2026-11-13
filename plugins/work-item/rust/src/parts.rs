//! The pieces both pages draw with, as the nodes `wings-plugin-ui/native` makes of them.

use crate::html::{Block, Kind};
use serde_json::{Value, json};
use wings_plugin::{
    format::{ago, parse_time},
    host,
    ui::{Node, card, muted, row, stack, text, title},
};

pub fn gap(node: Node, gap: &str) -> Node {
    node.prop("gap", gap)
}

pub fn strong(text: impl Into<String>) -> Node {
    title(text)
}

/// A title in the larger size, with what follows it in the same line.
pub fn large_title(text: impl Into<String>) -> Node {
    Node::new("Text").prop("weight", "strong").prop("size", "large").text(text)
}

pub fn avatar(name: &str, small: bool) -> Node {
    let avatar = Node::new("Avatar").prop("name", name);
    if small { avatar.prop("size", "small") } else { avatar }
}

/// A dot in a type's colour.
pub fn type_dot(color: u32) -> Node {
    Node::new("Dot").prop("color", color)
}

/// A state's dot, in the tones a `Dot` takes.
pub fn tone_dot(tone: Option<&str>) -> Node {
    match tone {
        Some(tone) => Node::new("Dot").prop("tone", tone),
        None => Node::new("Dot"),
    }
}

/// A badge of `variant` ("outline", "secondary"), with a tone when it has one, to put its content in.
pub fn badge(variant: &str, tone: Option<&str>) -> Node {
    let badge = Node::new("Badge").prop("variant", variant);
    match tone {
        Some(tone) => badge.prop("tone", tone),
        None => badge,
    }
}

pub fn pill(label: impl Into<String>, variant: &str, tone: Option<&str>) -> Node {
    badge(variant, tone).text(label)
}

/// A pill that is a button, as plugin-ui.css draws a `SummaryChip` with an `onClick`.
pub fn pill_button(label: impl Into<String>, tone: Option<&str>, on_click: impl Fn() + 'static) -> Node {
    pill(label, "outline", tone).on("onClick", move |_| on_click())
}

pub fn skeleton(width: Value, height: u32) -> Node {
    Node::new("Skeleton").prop("width", width).prop("height", height)
}

/// Skeletons that stand in for a section, announced once as `label`.
pub fn skeletons(label: &str, bones: Vec<Node>) -> Node {
    gap(stack(), "s").child(Node::new("Spinner").prop("label", label)).children(bones)
}

/// What a view shows with nothing to list and nothing to add to its title.
pub fn empty_title(title: &str) -> Node {
    Node::new("Empty").child(Node::new("Slot").prop("name", "title").text(title))
}

/// A card with its content in a column.
pub fn card_of(content: Vec<Node>) -> Node {
    card().child(gap(stack(), "m").children(content))
}

/// How long ago `iso` was, or none when it isn't a date.
pub fn time_text(iso: &str) -> Option<String> {
    parse_time(iso).map(|ms| ago(ms, host::now()))
}

/// A code block: a card of monospace lines.
fn code_block(code: &str) -> Node {
    let lines = code.split('\n').map(|line| gap(row(), "m").prop("align", "baseline").child(text(if line.is_empty() { " " } else { line }).prop("mono", true).prop("size", "small").prop("grow", true)));
    card().child(gap(stack(), "none").children(lines))
}

/// The lines of a description or a comment: paragraphs, list items and code, with the looks of their parts.
pub fn html(blocks: &[Block]) -> Node {
    let nodes = blocks.iter().map(|block| {
        if block.kind == Kind::Code {
            let code = block.text();
            let code = code.strip_prefix('\n').unwrap_or(&code);
            return code_block(code.strip_suffix('\n').unwrap_or(code));
        }
        let mut line = Node::new("Text").prop("grow", true);
        if block.kind == Kind::Heading {
            line = line.prop("weight", "strong");
        }
        for span in &block.spans {
            let look = &span.look;
            if look.strong || look.mono || look.muted || look.href.is_some() {
                let mut part = Node::new("Text").text(span.text.as_str());
                if look.strong {
                    part = part.prop("weight", "strong");
                }
                if look.mono {
                    part = part.prop("mono", true);
                }
                if look.muted {
                    part = part.prop("tone", "muted");
                }
                if let Some(href) = look.href.clone() {
                    part = part.on("onClick", move |_| drop(host::call("openUrl", json!({ "url": href }))));
                }
                line = line.child(part);
            } else {
                line = line.text(span.text.as_str());
            }
        }
        if block.kind != Kind::Item && block.depth == 0 {
            return line;
        }
        // List items indent by an em space a level, as the native app has no padding to give a row.
        let marker = format!("{}{}", "\u{2003}".repeat(block.depth.saturating_sub(1)), if block.kind == Kind::Item { block.marker.as_deref().unwrap_or_default() } else { "\u{2003}" });
        gap(row(), "s").prop("align", "start").child(muted(marker)).child(line)
    });
    gap(stack(), "s").children(nodes)
}
