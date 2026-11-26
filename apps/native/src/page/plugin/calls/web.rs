//! `webData`, `webExtensions` and `browserCookies`: what a plugin that shows web pages (`permissions.webView`) may do
//! with them beyond its pages' `WebView` nodes. What reads the disk or the network runs off the frame's thread; what
//! WebKit does runs on the window's (`Env::web`).

use std::{path::PathBuf, sync::Arc};

use serde_json::{Value, json};
use wings_plugins::permissions::{may_import_browser_cookies, may_show_web_views};

use super::{Shared, to_json};
use crate::{
    page::{plugin::env::Access, runtime::Reply},
    web::{Job, Profile, extensions},
};

/// The folders of the extensions `installed` last found for each plugin, in the order it listed them.
static FOUND: std::sync::Mutex<std::collections::BTreeMap<String, Vec<PathBuf>>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// The most bytes of a store's package: what `wings_web_extensions::unpack` takes at most, unpacked.
const PACKAGE_MAX: u64 = wings_web_extensions::MAX_TOTAL_BYTES;

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
            ("webExtensions", "list") => {
                let (env, plugin) = (access.env.clone(), id.clone());
                access.env.web(
                    Job::Supported,
                    Box::new(move |supported| {
                        let supported = supported.as_deref() == Ok("true");
                        let list: Vec<Value> = extensions::list(&profile)
                            .iter()
                            .map(|added| {
                                let icon = extensions::icon_path(&profile, added).and_then(|path| icon(&plugin, &path));
                                json!({
                                    "id": added.id, "name": added.name, "version": added.version,
                                    "description": added.description, "enabled": added.enabled, "icon": icon,
                                    "permissions": added.permissions, "hosts": added.hosts, "error": null,
                                })
                            })
                            .collect();
                        drop(env);
                        // `keepsCookies`: whether Keep cookies keeps them past this run here (macOS 14, `web::keeps_refusal`).
                        let keeps = per_plugin_stores();
                        reply.done(to_json(&json!({ "supported": supported, "keepsCookies": keeps, "extensions": list })));
                    }),
                )
            }
            ("webExtensions", "installed") => {
                let plugin = id.clone();
                self.off_thread(reply, move |_| {
                    let installed = wings_web_extensions::detect_installed();
                    // The page gets each by its place in this list, never its folder, which names the person's profiles.
                    FOUND.lock().unwrap().insert(plugin.clone(), installed.iter().map(|found| found.source.clone()).collect());
                    let found: Vec<Value> = installed
                        .into_iter()
                        .enumerate()
                        .map(|(at, found)| {
                            json!({
                                "source": at.to_string(), "browser": found.browser_name, "profile": found.profile,
                                "name": found.name, "description": found.description, "version": found.version,
                                "icon": found.icon.as_deref().and_then(|path| icon(&plugin, path)),
                                "permissions": found.permissions, "hosts": found.host_permissions,
                            })
                        })
                        .collect();
                    to_json(&found)
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
                    let (added, dir) = match extensions::add(&profile, &stage) {
                        Ok(added) => added,
                        Err(error) => return reply.err(error),
                    };
                    let entry = json!({
                        "id": added.id, "name": added.name, "version": added.version, "description": added.description,
                        "enabled": true, "permissions": added.permissions, "hosts": added.hosts, "error": null,
                        "icon": extensions::icon_path(&profile, &added).and_then(|path| icon(&plugin, &path)),
                    });
                    // Wings asks in its own dialog, which no click in the page answers, before it grants anything.
                    let ask = crate::web::Ask { name: added.name.clone(), permissions: added.permissions.clone(), hosts: added.hosts.clone() };
                    let id = added.id.clone();
                    env.web(
                        Job::Add { profile: profile.clone(), id: added.id, dir, ask },
                        Box::new(move |loaded| match loaded {
                            Err(error) if error == crate::web::NOT_ALLOWED => {
                                let _ = extensions::remove(&profile, &id);
                                reply.err(error);
                            }
                            loaded => {
                                let mut entry = entry;
                                if let Err(error) = loaded {
                                    entry["error"] = json!(error);
                                }
                                reply.done(to_json(&entry));
                            }
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
                let (added, dir) = match extensions::set_enabled(&profile, &ext, enabled) {
                    Ok(done) => done,
                    Err(error) => return reply.err(error),
                };
                let job = if enabled { Job::Load { profile, id: added.id, dir } } else { Job::Unload { profile, id: added.id } };
                access.env.web(job, done(reply))
            }
            ("webExtensions", "remove") => {
                let Some(ext) = text("id") else { return reply.err("remove takes an id".into()) };
                if let Err(error) = extensions::remove(&profile, &ext) {
                    return reply.err(error);
                }
                access.env.web(Job::Unload { profile, id: ext }, done(reply))
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
                        Job::Cookies { profile, keep_data, cookies: read.cookies },
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

/// Whether the call `method` with `params` waits for your click: picking, staging and adding an extension and importing
/// cookies happen only as the answer to one, never by the plugin alone.
pub(in crate::page::plugin) fn needs_click(method: &str, params: &Value) -> bool {
    let action = params.get("action").and_then(Value::as_str);
    // Listing another browser's extensions reads its profiles: only when the person asks, too.
    matches!((method, action), ("webExtensions", Some("stage" | "add" | "installed")) | ("browserCookies", Some("import")))
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

    use crate::page::plugin::testing::{Caller, Rig};

    #[test]
    fn web_calls_need_their_permission_and_adding_or_importing_needs_a_click() {
        let without = Rig::new(json!({ "permissions": { "openUrl": ["https://"] } }), vec![]);
        let page = Caller::start(&without.host);
        assert_eq!(page.call("webData", json!({ "action": "clear" })), Err("probe may not show web pages".into()));
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
            page.call("webExtensions", json!({ "action": "installed" })),
            Err("A page can add an extension only right after you click or type in it".into())
        );
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
