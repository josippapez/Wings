//! A `Table`: a card of rows split by hairlines, the header muted and set apart, the columns sized by what they hold.
use super::*;

/// A shade over the header row, a little lighter than the card.
const HEADER_TINT: u32 = 0xffffff0a;
/// A column's width in points at one character, a little over what 13 point text takes, with room for the chip of a word in code.
const CHAR: f32 = 8.0;
/// What a cell adds on both sides, in points.
const GUTTER: f32 = 32.0;
/// A column with words in it keeps at least this wide before the table scrolls sideways instead of squeezing it.
const MIN_WORDS: f32 = 150.0;
/// A column this many characters long or less is as wide as its longest cell and never grows: a name, a count, "none".
const SHORT: usize = 16;
/// Past this many characters a column's share of the room stops growing, so one long cell doesn't take the table.
const LONG: usize = 60;

/// How a table's column takes room: `basis` is the width it wants, `grow` how much of the spare room it takes, and `min`
/// the narrowest it is made before the table scrolls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Column {
    pub basis: f32,
    pub grow: f32,
    pub min: f32,
}

/// The cells of a row: a `Row`'s children, and any other node alone.
fn cells(row: &Node) -> &[Node] {
    match row {
        Node::Row(row) => &row.children,
        other => std::slice::from_ref(other),
    }
}

/// The columns of a table, from the longest text in each.
pub(super) fn columns(rows: &[Node]) -> Vec<Column> {
    let count = rows.iter().map(|row| cells(row).len()).max().unwrap_or(0);
    (0..count)
        .map(|at| {
            let longest = rows.iter().filter_map(|row| cells(row).get(at)).map(|cell| plain_text(std::slice::from_ref(cell)).chars().count()).max().unwrap_or(0);
            let natural = longest.min(LONG) as f32 * CHAR + GUTTER;
            match longest <= SHORT {
                true => Column { basis: natural, grow: 0.0, min: natural },
                false => Column { basis: natural, grow: natural, min: MIN_WORDS },
            }
        })
        .collect()
}

impl PageView {
    pub(super) fn table(&self, table: &tree::Table, cx: &mut Context<Self>) -> AnyElement {
        let columns = columns(&table.children);
        let width: f32 = columns.iter().map(|column| column.min).sum();
        let rows = table.children.iter().enumerate().map(|(at, row)| {
            let cells = cells(row).iter().enumerate().map(|(c, cell)| {
                let column = columns.get(c).copied().unwrap_or(Column { basis: 0.0, grow: 1.0, min: 0.0 });
                let mut holder = div().min_w(px(column.min)).px(px(12.0)).py(px(9.0)).flex().flex_col().items_start().child(self.node(cell, cx));
                let style = holder.style();
                style.flex_grow = Some(column.grow);
                style.flex_shrink = Some(if column.grow == 0.0 { 0.0 } else { 1.0 });
                style.flex_basis = Some(px(column.basis).into());
                holder.into_any_element()
            });
            div()
                .flex()
                .items_start()
                // The header is set apart by a rule of its own and a tint, the body rows by hairlines between them.
                .when(at == 0, |row| row.bg(rgba(HEADER_TINT)).border_b_1().border_color(rgba(theme::hairline_strong())))
                .when(at > 1, |row| row.border_t_1().border_color(rgba(ui::HAIRLINE)))
                .children(cells)
                .into_any_element()
        });
        let grid = div().flex().flex_col().min_w(px(width)).w_full().children(rows);
        let scroller = div().id(ElementId::NamedInteger("page-table".into(), table.id)).w_full().overflow_x_scroll().child(grid);
        div()
            .min_w_0()
            .w_full()
            .rounded(px(theme::RADIUS))
            .border_1()
            .border_color(rgba(ui::HAIRLINE))
            .overflow_hidden()
            .child(scroller)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::tree::Row;

    fn row(cells: &[&str]) -> Node {
        let text = |words: &str| Node::Str { id: 0, text: words.into() };
        Node::Row(Row {
            id: 0,
            gap: Default::default(),
            align: Default::default(),
            justify: Default::default(),
            wrap: false,
            grow: false,
            children: cells.iter().map(|words| text(words)).collect(),
        })
    }

    /// A short column is as wide as its longest cell and keeps it; a column of sentences shares the spare room and may
    /// be squeezed no further than a line of words, and the share of a very long one is capped.
    #[test]
    fn a_short_column_keeps_its_width_and_a_long_one_wraps() {
        let long = "words ".repeat(40);
        let columns = columns(&[row(&["Count", "Trail", "Notes"]), row(&["42", "Patient Management and the list crumb goes back to page 2", &long])]);
        assert_eq!(columns.len(), 3);
        assert_eq!((columns[0].grow, columns[0].min), (0.0, columns[0].basis));
        assert!(columns[1].grow > 0.0 && columns[1].min == MIN_WORDS);
        assert_eq!(columns[2].basis, LONG as f32 * CHAR + GUTTER, "one long cell doesn't take the table");
        assert!(columns[0].basis < columns[1].basis);
    }
}
