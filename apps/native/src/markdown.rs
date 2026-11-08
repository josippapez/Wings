//! Markdown for the Project page's sections and facts: the part of CommonMark and GitHub's tables that notes use, parsed
//! to blocks and drawn as GPUI elements. Headings, paragraphs, lists (nested), quotes, fenced code, rules and tables;
//! bold, italic, strikethrough, code, links and bare URLs. Anything else is shown as the text it is. Images aren't drawn:
//! one shows as its alt text in `[ ]`.

use std::{ops::Range, rc::Rc};

use gpui::{
    AnyElement, App, FontStyle, FontWeight, HighlightStyle, InteractiveText, SharedString,
    StrikethroughStyle, StyledText, UnderlineStyle, Window, div, hsla, prelude::*, px, rgb, rgba,
};

use crate::{terminal_view::DEFAULT_FONT_FAMILY, theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Bold,
    Italic,
    Strike,
    Code,
    Muted,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inline {
    pub text: String,
    pub styles: Vec<(Range<usize>, Style)>,
    pub links: Vec<(Range<usize>, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(usize, Inline),
    Paragraph(Inline),
    Code(String),
    Quote(Vec<Block>),
    List {
        /// The number the first item has, for an ordered list.
        start: Option<usize>,
        items: Vec<Vec<Block>>,
    },
    Rule,
    Table {
        head: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
    },
}

// ---- Inline ----

const PUNCT: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

struct Scan<'a> {
    chars: Vec<char>,
    out: &'a mut Inline,
}

/// The inline Markdown in `source`.
pub fn inline(source: &str) -> Inline {
    let mut out = Inline::default();
    let chars: Vec<char> = source.chars().collect();
    let mut scan = Scan {
        chars,
        out: &mut out,
    };
    let end = scan.chars.len();
    scan.run(0, end);
    out
}

impl Scan<'_> {
    fn at(&self, i: usize) -> Option<char> {
        self.chars.get(i).copied()
    }

    fn starts(&self, i: usize, pat: &str) -> bool {
        pat.chars()
            .enumerate()
            .all(|(k, c)| self.at(i + k) == Some(c))
    }

    /// The first `pat` at or after `from` and before `end`.
    fn find(&self, from: usize, end: usize, pat: &str) -> Option<usize> {
        let n = pat.chars().count();
        (from..end.saturating_sub(n - 1)).find(|&i| self.starts(i, pat))
    }

    fn is_word(c: Option<char>) -> bool {
        c.is_some_and(char::is_alphanumeric)
    }

    /// Appends `chars[start..end]` with its marks to the output.
    fn run(&mut self, start: usize, end: usize) {
        let mut i = start;
        while i < end {
            let c = self.chars[i];
            match c {
                '\\' if i + 1 < end && PUNCT.contains(self.chars[i + 1]) => {
                    self.out.text.push(self.chars[i + 1]);
                    i += 2;
                }
                '`' => {
                    let ticks = (i..end).take_while(|&k| self.chars[k] == '`').count();
                    let fence = "`".repeat(ticks);
                    match self.find(i + ticks, end, &fence) {
                        Some(close) if close > i + ticks => {
                            let code: String = self.chars[i + ticks..close].iter().collect();
                            self.mark(code.trim(), Style::Code);
                            i = close + ticks;
                        }
                        _ => {
                            self.out.text.push_str(&fence);
                            i += ticks;
                        }
                    }
                }
                '!' if self.at(i + 1) == Some('[') => match self.link(i + 1, end) {
                    Some((text_end, after, _)) => {
                        let alt: String = self.chars[i + 2..text_end].iter().collect();
                        self.mark(&format!("[{alt}]"), Style::Muted);
                        i = after;
                    }
                    None => {
                        self.out.text.push('!');
                        i += 1;
                    }
                },
                '[' => match self.link(i, end) {
                    Some((text_end, after, url)) => {
                        let from = self.out.text.len();
                        self.run(i + 1, text_end);
                        self.out.links.push((from..self.out.text.len(), url));
                        i = after;
                    }
                    None => {
                        self.out.text.push('[');
                        i += 1;
                    }
                },
                '*' | '_' | '~' => i = self.emphasis(i, end),
                'h' if (self.starts(i, "http://") || self.starts(i, "https://"))
                    && !Self::is_word(i.checked_sub(1).and_then(|p| self.at(p))) =>
                {
                    let mut stop = i;
                    while stop < end
                        && !self.chars[stop].is_whitespace()
                        && !"<>\"".contains(self.chars[stop])
                    {
                        stop += 1;
                    }
                    while stop > i && ".,;:!?)".contains(self.chars[stop - 1]) {
                        stop -= 1;
                    }
                    let url: String = self.chars[i..stop].iter().collect();
                    let from = self.out.text.len();
                    self.out.text.push_str(&url);
                    self.out.links.push((from..self.out.text.len(), url));
                    i = stop;
                }
                _ => {
                    self.out.text.push(c);
                    i += 1;
                }
            }
        }
    }

    fn mark(&mut self, text: &str, style: Style) {
        let from = self.out.text.len();
        self.out.text.push_str(text);
        self.out.styles.push((from..self.out.text.len(), style));
    }

    /// `[text](url)` opening at `i`: where the text ends, where the whole link ends and its url.
    fn link(&self, i: usize, end: usize) -> Option<(usize, usize, String)> {
        let mut depth = 0;
        let mut close = None;
        for k in i..end {
            match self.chars[k] {
                '\\' => continue,
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(k);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close?;
        if self.at(close + 1) != Some('(') {
            return None;
        }
        let url_end = (close + 2..end).find(|&k| self.chars[k] == ')')?;
        let url: String = self.chars[close + 2..url_end].iter().collect();
        let url = url.split_whitespace().next().unwrap_or("").to_owned();
        Some((close, url_end + 1, url))
    }

    /// An emphasis opening at `i`, or the delimiter as text. Where the scan goes on.
    fn emphasis(&mut self, i: usize, end: usize) -> usize {
        let c = self.chars[i];
        let (pattern, style) = match (c, self.at(i + 1) == Some(c)) {
            ('~', true) => ("~~", Style::Strike),
            ('~', false) => return self.literal(i),
            (_, true) => (if c == '*' { "**" } else { "__" }, Style::Bold),
            (_, false) => (if c == '*' { "*" } else { "_" }, Style::Italic),
        };
        let n = pattern.chars().count();
        let opens = !self.at(i + n).is_none_or(char::is_whitespace);
        // An underscore inside a word is part of the word.
        let intraword = c == '_' && Self::is_word(i.checked_sub(1).and_then(|p| self.at(p)));
        if !opens || intraword {
            return self.literal_run(i, n);
        }
        let mut from = i + n;
        while let Some(close) = self.find(from, end, pattern) {
            let before = self.chars[close - 1];
            let after = self.at(close + n);
            let closes = close > i + n
                && !before.is_whitespace()
                && !(c == '_' && Self::is_word(after))
                // `**` inside `***x***` leaves the third mark for the outer emphasis.
                && !(n == 1 && self.at(close + 1) == Some(c));
            if closes {
                let text_from = self.out.text.len();
                self.run(i + n, close);
                self.out
                    .styles
                    .push((text_from..self.out.text.len(), style));
                return close + n;
            }
            from = close + 1;
        }
        self.literal_run(i, n)
    }

    fn literal(&mut self, i: usize) -> usize {
        self.literal_run(i, 1)
    }

    fn literal_run(&mut self, i: usize, n: usize) -> usize {
        self.out.text.extend(self.chars[i..i + n].iter());
        i + n
    }
}

// ---- Blocks ----

fn indent(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

fn is_rule(line: &str) -> bool {
    let t: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && indent(line) < 4 && ['-', '*', '_'].iter().any(|m| t.chars().all(|c| c == *m))
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start();
    if indent(line) > 3 {
        return None;
    }
    let level = t.chars().take_while(|c| *c == '#').count();
    let rest = &t[level..];
    ((1..=6).contains(&level) && (rest.is_empty() || rest.starts_with(' '))).then(|| {
        let text = rest.trim();
        (level, text.trim_end_matches('#').trim_end())
    })
}

fn fence(line: &str) -> Option<&str> {
    let t = line.trim_start();
    (indent(line) < 4 && (t.starts_with("```") || t.starts_with("~~~"))).then(|| &t[..3])
}

/// A list marker at the start of `line`: its indent, the number for an ordered list, and where the item's text starts.
fn marker(line: &str) -> Option<(usize, Option<usize>, usize)> {
    let lead = indent(line);
    let t = &line[lead..];
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    let (number, after) =
        if digits > 0 && digits <= 9 && matches!(t[digits..].chars().next(), Some('.' | ')')) {
            (t[..digits].parse().ok(), digits + 1)
        } else if matches!(t.chars().next(), Some('-' | '*' | '+')) && !is_rule(line) {
            (None, 1)
        } else {
            return None;
        };
    match t[after..].chars().next() {
        None => Some((lead, number, lead + after)),
        Some(' ') => Some((lead, number, lead + after + 1)),
        _ => None,
    }
}

fn cells(line: &str) -> Vec<&str> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(str::trim).collect()
}

fn separator(line: &str) -> bool {
    line.contains('-')
        && line.contains('|')
        && cells(line).iter().all(|c| {
            !c.is_empty() && c.trim_matches(':').chars().all(|ch| ch == '-') && c.contains('-')
        })
}

fn starts_block(line: &str) -> bool {
    heading(line).is_some()
        || fence(line).is_some()
        || is_rule(line)
        || line.trim_start().starts_with('>')
        || marker(line).is_some()
}

/// The blocks of a Markdown document.
pub fn parse(source: &str) -> Vec<Block> {
    let lines: Vec<&str> = source.lines().collect();
    blocks(&lines)
}

fn blocks(lines: &[&str]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
        } else if let Some(open) = fence(line) {
            let mut code = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with(open) {
                code.push(lines[i]);
                i += 1;
            }
            i += 1;
            out.push(Block::Code(code.join("\n")));
        } else if let Some((level, text)) = heading(line) {
            out.push(Block::Heading(level, inline(text)));
            i += 1;
        } else if is_rule(line) {
            out.push(Block::Rule);
            i += 1;
        } else if line.trim_start().starts_with('>') {
            let mut quoted = Vec::new();
            while i < lines.len() && lines[i].trim_start().starts_with('>') {
                let t = lines[i].trim_start()[1..].to_owned();
                quoted.push(t.strip_prefix(' ').map_or(t.clone(), str::to_owned));
                i += 1;
            }
            let quoted: Vec<&str> = quoted.iter().map(String::as_str).collect();
            out.push(Block::Quote(blocks(&quoted)));
        } else if let Some((base, number, _)) = marker(line) {
            let ordered = number.is_some();
            let mut items = Vec::new();
            while i < lines.len() {
                let Some((lead, n, text_at)) = marker(lines[i]) else {
                    break;
                };
                if lead != base || n.is_some() != ordered {
                    break;
                }
                let mut item = vec![lines[i].get(text_at..).unwrap_or("").to_owned()];
                i += 1;
                while i < lines.len() {
                    let next = lines[i];
                    if next.trim().is_empty() {
                        // A blank line stays in the item only when what follows is indented under it.
                        let more = lines[i + 1..].iter().find(|l| !l.trim().is_empty());
                        if more.is_some_and(|l| indent(l) > base) {
                            item.push(String::new());
                            i += 1;
                            continue;
                        }
                        break;
                    }
                    if indent(next) > base {
                        item.push(
                            next.get(text_at.min(indent(next))..)
                                .unwrap_or("")
                                .to_owned(),
                        );
                        i += 1;
                    } else if marker(next).is_none() && !starts_block(next) {
                        // A lazy continuation of the item's paragraph.
                        item.push(next.trim_start().to_owned());
                        i += 1;
                    } else {
                        break;
                    }
                }
                let item: Vec<&str> = item.iter().map(String::as_str).collect();
                items.push(blocks(&item));
            }
            out.push(Block::List {
                start: number,
                items,
            });
        } else if line.contains('|') && lines.get(i + 1).is_some_and(|next| separator(next)) {
            let head = cells(line).into_iter().map(inline).collect();
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() && !lines[i].trim().is_empty() && lines[i].contains('|') {
                rows.push(cells(lines[i]).into_iter().map(inline).collect());
                i += 1;
            }
            out.push(Block::Table { head, rows });
        } else {
            let mut text = vec![line.trim()];
            i += 1;
            while i < lines.len() && !lines[i].trim().is_empty() && !starts_block(lines[i]) {
                text.push(lines[i].trim());
                i += 1;
            }
            out.push(Block::Paragraph(inline(&text.join(" "))));
        }
    }
    out
}

// ---- Drawing ----

const CODE_BACKGROUND: u32 = 0xffffff14;

/// What a click on a link does with its URL, when the text came from someone whose links are checked first, like a
/// plugin's.
pub type Opener = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// Text with its marks, as a GPUI element. Links open in the browser when clicked.
pub fn inline_element(id: impl Into<SharedString>, content: &Inline) -> AnyElement {
    inline_with(id, content, None)
}

/// `inline_element`, its links handed to `open` when there is one.
fn inline_with(id: impl Into<SharedString>, content: &Inline, open: Option<&Opener>) -> AnyElement {
    let id: SharedString = id.into();
    // The marks may overlap (bold inside a link), so cut the text where any of them starts or stops.
    let mut cuts: Vec<usize> = content
        .styles
        .iter()
        .map(|(r, _)| r)
        .chain(content.links.iter().map(|(r, _)| r))
        .flat_map(|r| [r.start, r.end])
        .collect();
    cuts.push(0);
    cuts.push(content.text.len());
    cuts.sort_unstable();
    cuts.dedup();
    let mut highlights = Vec::new();
    for pair in cuts.windows(2) {
        let range = pair[0]..pair[1];
        let covers = |r: &Range<usize>| r.start <= range.start && range.end <= r.end;
        let mut style = HighlightStyle::default();
        for (_, mark) in content.styles.iter().filter(|(r, _)| covers(r)) {
            match mark {
                Style::Bold => style.font_weight = Some(FontWeight::BOLD),
                Style::Italic => style.font_style = Some(FontStyle::Italic),
                Style::Strike => {
                    style.strikethrough = Some(StrikethroughStyle {
                        thickness: px(1.0),
                        color: None,
                    })
                }
                Style::Code => {
                    style.background_color = Some(rgba(CODE_BACKGROUND).into());
                }
                Style::Muted => style.color = Some(rgb(theme::MUTED_FOREGROUND).into()),
            }
        }
        if content.links.iter().any(|(r, _)| covers(r)) {
            style.color = Some(rgb(theme::INFO).into());
            style.underline = Some(UnderlineStyle {
                thickness: px(1.0),
                color: Some(hsla(0.0, 0.0, 0.0, 0.0)),
                wavy: false,
            });
        }
        if style != HighlightStyle::default() {
            highlights.push((range, style));
        }
    }
    let styled = StyledText::new(content.text.clone()).with_highlights(highlights);
    if content.links.is_empty() {
        return styled.into_any_element();
    }
    let ranges: Vec<Range<usize>> = content.links.iter().map(|(r, _)| r.clone()).collect();
    let urls: Vec<String> = content.links.iter().map(|(_, url)| url.clone()).collect();
    let open = open.cloned();
    InteractiveText::new(gpui::ElementId::Name(id), styled)
        .on_click(ranges, move |index, window, cx| {
            let url = &urls[index];
            if let Some(open) = &open {
                return open(url, window, cx);
            }
            // Only addresses a browser or mail client should be handed; a note can hold any text.
            if ["http://", "https://", "mailto:"]
                .iter()
                .any(|scheme| url.starts_with(scheme))
            {
                cx.open_url(url);
            }
        })
        .into_any_element()
}

/// The blocks, one under the other.
pub fn render(id: &str, blocks: &[Block]) -> AnyElement {
    render_with(id, blocks, None)
}

/// `render`, its links handed to `open` when there is one.
pub fn render_with(id: &str, blocks: &[Block], open: Option<&Opener>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .text_size(px(13.0))
        .line_height(px(20.0))
        .text_color(rgb(theme::FOREGROUND))
        .children(
            blocks
                .iter()
                .enumerate()
                .map(|(i, b)| block(&format!("{id}-{i}"), b, open)),
        )
        .into_any_element()
}

fn block(id: &str, b: &Block, open: Option<&Opener>) -> AnyElement {
    match b {
        Block::Heading(level, text) => {
            let size = match level {
                1 => 18.0,
                2 => 16.0,
                3 => 14.0,
                _ => 13.0,
            };
            div()
                .mt(px(4.0))
                .text_size(px(size))
                .line_height(px(size + 8.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(inline_with(id.to_owned(), text, open))
                .into_any_element()
        }
        Block::Paragraph(text) => div()
            .child(inline_with(id.to_owned(), text, open))
            .into_any_element(),
        Block::Code(code) => div()
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(8.0))
            .bg(rgba(0xffffff0d))
            .border_1()
            .border_color(rgba(theme::BORDER))
            .font_family(DEFAULT_FONT_FAMILY)
            .text_size(px(12.0))
            .line_height(px(18.0))
            .child(code.clone())
            .into_any_element(),
        Block::Quote(inner) => div()
            .pl(px(12.0))
            .border_l_2()
            .border_color(rgba(theme::HAIRLINE_STRONG))
            .text_color(rgb(theme::MUTED_FOREGROUND))
            .child(render_with(id, inner, open))
            .into_any_element(),
        Block::Rule => div()
            .h(px(1.0))
            .my(px(4.0))
            .bg(rgba(theme::HAIRLINE_STRONG))
            .into_any_element(),
        Block::List { start, items } => div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(items.iter().enumerate().map(|(n, item)| {
                let mark = match start {
                    Some(first) => format!("{}.", first + n),
                    None => "\u{2022}".to_owned(),
                };
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        div()
                            .w(px(20.0))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(mark),
                    )
                    .child(div().min_w_0().flex_1().child(render_with(
                        &format!("{id}-{n}"),
                        item,
                        open,
                    )))
            }))
            .into_any_element(),
        Block::Table { head, rows } => {
            let row = |cells: &[Inline], header: bool, r: usize| {
                div()
                    .flex()
                    .border_t_1()
                    .border_color(rgba(theme::BORDER))
                    .when(header, |row| {
                        row.border_t_0()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                    })
                    .children(cells.iter().enumerate().map(|(c, cell)| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(10.0))
                            .py(px(6.0))
                            .child(inline_with(format!("{id}-{r}-{c}"), cell, open))
                    }))
            };
            div()
                .rounded(px(8.0))
                .border_1()
                .border_color(rgba(theme::BORDER))
                .overflow_hidden()
                .text_size(px(12.5))
                .child(row(head, true, 0))
                .children(
                    rows.iter()
                        .enumerate()
                        .map(|(r, cells)| row(cells, false, r + 1)),
                )
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Styled = (String, Vec<(String, Style)>, Vec<(String, String)>);

    fn styled(text: &str) -> Styled {
        let i = inline(text);
        (
            i.text.clone(),
            i.styles
                .iter()
                .map(|(r, s)| (i.text[r.clone()].to_owned(), *s))
                .collect(),
            i.links
                .iter()
                .map(|(r, u)| (i.text[r.clone()].to_owned(), u.clone()))
                .collect(),
        )
    }

    #[test]
    fn inline_marks_nest_and_leave_plain_text_alone() {
        let (text, styles, links) = styled(
            "a **bold and `code`** and *it* ~~gone~~ [site](https://x.dev/a_b) snake_case_name 2 * 3",
        );
        assert_eq!(
            text,
            "a bold and code and it gone site snake_case_name 2 * 3"
        );
        assert!(styles.contains(&("bold and code".into(), Style::Bold)));
        assert!(styles.contains(&("code".into(), Style::Code)));
        assert!(styles.contains(&("it".into(), Style::Italic)));
        assert!(styles.contains(&("gone".into(), Style::Strike)));
        assert_eq!(
            links,
            vec![("site".to_owned(), "https://x.dev/a_b".to_owned())]
        );
    }

    #[test]
    fn a_bare_address_is_a_link_without_its_trailing_punctuation() {
        let (text, _, links) = styled("see https://example.com/x, then (https://example.com/y).");
        assert_eq!(
            text,
            "see https://example.com/x, then (https://example.com/y)."
        );
        assert_eq!(
            links
                .iter()
                .map(|(_, url)| url.as_str())
                .collect::<Vec<_>>(),
            ["https://example.com/x", "https://example.com/y"]
        );
    }

    #[test]
    fn blocks_cover_headings_lists_code_quotes_rules_and_tables() {
        let doc = "# Title\n\nsome *text*\nmore\n\n- a\n- b\n  - nested\n\n1. one\n2. two\n\n```\nlet x = 1;\n```\n\n> quoted\n\n---\n\n| k | v |\n| - | - |\n| a | b |\n";
        let blocks = parse(doc);
        assert!(matches!(&blocks[0], Block::Heading(1, t) if t.text == "Title"));
        assert!(matches!(&blocks[1], Block::Paragraph(t) if t.text == "some text more"));
        match &blocks[2] {
            Block::List { start: None, items } => {
                assert_eq!(items.len(), 2);
                assert!(matches!(&items[1][1], Block::List { items, .. } if items.len() == 1));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&blocks[3], Block::List { start: Some(1), items } if items.len() == 2));
        assert_eq!(blocks[4], Block::Code("let x = 1;".into()));
        assert!(matches!(&blocks[5], Block::Quote(inner) if inner.len() == 1));
        assert_eq!(blocks[6], Block::Rule);
        assert!(
            matches!(&blocks[7], Block::Table { head, rows } if head.len() == 2 && rows.len() == 1)
        );
    }
}
