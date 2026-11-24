//! What the coverage tests share: reading the app's source and pulling the names out of it.

#![allow(dead_code)]

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn read(path: &str) -> String {
    let full = root().join(path);
    fs::read_to_string(&full).unwrap_or_else(|error| panic!("{}: {error}", full.display()))
}

/// The text of `source` without comment lines.
pub fn code(source: &str) -> String {
    source.lines().filter(|line| !line.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

/// The part of `source` from the first `from` to the next `to` after it. Panics when a marker moved, so a refactor of
/// the app's dispatch can't silently make the scan read nothing.
/// An SDK file without its `#[cfg(test)] mod tests`, whose mock calls and test nodes aren't wrappers or builders.
pub fn without_tests(source: &str) -> &str {
    source.split("#[cfg(test)]\nmod tests").next().unwrap_or(source)
}

pub fn between<'a>(source: &'a str, from: &str, to: &str) -> &'a str {
    let start = source.find(from).unwrap_or_else(|| panic!("`{from}` is gone from the app's source: update tests/host_coverage.rs"));
    let rest = &source[start..];
    let end = rest.find(to).unwrap_or_else(|| panic!("`{to}` is gone from the app's source: update tests/host_coverage.rs"));
    &rest[..end]
}

/// Every string literal in `source` with the two characters before and after it (whitespace skipped). Raw strings and
/// char literals are skipped over.
pub fn literals(source: &str) -> Vec<(String, String, String)> {
    let chars: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    let around = |from: usize, to: usize| {
        let mut before: Vec<char> = chars[..from].iter().rev().filter(|c| !c.is_whitespace()).take(2).copied().collect();
        before.reverse();
        let after: String = chars[to..].iter().filter(|c| !c.is_whitespace()).take(2).collect();
        (before.into_iter().collect::<String>(), after)
    };
    while i < chars.len() {
        match chars[i] {
            'r' if chars.get(i + 1) == Some(&'#') && chars.get(i + 2) == Some(&'"') => {
                i += 3;
                while !(chars[i] == '"' && chars.get(i + 1) == Some(&'#')) {
                    i += 1;
                }
                i += 2;
            }
            '\'' if chars.get(i + 2) == Some(&'\'') => i += 3,
            '\'' if chars.get(i + 1) == Some(&'\\') => {
                i += 2;
                while chars[i] != '\'' {
                    i += 1;
                }
                i += 1;
            }
            '"' => {
                let start = i + 1;
                i += 1;
                while chars[i] != '"' {
                    i += if chars[i] == '\\' { 2 } else { 1 };
                }
                let (before, after) = around(start - 1, i + 1);
                found.push((chars[start..i].iter().collect(), before, after));
                i += 1;
            }
            _ => i += 1,
        }
    }
    found
}

pub fn is_name(text: &str) -> bool {
    text.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && text.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The names of the arms of a `match` in `source`: a literal that is followed by `=>`, or joined to another by `|`.
pub fn arms(source: &str) -> BTreeSet<String> {
    literals(source)
        .into_iter()
        .filter(|(name, before, after)| is_name(name) && (after.starts_with("=>") || (after.starts_with('|') && !after.starts_with("||")) || before == "\"|"))
        .map(|(name, ..)| name)
        .collect()
}


/// Every `.rs` file under `dir`.
pub fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// What the SDK's modules send or hear: the literal that follows each of `openers`, like `call(` or `on(`.
pub fn sdk_names(openers: &[&str]) -> BTreeSet<String> {
    let mut files = Vec::new();
    sources(&root().join("packages/plugin-rs/src"), &mut files);
    let mut names = BTreeSet::new();
    for path in files {
        let source = code(without_tests(&fs::read_to_string(&path).unwrap()));
        for open in openers {
            for piece in source.split(open).skip(1) {
                if let Some(rest) = piece.strip_prefix('"')
                    && let Some(end) = rest.find('"')
                {
                    names.insert(rest[..end].to_owned());
                }
            }
        }
    }
    names
}

