//! A scanned file or folder, as Wings' `diskUsage` answers it, and the paths of a tree: the root entry's name is its
//! full path, and every other entry's is its own name.

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
    Link,
    Other,
}

impl Kind {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "dir" => Self::Dir,
            "file" => Self::File,
            "link" => Self::Link,
            "other" => Self::Other,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Dir => "dir",
            Self::File => "file",
            Self::Link => "link",
            Self::Other => "other",
        }
    }
}

/// What a folder holds that the scan didn't keep one by one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Smaller {
    pub size: u64,
    pub items: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub kind: Kind,
    pub items: u64,
    pub children: Vec<Entry>,
    pub smaller: Option<Smaller>,
    /// A folder Wings couldn't open.
    pub restricted: bool,
}

impl Entry {
    pub fn from_value(value: &Value) -> Self {
        let number = |name: &str| value[name].as_u64().unwrap_or(0);
        Self {
            name: value["name"].as_str().unwrap_or_default().to_owned(),
            size: number("size"),
            kind: value["kind"].as_str().and_then(Kind::parse).unwrap_or(Kind::Other),
            items: number("items"),
            children: value["children"].as_array().map_or_else(Vec::new, |list| list.iter().map(Self::from_value).collect()),
            smaller: value.get("smaller").filter(|smaller| smaller.is_object()).map(|smaller| Smaller { size: smaller["size"].as_u64().unwrap_or(0), items: smaller["items"].as_u64().unwrap_or(0) }),
            restricted: value["restricted"] == true,
        }
    }
}

/// The path of `name` in the folder at `parent`.
pub fn child_path(parent: &str, name: &str) -> String {
    if parent.ends_with('/') { format!("{parent}{name}") } else { format!("{parent}/{name}") }
}

/// The folder `path` is in, or none for `/`.
pub fn parent_path(path: &str) -> Option<String> {
    if path == "/" {
        return None;
    }
    Some(match path.rfind('/') {
        Some(at) if at > 0 => path[..at].to_owned(),
        _ => "/".to_owned(),
    })
}

/// `path`'s names below `root`, like `["Library", "Caches"]`: none for the root itself, and `None` for a path outside it.
pub fn parts_below(root: &str, path: &str) -> Option<Vec<String>> {
    if path == root {
        return Some(Vec::new());
    }
    let prefix = if root.ends_with('/') { root.to_owned() } else { format!("{root}/") };
    path.strip_prefix(&prefix).map(|rest| rest.split('/').map(str::to_owned).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paths_join_and_split_the_way_a_trees_do() {
        assert_eq!([child_path("/", "usr"), child_path("/Users/ada", "Code")], ["/usr", "/Users/ada/Code"]);
        assert_eq!([parent_path("/Users/ada"), parent_path("/Users"), parent_path("/")], [Some("/Users".into()), Some("/".into()), None]);
        assert_eq!(parts_below("/Users/ada", "/Users/ada/Library/Caches"), Some(vec!["Library".into(), "Caches".into()]));
        assert_eq!(parts_below("/Users/ada", "/Users/ada"), Some(Vec::new()));
        assert_eq!(parts_below("/", "/usr/bin"), Some(vec!["usr".into(), "bin".into()]));
        assert_eq!([parts_below("/Users/ada", "/Users/adam"), parts_below("/Users/ada", "/tmp")], [None, None]);
    }

    #[test]
    fn an_entry_reads_what_wings_answers() {
        let answer = json!({
            "name": "/Users/ada", "size": 10, "kind": "dir", "items": 3,
            "children": [{ "name": "Code", "size": 6, "kind": "dir", "items": 1, "smaller": { "size": 2, "items": 4 } }, { "name": "Mail", "size": 0, "kind": "dir", "items": 0, "restricted": true }],
        });
        let entry = Entry::from_value(&answer);
        assert_eq!((entry.size, entry.items, entry.children.len(), entry.restricted), (10, 3, 2, false));
        assert_eq!(entry.children[0].smaller, Some(Smaller { size: 2, items: 4 }));
        assert!(entry.children[1].restricted && entry.smaller.is_none());
    }
}
