//! The commands that start an agent in a pane, one entry per agent CLI. Adding an agent is an entry in `AGENTS` and its two
//! commands in the manifest's `permissions.panes`.

use crate::domain::is_key;

pub struct AgentCli {
    pub kind: &'static str,
    /// What the UI calls it.
    pub label: &'static str,
    /// Starts it with a first prompt, as `openPane`'s `command`; the prompt goes in `args`.
    pub start: &'static str,
    /// Whether `id` has the shape of its session ids: they're typed into a shell, so nothing else is accepted.
    session_id: fn(&str) -> bool,
    /// Continues a session, as `openPane`'s `command`.
    resume: fn(&str) -> String,
}

fn within(id: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub const AGENTS: [AgentCli; 2] = [
    AgentCli { kind: "claude", label: "Claude Code", start: "claude", session_id: |id| within(id, 8, 64), resume: |id| format!("claude --resume {id}") },
    // `opencode --help`: --session continues a session, --prompt is the first message.
    AgentCli {
        kind: "opencode",
        label: "OpenCode",
        start: "opencode --prompt",
        session_id: |id| id.strip_prefix("ses_").is_some_and(|rest| (8..=64).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_alphanumeric())),
        resume: |id| format!("opencode --session {id}"),
    },
];

/// A session's agent. One recorded before sessions had a kind is Claude Code's, the only agent Wings followed then.
pub fn cli_of(kind: Option<&str>) -> &'static AgentCli {
    AGENTS.iter().find(|a| Some(a.kind) == kind).unwrap_or(&AGENTS[0])
}

pub fn agent_label(kind: Option<&str>) -> String {
    cli_of(kind).label.to_owned()
}

/// The command that continues a session. Pane commands are typed into the user's shell, so an id that isn't the shape of
/// its agent's is refused.
pub fn resume_command(id: &str, kind: Option<&str>) -> Result<String, String> {
    let agent = cli_of(kind);
    if !(agent.session_id)(id) {
        return Err(format!("That isn't a {} session id.", agent.label));
    }
    Ok((agent.resume)(id))
}

/// The agent with its first prompt for the issue with this key, as `openPane`'s `command` and `args`. It takes the
/// issue's own key: a tab can be opened with another spelling of it.
pub fn agent_command(key: &str, kind: Option<&str>) -> Result<(String, Vec<String>), String> {
    if !is_key(key) {
        return Err("That isn't an issue key.".into());
    }
    let prompt = format!("Continue issue {key}. Read it with tasks__get, do the work, and log progress with tasks__log and the status with tasks__update as you go.");
    Ok((cli_of(kind).start.to_owned(), vec![prompt]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wings takes command words of letters, digits and -_./:=@%+,.
    fn plain(word: &str) -> bool {
        !word.is_empty() && word.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.:/=@%+,-".contains(&b))
    }

    #[test]
    fn resume_and_start_commands_are_plain_words_wings_accepts_and_refuse_anything_else() {
        let id = "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11";
        // A session recorded before sessions had a kind is Claude Code's.
        let resume = resume_command(id, None).unwrap();
        assert_eq!(resume, format!("claude --resume {id}"));
        assert!(resume.split(' ').all(plain));
        let (command, args) = agent_command("WEB-12", None).unwrap();
        assert_eq!(command, "claude");
        assert_eq!(args.len(), 1);
        assert!(args[0].starts_with("Continue issue WEB-12. Read it with tasks__get"));
        assert!(!args[0].starts_with('-') && !args[0].chars().any(|c| c < ' ' || "'`$".contains(c)));
        for bad in ["x; rm -rf ~", "$(id)", "a b", "", "../x", "id'quote"] {
            assert!(resume_command(bad, None).unwrap_err().contains("session id"), "{bad}");
        }
        for bad in ["WEB-1; ls", "web-1", "WEB", "WEB-1 x", "$(id)-1", "WEBS-1"] {
            assert!(agent_command(bad, None).unwrap_err().contains("issue key"), "{bad}");
        }
    }

    #[test]
    fn each_agent_resumes_its_own_sessions_and_starts_with_the_prompt_as_one_arg() {
        assert_eq!(resume_command("ses_abc12345XYZ", Some("opencode")).unwrap(), "opencode --session ses_abc12345XYZ");
        assert_eq!(resume_command("6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", Some("claude")).unwrap(), "claude --resume 6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11");
        // A session id of one agent isn't taken for another's.
        assert!(resume_command("6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", Some("opencode")).unwrap_err().contains("OpenCode session id"));
        assert!(resume_command("ses_x; ls", Some("opencode")).unwrap_err().contains("session id"));
        let (command, args) = agent_command("WEB-12", Some("opencode")).unwrap();
        assert_eq!(command, "opencode --prompt");
        assert!(args[0].starts_with("Continue issue WEB-12. "));
        for agent in &AGENTS {
            assert!(agent.start.split(' ').all(plain));
        }
    }
}
