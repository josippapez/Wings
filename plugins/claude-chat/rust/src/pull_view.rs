//! The side panel's "Pull request" section (`pull.rs`): the PR of the chat's branch with its state, review and checks, and
//! the buttons that act on it; or, with none, a button that has Claude make one. Merging asks in a dialog first.

use wings_plugin::{
    host,
    ui::{self, Node, badge, dialog as panel, list_button, muted, notice, row, spinner, stack, text},
};

use crate::{
    chat::Chat,
    pull::{self, Load, Merge, Method, Pr, State},
};

const WIDTH: u32 = 480;

fn small(said: impl Into<String>) -> Node {
    text(said).prop("size", "small")
}

/// The badge beside the title: what the PR is.
fn state_badge(pr: &Pr) -> Node {
    match (pr.state, pr.draft) {
        (State::Merged, _) => badge("Merged", Some("success")),
        (State::Closed, _) => badge("Closed", Some("danger")),
        (State::Open, true) => badge("Draft", None),
        (State::Open, false) => badge("Open", Some("info")),
    }
}

/// The review and the checks, in one line, red when a check failed. A merged or closed PR has no more to say.
fn status_line(pr: &Pr) -> Option<Node> {
    if pr.state != State::Open {
        return None;
    }
    let (_, failed, running, queued) = pr.checks;
    let tone = if failed > 0 { "danger" } else if running + queued > 0 { "muted" } else { "success" };
    let said = if pr.review.is_empty() { pull::checks_text(pr.checks) } else { format!("{} · {}", pr.review, pull::checks_text(pr.checks)) };
    Some(crate::view::inset(small(said).prop("tone", tone).prop("truncate", true)))
}

/// Which checks are still to finish, one muted line each, so "1 queued" says which.
fn pending_lines(pr: &Pr) -> Vec<Node> {
    if pr.state != State::Open {
        return Vec::new();
    }
    let now = host::now();
    pr.pending.iter().map(|check| crate::view::inset(small(pull::pending_text(check, now)).prop("tone", "muted").prop("size", "tiny").prop("truncate", true))).collect()
}

fn found(c: &Chat, pr: &Pr) -> Vec<Node> {
    let open = pr.url.clone();
    let title = row()
        .prop("grow", true)
        .prop("gap", "s")
        .prop("align", "center")
        .child(small(pr.reference.as_str()).prop("mono", true))
        .child(stack().prop("grow", true).child(small(pr.title.as_str()).prop("tone", "muted").prop("truncate", true)))
        .child(state_badge(pr));
    // A click opens it in Wings' own PR tab, as a PR the conversation names does.
    let head = list_button([title], move || crate::refs_view::open_pull_request(&open)).prop("title", pr.title.as_str());
    let url = pr.url.clone();
    let no_claude = c.input.is_none();
    let mut actions = row().prop("gap", "s").prop("wrap", true).child(ui::button("Open", "outline", false, move || host::open_url(&url)).prop("size", "sm").prop("title", "Open it in the browser"));
    if pr.state == State::Open && pr.github && pr.mergeable {
        actions = actions.child(ui::button("Merge", "outline", false, pull::ask_merge).prop("size", "sm").prop("title", "Merge it on GitHub, after you confirm"));
    }
    if pr.state == State::Open && pr.checks.1 > 0 {
        actions = actions.child(ui::button("Why failing", "outline", false, pull::explain_failing).prop("size", "sm").prop("disabled", no_claude).prop("title", "Ask Claude to read the failed checks' logs and fix them"));
    }
    [Some(head), status_line(pr)].into_iter().flatten().chain(pending_lines(pr)).chain([crate::view::inset(actions)]).collect()
}

/// The section, with nothing in a folder no forge knows, a chat that isn't on a branch, or before the first read.
pub fn section(c: &Chat) -> Option<Node> {
    let branch = c.branch.as_deref()?;
    pull::with(|p| {
        if p.branch.as_deref() != Some(branch) {
            return None;
        }
        let rows = match &p.load {
            Load::Idle | Load::Unsupported => return None,
            Load::Found(pr) => found(c, pr),
            // No PR: no section. The message box offers Create PR (`create_button`).
            Load::NoPr => return None,
            Load::Failed(why) => vec![crate::view::inset(small(why.as_str()).prop("tone", "danger"))],
        };
        Some(crate::view::fold(c, "pull", "Pull request", rows))
    })
}

/// Lucide's `git-pull-request-arrow`, in a 24-point box.
const PR_ICON: [&str; 4] = ["M6 3a3 3 0 1 0 0 6a3 3 0 1 0 0-6z", "M6 9v12", "M18 15a3 3 0 1 0 0 6a3 3 0 1 0 0-6z", "M13 6h3a2 2 0 0 1 2 2v7"];

/// The message box's Create PR, beside the branch button, while the branch has none and isn't the default one.
pub fn create_button(c: &Chat) -> Option<Node> {
    if !pull::can_create(c) {
        return None;
    }
    let glyph = ui::icon(24, 2.0, &PR_ICON).prop("size", 14);
    let button = ui::button("", "ghost", false, pull::create).prop("disabled", c.input.is_none()).prop("label", "Create pull request").prop("title", "Ask Claude to push the branch and open a pull request for it");
    Some(if c.narrow == Some(true) { button.prop("size", "icon-sm").prop("round", true).child(glyph) } else { button.prop("size", "sm").child(glyph).child(text("Create PR")) }.key("create-pr"))
}

/// The merge dialog, while it asks, runs or failed.
pub fn dialog(_c: &Chat) -> Option<Node> {
    pull::with(|p| {
        let Load::Found(pr) = &p.load else { return None };
        if p.merge == Merge::Idle {
            return None;
        }
        let busy = p.merge == Merge::Running;
        let choice = |label: &str, variant: &str, how: Method| ui::button(label, variant, false, move || pull::merge(how)).prop("disabled", busy);
        let cancel = ui::button("Cancel", "ghost", false, pull::cancel_merge).prop("disabled", busy);
        let actions = ui::actions(
            Some(cancel),
            busy.then(spinner).into_iter().chain([
                choice("Rebase", "outline", Method::Rebase),
                choice("Merge commit", "outline", Method::Merge),
                choice("Squash", "default", Method::Squash),
            ]),
        );
        let error = match &p.merge {
            Merge::Failed(why) => Some(notice("danger", why.as_str())),
            _ => None,
        };
        Some(
            panel("Merge pull request", WIDTH, pull::cancel_merge)
                .child(text(format!("{} {}", pr.reference, pr.title)).prop("weight", "medium"))
                .child(muted(format!("{} · {}", pr.review, pull::checks_text(pr.checks))).prop("size", "small"))
                .child(notice("warn", "This merges the pull request on GitHub for everyone on the project. It can't be undone from here."))
                .maybe(error)
                .child(actions),
        )
    })
}
