//! What headless Claude Code refuses as a request, which the chat does by starting Claude Code again in the tab: forking
//! the session (`--fork-session`), forking before one of your messages (the fork, then Claude Code's own
//! `rewind_conversation` in the opening input), and adding a folder (`--add-dir`, which stays for the tab).
//!
//! Each restart answers one click or key, and nothing is written to the Claude Code that is stopped: what the new one is
//! asked goes with its start. What the buttons do is in `fork_view.rs`.

use serde_json::json;

use crate::session::{Prompt, REWIND_CONVERSATION, request};

/// How Claude Code is started in the tab, beyond the session it resumes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Launch {
    /// Start as a copy of the resumed session, under a new id. The session it was left as.
    pub fork: bool,
    /// With `fork`: cut the copy before this message of yours, by its uuid.
    pub before: Option<String>,
}

/// The flags that follow the fixed ones: the session to resume, whether to copy it, and the folders Claude may reach.
/// Each value is joined to its flag, as Wings passes a program nothing else.
pub fn flags(resume: Option<&str>, how: &Launch, dirs: &[String]) -> Vec<String> {
    let mut flags: Vec<String> = resume.map(|id| format!("--resume={id}")).into_iter().collect();
    if how.fork && resume.is_some() {
        flags.push("--fork-session".into());
    }
    flags.extend(dirs.iter().map(|dir| format!("--add-dir={dir}")));
    flags
}

/// The flag that starts Claude Code as the agent `name`, which takes on that agent's prompt, tools and model for the whole
/// session. None for no agent, and for a name that isn't one: agents are named in letters, digits and `-_.:`, and the name
/// comes from the tab's saved state, so it is checked before it goes on a command line.
pub fn agent_flag(name: Option<&str>) -> Option<String> {
    let name = name.filter(|name| !name.starts_with('-') && !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.:".contains(c)))?;
    Some(format!("--agent={name}"))
}

/// What a fork before a message asks as it starts: Claude Code's `rewind_conversation` to that message, which drops it and
/// what followed, and gives its text back for the message box. Only the conversation: your files stay as they are. None for
/// a plain fork, or a message the history doesn't hold.
pub fn opening(how: &Launch, prompts: &[Prompt]) -> String {
    let (Some(target), true) = (how.before.as_deref(), how.fork) else {
        return String::new();
    };
    match prompts.iter().find(|prompt| prompt.uuid == target).zip(prompts.last()) {
        // Claude Code refuses to cut a conversation with messages after the one named that it can't tell you saw.
        Some((_, last)) => request(REWIND_CONVERSATION, json!({ "subtype": "rewind_conversation", "target_message_uuid": target, "last_seen_user_message_uuid": last.uuid })),
        None => String::new(),
    }
}

/// The path what follows `/add-dir` names, without the quotes around a path with spaces in it.
pub fn unquoted(path: &str) -> &str {
    path.trim().trim_matches(|c: char| c == '"' || c == '\'').trim()
}

/// The folder as Claude Code is given it: a full path as it is, anything else from the chat's folder.
pub fn resolve(cwd: &str, path: &str) -> String {
    let full = path.starts_with('/') || path.as_bytes().get(1) == Some(&b':');
    let joined = if full { path.to_owned() } else { format!("{}/{path}", cwd.trim_end_matches('/')) };
    match joined.trim_end_matches('/') {
        "" => "/".to_owned(),
        trimmed => trimmed.to_owned(),
    }
}

/// Adds a folder. False when the chat has it already.
pub fn add(dirs: &mut Vec<String>, dir: String) -> bool {
    let new = !dirs.contains(&dir);
    if new {
        dirs.push(dir);
    }
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_goes_on_the_command_line_joined_to_its_flag_and_only_if_it_is_a_name() {
        assert_eq!(agent_flag(Some("Explore")).as_deref(), Some("--agent=Explore"));
        assert_eq!(agent_flag(Some("dev-core:mid-tier-coder")).as_deref(), Some("--agent=dev-core:mid-tier-coder"));
        assert_eq!(agent_flag(None), None);
        assert_eq!(agent_flag(Some("")), None);
        assert_eq!(agent_flag(Some("--dangerously-skip-permissions")), None);
        assert_eq!(agent_flag(Some("a b")), None);
    }

    #[test]
    fn initialize_lists_the_agents_to_start_as() {
        let mut options = crate::composer::Options::default();
        options.initialized(&json!({ "agents": [{ "name": "Plan", "description": "Plans" }, { "description": "no name" }] }));
        assert_eq!(options.agent_types, [("Plan".to_owned(), "Plans".to_owned())]);
    }

    fn prompts(uuids: &[&str]) -> Vec<Prompt> {
        uuids.iter().enumerate().map(|(at, uuid)| Prompt { at, uuid: (*uuid).to_owned(), parent: None }).collect()
    }

    #[test]
    fn a_fork_resumes_the_session_and_copies_it_with_the_folders_after() {
        let dirs = ["/work/docs".to_owned(), "/work/shared".to_owned()];
        assert_eq!(
            flags(Some("abc"), &Launch { fork: true, before: None }, &dirs),
            ["--resume=abc", "--fork-session", "--add-dir=/work/docs", "--add-dir=/work/shared"]
        );
        assert_eq!(flags(Some("abc"), &Launch::default(), &dirs[..1]), ["--resume=abc", "--add-dir=/work/docs"]);
        assert_eq!(flags(None, &Launch::default(), &[]), Vec::<String>::new());
        // A copy of nothing isn't asked for.
        assert_eq!(flags(None, &Launch { fork: true, before: None }, &[]), Vec::<String>::new());
    }

    #[test]
    fn a_fork_before_a_message_asks_for_the_conversation_rewind_only() {
        let how = Launch { fork: true, before: Some("m2".into()) };
        let sent = opening(&how, &prompts(&["m1", "m2", "m3"]));
        let line: serde_json::Value = serde_json::from_str(sent.trim()).unwrap();
        assert_eq!(sent.matches('\n').count(), 1, "one request, no file rewind");
        assert_eq!(line["request_id"], REWIND_CONVERSATION);
        assert_eq!(
            line["request"],
            json!({ "subtype": "rewind_conversation", "target_message_uuid": "m2", "last_seen_user_message_uuid": "m3" })
        );
    }

    #[test]
    fn nothing_is_rewound_for_a_plain_fork_or_a_message_the_history_lacks() {
        assert_eq!(opening(&Launch { fork: true, before: None }, &prompts(&["m1"])), "");
        assert_eq!(opening(&Launch { fork: true, before: Some("gone".into()) }, &prompts(&["m1"])), "");
        // Without the fork it would cut the session itself.
        assert_eq!(opening(&Launch { fork: false, before: Some("m1".into()) }, &prompts(&["m1"])), "");
    }

    #[test]
    fn add_dir_takes_its_path_without_quotes() {
        assert_eq!(unquoted("/work/docs"), "/work/docs");
        assert_eq!(unquoted("  \"/work/my docs\"  "), "/work/my docs");
        assert_eq!(unquoted(""), "");
    }

    #[test]
    fn a_folder_is_a_full_path_or_from_the_chats_folder() {
        assert_eq!(resolve("/work/shop", "/work/docs/"), "/work/docs");
        assert_eq!(resolve("/work/shop/", "../docs"), "/work/shop/../docs");
        assert_eq!(resolve("/work/shop", "docs"), "/work/shop/docs");
        assert_eq!(resolve("/work/shop", "C:\\work"), "C:\\work");
        assert_eq!(resolve("/work/shop", "/"), "/");
    }

    #[test]
    fn a_folder_is_added_once() {
        let mut dirs = Vec::new();
        assert!(add(&mut dirs, "/work/docs".into()));
        assert!(!add(&mut dirs, "/work/docs".into()));
        assert_eq!(dirs, ["/work/docs"]);
    }
}
