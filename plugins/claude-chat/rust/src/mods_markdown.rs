//! A mod's `Markdown` whose links the mod answers (`onLinkPress`, which comes as a `press` on the element): the links in
//! it go back to Claude Code as a press with their `href` instead of opening in the browser. The SDK's own `markdown`
//! opens every link, so this draws the same blocks with its own links. Only https links are links: the parser leaves
//! the rest as text.

use std::rc::Rc;

use wings_plugin::{
    markdown::{Block, Span, code_block, parse_blocks, parse_inline},
    ui::{Node, row, stack, text},
};

/// What a click on a link does, with its `href`.
pub type OnLink = Rc<dyn Fn(&str)>;

/// `source` as a column of blocks, its links answered by `on_link`.
pub fn linked(source: &str, on_link: &OnLink) -> Node {
    stack().prop("gap", "m").children(parse_blocks(source, 0).iter().map(|block| node_of(block, on_link)))
}

fn spans(source: &str, on_link: &OnLink) -> Node {
    let mut line = Node::new("Text").prop("grow", true);
    for span in parse_inline(source) {
        line = match span {
            Span::Text(t) | Span::Em(t) => line.text(t),
            Span::Code(t) => line.child(Node::new("Text").prop("mono", true).text(t)),
            Span::Strong(t) => line.child(Node::new("Text").prop("weight", "strong").text(t)),
            Span::Del(t) => line.child(Node::new("Text").prop("tone", "muted").text(t)),
            Span::Link { text, href } => {
                let (on_link, target) = (on_link.clone(), href.clone());
                line.child(Node::new("Text").prop("href", href.as_str()).on("onClick", move |_| on_link(&target)).text(text))
            }
        };
    }
    line
}

fn node_of(block: &Block, on_link: &OnLink) -> Node {
    match block {
        Block::Heading(level, t) => Node::new("Text").prop("weight", "strong").prop("size", if *level <= 2 { "field" } else { "small" }).child(spans(t, on_link)),
        Block::Paragraph(t) => spans(t, on_link),
        Block::List { ordered, start, items } => stack().prop("gap", "xs").children(items.iter().enumerate().map(|(j, item)| {
            let mark = match item.checked {
                None if *ordered => format!("{}.", *start + j as u64),
                None => "\u{2022}".to_owned(),
                Some(true) => "\u{2611}".to_owned(),
                Some(false) => "\u{2610}".to_owned(),
            };
            row().prop("gap", "s").prop("align", "start").child(stack().prop("width", item.depth * 16)).child(text(mark).prop("tone", "muted")).child(spans(&item.text, on_link))
        })),
        Block::Code { text: code, .. } => code_block(code),
        Block::Quote(t) => row().prop("gap", "s").prop("align", "start").child(text("\u{258e}").prop("tone", "muted")).child(spans(t, on_link).prop("tone", "muted")),
        Block::Rule => text("────────────────").prop("tone", "muted"),
        Block::Table { head, rows } => stack()
            .prop("gap", "xs")
            .child(row().prop("gap", "m").children(head.iter().map(|c| spans(c, on_link).prop("weight", "strong"))))
            .children(rows.iter().map(|r| row().prop("gap", "m").prop("align", "start").children(r.iter().map(|c| spans(c, on_link))))),
        Block::Details { summary, blocks } => {
            let body = stack().prop("gap", "m").children(blocks.iter().map(|block| node_of(block, on_link)));
            Node::new("Disclosure").child(Node::new("Slot").prop("name", "summary").child(spans(summary, on_link))).child(Node::new("Slot").prop("name", "body").child(body))
        }
    }
}
