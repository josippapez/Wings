use std::sync::atomic::AtomicUsize;

use serde_json::json;

use super::*;

/// Words of one meaning land on one dimension, as a model's would put them close.
fn concept(word: &str) -> &str {
    match word {
        "scrollback" | "history" => "history",
        "relaunching" | "relaunch" | "restart" | "restarted" => "restart",
        "lost" | "disappears" | "gone" | "lose" => "lost",
        word => word,
    }
}

fn fnv(text: &str) -> usize {
    text.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3)) as usize
}

/// A stand-in for the model: a bag of concepts.
pub(crate) struct Fake(pub Arc<AtomicUsize>);

impl Embed for Fake {
    fn embed(&mut self, text: &str, _query: bool) -> Result<(f32, [i8; DIM]), String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        let mut v = [0f32; DIM];
        for word in text.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2) {
            v[fnv(concept(&word.to_lowercase())) % DIM] += 1.0;
        }
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        Ok(index::quantize(&v.map(|x| x / norm)))
    }
}

/// An index in `dir` embedding with `Fake`, and the count of its calls.
pub(crate) fn fake(dir: &Path, model: u64) -> (Arc<Semantic>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let make: MakeEmbedder = Box::new(move |_| Ok(Box::new(Fake(counted.clone())) as Box<dyn Embed>));
    (Semantic::with(dir.to_owned(), model, make), calls)
}

pub(crate) fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wings-semantic-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub(crate) fn user(text: &str) -> String {
    json!({ "type": "user", "cwd": "/p", "timestamp": "2026-10-01T10:00:00.000Z", "message": { "role": "user", "content": text } }).to_string()
}

pub(crate) fn reply(text: &str) -> String {
    json!({ "type": "assistant", "timestamp": "2026-10-01T10:00:05.000Z", "message": { "model": "claude-opus-5-5", "role": "assistant", "content": [{ "type": "text", "text": text }] } }).to_string()
}

fn subagent_line(kind: &str, text: &str) -> String {
    let mut line: serde_json::Value = serde_json::from_str(&if kind == "user" { user(text) } else { reply(text) }).unwrap();
    line["isSidechain"] = json!(true);
    line.to_string()
}

/// Writes a transcript of `lines` for session `id` in the Claude folder `claude`.
pub(crate) fn write(claude: &Path, id: &str, lines: &[String]) -> PathBuf {
    let folder = claude.join("projects/-p");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(format!("{id}.jsonl"));
    fs::write(&path, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
    path
}

fn write_subagent(claude: &Path, parent: &str, lines: &[String]) -> PathBuf {
    let folder = claude.join("projects/-p").join(parent).join("subagents");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join("agent-1.jsonl");
    fs::write(&path, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
    path
}

fn sessions(claude: &Path) {
    write(claude, "a", &[user("the pane history is lost after a restart"), reply("I will keep the scrollback on disk")]);
    write_subagent(claude, "a", &[subagent_line("user", "find where panes save their state"), subagent_line("assistant", "panes save state in saved.rs")]);
    write(claude, "b", &[user("make the sidebar text size bigger please"), reply("done, the sidebar uses the setting now")]);
}

#[test]
fn a_round_indexes_every_session_with_its_subagents_toward_the_session() {
    let (claude, dir) = (temp("round-claude"), temp("round-dir"));
    sessions(&claude);
    let (semantic, calls) = fake(&dir, 1);
    semantic.round(std::slice::from_ref(&claude), &|| {});
    assert_eq!(semantic.progress().counts(), (2, 2), "sessions, not transcripts");
    assert_eq!(semantic.progress().state(), State::Ready);
    let index = semantic.index.lock().unwrap().clone();
    assert_eq!(index.transcripts.len(), 3);
    assert_eq!(index.sessions(), 2);
    let subagent = index.transcripts.iter().find(|t| is_subagent(&t.path)).unwrap();
    assert_eq!(subagent.session, "a", "a subagent counts toward the session that ran it");
    assert_eq!(calls.load(Ordering::Relaxed), 6, "each message once");
    assert_eq!(Index::decode(&fs::read(dir.join(INDEX)).unwrap(), index.header), index, "saved");

    semantic.round(std::slice::from_ref(&claude), &|| {});
    assert_eq!(calls.load(Ordering::Relaxed), 6, "nothing changed, nothing embedded");
}

#[test]
fn a_grown_transcript_goes_on_from_its_last_message_and_a_removed_one_is_dropped() {
    let (claude, dir) = (temp("grow-claude"), temp("grow-dir"));
    sessions(&claude);
    let (semantic, calls) = fake(&dir, 1);
    semantic.round(std::slice::from_ref(&claude), &|| {});
    let before = calls.load(Ordering::Relaxed);
    let b = write(
        &claude,
        "b",
        &[user("make the sidebar text size bigger please"), reply("done, the sidebar uses the setting now"), user("and the tab titles too please")],
    );
    semantic.round(std::slice::from_ref(&claude), &|| {});
    assert_eq!(calls.load(Ordering::Relaxed) - before, 2, "the last message again, and the new one");
    let rows = |semantic: &Semantic| {
        let index = semantic.index.lock().unwrap();
        let at = index.transcripts.iter().position(|t| t.path == b).map(|at| at as u32);
        index.rows.iter().filter(|r| Some(r.transcript) == at).map(|r| r.message).collect::<Vec<_>>()
    };
    assert_eq!(rows(&semantic), [0, 1, 2]);

    fs::remove_file(&b).unwrap();
    semantic.round(std::slice::from_ref(&claude), &|| {});
    assert!(rows(&semantic).is_empty());
    assert_eq!(semantic.index.lock().unwrap().sessions(), 1);
}

#[test]
fn another_model_builds_the_index_again() {
    let (claude, dir) = (temp("model-claude"), temp("model-dir"));
    sessions(&claude);
    let (semantic, _) = fake(&dir, 1);
    semantic.round(std::slice::from_ref(&claude), &|| {});
    let (again, calls) = fake(&dir, 2);
    again.round(std::slice::from_ref(&claude), &|| {});
    assert_eq!(calls.load(Ordering::Relaxed), 6, "the old model's vectors are dropped and all embedded again");
    let (same, calls) = fake(&dir, 2);
    same.round(std::slice::from_ref(&claude), &|| {});
    assert_eq!(calls.load(Ordering::Relaxed), 0, "the same model's index is read back and kept");
}

#[test]
fn a_transcript_still_being_written_waits_until_it_settles() {
    let (claude, dir) = (temp("settle-claude"), temp("settle-dir"));
    sessions(&claude);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let make: MakeEmbedder = Box::new(move |_| Ok(Box::new(Fake(counted.clone())) as Box<dyn Embed>));
    let semantic = Semantic::build(dir, 1, make, QUIET);
    semantic.round(std::slice::from_ref(&claude), &|| {});
    assert_eq!(calls.load(Ordering::Relaxed), 0, "just written: nothing embedded yet");
    let settled = std::time::SystemTime::now() - QUIET - Duration::from_secs(1);
    for file in on_disk(std::slice::from_ref(&claude)) {
        fs::File::options().append(true).open(&file.path).unwrap().set_modified(settled).unwrap();
    }
    semantic.round(std::slice::from_ref(&claude), &|| {});
    assert_eq!(calls.load(Ordering::Relaxed), 6, "settled: embedded");
}

#[test]
fn a_key_pressed_pauses_the_indexing_until_a_few_quiet_seconds() {
    let (claude, dir) = (temp("typing-claude"), temp("typing-dir"));
    sessions(&claude);
    let (semantic, calls) = fake(&dir, 1);
    semantic.typed();
    let started = Instant::now();
    let running = {
        let (semantic, claude) = (semantic.clone(), claude.clone());
        std::thread::spawn(move || semantic.round(&[claude], &|| {}))
    };
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(calls.load(Ordering::Relaxed), 0, "nothing embedded while typing");
    running.join().unwrap();
    assert!(started.elapsed() >= TYPING && calls.load(Ordering::Relaxed) == 6);
}

#[test]
fn turned_off_it_ranks_nothing_and_indexes_nothing() {
    let (claude, dir) = (temp("off-claude"), temp("off-dir"));
    sessions(&claude);
    let (semantic, calls) = fake(&dir, 1);
    semantic.round(std::slice::from_ref(&claude), &|| {});
    assert_eq!(semantic.rank("scrollback relaunching", false).unwrap()[0].0, "a");
    semantic.set_enabled(false);
    assert_eq!(semantic.rank("scrollback relaunching", false), None);
    write(&claude, "c", &[user("a new session about something else entirely")]);
    let before = calls.load(Ordering::Relaxed);
    semantic.round(std::slice::from_ref(&claude), &|| {});
    assert_eq!((calls.load(Ordering::Relaxed), semantic.progress().state()), (before, State::Off));
    let (reopened, _) = fake(&dir, 1);
    assert!(!reopened.enabled(), "the choice is kept");
}

#[test]
fn progress_shows_only_for_a_round_big_enough() {
    let progress = Progress::default();
    progress.set(State::Indexing, 3, SHOW_AT - 1);
    assert_eq!(progress.shown(), None, "a catch-up");
    progress.set(State::Indexing, 412, 929);
    assert!((progress.shown().unwrap() - 412.0 / 929.0).abs() < 1e-6);
    progress.set(State::Ready, 929, 929);
    assert_eq!(progress.shown(), None, "gone when done");
}

#[test]
fn fusion_puts_what_both_lists_rank_high_first() {
    assert_eq!(fuse(&[&["a", "b", "c"], &["c", "d"]]), ["c", "a", "b", "d"]);
    assert_eq!(fuse(&[&["a", "b"], &[]]), ["a", "b"], "one list keeps its order");
    assert_eq!(fuse(&[&[], &["x", "y"]]), ["x", "y"]);
}

#[test]
fn a_passage_is_read_back_only_while_the_message_holds_it() {
    let best = Best { transcript: 0, message: 0, start: 4, len: 7, score: 1.0 };
    assert_eq!(passage("the pane history", &best).as_deref(), Some("…pane hi…"));
    assert_eq!(passage("the", &best), None, "the message is shorter now");
    assert_eq!(passage("abč", &Best { start: 3, len: 1, ..best }), None, "not at a character's edge");
}

#[test]
fn a_search_does_not_wait_for_the_helper_to_start_and_the_tool_does() {
    let (claude, dir) = (temp("warm-claude"), temp("warm-dir"));
    sessions(&claude);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let make: MakeEmbedder = Box::new(move |_| {
        // A helper takes a while to start.
        std::thread::sleep(Duration::from_millis(200));
        Ok(Box::new(Fake(counted.clone())) as Box<dyn Embed>)
    });
    let semantic = Semantic::with(dir, 1, make);
    let dirs = [claude.clone()];
    semantic.round(&dirs, &|| {});
    // Ended after a minute with nothing to do.
    *semantic.embedder.lock().unwrap() = None;
    let history = crate::history::History::default();
    history.set_semantic(semantic.clone());
    let filter = crate::history::Filter::default();
    let first = history.search(&dirs, "scrollback relaunching", &filter, 10);
    assert!(first.sessions.is_empty() && first.semantic_loading, "by words alone for now, and it says to ask again");
    while semantic.warming() {
        std::thread::sleep(Duration::from_millis(10));
    }
    let next = history.search(&dirs, "scrollback relaunching", &filter, 10);
    assert_eq!((next.sessions[0].id.as_str(), next.semantic_loading), ("a", false));

    *semantic.embedder.lock().unwrap() = None;
    let tool = crate::history::call_tool(&history, &dirs, "search_history", &json!({ "query": "scrollback relaunching" }))
        .unwrap()
        .unwrap();
    let found: serde_json::Value = serde_json::from_str(&tool).unwrap();
    assert_eq!(found["sessions"][0]["sessionId"], "a", "the tool can't ask again, so it waits");
}

/// The real helper and model, end to end, on made-up sessions: a round indexes them, shows its progress, and a query in
/// other words finds the right one. By hand, with `WINGS_EMBED` set to a built `wings-embed`: it copies repo-docs' model
/// when this Mac has it, and downloads it otherwise.
#[test]
#[ignore]
fn the_real_model_finds_a_session_worded_differently() {
    let helper = PathBuf::from(std::env::var_os("WINGS_EMBED").expect("WINGS_EMBED isn't set"));
    let (claude, dir) = (temp("real-claude"), temp("real-dir"));
    let topics = [
        ("history", "the pane history is lost after a restart of the app", "I keep the scrollback on disk now"),
        ("sidebar", "make the sidebar text size bigger please", "the sidebar follows the text size setting"),
        ("pr", "review the open pull requests and leave inline comments", "left three comments on the PR"),
        ("audio", "does the TV play TrueHD inside mp4 files", "the demuxer has no TrueHD support in mp4"),
        ("memory", "is the local model using a lot of RAM while idle", "it unloads after ten idle minutes"),
        ("ci", "show the pipeline logs and steps for a run", "added a run view with jobs and steps"),
    ];
    for (id, prompt, answer) in topics.iter().chain(topics.iter()).enumerate().map(|(i, t)| (format!("{}-{i}", t.0), t.1, t.2)) {
        write(&claude, &id, &[user(prompt), reply(answer)]);
    }
    let semantic = Semantic::open(dir.clone(), helper);
    let shown = Arc::new(Mutex::new(Vec::new()));
    let seen = shown.clone();
    let progress = semantic.progress();
    let started = Instant::now();
    semantic.round(std::slice::from_ref(&claude), &move || seen.lock().unwrap().push((progress.state(), progress.shown().is_some())));
    println!("[measure] 12 sessions in {:.1} s with the real model", started.elapsed().as_secs_f64());
    assert!(shown.lock().unwrap().contains(&(State::Indexing, true)), "a round of 12 shows its progress: {:?}", shown.lock().unwrap());
    assert_eq!((semantic.progress().state(), semantic.progress().counts()), (State::Ready, (12, 12)));
    assert!(dir.join("model").join(model::MODEL.name).exists() && dir.join(INDEX).exists());
    let history = crate::history::History::default();
    history.set_semantic(semantic.clone());
    let filter = crate::history::Filter::default();
    let found = history.search(std::slice::from_ref(&claude), "scrollback disappears when I relaunch", &filter, 3);
    assert!(found.sessions[0].id.starts_with("history-"), "{:?}", found.sessions.iter().map(|h| &h.id).collect::<Vec<_>>());
    let found = history.search(std::slice::from_ref(&claude), "Dolby lossless audio in an mp4 container", &filter, 3);
    assert!(found.sessions[0].id.starts_with("audio-"), "{:?}", found.sessions.iter().map(|h| &h.id).collect::<Vec<_>>());
    // The model's copy is 34 MB.
    drop((history, semantic));
    fs::remove_dir_all(dir).unwrap();
    fs::remove_dir_all(claude).unwrap();
}

/// Indexes this Mac's own sessions with the real helper, then runs the queries in a file of JSON lines (`{"q", "target"}`)
/// through the history's search. By hand only, as it reads every transcript: set `WINGS_EMBED` (the helper),
/// `WINGS_SEMANTIC_DIR` (a scratch folder) and `WINGS_QUERIES`, and run the ignored tests named `this_mac`.
#[test]
#[ignore]
fn this_macs_sessions_indexed_and_searched() {
    let var = |name: &str| PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} isn't set")));
    let home = dirs::home_dir().unwrap();
    let mut claude: Vec<PathBuf> = fs::read_dir(&home)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".claude"))
        .map(|e| e.path())
        .filter(|p| p.join("projects").is_dir())
        .collect();
    claude.sort();
    let semantic = Semantic::open(var("WINGS_SEMANTIC_DIR"), var("WINGS_EMBED"));
    let started = Instant::now();
    semantic.round(&claude, &|| {});
    let status = semantic.status();
    let (rows, transcripts) = {
        let index = semantic.index.lock().unwrap();
        (index.rows.len(), index.transcripts.len())
    };
    println!(
        "[measure] {} sessions, {transcripts} transcripts, {rows} chunks in {:.0} s; index {:.1} MB; error {:?}",
        status.sessions,
        started.elapsed().as_secs_f64(),
        status.bytes as f64 / 1e6,
        status.error
    );
    let history = crate::history::History::default();
    history.set_semantic(semantic.clone());
    let filter = crate::history::Filter::default();
    history.search(&claude, "warm up", &filter, 1);
    for line in fs::read_to_string(var("WINGS_QUERIES")).unwrap().lines() {
        let query: serde_json::Value = serde_json::from_str(line).unwrap();
        let (q, target) = (query["q"].as_str().unwrap(), query["target"].as_str().unwrap());
        let started = Instant::now();
        let found = history.search(&claude, q, &filter, 5);
        let took = started.elapsed();
        let words = crate::history::History::default().search(&claude, q, &filter, 5).total;
        let rank = found.sessions.iter().position(|h| h.id.starts_with(target)).map_or("not in top 5".into(), |r| format!("#{}", r + 1));
        println!("\nQ: {q}\n   target {target}: {rank}; by words alone {words} sessions; search {:.0} ms", took.as_secs_f64() * 1000.0);
        for hit in &found.sessions {
            let title = hit.title.clone().or(hit.first_prompt.clone()).unwrap_or_default();
            let snippet = hit.snippets.first().map(|s| s.text.chars().take(110).collect::<String>()).unwrap_or_default();
            println!("   {} {:<50.50} | {snippet}", &hit.id[..8], title.replace('\n', " "));
        }
    }
}
