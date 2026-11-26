//! What the page last drew, read back for tests, shots and the other views of a page.
use super::*;

impl PageView {
    /// Takes the texts of `nodes`' inputs as given back, as a render takes them (`input`): what was sent up to each is
    /// heard, and each is the text the page last gave. A page's echo of what was typed isn't drawn, so this keeps the
    /// texts sent from piling up while someone types, and a later render from taking an earlier one as the page's own.
    pub(super) fn echoed(&self, nodes: &[Node]) {
        for node in nodes {
            if let Node::Input(input) = node
                && let Some(state) = self.inputs.borrow_mut().get_mut(&input.id)
                && let Some(at) = state.sent.iter().position(|sent| *sent == input.value)
            {
                state.sent.drain(..=at);
                state.given = input.value.clone();
            }
            for part in node.parts() {
                self.echoed(part);
            }
        }
    }

    /// Scrolls the page's shell as little as brings `field` wholly into view, with the shell's 14pt padding around it.
    /// False until the field has been drawn, when there is nothing yet to scroll to.
    pub(super) fn reveal(
        &self,
        field: &gpui::Entity<crate::text_field::TextField>,
        cx: &mut Context<Self>,
    ) -> bool {
        let shown = self.shell_scroll.bounds();
        let at = field.read(cx).bounds();
        if at.size.height <= px(0.0) || shown.size.height <= px(0.0) {
            return false;
        }
        let offset = self.shell_scroll.offset();
        let shift = if at.bottom() + px(14.0) > shown.bottom() {
            shown.bottom() - at.bottom() - px(14.0)
        } else if at.top() - px(14.0) < shown.top() {
            shown.top() - at.top() + px(14.0)
        } else {
            return true;
        };
        self.shell_scroll
            .set_offset(gpui::point(offset.x, (offset.y + shift).min(px(0.0))));
        cx.notify();
        true
    }

    /// How far the page's shell is scrolled down, in points.
    #[cfg(test)]
    pub(in super::super) fn scrolled(&self) -> f32 {
        -self.shell_scroll.offset().y.as_f32()
    }

    /// The keyboard focus of the page's first text input, for the screenshots to type into.
    #[cfg(feature = "test-support")]
    pub fn first_input_focus(&self, cx: &gpui::App) -> Option<gpui::FocusHandle> {
        let inputs = self.inputs.borrow();
        let first = inputs.keys().min()?;
        Some(inputs[first].field.read(cx).focus_handle(cx))
    }

    /// The focus of the last text input named `label`, as the page drew it: an entry's edit form comes after the start
    /// card's fields of the same names.
    #[cfg(feature = "test-support")]
    pub fn input_focus(&self, label: &str, cx: &gpui::App) -> Option<gpui::FocusHandle> {
        Some(self.input_field(label)?.read(cx).focus_handle(cx))
    }

    /// The field of the last text input named `label`.
    #[cfg(feature = "test-support")]
    pub fn input_field(&self, label: &str) -> Option<gpui::Entity<crate::text_field::TextField>> {
        fn find(nodes: &[Node], label: &str, found: &mut Option<u64>) {
            for node in nodes {
                match node {
                    Node::Input(input) if input.label == label => *found = Some(input.id),
                    other => {
                        for part in other.parts() {
                            find(part, label, found);
                        }
                    }
                }
            }
        }
        let mut found = None;
        find(&self.tree.as_ref()?.children, label, &mut found);
        Some(self.inputs.borrow().get(&found?)?.field.clone())
    }

    #[cfg(feature = "test-support")]
    pub fn text(&self) -> String {
        self.tree.as_ref().map(Tree::text).unwrap_or_default()
    }

    /// How many trees the page has sent.
    #[cfg(feature = "test-support")]
    pub fn trees_sent(&self) -> u64 {
        self.trees
    }

    /// How many times the page has rendered.
    #[cfg(feature = "test-support")]
    pub fn renders(&self) -> usize {
        self.renders.get()
    }

    /// The `showOnHover` buttons the last render drew hidden.
    #[cfg(feature = "test-support")]
    pub fn hidden_buttons(&self) -> Vec<u64> {
        self.hidden_buttons.borrow().clone()
    }

    /// How far the following `Scroll` `id` is scrolled from its top, and how far it can scroll at most, in points.
    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn follow_scrolled(&self, id: u64) -> Option<(f32, f32)> {
        let scrolls = self.follow_scrolls.borrow();
        let handle = &scrolls.get(&id)?.handle;
        Some(((-handle.offset().y).as_f32(), handle.max_offset().y.as_f32()))
    }

    /// How tall the view of the `Scroll` `id` was when last drawn, in points.
    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn scrolled_view(&self, id: u64) -> Option<f32> {
        Some(self.follow_scrolls.borrow().get(&id)?.handle.bounds().size.height.as_f32())
    }

    /// Whether the expandable text `id` was cut short when last drawn, and whether it is opened.
    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn text_cut(&self, id: u64) -> (Option<bool>, bool) {
        (self.text_cut.get(&id).copied(), self.text_open.contains(&id))
    }

    /// The width the docked split `id` was dragged to, if it was.
    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn split_set(&self, id: u64) -> Option<f32> {
        self.split_sizes.get(&id).map(|(size, _)| *size)
    }

    /// Whether the split `id` was narrower than its `collapseBelow` when last drawn, none before it was measured.
    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn split_narrow(&self, id: u64) -> Option<bool> {
        self.split_narrow.get(&id).copied()
    }

    /// How far the floating split `id` set its start in when last drawn, none before it was measured.
    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn split_inset(&self, id: u64) -> Option<f32> {
        self.split_inset.get(&id).map(|(_, by)| *by)
    }

    /// The mark of rail `id` the pointer is on, and that mark's title as its card shows it.
    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn rail_card(&self, id: u64) -> Option<(usize, String)> {
        let hover = self.prompt_rails.borrow().get(&id)?.hover?;
        let rail = self.tree.as_ref()?.find_rail(id)?;
        rail.marks.get(hover).map(|mark| (hover, mark.title.clone()))
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn open_disclosures(&self) -> usize {
        self.open.len()
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// Where the select `id` was last drawn, which its menu opens beside.
    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn select_box(&self, id: u64) -> Option<gpui::Bounds<Pixels>> {
        self.select_bounds.get(&id).map(|(bounds, _)| *bounds)
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn checkbox_checked(&self) -> Option<bool> {
        fn find(nodes: &[Node]) -> Option<bool> {
            nodes.iter().find_map(|node| match node {
                Node::Checkbox(checkbox) => Some(checkbox.checked),
                other => other.parts().into_iter().find_map(find),
            })
        }
        self.tree.as_ref().and_then(|tree| find(&tree.children))
    }

    /// Whether the drawn graph's rows show their author and their date, as its width allows.
    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn graph_columns(&self) -> Option<(bool, bool)> {
        use graph_view::Columns;
        let columns = self.graphs.borrow().values().next()?.columns;
        Some((columns == Columns::All, columns != Columns::NoAuthorOrDate))
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn graph_window(&self) -> Option<(usize, usize, usize)> {
        fn find(nodes: &[Node]) -> Option<(usize, usize, usize)> {
            nodes.iter().find_map(|node| match node {
                Node::Graph(graph) => Some((graph.total, graph.offset, graph.rows.len())),
                other => other.parts().into_iter().find_map(find),
            })
        }
        self.tree.as_ref().and_then(|tree| find(&tree.children))
    }
}
