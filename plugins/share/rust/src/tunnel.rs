//! What OpenTunnel's CLI takes and what it prints (CLI 0.6.2, `crates/opentunnel-cli/src/main.rs` and `config.rs`), and a
//! call of it through `exec`. It has no JSON output, so the text is read.
//!
//! - `route add <port> [--name <name>]` prints `Added route <name> → 127.0.0.1:<port>`, `Route <name> → <target> already
//!   exists` or `Updated route <name> → <target> (was <old>)`, and the public link as its last line.
//! - `route list` prints `No routes configured.` or one `<public host>  →  <target>` line for each route.
//! - `route remove <name>` prints `Removed route <public host>`, and fails with `no route named '<name>' ...`.
//!
//! Only a bare port is ever passed: OpenTunnel turns it into `127.0.0.1:<port>`, so nothing but this computer's own
//! loopback is shared.

use serde_json::Value;
use wings_plugin::{cli::last_line, host};

pub const PROGRAM: &str = "opentunnel";
/// The Homebrew command that installs it, as the badge's Install runs it in a terminal tab.
pub const INSTALL: &str = "brew install anomalyco/tap/opentunnel";
/// How `wings.exec` rejects for a program that isn't installed.
const NOT_INSTALLED: &str = "is not installed or not on PATH";
/// A first `route add` creates the tunnel and waits for its certificate, which can take minutes. A tool call gets 120 s, so
/// this ends under it: the route is saved before that wait, so asking again carries on.
pub const ADD_MS: u32 = 110_000;
pub const QUICK_MS: u32 = 30_000;
const ARROW: char = '→';
const LOOPBACK: &str = "127.0.0.1";

/// What a call of `opentunnel` failed with.
#[derive(Clone, Debug, PartialEq)]
pub enum Fail {
    Missing,
    Failed(String),
}

impl Fail {
    /// What Claude, or the user, is told.
    pub fn message(&self) -> String {
        match self {
            Fail::Missing => format!(
                "OpenTunnel (the `{PROGRAM}` command) isn't installed on this Mac. Wings now shows the user an Install button that runs `{INSTALL}`. Once it is installed, try again."
            ),
            Fail::Failed(message) => message.clone(),
        }
    }
}

/// Runs `opentunnel` with `args` and gives what it printed, or why it failed.
pub async fn run(args: Vec<String>, timeout_ms: u32) -> Result<String, Fail> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = host::exec_timeout(PROGRAM, &args, timeout_ms).await.map_err(|message| if message.contains(NOT_INSTALLED) { Fail::Missing } else { Fail::Failed(message) })?;
    if out["code"] == 0 {
        return Ok(out["stdout"].as_str().unwrap_or_default().to_owned());
    }
    let (stderr, stdout) = (out["stderr"].as_str().unwrap_or_default(), out["stdout"].as_str().unwrap_or_default());
    let said = last_line(if stderr.trim().is_empty() { stdout } else { stderr });
    Err(Fail::Failed(if said.is_empty() { format!("{PROGRAM} failed") } else { said.to_owned() }))
}

/// The port a tool was given: a whole number from 1 to 65535.
pub fn port_of(value: &Value) -> Result<u16, String> {
    let whole = value.as_f64().filter(|n| n.fract() == 0.0 && (1.0..=65535.0).contains(n));
    whole.map(|n| n as u16).ok_or_else(|| "port must be a whole number from 1 to 65535.".to_owned())
}

/// A route name: one lowercase DNS label, which is what OpenTunnel takes (`@`, the tunnel's own address, is not one).
pub fn check_name(name: &str) -> Result<(), String> {
    let letters = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let bytes = name.as_bytes();
    let ok = (1..=63).contains(&bytes.len()) && bytes.first().is_some_and(letters) && bytes.last().is_some_and(letters) && bytes.iter().all(|b| letters(b) || *b == b'-');
    if ok { Ok(()) } else { Err(format!("name must be 1 to 63 lowercase letters, digits and dashes, not starting or ending with a dash, this one is {}.", Value::String(name.to_owned()))) }
}

/// The arguments of `route add` for a bare port, never a `host:port`.
pub fn add_args(port: u16, name: Option<&str>) -> Vec<String> {
    let mut args = vec!["route".to_owned(), "add".to_owned(), port.to_string()];
    if let Some(name) = name {
        args.extend(["--name".to_owned(), name.to_owned()]);
    }
    args
}

pub fn remove_args(name: &str) -> Vec<String> {
    ["route", "remove", name].map(str::to_owned).to_vec()
}

pub fn list_args() -> Vec<String> {
    ["route", "list"].map(str::to_owned).to_vec()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Added,
    /// The route existed with this target.
    Unchanged,
    Updated,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Added {
    pub name: String,
    pub target: String,
    pub change: Change,
    /// The link, `https://<host>`.
    pub url: String,
    pub host: String,
}

/// What `route add` printed.
pub fn parse_add(out: &str) -> Result<Added, String> {
    let unclear = || format!("OpenTunnel didn't say which route it made. It printed: {}", last_line(out));
    let line = out.lines().map(str::trim).find(|line| line.contains(ARROW)).ok_or_else(unclear)?;
    let (rest, change) = [("Added route ", Change::Added), ("Updated route ", Change::Updated), ("Route ", Change::Unchanged)]
        .into_iter()
        .find_map(|(prefix, change)| line.strip_prefix(prefix).map(|rest| (rest, change)))
        .ok_or_else(unclear)?;
    let (name, target) = rest.split_once(ARROW).map(|(name, target)| (name.trim(), target.split_whitespace().next().unwrap_or_default())).unwrap_or_default();
    let url = out.lines().map(str::trim).rfind(|line| line.starts_with("https://")).ok_or_else(|| format!("OpenTunnel printed no link. It printed: {}", last_line(out)))?;
    Ok(Added { name: name.to_owned(), target: target.to_owned(), change, url: url.to_owned(), host: url.trim_start_matches("https://").trim_end_matches('/').to_owned() })
}

/// One line of `route list`.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    /// What `route remove` takes: the first label of the public host (`@` for the tunnel's own address).
    pub name: String,
    /// The public host, which is just the name while the profile has no tunnel yet.
    pub public: String,
    pub target: String,
    /// The port, when the target is this computer's loopback.
    pub port: Option<u16>,
}

impl Route {
    pub fn url(&self) -> String {
        format!("https://{}", self.public)
    }
}

/// What `route list` printed: none for `No routes configured.`. A tunnel's host is `<id>.opentunnel.xyz` and a route's
/// `<name>.<id>.opentunnel.xyz`, so a route's name is its first label when there are four.
pub fn parse_list(out: &str) -> Vec<Route> {
    out.lines()
        .filter_map(|line| {
            let (public, target) = line.split_once(ARROW)?;
            let (public, target) = (public.trim(), target.split_whitespace().next()?);
            let name = match public.split('.').collect::<Vec<_>>().as_slice() {
                [only] => (*only).to_owned(),
                labels if labels.len() >= 4 => labels[0].to_owned(),
                _ => "@".to_owned(),
            };
            let port = target.strip_prefix(LOOPBACK).and_then(|rest| rest.strip_prefix(':')).and_then(|port| port.parse().ok());
            Some(Route { name, public: public.to_owned(), target: target.to_owned(), port })
        })
        .collect()
}

/// Whether `route remove` took a route out.
pub fn parse_remove(out: &str) -> bool {
    out.lines().any(|line| line.trim().starts_with("Removed route "))
}

/// Whether a failed `route remove` says the route was not there.
pub fn route_gone(message: &str) -> bool {
    message.contains("no route named")
}

/// The one line a tool says with a link: who can open it, and what a Vite dev server needs.
pub fn note(host: &str) -> String {
    format!(
        "Anyone with this link can open the app, including any endpoint meant only for localhost. A Vite dev server shows \"Blocked request\" until {host} is in its server.allowedHosts."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wings_plugin::task::block_on;

    #[test]
    fn a_port_is_a_whole_number_from_1_to_65535() {
        for good in [json!(1), json!(3000), json!(65535), json!(3000.0)] {
            assert!(port_of(&good).is_ok(), "{good}");
        }
        for bad in [json!(0), json!(65536), json!(-1), json!(3000.5), json!("3000"), json!("127.0.0.1:3000"), Value::Null] {
            assert!(port_of(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_name_is_one_lowercase_label() {
        for good in ["api", "my-app", "a", "3d", &"a".repeat(63)] {
            assert!(check_name(good).is_ok(), "{good}");
        }
        for bad in ["", "-api", "api-", "My-App", "my_app", "my.app", "my app", "@", "api/x", "api;ls", &"a".repeat(64)] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_arguments_hold_a_bare_port_and_an_optional_name_and_nothing_else() {
        assert_eq!(add_args(3000, None), ["route", "add", "3000"]);
        assert_eq!(add_args(3000, Some("api")), ["route", "add", "3000", "--name", "api"]);
        assert_eq!(remove_args("api"), ["route", "remove", "api"]);
        assert_eq!(list_args(), ["route", "list"]);
    }

    /// What opentunnel 0.6.2 printed for the first route on this machine (2026-11-24), word for word: the tunnel's
    /// making comes first. And its `route list` for it.
    #[test]
    fn the_first_routes_output_from_a_real_opentunnel_is_read() {
        let out = "Creating tunnel...\nGenerating private key...\nGenerating certificate request...\nRequesting certificate...\nWaiting for certificate verification (this can take a few minutes)...........\nSaving tunnel identity...\nTunnel is ready.\nAdded route 71b16883271f5c8d \u{2192} 127.0.0.1:47123\nhttps://71b16883271f5c8d.s4qkjei6qdxd.opentunnel.xyz\n";
        let added = parse_add(out).unwrap();
        assert_eq!((added.name.as_str(), added.url.as_str()), ("71b16883271f5c8d", "https://71b16883271f5c8d.s4qkjei6qdxd.opentunnel.xyz"));
        let routes = parse_list("71b16883271f5c8d.s4qkjei6qdxd.opentunnel.xyz  \u{2192}  127.0.0.1:47123\n");
        assert_eq!(routes.iter().map(|r| (r.name.as_str(), r.port)).collect::<Vec<_>>(), [("71b16883271f5c8d", Some(47123))]);
    }

    #[test]
    fn route_add_output_gives_the_name_target_change_and_link() {
        let added = parse_add("Added route 21992cc9713e5fc5 → 127.0.0.1:3000\nhttps://21992cc9713e5fc5.abc123.opentunnel.xyz\n").unwrap();
        assert_eq!(
            added,
            Added { name: "21992cc9713e5fc5".into(), target: "127.0.0.1:3000".into(), change: Change::Added, url: "https://21992cc9713e5fc5.abc123.opentunnel.xyz".into(), host: "21992cc9713e5fc5.abc123.opentunnel.xyz".into() }
        );
        let first_time = "Creating tunnel...\nGenerating private key...\nWaiting for certificate verification (this can take a few minutes)....\nTunnel is ready.\nAdded route api → 127.0.0.1:3000\nhttps://api.abc123.opentunnel.xyz\n";
        assert_eq!((parse_add(first_time).unwrap().name, parse_add(first_time).unwrap().change), ("api".to_owned(), Change::Added));
        let again = parse_add("Route api → 127.0.0.1:3000 already exists\nhttps://api.abc123.opentunnel.xyz\n").unwrap();
        assert_eq!((again.name.as_str(), again.target.as_str(), again.change), ("api", "127.0.0.1:3000", Change::Unchanged));
        let updated = parse_add("Updated route api → 127.0.0.1:4000 (was 127.0.0.1:3000)\nhttps://api.abc123.opentunnel.xyz\n").unwrap();
        assert_eq!((updated.target.as_str(), updated.change), ("127.0.0.1:4000", Change::Updated));
    }

    #[test]
    fn route_add_output_without_a_route_or_a_link_is_an_error_that_says_what_was_printed() {
        assert!(parse_add("https://x.abc.opentunnel.xyz\n").unwrap_err().contains("which route"));
        let error = parse_add("Added route api → 127.0.0.1:3000\n").unwrap_err();
        assert!(error.contains("no link") && error.contains("Added route api"), "{error}");
    }

    #[test]
    fn route_list_output_gives_each_route_its_name_link_and_loopback_port() {
        assert_eq!(parse_list("No routes configured.\n"), []);
        let routes = parse_list("21992cc9713e5fc5.abc123.opentunnel.xyz  →  127.0.0.1:3000\napi.abc123.opentunnel.xyz               →  127.0.0.1:5173 (proxy protocol v1)\nabc123.opentunnel.xyz                   →  10.0.0.5:80\ndb.abc123.opentunnel.xyz                →  localhost:5432\n");
        let seen: Vec<_> = routes.iter().map(|r| (r.name.as_str(), r.target.as_str(), r.port)).collect();
        assert_eq!(
            seen,
            [("21992cc9713e5fc5", "127.0.0.1:3000", Some(3000)), ("api", "127.0.0.1:5173", Some(5173)), ("@", "10.0.0.5:80", None), ("db", "localhost:5432", None)]
        );
        assert_eq!(routes[0].url(), "https://21992cc9713e5fc5.abc123.opentunnel.xyz");
        // Before the profile has a tunnel the list has only names.
        assert_eq!(parse_list("api  →  127.0.0.1:3000\n")[0].name, "api");
    }

    #[test]
    fn route_remove_output_and_its_failure_are_told_apart() {
        assert!(parse_remove("Removed route api.abc123.opentunnel.xyz\n"));
        assert!(!parse_remove(""));
        assert!(route_gone("no route named 'api' in profile default"));
        assert!(!route_gone("permission denied"));
    }

    fn answer_with(code: i64, stdout: &str, stderr: &str) {
        let (stdout, stderr) = (stdout.to_owned(), stderr.to_owned());
        host::testing::answer(move |_, _| Ok(json!({ "code": code, "stdout": stdout, "stderr": stderr })));
    }

    #[test]
    fn a_missing_opentunnel_is_told_apart_and_says_how_to_install_it() {
        host::testing::answer(|method, params| {
            assert_eq!((method, params["program"].as_str()), ("exec", Some("opentunnel")));
            Err("opentunnel is not installed or not on PATH".into())
        });
        let fail = block_on(run(list_args(), QUICK_MS)).unwrap_err();
        assert_eq!(fail, Fail::Missing);
        let said = fail.message();
        assert!(said.contains("brew install anomalyco/tap/opentunnel") && said.contains("isn't installed") && said.contains("Install"), "{said}");
    }

    #[test]
    fn a_failed_call_gives_the_last_line_it_printed_and_a_good_one_its_output() {
        answer_with(1, "", "warning: x\nerror: could not reach the relay\n");
        assert_eq!(block_on(run(list_args(), QUICK_MS)), Err(Fail::Failed("error: could not reach the relay".into())));
        answer_with(1, "", "");
        assert_eq!(block_on(run(list_args(), QUICK_MS)), Err(Fail::Failed("opentunnel failed".into())));
        answer_with(0, "No routes configured.\n", "");
        assert_eq!(block_on(run(list_args(), QUICK_MS)), Ok("No routes configured.\n".into()));
    }
}
