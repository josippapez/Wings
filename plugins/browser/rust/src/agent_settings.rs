//! The Agent tools section of the Browser's settings: the folders agents may use for files (kept by Wings in its own data
//! (`webFolders`), never by this plugin, which only lists, adds through the system's picker and removes) and how long an
//! agent's tab can sit unused before it is closed.

use std::cell::RefCell;
use wings_plugin::{
    host,
    task::spawn,
    ui::{Entry, Node, button, chips, entry, error_state, icon_button, icons, muted, rows_card, section, setting, spinner, text, toast},
};

use crate::settings;

#[derive(Default)]
struct State {
    /// The folders Wings lists, as full paths.
    folders: Option<Result<Vec<String>, String>>,
    adding: bool,
    /// Why the last add or remove failed, as a toast.
    problem: Option<String>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(change: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| change(&mut state.borrow_mut()))
}

fn parse(answer: &serde_json::Value) -> Vec<String> {
    answer.as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(|path| path.as_str().map(str::to_owned)).collect()
}

pub fn load() {
    spawn(async {
        let folders = host::web_folders().await.map(|answer| parse(&answer));
        with(|state| state.folders = Some(folders));
    });
}

/// Opens the system's folder picker, which the host does only right after the click this runs in. Null is a cancelled
/// picker.
fn add() {
    if with(|state| std::mem::replace(&mut state.adding, true)) {
        return;
    }
    with(|state| state.problem = None);
    spawn(async {
        let answer = host::web_folders_add().await;
        with(|state| {
            state.adding = false;
            match answer {
                Ok(list) if list.is_null() => {}
                Ok(list) => state.folders = Some(Ok(parse(&list))),
                Err(error) => state.problem = Some(error),
            }
        });
    });
}

fn remove(path: String) {
    with(|state| state.problem = None);
    spawn(async move {
        let answer = host::web_folders_remove(&path).await;
        with(|state| match answer {
            Ok(list) => state.folders = Some(Ok(parse(&list))),
            Err(error) => state.problem = Some(error),
        });
    });
}

fn dismiss() {
    with(|state| state.problem = None);
}

/// The last part of a path: what a folder is called in the list.
fn name_of(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or(path)
}

const IDLE: [(&str, &str); 4] = [("10", "10 min"), ("30", "30 min"), ("60", "60 min"), ("0", "Never")];

/// The section: how long agents' tabs last, and the folders for their files.
pub fn view() -> Vec<Node> {
    let minutes = settings::idle_minutes().to_string();
    let idle = chips("Close agents' unused tabs after", &IDLE, &minutes, |value| {
        if let Ok(minutes) = value.parse() {
            drop(settings::idle().set(minutes));
        }
    })
    .segmented();
    let tabs = setting("Close agents' unused tabs after", "A tab an agent opened and nobody has used for this long closes.", idle);
    let rows: Vec<Node> = with(|state| match &state.folders {
        None => vec![entry(Entry { lead: Some(spinner()), title: muted("Loading folders"), detail: None, trailing: None })],
        Some(Err(_)) => Vec::new(),
        Some(Ok(folders)) => folders
            .iter()
            .map(|path| {
                let (name, removing) = (name_of(path), path.clone());
                entry(Entry {
                    lead: Some(wings_plugin::ui::icon(24, 2.0, &icons::FOLDER_OPEN).prop("size", 16)),
                    title: text(name).prop("weight", "medium"),
                    detail: Some(muted(path.as_str()).prop("size", "small").prop("truncate", true).prop("title", path.as_str())),
                    trailing: Some(icon_button(&format!("Remove {name}"), &icons::TRASH, move || remove(removing.clone()))),
                })
                .key(path.as_str())
            })
            .collect(),
    });
    let listed = with(|state| matches!(&state.folders, Some(Ok(folders)) if !folders.is_empty()));
    let loaded = with(|state| matches!(&state.folders, Some(Ok(_))));
    let mut files = vec![tabs];
    files.extend(rows);
    if loaded && !listed {
        files.push(entry(Entry { lead: None, title: muted("No folders added"), detail: None, trailing: None }));
    }
    let adding = with(|state| state.adding);
    files.push(setting("Folders for agents' files", "Agents can upload files from these and save screenshots and recordings in them.", button("Add folder…", "outline", adding, add).size("sm")));
    let failed = with(|state| match &state.folders {
        Some(Err(error)) => Some(error_state("Couldn't read the folders", error, Some(("Try again", Box::new(load))))),
        _ => None,
    });
    // Without the list, the idle time is still there to set.
    let card = match failed {
        Some(error) => vec![rows_card(files.drain(..1)), error],
        None => vec![rows_card(files)],
    };
    vec![section("Agent tools", Some("Temporary folders and the Browser's own folders are always allowed for agents' files."), card)]
}

/// What went wrong with the last add or remove, floating over the page.
pub fn problem() -> Option<Node> {
    with(|state| state.problem.clone()).map(|problem| toast("danger", problem, dismiss).key("folders-problem"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::rc::Rc;
    use wings_plugin::host::testing;

    fn drawn() -> String {
        wings_plugin::ui::export(view()).0.to_string()
    }

    /// Sends the handler named by the first `prop` in the page as it is now the event.
    fn fire(prop: &str, event: Value) -> bool {
        let (tree, handlers) = wings_plugin::ui::export(view());
        let tree = tree.to_string();
        let Some(at) = tree.find(&format!("\"{prop}\":\"")) else { return false };
        let name: String = tree[at + prop.len() + 4..].chars().take_while(|letter| *letter != '"').collect();
        handlers.call(&name, &event)
    }

    /// Answers `webFolders` from a list that `add` and `remove` change; keeps what was asked.
    fn host_keeps(start: &[&str], adds: Option<Result<&'static str, &'static str>>) -> Rc<std::cell::RefCell<Vec<(String, Value)>>> {
        let list = Rc::new(std::cell::RefCell::new(start.iter().map(|path| path.to_string()).collect::<Vec<_>>()));
        let asked = Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = asked.clone();
        testing::answer(move |method, params| {
            assert_eq!(method, "webFolders");
            let action = params["action"].as_str().unwrap_or_default().to_owned();
            seen.borrow_mut().push((action.clone(), params.clone()));
            match action.as_str() {
                "add" => match adds {
                    Some(Ok(path)) => {
                        list.borrow_mut().push(path.into());
                        Ok(json!(*list.borrow()))
                    }
                    Some(Err(why)) => Err(why.into()),
                    None => Ok(Value::Null),
                },
                "remove" => {
                    list.borrow_mut().retain(|path| Some(path.as_str()) != params["path"].as_str());
                    Ok(json!(*list.borrow()))
                }
                _ => Ok(json!(*list.borrow())),
            }
        });
        asked
    }

    #[test]
    fn the_section_says_what_is_always_allowed_and_lists_each_folder_with_its_name_and_path() {
        host_keeps(&["/Users/me/Pictures/refs", "/Users/me/work"], None);
        with(|state| *state = State::default());
        assert!(drawn().contains("Loading folders"));
        load();
        testing::settle();
        let now = drawn();
        for words in ["Agent tools", "Temporary folders and the Browser's own folders are always allowed", "refs", "/Users/me/Pictures/refs", "Remove work", "Add folder…", "Close agents' unused tabs after", "Never"] {
            assert!(now.contains(words), "{words} is not in {now}");
        }
        assert!(!now.contains("No folders added"));
        host_keeps(&[], None);
        load();
        testing::settle();
        assert!(drawn().contains("No folders added"));
    }

    #[test]
    fn adding_lists_the_folder_a_cancelled_picker_changes_nothing_and_a_refusal_is_a_toast() {
        with(|state| *state = State::default());
        let asked = host_keeps(&[], Some(Ok("/Users/me/new")));
        load();
        testing::settle();
        add();
        testing::settle();
        assert!(drawn().contains("/Users/me/new"));
        assert_eq!(asked.borrow().last().map(|(action, _)| action.as_str()), Some("add"));

        host_keeps(&["/Users/me/new"], None);
        add();
        testing::settle();
        assert!(drawn().contains("/Users/me/new") && problem().is_none());

        host_keeps(&[], Some(Err("/Users/me is writable by its group or others")));
        add();
        testing::settle();
        assert!(problem().is_some());
        let now = wings_plugin::ui::export(vec![problem().unwrap()]).0.to_string();
        assert!(now.contains("writable by its group or others") && now.contains("danger"), "{now}");
        dismiss();
        assert!(problem().is_none());
    }

    #[test]
    fn removing_sends_the_path_and_shows_the_list_without_it() {
        with(|state| *state = State::default());
        let asked = host_keeps(&["/a/one", "/a/two"], None);
        load();
        testing::settle();
        assert!(fire("onClick", json!({})), "the first button is the first folder's remove");
        testing::settle();
        let now = drawn();
        assert!(!now.contains("/a/one") && now.contains("/a/two"), "{now}");
        assert_eq!(asked.borrow().last().cloned(), Some(("remove".into(), json!({ "action": "remove", "path": "/a/one" }))));
    }

    /// Storage keeps nothing outside Wings, so what a choice wrote is what the setting's listeners hear.
    #[test]
    fn choosing_a_time_writes_the_idle_minutes_the_tabs_close_by() {
        with(|state| *state = State::default());
        assert_eq!(settings::idle_minutes(), 10);
        assert!(drawn().contains("\"value\":\"10\""));
        let written = Rc::new(std::cell::RefCell::new(Vec::new()));
        let heard = written.clone();
        settings::idle().on_change(move |minutes| heard.borrow_mut().push(*minutes));
        // Each press is the track's first change handler: they are the same node, so press by choosing each value.
        for value in ["30", "60", "0", "10"] {
            assert!(fire("onChange", json!({ "value": value })));
        }
        assert_eq!(*written.borrow(), [30, 60, 0, 10]);
    }

    #[test]
    fn a_folder_is_named_by_its_last_part() {
        assert_eq!((name_of("/Users/me/work"), name_of("/Users/me/work/"), name_of("/")), ("work", "work", "/"));
    }
}
