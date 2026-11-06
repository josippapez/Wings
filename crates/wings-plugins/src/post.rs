//! The POSTs a plugin may send through a signed-in gh, glab or az: which paths, which body fields, which hosts.

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::{
    cli_rules::{graphql_reads_only, AZURE_DEVOPS_RESOURCE},
    exec::{run, ExecResult, EXEC_TIMEOUT},
    fetch::{keys_unambiguous, percent_decoded, pins_hold, post_segment_ok, split_pins, Pin, POST_BODY_LIMIT},
    manifest::Plugin,
};

/// `permissions.post` entry → (program, pattern, pinned body fields), if it's well formed.
pub(crate) fn post_entry(entry: &str) -> Option<(&str, &str, Vec<Pin<'_>>)> {
    let (entry, pins) = split_pins(entry)?;
    let (program, pattern) = entry.split_once(' ')?;
    // `?` and `=` for a fixed query like `?api-version=7.1`. A `*` only matches a whole plain segment.
    let chars_ok = !pattern.is_empty() && pattern.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~*/:%?=".contains(&b));
    let shape_ok = match program {
        // A query or a `.` segment would let `graphql?x` or `graphql/.` reach GraphQL past the check in `post_args`.
        "gh" | "glab" => !pattern.contains([':', '?', '=']) && pattern.split('/').all(|s| !s.is_empty() && s != "." && s != ".."),
        "az" => pattern.strip_prefix("https://").is_some_and(|rest| rest.split('/').next().is_some_and(|host| !host.is_empty() && !host.contains('*'))),
        _ => false,
    };
    (chars_ok && shape_ok).then_some((program, pattern, pins))
}

/// Whether a `permissions.post` entry covers a POST by `program` to `url`.
pub(crate) fn may_post(plugin: &Plugin, program: &str, url: &str, fields: &serde_json::Map<String, Value>) -> bool {
    let parts: Vec<&str> = url.split('/').collect();
    plugin.manifest.permissions.post.iter().filter_map(|e| post_entry(e)).any(|(p, pattern, pins)| {
        let want: Vec<&str> = pattern.split('/').collect();
        p == program
            && want.len() == parts.len()
            && want.iter().zip(&parts).all(|(w, part)| if *w == "*" { post_segment_ok(part) } else { w == part })
            && (pins.is_empty() || keys_unambiguous(fields.keys().map(String::as_str)))
            && pins_hold(&pins, fields)
    })
}

/// A POST a plugin asks for: its fields become the JSON body. `host` is the GitLab server for `glab`, and the
/// GitHub Enterprise Server for `gh` (absent or `github.com` is github.com).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostRequest {
    pub program: String,
    pub url: String,
    #[serde(default)]
    pub host: Option<String>,
    pub fields: serde_json::Map<String, Value>,
}

pub(crate) fn hostname_ok(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && !host.starts_with(['-', '.']) && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

/// The CLI arguments for a POST the manifest allows. Wings builds them itself, so the plugin never picks a flag.
pub(crate) fn post_args(plugin: &Plugin, request: &PostRequest) -> Result<Vec<String>, String> {
    let PostRequest { program, url, host, fields } = request;
    let id = &plugin.manifest.id;
    if !may_post(plugin, program, url, fields) {
        return Err(format!("{id} may not post to {program} {url}"));
    }
    // GitHub answered GraphQL at `graphql?x=1` too, and glab posts GraphQL as well, so the check goes by the decoded path:
    // a `*` can hold `%2F`, so `graphql%2F` ends in `graphql` once decoded, which a server that tidies paths may route there.
    let last = |path: String| path.split('/').rfind(|s| !s.is_empty()).is_some_and(|s| s.eq_ignore_ascii_case("graphql"));
    let graphql = percent_decoded(url).is_some_and(last);
    if matches!(program.as_str(), "gh" | "glab") && graphql && !graphql_reads_only(fields) {
        return Err(format!("{id} may only read with {program} graphql"));
    }
    let key_ok = |k: &str| !k.is_empty() && k.len() <= 40 && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if fields.is_empty() || fields.len() > 10 {
        return Err("a post needs 1 to 10 fields".into());
    }
    if let Some((key, _)) = fields.iter().find(|(k, v)| !key_ok(k) || !(v.is_string() || v.is_i64())) {
        return Err(format!("post field {key:?} must be a name of letters, digits and _ with a string or whole number value"));
    }
    let body = Value::Object(fields.clone()).to_string();
    if body.len() > POST_BODY_LIMIT {
        return Err("a post is limited to 64 KB".into());
    }
    let mut args: Vec<String> = vec!["api".into()];
    match program.as_str() {
        "gh" | "glab" => {
            if program == "glab" {
                let host = host.as_deref().filter(|h| hostname_ok(h)).ok_or("a glab post needs the GitLab host name")?;
                args.extend(["--hostname".into(), host.into()]);
            } else if let Some(host) = host.as_deref().filter(|h| *h != "github.com") {
                if !hostname_ok(host) {
                    return Err("a gh post's host must be a plain host name".into());
                }
                args.extend(["--hostname".into(), host.into()]);
            }
            args.extend(["--method".into(), "POST".into(), url.clone()]);
            // `-f` sends the text as it is: only `-F` reads `@file` or fills in placeholders, and a number can't start with either.
            for (key, value) in fields {
                match value.as_str() {
                    Some(text) => args.extend(["-f".into(), format!("{key}={text}")]),
                    None => args.extend(["-F".into(), format!("{key}={value}")]),
                }
            }
        }
        // A JSON object starts with `{`, so az never reads it as `@file`.
        "az" => {
            args = ["rest", "--method", "post", "--resource", AZURE_DEVOPS_RESOURCE, "--url", url, "--body", &body, "-o", "json"].map(String::from).to_vec()
        }
        _ => unreachable!("may_post only matches gh, glab and az"),
    }
    Ok(args)
}

/// The hosts `gh` is signed in to, from `gh auth status --json hosts`.
fn parse_gh_hosts(json: &str) -> Vec<String> {
    serde_json::from_str::<Value>(json).ok().and_then(|v| v.get("hosts")?.as_object().map(|h| h.keys().cloned().collect())).unwrap_or_default()
}

/// The hosts `glab` is set up for: each block of `glab auth status --all` starts with its host name, unindented.
fn parse_glab_hosts(text: &str) -> Vec<String> {
    text.lines().filter(|l| !l.starts_with(char::is_whitespace) && hostname_ok(l.trim())).map(|l| l.trim().to_string()).collect()
}

/// Looks the CLI's signed-in hosts up, and keeps the answer for a minute.
fn signed_in_hosts(program: &str) -> Result<Vec<String>, String> {
    static CACHE: std::sync::Mutex<Vec<(String, std::time::Instant, Vec<String>)>> = std::sync::Mutex::new(Vec::new());
    let mut cache = CACHE.lock().unwrap();
    if let Some((_, at, hosts)) = cache.iter().find(|(p, ..)| p == program) {
        if at.elapsed() < Duration::from_secs(60) {
            return Ok(hosts.clone());
        }
    }
    let args: Vec<String> =
        (if program == "gh" { &["auth", "status", "--json", "hosts"][..] } else { &["auth", "status", "--all"][..] }).iter().map(|a| a.to_string()).collect();
    let out = run(program, &args, None, EXEC_TIMEOUT, None)?;
    // glab writes its report to stderr when a token is bad, so read both.
    let hosts = if program == "gh" { parse_gh_hosts(&out.stdout) } else { parse_glab_hosts(&format!("{}\n{}", out.stdout, out.stderr)) };
    cache.retain(|(p, ..)| p != program);
    cache.push((program.to_string(), std::time::Instant::now(), hosts.clone()));
    Ok(hosts)
}

/// A CLI sends its token to whatever `--hostname` names (`gh` sends GH_ENTERPRISE_TOKEN to any host but github.com), so a
/// plugin may only name a host the CLI is already set up for. github.com is gh's own default.
pub(crate) fn host_is_signed_in(program: &str, host: Option<&str>, hosts: &dyn Fn(&str) -> Result<Vec<String>, String>) -> Result<(), String> {
    let Some(host) = host.filter(|h| !(program == "gh" && *h == "github.com")) else { return Ok(()) };
    if hosts(program)?.iter().any(|h| h.eq_ignore_ascii_case(host)) {
        Ok(())
    } else {
        Err(format!("{program} isn't signed in to {host}, so Wings won't send a request there."))
    }
}

/// Sends a POST the manifest allows, like a reply to a review comment, as you through the CLI's own sign-in.
pub fn post(plugin: &Plugin, request: &PostRequest) -> Result<ExecResult, String> {
    post_checked(plugin, request, &signed_in_hosts)
}

pub(crate) fn post_checked(plugin: &Plugin, request: &PostRequest, hosts: &dyn Fn(&str) -> Result<Vec<String>, String>) -> Result<ExecResult, String> {
    let args = post_args(plugin, request)?;
    host_is_signed_in(&request.program, request.host.as_deref(), hosts)?;
    run(&request.program, &args, None, EXEC_TIMEOUT, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli_rules::is_unsafe_flag,
        fetch::fetch_url,
        test_support::{plugin, post_request},
    };
    use std::path::Path;

    #[test]
    fn post_only_reaches_declared_paths() {
        let fields = serde_json::Map::new();
        let mut p = plugin(Path::new("/tmp"), &["gh api"], &[]);
        p.manifest.permissions.post = vec!["gh repos/*/*/pulls/*/comments/*/replies".into(), "glab projects/*/merge_requests/*/discussions/*/notes".into()];
        assert!(may_post(&p, "gh", "repos/o/r/pulls/1/comments/2/replies", &fields));
        for url in [
            "repos/o/r/pulls/1/comments/2/replies/x",
            "repos/o/r/pulls/1/comments/replies",
            "repos/o/r/pulls/1/comments/../replies",
            "repos/o/r/pulls/1/comments/%2e%2e/replies",
            "repos/o/r/pulls/1/comments/%252e/replies",
            "repos/o/r/pulls/1/comments/%5c/replies",
            "repos/o/r/pulls/1/comments/2?x=1/replies",
            "repos/o/r/pulls/1/comments/a%2F..%2Fb/replies",
        ] {
            assert!(!may_post(&p, "gh", url, &fields), "{url}");
        }
        assert!(!may_post(&p, "glab", "repos/o/r/pulls/1/comments/2/replies", &fields));
        assert!(may_post(&p, "glab", "projects/g%2Fp/merge_requests/1/discussions/ab12/notes", &fields));
        // An exec permission for `gh api` still can't write.
        assert!(is_unsafe_flag("gh", "--method"));
    }

    #[test]
    fn post_builds_the_command_itself() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.post =
            vec!["gh repos/*/*/pulls/*/comments/*/replies".into(), "glab projects/*/notes".into(), "az https://dev.azure.com/*/threads".into()];
        let args = post_args(&p, &post_request("gh", "repos/o/r/pulls/1/comments/2/replies", None)).unwrap();
        assert_eq!(args, ["api", "--method", "POST", "repos/o/r/pulls/1/comments/2/replies", "-f", "body=Thanks, fixed"]);
        let mut at_file = post_request("gh", "repos/o/r/pulls/1/comments/2/replies", None);
        at_file.fields.insert("body".into(), "@/etc/passwd".into());
        assert_eq!(post_args(&p, &at_file).unwrap()[5], "body=@/etc/passwd", "sent as text with -f");
        assert!(post_args(&p, &post_request("glab", "projects/1/notes", None)).is_err(), "glab needs a host");
        assert!(post_args(&p, &post_request("glab", "projects/1/notes", Some("-x"))).is_err());
        let az = post_args(&p, &post_request("az", "https://dev.azure.com/o/threads", None)).unwrap();
        assert_eq!(az[..7], ["rest", "--method", "post", "--resource", AZURE_DEVOPS_RESOURCE, "--url", "https://dev.azure.com/o/threads"]);
        assert_eq!(az[8], r#"{"body":"Thanks, fixed"}"#);
        let mut bad = post_request("gh", "repos/o/r/pulls/1/comments/2/replies", None);
        bad.fields.insert("--x".into(), "1".into());
        assert!(post_args(&p, &bad).is_err());
        bad.fields = serde_json::Map::new();
        assert!(post_args(&p, &bad).is_err());
    }

    #[test]
    fn a_post_entry_can_pin_body_fields() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.post = vec![
            "gh repos/*/*/pulls/*/reviews {event=APPROVE}".into(),
            "az https://dev.azure.com/*/reviewers/*/vote?api-version=7.1 {vote=10|5|0|-5|-10}".into(),
        ];
        let fields = |v: serde_json::Value| v.as_object().unwrap().clone();
        let url = "repos/o/r/pulls/1/reviews";
        assert!(may_post(&p, "gh", url, &fields(serde_json::json!({ "event": "APPROVE" }))));
        assert!(may_post(&p, "gh", url, &fields(serde_json::json!({ "event": "APPROVE", "body": "Looks good" }))));
        for bad in [
            serde_json::json!({ "event": "REQUEST_CHANGES" }),
            serde_json::json!({ "event": "approve" }),
            serde_json::json!({ "event": "APPROVE|COMMENT" }),
            serde_json::json!({ "body": "no event" }),
            serde_json::json!({ "event": 1 }),
        ] {
            assert!(!may_post(&p, "gh", url, &fields(bad.clone())), "{bad}");
        }
        let vote = "https://dev.azure.com/o/reviewers/me/vote?api-version=7.1";
        assert!(may_post(&p, "az", vote, &fields(serde_json::json!({ "vote": -5 }))));
        assert!(!may_post(&p, "az", vote, &fields(serde_json::json!({ "vote": 7 }))));
        // The pin is part of the entry, so an entry with a malformed pin is refused on load.
        for entry in
            ["gh repos/x {event=APPROVE", "gh repos/x {}", "gh repos/x {event}", "gh repos/x {event=}", "gh repos/x {ev-ent=A}", "gh repos/x {event=A B}"]
        {
            assert!(post_entry(entry).is_none(), "{entry}");
        }
        assert!(post_entry("gh repos/x {event=APPROVE}").is_some());
    }

    #[test]
    fn a_gh_post_names_an_enterprise_host() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.post = vec!["gh repos/*/*/pulls/*/comments/*/replies".into()];
        let url = "repos/o/r/pulls/1/comments/2/replies";
        let args = post_args(&p, &post_request("gh", url, Some("ghe.example.com"))).unwrap();
        assert_eq!(args, ["api", "--hostname", "ghe.example.com", "--method", "POST", url, "-f", "body=Thanks, fixed"]);
        assert_eq!(post_args(&p, &post_request("gh", url, Some("github.com"))).unwrap()[1], "--method");
        assert!(post_args(&p, &post_request("gh", url, Some("-x"))).is_err());
        assert!(post_args(&p, &post_request("gh", url, Some("a b"))).is_err());
    }

    #[test]
    fn a_post_rule_is_only_a_post_to_exactly_its_path() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.fetch = vec!["POST https://api.example.com/v1/repos/*/pulls/*/comments".into()];
        let ok = "https://api.example.com/v1/repos/o/pulls/3/comments";
        assert!(fetch_url(&p, "POST", ok, Some("{}")).is_some());
        for method in ["HEAD", "OPTIONS", "GET", "TRACE"] {
            assert!(fetch_url(&p, method, ok, Some("{}")).is_none(), "{method}");
        }
        assert!(fetch_url(&p, "POST", &format!("{ok}/"), Some("{}")).is_none(), "trailing slash");
        assert!(fetch_url(&p, "POST", "https://api.example.com/v1/repos/o%2Fx/pulls/3/comments", Some("{}")).is_none(), "%2F in a segment");
        assert!(fetch_url(&p, "POST", "https://api.example.com/v1/repos/o/pulls/3/comments/", Some("{}")).is_none());
    }

    #[test]
    fn a_cli_post_only_names_a_host_it_is_signed_in_to() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.post = vec!["gh repos/*/*/pulls/*/comments/*/replies".into(), "glab projects/*/notes".into()];
        let signed_in = |program: &str| -> Result<Vec<String>, String> {
            Ok(if program == "gh" { vec!["github.com".into(), "ghe.example.com".into()] } else { vec!["gitlab.example.com".into()] })
        };
        assert!(host_is_signed_in("gh", None, &signed_in).is_ok());
        assert!(host_is_signed_in("gh", Some("github.com"), &|_| Err("gh not looked up for its default host".into())).is_ok());
        assert!(host_is_signed_in("gh", Some("GHE.example.com"), &signed_in).is_ok());
        let refused = host_is_signed_in("gh", Some("attacker.example"), &signed_in).unwrap_err();
        assert!(refused.contains("isn't signed in to attacker.example"), "{refused}");
        assert!(host_is_signed_in("glab", Some("gitlab.example.com"), &signed_in).is_ok());
        assert!(host_is_signed_in("glab", Some("gitlab.com"), &signed_in).is_err());
        // post_checked stops before running anything.
        let url = "repos/o/r/pulls/1/comments/2/replies";
        let err = post_checked(&p, &post_request("gh", url, Some("attacker.example")), &signed_in).unwrap_err();
        assert!(err.contains("isn't signed in"), "{err}");
        assert!(post_checked(&p, &post_request("gh", url, Some("a b")), &signed_in).is_err());
        assert_eq!(parse_gh_hosts(r#"{"hosts":{"github.com":[{"state":"success"}],"ghe.example.com":[]}}"#).len(), 2);
        assert!(parse_gh_hosts("not json").is_empty());
        let text = "gitlab.com\n  x gitlab.com: API call failed\n  ! No token found\n\ngitlab.example.com\n  ✓ Logged in to gitlab.example.com\n\n   ERROR  \n\n  X could not authenticate";
        assert_eq!(parse_glab_hosts(text), ["gitlab.com", "gitlab.example.com"]);
    }

    #[test]
    fn graphql_only_reads_whichever_path_reaches_it() {
        let mutation = "mutation { addStar(input: {starrableId: \"x\"}) { clientMutationId } }";
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.post = vec!["gh *".into(), "glab graphql".into(), "gh repos/*/*/issues/*/comments".into()];
        let post = |program: &str, url: &str, query: &str| {
            let mut r = post_request(program, url, (program == "glab").then_some("gitlab.com"));
            r.fields = serde_json::json!({ "query": query }).as_object().unwrap().clone();
            post_args(&p, &r)
        };
        for (program, url) in [("glab", "graphql"), ("gh", "GraphQL"), ("gh", "graph%71l"), ("gh", "graphql%2F")] {
            assert!(post(program, url, mutation).is_err(), "{program} {url}");
            assert!(post(program, url, "query { viewer { login } }").is_ok(), "{program} {url}");
        }
        // A repo named graphql is a REST path like any other.
        assert!(post("gh", "repos/o/graphql/issues/1/comments", "Thanks").is_ok());
        // GitHub served GraphQL at `graphql?x=1`, so a gh or glab entry can't hold a query, `=` or a `.` segment.
        for entry in ["gh graphql?x=1", "gh graphql/.", "gh x/../graphql", "glab graphql/", "gh a//graphql", "gh x=1"] {
            assert!(post_entry(entry).is_none(), "{entry}");
        }
        assert!(post_entry("az https://dev.azure.com/*/threads?api-version=7.1").is_some());
    }

    #[test]
    fn post_entries_are_checked_on_load() {
        for entry in ["gh repos/*/replies", "glab projects/*/notes", "az https://dev.azure.com/*/comments"] {
            assert!(post_entry(entry).is_some(), "{entry}");
        }
        for entry in ["curl https://x/", "gh /repos/x", "gh https://api.github.com/x", "az dev.azure.com/x", "az https://*/x", "gh repos/x y", "gh "] {
            assert!(post_entry(entry).is_none(), "{entry}");
        }
    }
}
