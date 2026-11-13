//! Markdown as nodes, like `wings-plugin-ui/native`'s `Markdown`: headings, paragraphs, lists (flat, with an indent depth,
//! and task items), fenced code, quotes, rules and tables, and in a line code, bold, strikethrough, italics, links, images
//! (shown as their alt text) and bare https URLs. It never makes HTML: text stays text, and the only links it opens are https.
//! Left out, as the TypeScript build's parser had them: `<details>` folds, hidden HTML comments and link reference lines, and
//! emoji shortcodes, which stay as written.

use crate::parts::{column, line, muted, row, stack};
use wings_plugin::{host, ui::Node};

#[derive(Debug, PartialEq)]
pub enum Span {
    Text(String),
    Code(String),
    Strong(String),
    Em(String),
    Del(String),
    Link { text: String, href: String },
}

#[derive(Debug, PartialEq)]
pub enum Block {
    Heading { level: usize, text: String },
    Paragraph(String),
    List { ordered: bool, items: Vec<Item> },
    Code { lang: String, text: String },
    Quote(String),
    Rule,
    Table { head: Vec<String>, rows: Vec<Vec<String>> },
}

#[derive(Debug, PartialEq)]
pub struct Item {
    pub text: String,
    pub depth: usize,
    pub checked: Option<bool>,
}

/// Only https links are ever opened: anything else (javascript:, http:, data:, relative) is not a link.
fn safe_href(href: &str) -> Option<&str> {
    let rest = href.get(..8).filter(|scheme| scheme.eq_ignore_ascii_case("https://")).map(|_| &href[8..])?;
    (!rest.is_empty() && !rest.contains(|c: char| c.is_whitespace() || "<>\"'`".contains(c))).then_some(href)
}

/// The cells of a table row: split on `|` that isn't escaped as `\|`, outer pipes dropped.
fn split_row(line: &str) -> Vec<String> {
    let mut s = line.trim();
    s = s.strip_prefix('|').unwrap_or(s);
    if s.ends_with('|') && !s.ends_with("\\|") {
        s = &s[..s.len() - 1];
    }
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut cell).trim().to_owned()),
            _ => cell.push(c),
        }
    }
    cells.push(cell.trim().to_owned());
    cells
}

/// A table's delimiter row (`| --- | :-: |`): whether the line is one, and how many cells it has.
fn delimiter_row(line: &str) -> Option<usize> {
    let bare = line.trim();
    if !line.contains('-') || (!line.contains('|') && !(bare.trim_matches(':').len() > 0 && bare.trim_matches(':').bytes().all(|b| b == b'-'))) {
        return None;
    }
    let cells = split_row(line);
    cells.iter().all(|c| c.trim_matches(':').len() > 0 && c.trim_matches(':').bytes().all(|b| b == b'-') && c.matches(':').count() <= 2).then_some(cells.len())
}

/// A heading's text without its closing hashes (`## Title ##`).
fn heading_text(text: &str) -> &str {
    let end = text.trim_end().len();
    let kept = text[..end].trim_end_matches('#');
    if kept.len() < end && !kept.is_empty() && kept.ends_with(char::is_whitespace) { kept.trim_end() } else { &text[..end] }
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    let hashes = rest.bytes().take_while(|b| *b == b'#').count();
    (indent <= 3 && (1..=6).contains(&hashes) && rest[hashes..].starts_with(char::is_whitespace)).then(|| (hashes, rest[hashes..].trim_start()))
}

/// A list item: its indent in characters, whether it is a bullet, and its text.
fn list_item(line: &str) -> Option<(usize, bool, &str)> {
    let trimmed = line.trim_start();
    let indent = line[..line.len() - trimmed.len()].replace('\t', "  ").chars().count();
    if let Some(rest) = trimmed.strip_prefix(['-', '*', '+']).filter(|rest| rest.starts_with(char::is_whitespace)) {
        return Some((indent, true, rest.trim_start()));
    }
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    let rest = trimmed[digits..].strip_prefix(['.', ')']).filter(|rest| digits > 0 && rest.starts_with(char::is_whitespace))?;
    Some((indent, false, rest.trim_start()))
}

/// Three or more of the same `-`, `*` or `_`, with spaces between them.
fn is_rule(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    compact.len() >= 3 && compact.chars().all(|c| Some(c) == compact.chars().next()) && "-*_".contains(&compact[..1])
}

fn fence(line: &str) -> Option<(&'static str, String)> {
    let trimmed = line.trim_start();
    let mark = if trimmed.starts_with("```") { "```" } else if trimmed.starts_with("~~~") { "~~~" } else { return None };
    let lang: String = trimmed[3..].trim_start().chars().take_while(|c| c.is_alphanumeric() || "_+-".contains(*c)).collect();
    Some((mark, lang))
}

pub fn parse_blocks(src: &str) -> Vec<Block> {
    let src = src.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = src.split('\n').collect();
    let mut blocks: Vec<Block> = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    fn flush(blocks: &mut Vec<Block>, paragraph: &mut Vec<String>) {
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(paragraph.join(" ")));
        }
        paragraph.clear();
    }
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if let Some((mark, lang)) = fence(line) {
            flush(&mut blocks, &mut paragraph);
            let mut code = Vec::new();
            while i < lines.len() && lines[i].trim() != mark {
                code.push(lines[i]);
                i += 1;
            }
            i += 1;
            blocks.push(Block::Code { lang, text: code.join("\n") });
            continue;
        }
        if line.trim().is_empty() {
            flush(&mut blocks, &mut paragraph);
            continue;
        }
        // A table: a header row with a pipe, then a delimiter row with as many cells.
        if line.contains('|') && i < lines.len() {
            let head = split_row(line);
            if delimiter_row(lines[i]) == Some(head.len()) {
                flush(&mut blocks, &mut paragraph);
                let mut rows = Vec::new();
                let mut j = i + 1;
                while j < lines.len() && !lines[j].trim().is_empty() && lines[j].contains('|') {
                    let cells = split_row(lines[j]);
                    // Short rows are padded and long ones cut, as GFM does.
                    rows.push((0..head.len()).map(|c| cells.get(c).cloned().unwrap_or_default()).collect());
                    j += 1;
                }
                blocks.push(Block::Table { head, rows });
                i = j;
                continue;
            }
        }
        if let Some((level, text)) = heading(line) {
            flush(&mut blocks, &mut paragraph);
            blocks.push(Block::Heading { level, text: heading_text(text).to_owned() });
        } else if is_rule(line) {
            flush(&mut blocks, &mut paragraph);
            blocks.push(Block::Rule);
        } else if let Some((indent, bullet, text)) = list_item(line) {
            flush(&mut blocks, &mut paragraph);
            let ordered = !bullet;
            let (checked, text) = match text.strip_prefix('[').and_then(|rest| rest.chars().next().filter(|c| matches!(c, ' ' | 'x' | 'X')).map(|c| (c, &rest[1..]))) {
                Some((mark, after)) if after.starts_with("] ") || after.starts_with("]\t") => (Some(mark != ' '), after[1..].trim_start()),
                _ => (None, text),
            };
            let item = Item { text: text.to_owned(), depth: (indent / 2).min(3), checked };
            match blocks.last_mut() {
                Some(Block::List { ordered: same, items }) if *same == ordered => items.push(item),
                _ => blocks.push(Block::List { ordered, items: vec![item] }),
            }
        } else if line.trim_start().starts_with('>') {
            flush(&mut blocks, &mut paragraph);
            let text = line.trim_start()[1..].strip_prefix(' ').unwrap_or(&line.trim_start()[1..]).to_owned();
            match blocks.last_mut() {
                Some(Block::Quote(quote)) => {
                    quote.push(' ');
                    quote.push_str(&text);
                }
                _ => blocks.push(Block::Quote(text)),
            }
        } else {
            paragraph.push(line.trim().to_owned());
        }
    }
    flush(&mut blocks, &mut paragraph);
    blocks
}

/// The part of `text` after `open`, up to the first `close`, when it has at least `least` characters and none of `stop`.
fn between<'a>(text: &'a str, open: &str, close: &str, least: usize, most: usize, stop: &[char]) -> Option<(&'a str, &'a str)> {
    let rest = text.strip_prefix(open)?;
    let end = rest.find(close)?;
    let inner = &rest[..end];
    let count = inner.chars().count();
    (count >= least && count <= most && !inner.contains(stop)).then(|| (inner, &rest[end + close.len()..]))
}

/// A `(url)` that follows: no spaces or closing parenthesis in it.
fn paren(text: &str, least: usize) -> Option<(&str, &str)> {
    between(text, "(", ")", least, 2000, &[' ', '\t', '\n'])
}

/// One span at the start of `text`, and what is left after it.
fn span_at(text: &str) -> Option<(Option<Span>, &str)> {
    // A linked image: its alt text is the link's text.
    if let Some((alt, rest)) = between(text, "[![", "]", 0, 200, &['[', ']', '\n']) {
        if let (Some((_, rest)), true) = (paren(rest, 0), true) {
            if let Some((url, rest)) = rest.strip_prefix(']').and_then(|r| paren(r, 1)) {
                return Some((Some(match safe_href(url) {
                    Some(href) => Span::Link { text: if alt.is_empty() { url.to_owned() } else { alt.to_owned() }, href: href.to_owned() },
                    None => Span::Text(alt.to_owned()),
                }), rest));
            }
        }
    }
    if let Some((alt, rest)) = between(text, "![", "]", 0, 200, &['[', ']', '\n']) {
        if let Some((_, rest)) = paren(rest, 0) {
            return Some(((!alt.is_empty()).then(|| Span::Text(alt.to_owned())), rest));
        }
    }
    if let Some((label, rest)) = between(text, "[", "]", 1, 200, &['[', ']', '\n']) {
        if let Some((url, rest)) = paren(rest, 1) {
            return Some((Some(match safe_href(url) {
                Some(href) => Span::Link { text: label.to_owned(), href: href.to_owned() },
                None => Span::Text(label.to_owned()),
            }), rest));
        }
    }
    if let Some((code, rest)) = between(text, "`", "`", 1, 2000, &['`', '\n']) {
        return Some((Some(Span::Code(code.to_owned())), rest));
    }
    if let Some((bold, rest)) = between(text, "**", "**", 1, 2000, &['*', '\n']) {
        return Some((Some(Span::Strong(bold.to_owned())), rest));
    }
    if let Some((del, rest)) = between(text, "~~", "~~", 1, 2000, &['~', '\n']) {
        return Some((Some(Span::Del(del.to_owned())), rest));
    }
    if let Some((em, rest)) = between(text, "*", "*", 1, 2001, &['*', '\n']).filter(|(em, _)| !em.starts_with(char::is_whitespace)) {
        return Some((Some(Span::Em(em.to_owned())), rest));
    }
    if text.get(..8).is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://")) {
        let len = text.find(|c: char| c.is_whitespace() || "<>\"'`)]".contains(c)).unwrap_or(text.len()).min(2008);
        if len > 8 {
            // A bare URL, without the punctuation that ends a sentence.
            let url = text[..len].trim_end_matches(['.', ',', ';', ':', '!', '?']);
            let span = if safe_href(url).is_some() { Span::Link { text: url.to_owned(), href: url.to_owned() } } else { Span::Text(url.to_owned()) };
            return Some((Some(span), &text[url.len()..]));
        }
    }
    None
}

/// The spans of a line of Markdown.
pub fn parse_inline(src: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut rest = src;
    while !rest.is_empty() {
        match span_at(rest) {
            Some((span, after)) if after.len() < rest.len() => {
                if !plain.is_empty() {
                    out.push(Span::Text(std::mem::take(&mut plain)));
                }
                out.extend(span);
                rest = after;
            }
            _ => {
                let c = rest.chars().next().expect("not empty");
                plain.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    if !plain.is_empty() {
        out.push(Span::Text(plain));
    }
    out
}

/// A line's spans as the children of a `Text`: a nested `Text` for each look, a link that opens in the browser.
fn spans(text: &str) -> Node {
    let mut node = Node::new("Text").prop("grow", true);
    for span in parse_inline(text) {
        node = match span {
            Span::Text(t) | Span::Em(t) => node.text(t),
            Span::Code(t) => node.child(Node::new("Text").prop("mono", true).text(t)),
            Span::Strong(t) => node.child(Node::new("Text").prop("weight", "strong").text(t)),
            Span::Del(t) => node.child(Node::new("Text").prop("tone", "muted").text(t)),
            Span::Link { text, href } => node.child(Node::new("Text").on("onClick", move |_| host::open_url(&href)).text(text)),
        };
    }
    node
}

fn code_block(lang: &str, text: &str) -> Node {
    let rows = text.split('\n').map(|code| row("m").prop("align", "baseline").child(line(if code.is_empty() { " " } else { code }, None, Some("small")).prop("mono", true).prop("grow", true)));
    let title = (!lang.is_empty()).then(|| line(lang, Some("muted"), Some("tiny")));
    Node::new("Card").child(stack("none").maybe(title).children(rows))
}

/// Markdown as nodes.
pub fn markdown(source: &str) -> Node {
    let blocks = parse_blocks(source).into_iter().map(|block| match block {
        Block::Heading { level, text } => {
            let node = spans(&text).prop("weight", "strong");
            if level <= 2 { node.prop("size", "field") } else { node }
        }
        Block::List { ordered, items } => stack("xs").children(items.into_iter().enumerate().map(|(n, item)| {
            let marker = match item.checked {
                None if ordered => format!("{}.", n + 1),
                None => "•".to_owned(),
                Some(true) => "☑".to_owned(),
                Some(false) => "☐".to_owned(),
            };
            row("s").prop("align", "start").maybe((item.depth > 0).then(|| column(item.depth as u32 * 16))).child(muted(marker)).child(spans(&item.text))
        })),
        Block::Code { lang, text } => code_block(&lang, &text),
        Block::Quote(text) => row("s").prop("align", "start").child(muted("▎")).child(spans(&text).prop("tone", "muted")),
        Block::Rule => muted("────────────────"),
        Block::Table { head, rows } => stack("xs")
            .child(row("m").children(head.into_iter().map(|cell| spans(&cell).prop("weight", "strong"))))
            .children(rows.into_iter().map(|cells| row("m").prop("align", "start").children(cells.into_iter().map(|cell| spans(&cell))))),
        Block::Paragraph(text) => spans(&text),
    });
    stack("m").children(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Span {
        Span::Text(s.into())
    }

    #[test]
    fn blocks_headings_lists_task_items_code_quotes_rules_and_tables() {
        let blocks = parse_blocks("## Goal\nShip it\nnow\n\n- one\n  - nested\n- [x] done\n- [ ] open\n\n1. first\n2) second\n\n```rust\nlet a = 1;\n\nlet b = 2;\n```\n> quoted\n> more\n---\n| a | b |\n| --- | :-: |\n| 1 | 2 |\n| 3 |\n");
        assert_eq!(blocks[0], Block::Heading { level: 2, text: "Goal".into() });
        assert_eq!(blocks[1], Block::Paragraph("Ship it now".into()));
        let Block::List { ordered, items } = &blocks[2] else { panic!("a list: {blocks:?}") };
        assert!(!ordered);
        assert_eq!(items.iter().map(|i| (i.text.as_str(), i.depth, i.checked)).collect::<Vec<_>>(), [("one", 0, None), ("nested", 1, None), ("done", 0, Some(true)), ("open", 0, Some(false))]);
        let Block::List { ordered, items } = &blocks[3] else { panic!("a numbered list") };
        assert!(*ordered && items.len() == 2);
        assert_eq!(blocks[4], Block::Code { lang: "rust".into(), text: "let a = 1;\n\nlet b = 2;".into() });
        assert_eq!(blocks[5], Block::Quote("quoted more".into()));
        assert_eq!(blocks[6], Block::Rule);
        assert_eq!(blocks[7], Block::Table { head: vec!["a".into(), "b".into()], rows: vec![vec!["1".into(), "2".into()], vec!["3".into(), String::new()]] });
        assert_eq!(parse_blocks("# Title ##")[0], Block::Heading { level: 1, text: "Title".into() });
    }

    #[test]
    fn spans_code_bold_strikethrough_italics_links_images_and_bare_urls() {
        assert_eq!(parse_inline("a `b` **c** ~~d~~ *e*"), [text("a "), Span::Code("b".into()), text(" "), Span::Strong("c".into()), text(" "), Span::Del("d".into()), text(" "), Span::Em("e".into())]);
        assert_eq!(parse_inline("see [docs](https://example.com/d) now"), [text("see "), Span::Link { text: "docs".into(), href: "https://example.com/d".into() }, text(" now")]);
        // A link that isn't https is its text, and an image is its alt text.
        assert_eq!(parse_inline("[x](http://a.example) ![logo](https://a.example/l.png)"), [text("x"), text(" "), text("logo")]);
        assert_eq!(parse_inline("go to https://example.com/a, then"), [text("go to "), Span::Link { text: "https://example.com/a".into(), href: "https://example.com/a".into() }, text(", then")]);
        assert_eq!(parse_inline("2 * 3 * 4 and *"), [text("2 * 3 * 4 and *")]);
        assert_eq!(parse_inline("[![alt](https://a.example/i.png)](https://b.example)"), [Span::Link { text: "alt".into(), href: "https://b.example".into() }]);
        assert_eq!(parse_inline("café `ünï`"), [text("café "), Span::Code("ünï".into())]);
    }
}
