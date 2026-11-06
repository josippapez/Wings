//! The flags a running `claude` was started with, so a restore can type them again after `--resume <id>`.
//! Arguments are read the way Claude Code's commander parser reads them: that is the only way to tell a
//! flag's value from a prompt given as a plain argument (`claude --model opus "fix the bug"`).

use std::{collections::VecDeque, ffi::OsStr, ffi::OsString, path::Path};

// The flag tables are copied from the argv pre-scan built into Claude Code 2.1.294. It covers the hidden flags
// too; `claude --help` lists the public ones.

/// Flags that take the next argument as their value, even one starting with `-`.
const VALUE: &[&str] = &[
    "--prefill", "--prefill-b64", "--deep-link-repo", "--deep-link-last-fetch", "--deep-link-cwd-b64", "--handle-uri",
    "--settings", "--managed-settings", "--setting-sources", "--client-data-url", "--watch-artifact",
    "--watch-artifact-no-autoreact", "--team-name", "--agent-id", "--agent-name", "--agent-color", "--parent-session-id",
    "--agent-type", "--model", "--agent", "--routine", "--effort", "--permission-mode", "--inherit-permission-mode",
    "--proactivity", "--debug-file", "--system-prompt", "--system-prompt-file", "--append-system-prompt",
    "--append-system-prompt-file", "--system-prompt-snapshot", "--append-subagent-system-prompt",
    "--append-subagent-system-prompt-file", "--plan-mode-instructions", "--permission-prompt-tool", "--permission-prompts",
    "--json-schema", "--fallback-model", "--advisor", "--agents", "--name", "-n", "--plugin-dir", "--plugin-dir-no-mcp",
    "--plugin-url", "--remote-control-session-name-prefix", "--sdk-url", "--exec", "-m", "--thinking", "--thinking-display",
    "--max-thinking-tokens", "--max-turns", "--max-budget-usd", "--task-budget", "--autocompact", "--rewind-files",
    "--resume-session-at", "--resume-drops-turn", "--workload", "--output-format", "--input-format", "--teammate-mode",
    "--messaging-socket-path", "--session-id", "--environment", "--pool", "--ref", "--on-branch", "--correlation-id",
    "--forward-home-settings", "--project-config-root", "--attach-serve",
];
/// Flags that take one value, then every following argument up to the next flag.
const VARIADIC: &[&str] = &[
    "--allowedTools", "--allowed-tools", "--disallowedTools", "--disallowed-tools", "--tools", "--add-dir", "--mcp-config",
    "--betas", "--file", "--channels", "--dangerously-load-development-channels",
];
/// Flags whose value is optional: they take the next argument only if it isn't a flag.
const OPTIONAL: &[&str] = &[
    "-d", "--debug", "-r", "--resume", "--from-pr", "-w", "--worktree", "--teleport", "--cloud", "--remote", "--project",
    "--remote-control", "--rc", "--prompt-suggestions",
];
/// Flags that take no value.
const SWITCH: &[&str] = &[
    "-d2e", "--debug-to-stderr", "--verbose", "-p", "--print", "--bare", "--safe-mode", "--init", "--init-only",
    "--maintenance", "--include-hook-events", "--include-partial-messages", "--forward-subagent-text", "--session-mirror",
    "--await-claim", "--await-initialize", "--dangerously-skip-permissions", "--allow-dangerously-skip-permissions",
    "--replay-user-messages", "--enable-auth-status", "--restricted", "--exclude-dynamic-system-prompt-sections", "-c",
    "--continue", "--fork-session", "--deep-link-origin", "--no-session-persistence", "--reply-on-resume", "--ide",
    "--desktop", "--strict-mcp-config", "--disable-slash-commands", "--chrome", "--no-chrome", "--tmux",
    "--enable-auto-mode", "--bg", "--background", "--brief", "--ax-screen-reader", "--plan-mode-required", "--hard-fail",
    "-h", "--help", "-v", "-V", "--version",
];

/// Flags that choose which session opens. A restore chooses it with `--resume <id>` instead.
const SESSION: &[&str] = &["-r", "--resume", "-c", "--continue", "--session-id", "--fork-session", "--from-pr", "--teleport"];

#[derive(Clone, Copy)]
enum Takes {
    Nothing,
    One,
    OneUnlessFlag,
    Many,
}

fn takes(flag: &str) -> Option<Takes> {
    if VARIADIC.contains(&flag) {
        Some(Takes::Many)
    } else if VALUE.contains(&flag) {
        Some(Takes::One)
    } else if OPTIONAL.contains(&flag) {
        Some(Takes::OneUnlessFlag)
    } else if SWITCH.contains(&flag) {
        Some(Takes::Nothing)
    } else {
        None
    }
}

/// Commander's test: two or more characters starting with `-`. A lone `-` is a plain argument.
fn is_flag(arg: &str) -> bool {
    arg.len() > 1 && arg.starts_with('-')
}

/// The flags in a `claude` process's argv. Native installs run as `claude …`, npm ones as `node …/cli.js …`.
pub fn launch_args(cmd: &[OsString]) -> Vec<String> {
    let program = cmd
        .iter()
        .position(|a| Path::new(a).file_name() == Some(OsStr::new("claude")) || a.to_string_lossy().contains("@anthropic-ai/claude-code"))
        .unwrap_or(0);
    let args: Vec<String> = cmd.iter().skip(program + 1).map(|a| a.to_string_lossy().into_owned()).collect();
    resume_args(&args)
}

/// Keeps the flags and their values. Drops the flags that choose a session, and every plain argument: the prompt,
/// a subcommand, anything after `--`.
pub fn resume_args(args: &[String]) -> Vec<String> {
    let mut rest: VecDeque<String> = args.iter().cloned().collect();
    let mut out = Vec::new();
    while let Some(arg) = rest.pop_front() {
        if arg == "--" {
            break;
        }
        if !is_flag(&arg) {
            continue;
        }
        let (name, group) = if let Some(how) = takes(&arg) {
            let mut group = vec![arg.clone()];
            take_values(how, &mut rest, &mut group);
            (arg, group)
        } else if let Some((short, attached)) = split_short(&arg).filter(|(short, _)| takes(short).is_some()) {
            // `-r<id>` carries its value; `-cp` is two switches, so `-p` goes back to be read on its own.
            if let Some(Takes::Nothing) = takes(&short) {
                rest.push_front(format!("-{attached}"));
                (short.clone(), vec![short])
            } else {
                (short, vec![arg])
            }
        } else if let Some((name, _)) = arg.split_once('=').filter(|(name, _)| name.starts_with("--")) {
            (name.to_string(), vec![arg])
        } else {
            // A flag from a newer Claude Code. The word after it is its value or the prompt, and replaying a prompt
            // would send it again, so both go.
            if rest.front().is_some_and(|next| !is_flag(next)) {
                rest.pop_front();
                continue;
            }
            (arg.clone(), vec![arg])
        };
        if !SESSION.contains(&name.as_str()) {
            out.extend(group);
        }
    }
    out
}

fn take_values(how: Takes, rest: &mut VecDeque<String>, group: &mut Vec<String>) {
    let next_is_value = |rest: &VecDeque<String>| rest.front().is_some_and(|a| !is_flag(a));
    match how {
        Takes::Nothing => {}
        Takes::One => group.extend(rest.pop_front()),
        Takes::OneUnlessFlag => {
            if next_is_value(rest) {
                group.extend(rest.pop_front());
            }
        }
        Takes::Many => {
            group.extend(rest.pop_front());
            while next_is_value(rest) {
                group.extend(rest.pop_front());
            }
        }
    }
}

/// `-xREST` as (`-x`, `REST`), for short flags written together or with their value attached.
fn split_short(arg: &str) -> Option<(String, &str)> {
    let mut chars = arg.strip_prefix('-')?.chars();
    let letter = chars.next().filter(|c| *c != '-')?;
    let attached = chars.as_str();
    (!attached.is_empty()).then(|| (format!("-{letter}"), attached))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0aa900ec-ed63-4df5-97c1-eecebeb8ef8d";

    fn clean(args: &[&str]) -> Vec<String> {
        resume_args(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn keeps_flags_and_their_values() {
        let args = ["--model", "opus", "--permission-mode", "plan", "--dangerously-skip-permissions", "--effort=high"];
        assert_eq!(clean(&args), args);
    }

    #[test]
    fn drops_resume_in_every_form() {
        let kept = ["--model", "opus"];
        assert_eq!(clean(&["--resume", ID, "--model", "opus"]), kept);
        assert_eq!(clean(&["-r", ID, "--model", "opus"]), kept);
        assert_eq!(clean(&[&format!("--resume={ID}"), "--model", "opus"]), kept);
        assert_eq!(clean(&[&format!("-r{ID}"), "--model", "opus"]), kept);
        // Without a value it opens the picker, and the next flag is not swallowed.
        assert_eq!(clean(&["--resume", "--model", "opus"]), kept);
        assert_eq!(clean(&["--model", "opus", "-r"]), kept);
    }

    #[test]
    fn drops_continue_session_id_and_fork() {
        let kept = ["--model", "opus"];
        assert_eq!(clean(&["-c", "--model", "opus"]), kept);
        assert_eq!(clean(&["--continue", "--model", "opus", "--fork-session"]), kept);
        assert_eq!(clean(&["--session-id", ID, "--model", "opus"]), kept);
        assert_eq!(clean(&[&format!("--session-id={ID}"), "--model", "opus"]), kept);
        assert_eq!(clean(&["--from-pr", "123", "--teleport", "--model", "opus"]), kept);
    }

    #[test]
    fn splits_short_switches_written_together() {
        assert_eq!(clean(&["-cd", "api"]), ["-d", "api"]);
        assert_eq!(clean(&["-pc"]), ["-p"]);
        assert_eq!(clean(&["-nFix bug"]), ["-nFix bug"]);
    }

    #[test]
    fn drops_a_prompt_given_as_an_argument() {
        assert_eq!(clean(&["fix the bug"]), Vec::<String>::new());
        assert_eq!(clean(&["--model", "opus", "fix the bug"]), ["--model", "opus"]);
        assert_eq!(clean(&["--dangerously-skip-permissions", "fix the bug"]), ["--dangerously-skip-permissions"]);
        assert_eq!(clean(&["--model", "opus", "--", "--not-a-flag"]), ["--model", "opus"]);
    }

    #[test]
    fn keeps_values_that_look_like_flags_or_hold_quotes() {
        let args = ["--append-system-prompt", "-be terse, say \"done\" and don't ask"];
        assert_eq!(clean(&args), args);
    }

    #[test]
    fn variadic_flags_take_words_until_the_next_flag() {
        let args = ["--add-dir", "../api", "../web", "--model", "opus", "fix the bug"];
        assert_eq!(clean(&args), ["--add-dir", "../api", "../web", "--model", "opus"]);
    }

    #[test]
    fn unknown_flags_never_replay_the_word_after_them() {
        assert_eq!(clean(&["--brand-new", "value", "--model", "opus"]), ["--model", "opus"]);
        assert_eq!(clean(&["--brand-new", "--model", "opus"]), ["--brand-new", "--model", "opus"]);
        assert_eq!(clean(&["--brand-new=value"]), ["--brand-new=value"]);
    }

    #[test]
    fn finds_the_program_in_argv() {
        let argv = |a: &[&str]| a.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(launch_args(&argv(&["claude", "--model", "opus"])), ["--model", "opus"]);
        assert_eq!(launch_args(&argv(&["/Users/me/.local/bin/claude", "-c", "--model", "opus"])), ["--model", "opus"]);
        let npm = ["node", "/usr/local/lib/node_modules/@anthropic-ai/claude-code/cli.js", "--model", "opus"];
        assert_eq!(launch_args(&argv(&npm)), ["--model", "opus"]);
    }
}
