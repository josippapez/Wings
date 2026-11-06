//! Web links on screen: which cells a link covers, for hovering and opening.

use wings_terminal::grid::Flags;

use super::snapshot::Snapshot;

/// A web link on screen and the cells it covers.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Link {
    pub(super) url: String,
    pub(super) cells: Vec<(usize, usize)>,
}

/// Finds a link the way the desktop terminal does (xterm.js web links addon 0.12, `strictUrlRegex`),
/// following rows that wrap, within the visible screen.
pub(super) fn link_at(snapshot: &Snapshot, row: usize, col: usize) -> Option<Link> {
    const BODY_STOP: &str = "\"'!*(){}|\\^<>`";
    const END_STOP: &str = "\"':,.!?{}|\\^~[]`()<>";
    let cols = snapshot.cols;
    if cols == 0 || row >= snapshot.rows || snapshot.cells.len() < snapshot.rows * cols {
        return None;
    }
    let wraps = |row: usize| {
        snapshot.cells[row * cols + cols - 1]
            .flags
            .contains(Flags::WRAPLINE)
    };
    let (mut first, mut last) = (row, row);
    while first > 0 && wraps(first - 1) {
        first -= 1;
    }
    while last + 1 < snapshot.rows && wraps(last) {
        last += 1;
    }
    let mut chars = Vec::new();
    for cell in &snapshot.cells[first * cols..(last + 1) * cols] {
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let at = (cell.row, cell.col);
        if cell.text.is_empty() {
            chars.push((' ', at));
        } else {
            chars.extend(cell.text.chars().map(|c| (c, at)));
        }
    }
    let text: Vec<char> = chars.iter().map(|(c, _)| *c).collect();
    let mut start = 0;
    while start < text.len() {
        let Some(scheme) = ["https://", "http://", "HTTPS://", "HTTP://"]
            .iter()
            .find(|scheme| text[start..].starts_with(&scheme.chars().collect::<Vec<_>>()))
        else {
            start += 1;
            continue;
        };
        let body = start + scheme.len();
        let mut stop = body;
        while stop < text.len() && !text[stop].is_whitespace() && !BODY_STOP.contains(text[stop]) {
            stop += 1;
        }
        let Some(end) = (body..=stop).rev().find(|&end| {
            end < text.len() && !text[end].is_whitespace() && !END_STOP.contains(text[end])
        }) else {
            start = body;
            continue;
        };
        let mut cells: Vec<_> = chars[start..=end].iter().map(|(_, at)| *at).collect();
        cells.dedup();
        if cells.contains(&(row, col)) {
            return Some(Link {
                url: text[start..=end].iter().collect(),
                cells,
            });
        }
        start = end + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_view::snapshot::snapshot;
    use wings_terminal::grid::GridPane;

    #[test]
    fn links_follow_the_desktop_pattern_across_wrapped_rows() {
        let mut pane = GridPane::new(20, 4);
        pane.feed(b"see https://example.com/a.b, next\r\n(http://x.y/z) ftp://no");
        let frame = snapshot(&mut pane, None, false, None);
        let link = link_at(&frame, 1, 2).unwrap();
        assert_eq!(link.url, "https://example.com/a.b");
        assert_eq!(
            (link.cells.first(), link.cells.last()),
            (Some(&(0, 4)), Some(&(1, 6)))
        );
        assert_eq!(link_at(&frame, 0, 4), Some(link));
        assert_eq!(link_at(&frame, 1, 7), None);
        assert_eq!(link_at(&frame, 0, 0), None);
        assert_eq!(link_at(&frame, 2, 1).unwrap().url, "http://x.y/z");
        assert_eq!(link_at(&frame, 2, 13), None);
        assert_eq!(link_at(&frame, 2, 17), None);
    }
}
