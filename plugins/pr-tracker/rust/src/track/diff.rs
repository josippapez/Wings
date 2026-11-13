//! Opening a PR's diff in the viewer, from a pane's badge or from a PR tab, and replies typed in it.

use super::{state, status::{load_diff, refresh_status}};
use crate::{forges, providers::link_ref, types::{Comment, Link}};
use serde_json::{Value, json};
use wings_plugin::{diff as viewer, git};

/// Opens the viewer at once, in its loading state when the diff isn't cached, then fills it in, and shows a new push
/// while it's open. `cwd` is the folder of the agent working on the PR, so the viewer's Send to agent offers it first.
/// `focus` is the file and line the viewer scrolls to once the patch is there.
pub async fn open_diff_for(link: &Link, title: Option<&str>, cwd: Option<&str>, focus: Option<Value>) -> Result<(), String> {
    let loaded = state::status_of(&link.url).and_then(|loaded| loaded.entry.ok());
    let title = loaded.as_ref().map(|st| st.title.clone()).or_else(|| title.filter(|title| !title.is_empty()).map(str::to_owned)).unwrap_or_else(|| format!("Pull request {}", link_ref(link)));
    let mut diff = json!({ "title": title, "subtitle": format!("{} {}", link.repo, link_ref(link)) });
    if let Some(cwd) = cwd {
        diff["cwd"] = cwd.into();
    }
    if let Some(focus) = focus {
        diff["focus"] = focus;
    }
    let cached = state::with(|s| s.diffs.get(&link.url).cloned());
    if let (Some((key, cached)), Some(st)) = (cached, &loaded)
        && key.as_deref() == Some(st.key.as_str())
    {
        diff["patch"] = cached.patch.clone().into();
        diff["comments"] = json!(cached.comments);
    }
    let url = link.url.clone();
    let id = viewer::open_live_id(diff, &link.url, move |fresh| {
        let url = url.clone();
        async move {
            // A PR from the list or a tab has no pane keeping its status fresh, and Refresh asks the forge again.
            if fresh || state::status_of(&url).is_none() {
                refresh_status(&url, fresh).await;
            }
            if let Some(Err(error)) = state::status_of(&url).map(|loaded| loaded.entry) {
                return Err(error);
            }
            let diff = load_diff(&url).await?;
            Ok(json!({ "patch": diff.patch, "comments": diff.comments }))
        }
    })
    .await?;
    if let Some(id) = id.as_str() {
        state::add_viewer(id.to_owned(), link.url.clone());
    }
    Ok(())
}

/// One commit's changes against its first parent, as the forge's commit page shows them, from git in the project's folder.
pub async fn open_commit(link: &Link, cwd: &str, sha: &str, message: &str) -> Result<(), String> {
    if sha.len() != 40 || !sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err("That isn't a commit id.".into());
    }
    let title = if message.is_empty() { &sha[..7] } else { message };
    let diff = json!({ "title": title, "subtitle": format!("{} {}", link.repo, &sha[..7]), "cwd": cwd });
    let (cwd, sha) = (cwd.to_owned(), sha.to_owned());
    viewer::open_while(diff, async move {
        git::git(&cwd, &["fetch", "--quiet", "--end-of-options", "origin", &sha]).await?;
        Ok(json!({ "patch": git::raw(&cwd, &["diff", "--no-color", "--no-ext-diff", &format!("{sha}^"), &sha]).await? }))
    })
    .await
}

const GONE: &str = "This pull request isn't open in a pane any more. Reopen it from the badge.";

/// A reply typed in a diff viewer is posted to the PR it shows, then added to what it shows.
pub async fn reply_in_viewer(diff_id: &str, reply_to: i64, body: &str) -> Result<(), String> {
    let url = state::viewer_url(diff_id);
    let link = url.as_deref().and_then(state::link_for);
    let cached = url.as_ref().and_then(|url| state::with(|s| s.diffs.get(url).cloned()));
    let root: Option<Comment> = cached.as_ref().and_then(|(_, diff)| diff.comments.iter().find(|c| c.id == reply_to).cloned());
    let (Some(link), Some((key, cached)), Some(root)) = (link, cached, root) else {
        return Err(GONE.into());
    };
    if root.can_reply != Some(true) {
        return Err(GONE.into());
    }
    let reply = forges::reply(&link, &root, body).await.map_err(|error| error.to_string())?;
    let mut comments = cached.comments.clone();
    comments.push(reply);
    let diff = std::rc::Rc::new(crate::types::Diff { key: cached.key.clone(), patch: cached.patch.clone(), comments: comments.clone() });
    state::with(|s| s.diffs.insert(link.url.clone(), (key, diff)));
    viewer::update(&Value::String(diff_id.to_owned()), &json!({ "comments": comments })).await
}
