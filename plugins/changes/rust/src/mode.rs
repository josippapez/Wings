//! What the sidebar shows for a repo, Files or the commit graph, kept per repo root in `wings.storage`.

use serde_json::{Map, Value};
use wings_plugin::host::storage;

/// One map of repo root to the section picked there. A root with no entry shows Files.
const KEY: &str = "sidebarMode";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Section {
    #[default]
    Files,
    Graph,
}

impl Section {
    pub fn name(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Graph => "graph",
        }
    }
}

/// The section `saved` (the stored map, or null) holds for `root`.
pub fn section_in(saved: &Value, root: &str) -> Section {
    if saved[root].as_str() == Some("graph") { Section::Graph } else { Section::Files }
}

/// `saved` with `root` set to `section`. Files is the default, so it leaves no entry.
pub fn with_section(saved: Value, root: &str, section: Section) -> Value {
    let mut map = match saved {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    match section {
        Section::Files => map.remove(root),
        Section::Graph => map.insert(root.to_owned(), Value::from(section.name())),
    };
    Value::Object(map)
}

pub fn saved(root: &str) -> Section {
    section_in(&storage::get(KEY).unwrap_or_default(), root)
}

pub fn save(root: &str, section: Section) {
    let _ = storage::set(KEY, &with_section(storage::get(KEY).unwrap_or_default(), root, section));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_repo_shows_files_until_the_graph_is_picked_there_and_each_repo_keeps_its_own_pick() {
        assert_eq!(section_in(&Value::Null, "/a"), Section::Files);
        let saved = with_section(Value::Null, "/a", Section::Graph);
        assert_eq!(saved, json!({ "/a": "graph" }));
        assert_eq!((section_in(&saved, "/a"), section_in(&saved, "/b")), (Section::Graph, Section::Files));
        let both = with_section(saved, "/b", Section::Graph);
        assert_eq!(section_in(&with_section(both, "/a", Section::Files), "/b"), Section::Graph);
        // Something else under the key reads as the default.
        assert_eq!(section_in(&json!("graph"), "/a"), Section::Files);
        assert_eq!(with_section(json!([1]), "/a", Section::Graph), json!({ "/a": "graph" }));
    }
}
