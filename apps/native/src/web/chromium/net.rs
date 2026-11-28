//! What a Chromium page asked of the network, kept as its DevTools channel told it (`Network` domain, on while the page
//! lives), and its console: the last `KEPT` requests with their headers (every credential scrubbed, `redact`, before
//! it is stored), and the console's lines and uncaught exceptions with where they came from.
//!
//! The network events are the busy ones, so the channel's observer hands their bytes to a thread of this module's own
//! (`feed`) and the main thread never parses them; a page is told of none of them. What an agent reads goes through
//! `NetLog::list` and `detail`, which leave out requests of a site signed in with imported cookies.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, OnceLock, mpsc},
};

use serde_json::{Map, Value, json};

use crate::page::tree::WebNetwork;

use self::redact::Redactor;

/// The most requests kept for a page.
pub const KEPT: usize = 500;
/// The largest response body given to an agent, in bytes.
pub const BODY_MAX: usize = 1024 * 1024;
/// What `Network.enable` may keep of bodies for `Network.getResponseBody`: none larger than `BODY_MAX` and 20 MB in all.
pub const BUFFER_TOTAL: u64 = 20 * 1024 * 1024;
pub const BUFFER_ONE: u64 = BODY_MAX as u64 + 1024;
/// The most characters of a console line or a stack.
const TEXT_MAX: usize = 4000;

/// The events `feed` takes.
pub const EVENTS: [&str; 6] = [
    "Network.requestWillBeSent",
    "Network.requestWillBeSentExtraInfo",
    "Network.responseReceived",
    "Network.responseReceivedExtraInfo",
    "Network.loadingFinished",
    "Network.loadingFailed",
];

/// One request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    pub id: String,
    pub method: String,
    pub url: String,
    /// The address of the document that made it.
    pub document: String,
    /// The protocol's resource type: `Document`, `XHR`, `Fetch`, `Image`...
    pub kind: String,
    pub status: Option<i64>,
    pub status_text: String,
    pub mime: String,
    /// What came over the wire, in bytes.
    pub encoded: Option<f64>,
    /// The request body's size, when the protocol gave it.
    pub sent: Option<usize>,
    /// Milliseconds since 1970 when it began.
    pub started: f64,
    /// The protocol's own clock, in seconds, when it began.
    clock: f64,
    pub duration: Option<f64>,
    pub failure: Option<String>,
    pub initiator: String,
    pub request_headers: Map<String, Value>,
    pub response_headers: Map<String, Value>,
    pub finished: bool,
    pub from_cache: bool,
}

/// The host of an address, empty for one with none.
pub fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or("", |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    host.rsplit_once('@').map_or(host, |(_, host)| host).split(':').next().unwrap_or_default()
}

impl Request {
    /// As an agent hears it: every value the page's credentials were, remembered since it was stored, replaced too.
    fn summary(&self, red: &Redactor) -> Value {
        json!({
            "id": self.id, "method": self.method, "url": red.text(&self.url), "type": self.kind, "status": self.status, "statusText": self.status_text,
            "mime": self.mime, "encodedBytes": self.encoded, "requestBytes": self.sent, "startedAt": self.started, "durationMs": self.duration,
            "failure": self.failure.as_deref().map(|why| red.text(why)), "initiator": red.text(&self.initiator), "pending": !self.finished, "fromCache": self.from_cache,
        })
    }

    /// Whether `filter` picks it. The address is matched as an agent hears it, redacted, so no filter probes a secret.
    fn matches(&self, filter: &WebNetwork, red: &Redactor) -> bool {
        let kind = filter.kind.as_deref().is_none_or(|kind| self.kind.eq_ignore_ascii_case(kind));
        let url = filter.url.as_deref().is_none_or(|part| red.text(&self.url).contains(part));
        let since = filter.since.is_none_or(|since| self.started > since);
        kind && url && since && filter.status.as_deref().is_none_or(|status| self.status_is(status))
    }

    /// Whether its outcome is `wanted`: `2xx`, `404`, `failed`.
    fn status_is(&self, wanted: &str) -> bool {
        let wanted = wanted.to_ascii_lowercase();
        if wanted == "failed" {
            return self.failure.is_some();
        }
        let Some(status) = self.status else { return false };
        match wanted.strip_suffix("xx") {
            Some(class) => class.parse::<i64>().is_ok_and(|class| status / 100 == class),
            None => wanted.parse::<i64>().is_ok_and(|exact| status == exact),
        }
    }
}

/// A page's last requests, the newest last.
#[derive(Default)]
pub struct NetLog {
    requests: VecDeque<Request>,
    red: Redactor,
    /// Requests that redirected, which count for the ids of their hops.
    hops: u64,
}

/// A page's log, which `feed`'s thread and the main thread share.
pub type Log = Arc<Mutex<NetLog>>;

impl NetLog {
    fn find(&mut self, id: &str) -> Option<&mut Request> {
        self.requests.iter_mut().rev().find(|request| request.id == id)
    }

    /// One event of the `Network` domain.
    pub fn ingest(&mut self, method: &str, params: &Value) {
        let Some(id) = params["requestId"].as_str() else { return };
        self.ingest_one(method, id, params);
        self.red.refresh();
    }

    fn ingest_one(&mut self, method: &str, id: &str, params: &Value) {
        match method {
            "Network.requestWillBeSent" => {
                if let Some(redirect) = params.get("redirectResponse").filter(|response| response.is_object()) {
                    self.hops += 1;
                    let hop = self.hops;
                    let done = params["timestamp"].as_f64();
                    let headers = self.red.headers(&redirect["headers"]);
                    if let Some(old) = self.find(id) {
                        old.apply_response(redirect, headers);
                        old.finish(done);
                        old.id = format!("{id}~{hop}");
                    }
                }
                let request = &params["request"];
                let initiator = &params["initiator"];
                let at = initiator["stack"]["callFrames"].get(0).filter(|frame| frame["url"].as_str().is_some_and(|url| !url.is_empty())).or_else(|| initiator.get("url").map(|_| initiator));
                let place = at.and_then(|at| Some(format!("{}:{}", self.red.url(at["url"].as_str()?), at["lineNumber"].as_i64().map_or(0, |line| line + 1))));
                let url = request["url"].as_str().map(|url| format!("{url}{}", request["urlFragment"].as_str().unwrap_or_default())).unwrap_or_default();
                let (url, document) = (self.red.url(&url), self.red.url(params["documentURL"].as_str().unwrap_or_default()));
                let request_headers = self.red.headers(&request["headers"]);
                self.requests.push_back(Request {
                    id: id.to_owned(),
                    method: request["method"].as_str().unwrap_or("GET").to_owned(),
                    url,
                    document,
                    kind: params["type"].as_str().unwrap_or("Other").to_owned(),
                    sent: request["postData"].as_str().map(str::len),
                    started: params["wallTime"].as_f64().unwrap_or(0.0) * 1000.0,
                    clock: params["timestamp"].as_f64().unwrap_or(0.0),
                    initiator: match (initiator["type"].as_str().unwrap_or("other"), place) {
                        (kind, Some(place)) => format!("{kind} {place}"),
                        (kind, None) => kind.to_owned(),
                    },
                    request_headers,
                    ..Default::default()
                });
                while self.requests.len() > KEPT {
                    self.requests.pop_front();
                }
            }
            // The headers the network stack sent, cookies and credentials among them: kept over what the page's own were.
            "Network.requestWillBeSentExtraInfo" => {
                let headers = self.red.headers(&params["headers"]);
                if let Some(request) = self.find(id) {
                    request.request_headers.extend(headers);
                }
            }
            "Network.responseReceived" => {
                let headers = self.red.headers(&params["response"]["headers"]);
                if let Some(request) = self.find(id) {
                    request.apply_response(&params["response"], headers);
                    if let Some(kind) = params["type"].as_str() {
                        kind.clone_into(&mut request.kind);
                    }
                }
            }
            "Network.responseReceivedExtraInfo" => {
                let headers = self.red.headers(&params["headers"]);
                if let Some(request) = self.find(id) {
                    request.response_headers.extend(headers);
                }
            }
            "Network.loadingFinished" => {
                if let Some(request) = self.find(id) {
                    request.encoded = params["encodedDataLength"].as_f64();
                    request.finish(params["timestamp"].as_f64());
                }
            }
            "Network.loadingFailed" => {
                if let Some(request) = self.find(id) {
                    let why = params["errorText"].as_str().unwrap_or("failed");
                    request.failure = Some(match params["blockedReason"].as_str() {
                        Some(blocked) => format!("{why} (blocked: {blocked})"),
                        None if params["canceled"] == true => format!("{why} (canceled)"),
                        None => why.to_owned(),
                    });
                    request.finish(params["timestamp"].as_f64());
                }
            }
            _ => {}
        }
    }

    /// The requests that match `filter`, the newest `filter.limit` of them, oldest first, leaving out those of a site
    /// whose host `hide` says to.
    pub fn list(&self, filter: &WebNetwork, hide: &dyn Fn(&str) -> bool, now: f64) -> Value {
        let shown: Vec<&Request> = self.requests.iter().filter(|request| request.matches(filter, &self.red) && !request.hidden(hide)).collect();
        let total = shown.len();
        let from = total.saturating_sub(filter.limit);
        json!({ "requests": shown[from..].iter().map(|request| request.summary(&self.red)).collect::<Vec<_>>(), "matching": total, "kept": self.requests.len(), "now": now })
    }

    /// One request, with its headers, or none that is of a site `hide` says to leave out.
    pub fn detail(&self, id: &str, hide: &dyn Fn(&str) -> bool) -> Option<(Request, Value)> {
        let request = self.requests.iter().rev().find(|request| request.id == id && !request.hidden(hide))?;
        let mut shown = request.summary(&self.red);
        let known = |headers: &Map<String, Value>| Value::Object(headers.iter().map(|(name, value)| (name.clone(), json!(self.red.text(value.as_str().unwrap_or_default())))).collect());
        shown["requestHeaders"] = known(&request.request_headers);
        shown["responseHeaders"] = known(&request.response_headers);
        shown["documentUrl"] = json!(self.red.text(&request.document));
        Some((request.clone(), shown))
    }

    /// A response body as an agent hears it: credential fields scrubbed (`Redactor::body`), by the type the response said.
    pub fn protect_body(&mut self, body: &str, mime: &str) -> String {
        self.red.body(body, mime)
    }

    /// A console line with every value remembered for the page replaced in its text, place and stack.
    pub fn console(&self, mut line: Value) -> Value {
        for key in ["text", "source", "stack"] {
            if let Some(text) = line[key].as_str() {
                line[key] = json!(self.red.text(text));
            }
        }
        line
    }

    /// What the legacy `logs` answer holds of the network: method, address, status and failure.
    pub fn legacy(&self, hide: &dyn Fn(&str) -> bool, most: usize) -> Vec<Value> {
        let shown: Vec<&Request> = self.requests.iter().filter(|request| !request.hidden(hide)).collect();
        shown[shown.len().saturating_sub(most)..]
            .iter()
            .map(|request| json!({ "kind": "network", "method": request.method, "url": self.red.text(&request.url), "status": request.status, "error": request.failure.as_deref().map(|why| self.red.text(why)) }))
            .collect()
    }
}

impl Request {
    fn apply_response(&mut self, response: &Value, headers: Map<String, Value>) {
        self.status = response["status"].as_f64().map(|status| status as i64);
        self.status_text = response["statusText"].as_str().unwrap_or_default().to_owned();
        self.mime = response["mimeType"].as_str().unwrap_or_default().to_owned();
        self.response_headers.extend(headers);
        self.from_cache = response["fromDiskCache"] == true || response["fromPrefetchCache"] == true || response["fromServiceWorker"] == true;
    }

    fn finish(&mut self, clock: Option<f64>) {
        self.finished = true;
        self.duration = clock.filter(|clock| *clock >= self.clock && self.clock > 0.0).map(|clock| ((clock - self.clock) * 1000.0 * 10.0).round() / 10.0);
    }

    /// Whether the request, or the document that made it, is of a host `hide` says to leave out. A request with no host (a
    /// `data:` address) is left in.
    fn hidden(&self, hide: &dyn Fn(&str) -> bool) -> bool {
        [host_of(&self.url), host_of(&self.document)].into_iter().any(|host| !host.is_empty() && hide(host))
    }
}

/// Why a response body isn't given, or none when it may be: it has to be done, not failed, text, and not over `BODY_MAX`.
pub fn body_refusal(request: &Request) -> Option<&'static str> {
    if request.id.contains('~') {
        return Some("A redirect has no body");
    }
    if request.failure.is_some() {
        return Some("The request failed, so it has no body");
    }
    if !request.finished {
        return Some("The request hasn't finished yet: ask again in a moment");
    }
    if !text_mime(&request.mime) {
        return Some("Only a text body (text, JSON, XML, JavaScript, a form) is given, and this one isn't");
    }
    if request.encoded.is_some_and(|bytes| bytes > BODY_MAX as f64 * 4.0) {
        return Some("The body is larger than 1 MB");
    }
    None
}

fn text_mime(mime: &str) -> bool {
    let mime = mime.to_ascii_lowercase();
    mime.starts_with("text/") || ["json", "xml", "javascript", "ecmascript", "x-www-form-urlencoded", "graphql"].iter().any(|kind| mime.contains(kind))
}

/// What an agent reads when Chromium no longer holds a body, in place of its words for its own cache.
pub fn body_lost(why: String) -> String {
    if why.contains("No resource with given identifier") || why.contains("evicted from inspector cache") {
        return "The body isn't kept any more: the page that asked for it moved on, or it was larger than 1 MB".into();
    }
    why
}

/// The text of `Network.getResponseBody`'s answer: not base64 (a binary body) and at most `BODY_MAX` bytes.
pub fn body_text(answer: &Value) -> Result<String, &'static str> {
    if answer["base64Encoded"] == true {
        return Err("The body is binary, so it isn't given");
    }
    let body = answer["body"].as_str().ok_or("The body isn't kept any more")?;
    if body.len() > BODY_MAX {
        return Err("The body is larger than 1 MB");
    }
    Ok(body.to_owned())
}

/// Hands one `Network` event's bytes to the thread that reads them, off the main thread.
pub fn feed(log: &Log, message: &[u8]) {
    static WORKER: OnceLock<mpsc::Sender<(Log, Vec<u8>)>> = OnceLock::new();
    let worker = WORKER.get_or_init(|| {
        let (send, receive) = mpsc::channel::<(Log, Vec<u8>)>();
        std::thread::Builder::new()
            .name("wings-net-log".into())
            .spawn(move || {
                for (log, message) in receive {
                    let Ok(mut event) = serde_json::from_slice::<Value>(&message) else { continue };
                    let method = event["method"].as_str().unwrap_or_default().to_owned();
                    if let Ok(mut log) = log.lock() {
                        log.ingest(&method, &event["params"].take());
                    }
                }
            })
            .expect("a thread starts");
        send
    });
    drop(worker.send((log.clone(), message.to_vec())));
}

fn clip(text: &str) -> String {
    match text.char_indices().nth(TEXT_MAX) {
        Some((at, _)) => format!("{}... (cut)", &text[..at]),
        None => text.to_owned(),
    }
}

/// `where` as `url:line:column`, lines and columns from 1, for a call frame or exception details (zero-based in the protocol).
fn place(url: &str, line: Option<i64>, column: Option<i64>) -> Option<String> {
    (!url.is_empty()).then(|| format!("{url}:{}:{}", line.unwrap_or(0) + 1, column.unwrap_or(0) + 1))
}

fn frames(stack: &Value) -> String {
    stack["callFrames"]
        .as_array()
        .into_iter()
        .flatten()
        .take(30)
        .map(|frame| {
            let name = frame["functionName"].as_str().filter(|name| !name.is_empty()).unwrap_or("(anonymous)");
            format!("at {name} ({})", place(frame["url"].as_str().unwrap_or_default(), frame["lineNumber"].as_i64(), frame["columnNumber"].as_i64()).unwrap_or_default())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One `Runtime.consoleAPICalled` as a console line: its level (`error`, `warning`, `info`, `debug`, `log`), what it
/// printed and where.
pub fn console_line(params: &Value) -> Value {
    let level = match params["type"].as_str().unwrap_or("log") {
        "assert" | "error" => "error",
        "warning" => "warning",
        "info" => "info",
        "debug" => "debug",
        _ => "log",
    };
    let text = params["args"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|arg| match &arg["value"] {
            Value::String(text) => text.clone(),
            Value::Null => arg["description"].as_str().or(arg["unserializableValue"].as_str()).unwrap_or("null").to_owned(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    let frame = &params["stackTrace"]["callFrames"][0];
    let source = place(frame["url"].as_str().unwrap_or_default(), frame["lineNumber"].as_i64(), frame["columnNumber"].as_i64());
    json!({ "kind": "console", "level": level, "text": clip(&text), "source": source, "at": params["timestamp"] })
}

/// One `Runtime.exceptionThrown`: an uncaught exception with its message, where it was thrown and its stack.
pub fn exception_line(params: &Value) -> Value {
    let details = &params["exceptionDetails"];
    let described = details["exception"]["description"].as_str().map(|text| text.lines().next().unwrap_or_default());
    let text = match described {
        Some(first) if !first.is_empty() => format!("Uncaught {first}"),
        _ => details["text"].as_str().unwrap_or("Uncaught").to_owned(),
    };
    let stack = frames(&details["stackTrace"]);
    json!({
        "kind": "exception", "level": "error", "text": clip(&text), "at": params["timestamp"],
        "source": place(details["url"].as_str().unwrap_or_default(), details["lineNumber"].as_i64(), details["columnNumber"].as_i64()),
        "stack": if stack.is_empty() { Value::Null } else { json!(clip(&stack)) },
    })
}

/// Scrubbing of credentials, by the rules of browse's `network-capture/redaction.ts`: a header, URL field or body value
/// that is a credential by its name or its look is replaced by `[redacted]` as it is stored, and remembered (in memory,
/// the last `KNOWN_MAX`) so it is replaced where it shows up later under a name that doesn't look secret.
pub mod redact {
    use std::collections::{HashSet, VecDeque};

    use serde_json::{Map, Value};

    pub const PLACEHOLDER: &str = "[redacted]";
    /// The most values remembered for a page: the oldest are forgotten first.
    pub const KNOWN_MAX: usize = 2000;
    /// A remembered value shorter than this is replaced only when it is the whole text.
    const SUBSTRING_MIN: usize = 8;

    const SECRET_HEADERS: [&str; 7] = ["authorization", "proxy-authorization", "x-api-key", "api-key", "x-csrf-token", "csrf-token", "x-xsrf-token"];
    /// HTTP/2 pseudo-headers (but `:path`, which is a URL) and the CORS headers: routing and policy, never credentials.
    const PLAIN_HEADERS: [&str; 15] = [
        ":authority", ":method", ":scheme", ":status", ":protocol", "access-control-allow-credentials", "access-control-allow-origin",
        "access-control-allow-methods", "access-control-allow-headers", "access-control-expose-headers", "access-control-max-age",
        "access-control-request-method", "access-control-request-headers", "access-control-allow-private-network", "access-control-request-private-network",
    ];
    const COOKIE_ATTRIBUTES: [&str; 9] = ["domain", "path", "expires", "max-age", "samesite", "priority", "partitioned", "secure", "httponly"];

    fn separator(c: char) -> bool {
        matches!(c, '-' | '_' | '.')
    }

    /// Whether `word` is in `name` as a whole segment: at its start or after `-`, `_` or `.`, and at its end or before one.
    fn has_segment(name: &str, word: &str) -> bool {
        name.match_indices(word).any(|(at, _)| {
            let before = name[..at].chars().next_back().is_none_or(separator);
            let after = name[at + word.len()..].chars().next().is_none_or(separator);
            before && after
        })
    }

    /// A name that says what it holds is a credential: auth, cookie, token, session, password, a key, a signature...
    pub fn secret_name(name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        const WORDS: [&str; 11] = ["auth", "cookie", "credential", "csrf", "xsrf", "token", "secret", "session", "password", "passwd", "pwd"];
        const MORE: [&str; 3] = ["passcode", "otp", "signature"];
        if WORDS.iter().chain(&MORE).any(|word| name.contains(word)) || has_segment(&name, "code") || has_segment(&name, "sig") {
            return true;
        }
        ["api", "access", "refresh"].iter().any(|prefix| {
            name.match_indices(prefix).any(|(at, _)| {
                let rest = &name[at + prefix.len()..];
                rest.strip_prefix(|c| separator(c)).unwrap_or(rest).starts_with("key")
            })
        })
    }

    /// A field a person types a password or a code into: replaced at any length, whatever it is in a later text.
    fn one_time_name(name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        ["password", "passwd", "passcode", "passphrase", "otp"].iter().any(|word| name.contains(word))
            || ["pwd", "code", "mfa_code", "mfacode", "2fa_code", "2facode", "verification_code", "verificationcode", "auth_code", "authcode", "pin"].iter().any(|word| has_segment(&name, word))
    }

    fn is_uuid(text: &str) -> bool {
        let groups: Vec<&str> = text.split('-').collect();
        groups.len() == 5 && groups.iter().zip([8, 4, 4, 4, 12]).all(|(group, len)| group.len() == len && group.bytes().all(|b| b.is_ascii_hexdigit()))
    }

    /// A path segment that looks like a credential: a JWT, 32 or more hex digits, or a long base64-looking string that is
    /// no UUID and no slug of words.
    pub fn looks_like_token(segment: &str) -> bool {
        let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
        if segment.len() < 24 || is_uuid(segment) || segment.contains("${") {
            return false;
        }
        let jwt: Vec<&str> = segment.split('.').collect();
        if jwt.len() == 3 && jwt[0].starts_with("eyJ") && jwt[0].chars().all(word) && !jwt[1].is_empty() && jwt[1].chars().all(word) && jwt[2].chars().all(word) {
            return true;
        }
        if segment.len() >= 32 && segment.bytes().all(|b| b.is_ascii_hexdigit()) {
            return true;
        }
        let body = segment.trim_end_matches('=');
        if segment.len() - body.len() > 2 || body.is_empty() || !body.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '~' | '+' | '/' | '-')) {
            return false;
        }
        let words = body.split(['-', '_']).collect::<Vec<_>>();
        if words.len() > 1 && words.iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphabetic())) {
            return false;
        }
        let (digit, lower, upper) = (body.bytes().any(|b| b.is_ascii_digit()), body.bytes().any(|b| b.is_ascii_lowercase()), body.bytes().any(|b| b.is_ascii_uppercase()));
        let distinct = body.chars().collect::<HashSet<_>>().len();
        distinct >= 12 && digit && (lower || upper) && ((lower && upper) || segment.len() >= 32)
    }

    fn decode(text: &str, plus: bool) -> String {
        let bytes = text.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut at = 0;
        while at < bytes.len() {
            match bytes[at] {
                b'%' if at + 2 < bytes.len() && bytes[at + 1].is_ascii_hexdigit() && bytes[at + 2].is_ascii_hexdigit() => {
                    out.push(u8::from_str_radix(&text[at + 1..at + 3], 16).unwrap_or(b'%'));
                    at += 3;
                    continue;
                }
                b'+' if plus => out.push(b' '),
                byte => out.push(byte),
            }
            at += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// JavaScript's `encodeURIComponent`.
    fn encode(text: &str) -> String {
        let mut out = String::new();
        for byte in text.bytes() {
            if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
                out.push(char::from(byte));
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        out
    }

    /// What a page's credentials were, and the rules that scrub them.
    #[derive(Default)]
    pub struct Redactor {
        known: HashSet<String>,
        order: VecDeque<String>,
        always: HashSet<String>,
        /// The values (and their URL-encoded forms) `text` replaces in a text, longest first.
        candidates: Vec<String>,
        dirty: bool,
    }

    impl Redactor {
        /// Remembers `value`, found under the field `name`, and answers its stand-in.
        fn scrub(&mut self, value: &str, name: &str) -> String {
            if value.is_empty() {
                return String::new();
            }
            if self.known.insert(value.to_owned()) {
                self.order.push_back(value.to_owned());
                while self.order.len() > KNOWN_MAX {
                    if let Some(old) = self.order.pop_front() {
                        self.known.remove(&old);
                        self.always.remove(&old);
                    }
                }
                self.dirty = true;
            }
            if one_time_name(name) && self.always.insert(value.to_owned()) {
                self.dirty = true;
            }
            PLACEHOLDER.to_owned()
        }

        #[cfg(test)]
        pub fn remembered(&self) -> usize {
            self.known.len()
        }

        /// The candidates `text` replaces are made again when a value came since: call it when a batch of scrubbing is done.
        pub fn refresh(&mut self) {
            if !self.dirty {
                return;
            }
            self.dirty = false;
            let mut all: Vec<String> = Vec::new();
            for value in self.order.iter().filter(|value| value.len() >= SUBSTRING_MIN || self.always.contains(*value)) {
                all.push(value.clone());
                let encoded = encode(value);
                if encoded != *value {
                    all.push(encoded);
                }
            }
            all.sort_by_key(|value| std::cmp::Reverse(value.len()));
            self.candidates = all;
        }

        /// `text` with every value remembered for the page replaced: the whole text at any length, a part of it from 8
        /// characters (or for a password or code), and the URL-encoded form of each.
        pub fn text(&self, text: &str) -> String {
            if self.known.contains(text) {
                return PLACEHOLDER.to_owned();
            }
            let mut out = text.to_owned();
            for value in &self.candidates {
                if out.contains(value.as_str()) {
                    out = out.replace(value.as_str(), PLACEHOLDER);
                }
            }
            out
        }

        /// A URL with the values of credential-named query and fragment fields and the path segments that look like
        /// tokens replaced.
        pub fn url(&mut self, raw: &str) -> String {
            // A `data:` address is its contents: only its type is kept, as a page's inline images listed tens of KB of
            // base64 a line.
            if raw.get(..5).is_some_and(|scheme| scheme.eq_ignore_ascii_case("data:")) {
                let head = raw.split_once(',').map_or(raw, |(head, _)| head);
                return format!("{head},... ({} characters)", raw.len());
            }
            let (before, fragment) = raw.split_once('#').map_or((raw, None), |(before, fragment)| (before, Some(fragment)));
            let (base, query) = before.split_once('?').map_or((before, None), |(base, query)| (base, Some(query)));
            let path_at = base.find("://").map_or(0, |at| base[at + 3..].find('/').map_or(base.len(), |slash| at + 3 + slash));
            let (head, path) = base.split_at(path_at);
            let path: Vec<String> = path
                .split('/')
                .map(|segment| {
                    let decoded = decode(segment, false);
                    if looks_like_token(&decoded) { self.scrub(&decoded, "") } else { segment.to_owned() }
                })
                .collect();
            let mut out = format!("{head}{}", path.join("/"));
            if let Some(query) = query {
                out.push('?');
                out.push_str(&query.split('&').map(|part| self.field(part, true)).collect::<Vec<_>>().join("&"));
            }
            if let Some(fragment) = fragment {
                out.push('#');
                out.push_str(&fragment.split('&').map(|part| self.field(part, false)).collect::<Vec<_>>().join("&"));
            }
            out
        }

        /// One `name=value` of a query (or fragment): the value goes when the name is a credential's. A fragment's part
        /// counts only for a plain name, as route text isn't a field.
        fn field(&mut self, part: &str, query: bool) -> String {
            let Some((raw_name, raw_value)) = part.split_once('=') else { return part.to_owned() };
            let name = decode(raw_name, true);
            let plain = query || (!raw_name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')));
            if raw_value.is_empty() || !plain || !secret_name(&name) {
                return part.to_owned();
            }
            self.scrub(&decode(raw_value, true), &name);
            format!("{raw_name}={PLACEHOLDER}")
        }

        /// A request's or response's headers (the protocol's name to value object) with each credential scrubbed.
        pub fn headers(&mut self, headers: &Value) -> Map<String, Value> {
            let Some(headers) = headers.as_object() else { return Map::new() };
            let kept = headers
                .iter()
                .map(|(name, value)| {
                    let value = value.as_str().map_or_else(|| value.to_string(), str::to_owned);
                    (name.clone(), Value::String(self.header(name, &value)))
                })
                .collect();
            self.refresh();
            kept
        }

        fn header(&mut self, name: &str, value: &str) -> String {
            let lower = name.to_ascii_lowercase();
            if value.is_empty() {
                return String::new();
            }
            if lower == ":path" {
                return self.url(value);
            }
            if PLAIN_HEADERS.contains(&lower.as_str()) {
                return value.to_owned();
            }
            if lower == "cookie" || lower == "set-cookie" {
                let set = lower == "set-cookie";
                let line = |this: &mut Self, line: &str| {
                    line.split(';')
                        .enumerate()
                        .map(|(index, part)| {
                            let part = if index > 0 { part.trim_start() } else { part };
                            let Some((cookie, cookie_value)) = part.split_once('=').filter(|(cookie, _)| !cookie.is_empty()) else { return part.to_owned() };
                            let cookie = cookie.trim();
                            if (set && index > 0 && COOKIE_ATTRIBUTES.iter().any(|attribute| cookie.eq_ignore_ascii_case(attribute))) || cookie_value.is_empty() {
                                return part.to_owned();
                            }
                            format!("{cookie}={}", this.scrub(cookie_value, ""))
                        })
                        .collect::<Vec<_>>()
                        .join("; ")
                };
                return if set { value.split('\n').map(|one| line(self, one)).collect::<Vec<_>>().join("\n") } else { line(self, value) };
            }
            if matches!(lower.as_str(), "location" | "referer" | "referrer") {
                return self.url(value);
            }
            if lower == "authorization" || lower == "proxy-authorization" {
                if let Some((scheme, credential)) = value.split_once(char::is_whitespace).filter(|(scheme, credential)| !scheme.is_empty() && !credential.trim().is_empty()) {
                    return format!("{scheme} {}", self.scrub(credential.trim_start(), ""));
                }
            }
            if SECRET_HEADERS.contains(&lower.as_str()) || secret_name(&lower) {
                return self.scrub(value, "");
            }
            value.to_owned()
        }

        /// A response body as text: JSON with the values under credential-named keys, and a form with its credential
        /// fields, scrubbed; any other text, or JSON that doesn't parse, as it is. Then every remembered value goes.
        pub fn body(&mut self, body: &str, content_type: &str) -> String {
            let kind = content_type.to_ascii_lowercase();
            let scrubbed = if kind.contains("json") {
                serde_json::from_str::<Value>(body).ok().map(|mut value| {
                    self.json(&mut value, false, "");
                    value.to_string()
                })
            } else if kind.contains("application/x-www-form-urlencoded") {
                Some(
                    body.split('&')
                        .map(|part| {
                            let Some((raw_name, raw_value)) = part.split_once('=') else { return part.to_owned() };
                            let name = decode(raw_name, true);
                            if raw_value.is_empty() || !secret_name(&name) {
                                return part.to_owned();
                            }
                            self.scrub(&decode(raw_value, true), &name);
                            format!("{raw_name}={PLACEHOLDER}")
                        })
                        .collect::<Vec<_>>()
                        .join("&"),
                )
            } else {
                None
            };
            self.refresh();
            self.text(&scrubbed.unwrap_or_else(|| body.to_owned()))
        }

        /// Replaces every value under a credential-named key, at any depth, keeping the keys.
        fn json(&mut self, value: &mut Value, secret: bool, name: &str) {
            match value {
                Value::Array(items) => items.iter_mut().for_each(|item| self.json(item, secret, name)),
                Value::Object(fields) => {
                    for (key, item) in fields.iter_mut() {
                        self.json(item, secret || secret_name(key), key);
                    }
                }
                Value::Null => {}
                Value::String(text) if text.is_empty() => {}
                other if secret => {
                    let shown = other.as_str().map_or_else(|| other.to_string(), str::to_owned);
                    self.scrub(&shown, name);
                    *other = Value::String(PLACEHOLDER.to_owned());
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent(id: &str, url: &str, headers: Value) -> Value {
        json!({ "requestId": id, "documentURL": "https://app.test/", "timestamp": 100.0, "wallTime": 1700000000.5, "type": "Fetch",
            "request": { "method": "POST", "url": url, "headers": headers, "postData": "{\"a\":1}" },
            "initiator": { "type": "script", "stack": { "callFrames": [{ "url": "https://app.test/app.js", "lineNumber": 9, "columnNumber": 3 }] } } })
    }

    fn answered(id: &str, status: i64, mime: &str) -> Value {
        json!({ "requestId": id, "response": { "status": status, "statusText": "x", "mimeType": mime, "headers": { "Content-Type": mime } } })
    }

    fn filter() -> WebNetwork {
        WebNetwork { limit: 100, ..Default::default() }
    }

    fn red() -> Redactor {
        Redactor::default()
    }

    fn headers(red: &mut Redactor, headers: Value) -> Map<String, Value> {
        red.headers(&headers)
    }

    /// A header is a credential by its name: the set browse lists, and any name that says auth, cookie, token, session,
    /// a key, a code or a signature; the CORS headers and HTTP/2 pseudo-headers are not, whatever they are called.
    #[test]
    fn a_header_is_scrubbed_by_its_name_and_the_plain_ones_are_kept() {
        let mut red = red();
        let kept = headers(&mut red, json!({
            "X-Api-Key": "k-1234", "X-CSRF-Token": "c-1", "X-Session-Id": "s-9", "X-Amz-Signature": "sig-abc", "X-Auth-Code": "123456",
            "X-Refresh-Key": "r-1", "X-Sig": "z", "Client-Secret": "shh", "X-Passcode": "42", "X-Otp": "9",
            "Accept": "*/*", "X-Codec": "h264", "X-Significant": "yes", "Access-Control-Allow-Credentials": "true",
            "Access-Control-Allow-Origin": "https://a.test", ":authority": "a.test", ":method": "GET", "X-Request-Id": "r-77",
        }));
        for name in ["X-Api-Key", "X-CSRF-Token", "X-Session-Id", "X-Amz-Signature", "X-Auth-Code", "X-Refresh-Key", "X-Sig", "Client-Secret", "X-Passcode", "X-Otp"] {
            assert_eq!(kept[name], "[redacted]", "{name}");
        }
        for (name, value) in [("Accept", "*/*"), ("X-Codec", "h264"), ("X-Significant", "yes"), ("Access-Control-Allow-Credentials", "true"), ("Access-Control-Allow-Origin", "https://a.test"), (":authority", "a.test"), (":method", "GET"), ("X-Request-Id", "r-77")] {
            assert_eq!(kept[name], value, "{name}");
        }
        let auth = headers(&mut red, json!({ "Authorization": "Bearer abc.def", "Proxy-Authorization": "Basic Zm9vOmJhcg==", "authorization": "rawtoken" }));
        assert_eq!((auth["Authorization"].as_str(), auth["Proxy-Authorization"].as_str(), auth["authorization"].as_str()), (Some("Bearer [redacted]"), Some("Basic [redacted]"), Some("[redacted]")));
        assert!(headers(&mut red, json!(null)).is_empty());
    }

    /// A cookie's value goes and its name stays; a Set-Cookie keeps its attributes.
    #[test]
    fn a_cookies_value_is_scrubbed_and_its_name_and_a_set_cookies_attributes_stay() {
        let mut red = red();
        let kept = headers(&mut red, json!({ "Cookie": "sid=abc123; theme=dark", "Set-Cookie": "sid=abc123; Path=/; Domain=a.test; Max-Age=60; SameSite=Lax; Secure; HttpOnly\nid=zzz; Expires=Wed, 21 Oct 2026 07:28:00 GMT" }));
        assert_eq!(kept["Cookie"], "sid=[redacted]; theme=[redacted]");
        assert_eq!(kept["Set-Cookie"], "sid=[redacted]; Path=/; Domain=a.test; Max-Age=60; SameSite=Lax; Secure; HttpOnly\nid=[redacted]; Expires=Wed, 21 Oct 2026 07:28:00 GMT");
    }

    /// A query's and a fragment's fields go by their names, in the address, `Location`, `Referer` and `:path`; the rest of
    /// the address stays as it was.
    #[test]
    fn a_urls_credential_fields_are_scrubbed_in_its_query_and_its_fragment() {
        let mut red = red();
        assert_eq!(
            red.url("https://a.test/cb?code=XYZ789&page=2&access_token=abcdefgh&q=a+b#access_token=tok12345&view=x&state_token=s9&note"),
            "https://a.test/cb?code=[redacted]&page=2&access_token=[redacted]&q=a+b#access_token=[redacted]&view=x&state_token=[redacted]&note"
        );
        assert_eq!(red.url("https://a.test/#/route/token=abc"), "https://a.test/#/route/token=abc", "route text in a fragment isn't a field");
        assert_eq!(red.url("/relative?session=s1&x=1"), "/relative?session=[redacted]&x=1");
        let kept = headers(&mut red, json!({ "Location": "https://a.test/next?token=t0k3n", "Referer": "https://a.test/?sig=aaaa&b=1", ":path": "/api?password=pw&x=1" }));
        assert_eq!((kept["Location"].as_str(), kept["Referer"].as_str(), kept[":path"].as_str()), (Some("https://a.test/next?token=[redacted]"), Some("https://a.test/?sig=[redacted]&b=1"), Some("/api?password=[redacted]&x=1")));
    }

    /// A path segment that looks like a credential goes: a JWT, 32 hex digits, a long mixed base64 string. A UUID, a slug of
    /// words, a short or a plain name stay.
    /// A `data:` address keeps only its type and length, and nothing of its contents is remembered as a credential.
    #[test]
    fn a_data_address_is_its_type_and_length() {
        let mut red = red();
        let image = format!("data:image/png;base64,{}", "iVBORw0KGgoAAAANSUhEUgAABNEAAABkBAMAAABay/".repeat(50));
        assert_eq!(red.url(&image), format!("data:image/png;base64,... ({} characters)", image.len()));
        assert_eq!(red.url("DATA:text/plain,hi"), "DATA:text/plain,... (18 characters)");
        assert_eq!(red.remembered(), 0);
    }

    #[test]
    fn a_token_looking_path_segment_is_scrubbed_and_uuids_and_slugs_are_not() {
        let mut red = red();
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abc-def_ghi";
        let url = red.url(&format!("https://a.test/v1/{jwt}/d41d8cd98f00b204e9800998ecf8427e/Zx9Qp3Lm7Vb2Nc5Td8Hk1Ws4/550e8400-e29b-41d4-a716-446655440000/how-to-install-the-extension/users/ana"));
        assert_eq!(url, "https://a.test/v1/[redacted]/[redacted]/[redacted]/550e8400-e29b-41d4-a716-446655440000/how-to-install-the-extension/users/ana");
        for yes in [jwt, "d41d8cd98f00b204e9800998ecf8427e", "Zx9Qp3Lm7Vb2Nc5Td8Hk1Ws4"] {
            assert!(redact::looks_like_token(yes), "{yes}");
        }
        for no in ["550e8400-e29b-41d4-a716-446655440000", "how-to-install-the-extension-now", "short", "aaaaaaaaaaaaaaaaaaaaaaaaaaaa", "${templateVariableName123}"] {
            assert!(!redact::looks_like_token(no), "{no}");
        }
    }

    /// JSON keeps its keys and loses every value under a credential-named key, at any depth; a form loses its credential
    /// fields; JSON that doesn't parse is text.
    #[test]
    fn a_json_or_form_body_loses_the_values_under_credential_names() {
        let mut red = red();
        let json = red.body(r#"{"user":{"name":"ana","password":"hunter2hunter2","tokens":["a","b",{"x":1}],"id":5},"access_token":123,"empty":"","note":null}"#, "application/json; charset=utf-8");
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, json!({ "user": { "name": "ana", "password": "[redacted]", "tokens": ["[redacted]", "[redacted]", { "x": "[redacted]" }], "id": 5 }, "access_token": "[redacted]", "empty": "", "note": null }));
        assert!(!json.contains("hunter2"));
        assert_eq!(red.body("user=ana&password=pw+1&q=x&otp=", "application/x-www-form-urlencoded"), "user=ana&password=[redacted]&q=x&otp=");
        assert_eq!(red.body("{not json: hunter2hunter2", "application/json"), "{not json: [redacted]", "text is scrubbed of what is known");
    }

    /// A value scrubbed once is replaced wherever it shows up after, under a name that isn't secret: in a console line and
    /// in a plain text body, whole at any length, in a part from 8 characters, and URL-encoded.
    #[test]
    fn a_scrubbed_value_is_replaced_in_a_console_line_and_a_plain_text_body() {
        let mut log = NetLog::default();
        let event = sent("1", "https://api.test/x", json!({ "Cookie": "sid=supersecretvalue123; pin=ab; g=pa ss/word99" }));
        log.ingest("Network.requestWillBeSent", &event);
        // A code typed into a field named for it is replaced at any length.
        log.protect_body(r#"{"otp":"424242"}"#, "application/json");
        let line = log.console(json!({ "kind": "console", "level": "error", "text": "sign in failed for supersecretvalue123 (ab) code 424242", "source": "https://app.test/a.js:1:1", "stack": null }));
        assert_eq!(line["text"], "sign in failed for [redacted] (ab) code [redacted]", "a short one only as the whole text");
        let body = log.protect_body("<p>supersecretvalue123 and pa%20ss%2Fword99 and ab</p>", "text/html");
        assert_eq!(body, "<p>[redacted] and [redacted] and ab</p>");
        assert_eq!(log.protect_body("ab", "text/plain"), "[redacted]", "the whole text, at any length");
        assert_eq!(log.protect_body("424242", "text/plain"), "[redacted]");
        assert_eq!(log.protect_body("x 424242 y", "text/plain"), "x [redacted] y", "a code is replaced at any length");
    }

    /// What is listed, in a detail and in the filter is the redacted address: a secret in a query can't be found by asking
    /// whether the address holds it, and a value learned later is replaced in earlier requests' answers.
    #[test]
    fn the_url_filter_matches_the_redacted_address_so_a_secret_cant_be_probed() {
        let mut log = NetLog::default();
        log.ingest("Network.requestWillBeSent", &sent("1", "https://api.test/x?token=SECRETVALUE9&a=1#access_token=FRAGSECRET77", json!({})));
        let by = |url: &str| log.list(&WebNetwork { url: Some(url.into()), limit: 10, ..Default::default() }, &|_| false, 0.0)["matching"].as_u64().unwrap();
        assert_eq!((by("SECRETVALUE9"), by("FRAGSECRET77"), by("token=[redacted]&a=1"), by("api.test")), (0, 0, 1, 1));
        let (request, shown) = log.detail("1", &|_| false).unwrap();
        assert_eq!(shown["url"], "https://api.test/x?token=[redacted]&a=1#access_token=[redacted]");
        assert_eq!(request.url, "https://api.test/x?token=[redacted]&a=1#access_token=[redacted]", "nothing else is held");
        assert!(!shown.to_string().contains("SECRETVALUE9"));
        // A value learned later is replaced in a request stored before.
        log.ingest("Network.requestWillBeSent", &sent("2", "https://api.test/laterlooksfine", json!({ "X-Session-Id": "laterlooksfine" })));
        assert_eq!(log.list(&filter(), &|_| false, 0.0)["requests"][1]["url"], "https://api.test/[redacted]");
    }

    #[test]
    fn at_most_the_last_2000_values_are_remembered() {
        let mut red = red();
        for n in 0..(redact::KNOWN_MAX + 100) {
            red.url(&format!("https://a.test/?token=value-number-{n}"));
        }
        red.refresh();
        assert_eq!(red.remembered(), redact::KNOWN_MAX);
        assert_eq!(red.text("value-number-0 value-number-2099"), "value-number-0 [redacted]", "the oldest was forgotten");
    }

    /// The events of one request make one record, and the headers the network stack adds (cookies, credentials, Set-Cookie)
    /// are hidden as they come: no value of one is ever held.
    #[test]
    fn one_requests_events_make_one_record_with_its_credentials_hidden() {
        let mut log = NetLog::default();
        log.ingest("Network.requestWillBeSent", &sent("1.1", "https://api.test/x?a=1", json!({ "Accept": "*/*", "Authorization": "Bearer abc" })));
        log.ingest("Network.requestWillBeSentExtraInfo", &json!({ "requestId": "1.1", "headers": { "Cookie": "sid=secret", "Accept": "*/*" } }));
        log.ingest("Network.responseReceived", &answered("1.1", 201, "application/json"));
        log.ingest("Network.responseReceivedExtraInfo", &json!({ "requestId": "1.1", "headers": { "Set-Cookie": "sid=secret2" } }));
        log.ingest("Network.loadingFinished", &json!({ "requestId": "1.1", "timestamp": 100.25, "encodedDataLength": 512.0 }));
        let (request, shown) = log.detail("1.1", &|_| false).unwrap();
        assert_eq!((request.method.as_str(), request.status, request.mime.as_str(), request.encoded, request.sent), ("POST", Some(201), "application/json", Some(512.0), Some(7)));
        assert_eq!((request.started, request.duration), (1_700_000_000_500.0, Some(250.0)));
        assert_eq!(request.initiator, "script https://app.test/app.js:10");
        assert!(!shown.to_string().contains("secret") && !shown.to_string().contains("abc"), "{shown}");
        assert_eq!((shown["requestHeaders"]["Cookie"].as_str(), shown["requestHeaders"]["Authorization"].as_str(), shown["responseHeaders"]["Set-Cookie"].as_str()), (Some("sid=[redacted]"), Some("Bearer [redacted]"), Some("sid=[redacted]")));
        assert_eq!(shown["pending"], false);
        // A failed one keeps why, and an event of a request nobody saw begin is dropped.
        log.ingest("Network.requestWillBeSent", &sent("2.1", "https://api.test/y", json!({})));
        log.ingest("Network.loadingFailed", &json!({ "requestId": "2.1", "timestamp": 100.5, "errorText": "net::ERR_BLOCKED_BY_CLIENT", "blockedReason": "inspector" }));
        log.ingest("Network.loadingFinished", &json!({ "requestId": "9.9", "timestamp": 1.0, "encodedDataLength": 1.0 }));
        let all = log.list(&filter(), &|_| false, 5.0);
        assert_eq!((all["matching"].as_u64(), all["kept"].as_u64()), (Some(2), Some(2)));
        assert_eq!(all["requests"][1]["failure"], "net::ERR_BLOCKED_BY_CLIENT (blocked: inspector)");
    }

    #[test]
    fn a_redirect_ends_its_record_and_starts_the_next_with_the_same_id() {
        let mut log = NetLog::default();
        log.ingest("Network.requestWillBeSent", &sent("1.1", "http://a.test/", json!({})));
        let mut next = sent("1.1", "https://a.test/", json!({}));
        next["redirectResponse"] = json!({ "status": 301, "statusText": "Moved", "mimeType": "text/html", "headers": { "Location": "https://a.test/" } });
        next["timestamp"] = json!(100.1);
        log.ingest("Network.requestWillBeSent", &next);
        let all = log.list(&filter(), &|_| false, 0.0);
        let urls: Vec<(&str, Option<i64>, bool)> = all["requests"].as_array().unwrap().iter().map(|r| (r["url"].as_str().unwrap(), r["status"].as_i64(), r["pending"].as_bool().unwrap())).collect();
        assert_eq!(urls, [("http://a.test/", Some(301), false), ("https://a.test/", None, true)]);
        assert_ne!(all["requests"][0]["id"], all["requests"][1]["id"]);
        assert_eq!(all["requests"][1]["id"], "1.1", "the live one keeps the protocol's id");
    }

    /// The buffer holds the newest `KEPT`; a filter picks by address, status class or exact status, type and time, and
    /// `limit` keeps the newest of what matches.
    #[test]
    fn the_buffer_keeps_the_newest_and_the_filters_pick_what_they_say() {
        let mut log = NetLog::default();
        for n in 0..(KEPT + 20) {
            let id = format!("r{n}");
            let mut event = sent(&id, &format!("https://api.test/item/{n}"), json!({}));
            event["wallTime"] = json!(1000.0 + n as f64);
            event["type"] = json!(if n % 2 == 0 { "XHR" } else { "Image" });
            log.ingest("Network.requestWillBeSent", &event);
            log.ingest("Network.responseReceived", &answered(&id, if n % 5 == 0 { 404 } else if n % 7 == 0 { 503 } else { 200 }, "application/json"));
        }
        assert_eq!(log.requests.len(), KEPT);
        assert!(log.detail("r19", &|_| false).is_none() && log.detail("r20", &|_| false).is_some(), "the oldest went");
        let pick = |change: &dyn Fn(&mut WebNetwork)| {
            let mut wanted = filter();
            change(&mut wanted);
            let all = log.list(&wanted, &|_| false, 0.0);
            (all["matching"].as_u64().unwrap(), all["requests"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_owned()).collect::<Vec<_>>())
        };
        let (count, ids) = pick(&|f| f.status = Some("4xx".into()));
        assert_eq!((count, ids.first().map(String::as_str)), (100, Some("r20")));
        assert_eq!(pick(&|f| f.status = Some("404".into())).0, 100);
        assert!(pick(&|f| f.status = Some("5XX".into())).1.iter().all(|id| id[1..].parse::<usize>().unwrap() % 7 == 0));
        assert_eq!(pick(&|f| f.status = Some("failed".into())).0, 0);
        assert_eq!(pick(&|f| f.kind = Some("image".into())).0, 250);
        assert_eq!(pick(&|f| f.url = Some("item/519".into())).1, ["r519"]);
        assert_eq!(pick(&|f| f.since = Some(1_000_000.0 * 1.0 + 515_000.0)).1, ["r516", "r517", "r518", "r519"]);
        let (count, ids) = pick(&|f| f.limit = 3);
        assert_eq!((count, ids), (500, vec!["r517".to_owned(), "r518".into(), "r519".into()]), "the newest three, oldest first");
    }

    /// A request of a site signed in with imported cookies, or from its document, is in no list and has no detail, though
    /// it was kept as it came.
    #[test]
    fn requests_of_an_imported_site_are_left_out_of_every_answer() {
        let mut log = NetLog::default();
        log.ingest("Network.requestWillBeSent", &sent("1", "https://bank.test/api", json!({})));
        let mut other = sent("2", "https://cdn.test/a.js", json!({}));
        other["documentURL"] = json!("https://bank.test/home");
        log.ingest("Network.requestWillBeSent", &other);
        let mut free = sent("3", "https://free.test/a", json!({}));
        free["documentURL"] = json!("https://free.test/");
        log.ingest("Network.requestWillBeSent", &free);
        log.ingest("Network.requestWillBeSent", &{
            let mut data = sent("4", "data:text/plain,hi", json!({}));
            data["documentURL"] = json!("https://free.test/");
            data
        });
        let hide = |host: &str| host == "bank.test";
        let all = log.list(&filter(), &hide, 0.0);
        assert_eq!(all["requests"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect::<Vec<_>>(), ["3", "4"]);
        assert!(log.detail("1", &hide).is_none() && log.detail("2", &hide).is_none() && log.detail("3", &hide).is_some());
        assert_eq!(log.legacy(&hide, 10).len(), 2);
    }

    /// A body is given only for a finished request with a text type of at most 1 MB, and never a binary one.
    #[test]
    fn a_body_is_refused_unless_the_request_finished_with_a_small_text_body() {
        let mut request = Request { id: "1".into(), finished: true, mime: "application/json".into(), encoded: Some(10.0), ..Default::default() };
        assert_eq!(body_refusal(&request), None);
        request.mime = "text/html; charset=utf-8".into();
        assert_eq!(body_refusal(&request), None);
        request.mime = "image/png".into();
        assert!(body_refusal(&request).unwrap().contains("text body"));
        request.mime = "application/octet-stream".into();
        assert!(body_refusal(&request).is_some());
        request.mime = "application/json".into();
        request.finished = false;
        assert!(body_refusal(&request).unwrap().contains("hasn't finished"));
        request.finished = true;
        request.failure = Some("net::ERR_FAILED".into());
        assert!(body_refusal(&request).unwrap().contains("failed"));
        request.failure = None;
        request.id = "1~2".into();
        assert!(body_refusal(&request).unwrap().contains("redirect"));
        request.id = "1".into();
        request.encoded = Some(BODY_MAX as f64 * 5.0);
        assert!(body_refusal(&request).unwrap().contains("larger"));
        assert_eq!(body_text(&json!({ "body": "{}", "base64Encoded": false })), Ok("{}".into()));
        assert!(body_text(&json!({ "body": "AAAA", "base64Encoded": true })).unwrap_err().contains("binary"));
        assert!(body_text(&json!({ "body": "a".repeat(BODY_MAX + 1), "base64Encoded": false })).unwrap_err().contains("larger"));
        assert!(body_text(&json!({})).is_err());
        for gone in ["No resource with given identifier found", "Request content was evicted from inspector cache"] {
            assert!(body_lost(gone.into()).starts_with("The body isn't kept any more"), "{gone}");
        }
        assert_eq!(body_lost("Target closed".into()), "Target closed");
    }

    #[test]
    fn a_console_call_keeps_its_level_text_and_place_and_an_exception_its_stack() {
        let line = console_line(&json!({ "type": "warning", "timestamp": 12.0, "args": [{ "type": "string", "value": "slow" }, { "type": "number", "value": 3 }, { "type": "object", "description": "Object" }],
            "stackTrace": { "callFrames": [{ "functionName": "f", "url": "https://app.test/app.js", "lineNumber": 4, "columnNumber": 1 }] } }));
        assert_eq!(line, json!({ "kind": "console", "level": "warning", "text": "slow 3 Object", "source": "https://app.test/app.js:5:2", "at": 12.0 }));
        assert_eq!(console_line(&json!({ "type": "assert", "args": [] }))["level"], "error");
        assert_eq!(console_line(&json!({ "type": "table", "args": [] }))["level"], "log");
        let exception = exception_line(&json!({ "timestamp": 7.0, "exceptionDetails": { "text": "Uncaught", "url": "https://app.test/app.js", "lineNumber": 9, "columnNumber": 2,
            "exception": { "description": "TypeError: x is undefined\n    at go (https://app.test/app.js:10:3)" },
            "stackTrace": { "callFrames": [{ "functionName": "go", "url": "https://app.test/app.js", "lineNumber": 9, "columnNumber": 2 }, { "functionName": "", "url": "https://app.test/app.js", "lineNumber": 20, "columnNumber": 0 }] } } }));
        assert_eq!(exception["kind"], "exception");
        assert_eq!(exception["text"], "Uncaught TypeError: x is undefined");
        assert_eq!(exception["source"], "https://app.test/app.js:10:3");
        assert_eq!(exception["stack"], "at go (https://app.test/app.js:10:3)\nat (anonymous) (https://app.test/app.js:21:1)");
        assert_eq!(console_line(&json!({ "type": "log", "args": [{ "type": "string", "value": "x".repeat(5000) }] }))["text"].as_str().unwrap().len(), TEXT_MAX + "... (cut)".len());
    }

    /// The thread reads what it is fed into the shared log, so the main thread parses none of it.
    #[test]
    fn fed_bytes_reach_the_shared_log_from_another_thread() {
        let log: Log = Arc::default();
        let message = json!({ "method": "Network.requestWillBeSent", "params": sent("7", "https://a.test/x", json!({ "Cookie": "a=b" })) }).to_string();
        feed(&log, message.as_bytes());
        feed(&log, b"not json");
        for _ in 0..200 {
            if log.lock().unwrap().requests.len() == 1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (_, shown) = log.lock().unwrap().detail("7", &|_| false).expect("read by the thread");
        assert_eq!(shown["requestHeaders"]["Cookie"], "a=[redacted]");
    }
}
