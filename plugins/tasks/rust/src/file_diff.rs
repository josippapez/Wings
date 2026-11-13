//! A changed file's diff, for a row of Files changed.

use serde_json::json;
use wings_plugin::{diff::open_live, format::basename, git};

/// The added and removed line counts in a journal's `changed (+5 -2)`, or none when it says something else.
pub fn line_counts(what: &str) -> Option<(u64, u64)> {
    let counts = what.trim().strip_prefix("changed (+")?.strip_suffix(')')?;
    let (added, removed) = counts.split_once(" -")?;
    let number = |text: &str| (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())).then(|| text.parse().ok()).flatten();
    Some((number(added)?, number(removed)?))
}

/// The git diff for one file of the issue: from before its first linked commit to the working tree, or against HEAD without one.
pub fn diff_args(first_commit: Option<&str>, path: &str) -> Vec<String> {
    let from = first_commit.map_or_else(|| "HEAD".to_owned(), |commit| format!("{commit}^"));
    ["diff", &from, "--no-color", "--no-ext-diff", "--", path].map(str::to_owned).to_vec()
}

/// Opens the diff viewer at once, fills it when git answers, so a click never waits, and keeps it up to date while it's
/// open. A path from the journal can't lead out of the project: one with `..` or that starts at the root is refused.
pub async fn open_file_diff(first_commit: Option<String>, path: String, root: String) -> Result<(), String> {
    if path.starts_with('/') || path.split('/').any(|part| part == "..") {
        return Err("That file is outside the project, so there is no diff to show.".into());
    }
    let subtitle = match &first_commit {
        Some(commit) => format!("Since before {}, with what isn't committed yet", commit.chars().take(7).collect::<String>()),
        None => "Working tree against HEAD".to_owned(),
    };
    let key = root.clone();
    let diff = json!({ "title": basename(&path), "subtitle": subtitle, "cwd": root });
    open_live(diff, &key, move |_fresh| {
        let (first_commit, path, root) = (first_commit.clone(), path.clone(), root.clone());
        async move {
            let run = |from: Option<&str>| {
                let args = diff_args(from, &path);
                let root = root.clone();
                async move { git::run(&root, &args.iter().map(String::as_str).collect::<Vec<_>>()).await }
            };
            let mut output = run(first_commit.as_deref()).await?;
            // A commit that's gone, like after a rebase, leaves the working tree against HEAD.
            if output.code != 0 && first_commit.is_some() {
                output = run(None).await?;
            }
            if output.code != 0 {
                let said = output.stderr.trim();
                return Err(if said.is_empty() { "git diff failed".to_owned() } else { said.to_owned() });
            }
            Ok(if output.stdout.trim().is_empty() { json!({ "error": "No changes to show: it may be committed already before this issue, or not tracked yet." }) } else { json!({ "patch": output.stdout }) })
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_reads_its_line_counts_from_the_journals_text_and_anything_else_stays_text() {
        assert_eq!(line_counts("changed (+15 -6)"), Some((15, 6)));
        assert_eq!(line_counts("renames X to Y"), None);
        assert_eq!(line_counts("changed (+x -6)"), None);
    }

    #[test]
    fn a_files_diff_starts_before_the_issues_first_commit_or_at_head_without_one() {
        assert_eq!(diff_args(Some("abc1234"), "src/a.ts"), ["diff", "abc1234^", "--no-color", "--no-ext-diff", "--", "src/a.ts"]);
        assert_eq!(diff_args(None, "src/a.ts"), ["diff", "HEAD", "--no-color", "--no-ext-diff", "--", "src/a.ts"]);
    }
}
