//! The guard for the code of the window's pieces and pages (`src/workspace`, `src/page`): nothing there steps the view
//! it is drawn in, which redraws that view, the piece around it and, in a page, the whole page at every step. What
//! moves goes in a live slot (`live::slot`) instead. Each place that still steps its view in place is listed, with why.

use std::{fs, path::Path};

/// A repeating `Animation`: GPUI draws the view it is in again at every frame it asks for.
const REPEATING: &str = "a repeating animation";
/// A loop on a timer that notifies: the view it notifies draws again at each turn.
const TIMED: &str = "a timer that notifies";
/// Something stepping on the clock in place (`live::ticking_every`, or what calls it: an in-place shimmer, glow,
/// spinner or working dot), which draws the view it is in again at each step.
const IN_PLACE: &str = "motion in place";

/// The places that step in place, by file under `src`, the function they are in and what they are, and why each may.
const ALLOWED: &[(&str, &str, &str, &str)] = &[
    (
        "page/view.rs",
        "text_element",
        REPEATING,
        "a `since` in a page no piece puts on screen with the layers (in a window of its own, under its own dialog) counts in place, once a second",
    ),
    ("page/view.rs", "text_element", IN_PLACE, "the same page's shimmer pulses in place"),
    ("page/view.rs", "node", REPEATING, "the same page's spinner turns in place"),
    ("page/view.rs", "node", IN_PLACE, "the same page's working dot turns in place"),
    (
        "page/view.rs",
        "empty",
        IN_PLACE,
        "known: the empty chat's glow steps its page 10 times a second; next, it moves to a layer",
    ),
    (
        "workspace/plugin_manage_ui.rs",
        "header_button",
        REPEATING,
        "known: a plugin manager button's ring while an install or update runs; next, it moves to a live slot",
    ),
    (
        "workspace/updater.rs",
        "render",
        REPEATING,
        "the updater's ring, in a view of its own (`Busy`), while an update is prepared",
    ),
    (
        "workspace/updater.rs",
        "report_progress",
        TIMED,
        "a download's progress, in the updater's own view, at most once a step while it downloads",
    ),
    (
        "workspace/project_page/mod.rs",
        "new",
        TIMED,
        "known: the project page's \"2 minutes ago\" draws the page once a minute while shown; next, a counting slot",
    ),
    (
        "workspace/history.rs",
        "history_header",
        IN_PLACE,
        "known: the history sheet's working dot while a search runs; next, a live slot",
    ),
    (
        "workspace/palette.rs",
        "palette_layer",
        IN_PLACE,
        "known: a working agent's dot in the palette's list, drawn by the window over its pieces; next, a live slot",
    ),
];

/// A place that steps in place: its file under `src`, the function it is in, and what it is.
#[derive(Debug, PartialEq)]
struct Step {
    file: String,
    function: String,
    what: &'static str,
}

/// The places in `source`, the file `file` under `src`, that step the view they are drawn in, outside its tests.
fn steps_in(file: &str, source: &str) -> Vec<Step> {
    let code = without_tests(&without_comments(source));
    let at = |offset: usize, what| Step { file: file.into(), function: function_at(&code, offset), what };
    let mut steps = Vec::new();
    for (offset, _) in code.match_indices("Animation::new(") {
        let after = close_of(&code, offset + "Animation::new".len(), '(', ')');
        if code[after..].trim_start().starts_with(".repeat()") {
            steps.push(at(offset, REPEATING));
        }
    }
    for keyword in ["loop {", "while "] {
        for (offset, _) in code.match_indices(keyword) {
            let Some(open) = code[offset..].find('{').map(|at| offset + at) else { continue };
            let body = &code[open..close_of(&code, open, '{', '}')];
            if body.contains(".timer(") && body.contains("notify(") {
                steps.push(at(offset, TIMED));
            }
        }
    }
    for mover in ["ticking_every(", "ui::shimmer(", "ui::glow(", "spinning()", "status_dot("] {
        for (offset, _) in code.match_indices(mover) {
            let still = ["AgentState::Blocked", "AgentState::Done", "AgentState::Idle"]
                .iter()
                .any(|state| code[offset + mover.len()..].starts_with(state));
            if !(mover == "status_dot(" && still) {
                steps.push(at(offset, IN_PLACE));
            }
        }
    }
    steps
}

/// `source` with each line comment blanked, so what a comment says isn't taken for code.
fn without_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `code` without its test modules: a `mod` under a `#[cfg(...)]` that names `test`.
fn without_tests(code: &str) -> String {
    let mut kept = String::new();
    let mut rest = code;
    while let Some(at) = rest.find("#[cfg(") {
        let attribute_end = rest[at..].find(")]").map_or(rest.len(), |end| at + end + 2);
        let attribute = &rest[at..attribute_end];
        let after = rest[attribute_end..].trim_start();
        // A module in the file, not one in a file of its own (`mod tests;`).
        let inline = after.find('{').is_some_and(|open| after.find(';').is_none_or(|end| open < end));
        if attribute.contains("test") && after.starts_with("mod ") && inline {
            kept.push_str(&rest[..at]);
            let open = attribute_end + rest[attribute_end..].find('{').expect("the module's block");
            rest = &rest[close_of(rest, open, '{', '}')..];
        } else {
            kept.push_str(&rest[..attribute_end]);
            rest = &rest[attribute_end..];
        }
    }
    kept.push_str(rest);
    kept
}

/// Where the bracket `open` at `at` in `code` closes, just after it.
fn close_of(code: &str, at: usize, open: char, close: char) -> usize {
    let mut depth = 0;
    for (offset, c) in code[at..].char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return at + offset + 1;
            }
        }
    }
    code.len()
}

/// The name of the last function declared in `code` before `offset`.
fn function_at(code: &str, offset: usize) -> String {
    code[..offset]
        .rmatch_indices("fn ")
        .find(|(at, _)| *at == 0 || code[..*at].ends_with(|c: char| c.is_whitespace()))
        .map(|(at, _)| code[at + 3..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect())
        .unwrap_or_default()
}

/// The Rust files under `dir`, outside test files, with their paths from `src`.
fn sources(src: &Path, dir: &Path, found: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).expect("the folder") {
        let path = entry.expect("an entry").path();
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        if path.is_dir() {
            if name != "tests" {
                sources(src, &path, found);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
            let file = path.strip_prefix(src).expect("under src").to_string_lossy().into_owned();
            found.push((file, fs::read_to_string(&path).expect("the file")));
        }
    }
}

/// A piece or a page that steps itself draws all of itself again at each step: one "Running · 12s" rebuilt a whole chat
/// each second, a page's own spinner 30 times a second.
#[test]
fn nothing_in_a_piece_or_a_page_steps_it_but_the_places_listed() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&src, &src.join("page"), &mut files);
    sources(&src, &src.join("workspace"), &mut files);
    let steps: Vec<Step> = files.iter().flat_map(|(file, source)| steps_in(file, source)).collect();
    let allowed = |step: &Step| {
        ALLOWED.iter().any(|(file, function, what, _)| step.file == *file && step.function == *function && step.what == *what)
    };
    let offences: Vec<&Step> = steps.iter().filter(|step| !allowed(step)).collect();
    assert!(
        offences.is_empty(),
        "these step the piece or page they are drawn in; put what moves in a live slot (`ui::live::slot`): {offences:#?}"
    );
    let stale: Vec<_> = ALLOWED
        .iter()
        .filter(|(file, function, what, _)| {
            !steps.iter().any(|step| step.file == *file && step.function == *function && step.what == *what)
        })
        .collect();
    assert!(stale.is_empty(), "these are allowed but no longer there; take them off the list: {stale:#?}");
}

#[test]
fn the_guard_finds_a_repeating_animation_and_a_timer_that_notifies_and_not_what_is_fine() {
    let source = r#"
        // A comment about Animation::new(d).repeat() is no code.
        fn render(&mut self) -> impl IntoElement {
            div().with_animation("turn", Animation::new(Duration::from_secs(1))
                .repeat(), |el, turn| el)
        }
        fn rise(&mut self) -> impl IntoElement {
            div().with_animation("rise", Animation::new(RISE), |el, t| el.opacity(t))
        }
        fn start(cx: &mut Context<Self>) {
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(EVERY).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            });
        }
        fn dot(state: AgentState) -> AnyElement {
            div().child(ui::status_dot(state)).child(ui::status_dot(AgentState::Done))
        }
        #[cfg(test)]
        mod tests {
            fn spins() { Animation::new(STEP).repeat(); }
        }
    "#;
    let found: Vec<(String, &str)> = steps_in("page/view.rs", source).into_iter().map(|step| (step.function, step.what)).collect();
    assert_eq!(
        found,
        [("render".into(), REPEATING), ("start".into(), TIMED), ("dot".into(), IN_PLACE)],
        "a repeating animation, a timer that notifies and a working dot in place; not a tween, a done dot, a comment or a test"
    );
}
