//! The Browser page in Wings Settings: the search engine, what the web views keep, clearing it, signing in with the
//! cookies of another browser on this Mac, and extensions.

use crate::{
    address::Engine,
    extension::{Staged, dialog_for},
    history, settings,
};
use serde_json::Value;
use std::{cell::RefCell, collections::HashMap};
use wings_plugin::{
    host::{self, Stage},
    task::spawn,
    ui::{self, Entry, Node, actions, button, column_page, confirm, entry, error_state, icon_button, icons, image, muted, notice, rows_card, section, select, setting, setting_field, setting_toggle, spinner, stack, text, toolbar},
};

#[derive(Clone, Debug, PartialEq)]
struct Source {
    id: String,
    browser: String,
    profile: String,
}

impl Source {
    /// "Chrome, Josip": the browser and the profile in it.
    fn label(&self) -> String {
        if self.profile.is_empty() { self.browser.clone() } else { format!("{}, {}", self.browser, self.profile) }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Import {
    Busy,
    Done(String),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
struct Extension {
    id: String,
    name: String,
    version: String,
    enabled: bool,
    icon: Option<String>,
    error: Option<String>,
}

/// An extension of another browser that can be added from there.
#[derive(Clone, Debug, PartialEq)]
struct Installed {
    source: String,
    name: String,
    description: String,
    icon: Option<String>,
}

struct Extensions {
    supported: bool,
    added: Vec<Extension>,
    installed: Vec<Installed>,
}

#[derive(Default)]
struct Page {
    sources: Option<Result<Vec<Source>, String>>,
    imports: HashMap<String, Import>,
    extensions: Option<Result<Extensions, String>>,
    /// What is being staged, as `PICK` or an installed extension's source, so only its button shows busy.
    staging: Option<String>,
    staged: Option<Staged>,
    adding: bool,
    problem: Option<String>,
    removing: Option<String>,
    confirm_clear: bool,
    clearing: bool,
    cleared: Option<Result<(), String>>,
}

const PICK: &str = "pick";

fn parse_sources(answer: &Value) -> Vec<Source> {
    let text = |source: &Value, key: &str| source[key].as_str().unwrap_or_default().to_owned();
    answer.as_array().map(Vec::as_slice).unwrap_or_default().iter().filter(|source| source["id"].is_string()).map(|source| Source { id: text(source, "id"), browser: text(source, "browser"), profile: text(source, "profile") }).collect()
}

fn parse_added(answer: &Value) -> (bool, Vec<Extension>) {
    let list = answer["extensions"].as_array().map(Vec::as_slice).unwrap_or_default();
    let added = list
        .iter()
        .filter(|extension| extension["id"].is_string())
        .map(|extension| Extension {
            id: extension["id"].as_str().unwrap_or_default().to_owned(),
            name: extension["name"].as_str().unwrap_or_default().to_owned(),
            version: extension["version"].as_str().unwrap_or_default().to_owned(),
            enabled: extension["enabled"].as_bool().unwrap_or_default(),
            icon: extension["icon"].as_str().map(str::to_owned),
            error: extension["error"].as_str().map(str::to_owned),
        })
        .collect();
    (answer["supported"].as_bool().unwrap_or_default(), added)
}

fn parse_installed(answer: &Value) -> Vec<Installed> {
    answer
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|extension| extension["source"].is_string())
        .map(|extension| Installed {
            source: extension["source"].as_str().unwrap_or_default().to_owned(),
            name: extension["name"].as_str().unwrap_or_default().to_owned(),
            description: extension["description"].as_str().unwrap_or_default().to_owned(),
            icon: extension["icon"].as_str().map(str::to_owned),
        })
        .collect()
}

/// The other browsers' extensions that aren't added yet. The host gives no link between the two, so an added one is
/// told by its name.
fn not_added<'a>(installed: &'a [Installed], added: &[Extension]) -> Vec<&'a Installed> {
    installed.iter().filter(|extension| !added.iter().any(|have| have.name.eq_ignore_ascii_case(&extension.name))).collect()
}

fn imported(source: &Source, answer: &Value) -> String {
    let (count, skipped) = (answer["imported"].as_u64().unwrap_or_default(), answer["skipped"].as_u64().unwrap_or_default());
    let mut line = format!("Imported {count} cookie{} from {}", if count == 1 { "" } else { "s" }, source.label());
    if skipped > 0 {
        line.push_str(&format!(", {skipped} skipped"));
    }
    line
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::new(Page::default());
}

fn with<R>(change: impl FnOnce(&mut Page) -> R) -> R {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

fn load_sources() {
    with(|page| page.sources = None);
    spawn(async {
        let sources = host::browser_cookie_sources().await.map(|answer| parse_sources(&answer));
        with(|page| page.sources = Some(sources));
    });
}

fn load_extensions() {
    with(|page| page.extensions = None);
    spawn(async {
        let listed = host::web_extensions().await;
        let extensions = match listed {
            Ok(answer) => {
                let (supported, added) = parse_added(&answer);
                // What other browsers have is only worth reading where extensions run.
                let installed = if supported { host::web_extensions_installed().await.map(|answer| parse_installed(&answer)).unwrap_or_default() } else { Vec::new() };
                Ok(Extensions { supported, added, installed })
            }
            Err(error) => Err(error),
        };
        with(|page| page.extensions = Some(extensions));
    });
}

fn import(source: Source) {
    if with(|page| page.imports.insert(source.id.clone(), Import::Busy)) == Some(Import::Busy) {
        return;
    }
    let keep = settings::keep_data().get();
    spawn(async move {
        let result = match host::browser_cookies_import(&source.id, keep).await {
            Ok(answer) => Import::Done(imported(&source, &answer)),
            Err(error) => Import::Failed(error),
        };
        with(|page| page.imports.insert(source.id, result));
    });
}

fn clear() {
    with(|page| {
        page.confirm_clear = false;
        page.clearing = true;
        page.cleared = None;
    });
    spawn(async {
        let web = host::web_data_clear().await.map(drop);
        let kept = history::clear();
        with(|page| {
            page.clearing = false;
            page.cleared = Some(web.and(kept));
        });
    });
}

/// Starts staging. The host only opens a picker right after a click, so this runs in the click's turn.
fn stage(source: Option<String>) {
    if with(|page| page.staging.is_some()) {
        return;
    }
    with(|page| {
        page.staging = Some(source.clone().unwrap_or_else(|| PICK.to_owned()));
        page.problem = None;
    });
    spawn(async move {
        let answer = host::web_extension_stage(match &source {
            Some(source) => Stage::Browser(source),
            None => Stage::Pick,
        })
        .await;
        with(|page| {
            page.staging = None;
            match answer {
                Ok(staged) => page.staged = Staged::from_json(&staged),
                Err(error) => page.problem = Some(error),
            }
        });
    });
}

fn add() {
    let Some(stage) = with(|page| if page.adding { None } else { page.staged.as_ref().map(|staged| staged.stage.clone()) }) else {
        return;
    };
    with(|page| {
        page.adding = true;
        page.problem = None;
    });
    spawn(async move {
        let answer = host::web_extension_add(&stage).await;
        let added = answer.is_ok();
        with(|page| {
            page.adding = false;
            match answer {
                Ok(_) => page.staged = None,
                Err(error) => page.problem = Some(error),
            }
        });
        if added {
            load_extensions();
        }
    });
}

fn cancel() {
    if let Some(staged) = with(|page| {
        page.problem = None;
        page.staged.take()
    }) {
        drop(host::web_extension_discard(&staged.stage));
    }
}

/// Runs a change to an extension and reads the list again, which shows what the host did.
fn change_extension(call: host::Pending<Result<Value, String>>) {
    spawn(async move {
        if let Err(error) = call.await {
            with(|page| page.problem = Some(error));
        }
        load_extensions();
    });
}

fn remove(id: &str) {
    with(|page| page.removing = None);
    change_extension(host::web_extension_remove(id));
}

fn cookies(page: &Page, keep: bool) -> Vec<Node> {
    let note = "Cookies stay on this Mac. Chrome-based browsers ask for Keychain access to read theirs.";
    let rows: Vec<Node> = match &page.sources {
        None => vec![entry(Entry { lead: Some(spinner()), title: muted("Looking for browsers"), detail: None, trailing: None })],
        Some(Err(error)) => return vec![error_state("Couldn't look for your browsers", error, Some(("Try again", Box::new(load_sources))))],
        Some(Ok(sources)) if sources.is_empty() => vec![entry(Entry { lead: None, title: muted("No other browsers found"), detail: None, trailing: None })],
        Some(Ok(sources)) => sources
            .iter()
            .map(|source| {
                let state = page.imports.get(&source.id);
                let done = match state {
                    Some(Import::Done(line)) => Some(muted(line.as_str()).prop("size", "small")),
                    _ => None,
                };
                let this = source.clone();
                entry(Entry {
                    lead: None,
                    title: text(source.label()),
                    detail: done,
                    trailing: Some(button("Import", "outline", state == Some(&Import::Busy), move || import(this.clone())).size("sm")),
                })
                .key(source.id.as_str())
            })
            .collect(),
    };
    let failed = page.sources.iter().flatten().flatten().filter_map(|source| match page.imports.get(&source.id) {
        Some(Import::Failed(error)) => Some(notice("danger", format!("{}: {error}", source.label())).key(format!("failed{}", source.id))),
        _ => None,
    });
    let description = if keep { note } else { "Cookies stay on this Mac. They are kept only while Wings runs, as Keep cookies and site data is off. Chrome-based browsers ask for Keychain access." };
    vec![section("Sign in with another browser", Some(description), rows)].into_iter().chain(failed).collect()
}

fn extension_rows(extensions: &Extensions, page: &Page) -> Vec<Node> {
    let mut rows: Vec<Node> = extensions
        .added
        .iter()
        .map(|extension| {
            let (id, removing) = (extension.id.clone(), extension.id.clone());
            let toggle = ui::checkbox("", extension.enabled, move |on| change_extension(host::web_extension_set_enabled(&id, on))).prop("switch", true).aria_label(&extension.name);
            let detail = match &extension.error {
                Some(error) => text(error.as_str()).prop("tone", "danger").prop("size", "small"),
                None => muted(format!("Version {}", extension.version)).prop("size", "small"),
            };
            entry(Entry {
                lead: extension.icon.as_deref().map(|icon| image(icon, &extension.name, 20, 20)),
                title: text(extension.name.as_str()).prop("weight", "medium"),
                detail: Some(detail),
                trailing: Some(ui::row().prop("gap", "s").prop("align", "center").child(toggle).child(icon_button(&format!("Remove {}", extension.name), &icons::TRASH, move || with(|page| page.removing = Some(removing.clone()))))),
            })
            .key(extension.id.as_str())
        })
        .collect();
    if rows.is_empty() {
        rows.push(entry(Entry { lead: None, title: muted("No extensions added yet"), detail: None, trailing: None }));
    }
    rows.push(setting("Add from folder or ZIP", "An unpacked extension folder or a ZIP of one.", button("Choose", "outline", page.staging.as_deref() == Some(PICK), || stage(None)).size("sm")));
    rows
}

/// A description's first line: one with a line break in it drew its second line under the row's icon.
fn first_line(description: &str) -> &str {
    description.lines().find(|line| !line.trim().is_empty()).unwrap_or_default().trim()
}

fn extensions(page: &Page) -> Vec<Node> {
    let loaded = match &page.extensions {
        None => return vec![section("Extensions", None, [entry(Entry { lead: Some(spinner()), title: muted("Loading extensions"), detail: None, trailing: None })])],
        Some(Err(error)) => return vec![error_state("Couldn't load extensions", error, Some(("Try again", Box::new(load_extensions))))],
        Some(Ok(loaded)) => loaded,
    };
    if !loaded.supported {
        return vec![section("Extensions", None, []), notice("warn", "Extensions need macOS 15.4 or newer")];
    }
    let mut out = vec![section("Extensions", Some("Chrome and Firefox extensions, which run on the pages you open in the browser."), extension_rows(loaded, page))];
    let others = not_added(&loaded.installed, &loaded.added);
    if !others.is_empty() {
        let rows = others.into_iter().map(|extension| {
            let source = extension.source.clone();
            entry(Entry {
                lead: extension.icon.as_deref().map(|icon| image(icon, &extension.name, 20, 20)),
                title: text(extension.name.as_str()).prop("weight", "medium"),
                detail: (!extension.description.is_empty()).then(|| muted(first_line(&extension.description)).prop("size", "small").prop("truncate", true)),
                trailing: Some(button("Add", "outline", page.staging.as_deref() == Some(extension.source.as_str()), move || stage(Some(source.clone()))).size("sm")),
            })
            .key(extension.source.as_str())
        });
        out.push(section("From your browsers", Some("Extensions you already use in another browser on this Mac."), rows));
    }
    let removing = page.removing.as_ref().and_then(|id| loaded.added.iter().find(|extension| &extension.id == id));
    if let Some(extension) = removing {
        let id = extension.id.clone();
        let buttons = actions(Some(button("Cancel", "ghost", false, || with(|page| page.removing = None))), [button("Remove", "destructive", false, move || remove(&id))]);
        out.push(confirm(format!("Remove {}?", extension.name), [muted("Its settings and data go with it.")], [buttons]));
    }
    out
}

fn view() -> Vec<Node> {
    let (engine, keep) = (settings::engine().get(), settings::keep_data().get());
    PAGE.with(|page| {
        let page = page.borrow();
        let engines: Vec<(String, String)> = Engine::ALL.iter().map(|engine| (engine.id().to_owned(), engine.label().to_owned())).collect();
        let search = section(
            "Search",
            None,
            [rows_card([setting_field(
                "Search engine",
                "Used for what you type in the address bar that isn't an address.",
                select("Search engine", &engines, engine.id(), |id| {
                    if let Some(engine) = Engine::from_id(id) {
                        drop(settings::engine().set(engine));
                    }
                }),
            )])],
        );
        let privacy = section(
            "Privacy",
            None,
            [rows_card([
                setting_toggle("Keep cookies and site data", "Stay signed in to sites after Wings quits. Off keeps them only while Wings runs.", keep, |on| drop(settings::keep_data().set(on))),
                setting("Clear browsing data", "Removes your history, cookies and site data.", button("Clear", "outline", page.clearing, || with(|page| page.confirm_clear = true)).size("sm")),
            ])],
        );
        let ask = page.confirm_clear.then(|| {
            let buttons = actions(Some(button("Cancel", "ghost", false, || with(|page| page.confirm_clear = false))), [button("Clear", "destructive", false, clear)]);
            confirm("Clear browsing data?", [muted("This signs you out of the sites you are signed in to and empties your history.")], [buttons])
        });
        let cleared = page.cleared.as_ref().map(|result| match result {
            Ok(()) => notice("success", "Cleared your history, cookies and site data."),
            Err(error) => notice("danger", error.as_str()),
        });
        let problem = (page.staged.is_none()).then(|| page.problem.as_deref().map(|problem| notice("danger", problem))).flatten();
        let column = stack().prop("gap", "l").children([search, privacy]).maybe(ask).maybe(cleared).children(cookies(&page, keep)).children(extensions(&page)).maybe(problem);
        let dialog = page.staged.as_ref().map(|staged| dialog_for(staged, page.adding, page.problem.as_deref(), cancel, add));
        vec![column_page(toolbar("Browser", None), std::iter::once(column).chain(dialog))]
    })
}

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    load_sources();
    load_extensions();
    wings_plugin::run(view);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::strings;
    use serde_json::json;

    fn source(id: &str, browser: &str, profile: &str) -> Source {
        Source { id: id.into(), browser: browser.into(), profile: profile.into() }
    }

    #[test]
    fn a_source_is_named_by_its_browser_and_profile() {
        assert_eq!(source("c", "Chrome", "Josip").label(), "Chrome, Josip");
        assert_eq!(source("s", "Safari", "").label(), "Safari");
    }

    #[test]
    fn the_result_of_an_import_says_how_many_came_over() {
        let chrome = source("c", "Chrome", "Josip");
        assert_eq!(imported(&chrome, &json!({ "imported": 214, "skipped": 0 })), "Imported 214 cookies from Chrome, Josip");
        assert_eq!(imported(&chrome, &json!({ "imported": 1, "skipped": 3 })), "Imported 1 cookie from Chrome, Josip, 3 skipped");
    }

    #[test]
    fn the_host_answers_are_read_and_what_has_no_id_is_left_out() {
        assert_eq!(parse_sources(&json!([{ "id": "c:1", "browser": "Chrome", "profile": "Josip" }, { "browser": "x" }])), [source("c:1", "Chrome", "Josip")]);
        assert_eq!(parse_sources(&Value::Null), []);
        let (supported, added) = parse_added(&json!({ "supported": true, "extensions": [{ "id": "e1", "name": "Dark", "version": "2", "enabled": true, "icon": null, "error": "no" }, {}] }));
        assert_eq!((supported, added.len(), added[0].enabled, added[0].error.clone()), (true, 1, true, Some("no".to_owned())));
        assert!(!parse_added(&json!({ "supported": false, "extensions": [] })).0);
        let installed = parse_installed(&json!([{ "source": "c:1", "name": "Dark", "description": "d", "icon": "i1" }, { "name": "no source" }]));
        assert_eq!(installed, [Installed { source: "c:1".into(), name: "Dark".into(), description: "d".into(), icon: Some("i1".into()) }]);
        assert_eq!(strings(&json!(["a", 1])), ["a"]);
    }

    #[test]
    fn an_extension_already_added_is_not_offered_from_the_other_browsers() {
        let installed = |name: &str| Installed { source: name.into(), name: name.into(), description: String::new(), icon: None };
        let added = [Extension { id: "e1".into(), name: "dark reader".into(), version: "1".into(), enabled: true, icon: None, error: None }];
        let list = [installed("Dark Reader"), installed("uBlock")];
        let left: Vec<&str> = not_added(&list, &added).into_iter().map(|extension| extension.name.as_str()).collect();
        assert_eq!(left, ["uBlock"]);
    }

    #[test]
    fn the_page_shows_the_extensions_notice_before_macos_15_4_and_the_sources_with_an_import_button() {
        with(|page| {
            page.sources = Some(Ok(vec![source("c", "Chrome", "Josip")]));
            page.extensions = Some(Ok(Extensions { supported: false, added: Vec::new(), installed: Vec::new() }));
        });
        let drawn = wings_plugin::ui::export(view()).0.to_string();
        for shown in ["Chrome, Josip", "Import", "Extensions need macOS 15.4 or newer", "Search engine", "Clear browsing data"] {
            assert!(drawn.contains(shown), "{shown} is not in {drawn}");
        }
    }
}
