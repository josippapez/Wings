//! Each Claude account's usage limits, read the way Claude Code's `/usage` reads them: `GET /api/oauth/usage` on
//! Anthropic's API with the account's own claude.ai login. A status line's limits come from its session's last
//! response, so they lag behind; these are now, and `Statusline::record_usage` keeps them with the status line's.
//!
//! The endpoint isn't documented. What is asked, and how, is what Claude Code 2.1.323 does (read from its program):
//! the URL, the `anthropic-beta` header, the keychain item and file it keeps the login in, and the `user:profile` scope
//! it needs before it asks. The token is read when a fetch starts and dropped when it ends, sent only to
//! `api.anthropic.com`, and never refreshed, kept, logged or put in an error: one that has expired means no fetch, and
//! the status line's numbers stand until Claude Code refreshes it.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

use serde_json::Value;
use sha2::{Digest, Sha256};
use wings_spaces::claude_accounts;

use crate::statusline::Window;

const URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// What Claude Code sends with a claude.ai login (its `oauth-2025-04-20`).
const BETA: &str = "oauth-2025-04-20";
/// Claude Code waits 5 seconds for it.
const TIMEOUT: Duration = Duration::from_secs(5);
/// The answer is a few hundred bytes.
const BODY_MAX: u64 = 256 * 1024;

/// How often a followed account is asked while Wings is in front. Behind it nothing is asked: the status line still
/// reports, and network polling is for a focused app only (`.claude/skills/wings-performance`).
pub const EVERY: Duration = Duration::from_secs(5);
/// The longest wait after failures in a row.
const BACKOFF_MAX: Duration = Duration::from_secs(600);

/// A claude.ai access token. It has no `Display`, and `Debug` leaves it out.
pub struct Token(String);

impl Token {
    pub fn new(token: String) -> Self {
        Self(token)
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(..)")
    }
}

/// Why there is no token to ask with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoToken {
    /// Nobody is signed in to the account with claude.ai, or what is stored isn't a login.
    Missing,
    /// It has expired. Claude Code refreshes it on its next request; Wings doesn't.
    Expired,
    /// The login can't read usage: Claude Code only asks with the `user:profile` scope, which `claude setup-token`'s
    /// tokens don't have.
    Unscoped,
    /// The keychain didn't hand it over: you denied it, or it is locked.
    Refused,
}

/// Where an account's claude.ai login is.
pub trait Logins: Send + Sync {
    fn token(&self, account: Option<&str>) -> Result<Token, NoToken>;
}

/// Anthropic's usage endpoint.
pub trait Api: Send + Sync {
    /// The status and body of a `GET` of the usage endpoint with `token`, `None` when no answer came.
    fn usage(&self, token: &Token) -> Option<(u16, String)>;
}

/// What asking for an account's usage came to. Nothing in it can hold the token.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetched {
    /// The account's windows now. One the answer doesn't have is `None`.
    Limits { five_hour: Option<Window>, seven_day: Option<Window> },
    /// Nothing was asked.
    NoToken(NoToken),
    /// 401 or 403: the token was turned down.
    Rejected(u16),
    /// Try again later.
    Failed(Failure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// 429, a server error or another status.
    Status(u16),
    /// No answer.
    Network,
    /// A 2xx that isn't a usage answer.
    Unreadable,
}

/// Asks for `account`'s usage, `None` being the default account.
pub fn fetch(logins: &dyn Logins, api: &dyn Api, account: Option<&str>) -> Fetched {
    let token = match logins.token(account) {
        Ok(token) => token,
        Err(no) => return Fetched::NoToken(no),
    };
    match api.usage(&token) {
        None => Fetched::Failed(Failure::Network),
        Some((200..=299, body)) => match parse(&body) {
            Some((five_hour, seven_day)) => Fetched::Limits { five_hour, seven_day },
            None => Fetched::Failed(Failure::Unreadable),
        },
        Some((status @ (401 | 403), _)) => Fetched::Rejected(status),
        Some((status, _)) => Fetched::Failed(Failure::Status(status)),
    }
}

/// The 5-hour and weekly windows of a usage answer: `{"five_hour": {"utilization": 37.0, "resets_at":
/// "2026-11-13T18:00:00.410337+00:00"}, "seven_day": {...}, ...}`, where `utilization` is the percent used (Claude
/// Code's `/usage` prints `Math.floor(utilization)% used`). `None` for an answer that has neither, like an error.
pub fn parse(body: &str) -> Option<(Option<Window>, Option<Window>)> {
    let answer: Value = serde_json::from_str(body).ok()?;
    if answer.get("five_hour").is_none() && answer.get("seven_day").is_none() {
        return None;
    }
    // Claude Code skips a window whose `utilization` is null.
    let window = |w: &Value| {
        let used = w["utilization"].as_f64().filter(|n| n.is_finite())?;
        Some(Window { used_percentage: used.clamp(0.0, 100.0), resets_at: w["resets_at"].as_str().and_then(epoch) })
    };
    Some((window(&answer["five_hour"]), window(&answer["seven_day"])))
}

/// Unix seconds of an RFC 3339 time like `2026-11-13T18:00:00.410337+00:00`, to the nearest second.
fn epoch(time: &str) -> Option<i64> {
    let number = |part: Option<&str>| -> Option<i64> { part.filter(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))?.parse().ok() };
    let b = time.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (year, month, day) = (number(time.get(0..4))?, number(time.get(5..7))?, number(time.get(8..10))?);
    let (hour, minute, second) = (number(time.get(11..13))?, number(time.get(14..16))?, number(time.get(17..19))?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = time.get(19..)?;
    let mut fraction: f64 = 0.0;
    if let Some(after) = rest.strip_prefix('.') {
        let n = after.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 {
            return None;
        }
        fraction = format!("0.{}", &after[..n]).parse().ok()?;
        rest = &after[n..];
    }
    let offset = match rest.as_bytes() {
        b"Z" | b"z" => 0,
        [sign @ (b'+' | b'-'), _, _, b':', _, _] => {
            let offset = number(rest.get(1..3))? * 3600 + number(rest.get(4..6))? * 60;
            if *sign == b'-' {
                -offset
            } else {
                offset
            }
        }
        _ => return None,
    };
    // Days since 1970 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let days = era * 146_097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset + fraction.round() as i64)
}

/// The access token in what Claude Code stores for a login, `{"claudeAiOauth": {"accessToken": ..., "expiresAt": <ms>,
/// "scopes": [...]}}`, if it can still read usage at `now`, in ms.
fn token_in(stored: &str, now: u64) -> Result<Token, NoToken> {
    let stored: Value = serde_json::from_str(stored.trim()).map_err(|_| NoToken::Missing)?;
    let oauth = &stored["claudeAiOauth"];
    let token = oauth["accessToken"].as_str().filter(|token| !token.is_empty()).ok_or(NoToken::Missing)?;
    if !oauth["scopes"].as_array().is_some_and(|scopes| scopes.iter().any(|scope| scope == "user:profile")) {
        return Err(NoToken::Unscoped);
    }
    if oauth["expiresAt"].as_f64().is_some_and(|at| at <= now as f64) {
        return Err(NoToken::Expired);
    }
    Ok(Token(token.to_string()))
}

/// The keychain item Claude Code keeps a login in: `Claude Code-credentials`, and when it runs with `CLAUDE_CONFIG_DIR`
/// set, `-` and the first 8 hex digits of the SHA-256 of that value as it was given.
fn service(config_dir: Option<&str>) -> String {
    match config_dir {
        Some(dir) => format!("Claude Code-credentials-{}", Sha256::digest(dir.as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect::<String>()),
        None => "Claude Code-credentials".to_string(),
    }
}

/// The keychain account Claude Code files the item under: your user name, or `claude-code-user` when it has other
/// characters than these.
fn keychain_user() -> String {
    let plain = |name: &String| !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    std::env::var("USER").ok().filter(plain).unwrap_or_else(|| "claude-code-user".to_string())
}

/// The logins Claude Code keeps: on macOS in the login keychain, read with `security` as Claude Code reads them (so the
/// item's access list, which lets `security` read it, applies and macOS asks only if it doesn't), else in the account's
/// folder's `.credentials.json`, as on other systems.
pub struct Claude;

impl Logins for Claude {
    fn token(&self, account: Option<&str>) -> Result<Token, NoToken> {
        if account.is_some_and(|name| !claude_accounts::valid_name(name)) {
            return Err(NoToken::Missing);
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        if cfg!(target_os = "macos") {
            // Wings starts a named account's terminals with its folder as `CLAUDE_CONFIG_DIR`; the default account's
            // have Wings' own.
            let config_dir = match account {
                Some(name) => Some(claude_accounts::dir(Some(name)).to_string_lossy().into_owned()),
                None => std::env::var("CLAUDE_CONFIG_DIR").ok().filter(|dir| !dir.is_empty()),
            };
            let found = Command::new("/usr/bin/security")
                .args(["find-generic-password", "-a", &keychain_user(), "-w", "-s", &service(config_dir.as_deref())])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .map_err(|_| NoToken::Refused)?;
            match found.status.code() {
                Some(0) => return token_in(&String::from_utf8_lossy(&found.stdout), now),
                // No such item: Claude Code then reads the file.
                Some(44) => {}
                _ => return Err(NoToken::Refused),
            }
        }
        let file = std::fs::read_to_string(claude_accounts::dir(account).join(".credentials.json")).map_err(|_| NoToken::Missing)?;
        token_in(&file, now)
    }
}

/// `api.anthropic.com`, the only host the token goes to: HTTPS only, and no redirects.
pub struct Anthropic(ureq::Agent);

impl Anthropic {
    pub fn new() -> Self {
        Self(ureq::Agent::config_builder().https_only(true).max_redirects(0).http_status_as_error(false).timeout_global(Some(TIMEOUT)).build().into())
    }
}

impl Default for Anthropic {
    fn default() -> Self {
        Self::new()
    }
}

impl Api for Anthropic {
    fn usage(&self, token: &Token) -> Option<(u16, String)> {
        let mut answer = self
            .0
            .get(URL)
            .header("Authorization", &format!("Bearer {}", token.0))
            .header("anthropic-beta", BETA)
            .header("Content-Type", "application/json")
            .call()
            .ok()?;
        let status = answer.status().as_u16();
        let body = answer.body_mut().with_config().limit(BODY_MAX).read_to_string().ok()?;
        Some((status, body))
    }
}

/// A Claude account Wings follows: one with a session on screen or that reported to the status line.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    /// `None` for the default account.
    pub name: Option<String>,
    /// Who is signed in to it, when known.
    pub login: Option<String>,
    pub sessions: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct Turn {
    /// Not before then: `EVERY` after the last fetch started, longer after failures.
    next: Option<Instant>,
    failures: u32,
    asking: bool,
    /// The last turn found no token to ask with.
    tokenless: bool,
    /// The sessions at the last turn.
    sessions: BTreeSet<String>,
    /// After its token was turned down, the sessions there were then. Only a new session may bring a new login.
    stopped: Option<BTreeSet<String>>,
}

/// Whose turn it is to be asked.
#[derive(Debug, Default)]
pub struct Poller {
    turns: HashMap<Option<String>, Turn>,
}

fn backoff(failures: u32) -> Duration {
    EVERY.saturating_mul(1 << failures.min(16)).min(BACKOFF_MAX)
}

impl Poller {
    /// The accounts of `accounts` to ask now, each until `done`: nothing while Wings is behind other apps, one request
    /// at a time per account, and one account per login.
    pub fn due(&mut self, accounts: &[Account], focused: bool, now: Instant) -> Vec<Option<String>> {
        let mut logins = HashSet::new();
        let mut due = Vec::new();
        for account in accounts {
            let turn = self.turns.entry(account.name.clone()).or_default();
            if let Some(seen) = &turn.stopped {
                if account.sessions.is_subset(seen) {
                    continue;
                }
                *turn = Turn::default();
            }
            // Accounts signed in to one login have one set of limits, so one of them that has a token is asked. A
            // folder's config still names the login after it signed out.
            if !turn.tokenless && account.login.as_ref().is_some_and(|login| !logins.insert(login)) {
                continue;
            }
            turn.sessions = account.sessions.clone();
            if focused && !turn.asking && turn.next.is_none_or(|next| now >= next) {
                turn.asking = true;
                turn.next = Some(now + EVERY);
                due.push(account.name.clone());
            }
        }
        due
    }

    /// What asking for `account` came to, at `now`.
    pub fn done(&mut self, account: &Option<String>, fetched: &Fetched, now: Instant) {
        let Some(turn) = self.turns.get_mut(account) else { return };
        turn.asking = false;
        turn.tokenless = matches!(fetched, Fetched::NoToken(_));
        match fetched {
            Fetched::Limits { .. } => turn.failures = 0,
            Fetched::Rejected(_) | Fetched::NoToken(NoToken::Refused) => turn.stopped = Some(std::mem::take(&mut turn.sessions)),
            Fetched::NoToken(_) | Fetched::Failed(_) => {
                turn.failures += 1;
                turn.next = Some(now + backoff(turn.failures));
            }
        }
    }
}

/// The logins, the API and whose turn it is, for an app to ask on its own threads.
pub struct Usage {
    logins: Box<dyn Logins>,
    api: Box<dyn Api>,
    poller: Mutex<Poller>,
}

impl Usage {
    pub fn new(logins: impl Logins + 'static, api: impl Api + 'static) -> Self {
        Self { logins: Box::new(logins), api: Box::new(api), poller: Mutex::default() }
    }

    /// Claude Code's logins and Anthropic's API.
    pub fn claude() -> Self {
        Self::new(Claude, Anthropic::new())
    }

    pub fn due(&self, accounts: &[Account], focused: bool, now: Instant) -> Vec<Option<String>> {
        self.poller.lock().unwrap().due(accounts, focused, now)
    }

    /// Blocks for up to `TIMEOUT` and the keychain.
    pub fn fetch(&self, account: Option<&str>) -> Fetched {
        fetch(&*self.logins, &*self.api, account)
    }

    pub fn done(&self, account: &Option<String>, fetched: &Fetched, now: Instant) {
        self.poller.lock().unwrap().done(account, fetched, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands for a real token: it must turn up nowhere but in the request.
    const SENTINEL: &str = "wings-test-SENTINEL-4f1d2c";

    /// Reconstructed, not captured: the shape Claude Code 2.1.323 checks the answer against (`five_hour` and
    /// `seven_day` objects of `utilization`, a number or null, and `resets_at`, a string or null, beside other windows,
    /// `extra_usage` and `limits`), with a reset time to the microsecond as community clients show it.
    const ANSWER: &str = r#"{
        "five_hour": { "utilization": 37.0, "resets_at": "2026-11-13T18:00:00.410337+00:00" },
        "seven_day": { "utilization": 12.5, "resets_at": "2026-11-18T09:00:00.410358+00:00" },
        "seven_day_oauth_apps": null,
        "seven_day_opus": null,
        "seven_day_sonnet": { "utilization": 3.0, "resets_at": "2026-11-18T09:00:00.410373+00:00" },
        "extra_usage": { "is_enabled": false, "monthly_limit": null, "used_credits": null, "utilization": null }
    }"#;

    struct Login(Result<&'static str, NoToken>);

    impl Logins for Login {
        fn token(&self, _: Option<&str>) -> Result<Token, NoToken> {
            self.0.map(|token| Token(token.to_string()))
        }
    }

    struct Answer(Option<(u16, String)>);

    impl Api for Answer {
        fn usage(&self, token: &Token) -> Option<(u16, String)> {
            assert_eq!(token.0, SENTINEL);
            self.0.clone()
        }
    }

    struct Unasked;

    impl Api for Unasked {
        fn usage(&self, _: &Token) -> Option<(u16, String)> {
            panic!("asked without a token");
        }
    }

    fn window(used: f64, resets_at: i64) -> Option<Window> {
        Some(Window { used_percentage: used, resets_at: Some(resets_at) })
    }

    #[test]
    fn reads_the_5_hour_and_weekly_windows_of_an_answer() {
        assert_eq!(parse(ANSWER), Some((window(37.0, 1794592800), window(12.5, 1794992400))));
        // A null window, or one with a null `utilization`, is left out; a null reset time is unknown.
        let answer = r#"{ "five_hour": null, "seven_day": { "utilization": 140, "resets_at": null } }"#;
        assert_eq!(parse(answer), Some((None, Some(Window { used_percentage: 100.0, resets_at: None }))));
        assert_eq!(parse(r#"{ "five_hour": { "utilization": null, "resets_at": null } }"#), Some((None, None)));
        // An error in a 200, or no JSON, isn't an answer.
        for body in [r#"{ "error": { "type": "rate_limit_error" } }"#, "[]", "", "<html>"] {
            assert_eq!(parse(body), None, "{body}");
        }
    }

    #[test]
    fn reads_rfc_3339_times_to_the_nearest_second() {
        assert_eq!(epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch("2026-11-13T18:00:00.410337+00:00"), Some(1794592800));
        assert_eq!(epoch("2026-11-13T17:59:59.6Z"), Some(1794592800));
        assert_eq!(epoch("2026-11-13T13:00:00-05:00"), Some(1794592800));
        assert_eq!(epoch("2000-02-29T00:00:00Z"), Some(951782400));
        for bad in [
            "",
            "2026-11-13",
            "2026-11-13T18:00:00",
            "2026-13-01T00:00:00Z",
            "2026-11-13T18:00:00.Z",
            "2026-11-13T18:00:00+0000",
            "2026-11-13T18:00:0é+00:00",
            "+026-11-13T18:00:00Z",
        ] {
            assert_eq!(epoch(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_login_is_used_only_with_the_profile_scope_and_before_it_expires() {
        let stored = |expires: u64, scopes: &str| {
            format!(r#"{{"claudeAiOauth":{{"accessToken":"{SENTINEL}","refreshToken":"r","expiresAt":{expires},"scopes":{scopes},"subscriptionType":"max"}}}}"#)
        };
        let both = r#"["user:inference","user:profile","user:sessions:claude_code"]"#;
        assert_eq!(token_in(&format!("{}\n", stored(2000, both)), 1000).unwrap().0, SENTINEL);
        assert_eq!(token_in(&stored(1000, both), 1000).unwrap_err(), NoToken::Expired);
        assert_eq!(token_in(&stored(2000, r#"["user:inference"]"#), 1000).unwrap_err(), NoToken::Unscoped);
        for stored in ["", "{}", r#"{"claudeAiOauth":{"accessToken":""}}"#, r#"{"claudeAiOauth":null}"#] {
            assert_eq!(token_in(stored, 1000).unwrap_err(), NoToken::Missing, "{stored}");
        }
    }

    #[test]
    fn the_keychain_item_is_named_as_claude_code_names_it() {
        assert_eq!(service(None), "Claude Code-credentials");
        // `echo -n /Users/me/.claude-work | shasum -a 256`
        assert_eq!(service(Some("/Users/me/.claude-work")), "Claude Code-credentials-1e91dd84");
    }

    #[test]
    fn each_answer_comes_to_what_the_poller_acts_on() {
        let with = |answer: Option<(u16, String)>| fetch(&Login(Ok(SENTINEL)), &Answer(answer), None);
        assert_eq!(with(Some((200, ANSWER.into()))), Fetched::Limits { five_hour: window(37.0, 1794592800), seven_day: window(12.5, 1794992400) });
        assert_eq!(with(Some((401, String::new()))), Fetched::Rejected(401));
        assert_eq!(with(Some((403, String::new()))), Fetched::Rejected(403));
        assert_eq!(with(Some((429, String::new()))), Fetched::Failed(Failure::Status(429)));
        assert_eq!(with(Some((503, String::new()))), Fetched::Failed(Failure::Status(503)));
        assert_eq!(with(Some((302, String::new()))), Fetched::Failed(Failure::Status(302)));
        assert_eq!(with(Some((200, "{}".into()))), Fetched::Failed(Failure::Unreadable));
        assert_eq!(with(None), Fetched::Failed(Failure::Network));
        // Without a token nothing is asked.
        for no in [NoToken::Missing, NoToken::Expired, NoToken::Unscoped, NoToken::Refused] {
            assert_eq!(fetch(&Login(Err(no)), &Unasked, Some("work")), Fetched::NoToken(no));
        }
    }

    #[test]
    fn the_token_turns_up_in_no_error_or_result() {
        // An answer that quotes the token back, on every path.
        let quoted = format!(r#"{{"type":"error","error":{{"type":"authentication_error","message":"Bearer {SENTINEL} is not valid"}}}}"#);
        let mut shown = Vec::new();
        for status in [200, 201, 302, 400, 401, 403, 404, 429, 500, 503] {
            shown.push(format!("{:?}", fetch(&Login(Ok(SENTINEL)), &Answer(Some((status, quoted.clone()))), None)));
        }
        for answer in [None, Some((200, ANSWER.to_string())), Some((200, SENTINEL.to_string())), Some((200, format!(r#"{{"five_hour":"{SENTINEL}"}}"#)))] {
            shown.push(format!("{:?}", fetch(&Login(Ok(SENTINEL)), &Answer(answer), None)));
        }
        for no in [NoToken::Missing, NoToken::Expired, NoToken::Unscoped, NoToken::Refused] {
            shown.push(format!("{:?}", fetch(&Login(Err(no)), &Unasked, None)));
        }
        // What is stored, however it is broken.
        for stored in [
            format!(r#"{{"claudeAiOauth":{{"accessToken":"{SENTINEL}","expiresAt":1,"scopes":["user:profile"]}}}}"#),
            format!(r#"{{"claudeAiOauth":{{"accessToken":"{SENTINEL}","scopes":[]}}}}"#),
            format!(r#"{{"claudeAiOauth":{{"accessToken":"{SENTINEL}""#),
            format!(r#"{{"claudeAiOauth":{{"accessToken":"{SENTINEL}","scopes":["user:profile"]}}}}"#),
            SENTINEL.to_string(),
        ] {
            shown.push(format!("{:?}", token_in(&stored, 1000)));
        }
        assert!(shown.len() > 20);
        for shown in &shown {
            assert!(!shown.contains("SENTINEL"), "{shown}");
        }
    }

    fn account(name: Option<&str>, login: Option<&str>, sessions: &[&str]) -> Account {
        Account { name: name.map(str::to_string), login: login.map(str::to_string), sessions: sessions.iter().map(|s| s.to_string()).collect() }
    }

    fn limits() -> Fetched {
        Fetched::Limits { five_hour: window(10.0, 1794592800), seven_day: None }
    }

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn asks_every_5_seconds_while_wings_is_in_front_and_nothing_behind_it() {
        let accounts = [account(None, Some("me"), &["a"])];
        let (mut poller, start) = (Poller::default(), Instant::now());
        let mut asked = Vec::new();
        for ms in (0..60_000).step_by(100) {
            for account in poller.due(&accounts, true, at(start, ms)) {
                asked.push(ms / 1000);
                poller.done(&account, &limits(), at(start, ms + 400));
            }
        }
        assert_eq!(asked, (0..60).step_by(5).collect::<Vec<_>>(), "12 a minute");
        let behind: usize = (60_000..660_000).step_by(100).map(|ms| poller.due(&accounts, false, at(start, ms)).len()).sum();
        assert_eq!(behind, 0);
        // Back in front: at once.
        assert_eq!(poller.due(&accounts, true, at(start, 660_000)), [None]);
    }

    #[test]
    fn one_request_at_a_time_per_account_and_one_account_per_login() {
        let accounts = [
            account(None, Some("me"), &["a"]),
            account(Some("personal"), Some("me"), &["b"]),
            account(Some("work"), Some("them"), &["c"]),
            account(Some("other"), None, &["d"]),
        ];
        let (mut poller, start) = (Poller::default(), Instant::now());
        let names = |due: Vec<Option<String>>| due.into_iter().map(|a| a.unwrap_or_default()).collect::<Vec<_>>();
        assert_eq!(names(poller.due(&accounts, true, start)), ["", "work", "other"]);
        // Still waiting for the answers, long after.
        assert!(poller.due(&accounts, true, at(start, 60_000)).is_empty());
        poller.done(&Some("work".into()), &limits(), at(start, 60_000));
        assert_eq!(names(poller.due(&accounts, true, at(start, 60_000))), ["work"]);
        // The default account's folder has no token: the other account on the login is asked instead.
        poller.done(&None, &Fetched::NoToken(NoToken::Missing), at(start, 60_000));
        assert_eq!(names(poller.due(&accounts, true, at(start, 61_000))), ["personal"]);
    }

    #[test]
    fn a_turned_down_token_stops_the_account_until_a_new_session_reports() {
        for turned_down in [Fetched::Rejected(401), Fetched::Rejected(403), Fetched::NoToken(NoToken::Refused)] {
            let mut accounts = vec![account(None, Some("me"), &["a", "b"]), account(Some("personal"), Some("me"), &["c"])];
            let (mut poller, start) = (Poller::default(), Instant::now());
            assert_eq!(poller.due(&accounts, true, start), [None]);
            poller.done(&None, &turned_down, at(start, 300));
            // The other account on the login is asked instead, and this one not again, however long.
            let mut asked = Vec::new();
            for ms in (1_000..3_600_000).step_by(1_000) {
                for account in poller.due(&accounts, true, at(start, ms)) {
                    poller.done(&account, &limits(), at(start, ms));
                    asked.push(account);
                }
            }
            assert!(asked.iter().all(|a| a.as_deref() == Some("personal")), "{turned_down:?}");
            // A session it hadn't seen, which may have signed in again.
            accounts[0].sessions.insert("e".into());
            assert!(poller.due(&accounts, true, at(start, 3_600_000)).contains(&None), "{turned_down:?}");
        }
    }

    #[test]
    fn failures_wait_longer_each_time_and_never_loop() {
        let accounts = [account(Some("work"), None, &["a"])];
        for failure in [
            Fetched::Failed(Failure::Status(429)),
            Fetched::Failed(Failure::Status(503)),
            Fetched::Failed(Failure::Network),
            Fetched::NoToken(NoToken::Expired),
        ] {
            let (mut poller, start) = (Poller::default(), Instant::now());
            let mut asked = Vec::new();
            for ms in (0..3_600_000).step_by(100) {
                for account in poller.due(&accounts, true, at(start, ms)) {
                    asked.push(ms / 1000);
                    poller.done(&account, &failure, at(start, ms));
                }
            }
            assert_eq!(asked, [0, 10, 30, 70, 150, 310, 630, 1230, 1830, 2430, 3030], "{failure:?}: 10 minutes apart at most");
            // An answer brings it back to every 5 seconds.
            let now = at(start, 3_630_000);
            assert_eq!(poller.due(&accounts, true, now).len(), 1);
            poller.done(&Some("work".into()), &limits(), now);
            assert_eq!(poller.due(&accounts, true, at(start, 3_635_000)).len(), 1);
        }
    }
}
