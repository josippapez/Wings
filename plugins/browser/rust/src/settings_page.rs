//! The Browser page in Wings Settings: the engine (WebKit or Chromium), the search engine, what the web views keep, clearing it, signing in with the
//! cookies of another browser on this Mac, and extensions.

use crate::{
    address::Engine,
    agent_settings, engine_settings,
    extension::{Staged, dialog_for},
    history, settings,
};
use serde_json::Value;
use std::{cell::RefCell, collections::HashMap};
use wings_plugin::{
    host::{self, Stage},
    task::spawn,
    ui::{self, Entry, Node, actions, button, column_page, confirm, dialog, entry, error_state, icon_button, icons, image, muted, notice, rows_card, section, select, setting, setting_field, setting_toggle, spinner, stack, text, toast, toolbar},
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
    /// Wings couldn't read the Keychain key it checks extensions with, so none loads until it asks again.
    keychain: bool,
}

#[derive(Default)]
struct Page {
    /// Wings says the cookies can't be kept past this run on this Mac (before macOS 14).
    cookies_not_kept: bool,
    sources: Option<Result<Vec<Source>, String>>,
    imports: HashMap<String, Import>,
    extensions: Option<Result<Extensions, String>>,
    /// What other browsers have, once the person asked to look (`look_in_browsers`): Wings reads their profiles only then.
    /// It is the page's own, so reading the added extensions again (after an add, a remove or a switch) leaves it be.
    installed: Option<Vec<Installed>>,
    /// Beside the list: what macOS wouldn't let Wings read of the other browsers (a newer Wings sends it with what it did find).
    refused: Option<String>,
    /// Looking in the other browsers is under way.
    looking: bool,
    /// Why the last look found nothing: shown with Try again, never as an empty list.
    look_failed: Option<String>,
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

/// The host answers a list, or `{ extensions, refused }` when some browser's folder was refused.
fn parse_refused(answer: &Value) -> Option<String> {
    answer["refused"].as_str().map(str::to_owned)
}

fn parse_installed(answer: &Value) -> Vec<Installed> {
    let list = if answer.is_array() { answer } else { &answer["extensions"] };
    list
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
                // Older Wings don't say, and keep them.
                with(|page| page.cookies_not_kept = answer["keepsCookies"] == false);
                Ok(Extensions { supported, added, keychain: answer["keychain"] == true })
            }
            Err(error) => Err(error),
        };
        with(|page| page.extensions = Some(extensions));
    });
}

/// Lists the extensions the other browsers on this Mac have, on the person's click, as Wings asks. The answer is kept
/// whatever the added extensions are doing, and a failure is kept to show, not turned into an empty list.
fn look_in_browsers() {
    if with(|page| std::mem::replace(&mut page.looking, true)) {
        return;
    }
    with(|page| page.look_failed = None);
    spawn(async {
        let listed = host::web_extensions_installed().await.map(|answer| (parse_installed(&answer), parse_refused(&answer)));
        with(|page| {
            page.looking = false;
            match listed {
                Ok((installed, refused)) => {
                    page.installed = Some(installed);
                    page.refused = refused;
                }
                Err(error) => page.look_failed = Some(error),
            }
        });
    });
}

fn import(source: Source) {
    if with(|page| page.imports.insert(source.id.clone(), Import::Busy)) == Some(Import::Busy) {
        return;
    }
    let (keep, signins) = (settings::keep_data().get(), settings::keep_signins().get());
    spawn(async move {
        let result = match host::browser_cookies_import(&source.id, keep, signins).await {
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

fn dismiss() {
    with(|page| page.problem = None);
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
    dismiss();
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

/// Has Wings ask the Keychain again, on the click that asked: it shows the Keychain's prompt.
fn check_again() {
    change_extension(host::web_extensions_check_again());
}

fn cookies(page: &Page, keep: bool, signins: bool) -> Vec<Node> {
    let mut rows: Vec<Node> = match &page.sources {
        None => vec![entry(Entry { lead: Some(spinner()), title: muted("Looking for browsers"), detail: None, trailing: None })],
        Some(Err(error)) => return vec![error_state("Couldn't look for your browsers", error, Some(("Try again", Box::new(load_sources))))],
        Some(Ok(sources)) if sources.is_empty() => vec![entry(Entry { lead: None, title: muted("No other browsers found"), detail: None, trailing: None })],
        Some(Ok(sources)) => sources
            .iter()
            .map(|source| {
                let state = page.imports.get(&source.id);
                let done = match state {
                    Some(Import::Done(line)) => line.as_str(),
                    _ => "Copies its cookies here, so you are signed in where it is.",
                };
                let this = source.clone();
                let import = button("Import", "outline", state == Some(&Import::Busy), move || import(this.clone())).size("sm");
                setting(&source.label(), done, import).key(source.id.as_str())
            })
            .collect(),
    };
    let failed = page.sources.iter().flatten().flatten().filter_map(|source| match page.imports.get(&source.id) {
        Some(Import::Failed(error)) => Some(notice("danger", format!("{}: {error}", source.label())).key(format!("failed{}", source.id))),
        _ => None,
    });
    // Kept, they are a file of this Mac's that WebKit doesn't encrypt: the note says so whenever they are.
    let description = match (keep && !page.cookies_not_kept, signins) {
        (true, true) => "Imported sign-ins are kept after Wings quits, stored unencrypted on this Mac, where apps running as you can read them, until you clear them. Chrome-based browsers ask for Keychain access to read theirs.",
        (true, false) => "Imported sign-ins last until Wings quits. Chrome-based browsers ask for Keychain access to read theirs.",
        (false, _) => "Imported sign-ins last until Wings quits, as Keep cookies and site data is off. Chrome-based browsers ask for Keychain access to read theirs.",
    };
    let keeping = "Keep imported sign-ins after Wings quits";
    // Before the browsers: it decides what an import does.
    rows.insert(0, if keep && !page.cookies_not_kept {
        setting_toggle(keeping, "They are stored unencrypted on this Mac while kept.", signins, |on| drop(settings::keep_signins().set(on)))
    } else {
        let off = ui::checkbox("", false, |_| {}).prop("switch", true).aria_label(keeping).disabled(true);
        setting(keeping, "Needs Keep cookies and site data.", off)
    });
    vec![section("Sign in with another browser", Some(description), [rows_card(rows)])].into_iter().chain(failed).collect()
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
                lead: extension.icon.as_deref().map(|icon| image(icon, &extension.name, 16, 16)),
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

/// The "From your browsers" section: the Look row, that it is looking, why it couldn't, or what it found.
fn from_browsers(page: &Page, loaded: &Extensions) -> Vec<Node> {
    if page.looking {
        return vec![section("From your browsers", None, [rows_card([entry(Entry { lead: Some(spinner()), title: muted("Looking in your browsers"), detail: None, trailing: None })])])];
    }
    if let Some(error) = &page.look_failed {
        return vec![error_state("Couldn't look in your browsers", error, Some(("Try again", Box::new(look_in_browsers))))];
    }
    let Some(installed) = &page.installed else {
        let look = button("Look", "outline", false, look_in_browsers).size("sm");
        return vec![section("From your browsers", None, [rows_card([setting("Extensions you already use", "Lists the extensions of the other browsers on this Mac, to add here.", look)])])];
    };
    let others = not_added(installed, &loaded.added);
    if others.is_empty() {
        let line = if installed.is_empty() { "No extensions found in your other browsers" } else { "Everything your other browsers have is already added" };
        let card = section("From your browsers", None, [rows_card([entry(Entry { lead: None, title: muted(line), detail: None, trailing: None })])]);
        return std::iter::once(card).chain(page.refused.as_deref().map(|why| notice("warn", why))).collect();
    }
    let rows = others.into_iter().map(|extension| {
        let source = extension.source.clone();
        entry(Entry {
            lead: extension.icon.as_deref().map(|icon| image(icon, &extension.name, 16, 16)),
            title: text(extension.name.as_str()).prop("weight", "medium"),
            detail: (!extension.description.is_empty()).then(|| muted(first_line(&extension.description)).prop("size", "small").prop("truncate", true)),
            trailing: Some(button("Add", "outline", page.staging.as_deref() == Some(extension.source.as_str()), move || stage(Some(source.clone()))).size("sm")),
        })
        .key(extension.source.as_str())
    });
    let list = section("From your browsers", Some("Extensions you already use in another browser on this Mac."), [rows_card(rows)]);
    std::iter::once(list).chain(page.refused.as_deref().map(|why| notice("warn", why))).collect()
}

/// Asks before an added extension goes, as a dialog over the page wherever the page is scrolled to.
fn remove_dialog(page: &Page) -> Option<Node> {
    let extension = page.removing.as_ref().and_then(|id| page.extensions.as_ref()?.as_ref().ok()?.added.iter().find(|extension| &extension.id == id))?;
    let id = extension.id.clone();
    let cancel = || with(|page| page.removing = None);
    Some(
        dialog(&format!("Remove {}?", extension.name), 420, cancel)
            .child(muted("Its settings and data go with it."))
            .child(actions(Some(button("Cancel", "ghost", false, cancel)), [button("Remove", "destructive", false, move || remove(&id))])),
    )
}

fn extensions(page: &Page) -> Vec<Node> {
    let loaded = match &page.extensions {
        None => return vec![section("Extensions", None, [rows_card([entry(Entry { lead: Some(spinner()), title: muted("Loading extensions"), detail: None, trailing: None })])])],
        Some(Err(error)) => return vec![error_state("Couldn't load extensions", error, Some(("Try again", Box::new(load_extensions))))],
        Some(Ok(loaded)) => loaded,
    };
    if !loaded.supported {
        return vec![section("Extensions", None, []), notice("warn", "Extensions need macOS 15.4 or newer")];
    }
    let mut out = vec![section("Extensions", Some("Chrome and Firefox extensions, which run on the pages you open in the browser."), [rows_card(extension_rows(loaded, page))])];
    // None loads until Wings can check them, and it asks the Keychain only when the person says so.
    if loaded.keychain {
        out.push(error_state(
            "Wings needs Keychain access to check your extensions",
            "They stay off until it can. Try again shows the Keychain's prompt: choose Allow or Always Allow.",
            Some(("Try again", Box::new(check_again))),
        ));
    }
    out.extend(from_browsers(page, loaded));
    out
}

fn view() -> Vec<Node> {
    let (engine, keep, signins) = (settings::engine().get(), settings::keep_data().get(), settings::keep_signins().get());
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
                // Off and fixed on a Mac before 14, which keeps a plugin's cookies only while Wings runs.
                if page.cookies_not_kept {
                    let off = ui::checkbox("", false, |_| {}).prop("switch", true).aria_label("Keep cookies and site data").disabled(true);
                    setting("Keep cookies and site data", "Needs macOS 14. On this Mac cookies are kept only while Wings runs.", off)
                } else {
                    setting_toggle("Keep cookies and site data", "Stay signed in to sites after Wings quits. Off keeps them only while Wings runs.", keep, |on| drop(settings::keep_data().set(on)))
                },
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
        // A toast floats over the page's foot wherever it is scrolled to: below a long list of extensions a notice went unseen.
        let problem = (page.staged.is_none()).then(|| page.problem.as_deref().map(|problem| toast("danger", problem, dismiss).key("problem"))).flatten();
        let column = stack().prop("gap", "l").children(engine_settings::view()).children([search, privacy]).maybe(ask).maybe(cleared).children(cookies(&page, keep, signins)).children(extensions(&page)).children(agent_settings::view());
        let dialog = page.staged.as_ref().map(|staged| dialog_for(staged, page.adding, page.problem.as_deref(), cancel, add));
        vec![column_page(toolbar("Browser", None), std::iter::once(column).chain(dialog).chain(remove_dialog(&page))).toasts(problem.into_iter().chain(agent_settings::problem()))]
    })
}

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    engine_settings::load();
    agent_settings::load();
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
            page.extensions = Some(Ok(Extensions { supported: false, added: Vec::new(), keychain: false }));
        });
        let drawn = wings_plugin::ui::export(view()).0.to_string();
        for shown in ["Chrome, Josip", "Import", "Extensions need macOS 15.4 or newer", "Search engine", "Clear browsing data"] {
            assert!(drawn.contains(shown), "{shown} is not in {drawn}");
        }
    }

    #[test]
    fn on_a_mac_that_cant_keep_cookies_the_switch_is_off_and_fixed_and_says_why_and_elsewhere_it_works() {
        let why = "Needs macOS 14";
        with(|page| page.cookies_not_kept = false);
        let drawn = wings_plugin::ui::export(view()).0.to_string();
        assert!(!drawn.contains(why) && !drawn.contains("\"disabled\":true"), "{drawn}");
        with(|page| page.cookies_not_kept = true);
        let drawn = wings_plugin::ui::export(view()).0.to_string();
        assert!(drawn.contains(why) && drawn.contains("\"disabled\":true"), "{drawn}");
    }

    #[test]
    fn the_sign_in_and_extension_sections_are_cards_of_rows_as_search_and_privacy_are() {
        with(|page| {
            page.cookies_not_kept = false;
            page.sources = Some(Ok(vec![Source { id: "chrome:Default".into(), browser: "Chrome".into(), profile: "Josip".into() }]));
        });
        let drawn = wings_plugin::ui::export(view()).0.to_string();
        // Search, Privacy, Sign in and Extensions each hold a card of rows.
        assert!(drawn.matches("\"variant\":\"rows\"").count() >= 4, "{drawn}");
    }

    fn answer_host(installed: Result<Value, String>, added: Vec<&'static str>) {
        wings_plugin::host::testing::answer(move |method, params| match (method, params["action"].as_str()) {
            ("webExtensions", Some("list")) => Ok(json!({ "supported": true, "extensions": added.iter().map(|name| json!({ "id": format!("id-{name}"), "name": name, "version": "1", "enabled": true })).collect::<Vec<_>>() })),
            ("webExtensions", Some("installed")) => installed.clone(),
            _ => Ok(Value::Null),
        });
    }

    fn found(names: &[&str]) -> Value {
        Value::Array(names.iter().enumerate().map(|(at, name)| json!({ "source": at.to_string(), "name": name, "description": "d" })).collect())
    }

    fn drawn() -> String {
        wings_plugin::ui::export(view()).0.to_string()
    }

    #[test]
    fn what_a_look_found_stays_when_the_added_extensions_are_read_again() {
        answer_host(Ok(found(&["Bitwarden", "uBlock"])), vec![]);
        load_extensions();
        wings_plugin::host::testing::settle();
        look_in_browsers();
        wings_plugin::host::testing::settle();
        assert!(drawn().contains("uBlock"), "{}", drawn());
        // Adding one, switching one or removing one reads the added list again.
        answer_host(Ok(found(&["Bitwarden", "uBlock"])), vec!["Bitwarden"]);
        load_extensions();
        wings_plugin::host::testing::settle();
        let now = drawn();
        assert!(now.contains("uBlock") && !now.contains("\"Look\""), "{now}");
    }

    #[test]
    fn a_look_that_fails_says_so_with_try_again_and_not_an_empty_list() {
        answer_host(Err("A page can add an extension only right after you click or type in it".into()), vec![]);
        load_extensions();
        look_in_browsers();
        wings_plugin::host::testing::settle();
        let now = drawn();
        for shown in ["Couldn't look in your browsers", "only right after you click", "Try again"] {
            assert!(now.contains(shown), "{shown} is not in {now}");
        }
        // Try again asks once more, and the list shows when it answers.
        answer_host(Ok(found(&["uBlock"])), vec![]);
        look_in_browsers();
        wings_plugin::host::testing::settle();
        let now = drawn();
        assert!(now.contains("uBlock") && !now.contains("Couldn't look"), "{now}");
    }

    /// What the Toast in the page's tree sends back when it is dismissed, drawn from the page as it is now.
    fn press_dismiss() -> bool {
        let (tree, handlers) = wings_plugin::ui::export(view());
        let tree = tree.to_string();
        let Some(at) = tree.find("\"onDismiss\":\"") else { return false };
        let name: String = tree[at + 13..].chars().take_while(|letter| *letter != '"').collect();
        handlers.call(&name, &json!({}))
    }

    #[test]
    fn a_failing_action_shows_a_toast_its_close_clears_and_the_next_action_clears() {
        wings_plugin::host::testing::answer(|_, params| match params["action"].as_str() {
            Some("list") => Ok(json!({ "supported": true, "extensions": [{ "id": "e1", "name": "Dark", "version": "1", "enabled": true }] })),
            Some("remove") => Err("Couldn't remove Dark".into()),
            _ => Ok(Value::Null),
        });
        with(|page| page.problem = None);
        assert!(!drawn().contains("\"Toast\""), "{}", drawn());
        remove("e1");
        wings_plugin::host::testing::settle();
        let now = drawn();
        assert!(now.contains("\"Toast\"") && now.contains("Couldn't remove Dark") && now.contains("\"tone\":\"danger\""), "{now}");
        // The close button leaves it out.
        assert!(press_dismiss());
        assert!(!drawn().contains("\"Toast\""), "{}", drawn());
        // Another action leaves out the one the last had, before its own answer comes.
        remove("e1");
        wings_plugin::host::testing::settle();
        assert!(drawn().contains("Couldn't remove Dark"));
        wings_plugin::host::testing::answer(|_, _| Ok(Value::Null));
        change_extension(host::web_extension_set_enabled("e1", false));
        assert!(!drawn().contains("Couldn't remove Dark"), "{}", drawn());
        wings_plugin::host::testing::settle();
    }

    #[test]
    fn when_everything_found_is_added_a_line_says_so() {
        answer_host(Ok(found(&["Bitwarden"])), vec!["bitwarden"]);
        load_extensions();
        look_in_browsers();
        wings_plugin::host::testing::settle();
        assert!(drawn().contains("already added"), "{}", drawn());
        answer_host(Ok(found(&[])), vec![]);
        with(|page| page.installed = None);
        look_in_browsers();
        wings_plugin::host::testing::settle();
        assert!(drawn().contains("No extensions found in your other browsers"), "{}", drawn());
    }

    /// Imported sign-ins go as Wings quits unless the person turned on keeping them (off by default).
    #[test]
    fn an_import_keeps_sign_ins_past_the_run_only_when_the_setting_says_so() {
        let sent = std::rc::Rc::new(RefCell::new(Vec::new()));
        let seen = sent.clone();
        wings_plugin::host::testing::answer(move |method, params| {
            seen.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(json!({ "imported": 1, "skipped": 0 }))
        });
        import(source("c:1", "Chrome", "Josip"));
        wings_plugin::host::testing::settle();
        let sent = sent.borrow();
        let import = sent.iter().find(|(method, _)| method == "browserCookies").expect("an import");
        assert_eq!((import.1["source"].clone(), import.1["keepSignIns"].clone()), (json!("c:1"), json!(false)));
    }

    /// Kept, the imported cookies are a file WebKit doesn't encrypt: the note says so then, and only then.
    #[test]
    fn the_sign_in_note_says_kept_sign_ins_are_stored_unencrypted() {
        let page = Page::default();
        let drawn = |keep: bool, signins: bool| wings_plugin::ui::export(cookies(&page, keep, signins)).0.to_string();
        assert!(drawn(true, true).contains("kept after Wings quits, stored unencrypted on this Mac, where apps running as you can read them"));
        for (keep, signins) in [(true, false), (false, true), (false, false)] {
            let note = drawn(keep, signins);
            assert!(note.contains("Imported sign-ins last until Wings quits") && note.contains("Keep imported sign-ins after Wings quits"), "{note}");
        }
        assert!(drawn(false, true).contains("Needs Keep cookies and site data"));
    }

    /// Wings couldn't read its Keychain key: the page says so, and Try again asks the host once, on the click.
    #[test]
    fn when_wings_cant_read_its_keychain_key_the_page_says_so_and_try_again_asks_again() {
        let asked = std::rc::Rc::new(RefCell::new(Vec::new()));
        let seen = asked.clone();
        wings_plugin::host::testing::answer(move |_, params| {
            let action = params["action"].as_str().unwrap_or_default().to_owned();
            seen.borrow_mut().push(action.clone());
            match action.as_str() {
                "list" => Ok(json!({ "supported": true, "keychain": true, "extensions": [{ "id": "e1", "name": "Bitwarden", "version": "1", "enabled": true, "error": "Wings needs Keychain access to check your extensions" }] })),
                _ => Ok(Value::Null),
            }
        });
        load_extensions();
        wings_plugin::host::testing::settle();
        let now = drawn();
        for shown in ["Wings needs Keychain access to check your extensions", "Try again", "Bitwarden"] {
            assert!(now.contains(shown), "{shown} is not in {now}");
        }
        check_again();
        wings_plugin::host::testing::settle();
        assert_eq!(asked.borrow().iter().filter(|action| *action == "checkAgain").count(), 1);
    }

    /// macOS refusing Chrome's folder was shown as "No extensions found": now it says what to turn on, with Try again.
    #[test]
    fn a_refused_browser_is_named_with_what_to_turn_on_not_shown_as_no_extensions() {
        let why = "macOS doesn't let Wings read Chrome's extensions. Turn on Full Disk Access for Wings in System Settings, Privacy & Security, then try again.";
        answer_host(Err(why.into()), vec![]);
        load_extensions();
        look_in_browsers();
        wings_plugin::host::testing::settle();
        let now = drawn();
        for shown in ["Chrome's extensions", "Full Disk Access", "Try again"] {
            assert!(now.contains(shown), "{shown} is not in {now}");
        }
        assert!(!now.contains("No extensions found"), "{now}");
        // Some found and one refused: the list and a notice naming it.
        answer_host(Ok(json!({ "extensions": found(&["uBlock"]), "refused": why })), vec![]);
        look_in_browsers();
        wings_plugin::host::testing::settle();
        let now = drawn();
        assert!(now.contains("uBlock") && now.contains("Chrome's extensions") && !now.contains("Couldn't look"), "{now}");
    }

    /// The question showed at the foot of the extensions, off screen on a long page: it is a dialog over the page.
    #[test]
    fn removing_an_extension_asks_in_a_dialog_with_cancel_and_remove() {
        with(|page| {
            page.extensions = Some(Ok(Extensions { supported: true, added: vec![Extension { id: "e1".into(), name: "Dark".into(), version: "1".into(), enabled: true, icon: None, error: None }], keychain: false }));
            page.removing = None;
        });
        assert!(!drawn().contains("Remove Dark?"));
        with(|page| page.removing = Some("e1".into()));
        let now = drawn();
        for shown in ["\"Dialog\"", "Remove Dark?", "Its settings and data go with it.", "\"Cancel\"", "\"destructive\""] {
            assert!(now.contains(shown), "{shown} is not in {now}");
        }
        assert!(!now.contains("\"Confirm\""), "{now}");
        with(|page| page.removing = None);
    }

    #[test]
    fn while_it_looks_the_page_says_so_and_a_second_press_asks_nothing() {
        with(|page| page.extensions = Some(Ok(Extensions { supported: true, added: Vec::new(), keychain: false })));
        let asked = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = asked.clone();
        wings_plugin::host::testing::answer(move |_, _| {
            count.set(count.get() + 1);
            Ok(Value::Null)
        });
        with(|page| page.looking = true);
        assert!(drawn().contains("Looking in your browsers"));
        look_in_browsers();
        wings_plugin::host::testing::settle();
        assert_eq!(asked.get(), 0);
    }
}
