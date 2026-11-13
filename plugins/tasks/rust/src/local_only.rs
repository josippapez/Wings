//! Keep issues local: finds a Wings issue key in a command that writes a commit or a pull request, so the hook can ask for
//! it to be removed. The patterns are matched by hand, as the TypeScript build's regular expressions read them.

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether `text` starts at a word boundary after `at`, for text that ends the word there.
fn word_ends(text: &[u8], at: usize) -> bool {
    text.get(at).is_none_or(|b| !is_word(*b))
}

/// Where each `word` that stands alone starts in `command`.
fn words<'a>(command: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    command.match_indices(word).map(|(at, _)| at).filter(move |at| (*at == 0 || !is_word(command.as_bytes()[at - 1])) && word_ends(command.as_bytes(), at + word.len()))
}

/// The text after `git` and its options (`-c name=value`, `-C dir`, `--no-pager`, `-p`), starting at the subcommand.
/// More than one reading of an option is tried, as a pattern with alternatives would.
fn after_options(rest: &str) -> Vec<&str> {
    let after_space = rest.trim_start_matches(|c: char| c.is_whitespace());
    if after_space.len() == rest.len() {
        return Vec::new();
    }
    let mut found = vec![after_space];
    let token_end = after_space.find(|c: char| c.is_whitespace()).unwrap_or(after_space.len());
    let token = &after_space[..token_end];
    // `-c` and `-C` take the next word.
    if (token == "-c" || token == "-C") && token_end < after_space.len() {
        let value = after_space[token_end..].trim_start();
        if value.len() < after_space.len() - token_end {
            let end = value.find(|c: char| c.is_whitespace()).unwrap_or(value.len());
            if end > 0 {
                found.extend(after_options(&value[end..]));
            }
        }
    }
    // `-x`, `--flag`, `--flag=value`.
    let flag = token.strip_prefix("--").or_else(|| token.strip_prefix('-')).unwrap_or("");
    let name = flag.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')).unwrap_or(flag.len());
    if name > 0 && (name == flag.len() || (flag.as_bytes()[name] == b'=' && flag.len() > name + 1)) {
        found.extend(after_options(&after_space[token_end..]));
    }
    found
}

/// The text after each `git` and its options.
fn git_subcommands(command: &str) -> Vec<&str> {
    words(command, "git").flat_map(|at| after_options(&command[at + 3..])).collect()
}

fn starts_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    text.strip_prefix(word).filter(|rest| word_ends(rest.as_bytes(), 0))
}

/// `git tag` with `-a` or `-m` (also joined, as `-am`) before the end of its command.
fn annotated_tag(after_tag: &str) -> bool {
    let segment = after_tag.split(['\n', ';', '&', '|']).next().unwrap_or_default();
    let bytes = segment.as_bytes();
    (0..bytes.len().saturating_sub(1)).filter(|i| bytes[*i].is_ascii_whitespace() && bytes[i + 1] == b'-').any(|i| {
        let run = bytes[i + 2..].iter().take_while(|b| b.is_ascii_alphabetic()).count();
        run > 0 && matches!(bytes[i + 1 + run], b'a' | b'm') && word_ends(bytes, i + 2 + run)
    })
}

/// A word, whitespace, a second word and one of `actions`, as in `gh pr create`.
fn runs(command: &str, programs: &[&str], actions: &[&str]) -> bool {
    words(command, programs[0]).any(|at| {
        let mut rest = &command[at + programs[0].len()..];
        for program in &programs[1..] {
            let trimmed = rest.trim_start();
            if trimmed.len() == rest.len() {
                return false;
            }
            let Some(after) = starts_word(trimmed, program) else {
                return false;
            };
            rest = after;
        }
        let trimmed = rest.trim_start();
        trimmed.len() < rest.len() && actions.iter().any(|action| starts_word(trimmed, action).is_some())
    })
}

/// Whether the command commits, tags or writes to a pull request or issue on a host.
pub fn writes_to_history(command: &str) -> bool {
    git_subcommands(command).into_iter().any(|rest| {
        starts_word(rest, "commit").is_some() || starts_word(rest, "tag").is_some_and(annotated_tag)
    }) || runs(command, &["gh", "pr"], &["create", "edit", "comment", "review"])
        || runs(command, &["gh", "issue"], &["create", "comment"])
        || runs(command, &["glab", "mr"], &["create", "update", "note"])
        || runs(command, &["az", "repos", "pr"], &["create", "update"])
}

/// The first issue key with one of `prefixes` that the command holds as a whole word, or none: `WEB-12` yes, `XWEB-12`
/// and `web-12` no.
pub fn issue_key_in(command: &str, prefixes: &[String]) -> Option<String> {
    let bytes = command.as_bytes();
    (0..bytes.len()).filter(|at| *at == 0 || !bytes[at - 1].is_ascii_alphanumeric()).find_map(|at| {
        prefixes.iter().find_map(|prefix| {
            let digits = command[at..].strip_prefix(prefix.as_str())?.strip_prefix('-')?;
            let count = digits.bytes().take_while(u8::is_ascii_digit).count().min(9);
            let after = digits.as_bytes().get(count);
            (count > 0 && after.is_none_or(|b| !b.is_ascii_alphanumeric())).then(|| command[at..at + prefix.len() + 1 + count].to_owned())
        })
    })
}

/// The reason to deny a command, or none when it may run: it writes to history and holds a key of a tracked project.
pub fn local_only_denial(command: &str, prefixes: &[String]) -> Option<String> {
    if !writes_to_history(command) {
        return None;
    }
    issue_key_in(command, prefixes).map(|key| format!("{key} is a local Wings issue: remove the key from the message, then run it again."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn web() -> Vec<String> {
        vec!["WEB".into(), "API".into()]
    }

    #[test]
    fn a_key_in_a_commit_or_pull_request_command_is_denied_whichever_way_the_message_is_given() {
        let denied = |command: &str| local_only_denial(command, &web()).unwrap_or_default();
        assert!(denied(r#"git commit -m "WEB-12 fix totals""#).starts_with("WEB-12 is a local Wings issue"));
        assert!(denied("git add . && git commit -m \"$(cat <<'EOF'\nFix totals\n\nRefs API-3\nEOF\n)\"").starts_with("API-3 "));
        assert!(denied(r#"git -C /work/web commit -m "fix (WEB-1)""#).contains("WEB-1"));
        assert!(denied(r#"gh pr create --title "WEB-9 totals" --body x"#).contains("WEB-9"));
        assert!(denied(r#"glab mr note 4 -m "see WEB-2""#).contains("WEB-2"));
        assert!(denied(r#"git -c user.name=x --no-pager commit -m "WEB-4""#).contains("WEB-4"));
        assert!(denied(r#"git tag -a v1 -m "WEB-5 release""#).contains("WEB-5"));
        assert!(denied(r#"az repos pr update --id 3 --description "WEB-6""#).contains("WEB-6"));
    }

    #[test]
    fn text_that_only_looks_like_a_key_a_prefix_the_project_doesnt_use_and_commands_that_dont_write_are_allowed() {
        let allowed = |command: &str| local_only_denial(command, &web()).is_none();
        assert!(allowed(r#"git commit -m "Switch to UTF-8""#));
        assert!(allowed(r#"git commit -m "ABC-12 fix""#));
        assert!(allowed(r#"git commit -m "XWEB-12 and web-12""#));
        assert!(allowed("git log --grep WEB-12"));
        assert!(allowed("rg WEB-12 src"));
        assert!(allowed(r#"git tag v1 WEB-5"#));
        assert_eq!(issue_key_in("WEB-12", &[]), None);
        assert_eq!(issue_key_in("WEB-1234567890", &web()), None);
    }
}
