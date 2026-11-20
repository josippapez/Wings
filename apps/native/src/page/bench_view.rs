//! What the typing bench (`shot_plugins --claude-chat` with `CHAT_TYPING`) reads of a page and does to it: an outline of
//! its tree with how many nodes each part holds, and parts it leaves out of the draw, so a key's frame can be told apart
//! by what the page shows.

use super::*;

impl PageView {
    /// The page's tree down to `depth`, a line a node: its kind, id, children, the nodes under it, and for a `Scroll`
    /// whether its column is drawn a window at a time. Nodes holding fewer than `least` nodes are left out.
    pub fn outline(&self, depth: usize, least: usize) -> String {
        fn count(node: &Node) -> usize {
            1 + node.parts().iter().flat_map(|part| part.iter()).map(count).sum::<usize>()
        }
        fn kind(node: &Node) -> String {
            let debug = format!("{node:?}");
            debug.split(['(', ' ', '{']).next().unwrap_or_default().to_owned()
        }
        fn walk(nodes: &[Node], level: usize, depth: usize, least: usize, out: &mut String) {
            for node in nodes {
                let total = count(node);
                if total < least {
                    continue;
                }
                let children: usize = node.parts().iter().map(|part| part.len()).sum();
                let windowed = match node {
                    Node::Scroll(scroll) => match scroll.children.as_slice() {
                        [Node::Stack(column)] => format!(" column of {} windowed={}", column.children.len(), column_window_view::windowed(column)),
                        other => format!(" not one Stack ({} children)", other.len()),
                    },
                    _ => String::new(),
                };
                out.push_str(&format!("{}{} #{:?} children={children} nodes={total}{windowed}\n", "  ".repeat(level), kind(node), node.id()));
                if level + 1 < depth {
                    for part in node.parts() {
                        walk(part, level + 1, depth, least, out);
                    }
                }
            }
        }
        let mut out = String::new();
        if let Some(tree) = &self.tree {
            walk(&tree.children, 0, depth, least, &mut out);
        }
        out
    }

    /// The ids of the nodes at `path` in the tree, each step an index among a node's children (all parts, in order).
    pub fn ids_at(&self, path: &[usize]) -> Vec<(u64, String)> {
        let Some(tree) = &self.tree else { return Vec::new() };
        let mut nodes: Vec<&Node> = tree.children.iter().collect();
        for at in path {
            let Some(node) = nodes.get(*at) else { return Vec::new() };
            nodes = node.parts().into_iter().flat_map(|part| part.iter()).collect();
        }
        nodes
            .into_iter()
            .filter_map(|node| Some((node.id()?, format!("{node:?}").split(['(', ' ', '{']).next().unwrap_or_default().to_owned())))
            .collect()
    }

    /// For each `Scroll` whose column is drawn a window at a time: its id, the children the last render drew and of how
    /// many.
    pub fn windows_drawn(&self) -> Vec<(u64, std::ops::Range<usize>, usize)> {
        self.follow_scrolls
            .borrow()
            .iter()
            .map(|(id, state)| {
                let (drawn, of) = state.windowing.borrow().drawn.clone();
                (*id, drawn, of)
            })
            .filter(|(_, _, of)| *of > 0)
            .collect()
    }

    /// Leaves the nodes of these ids out of every draw from now on, each drawn as an empty box.
    pub fn hide_for_bench(&self, ids: HashSet<u64>) {
        *self.bench_hidden.borrow_mut() = ids;
    }

    pub(super) fn hidden_for_bench(&self, node: &Node) -> bool {
        let hidden = self.bench_hidden.borrow();
        !hidden.is_empty() && node.id().is_some_and(|id| hidden.contains(&id))
    }
}
