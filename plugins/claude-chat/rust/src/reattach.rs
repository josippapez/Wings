//! A chat whose plugin was updated or restarted while its Claude Code ran goes on with that same Claude Code: Wings keeps
//! it running (`host::processes`), and the chat's next page takes it over (`host::adopt`) instead of starting another.
//! So does a chat whose Wings restarted or updated: Wings' terminal server ran its Claude Code (`host::spawn_surviving`).
//! A chat never starts a Claude Code on a session another one still runs (`host::processes_of`).
//!
//! The new page shows what the page before showed. The conversation comes from the session's transcript up to the entry
//! before the first message among the lines Wings kept, and the rest from those lines: Claude Code prints each message
//! under its transcript entry's uuid (measured), so nothing shows twice. Wings keeps the lines from where the page
//! before last said it had all it needed of them, which is when a turn ended. What only a page knows (your answers to
//! Claude's questions, the messages you queued, the effort you picked) and the answers to its opening requests, which
//! Claude Code doesn't print again, it keeps with the program as it goes (`ProcessInput::note`).
//!
//! Taking Claude Code over writes nothing to it: the next write is still yours.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::session::{self, Ask, Item, Queued, Session};

/// Of what Claude Code prints, the answers to the chat's opening requests and the lines that say how the session stands,
/// by kind, the last of each of which a page that takes it over hears once Wings let go of the lines before.
fn lasting(line: &Value) -> Option<&str> {
    match line["type"].as_str()? {
        "control_response" => {
            let id = line["response"]["request_id"].as_str()?;
            [session::INITIALIZE, session::CONTEXT, session::SETTINGS, session::USAGE, session::START_MODE, crate::mods::ATTACH, crate::remote::REQUEST].contains(&id).then_some(id)
        }
        // The panes a plugin has open, which Claude Code says once: the new page asks for each again.
        "system" if line["subtype"] == "ui_panes" => Some("ui_panes"),
        "system" if line["subtype"] == "background_tasks_changed" => Some("tasks"),
        "rate_limit_event" => Some("limits"),
        _ => None,
    }
}

/// The uuid of a message of the conversation, yours or Claude's, which its transcript entry has too.
fn said(line: &Value) -> Option<&str> {
    let message = matches!(line["type"].as_str(), Some("user" | "assistant")) && line["parent_tool_use_id"].is_null();
    message.then(|| line["uuid"].as_str()).flatten()
}

fn asked(line: &Value) -> Option<&str> {
    (line["type"] == "control_request" && line["request"]["subtype"] == "can_use_tool").then(|| line["request_id"].as_str()).flatten()
}

fn ask<'a>(session: &'a Session, id: &str) -> Option<&'a Ask> {
    session.items.iter().rev().find_map(|item| match item {
        Item::Ask(ask) if ask.request == id => Some(ask),
        _ => None,
    })
}

fn ask_mut<'a>(session: &'a mut Session, id: &str) -> Option<&'a mut Ask> {
    session.items.iter_mut().rev().find_map(|item| match item {
        Item::Ask(ask) if ask.request == id => Some(ask),
        _ => None,
    })
}

/// A line Claude Code printed as the new page hears it: your message it read, which the page before showed as you sent
/// it and this page hasn't, is heard as your message.
pub fn as_heard(session: &Session, line: &Value) -> Option<Value> {
    let uuid = line["uuid"].as_str().filter(|_| line["type"] == "user" && line["isReplay"] == true)?;
    let known = session.queued.iter().any(|queued| queued.uuid == uuid) || session.prompts.iter().any(|prompt| prompt.uuid == uuid);
    if known {
        return None;
    }
    let mut mine = line.clone();
    mine.as_object_mut()?.remove("isReplay");
    Some(mine)
}

/// What a chat's page keeps with its Claude Code for the page that may take it over.
#[derive(Debug, Default, PartialEq)]
pub struct Note {
    /// The last line of each lasting kind.
    pub lasting: Vec<Value>,
    /// Your answers to the questions Claude asked in the turn that runs, by request.
    pub answered: Vec<(String, bool)>,
    /// The messages you queued that Claude hasn't read yet, as uuid and text. Their pictures stay out.
    pub queued: Vec<(String, String)>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: Option<String>,
    pub cost: f64,
}

impl Note {
    /// The note a chat's page kept, none for anything else.
    pub fn read(note: &Value) -> Option<Note> {
        if note["chat"] != 1 {
            return None;
        }
        let text = |key: &str| note[key].as_str().map(str::to_owned);
        let list = |key: &str| note[key].as_array().cloned().unwrap_or_default();
        Some(Note {
            lasting: list("lasting"),
            answered: list("answered").iter().filter_map(|pair| Some((pair[0].as_str()?.to_owned(), pair[1].as_bool()?))).collect(),
            queued: list("queued").iter().filter_map(|queued| Some((queued["uuid"].as_str()?.to_owned(), queued["text"].as_str()?.to_owned()))).collect(),
            model: text("model"),
            effort: text("effort"),
            mode: text("mode"),
            cost: note["cost"].as_f64().unwrap_or(0.0),
        })
    }

    fn to_value(&self) -> Value {
        json!({
            "chat": 1,
            "lasting": self.lasting,
            "answered": self.answered.iter().map(|(id, yes)| json!([id, yes])).collect::<Vec<_>>(),
            "queued": self.queued.iter().map(|(uuid, text)| json!({ "uuid": uuid, "text": text })).collect::<Vec<_>>(),
            "model": self.model,
            "effort": self.effort,
            "mode": self.mode,
            "cost": self.cost,
        })
    }
}

/// What a page notes of its Claude Code as it hears it.
#[derive(Default)]
pub struct Track {
    /// The lines heard of it, as Wings counts them.
    heard: u64,
    lasting: Vec<(String, Value)>,
    /// How many times `lasting` changed, so a note is sent again only when something in it did.
    changed: u32,
    /// The questions Claude asked in the turn that runs, by request.
    asked: Vec<String>,
    /// Your answers the page before had, for its questions as they are heard again.
    answers: HashMap<String, bool>,
    sent: Option<Sent>,
}

/// What the note last sent said, but its lasting lines, of which `changed` counts the changes.
#[derive(PartialEq)]
struct Sent {
    /// The session, which Wings is told with the note once Claude Code names it.
    session: Option<String>,
    answered: Vec<(String, bool)>,
    queued: Vec<String>,
    model: Option<String>,
    effort: Option<String>,
    mode: Option<String>,
    cost: u64,
    changed: u32,
}

impl Track {
    /// One line of Claude Code, after `session` took it in: the note to keep with it, when it changed, and with the number
    /// of lines heard once a turn has ended, when the transcript has all they said and Wings may let go of them.
    pub fn heard(&mut self, session: &mut Session, line: &Value) -> Option<(Value, Option<u64>)> {
        let ended = self.take(session, line);
        self.note(session, ended)
    }

    /// Counts the line and keeps what the note needs of it. Whether it ended a turn.
    fn take(&mut self, session: &mut Session, line: &Value) -> bool {
        self.heard += 1;
        if let Some(kind) = lasting(line) {
            let kind = kind.to_owned();
            self.lasting.retain(|(kept, _)| *kept != kind);
            self.lasting.push((kind, line.clone()));
            self.changed += 1;
        }
        if let Some(id) = asked(line) {
            if let (Some(yes), Some(ask)) = (self.answers.remove(id), ask_mut(session, id)) {
                ask.answer = Some(yes);
            }
            self.asked.push(id.to_owned());
        }
        let ended = line["type"] == "result" && !session.working;
        if ended {
            self.asked.clear();
            self.answers.clear();
        }
        ended
    }

    /// The note, when it differs from the one last sent or a turn has `ended`.
    pub fn note(&mut self, session: &Session, ended: bool) -> Option<(Value, Option<u64>)> {
        let mut answered = Vec::new();
        self.asked.retain(|id| match ask(session, id) {
            Some(ask) => {
                answered.extend(ask.answer.map(|yes| (id.clone(), yes)));
                true
            }
            None => false,
        });
        let sent = Sent {
            session: session.id.clone(),
            answered,
            queued: session.queued.iter().map(|queued| queued.uuid.clone()).collect(),
            model: session.model.clone(),
            effort: session.effort.clone(),
            mode: session.mode.clone(),
            cost: session.cost.to_bits(),
            changed: self.changed,
        };
        if !ended && self.sent.as_ref() == Some(&sent) {
            return None;
        }
        let note = Note {
            lasting: self.lasting.iter().map(|(_, line)| line.clone()).collect(),
            answered: sent.answered.clone(),
            queued: session.queued.iter().map(|queued| (queued.uuid.clone(), queued.text.clone())).collect(),
            model: sent.model.clone(),
            effort: sent.effort.clone(),
            mode: sent.mode.clone(),
            cost: session.cost,
        };
        self.sent = Some(sent);
        Some((note.to_value(), ended.then_some(self.heard)))
    }
}

/// Builds what the page before showed into `session`: from the transcript `entries`, the note the page before kept, if
/// it kept one, and the `lines` Wings kept of Claude Code, after a line saying it let go of some when `dropped`. The
/// track goes on from there.
pub fn rebuild(session: &mut Session, entries: &[Value], note: Option<&Note>, lines: &[Value], dropped: bool) -> Track {
    let mut track = Track::default();
    // The transcript up to the first message the lines hold. Without it there, or with lines let go of, the transcript's
    // whole, and of the lines only what it doesn't have.
    let first = lines.iter().find_map(said);
    let cut = match first {
        Some(uuid) => entries.iter().position(|entry| entry["uuid"] == uuid),
        None => Some(entries.len()),
    }
    .filter(|_| !dropped);
    let history = &entries[..cut.unwrap_or(entries.len())];
    session.read_back(entries);
    history.iter().for_each(|entry| session.hear(entry));
    let shown: HashSet<&str> = match cut {
        Some(_) => HashSet::new(),
        None => entries.iter().filter_map(|entry| entry["uuid"].as_str()).collect(),
    };
    if let Some(note) = note {
        note.lasting.iter().for_each(|line| session.hear(line));
        if note.model.is_some() {
            session.model.clone_from(&note.model);
        }
        session.effort.clone_from(&note.effort);
        if note.mode.is_some() {
            session.mode.clone_from(&note.mode);
        }
        session.cost = note.cost;
        let unread = note.queued.iter().filter(|(uuid, _)| !shown.contains(uuid.as_str()));
        session.queued = unread.map(|(uuid, text)| Queued { uuid: uuid.clone(), text: text.clone(), images: Vec::new() }).collect();
        track.lasting = note.lasting.iter().filter_map(|line| Some((lasting(line)?.to_owned(), line.clone()))).collect();
        track.answers = note.answered.iter().cloned().collect();
    }
    // Without a note, the lines go back to when Claude Code started: of those before the turn that runs, only the lasting.
    let turn = match (cut, note) {
        (None, None) => lines.iter().rposition(|line| line["type"] == "result").map_or(0, |at| at + 1),
        _ => 0,
    };
    for (at, line) in lines.iter().enumerate() {
        let new = at >= turn && said(line).is_none_or(|uuid| !shown.contains(uuid)) && (cut.is_some() || line["type"] != "stream_event");
        if new || lasting(line).is_some() {
            let heard = as_heard(session, line);
            session.hear(heard.as_ref().unwrap_or(line));
        }
        track.take(session, line);
    }
    // Your answers the page before had and kept no note of: a question the turn went on past was answered.
    if note.is_none() {
        for (at, line) in lines.iter().enumerate() {
            let Some(id) = asked(line) else {
                continue;
            };
            let went_on = lines[at + 1..].iter().any(|later| later["type"] == "user" || later["status"] == "requesting");
            if let Some(ask) = ask_mut(session, id).filter(|_| went_on) {
                ask.answer.get_or_insert(true);
            }
        }
    }
    // What the page before told you, and took from what it heard, it did then.
    session.alerts.clear();
    session.restored = None;
    session.cleared = false;
    // The page notes it all again, as it is now.
    track.sent = None;
    track
}

#[cfg(feature = "chat")]
mod page {
    //! The chat page's side: it takes its tab's Claude Code over as it opens, and notes what it hears of it.

    use std::cell::RefCell;

    use serde_json::Value;
    use wings_plugin::host::{self, Left, ProcessInput};

    use super::{Note, Track, as_heard, rebuild};
    use crate::{chat, images};

    /// The Claude Code the chat follows now.
    struct Followed {
        run: u32,
        input: ProcessInput,
        track: Track,
        /// While the lines Wings kept are still coming, after it was taken over.
        waiting: Option<Waiting>,
    }

    struct Waiting {
        /// How many Wings kept.
        count: u64,
        lines: Vec<Value>,
        dropped: bool,
        entries: Vec<Value>,
        note: Option<Note>,
    }

    thread_local! {
        static FOLLOWED: RefCell<Option<Followed>> = const { RefCell::new(None) };
    }

    /// The chat's Claude Code: `-p`, not one of the short runs of a setup dialog.
    fn chat_runs(left: &Left) -> bool {
        left.program.starts_with("claude") && left.args.iter().any(|arg| arg == "-p")
    }

    /// The chat started Claude Code `run` anew.
    pub fn started(run: u32, input: ProcessInput) {
        FOLLOWED.with(|followed| *followed.borrow_mut() = Some(Followed { run, input, track: Track::default(), waiting: None }));
    }

    /// Said in place of a Claude Code the tab can't start: another one runs its session.
    const TAKEN: &str = "Claude Code is already running this session elsewhere in Wings. Open it again here once it has finished.";

    /// Takes over the Claude Code this tab's page before ran, which Wings kept running through an update or restart of
    /// the plugin or of Wings. False when there is none, as when the tab opens after Wings started: the chat starts it
    /// then. True too when another Claude Code runs the session `resume`, which this tab mustn't start again.
    pub async fn take_over(resume: Option<String>) -> bool {
        // An older Wings doesn't know the call, or the session.
        let Ok(listed) = host::processes_of(resume.as_deref()).await else {
            return false;
        };
        let Some(here) = listed.iter().rev().find(|left| chat_runs(left)) else {
            if listed.iter().any(|left| left.elsewhere) {
                taken();
                return true;
            }
            return false;
        };
        // What Wings knows it runs, over what the tab last saved.
        let resume = here.session.clone().or(resume);
        let (run, cwd) = chat::with(|c| (chat::begin(c), c.cwd.clone()));
        chat::load_files(run, cwd.clone());
        chat::load_accounts(run, resume.as_deref(), cwd.clone());
        chat::load_git(run, cwd);
        // Read before the lines are counted, so that what it says is said before the lines Wings counts.
        let entries = match &resume {
            Some(id) => {
                let read = host::transcript_cut(id, &chat::HISTORY_KINDS, chat::HISTORY_ALL, chat::HISTORY_CUT).await;
                read.ok().and_then(|entries| entries.as_array().cloned()).unwrap_or_default()
            }
            None => Vec::new(),
        };
        // It may have ended meanwhile, and the chat starts it again.
        let Some(left) = host::processes().await.ok().and_then(|left| left.into_iter().rev().find(chat_runs)) else {
            return false;
        };
        chat::with(|c| c.session.id = resume);
        let process = host::adopt(&left.id, move |line| arrived(run, line));
        let waiting = Waiting { count: left.kept, lines: Vec::new(), dropped: false, entries, note: Note::read(&left.note) };
        FOLLOWED.with(|followed| *followed.borrow_mut() = Some(Followed { run, input: process.input(), track: Track::default(), waiting: Some(waiting) }));
        if left.kept == 0 {
            settle(run);
        }
        chat::follow(run, process).await;
        true
    }

    /// Another chat, or a Claude Code Wings kept from before it restarted that no tab took over, runs the tab's session.
    /// The tab says so and starts none: two on one session would both write to it. It keeps the session for when it opens
    /// again, and offers no terminal for it either.
    fn taken() {
        chat::with(|c| {
            chat::begin(c);
            c.state = chat::State::Failed(TAKEN.into());
        });
    }

    /// A line of the Claude Code taken over: one Wings kept, until they have all come, then each as it is printed.
    fn arrived(run: u32, line: &Value) {
        let dropped = line.get("wingsDropped").is_some();
        let complete = FOLLOWED.with(|followed| {
            let mut followed = followed.borrow_mut();
            let waiting = followed.as_mut().filter(|followed| followed.run == run)?.waiting.as_mut()?;
            if dropped {
                waiting.dropped = true;
            } else {
                waiting.lines.push(line.clone());
            }
            Some(waiting.lines.len() as u64 >= waiting.count)
        });
        match complete {
            Some(true) => settle(run),
            Some(false) => {}
            None if dropped => {}
            None => {
                let heard = chat::with(|c| as_heard(&c.session, line));
                chat::hear(run, heard.as_ref().unwrap_or(line));
            }
        }
    }

    /// Builds what the page before showed, once the lines Wings kept have all come.
    fn settle(run: u32) {
        let waiting = FOLLOWED.with(|followed| followed.borrow_mut().as_mut().filter(|followed| followed.run == run)?.waiting.take());
        let Some(waiting) = waiting else {
            return;
        };
        let track = chat::with(|c| {
            c.session.now = host::now() as u64;
            let track = rebuild(&mut c.session, &waiting.entries, waiting.note.as_ref(), &waiting.lines, waiting.dropped);
            c.doing = (c.session.activity(), host::now() as u64);
            track
        });
        FOLLOWED.with(|followed| {
            if let Some(followed) = followed.borrow_mut().as_mut().filter(|followed| followed.run == run) {
                followed.track = track;
            }
        });
        images::flush();
        chat::report_agent();
        sync();
    }

    /// One line the chat heard of its Claude Code `run`, after the session took it in.
    pub fn heard(run: u32, line: &Value) {
        let note = FOLLOWED.with(|followed| {
            let mut followed = followed.borrow_mut();
            let followed = followed.as_mut().filter(|followed| followed.run == run && followed.waiting.is_none())?;
            chat::with(|c| followed.track.heard(&mut c.session, line))
        });
        send(note);
    }

    /// Notes what changed by your doing, like an answer or a queued message.
    pub fn sync() {
        let note = FOLLOWED.with(|followed| {
            let mut followed = followed.borrow_mut();
            let followed = followed.as_mut().filter(|followed| followed.waiting.is_none())?;
            let run = followed.run;
            chat::with(|c| if c.run == run { followed.track.note(&c.session, false) } else { None })
        });
        send(note);
    }

    fn send(note: Option<(Value, Option<u64>)>) {
        let Some((note, heard)) = note else {
            return;
        };
        let session = chat::with(|c| c.session.id.clone());
        FOLLOWED.with(|followed| {
            if let Some(followed) = followed.borrow().as_ref() {
                followed.input.note_session(&note, heard, session.as_deref());
            }
        });
    }

    #[cfg(test)]
    mod tests {
        use std::{cell::RefCell, rc::Rc};

        use serde_json::{Value, json};
        use wings_plugin::{host, task};

        use super::take_over;
        use crate::chat;

        const SESSION: &str = "11111111-2222-4333-8444-555555555555";

        type Calls = Rc<RefCell<Vec<(String, Value)>>>;

        /// Wings, as far as the chat asks it: `processes` lists `listed`, every other call is answered with null, and what
        /// was called is kept.
        fn wings(listed: Value) -> Calls {
            let calls = Calls::default();
            let kept = calls.clone();
            host::testing::answer(move |method, params| {
                kept.borrow_mut().push((method.to_owned(), params.clone()));
                Ok(if method == "processes" { listed.clone() } else { Value::Null })
            });
            calls
        }

        fn called(calls: &Calls, method: &str) -> Vec<Value> {
            calls.borrow().iter().filter(|(name, _)| name == method).map(|(_, params)| params.clone()).collect()
        }

        #[test]
        fn a_chat_takes_over_the_claude_code_wings_kept_for_its_session_and_starts_none() {
            let resume = format!("--resume={SESSION}");
            let calls = wings(json!([{ "id": "left-0", "token": "process-1", "program": "claude", "args": ["-p", resume], "cwd": "/p", "note": null, "kept": 0, "session": SESSION }]));
            assert!(task::block_on(take_over(Some(SESSION.into()))));
            assert_eq!(called(&calls, "processes")[0], json!({ "session": SESSION }), "it asks whether its session runs");
            assert_eq!(called(&calls, "adopt").len(), 1);
            assert!(called(&calls, "spawn").is_empty());
            assert_eq!(chat::with(|c| c.session.id.clone()).as_deref(), Some(SESSION));
            // The header's account button needs the accounts, which a chat that took over still asks for.
            task::block_on(async {});
            assert_eq!(called(&calls, "claudeAccountChoices").len(), 1, "{:?}", calls.borrow().iter().map(|(name, _)| name.clone()).collect::<Vec<_>>());
        }

        #[test]
        fn a_chat_starts_no_claude_code_on_a_session_another_one_runs() {
            let calls = wings(json!([{ "elsewhere": true, "session": SESSION }]));
            assert!(task::block_on(take_over(Some(SESSION.into()))), "nothing for the chat to start");
            assert!(called(&calls, "adopt").is_empty() && called(&calls, "spawn").is_empty());
            assert!(chat::with(|c| matches!(&c.state, chat::State::Failed(why) if why.contains("already running this session"))));
        }

        #[test]
        fn a_chat_runs_its_claude_code_in_the_terminal_server() {
            let calls = wings(json!([]));
            chat::launch_with(Some(SESSION.into()), crate::fork::Launch::default());
            task::block_on(async {});
            let spawned = called(&calls, "spawn");
            assert_eq!(spawned.len(), 1);
            assert_eq!(spawned[0]["survive"], true);
            assert!(spawned[0]["args"].as_array().unwrap().contains(&json!(format!("--resume={SESSION}"))));
            // Remote Control is on in every new chat unless you turned the setting off.
            assert!(spawned[0]["input"].as_str().unwrap().contains(r#""enabled":true,"subtype":"remote_control""#), "{}", spawned[0]["input"]);
            assert_eq!(chat::with(|c| c.session.remote.clone()), crate::remote::Remote::Connecting);
        }
    }
}

#[cfg(feature = "chat")]
pub use page::{heard, started, sync, take_over};

#[cfg(test)]
mod tests {
    use super::*;

    /// A turn as Claude Code 2.1.327 printed it.
    const TURN: &str = include_str!("../tests/turn.jsonl");

    fn lines(text: &str) -> Vec<Value> {
        text.lines().filter(|line| !line.trim().is_empty()).map(|line| serde_json::from_str(line).unwrap()).collect()
    }

    fn first(written: &str) -> Value {
        serde_json::from_str(written.lines().next().unwrap()).unwrap()
    }

    /// Claude Code's echo of a message you sent, as `--replay-user-messages` prints it.
    fn echo(sent: &Value) -> Value {
        json!({ "type": "user", "isReplay": true, "uuid": sent["uuid"], "message": sent["message"], "parent_tool_use_id": null })
    }

    /// The transcript entry Claude Code writes for a message it printed: the same message under the same uuid.
    fn entry(line: &Value) -> Value {
        json!({ "type": line["type"], "uuid": line["uuid"], "message": line["message"] })
    }

    fn ask(id: &str) -> Value {
        json!({ "type": "control_request", "request_id": id, "request": { "subtype": "can_use_tool", "tool_name": "Bash", "input": { "command": "du -sh ." } } })
    }

    fn text_delta(text: &str) -> Value {
        json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": text } }, "parent_tool_use_id": null })
    }

    /// The page before: a turn that ended, then one that runs, where you allowed a command, Claude asks for another, and
    /// you queued a message. What it heard of Claude Code, the transcript Claude Code wrote, the note it kept, and the
    /// lines Wings kept past it.
    struct Before {
        session: Session,
        entries: Vec<Value>,
        note: Value,
        kept: Vec<Value>,
    }

    /// The page before as it hears Claude Code: what it printed, the note last kept, and how many lines Wings let go of.
    struct Page {
        session: Session,
        track: Track,
        printed: Vec<Value>,
        note: Value,
        forgot: u64,
    }

    impl Page {
        fn hear(&mut self, line: Value) {
            self.session.hear(&line);
            if let Some((note, heard)) = self.track.heard(&mut self.session, &line) {
                self.note = note;
                self.forgot = heard.unwrap_or(self.forgot);
            }
            self.printed.push(line);
        }

        /// What the chat notes after a write of yours.
        fn sync(&mut self) {
            if let Some((note, _)) = self.track.note(&self.session, false) {
                self.note = note;
            }
        }
    }

    fn requesting() -> Value {
        json!({ "type": "system", "subtype": "status", "status": "requesting" })
    }

    fn before() -> Before {
        let mut page = Page { session: Session::default(), track: Track::default(), printed: Vec::new(), note: Value::Null, forgot: 0 };
        let mut entries = Vec::new();
        page.hear(json!({ "type": "control_response", "response": { "subtype": "success", "request_id": session::INITIALIZE, "response": {
            "models": [{ "value": "haiku", "displayName": "Haiku", "resolvedModel": "claude-haiku-5-5" }], "commands": [{ "name": "review", "description": "Review" }] } } }));
        let one = first(&page.session.send("How many files?", &[], 1, 1));
        entries.push(entry(&one));
        page.hear(echo(&one));
        for line in lines(TURN) {
            if said(&line).is_some() {
                entries.push(entry(&line));
            }
            page.hear(line);
        }
        let two = first(&page.session.send("And their sizes?", &[], 2, 2));
        entries.push(entry(&two));
        page.hear(echo(&two));
        page.hear(requesting());
        page.hear(ask("r1"));
        page.session.allow("r1", false).unwrap();
        page.sync();
        page.hear(requesting());
        page.hear(text_delta("Checking the sizes"));
        page.hear(ask("r2"));
        page.session.queue("and the dates", Vec::new(), 3, 3);
        page.sync();
        let kept = page.printed[page.forgot as usize..].to_vec();
        Before { session: page.session, entries, note: page.note, kept }
    }

    #[test]
    fn a_page_that_takes_claude_code_over_shows_what_the_page_before_did_and_hears_on() {
        let before = before();
        assert!(before.session.working);
        let note = Note::read(&before.note).unwrap();
        // Wings let go of the turn that ended.
        assert!(before.kept.iter().all(|line| line["type"] != "result"));
        let mut after = Session::default();
        let mut track = rebuild(&mut after, &before.entries, Some(&note), &before.kept, false);
        assert_eq!(after.items, before.session.items);
        assert_eq!(after.live, "Checking the sizes");
        assert!(after.working);
        assert_eq!(after.queued, [Queued { uuid: before.session.queued[0].uuid.clone(), text: "and the dates".into(), images: Vec::new() }]);
        assert_eq!(after.commands.len(), 1, "Claude Code's answer to the opening, kept in the note");
        assert_eq!((after.model.as_deref(), after.cost), (before.session.model.as_deref(), before.session.cost));
        // It goes on with the same Claude Code: the queued message is read, Claude answers, and the turn ends.
        let queued = json!({ "type": "user", "isReplay": true, "uuid": after.queued[0].uuid, "parent_tool_use_id": null,
            "message": { "role": "user", "content": [{ "type": "text", "text": "and the dates" }] } });
        let answer = json!({ "type": "assistant", "uuid": "a-sizes", "parent_tool_use_id": null, "message": { "role": "assistant", "content": [{ "type": "text", "text": "4 KB each." }] } });
        let end = json!({ "type": "result", "subtype": "success", "is_error": false, "total_cost_usd": 0.01 });
        let mut forgot = None;
        for line in [queued, answer, end] {
            after.hear(&line);
            forgot = track.heard(&mut after, &line).and_then(|(_, heard)| heard).or(forgot);
        }
        assert!(!after.working && after.queued.is_empty());
        assert_eq!(after.items.iter().filter(|item| matches!(item, Item::User(_))).count(), 3);
        assert_eq!(after.items.last(), Some(&Item::Text("4 KB each.".into())));
        assert_eq!(forgot, Some(before.kept.len() as u64 + 3), "Wings may let go of all it kept and heard since");
    }

    #[test]
    fn the_questions_you_answered_stay_answered_and_the_one_that_waits_still_waits() {
        let before = before();
        let mut after = Session::default();
        rebuild(&mut after, &before.entries, Note::read(&before.note).as_ref(), &before.kept, false);
        let answers: Vec<_> = after.items.iter().filter_map(|item| match item { Item::Ask(ask) => Some((ask.request.as_str(), ask.answer)), _ => None }).collect();
        assert_eq!(answers, [("r1", Some(true)), ("r2", None)]);
        assert!(after.alerts.is_empty(), "the page before told you already");
    }

    #[test]
    fn without_a_note_the_lines_since_claude_code_started_show_the_same() {
        let before = before();
        // A page of an older build keeps no note, so Wings keeps every line.
        let mut printed = vec![];
        let mut session = Session::default();
        let one = first(&session.send("How many files?", &[], 1, 1));
        printed.push(echo(&one));
        printed.extend(lines(TURN));
        let two = first(&session.send("And their sizes?", &[], 2, 2));
        printed.extend([echo(&two), requesting(), ask("r1"), requesting(), ask("r2")]);
        let mut after = Session::default();
        rebuild(&mut after, &before.entries, None, &printed, false);
        let users = after.items.iter().filter(|item| matches!(item, Item::User(_))).count();
        assert_eq!(users, 2, "each message once");
        let answers: Vec<_> = after.items.iter().filter_map(|item| match item { Item::Ask(ask) => Some((ask.request.as_str(), ask.answer)), _ => None }).collect();
        assert_eq!(answers, [("r1", Some(true)), ("r2", None)], "a question the turn went on past was answered");
        assert!(after.working);
    }

    #[test]
    fn with_lines_let_go_of_the_transcript_says_what_was_said_and_nothing_shows_twice() {
        let before = before();
        let mut after = Session::default();
        rebuild(&mut after, &before.entries, Note::read(&before.note).as_ref(), &before.kept, true);
        let texts = after.items.iter().filter(|item| matches!(item, Item::Text(_))).count();
        let users = after.items.iter().filter(|item| matches!(item, Item::User(_))).count();
        assert_eq!((users, texts), (2, 1));
        assert!(after.working);
    }

    #[test]
    fn a_note_is_sent_when_something_in_it_changed_and_at_the_end_of_a_turn() {
        let mut session = Session::default();
        let mut track = Track::default();
        let status = requesting();
        session.hear(&status);
        assert!(track.heard(&mut session, &status).is_some(), "the first note");
        session.hear(&text_delta("Hi"));
        assert_eq!(track.heard(&mut session, &text_delta("Hi")), None, "nothing in it changed");
        let end = json!({ "type": "result", "subtype": "success", "is_error": false });
        session.hear(&end);
        let (note, heard) = track.heard(&mut session, &end).unwrap();
        assert_eq!(heard, Some(3));
        assert_eq!(Note::read(&note).unwrap(), Note::default());
        assert_eq!(Note::read(&json!({ "token": "x" })), None, "another page's note");
    }
}
