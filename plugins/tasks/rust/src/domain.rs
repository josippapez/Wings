//! What an issue is: the types, the limits that keep a project inside the plugin's storage, the labels, how an issue is
//! saved as JSON, and the small rules about keys and descriptions that every other module shares. Nothing here touches
//! Wings.
//!
//! Each issue is its own storage entry, and Wings holds 1 MB per entry and 50 MB for the whole plugin, shared by every
//! project. An issue with a short description and ten one-line log entries is about 2 KB of JSON, so 300 of them are about
//! 600 KB. `ISSUE_BUDGET` stops one issue before the 1 MB wall, with a message instead of storage's own refusal.

use serde_json::{Map, Value, json};

pub mod limits {
    pub const ISSUES: usize = 300;
    pub const TITLE: usize = 200;
    pub const DESCRIPTION: usize = 20_000;
    pub const NOTE: usize = 20_000;
    pub const BRANCH: usize = 200;
    pub const PR: usize = 500;
    pub const LOG: usize = 300;
    pub const COMMIT: usize = 40;
    pub const FILES: usize = 100;
    pub const FILE_PATH: usize = 300;
    pub const FILE_WHAT: usize = 300;
}

/// The most one issue's entry may take, in bytes of JSON.
pub const ISSUE_BUDGET: usize = 1_000_000;
pub const FORMAT_VERSION: u64 = 3;
/// Links one issue keeps to others, of each kind.
pub const MAX_LINKS: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Todo,
    InProgress,
    InReview,
    Done,
    Canceled,
}

impl Status {
    pub const ALL: [Status; 5] = [Status::Todo, Status::InProgress, Status::InReview, Status::Done, Status::Canceled];
    /// The columns of the board, in the order an issue moves through them. Canceled sits behind a toggle.
    pub const FLOW: [Status; 4] = [Status::Todo, Status::InProgress, Status::InReview, Status::Done];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Todo => "todo",
            Status::InProgress => "in_progress",
            Status::InReview => "in_review",
            Status::Done => "done",
            Status::Canceled => "canceled",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Todo => "To do",
            Status::InProgress => "In progress",
            Status::InReview => "In review",
            Status::Done => "Done",
            Status::Canceled => "Canceled",
        }
    }

    pub fn parse(text: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|status| status.as_str() == text)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IssueType {
    Task,
    Bug,
    Feature,
    Epic,
}

impl IssueType {
    pub const ALL: [IssueType; 4] = [IssueType::Task, IssueType::Bug, IssueType::Feature, IssueType::Epic];

    pub fn as_str(self) -> &'static str {
        match self {
            IssueType::Task => "task",
            IssueType::Bug => "bug",
            IssueType::Feature => "feature",
            IssueType::Epic => "epic",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            IssueType::Task => "Task",
            IssueType::Bug => "Bug",
            IssueType::Feature => "Feature",
            IssueType::Epic => "Epic",
        }
    }

    pub fn parse(text: &str) -> Option<IssueType> {
        IssueType::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    /// Only these can be somebody's parent.
    pub fn can_parent(self) -> bool {
        matches!(self, IssueType::Epic | IssueType::Feature)
    }
}

/// Linear's priorities. An issue without one has none: "none" is only what a person or an agent asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Priority {
    Urgent,
    High,
    Medium,
    Low,
}

impl Priority {
    pub const NAMES: [&'static str; 5] = ["urgent", "high", "medium", "low", "none"];

    pub fn as_str(self) -> &'static str {
        Priority::NAMES[self as usize]
    }

    pub fn parse(text: &str) -> Option<Priority> {
        [Priority::Urgent, Priority::High, Priority::Medium, Priority::Low].into_iter().find(|p| p.as_str() == text)
    }
}

pub fn priority_label(priority: Option<Priority>) -> &'static str {
    match priority {
        Some(Priority::Urgent) => "Urgent",
        Some(Priority::High) => "High",
        Some(Priority::Medium) => "Medium",
        Some(Priority::Low) => "Low",
        None => "No priority",
    }
}

/// Sorts urgent first and no priority last.
pub fn priority_rank(priority: Option<Priority>) -> usize {
    priority.map_or(4, |p| p as usize)
}

/// Who wrote a log entry: the user in the UI, "git" for what the plugin found in the repository by itself, or the agent
/// session a tool call came from. "git" is its own kind rather than "you" so the log never claims the user did something
/// they didn't do.
#[derive(Clone, PartialEq, Debug)]
pub enum Actor {
    You,
    Git,
    Agent(Agent),
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Agent {
    pub id: String,
    pub name: Option<String>,
    /// Which agent CLI the session belongs to. Absent on sessions recorded before there was more than Claude Code.
    pub kind: Option<String>,
    /// "journal" for what an agent's change journal wrote at the end of a turn.
    pub via: Option<String>,
}

impl Actor {
    pub fn agent(&self) -> Option<&Agent> {
        match self {
            Actor::Agent(agent) => Some(agent),
            _ => None,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Actor::You => json!("you"),
            Actor::Git => json!("git"),
            Actor::Agent(a) => {
                let mut out = Map::new();
                out.insert("id".into(), json!(a.id));
                put(&mut out, "name", a.name.as_deref());
                put(&mut out, "kind", a.kind.as_deref());
                put(&mut out, "via", a.via.as_deref());
                Value::Object(out)
            }
        }
    }

    fn from_value(value: &Value) -> Actor {
        match value.as_str() {
            Some("you") => return Actor::You,
            Some("git") => return Actor::Git,
            _ => {}
        }
        Actor::Agent(Agent { id: text(value, "id").unwrap_or_default(), name: text(value, "name"), kind: text(value, "kind"), via: text(value, "via") })
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct LogEntry {
    pub at: f64,
    pub by: Actor,
    /// created, status, note, link, type or priority.
    pub kind: String,
    pub text: String,
}

#[derive(Clone, PartialEq, Debug)]
pub struct SessionRef {
    pub id: String,
    pub name: Option<String>,
    pub kind: Option<String>,
    /// When a tool call from this session last touched the issue.
    pub seen: f64,
}

/// One file an agent changed on this issue, as its change journal described it.
#[derive(Clone, PartialEq, Debug)]
pub struct FileChange {
    pub path: String,
    pub what: String,
    /// When the newest turn that changed it reported it.
    pub at: f64,
    /// The agent session of that turn.
    pub session: Option<FileSession>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct FileSession {
    pub id: String,
    pub name: Option<String>,
    pub kind: Option<String>,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Links {
    pub branch: Option<String>,
    pub pr: Option<String>,
    pub commits: Option<Vec<String>>,
}

impl Links {
    pub fn is_empty(&self) -> bool {
        self.branch.is_none() && self.pr.is_none() && self.commits.is_none()
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Issue {
    /// Like `WIN-12`. Never reused.
    pub key: String,
    pub title: String,
    /// Markdown: goal, scope, acceptance criteria.
    pub description: Option<String>,
    pub status: Status,
    pub kind: IssueType,
    /// Absent for no priority.
    pub priority: Option<Priority>,
    /// The epic or feature this belongs to. Same project, never itself or one of its own children.
    pub parent: Option<String>,
    /// Symmetric: if A lists B, B lists A.
    pub related: Option<Vec<String>>,
    /// The issues that have to finish first. Their `blocks` is worked out from this, not stored.
    pub blocked_by: Option<Vec<String>>,
    pub links: Option<Links>,
    /// One row per path, newest first, up to `limits::FILES`.
    pub files: Option<Vec<FileChange>>,
    /// Append-only and deduped by id.
    pub sessions: Vec<SessionRef>,
    /// Append-only. Nothing edits or removes an entry.
    pub log: Vec<LogEntry>,
    pub created_at: f64,
    pub updated_at: f64,
}

/// What one project's storage entry holds. `next` is the number the next issue gets, and only ever grows.
#[derive(Clone, PartialEq, Debug)]
pub struct Store {
    pub prefix: String,
    pub next: u64,
    pub issues: Vec<Issue>,
}

fn text(value: &Value, name: &str) -> Option<String> {
    value.get(name).and_then(Value::as_str).map(str::to_owned)
}

fn put(out: &mut Map<String, Value>, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        out.insert(name.to_owned(), json!(value));
    }
}

fn list(value: &Value, name: &str) -> Option<Vec<String>> {
    value.get(name).and_then(Value::as_array).map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
}

fn put_list(out: &mut Map<String, Value>, name: &str, items: &Option<Vec<String>>) {
    if let Some(items) = items {
        out.insert(name.to_owned(), json!(items));
    }
}

/// A time as JSON: JavaScript wrote them as whole milliseconds, and they are written back the same way.
pub fn num(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9e15 { json!(value as i64) } else { json!(value) }
}

fn at(value: &Value, name: &str) -> f64 {
    value.get(name).and_then(Value::as_f64).unwrap_or(0.0)
}

impl Issue {
    /// An issue as storage holds it, or none for a value that isn't one: it needs a key.
    pub fn from_value(value: &Value) -> Option<Issue> {
        let key = text(value, "key")?;
        let links = value.get("links").filter(|links| links.is_object()).map(|links| Links { branch: text(links, "branch"), pr: text(links, "pr"), commits: list(links, "commits") });
        let files = value.get("files").and_then(Value::as_array).map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let session = row.get("session").filter(|session| session.is_object()).map(|s| FileSession { id: text(s, "id").unwrap_or_default(), name: text(s, "name"), kind: text(s, "kind") });
                    Some(FileChange { path: text(row, "path")?, what: text(row, "what").unwrap_or_default(), at: at(row, "at"), session })
                })
                .collect()
        });
        Some(Issue {
            key,
            title: text(value, "title").unwrap_or_default(),
            description: text(value, "description"),
            status: text(value, "status").and_then(|s| Status::parse(&s)).unwrap_or(Status::Todo),
            kind: text(value, "type").and_then(|s| IssueType::parse(&s)).unwrap_or(IssueType::Task),
            priority: text(value, "priority").and_then(|s| Priority::parse(&s)),
            parent: text(value, "parent"),
            related: list(value, "related"),
            blocked_by: list(value, "blockedBy"),
            links,
            files,
            sessions: value
                .get("sessions")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().map(|s| SessionRef { id: text(s, "id").unwrap_or_default(), name: text(s, "name"), kind: text(s, "kind"), seen: at(s, "seen") }).collect())
                .unwrap_or_default(),
            log: value
                .get("log")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().map(|e| LogEntry { at: at(e, "at"), by: e.get("by").map_or(Actor::You, Actor::from_value), kind: text(e, "kind").unwrap_or_default(), text: text(e, "text").unwrap_or_default() }).collect())
                .unwrap_or_default(),
            created_at: at(value, "createdAt"),
            updated_at: at(value, "updatedAt"),
        })
    }

    pub fn to_value(&self) -> Value {
        let mut out = Map::new();
        out.insert("key".into(), json!(self.key));
        out.insert("title".into(), json!(self.title));
        put(&mut out, "description", self.description.as_deref());
        out.insert("status".into(), json!(self.status.as_str()));
        out.insert("type".into(), json!(self.kind.as_str()));
        put(&mut out, "priority", self.priority.map(Priority::as_str));
        put(&mut out, "parent", self.parent.as_deref());
        put_list(&mut out, "related", &self.related);
        put_list(&mut out, "blockedBy", &self.blocked_by);
        if let Some(links) = &self.links {
            let mut l = Map::new();
            put(&mut l, "branch", links.branch.as_deref());
            put(&mut l, "pr", links.pr.as_deref());
            put_list(&mut l, "commits", &links.commits);
            out.insert("links".into(), Value::Object(l));
        }
        if let Some(files) = &self.files {
            let rows: Vec<Value> = files
                .iter()
                .map(|f| {
                    let mut row = Map::new();
                    row.insert("path".into(), json!(f.path));
                    row.insert("what".into(), json!(f.what));
                    row.insert("at".into(), num(f.at));
                    if let Some(s) = &f.session {
                        let mut session = Map::new();
                        session.insert("id".into(), json!(s.id));
                        put(&mut session, "name", s.name.as_deref());
                        put(&mut session, "kind", s.kind.as_deref());
                        row.insert("session".into(), Value::Object(session));
                    }
                    Value::Object(row)
                })
                .collect();
            out.insert("files".into(), Value::Array(rows));
        }
        let sessions: Vec<Value> = self
            .sessions
            .iter()
            .map(|s| {
                let mut row = Map::new();
                row.insert("id".into(), json!(s.id));
                put(&mut row, "name", s.name.as_deref());
                put(&mut row, "kind", s.kind.as_deref());
                row.insert("seen".into(), num(s.seen));
                Value::Object(row)
            })
            .collect();
        out.insert("sessions".into(), Value::Array(sessions));
        let log: Vec<Value> = self.log.iter().map(|e| json!({ "at": num(e.at), "by": e.by.to_value(), "kind": e.kind, "text": e.text })).collect();
        out.insert("log".into(), Value::Array(log));
        out.insert("createdAt".into(), num(self.created_at));
        out.insert("updatedAt".into(), num(self.updated_at));
        Value::Object(out)
    }
}

/// What a page or a tool asks for about an issue's type and its place among the others. For an update, "" clears `parent`.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Relations {
    pub kind: Option<String>,
    pub parent: Option<String>,
    pub related_add: Option<Vec<String>>,
    pub related_remove: Option<Vec<String>>,
    pub blocked_by_add: Option<Vec<String>>,
    pub blocked_by_remove: Option<Vec<String>>,
}

impl Relations {
    pub fn is_empty(&self) -> bool {
        *self == Relations::default()
    }
}

/// What a page or a tool asks main to do.
#[derive(Clone, PartialEq, Debug)]
pub enum Op {
    Create(Create),
    Update(Update),
    Note { key: String, text: String },
    /// What an agent's journal says a turn changed: one row per path, the newest description wins. Writes no log entry.
    Files { key: String, files: Vec<(String, String)> },
    /// With `children`, its child issues, theirs and so on go too. Otherwise they stay and lose their parent.
    Remove { key: String, children: bool },
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Create {
    pub title: String,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub branch: Option<String>,
    pub rel: Relations,
}

/// For `branch` and `pr`, "" clears the link.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Update {
    pub key: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub branch: Option<String>,
    pub pr: Option<String>,
    pub commit: Option<String>,
    pub rel: Relations,
}

impl Op {
    pub fn update(key: &str) -> Update {
        Update { key: key.to_owned(), ..Update::default() }
    }

    pub fn create(title: &str) -> Create {
        Create { title: title.to_owned(), ..Create::default() }
    }

    /// The operation as a page sends it to main.
    pub fn to_value(&self) -> Value {
        let mut out = Map::new();
        let mut set = |name: &str, value: &Option<String>| {
            if let Some(value) = value {
                out.insert(name.to_owned(), json!(value));
            }
        };
        let rel = |out: &mut Map<String, Value>, rel: &Relations| {
            if let Some(kind) = &rel.kind {
                out.insert("type".into(), json!(kind));
            }
            if let Some(parent) = &rel.parent {
                out.insert("parent".into(), json!(parent));
            }
            for (name, items) in [("relatedAdd", &rel.related_add), ("relatedRemove", &rel.related_remove), ("blockedByAdd", &rel.blocked_by_add), ("blockedByRemove", &rel.blocked_by_remove)] {
                put_list(out, name, items);
            }
        };
        match self {
            Op::Create(c) => {
                set("title", &Some(c.title.clone()));
                set("description", &c.description);
                set("status", &c.status);
                set("priority", &c.priority);
                set("branch", &c.branch);
                out.insert("kind".into(), json!("create"));
                rel(&mut out, &c.rel);
            }
            Op::Update(u) => {
                set("key", &Some(u.key.clone()));
                set("title", &u.title);
                set("description", &u.description);
                set("status", &u.status);
                set("priority", &u.priority);
                set("branch", &u.branch);
                set("pr", &u.pr);
                set("commit", &u.commit);
                out.insert("kind".into(), json!("update"));
                rel(&mut out, &u.rel);
            }
            Op::Note { key, text } => {
                out.insert("kind".into(), json!("note"));
                out.insert("key".into(), json!(key));
                out.insert("text".into(), json!(text));
            }
            Op::Files { key, files } => {
                out.insert("kind".into(), json!("files"));
                out.insert("key".into(), json!(key));
                out.insert("files".into(), Value::Array(files.iter().map(|(path, what)| json!({ "path": path, "what": what })).collect()));
            }
            Op::Remove { key, children } => {
                out.insert("kind".into(), json!("remove"));
                out.insert("key".into(), json!(key));
                out.insert("children".into(), json!(children));
            }
        }
        Value::Object(out)
    }

    pub fn from_value(value: &Value) -> Result<Op, String> {
        let get = |name: &str| text(value, name);
        let key = || get("key").unwrap_or_default();
        let rel = Relations {
            kind: get("type"),
            parent: get("parent"),
            related_add: list(value, "relatedAdd"),
            related_remove: list(value, "relatedRemove"),
            blocked_by_add: list(value, "blockedByAdd"),
            blocked_by_remove: list(value, "blockedByRemove"),
        };
        match get("kind").as_deref() {
            Some("create") => Ok(Op::Create(Create { title: get("title").unwrap_or_default(), description: get("description"), status: get("status"), priority: get("priority"), branch: get("branch"), rel })),
            Some("update") => Ok(Op::Update(Update { key: key(), title: get("title"), description: get("description"), status: get("status"), priority: get("priority"), branch: get("branch"), pr: get("pr"), commit: get("commit"), rel })),
            Some("note") => Ok(Op::Note { key: key(), text: get("text").unwrap_or_default() }),
            Some("files") => Ok(Op::Files {
                key: key(),
                files: value["files"].as_array().map(|rows| rows.iter().map(|f| (text(f, "path").unwrap_or_default(), text(f, "what").unwrap_or_default())).collect()).unwrap_or_default(),
            }),
            Some("remove") => Ok(Op::Remove { key: key(), children: value["children"] == true }),
            _ => Err("That isn't an operation Issues knows.".into()),
        }
    }
}

/// What an empty description starts as when it is first edited on the issue page.
pub const DESCRIPTION_TEMPLATE: &str = "## Goal\n\n## Scope\n\n## Done when\n- [ ] \n\n## Validation\n";

/// The description without its empty sections: a `## heading` whose body is blank or only empty checklist items (`- [ ]`)
/// is dropped, and text before the first heading stays. The untouched template comes out as "".
pub fn visible_description(description: Option<&str>) -> String {
    let description = description.unwrap_or_default();
    let mut parts: Vec<&str> = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    for line in description.split_inclusive('\n') {
        if line.starts_with("## ") && offset > start {
            parts.push(&description[start..offset]);
            start = offset;
        }
        offset += line.len();
    }
    parts.push(&description[start..]);
    let body = |part: &str| {
        // The heading's own line goes, and so does every empty checklist item.
        let rest: Vec<&str> = part.lines().skip(1).filter(|line| !is_empty_item(line)).collect();
        rest.join("\n").trim().to_owned()
    };
    parts.into_iter().filter(|part| if part.starts_with("## ") { !body(part).is_empty() } else { !part.trim().is_empty() }).collect::<String>().trim().to_owned()
}

/// A line that is only `- [ ]` or `* [ ]`.
fn is_empty_item(line: &str) -> bool {
    line.trim().strip_prefix(['-', '*']).is_some_and(|rest| rest.trim_start().strip_prefix("[ ]").is_some_and(|after| after.trim().is_empty()))
}

/// Whether a description says anything beyond the template's headings and empty checklist items.
pub fn has_content(description: Option<&str>) -> bool {
    !visible_description(description).is_empty()
}

// ---------- keys ----------

/// Up to three letters of the project's name, uppercase, or ISS when it has none. "my-app" is MYA.
pub fn prefix_for(name: &str) -> String {
    let letters: String = name.chars().filter(char::is_ascii_alphabetic).take(3).collect::<String>().to_uppercase();
    if letters.is_empty() { "ISS".to_owned() } else { letters }
}

pub fn key_number(key: &str) -> u64 {
    key.rsplit('-').next().and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// 1 to 3 capital letters, a dash and 1 to 9 digits.
pub fn is_key(text: &str) -> bool {
    let Some((letters, digits)) = text.split_once('-') else {
        return false;
    };
    (1..=3).contains(&letters.len()) && letters.bytes().all(|b| b.is_ascii_uppercase()) && (1..=9).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
}

/// What a person or an agent typed, as a key: trimmed and uppercased.
pub fn normalize_key(value: &str) -> String {
    value.trim().to_uppercase()
}

/// An error unless `value` is one of `names`, saying which `field` takes what.
pub fn check_enum(value: &str, names: &[&str], field: &str) -> Result<(), String> {
    if names.contains(&value) { Ok(()) } else { Err(format!("{field} must be one of {}.", names.join(", "))) }
}

pub const STATUS_NAMES: [&str; 5] = ["todo", "in_progress", "in_review", "done", "canceled"];
pub const TYPE_NAMES: [&str; 4] = ["task", "bug", "feature", "epic"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_prefix_is_letters_only_up_to_three_uppercase_and_iss_when_none() {
        assert_eq!(prefix_for("web"), "WEB");
        assert_eq!(prefix_for("my-app-2"), "MYA");
        assert_eq!(prefix_for("42-x"), "X");
        assert_eq!(prefix_for("12345"), "ISS");
        assert_eq!(prefix_for("Ünï"), "N");
        assert_eq!(prefix_for(""), "ISS");
    }

    #[test]
    fn sections_with_nothing_in_them_dont_count_as_content() {
        assert_eq!(DESCRIPTION_TEMPLATE, "## Goal\n\n## Scope\n\n## Done when\n- [ ] \n\n## Validation\n");
        assert!(!has_content(Some(DESCRIPTION_TEMPLATE)));
        assert!(!has_content(None));
        assert!(!has_content(Some("  \n")));
        assert!(has_content(Some("## Goal\nShip it\n\n## Scope\n")));
        assert!(has_content(Some("## Done when\n- [x] done\n")));
        assert!(has_content(Some("plain text, no headings")));
        assert_eq!(visible_description(Some("## Goal\nShip it\n\n## Scope\n\n## Done when\n- [ ] \n- [ ] tests\n\n## Validation\n")), "## Goal\nShip it\n\n## Done when\n- [ ] \n- [ ] tests");
        assert_eq!(visible_description(Some("Intro\n\n## Goal\n")), "Intro");
        assert_eq!(visible_description(Some(DESCRIPTION_TEMPLATE)), "");
    }

    #[test]
    fn keys_are_read_as_the_tracker_writes_them() {
        assert!(is_key("WEB-12") && is_key("A-1") && is_key("ABC-123456789"));
        assert!(!is_key("WEBS-1") && !is_key("web-1") && !is_key("WEB-") && !is_key("WEB-1 x") && !is_key("WEB") && !is_key("WEB-1234567890"));
        assert_eq!(normalize_key(" web-3 "), "WEB-3");
        assert_eq!(key_number("WEB-12"), 12);
    }

    #[test]
    fn an_issue_reads_and_writes_the_json_it_was_saved_as() {
        let saved = json!({
            "key": "WEB-1", "title": "A", "description": "d", "status": "in_progress", "type": "bug", "priority": "high", "parent": "WEB-9",
            "related": ["WEB-2"], "blockedBy": ["WEB-3"], "links": { "branch": "feat/a", "pr": "https://x.example/1", "commits": ["abc1234"] },
            "files": [{ "path": "a.rs", "what": "changed (+1 -0)", "at": 5, "session": { "id": "s", "name": "n", "kind": "opencode" } }],
            "sessions": [{ "id": "s", "name": "n", "kind": "claude", "seen": 4 }],
            "log": [{ "at": 1, "by": "you", "kind": "created", "text": "Created" }, { "at": 2, "by": { "id": "s", "name": "n", "via": "journal" }, "kind": "note", "text": "x" }, { "at": 3, "by": "git", "kind": "link", "text": "y" }],
            "createdAt": 1, "updatedAt": 6,
        });
        let issue = Issue::from_value(&saved).unwrap();
        assert_eq!((issue.status, issue.kind, issue.priority), (Status::InProgress, IssueType::Bug, Some(Priority::High)));
        assert_eq!(issue.to_value(), saved);
        let plain = json!({ "key": "WEB-2", "title": "B", "status": "todo", "type": "task", "sessions": [], "log": [], "createdAt": 1700000000000_i64, "updatedAt": 1700000000001_i64 });
        assert_eq!(Issue::from_value(&plain).unwrap().to_value(), plain);
    }

    #[test]
    fn an_operation_crosses_between_a_page_and_main_unchanged() {
        let ops = [
            Op::Create(Create { title: "T".into(), description: Some("d".into()), status: Some("todo".into()), priority: Some("high".into()), branch: Some("b".into()), rel: Relations { kind: Some("epic".into()), parent: Some("WEB-1".into()), related_add: Some(vec!["WEB-2".into()]), ..Relations::default() } }),
            Op::Update(Update { key: "WEB-1".into(), title: Some("t".into()), pr: Some(String::new()), commit: Some("abc1234".into()), rel: Relations { blocked_by_remove: Some(vec!["WEB-3".into()]), ..Relations::default() }, ..Update::default() }),
            Op::Note { key: "WEB-1".into(), text: "n".into() },
            Op::Files { key: "WEB-1".into(), files: vec![("a".into(), "b".into())] },
            Op::Remove { key: "WEB-1".into(), children: true },
        ];
        for op in ops {
            assert_eq!(Op::from_value(&op.to_value()), Ok(op));
        }
    }
}
