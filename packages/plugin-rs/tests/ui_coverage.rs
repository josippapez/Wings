//! The SDK has a builder for every kind of node the app draws. The kinds are read from the list `page/tree.rs` accepts
//! (`Tree::node` drops any other), so a kind added there fails this test until `src/ui.rs` builds it.

mod common;

use common::*;
use std::collections::BTreeSet;

/// Kinds the app accepts that the SDK has no builder for on purpose, and why.
const NOT_BUILT: &[(&str, &str)] = &[];

fn is_kind(text: &str) -> bool {
    text.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && text.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The kinds `Tree::node` keeps: the literals of its `matches!(kind.as_str(), ...)` list.
fn app_kinds() -> BTreeSet<String> {
    let tree = code(&read("apps/native/src/page/tree.rs"));
    let list = between(&tree, "kind.as_str(),\n            \"Slot\"", "return Ok(None)");
    literals(list).into_iter().map(|(name, ..)| name).filter(|name| is_kind(name)).collect()
}

/// The kinds the SDK makes: every `Node::new("<Kind>")`.
fn sdk_kinds() -> BTreeSet<String> {
    let mut files = Vec::new();
    sources(&root().join("packages/plugin-rs/src"), &mut files);
    let mut kinds = BTreeSet::new();
    for path in files {
        let source = code(without_tests(&std::fs::read_to_string(path).unwrap()));
        for piece in source.split("Node::new(").skip(1) {
            if let Some(rest) = piece.strip_prefix('"')
                && let Some(end) = rest.find('"')
            {
                kinds.insert(rest[..end].to_owned());
            }
        }
    }
    kinds
}

#[test]
fn the_scan_reads_the_app() {
    let kinds = app_kinds();
    for known in ["Page", "Stack", "Gauge", "Sunburst", "Menu", "Terminal"] {
        assert!(kinds.contains(known), "{known} is not in {kinds:?}");
    }
    assert!(kinds.len() > 50, "{kinds:?}");
}

#[test]
fn every_kind_the_app_draws_has_a_builder() {
    let built = sdk_kinds();
    let missing: Vec<_> = app_kinds().into_iter().filter(|kind| !built.contains(kind) && !NOT_BUILT.iter().any(|(skipped, _)| skipped == kind)).collect();
    assert!(missing.is_empty(), "the app draws these kinds and the SDK builds none of them (add a builder to src/ui.rs): {missing:?}");
}

#[test]
fn the_sdk_builds_only_kinds_the_app_draws() {
    let drawn = app_kinds();
    let wrong: Vec<_> = sdk_kinds().into_iter().filter(|kind| !drawn.contains(kind)).collect();
    assert!(wrong.is_empty(), "the SDK builds these kinds and the app drops them: {wrong:?}");
    for (kind, _) in NOT_BUILT {
        assert!(drawn.contains(*kind), "NOT_BUILT names {kind}, which the app no longer draws");
    }
}
