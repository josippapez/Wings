//! The plugins installed and turned on, found and started for the native app.
//!
//! The rules are the desktop's, because the code is: `wings-plugins` reads the desktop's plugin folder and its
//! `plugins.json` (which plugins are on, and whether their permissions are approved), so a plugin loads here exactly
//! when it would there. Finding them only reads: installing, turning on, approving and removing go through the
//! window's `Store` (`workspace/plugin_manage.rs`), which writes `plugins.json` as the desktop does. A plugin shares the
//! desktop's data too: its main and pages get the desktop's storage root and the install scope `plugins.json` names,
//! so what the desktop saved for it is what it reads.
//!
//! A plugin runs here only if it ships a native build, which sits beside its web one in `dist/native/`: `main.js` is
//! its main, and a view that opens as a tab and sets `newTab` (what the desktop's + menu lists) has a page at
//! `<page>.js` for the page file its manifest names, `dist/board.html` being `board.js`; so does each of its sidebars
//! (`contributes.sidebars`), which opens in the right-hand panel. A plugin without `dist/native/main.js` isn't ported
//! yet and is skipped without a word. A plugin built since its pages share one React (`runtime::Library`) also has
//! `dist/native/shared.js` and each page again in `dist/native/shared/`, which run in its place (`page_source`).
//!
//! A debug build can also run the plugins of a folder it is given (`--plugins-dev-root`), as the desktop's debug builds
//! run the repo's `plugins/`: they are always on and approved, and one the desktop has never run stores what it saves
//! under the scope `dev`, since this never writes `plugins.json` to give it one.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use wings_plugins::{loading, manifest::Plugin, store::Store};

use crate::{
    host::PluginPaths,
    page::{
        plugin::{Access, Env, PluginConfig, PluginHost},
        runtime::{Event, Library},
    },
};

/// The Wings a plugin's `minWingsVersion` is checked against: the desktop's version, read from its `Cargo.toml` when this
/// crate is built (see `build.rs`). A plugin's minimum is a promise about the desktop release it was written for, so this
/// crate's own version would be wrong, and a constant here would need an edit with every desktop release.
pub const WINGS_VERSION: &str = env!("WINGS_DESKTOP_VERSION");

/// The folder of a plugin's native build.
const NATIVE_DIR: &str = "dist/native";

/// The scope of a dev plugin the desktop has no record of.
pub const DEV_SCOPE: &str = "dev";

/// A plugin the desktop runs and that has a native build, before it is started.
#[derive(Clone)]
pub struct Found {
    pub id: String,
    pub name: String,
    /// The install scope its storage lives under, from `plugins.json`.
    pub scope: String,
    pub main: PathBuf,
    pub pages: Vec<FoundPage>,
    /// Its pages for the right-hand sidebar, which open from a title bar button.
    pub sidebars: Vec<FoundPage>,
    /// Its page for Wings Settings, which `wings.openSettings` shows.
    pub settings: Option<FoundPage>,
    /// The plugin as the desktop loads it: what every call it makes is checked against.
    pub plugin: Plugin,
    /// One of Wings' own plugins.
    pub builtin: bool,
}

impl std::fmt::Debug for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Found")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("scope", &self.scope)
            .field("main", &self.main)
            .field("pages", &self.pages)
            .field("sidebars", &self.sidebars)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundPage {
    /// The view's id in the manifest.
    pub id: String,
    pub title: String,
    pub bundle: PathBuf,
    /// Opening it again opens another tab, instead of showing the one in this project.
    pub multiple: bool,
    /// Offered as a new tab. A view without it opens only through `wings.openView`.
    pub new_tab: bool,
}

/// A plugin that is running: its main, and the pages it can open.
pub struct Installed {
    pub id: String,
    pub name: String,
    pub host: Arc<PluginHost>,
    pub pages: Vec<Page>,
    /// Its right-hand sidebar pages.
    pub sidebars: Vec<Sidebar>,
    /// Its page for Wings Settings.
    pub settings: Option<Sidebar>,
    /// The plugin as the desktop loads it, with its icon's SVG.
    pub plugin: Plugin,
    /// One of Wings' own plugins.
    pub builtin: bool,
}

/// A page of the right-hand sidebar.
pub struct Sidebar {
    pub id: String,
    pub title: String,
    pub source: Arc<str>,
}

pub struct Page {
    pub id: String,
    pub title: String,
    pub multiple: bool,
    pub new_tab: bool,
    /// The page's bundle, read when the plugin started so that it and the main are of one version.
    pub source: Arc<str>,
}

/// `dist/native/<name>.js` of the plugin in `dir`, if that file is there and inside the plugin folder.
fn native_file(dir: &Path, name: &str) -> Option<PathBuf> {
    loading::resolve(dir, &format!("{NATIVE_DIR}/{name}.js")).filter(|file| file.is_file())
}

/// The plugin's `dist/native/shared.js`: the React and `wings-plugin-ui` its pages built to share them run on.
pub fn library(dir: &Path) -> Option<Arc<Library>> {
    Some(Library::new(fs::read_to_string(native_file(dir, "shared")?).ok()?))
}

/// The source of the page whose own build is `bundle` (`dist/native/<page>.js`) in the plugin in `dir`: its build that
/// shares `library`, `dist/native/shared/<page>.js`, if it was built with that library, and otherwise `bundle`, which
/// carries its own React, as every page did before.
pub fn page_source(dir: &Path, bundle: &Path, library: Option<&Library>) -> std::io::Result<String> {
    // A page built to WebAssembly, `dist/native/<page>.wasm`, runs in place of its JS build (`runtime::wasm`).
    let module = bundle.with_extension("wasm");
    if module.is_file() {
        return Ok(format!("{}{}", crate::page::runtime::wasm::MARKER, module.display()));
    }
    let shared = library.and_then(|library| {
        let stem = bundle.file_stem()?.to_str()?;
        let file = native_file(dir, &format!("shared/{stem}"))?;
        let source = fs::read_to_string(file).ok()?;
        (Library::needed_by(&source) == Some(library.hash())).then_some(source)
    });
    match shared {
        Some(source) => Ok(source),
        None => fs::read_to_string(bundle),
    }
}

/// The plugins that are installed, turned on, approved and have a native build, by name. Plugins the desktop skips
/// (too new for it, broken, not built) never show up, as `wings-plugins` skips them while loading.
pub fn discover(paths: &PluginPaths) -> Vec<Found> {
    discover_in(&load_store(paths))
}

/// The plugin store at `paths`, read with this Wings' version, which the window keeps and changes.
pub fn load_store(paths: &PluginPaths) -> Store {
    wings_plugins::set_wings_version(WINGS_VERSION);
    Store::load(paths.dir.clone(), paths.state.clone(), paths.dev.as_deref())
}

/// `discover`, of a store already loaded. It never writes `plugins.json`.
pub fn discover_in(store: &Store) -> Vec<Found> {
    store
        .list()
        .into_iter()
        .filter_map(|listed| {
            let id = &listed.manifest.id;
            let plugin = store.active(id)?;
            let main = native_file(&plugin.dir, "main")?;
            // A dev plugin the desktop has never run has no scope on file, and this never writes one.
            let scope = store
                .known_storage_scope(id)
                .or_else(|| listed.dev.then(|| DEV_SCOPE.to_owned()))?;
            let bundle = |page: &str| {
                let stem = Path::new(page).file_stem()?.to_str()?;
                native_file(&plugin.dir, stem)
            };
            let contributes = &plugin.manifest.contributes;
            let pages = contributes
                .views
                .iter()
                .filter(|view| view.opens.iter().any(|place| place == "tab"))
                .filter_map(|view| {
                    Some(FoundPage {
                        id: view.id.clone(),
                        title: view.title.clone(),
                        bundle: bundle(&view.page)?,
                        multiple: view.multiple,
                        new_tab: view.new_tab,
                    })
                })
                .collect();
            let sidebars = contributes
                .sidebars
                .iter()
                .filter_map(|sidebar| {
                    Some(FoundPage {
                        id: sidebar.id.clone(),
                        title: sidebar.title.clone(),
                        bundle: bundle(&sidebar.page)?,
                        multiple: false,
                        new_tab: false,
                    })
                })
                .collect();
            let settings = contributes.settings.as_ref().and_then(|settings| {
                Some(FoundPage {
                    id: "settings".into(),
                    title: format!("{} settings", plugin.manifest.name),
                    bundle: bundle(&settings.page)?,
                    multiple: false,
                    new_tab: false,
                })
            });
            Some(Found {
                id: id.clone(),
                name: plugin.manifest.name.clone(),
                scope,
                main,
                pages,
                sidebars,
                settings,
                builtin: store.is_builtin(id),
                plugin,
            })
        })
        .collect()
}

/// Starts each plugin's main with the desktop's storage root and its scope, and gives its frames `env` to ask for what
/// its manifest grants. A plugin that can't be read or started is skipped with a line on stderr, and so is a page that
/// can't be read.
pub fn start(found: Vec<Found>, paths: &PluginPaths, env: &Arc<dyn Env>) -> Vec<Installed> {
    found
        .into_iter()
        .filter_map(|found| {
            let id = found.id;
            let dir = &found.plugin.dir;
            let shared = library(dir);
            let read = |file: &Path| {
                fs::read_to_string(file)
                    .inspect_err(|error| eprintln!("[plugins] {id}: {}: {error}", file.display()))
                    .ok()
            };
            let read_page = |bundle: &Path| {
                page_source(dir, bundle, shared.as_deref())
                    .inspect_err(|error| eprintln!("[plugins] {id}: {}: {error}", bundle.display()))
                    .ok()
            };
            let main = read(&found.main)?;
            let host = Arc::new(PluginHost::with_access(
                PluginConfig {
                    id: id.clone(),
                    root: paths.storage.clone(),
                    scope: found.scope,
                },
                Access {
                    plugin: found.plugin.clone(),
                    builtin: found.builtin,
                    env: env.clone(),
                },
            ));
            host.set_library(shared.clone());
            let reported = id.clone();
            host.keep_main(
                main.into(),
                Arc::new(move |event| {
                    if let Event::Error { message, .. } = event {
                        eprintln!("[plugins] {reported}: {message}");
                    }
                }),
            )
            .inspect_err(|error| eprintln!("[plugins] {id}: main didn't start: {error}"))
            .ok()?;
            let pages = found
                .pages
                .iter()
                .filter_map(|page| {
                    Some(Page {
                        id: page.id.clone(),
                        title: page.title.clone(),
                        multiple: page.multiple,
                        new_tab: page.new_tab,
                        source: read_page(&page.bundle)?.into(),
                    })
                })
                .collect();
            let sidebars = found
                .sidebars
                .iter()
                .filter_map(|sidebar| {
                    Some(Sidebar {
                        id: sidebar.id.clone(),
                        title: sidebar.title.clone(),
                        source: read_page(&sidebar.bundle)?.into(),
                    })
                })
                .collect();
            let settings = found.settings.as_ref().and_then(|settings| {
                Some(Sidebar {
                    id: settings.id.clone(),
                    title: settings.title.clone(),
                    source: read_page(&settings.bundle)?.into(),
                })
            });
            Some(Installed {
                id,
                name: found.name,
                host,
                pages,
                sidebars,
                settings,
                plugin: found.plugin,
                builtin: found.builtin,
            })
        })
        .collect()
}

/// A page of a running plugin that can be placed on a project's page, with the plugin that runs it.
#[derive(Clone)]
pub struct BlockPage {
    pub plugin: String,
    pub id: String,
    pub title: String,
    /// The icon its manifest names, and its plugin's own SVG, drawn by `icons::plugin_glyph`.
    pub icon: String,
    pub icon_svg: Option<String>,
    /// The least height in points, within the manifest's range.
    pub min_height: u32,
    pub host: Arc<PluginHost>,
    pub source: Arc<str>,
}

/// The blocks of `installed` that have a native build (`dist/native/<page>.js`), read now.
pub fn blocks(installed: &Installed) -> Vec<BlockPage> {
    let (least, most) = wings_plugins::manifest::BLOCK_MIN_HEIGHT;
    installed
        .plugin
        .manifest
        .contributes
        .blocks
        .iter()
        .filter_map(|block| {
            let stem = Path::new(&block.page).file_stem()?.to_str()?;
            let file = native_file(&installed.plugin.dir, stem)?;
            Some(BlockPage {
                plugin: installed.id.clone(),
                id: block.id.clone(),
                title: block.title.clone(),
                icon: block.icon.clone(),
                icon_svg: installed.plugin.icon_svg.clone(),
                min_height: block.min_height.unwrap_or(160).clamp(least, most),
                host: installed.host.clone(),
                source: page_source(&installed.plugin.dir, &file, installed.host.library().as_deref())
                    .ok()?
                    .into(),
            })
        })
        .collect()
}

/// The popover page of `installed`'s sidebar `sidebar`, shown under its title bar button: its native build
/// (`dist/native/<page>.js`), read now, and its size in points.
pub fn popover(installed: &Installed, sidebar: &str) -> Option<(Arc<str>, u32, u32)> {
    let contributes = &installed.plugin.manifest.contributes;
    let popover = contributes
        .sidebars
        .iter()
        .find(|page| page.id == sidebar)?
        .popover
        .as_ref()?;
    let stem = Path::new(&popover.page).file_stem()?.to_str()?;
    let file = native_file(&installed.plugin.dir, stem)?;
    let source =
        page_source(&installed.plugin.dir, &file, installed.host.library().as_deref()).ok()?;
    Some((source.into(), popover.width, popover.height))
}

/// Installed plugins on disk for tests and the screenshot example.
#[cfg(any(test, feature = "test-support"))]
pub mod fixture;
#[cfg(test)]
mod tests;
