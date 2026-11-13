//! Reading `git log`.

/// `git log -z` fields, each ended by a NUL: hash, parents, author, author time, committer time, refs, subject.
/// `--decorate=full` so a `log.decorate` setting can't change the ref names, and local, remote and tag refs tell apart.
pub const LOG: [&str; 4] = ["-z", "--no-color", "--decorate=full", "--format=%H%x00%P%x00%an%x00%at%x00%ct%x00%D%x00%s"];
const LOG_FIELDS: usize = 7;

/// `refs` are `kind:name`, where kind is `H` (HEAD, on that branch if it has a name), `b` local branch, `r` remote
/// branch, `t` tag or `s` stash.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub at: f64,
    pub ct: f64,
    pub refs: Vec<String>,
    pub subject: String,
}

/// `%D` with `--decorate=full`, as short `kind:name` refs. `origin/HEAD` and the like are left out.
fn parse_refs(decoration: &str) -> Vec<String> {
    let mut out = Vec::new();
    for mut name in decoration.split(", ").filter(|name| !name.is_empty()) {
        if name == "HEAD" {
            out.push("H:HEAD".to_owned());
            continue;
        }
        let head = name.starts_with("HEAD -> ");
        name = name.strip_prefix("HEAD -> ").unwrap_or(name);
        name = name.strip_prefix("tag: ").unwrap_or(name);
        if let Some(branch) = name.strip_prefix("refs/heads/") {
            out.push(format!("{}:{branch}", if head { "H" } else { "b" }));
        } else if let Some(remote) = name.strip_prefix("refs/remotes/").filter(|remote| !remote.ends_with("/HEAD")) {
            out.push(format!("r:{remote}"));
        } else if let Some(tag) = name.strip_prefix("refs/tags/") {
            out.push(format!("t:{tag}"));
        } else if name == "refs/stash" {
            out.push("s:stash".to_owned());
        }
    }
    out
}

/// How a `kind:name` ref shows as a pill: its label and its tone, if it has one.
pub fn describe_ref(name: &str) -> (String, Option<&'static str>) {
    let (kind, rest) = name.split_once(':').unwrap_or(("", name));
    let label = if kind == "t" { format!("◆ {rest}") } else { rest.to_owned() };
    let tone = match kind {
        "H" => Some("info"),
        "b" => Some("success"),
        "t" => Some("warn"),
        _ => None,
    };
    (label, tone)
}

/// `out` is `git log` output with `LOG`.
pub fn parse_log(out: &str) -> Vec<Commit> {
    let fields: Vec<&str> = out.split('\0').collect();
    let mut commits = Vec::new();
    for f in fields.as_chunks::<LOG_FIELDS>().0 {
        if f[0].is_empty() {
            break;
        }
        commits.push(Commit {
            hash: f[0].to_owned(),
            parents: f[1].split(' ').filter(|parent| !parent.is_empty()).map(str::to_owned).collect(),
            author: f[2].to_owned(),
            at: f[3].parse().unwrap_or(0.0),
            ct: f[4].parse().unwrap_or(0.0),
            refs: parse_refs(f[5]),
            subject: f[6].to_owned(),
        });
    }
    commits
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn log_of(commits: &[(&str, &str, &str, &str)]) -> String {
        commits.iter().map(|(hash, parents, refs, subject)| format!("{hash}\0{parents}\0Ana\0100\0200\0{refs}\0{subject}\0")).collect()
    }

    #[test]
    fn a_log_reads_as_commits_with_their_parents_and_refs() {
        let commits = parse_log(&log_of(&[
            ("a1", "b2 c3", "HEAD -> refs/heads/main, tag: refs/tags/v1, refs/remotes/origin/main, refs/remotes/origin/HEAD, refs/stash", "Merge it"),
            ("b2", "", "HEAD", "First"),
        ]));
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].parents, ["b2", "c3"]);
        assert_eq!(commits[0].refs, ["H:main", "t:v1", "r:origin/main", "s:stash"]);
        assert_eq!((commits[0].at, commits[0].ct, commits[0].subject.as_str()), (100.0, 200.0, "Merge it"));
        assert_eq!((commits[1].parents.len(), commits[1].refs.as_slice()), (0, ["H:HEAD".to_owned()].as_slice()));
        assert!(parse_log("").is_empty());
        assert_eq!(describe_ref("t:v1"), ("◆ v1".to_owned(), Some("warn")));
        assert_eq!(describe_ref("r:origin/main"), ("origin/main".to_owned(), None));
    }
}
