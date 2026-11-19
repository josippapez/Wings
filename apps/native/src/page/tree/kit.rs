//! The components a page names by what they are, laid out by the app from its own parts: a `ListRow`, a `Section` of them
//! and a dialog's `Actions`. Pages that built these from stacks and rows each built them a little differently: headings
//! a few points left of their rows, a row's text at another x when it had no icon, Cancel after the button it cancels.
//! Here each is made once, so every page's look the same.

use super::{
    Actions, Align, ButtonSize, Gap, Heading, Justify, ListButton, Node, Row, Slots, Stack, Text, TextSize, Tone, Variant, Weight, parts,
};

/// The box a row's lead (an icon, a spinner, a mark) is centred in, so the text after it starts at one x whichever it is.
const LEAD_WIDTH: f32 = 16.0;

/// How wide a section's description runs at most: about 80 characters of small text.
const MEASURE: f32 = 560.0;

/// An id for a part the app makes inside a component, from the component's id and the part's number. A page's ids are
/// under 2^53 (`ui.rs`'s `id_of`), so these, from 2^60 up, never meet one.
fn part(id: u64, n: u64) -> u64 {
    (1 << 60) | (id.wrapping_mul(8).wrapping_add(n) & ((1 << 60) - 1))
}

fn stack(id: u64, gap: Gap, children: Vec<Node>) -> Stack {
    Stack {
        id,
        gap,
        grow: false,
        fill: false,
        width: None,
        max_width: None,
        enter: false,
        links: false,
        on_context_menu: None,
        inset: false,
        measure: None,
        children,
    }
}

fn row(id: u64, grow: bool, children: Vec<Node>) -> Row {
    Row { id, gap: Gap::S, align: Align::Center, justify: Justify::Start, wrap: false, grow, children }
}

/// `ListRow`: a row of a list with a `lead` slot (an icon or mark), a `title`, a `detail` under it and `trailing` at its
/// end. With an `onClick` it is a `ListButton`; without one it sits in the same inset, so its text starts where a
/// clickable row's does and a screen reader doesn't call it a button.
pub(super) fn list_row(id: u64, children: Vec<Node>, on_click: Option<String>, title: Option<String>) -> Node {
    let mut slots = Slots(children);
    let lead = slots.take("lead");
    let label = slots.take("title");
    let detail = slots.take("detail");
    let trailing = slots.take("trailing");
    let mut line = Vec::new();
    if !lead.is_empty() {
        let centred = Node::Row(row(part(id, 2), false, lead));
        line.push(Node::Stack(Stack { width: Some(LEAD_WIDTH), ..stack(part(id, 1), Gap::M, vec![centred]) }));
    }
    let words = [label, detail].concat();
    line.push(Node::Stack(Stack { grow: true, ..stack(part(id, 3), Gap::None, words) }));
    line.extend(trailing);
    let clickable = on_click.is_some();
    // In a list button, a row that grows takes the button's width, so `trailing` sits at its end. In a stack, which is a
    // column, it is as wide as the stack already.
    let line = Node::Row(row(part(id, 4), clickable, line));
    if clickable {
        Node::ListButton(ListButton {
            button: super::Button {
                id,
                variant: Variant::Ghost,
                size: ButtonSize::Default,
                chip: false,
                pressed: false,
                disabled: false,
                label: None,
                title,
                wrap: false,
                round: false,
                on_click,
                on_key_down: None,
                shortcut: None,
                description: None,
                children: vec![line],
            },
            indent: 0.0,
            expanded: None,
            vertical: false,
            baseline: false,
        })
    } else {
        Node::Stack(Stack { inset: true, ..stack(id, Gap::M, vec![line]) })
    }
}

/// `Section`: a heading (`title` slot), a `description` under it at a readable measure, and its rows, one under the
/// other with no gap, as list buttons are. The heading and the description are in a list button's inset, so they start
/// where the rows' text does.
pub(super) fn section(id: u64, children: Vec<Node>) -> Node {
    let mut slots = Slots(children);
    let title = slots.take("title");
    let description = slots.take("description");
    let rows = parts(slots.0);
    let mut out = Vec::new();
    if !title.is_empty() {
        let heading = Node::Heading(Heading { id: part(id, 2), section: false, children: title });
        out.push(Node::Stack(Stack { inset: true, ..stack(part(id, 1), Gap::M, vec![heading]) }));
    }
    if !description.is_empty() {
        let said = Node::Text(Text {
            id: part(id, 4),
            tone: Some(Tone::Muted),
            size: TextSize::Small,
            weight: Weight::Normal,
            mono: false,
            truncate: false,
            expandable: false,
            clamp: false,
            grow: false,
            on_click: None,
            title: None,
            shimmer: false,
            mark: false,
            since: None,
            links: false,
            href: None,
            children: description,
        });
        out.push(Node::Stack(Stack { inset: true, measure: Some(MEASURE), ..stack(part(id, 3), Gap::M, vec![said]) }));
    }
    out.push(Node::Stack(stack(part(id, 5), Gap::None, rows)));
    Node::Stack(stack(id, Gap::Xs, out))
}

/// `Actions`: the buttons in the `dismiss` slot (Cancel, Close) first and always ghost, then the rest as the page made
/// them, together at the end of the line.
pub(super) fn actions(id: u64, children: Vec<Node>) -> Node {
    let mut slots = Slots(children);
    let dismiss = slots.take("dismiss").into_iter().map(|node| match node {
        Node::Button(button) => Node::Button(super::Button { variant: Variant::Ghost, ..button }),
        node => node,
    });
    let children = dismiss.chain(parts(slots.0)).collect();
    Node::Actions(Actions { id, children })
}
