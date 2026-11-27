//! `webData`, `webExtensions`, `webEngine` and `browserCookies`: what a plugin that shows web pages (`permissions.webView`) may do
//! with them beyond its pages' `WebView` nodes. What reads the disk or the network runs off the frame's thread; what
//! WebKit does runs on the window's (`Env::web`).

use std::{path::PathBuf, sync::Arc};

use serde_json::{Value, json};
use wings_plugins::permissions::{may_import_browser_cookies, may_show_web_views};

use super::{Shared, to_json};
use crate::{
    page::{plugin::env::Access, runtime::Reply},
    web::{Job, Profile, engine, extensions},
};

/// The folders of the extensions `installed` last found for each plugin, in the order it listed them.
static FOUND: std::sync::Mutex<std::collections::BTreeMap<String, Vec<PathBuf>>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// The most bytes of a store's package: what `wings_web_extensions::unpack` takes at most, unpacked.
const PACKAGE_MAX: u64 = wings_web_extensions::MAX_TOTAL_BYTES;

/// The profile whose page's click downloaded the Chromium engine that waits to be installed: only it may install it, once.
static FETCHED_FOR: std::sync::Mutex<Option<Profile>> = std::sync::Mutex::new(None);
const NOT_FETCHED: &str = "A page can install Chromium only after you click to download it there";

impl Shared {
    fn web_profile(&self) -> Profile {
        Profile::of(&self.config.id, &self.config.scope, &self.config.root)
    }

    /// The three calls. The frame has checked that you just clicked for those that need it (`needs_click`).
    pub(super) fn web_call(self: &Arc<Self>, access: Access, method: &str, params: Value, reply: Reply) {
        let id = &self.config.id;
        let allowed = match method {
            "browserCookies" => may_import_browser_cookies(&access.plugin),
            _ => may_show_web_views(&access.plugin),
        };
        if !allowed {
            let what = if method == "browserCookies" { "import another browser's cookies" } else { "show web pages" };
            return reply.err(format!("{id} may not {what}"));
        }
        let action = params.get("action").and_then(Value::as_str).unwrap_or_default().to_owned();
        let profile = self.web_profile();
        let text = |key: &str| params.get(key).and_then(Value::as_str).map(str::to_owned);
        let done = move |reply: Reply| -> Box<dyn FnOnce(Result<String, String>) + Send> { Box::new(move |result| reply.done(result)) };
        match (method, action.as_str()) {
            ("webData", "clear") => access.env.web(Job::Clear(profile), done(reply)),
            ("webData", "clearSite") => match text("url").and_then(|url| engine::site::Site::of(&url)) {
                Some(site) => access.env.clear_site(profile, site, done(reply)),
                None => reply.err("clearSite needs the address of a web page".into()),
            },
            ("webExtensions", "list") => {
                let (env, plugin) = (access.env.clone(), id.clone());
                access.env.web(
                    Job::Supported,
                    Box::new(move |supported| {
                        let supported = supported.as_deref() == Ok("true");
                        // Each with why it doesn't load, if it doesn't: the check every load goes through (`verify`).
                        let key = || extensions::key(&profile);
                        let list: Vec<Value> = extensions::list(&profile)
                            .iter()
                            .map(|added| {
                                let icon = extensions::icon_path(&profile, added).and_then(|path| icon(&plugin, &path));
                                json!({
                                    "id": added.id, "name": added.name, "version": added.version,
                                    "description": added.description, "enabled": added.enabled, "icon": icon,
                                    "permissions": added.permissions, "hosts": added.hosts,
                                    "error": extensions::verify(&profile, added, &key).err(),
                                })
                            })
                            .collect();
                        drop(env);
                        // `keepsCookies`: whether Keep cookies keeps them past this run here (macOS 14, `web::keeps_refusal`).
                        let keeps = per_plugin_stores();
                        // `keychain`: Wings couldn't read the key its extensions are checked with; `checkAgain` asks again.
                        let keychain = list.iter().any(|entry| entry["error"] == extensions::NO_KEY);
                        reply.done(to_json(&json!({ "supported": supported, "keepsCookies": keeps, "keychain": keychain, "extensions": list })));
                    }),
                )
            }
            ("webExtensions", "installed") => {
                let plugin = id.clone();
                self.off_thread(reply, move |_| {
                    let detected = wings_web_extensions::detect_installed();
                    let refusal = detected.refusal_message();
                    // macOS refused every folder there was: say so, rather than answering "none".
                    if detected.found.is_empty() {
                        if let Some(why) = refusal {
                            return Err(why);
                        }
                    }
                    let installed = detected.found;
                    // The page gets each by its place in this list, never its folder, which names the person's profiles.
                    FOUND.lock().unwrap().insert(plugin.clone(), installed.iter().map(|found| found.source.clone()).collect());
                    // Nor its profiles' names, which are often the person's own: "Chrome (2)" tells two copies apart.
                    let from = copies(installed.iter().map(|found| (found.browser_name.as_str(), found.profile.as_str())));
                    let found: Vec<Value> = installed
                        .into_iter()
                        .zip(from)
                        .enumerate()
                        .map(|(at, (found, from))| {
                            json!({
                                "source": at.to_string(), "browser": found.browser_name, "from": from,
                                "name": found.name, "description": found.description, "version": found.version,
                                "icon": found.icon.as_deref().and_then(|path| icon(&plugin, path)),
                                "permissions": found.permissions, "hosts": found.host_permissions,
                            })
                        })
                        .collect();
                    // Some found and some refused: the list, with the words for the page to show beside it.
                    match refusal {
                        Some(why) => to_json(&json!({ "extensions": found, "refused": why })),
                        None => to_json(&found),
                    }
                })
            }
            ("webExtensions", "storePage") => {
                let page = text("url").and_then(|url| wings_web_extensions::store_package(&url));
                reply.done(to_json(&page.map(|page| {
                    let store = match page.store {
                        wings_web_extensions::Store::Chrome => "chrome",
                        wings_web_extensions::Store::Firefox => "firefox",
                    };
                    json!({ "store": store, "id": page.id })
                })))
            }
            ("webExtensions", "stage") => {
                let plugin = id.clone();
                match text("from").as_deref() {
                    Some("pick") => {
                        let this = self.clone();
                        access.env.web(
                            Job::Pick,
                            Box::new(move |picked| {
                                let path = picked.and_then(|picked| serde_json::from_str::<Option<PathBuf>>(&picked).map_err(|e| e.to_string()));
                                match path {
                                    Ok(Some(path)) => this.off_thread(reply, move |_| staged(&plugin, &profile, extensions::stage_path(&profile, &path))),
                                    Ok(None) => reply.ok("null".into()),
                                    Err(error) => reply.err(error),
                                }
                            }),
                        )
                    }
                    // One of the folders `installed` last found, by its place in that list: never a path the plugin names.
                    Some("browser") => {
                        let at = text("source").and_then(|source| source.parse::<usize>().ok());
                        let Some(path) = at.and_then(|at| FOUND.lock().unwrap().get(&plugin)?.get(at).cloned()) else {
                            return reply.err("That extension isn't one your other browsers have: look again".into());
                        };
                        self.off_thread(reply, move |_| staged(&plugin, &profile, extensions::stage_path(&profile, &path)))
                    }
                    Some("store") => {
                        let Some(package) = text("url").and_then(|url| wings_web_extensions::store_package(&url)) else {
                            return reply.err("That isn't an extension's page in the Chrome Web Store or on addons.mozilla.org".into());
                        };
                        self.off_thread(reply, move |_| {
                            let bytes = download(&package)?;
                            staged(&plugin, &profile, extensions::stage_bytes(&profile, &bytes))
                        })
                    }
                    _ => reply.err(r#"stage takes from: "pick", "browser" or "store""#.into()),
                }
            }
            ("webExtensions", "add") => {
                let Some(stage) = text("stage") else { return reply.err("add takes the stage".into()) };
                let (env, plugin) = (access.env.clone(), id.clone());
                // Answered once WebKit has it, with why it didn't load if it didn't, so not by `off_thread`.
                std::thread::spawn(move || {
                    let (added, _) = match extensions::prepare(&profile, &stage) {
                        Ok(added) => added,
                        Err(error) => return reply.err(error),
                    };
                    let entry = json!({
                        "id": added.id, "name": added.name, "version": added.version, "description": added.description,
                        "enabled": true, "permissions": added.permissions, "hosts": added.hosts, "error": null,
                        "icon": extensions::icon_path(&profile, &added).and_then(|path| icon(&plugin, &path)),
                    });
                    // Wings asks in its own dialog, which no click in the page answers, before it grants anything.
                    let ask = crate::web::Ask {
                        name: added.name.clone(),
                        permissions: added.permissions.clone(),
                        hosts: added.hosts.clone(),
                        compat: added.compat,
                        optional: added.optional.clone(),
                    };
                    let loader = env.clone();
                    env.web(
                        Job::Add { ask },
                        Box::new(move |answer| {
                            // Anything but the dialog's Add, an error on the way included, is not allowed.
                            if crate::web::allowed(&answer).is_none() {
                                extensions::forget(&profile, &added.id);
                                return reply.err(answer.err().unwrap_or_else(|| crate::web::NOT_ALLOWED.into()));
                            }
                            // What it may ask for later that the person allowed on the dialog's own button is had from
                            // the start, sealed with the rest.
                            let mut added = added;
                            let mut entry = entry;
                            for name in crate::web::also_allowed(&answer) {
                                if added.optional.contains(&name) && !added.permissions.contains(&name) {
                                    added.permissions.push(name);
                                }
                            }
                            entry["permissions"] = json!(added.permissions);
                            // Listed, sealed, only now that the person allowed it, and before it loads: a list that
                            // can't be written leaves nothing loaded. It then loads as any other does, checked first.
                            if let Err(error) = extensions::keep(&profile, &added, &|| extensions::key(&profile)) {
                                extensions::forget(&profile, &added.id);
                                return reply.err(error);
                            }
                            let verified = extensions::verified(&profile, &added.id, &|| extensions::key(&profile));
                            let Ok(extension) = verified else {
                                entry["error"] = json!(verified.err());
                                return reply.done(to_json(&entry));
                            };
                            loader.web(
                                Job::Load { profile, extension },
                                Box::new(move |loaded| {
                                    if let Err(error) = loaded {
                                        entry["error"] = json!(error);
                                    }
                                    reply.done(to_json(&entry));
                                }),
                            );
                        }),
                    );
                });
            }
            ("webExtensions", "discard") => {
                let Some(stage) = text("stage") else { return reply.err("discard takes the stage".into()) };
                self.off_thread(reply, move |_| {
                    extensions::discard(&profile, &stage);
                    Ok("null".into())
                })
            }
            ("webExtensions", "setEnabled") => {
                let (Some(ext), Some(enabled)) = (text("id"), params.get("enabled").and_then(Value::as_bool)) else {
                    return reply.err("setEnabled takes an id and enabled".into());
                };
                let env = access.env.clone();
                // Off the frame's thread, answered once WebKit has it: turning one on hashes its folder, and may wait on
                // the Keychain.
                std::thread::spawn(move || {
                    let job = match extensions::set_enabled(&profile, &ext, enabled, &|| extensions::key(&profile)) {
                        Ok(Some(extension)) => Job::Load { profile, extension },
                        Ok(None) => Job::Unload { profile, id: ext },
                        Err(error) => return reply.err(error),
                    };
                    env.web(job, done(reply))
                });
            }
            ("webExtensions", "remove") => {
                let Some(ext) = text("id") else { return reply.err("remove takes an id".into()) };
                if !extensions::list(&profile).iter().any(|added| added.id == ext) {
                    return reply.err("No extension by that id".into());
                }
                // What it stored goes first, while WebKit still knows it, then it unloads, then its folder and record.
                access.env.web(
                    Job::Remove { profile: profile.clone(), id: ext.clone() },
                    Box::new(move |removed| reply.done(removed.and_then(|_| extensions::remove(&profile, &ext).map(|()| "null".into())))),
                )
            }
            // Asks the Keychain again for the key extensions are checked with, after the person refused it, and loads the
            // extensions that are on. Only on a click: it may show the Keychain's prompt.
            ("webExtensions", "checkAgain") => {
                let env = access.env.clone();
                self.off_thread(reply, move |_| {
                    extensions::ask_for_key_again();
                    for extension in extensions::loadable(&profile, &|| extensions::key(&profile)) {
                        env.web(Job::Load { profile: profile.clone(), extension }, Box::new(|_| {}));
                    }
                    Ok("null".into())
                })
            }
            ("webEngine", "status") => {
                let (kind, installed, available) = (engine::choice(&profile), engine::bundle().is_some(), engine::available());
                let engine_name = if kind == engine::Kind::Chromium { "chromium" } else { "webkit" };
                reply.done(to_json(&json!({
                    "engine": engine_name, "installed": installed, "version": engine::PINNED.version,
                    "downloadBytes": engine::PINNED.size, "diskBytes": engine::PINNED.unpacked,
                    "available": available.is_ok(), "why": available.err(),
                    // Chromium's files failed their check as it started this run (`engine::verify_bundle`).
                    "problem": engine::failed_check().filter(|_| installed),
                })))
            }
            // Each step off the frame's thread: the page calls them in turn so it can say which it is on. The download
            // needs your click (`needs_click`), and only the profile it was for may install it, once.
            ("webEngine", "download") => self.off_thread(reply, move |_| {
                engine::download()?;
                *FETCHED_FOR.lock().unwrap() = Some(profile);
                Ok("null".into())
            }),
            ("webEngine", "verify") => self.off_thread(reply, |_| engine::verify().map(|()| "null".into())),
            // Installs what your click in this page downloaded and switches its web views to it: the end of the steps
            // that click started, which no plugin starts on its own.
            ("webEngine", "install") => {
                if FETCHED_FOR.lock().unwrap().take_if(|fetched| *fetched == profile).is_none() {
                    return reply.err(NOT_FETCHED.into());
                }
                let env = access.env.clone();
                self.off_thread(reply, move |_| {
                    engine::install()?;
                    let (switched, answer) = std::sync::mpsc::channel();
                    env.web(Job::Engine(profile, engine::Kind::Chromium), Box::new(move |done| drop(switched.send(done))));
                    answer.recv().unwrap_or_else(|_| Err("The Wings window didn't answer".into()))
                })
            }
            ("webEngine", "use") => {
                let kind = match text("engine").as_deref() {
                    Some("chromium") => engine::Kind::Chromium,
                    Some("webkit") => engine::Kind::WebKit,
                    _ => return reply.err(r#"use takes engine: "chromium" or "webkit""#.into()),
                };
                if kind == engine::Kind::Chromium {
                    if let Err(why) = engine::available() {
                        return reply.err(why.into());
                    }
                    if engine::bundle().is_none() {
                        return reply.err("Chromium isn't downloaded yet".into());
                    }
                }
                access.env.web(Job::Engine(profile, kind), done(reply))
            }
            // Every profile's Chromium data goes, so only the Browser asks, and only Wings' own dialog says yes: `true`
            // once it is gone, `false` when you cancel.
            ("webEngine", "remove") => {
                if !(access.builtin && id == "browser") {
                    return reply.err("Only the Browser can remove Chromium".into());
                }
                let (this, env) = (self.clone(), access.env.clone());
                access.env.web(
                    Job::Confirm(engine::REMOVE),
                    Box::new(move |answer| {
                        match answer.as_deref() {
                            Ok("true") => {}
                            Ok(_) => return reply.ok("false".into()),
                            Err(error) => return reply.err(error.clone()),
                        }
                        // The window switches to WebKit and places its pages again first, so nothing has Chromium's
                        // files open while they go; then the files go, off the window's thread.
                        env.web(
                            Job::Engine(profile, engine::Kind::WebKit),
                            Box::new(move |switched| match switched {
                                Ok(_) => this.off_thread(reply, |_| engine::remove().map(|()| "true".into())),
                                Err(error) => reply.err(error),
                            }),
                        )
                    }),
                )
            }
            ("browserCookies", "sources") => self.off_thread(reply, move |_| {
                let sources: Vec<Value> = wings_browser_cookies::detect()
                    .into_iter()
                    .map(|source| json!({ "id": source.id, "browser": source.browser_name, "profile": source.profile }))
                    .collect();
                to_json(&sources)
            }),
            ("browserCookies", "import") => {
                let Some(source) = text("source") else { return reply.err("import takes the source".into()) };
                let keep_data = params.get("keepData") != Some(&Value::Bool(false));
                // Imported sign-ins last until Wings quits unless the person keeps them, kept cookies or not.
                let keep_signins = params.get("keepSignIns") == Some(&Value::Bool(true));
                // Before anything is read, so no Keychain prompt asks for cookies that would have nowhere safe to go.
                if let Some(why) = crate::web::keeps_refusal(keep_data, per_plugin_stores()) {
                    return reply.err(why.into());
                }
                let env = access.env.clone();
                // Answered once WebKit has the cookies, so not by `off_thread`; the Keychain's prompt may keep it waiting.
                std::thread::spawn(move || {
                    let Some(found) = wings_browser_cookies::detect().into_iter().find(|found| found.id == source) else {
                        return reply.err("That browser profile isn't on this Mac any more".into());
                    };
                    // The values stay in memory until WebKit has them, and are never logged.
                    let read = match wings_browser_cookies::read(&found) {
                        Ok(read) => read,
                        Err(error) => return reply.err(error.to_string()),
                    };
                    let skipped = read.skipped;
                    env.web(
                        Job::Cookies { profile, keep_data, keep_signins, cookies: read.cookies },
                        Box::new(move |imported| {
                            reply.done(imported.map(|imported| {
                                let imported: u64 = imported.parse().unwrap_or(0);
                                json!({ "imported": imported, "skipped": skipped }).to_string()
                            }))
                        }),
                    );
                });
            }
            _ => reply.err(format!("{method} has no action {action:?}")),
        }
    }
}

/// Whether the call `method` with `params` waits for your click: picking, staging, adding, turning on or off and removing
/// an extension, clearing the site data, downloading, choosing or removing an engine and importing cookies happen only
/// as the answer to one, never by the plugin alone. Installing Chromium follows the click that downloaded it (`install`).
pub(in crate::page::plugin) fn needs_click(method: &str, params: &Value) -> bool {
    let action = params.get("action").and_then(Value::as_str);
    // Listing another browser's extensions reads its profiles, and checking again may show the Keychain's prompt: only
    // when the person asks, too.
    matches!(
        (method, action),
        ("webExtensions", Some("stage" | "add" | "installed" | "setEnabled" | "remove" | "checkAgain"))
            | ("webData", Some("clear" | "clearSite"))
            | ("webEngine", Some("download" | "use" | "remove"))
            | ("browserCookies", Some("import"))
    )
}

/// What tells copies of extensions in other browsers apart without their profiles' names: the browser's name, and
/// "(2)", "(3)" for its second and third profile.
fn copies<'a>(found: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<String> {
    let mut profiles: Vec<(&str, &str)> = Vec::new();
    found
        .map(|(browser, profile)| {
            if !profiles.contains(&(browser, profile)) {
                profiles.push((browser, profile));
            }
            let nth = profiles.iter().filter(|(of, _)| *of == browser).position(|(_, name)| *name == profile).unwrap_or(0) + 1;
            if nth == 1 { browser.to_owned() } else { format!("{browser} ({nth})") }
        })
        .collect()
}

/// Whether WebKit keeps a store for each plugin on this Mac (`web::keeps_refusal`).
fn per_plugin_stores() -> bool {
    #[cfg(all(feature = "ui", target_os = "macos"))]
    return crate::web::mac::keeps_per_plugin();
    #[cfg(not(all(feature = "ui", target_os = "macos")))]
    false
}

/// `staged`'s answer: the stage and what the extension asks for.
fn staged(plugin: &str, profile: &Profile, staged: Result<extensions::Staged, String>) -> Result<String, String> {
    let _ = profile;
    let staged = staged?;
    let info = staged.info;
    to_json(&json!({
        "stage": staged.stage, "name": info.name, "version": info.version, "description": info.description,
        "icon": info.icon.as_deref().and_then(|path| icon(plugin, path)),
        "permissions": info.permissions, "hosts": info.host_permissions,
    }))
}

/// Registers the picture at `path` for the plugin's `Image` nodes, by an id made of the path: its id.
fn icon(plugin: &str, path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok().filter(|bytes| bytes.len() < 1_000_000)?;
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    let id = format!("ext-{}", digest.iter().take(12).map(|byte| format!("{byte:02x}")).collect::<String>());
    #[cfg(feature = "ui")]
    crate::page::images::register_bytes(plugin, &id, bytes).ok()?;
    #[cfg(not(feature = "ui"))]
    let _ = (plugin, bytes);
    Some(id)
}

/// The store's package for `package`: the Chrome Web Store's update service redirects to it, and addons.mozilla.org's
/// API names its file. Only over https, from the hosts `store_package` and `amo_file_url` allow and the CDNs they send
/// to.
fn download(package: &wings_web_extensions::StorePackage) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .build()
        .into();
    let get = |url: &str, max: u64| -> Result<Vec<u8>, String> {
        let mut response = agent.get(url).call().map_err(|error| format!("The store didn't answer: {error}"))?;
        // The update service answers 204 for an extension it has no package of for this browser.
        if response.status() == 204 {
            return Err("The store has no package of that extension to download".into());
        }
        response.body_mut().with_config().limit(max).read_to_vec().map_err(|error| format!("The download failed: {error}"))
    };
    let url = match package.store {
        wings_web_extensions::Store::Chrome => package.download.clone(),
        wings_web_extensions::Store::Firefox => {
            let api = String::from_utf8(get(&package.download, 1_000_000)?).map_err(|error| error.to_string())?;
            wings_web_extensions::amo_file_url(&api).ok_or("addons.mozilla.org named no file for that add-on")?
        }
    };
    get(&url, PACKAGE_MAX)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// The page tells copies apart by browser and a number, never by the profiles' names, which are often the person's.
    #[test]
    fn other_browsers_copies_are_told_apart_without_their_profiles_names() {
        let found = [("Chrome", "Josip"), ("Chrome", "Josip"), ("Chrome", "Work"), ("Arc", "Default"), ("Chrome", "Josip")];
        assert_eq!(super::copies(found.into_iter()), ["Chrome", "Chrome", "Chrome (2)", "Arc", "Chrome"]);
    }

    use crate::{
        page::plugin::testing::{Caller, Rig},
        web::engine,
    };

    #[test]
    fn the_engine_call_needs_the_web_view_permission_and_a_click_to_download_choose_or_remove() {
        let without = Rig::new(json!({ "permissions": { "openUrl": ["https://"] } }), vec![]);
        let page = Caller::start(&without.host);
        assert_eq!(page.call("webEngine", json!({ "action": "status" })), Err("probe may not show web pages".into()));
        assert_eq!(page.call_after_click("webEngine", json!({ "action": "download" })), Err("probe may not show web pages".into()));

        let web = Rig::new(json!({ "permissions": { "webView": true } }), vec![]);
        let page = Caller::start(&web.host);
        for action in ["download", "remove"] {
            assert_eq!(
                page.call("webEngine", json!({ "action": action })),
                Err("A page can download or remove Chromium only right after you click or type in it".into()),
                "{action}"
            );
        }
        // Nor does a plugin switch its pages to an engine on its own, either way.
        for engine in ["chromium", "webkit"] {
            assert_eq!(
                page.call("webEngine", json!({ "action": "use", "engine": engine })),
                Err("A page can change the browser engine only right after you click or type in it".into()),
                "{engine}"
            );
        }
        let status = page.call("webEngine", json!({ "action": "status" })).unwrap();
        assert_eq!(status["version"], engine::PINNED.version);
        assert_eq!(status["downloadBytes"], engine::PINNED.size);
        assert_eq!(status["diskBytes"], engine::PINNED.unpacked);
        assert_eq!(status["engine"], "webkit");
        assert_eq!(
            page.call_after_click("webEngine", json!({ "action": "use", "engine": "firefox" })),
            Err(r#"use takes engine: "chromium" or "webkit""#.into())
        );
        assert_eq!(page.call("webEngine", json!({ "action": "nope" })), Err(r#"webEngine has no action "nope""#.into()));
        for action in [json!({ "action": "download" }), json!({ "action": "use", "engine": "webkit" }), json!({ "action": "remove" })] {
            assert!(super::needs_click("webEngine", &action), "{action}");
        }
        // Checking needs nothing, and installing follows the click that downloaded (below).
        for action in ["status", "verify", "install"] {
            assert!(!super::needs_click("webEngine", &json!({ "action": action })), "{action}");
        }
    }

    /// A plugin can't install Chromium and switch to it on its own: only the profile whose page's click downloaded it
    /// installs it, and a click alone isn't a download.
    #[test]
    fn chromium_installs_only_for_the_profile_whose_click_downloaded_it() {
        let web = Rig::new(json!({ "permissions": { "webView": true } }), vec![]);
        let page = Caller::start(&web.host);
        let refused = Err(super::NOT_FETCHED.to_owned());
        assert_eq!(page.call("webEngine", json!({ "action": "install" })), refused);
        assert_eq!(page.call_after_click("webEngine", json!({ "action": "install" })), refused);
        let other = crate::web::Profile::of("other", "main", std::path::Path::new("/nowhere/plugin-storage"));
        *super::FETCHED_FOR.lock().unwrap() = Some(other.clone());
        assert_eq!(page.call("webEngine", json!({ "action": "install" })), refused);
        // Still the other profile's to install.
        assert_eq!(super::FETCHED_FOR.lock().unwrap().take(), Some(other));
    }

    /// Removing deletes every profile's Chromium data, so only Wings' own Browser asks, after a click, and Wings asks the
    /// person in its own dialog first: cancelled, nothing goes.
    #[test]
    fn only_the_browser_removes_chromium_and_only_once_you_say_so_in_wings_dialog() {
        use crate::page::plugin::testing::{Seen, plugin};
        let asked = |rig: &Rig| rig.env.seen().into_iter().any(|seen| matches!(seen, Seen::Confirm(..)));
        let manifest = |id: &str| plugin(json!({ "id": id, "permissions": { "webView": true } }));
        for (rig, what) in [(Rig::with(manifest("probe"), true, vec![]), "another of Wings' plugins"), (Rig::with(manifest("browser"), false, vec![]), "a plugin named browser")] {
            let page = Caller::start(&rig.host);
            assert_eq!(page.call_after_click("webEngine", json!({ "action": "remove" })), Err("Only the Browser can remove Chromium".into()), "{what}");
            assert!(!asked(&rig), "{what}");
        }
        let browser = Rig::with(manifest("browser"), true, vec![]);
        let page = Caller::start(&browser.host);
        assert_eq!(
            page.call("webEngine", json!({ "action": "remove" })),
            Err("A page can download or remove Chromium only right after you click or type in it".into())
        );
        assert!(!asked(&browser));
        assert_eq!(page.call_after_click("webEngine", json!({ "action": "remove" })), Ok(json!(false)));
        let words = Seen::Confirm(engine::REMOVE.question.into(), engine::REMOVE.detail.into());
        assert!(browser.env.seen().contains(&words), "{:?}", browser.env.seen());
        assert!(engine::REMOVE.detail.contains("deletes Chromium and its sign-ins and site data for every profile"));
    }

    #[test]
    fn clearing_one_site_needs_a_click_and_the_address_of_a_web_page() {
        let web = Rig::new(json!({ "permissions": { "webView": true } }), vec![]);
        let page = Caller::start(&web.host);
        let params = json!({ "action": "clearSite", "url": "http://localhost:4300/app" });
        assert_eq!(page.call("webData", params.clone()), Err("A page can clear site data only right after you click or type in it".into()));
        // Not a web page: nothing to clear, so nothing reaches the window.
        assert_eq!(page.call_after_click("webData", json!({ "action": "clearSite", "url": "about:blank" })), Err("clearSite needs the address of a web page".into()));
        assert_eq!(page.call_after_click("webData", json!({ "action": "clearSite" })), Err("clearSite needs the address of a web page".into()));
        // A web page goes on to the window, which this host doesn't have.
        assert_eq!(page.call_after_click("webData", params), Err("Web pages aren't available in this host".into()));
        let without = Rig::new(json!({ "permissions": { "openUrl": ["https://"] } }), vec![]);
        assert_eq!(Caller::start(&without.host).call_after_click("webData", json!({ "action": "clearSite", "url": "https://x.com" })), Err("probe may not show web pages".into()));
    }

    #[test]
    fn web_calls_need_their_permission_and_adding_or_importing_needs_a_click() {
        let without = Rig::new(json!({ "permissions": { "openUrl": ["https://"] } }), vec![]);
        let page = Caller::start(&without.host);
        assert_eq!(page.call_after_click("webData", json!({ "action": "clear" })), Err("probe may not show web pages".into()));
        assert_eq!(page.call("webExtensions", json!({ "action": "list" })), Err("probe may not show web pages".into()));

        let web = Rig::new(json!({ "permissions": { "webView": true } }), vec![]);
        let page = Caller::start(&web.host);
        // Showing pages is not reading another browser's cookies.
        assert_eq!(
            page.call_after_click("browserCookies", json!({ "action": "import", "source": "chrome:Default" })),
            Err("probe may not import another browser's cookies".into())
        );
        assert_eq!(
            page.call("webExtensions", json!({ "action": "stage", "from": "pick" })),
            Err("A page can add an extension only right after you click or type in it".into())
        );
        assert_eq!(
            page.call("webExtensions", json!({ "action": "add", "stage": "abc" })),
            Err("A page can add an extension only right after you click or type in it".into())
        );
        assert_eq!(
            page.call("webExtensions", json!({ "action": "setEnabled", "id": "abc", "enabled": true })),
            Err("A page can add an extension only right after you click or type in it".into())
        );
        assert_eq!(
            page.call("webExtensions", json!({ "action": "installed" })),
            Err("A page can add an extension only right after you click or type in it".into())
        );
        // Nor does a plugin remove an extension, wipe the site data or ask the Keychain again on its own.
        assert_eq!(
            page.call("webExtensions", json!({ "action": "remove", "id": "abc" })),
            Err("A page can remove an extension only right after you click or type in it".into())
        );
        assert_eq!(page.call("webData", json!({ "action": "clear" })), Err("A page can clear site data only right after you click or type in it".into()));
        assert_eq!(
            page.call("webExtensions", json!({ "action": "checkAgain" })),
            Err("A page can add an extension only right after you click or type in it".into())
        );
        assert_eq!(page.call_after_click("webExtensions", json!({ "action": "remove", "id": "abc" })), Err("No extension by that id".into()));
        // A path is never a source: only a place in the list `installed` gave.
        assert_eq!(
            page.call_after_click("webExtensions", json!({ "action": "stage", "from": "browser", "source": "/Users/me/Library/Application Support/Google/Chrome/Default/Extensions/x" })),
            Err("That extension isn't one your other browsers have: look again".into())
        );
        assert_eq!(
            page.call("webExtensions", json!({ "action": "storePage", "url": "https://chromewebstore.google.com/detail/x/cjpalhdlnbpafiamejdnhcphjbkeiagm" })),
            Ok(json!({ "store": "chrome", "id": "cjpalhdlnbpafiamejdnhcphjbkeiagm" }))
        );
        assert_eq!(page.call("webExtensions", json!({ "action": "storePage", "url": "https://example.com/" })), Ok(json!(null)));

        let cookies = Rig::new(json!({ "permissions": { "webView": true, "browserCookies": true } }), vec![]);
        let page = Caller::start(&cookies.host);
        assert_eq!(
            page.call("browserCookies", json!({ "action": "import", "source": "chrome:Default" })),
            Err("A page can import cookies only right after you click or type in it".into())
        );
        // A source that isn't one `sources` lists reads nothing.
        assert_eq!(
            page.call_after_click("browserCookies", json!({ "action": "import", "source": "../../etc", "keepData": false })),
            Err("That browser profile isn't on this Mac any more".into())
        );
        // Kept cookies go only to a store of the plugin's own, which a Mac before 14 hasn't: refused before anything is read.
        let kept = page.call_after_click("browserCookies", json!({ "action": "import", "source": "../../etc" }));
        match super::per_plugin_stores() {
            true => assert_eq!(kept, Err("That browser profile isn't on this Mac any more".into())),
            false => assert_eq!(kept, Err(crate::web::keeps_refusal(true, false).unwrap().into())),
        }
    }
}
