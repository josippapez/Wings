//! The desktop's plugin folders in a temp dir, for tests and the screenshot scenes: plugins installed from `pages/fixture`
//! the way the desktop installs them, and a `plugins.json` that turns them on.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{Value, json};
use wings_plugins::manifest::Manifest;

use crate::{
    host::PluginPaths,
    page::runtime::{
        Store,
        fake::{self, Cx, Guest},
        wasm::MARKER,
    },
};

/// A text node.
fn text_node(id: usize, text: &str) -> Value {
    json!({ "id": id, "type": "#text", "text": text })
}

/// The fixture plugin's page: the count its main keeps, and a button that asks main to add one.
pub fn fixture_page() -> String {
    struct Page(Option<u64>);
    impl Page {
        fn draw(&self, cx: &mut Cx<'_>) {
            let count = self
                .0
                .map_or("Loading…".to_owned(), |count| format!("Count: {count}"));
            cx.render(&json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
                { "id": 15, "type": "Page", "props": {}, "children": [
                    { "id": 7, "type": "Slot", "props": { "name": "toolbar" }, "children": [
                        { "id": 6, "type": "Toolbar", "props": {}, "children": [
                            { "id": 2, "type": "Slot", "props": { "name": "title" }, "children": [text_node(1, "Fixture")] },
                            { "id": 5, "type": "Slot", "props": { "name": "actions" }, "children": [
                                { "id": 4, "type": "Badge", "props": { "tone": "success" }, "children": [text_node(3, "Plugin")] }] }] }] },
                    { "id": 14, "type": "Slot", "props": { "name": "body" }, "children": [
                        { "id": 13, "type": "Stack", "props": { "gap": "l" }, "children": [
                            { "id": 12, "type": "Row", "props": { "gap": "m" }, "children": [
                                { "id": 9, "type": "Button", "props": { "onClick": "9:onClick" }, "children": [text_node(8, "Increment")] },
                                { "id": 11, "type": "Text", "props": { "weight": "medium" }, "children": [text_node(10, &count)] }] }] }] }] }] }));
        }
    }
    impl Guest for Page {
        fn start(&mut self, cx: &mut Cx<'_>) {
            self.draw(cx);
            cx.link()
                .broadcast(json!({ "ask": "count" }).to_string())
                .unwrap();
        }
        fn event(&mut self, cx: &mut Cx<'_>, _handler: &str, _payload: &Value) {
            cx.link()
                .broadcast(json!({ "ask": "increment" }).to_string())
                .unwrap();
        }
        fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, text: &str) {
            let heard: Value = serde_json::from_str(text).unwrap_or_default();
            if let (true, Some(count)) = (kind == "broadcast", heard["count"].as_u64()) {
                self.0 = Some(count);
                self.draw(cx);
            }
        }
    }
    fake::source("page", || Box::new(Page(None)))
}

/// The fixture plugin's main: a count kept in storage, which it broadcasts as it starts, when asked for it, and
/// when asked to add one to it.
pub fn fixture_main() -> String {
    struct Main;
    impl Main {
        fn send(cx: &mut Cx<'_>, add: bool) {
            let link = cx.link();
            let saved: Value =
                serde_json::from_str(&link.storage(Store::Get("count".into())).unwrap()).unwrap();
            let mut count = saved.as_u64().unwrap_or(0);
            if add {
                count += 1;
                link.storage(Store::Set("count".into(), count.to_string()))
                    .unwrap();
            }
            link.broadcast(json!({ "count": count }).to_string())
                .unwrap();
        }
    }
    impl Guest for Main {
        // A page that asked before main was up went unheard, as a broadcast reaches only the frames running then.
        // The SDK's main says it's up for the same reason.
        fn start(&mut self, cx: &mut Cx<'_>) {
            Self::send(cx, false);
        }
        fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, text: &str) {
            let heard: Value = serde_json::from_str(text).unwrap_or_default();
            if kind == "broadcast" && !heard["ask"].is_null() {
                Self::send(cx, heard["ask"] == "increment");
            }
        }
    }
    fake::source("main", || Box::new(Main))
}

/// Puts the frame `source` in `native` as a build is: `<stem>.wasm` for a stand-in, `<stem>.js` for a script.
fn put_frame(native: &Path, stem: &str, source: &str) {
    match source.strip_prefix(MARKER) {
        Some(stand_in) => fs::copy(stand_in, native.join(format!("{stem}.wasm"))).map(drop),
        None => fs::write(native.join(format!("{stem}.js")), source),
    }
    .unwrap();
}

const GAUGE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m12 14 4-4" /><path d="M3.34 19a10 10 0 1 1 17.32 0" /></svg>"#;

/// What the user did to a plugin in the desktop's manager.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// Approved what its manifest asks for now.
    Current,
    /// Approved an older version's permissions, which an update has since changed.
    Stale,
    Never,
}

/// A data folder laid out like the desktop's, removed when dropped.
pub struct Data {
    pub root: PathBuf,
    entries: std::cell::RefCell<serde_json::Map<String, serde_json::Value>>,
}

impl Data {
    pub fn temp() -> Self {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "wings-native-installed-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        wings_plugins::set_wings_version(super::WINGS_VERSION);
        Self {
            root,
            entries: Default::default(),
        }
    }

    pub fn paths(&self) -> PluginPaths {
        PluginPaths::under(&self.root)
    }

    /// Installs the fixture plugin as `id`, which asks for the desktop's own version or newer, `min_wings`. It has three
    /// views that open as tabs: `fixture` and `later` set `newTab`, `plain` doesn't. Its web build is stand-in files, and
    /// with `native`, its native build is the fixture's main and page for `fixture` and a page for `plain`, but none for
    /// `later`. Then `plugins.json` records the choice, with `scope` as its storage scope.
    pub fn install(
        &self,
        id: &str,
        min_wings: &str,
        native: bool,
        enabled: bool,
        approval: Approval,
        scope: &str,
    ) -> PathBuf {
        let dir = self.paths().dir.join(id);
        fs::create_dir_all(dir.join("dist/native")).unwrap();
        let view = |id: &str, new_tab: bool| {
            json!({ "id": id, "title": id[..1].to_uppercase() + &id[1..], "page": format!("dist/{id}.html"),
                    "opens": ["tab"], "newTab": new_tab })
        };
        let manifest = json!({
            "id": id, "name": format!("Plugin {id}"), "version": "1.0.0", "minWingsVersion": min_wings, "api": 1,
            "main": "dist/main.js",
            "contributes": { "views": [view("fixture", true), view("plain", false), view("later", true)] },
        });
        fs::write(
            dir.join("wings-plugin.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        for file in ["main.js", "fixture.html", "plain.html", "later.html"] {
            fs::write(dir.join("dist").join(file), "").unwrap();
        }
        if native {
            let native = dir.join("dist/native");
            put_frame(&native, "main", &fixture_main());
            put_frame(&native, "fixture", &fixture_page());
            put_frame(&native, "plain", &fixture_page());
        }
        let approved = match approval {
            Approval::Never => serde_json::Value::Null,
            Approval::Current | Approval::Stale => {
                // Read as a manifest, not loaded: a plugin the desktop refuses (too new) still has a grant on file.
                let manifest: Manifest = serde_json::from_value(manifest).unwrap();
                let mut permissions = serde_json::to_value(&manifest.permissions).unwrap();
                if approval == Approval::Stale {
                    permissions["exec"] = json!(["git status"]);
                }
                json!({ "permissions": permissions, "contributes": manifest.contributes })
            }
        };
        let mut entries = self.entries.borrow_mut();
        entries.insert(
            id.into(),
            json!({ "enabled": enabled, "approved": approved, "source": { "kind": "file" }, "secrets": [], "scope": scope }),
        );
        fs::write(
            &self.paths().state,
            serde_json::to_vec_pretty(&*entries).unwrap(),
        )
        .unwrap();
        dir
    }

    /// Installs a plugin the way `install` does, but of the manifest `manifest` (over a default of its `id`, `name`,
    /// `version`, `api` and `main`) and with the native main `main` and the native page `source` for each
    /// `(file stem, source)` of `pages`, which the manifest's sidebars and views name as `dist/<stem>.html`. It is turned
    /// on, with what its manifest asks for approved, and `scope` is its storage scope.
    pub fn install_script(
        &self,
        id: &str,
        manifest: serde_json::Value,
        main: &str,
        pages: &[(&str, &str)],
        scope: &str,
    ) -> PathBuf {
        let dir = self.paths().dir.join(id);
        fs::create_dir_all(dir.join("dist/native")).unwrap();
        let mut whole = json!({ "id": id, "name": format!("Plugin {id}"), "version": "1.0.0", "api": 1, "main": "dist/main.js" });
        whole
            .as_object_mut()
            .unwrap()
            .extend(manifest.as_object().unwrap().clone());
        fs::write(
            dir.join("wings-plugin.json"),
            serde_json::to_vec_pretty(&whole).unwrap(),
        )
        .unwrap();
        fs::write(dir.join("dist/main.js"), "").unwrap();
        put_frame(&dir.join("dist/native"), "main", main);
        // A manifest that names an icon gets a Lucide "gauge" (ISC) of that name.
        if let Some(icon) = whole.get("icon").and_then(|icon| icon.as_str()) {
            fs::write(dir.join(icon), GAUGE).unwrap();
        }
        for (stem, source) in pages {
            fs::write(dir.join("dist").join(format!("{stem}.html")), "").unwrap();
            put_frame(&dir.join("dist/native"), stem, source);
        }
        let granted: Manifest = serde_json::from_value(whole).unwrap();
        let approved =
            json!({ "permissions": granted.permissions, "contributes": granted.contributes });
        let mut entries = self.entries.borrow_mut();
        entries.insert(
            id.into(),
            json!({ "enabled": true, "approved": approved, "source": { "kind": "file" }, "secrets": [], "scope": scope }),
        );
        fs::write(
            &self.paths().state,
            serde_json::to_vec_pretty(&*entries).unwrap(),
        )
        .unwrap();
        dir
    }

    /// Installs the plugin whose files are the folder `from`, as a package unpacks: its manifest at the top and what
    /// it ships beside it. It is turned on, with what its manifest asks for approved, and `scope` is its storage scope.
    pub fn install_folder(&self, from: &Path, scope: &str) -> PathBuf {
        fn copy(from: &Path, to: &Path) {
            fs::create_dir_all(to).unwrap();
            for entry in fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                let target = to.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy(&entry.path(), &target);
                } else {
                    fs::copy(entry.path(), target).unwrap();
                }
            }
        }
        let granted: Manifest =
            serde_json::from_slice(&fs::read(from.join("wings-plugin.json")).unwrap()).unwrap();
        let dir = self.paths().dir.join(&granted.id);
        copy(from, &dir);
        let approved =
            json!({ "permissions": granted.permissions, "contributes": granted.contributes });
        let mut entries = self.entries.borrow_mut();
        entries.insert(
            granted.id,
            json!({ "enabled": true, "approved": approved, "source": { "kind": "file" }, "secrets": [], "scope": scope }),
        );
        fs::write(
            &self.paths().state,
            serde_json::to_vec_pretty(&*entries).unwrap(),
        )
        .unwrap();
        dir
    }

    /// What `wings.storage` of `plugin` holds under `key`, read the way the desktop reads it.
    pub fn saved(&self, plugin: &str, scope: &str, key: &str) -> Option<serde_json::Value> {
        wings_plugin_storage::Storage::new(self.paths().storage)
            .get(plugin, scope, key)
            .unwrap()
    }

    pub fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
