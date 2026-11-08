//! Finds plugin folders and reads and checks each one: its manifest, its files and its icon.

use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::{
    exec::exec_entry,
    fetch::{fetch_prefix_ok, fetch_rule, open_url_prefix_ok, valid_secret_name},
    log_line,
    manifest::{
        Contributes, Manifest, Permissions, Plugin, SectionSort, BLOCK_MIN_HEIGHT, HOOK_EVENTS, MAX_BLOCKS, MAX_MCP_INSTRUCTIONS, PANEL_ICONS, POPOVER_HEIGHT,
        POPOVER_WIDTH, UI_KINDS, VIEW_PLACES,
    },
    panes::plain_word,
    post::post_entry,
    wings_version,
};

/// The largest icon file Wings reads.
pub const ICON_MAX_BYTES: u64 = 32 * 1024;

/// Reads and checks a plugin's icon: a relative path to an SVG file inside the plugin folder, up to `ICON_MAX_BYTES`.
/// The markup is only ever handed to the UI as an image (a CSS mask), never as part of the page.
fn load_icon(dir: &Path, relative: &str) -> Result<String, String> {
    use std::path::Component;
    let path = Path::new(relative);
    let plain = path.components().all(|c| matches!(c, Component::Normal(_)));
    if relative.is_empty() || !plain || relative.contains('\\') || !relative.to_ascii_lowercase().ends_with(".svg") {
        return Err(format!("icon {relative:?} must be a relative path to an .svg file in the plugin folder, like icon.svg"));
    }
    let file = resolve(dir, relative).filter(|f| f.is_file()).ok_or_else(|| format!("icon {relative:?} isn't a file in the plugin folder"))?;
    let mut bytes = Vec::new();
    fs::File::open(&file).and_then(|f| f.take(ICON_MAX_BYTES + 1).read_to_end(&mut bytes)).map_err(|e| format!("icon {relative:?}: {e}"))?;
    if bytes.len() as u64 > ICON_MAX_BYTES {
        return Err(format!("icon {relative:?} is over {} KB", ICON_MAX_BYTES / 1024));
    }
    let text = String::from_utf8(bytes).map_err(|_| format!("icon {relative:?} isn't UTF-8 text"))?;
    if !is_svg(&text) {
        return Err(format!("icon {relative:?} isn't an SVG: its root element must be <svg>"));
    }
    Ok(text)
}

/// Whether the document's root element is `svg`, after an optional BOM, XML declaration, comments and doctype.
fn is_svg(text: &str) -> bool {
    let mut rest = text.trim_start_matches('\u{feff}').trim_start();
    loop {
        let skip = if rest.starts_with("<?") {
            rest.find("?>").map(|i| i + 2)
        } else if rest.starts_with("<!--") {
            rest.find("-->").map(|i| i + 3)
        } else if rest.len() >= 9 && rest[..9].eq_ignore_ascii_case("<!doctype") {
            // A doctype may hold an internal subset in [...]; Wings never expands entities, but doesn't read past one.
            if rest.contains('[') {
                return false;
            } else {
                rest.find('>').map(|i| i + 1)
            }
        } else {
            break;
        };
        match skip {
            Some(n) => rest = rest[n..].trim_start(),
            None => return false,
        }
    }
    rest.strip_prefix("<svg").is_some_and(|r| r.starts_with(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')) && rest.contains('>')
}

const API_VERSION: u32 = 1;

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn version_parts(version: &str) -> Option<Vec<u64>> {
    version.trim().trim_start_matches('v').split('.').map(|p| p.parse().ok()).collect()
}

/// Whether this Wings is at least `min`, a version like `0.1.12`.
pub fn runs_on_this_wings(min: &str) -> Result<bool, String> {
    let min = version_parts(min).ok_or_else(|| format!("minWingsVersion {min:?} isn't a version like 0.1.12"))?;
    Ok(version_parts(wings_version()).expect("Wings' own version") >= min)
}

/// Loads every plugin folder under `roots`. Broken or incompatible plugins are skipped with a log line.
pub fn discover(roots: &[PathBuf]) -> HashMap<String, Plugin> {
    let mut found = HashMap::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else { continue };
        for entry in entries.flatten() {
            let dir = entry.path();
            match load(&dir) {
                Ok(plugin) => {
                    found.entry(plugin.manifest.id.clone()).or_insert(plugin);
                }
                Err(e) if dir.join("wings-plugin.json").exists() => log_line!("[plugins] skipped {}: {e}", dir.display()),
                Err(_) => {}
            }
        }
    }
    found
}

pub fn load(dir: &Path) -> Result<Plugin, String> {
    let text = fs::read_to_string(dir.join("wings-plugin.json")).map_err(|e| e.to_string())?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if !valid_id(&manifest.id) {
        return Err(format!("invalid id {:?}", manifest.id));
    }
    if manifest.api != API_VERSION {
        return Err(format!("needs plugin API {}, this Wings has {API_VERSION}", manifest.api));
    }
    if let Some(min) = &manifest.min_wings_version {
        if !runs_on_this_wings(min)? {
            return Err(format!("needs Wings {min} or newer, this is {}", wings_version()));
        }
    }
    if !dir.join(manifest.main.trim_start_matches('/')).is_file() {
        // A TypeScript plugin's repo holds its source, and its built files come in its release.
        return Err(if dir.join("src").is_dir() {
            format!("{} isn't built. Install the plugin from a release, or run `wings-plugin build` in its folder", manifest.main)
        } else {
            format!("{} is missing", manifest.main)
        });
    }
    resolve(dir, &manifest.main).ok_or("main is outside the plugin folder")?;
    check_contributes(dir, &manifest.contributes)?;
    check_permissions(&manifest.permissions)?;
    // After the permissions, so a manifest with both wrong reports the permission.
    for panel in &manifest.contributes.panels {
        check_entry("panel", &panel.id, &panel.title, &panel.icon, false)?;
        if !panel.url.starts_with("https://") || url::Url::parse(&panel.url).is_err() {
            return Err(format!("panel url {:?} must be an https URL", panel.url));
        }
    }
    let icon_svg = manifest.icon.as_deref().map(|icon| load_icon(dir, icon)).transpose()?;
    Ok(Plugin { manifest, dir: dir.to_path_buf(), icon_svg })
}

/// A sidebar's, view's, project section's, block's or panel's id, title and icon, all checked the same way.
fn check_entry(kind: &str, id: &str, title: &str, icon: &str, used_twice: bool) -> Result<(), String> {
    if !valid_id(id) || title.is_empty() || title.len() > 40 {
        return Err(format!("{kind} {id:?} needs an id like a plugin id and a title up to 40 characters"));
    }
    if used_twice {
        return Err(format!("{kind} id {id:?} is used twice"));
    }
    if !icon.is_empty() && !PANEL_ICONS.iter().any(|i| i == icon) {
        return Err(format!("{kind} icon {icon:?} must be one of {:?}, or left out to use the plugin icon", PANEL_ICONS.as_slice()));
    }
    Ok(())
}

fn check_page(dir: &Path, kind: &str, page: &str) -> Result<(), String> {
    resolve(dir, page).map(drop).ok_or_else(|| format!("{kind} page {page:?} isn't in the plugin folder"))
}

/// Everything in `contributes` but its panels' entries, which `load` checks after the permissions.
fn check_contributes(dir: &Path, contributes: &Contributes) -> Result<(), String> {
    if let Some(kind) = contributes.ui.iter().find(|k| !UI_KINDS.contains(&k.as_str())) {
        return Err(format!("unknown ui {kind:?}, expected one of {UI_KINDS:?}"));
    }
    if let Some(event) = contributes.hooks.iter().find(|e| !HOOK_EVENTS.contains(&e.as_str())) {
        return Err(format!("unknown hook {event:?}, expected one of {HOOK_EVENTS:?}"));
    }
    // Up to 60 so `<plugin id>__<tool>` stays within MCP's 128 characters.
    let tool_name = |n: &str| !n.is_empty() && n.len() <= 60 && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if let Some(tool) = contributes.mcp_tools.iter().find(|t| !tool_name(&t.name)) {
        return Err(format!("MCP tool name {:?} must be up to 60 lowercase letters, digits and _", tool.name));
    }
    if let Some(tool) = contributes.mcp_tools.iter().find(|t| t.input_schema.get("type").and_then(Value::as_str) != Some("object")) {
        return Err(format!("MCP tool {:?} needs an inputSchema with \"type\": \"object\"", tool.name));
    }
    if contributes.mcp_instructions.chars().count() > MAX_MCP_INSTRUCTIONS {
        return Err(format!("mcpInstructions is up to {MAX_MCP_INSTRUCTIONS} characters"));
    }
    if contributes.panels.len() > 3 {
        return Err("at most 3 panels".into());
    }
    if contributes.sidebars.len() > 3 {
        return Err("at most 3 sidebars".into());
    }
    for sidebar in &contributes.sidebars {
        check_entry("sidebar", &sidebar.id, &sidebar.title, &sidebar.icon, false)?;
        check_page(dir, "sidebar", &sidebar.page)?;
        if let Some(popover) = &sidebar.popover {
            let ((w0, w1), (h0, h1)) = (POPOVER_WIDTH, POPOVER_HEIGHT);
            if !(w0..=w1).contains(&popover.width) || !(h0..=h1).contains(&popover.height) {
                return Err(format!("sidebar {:?} popover must be {w0} to {w1} wide and {h0} to {h1} high", sidebar.id));
            }
            check_page(dir, "popover", &popover.page)?;
        }
    }
    if contributes.views.len() > 3 {
        return Err("at most 3 views".into());
    }
    for view in &contributes.views {
        // Sidebars and views share title bar buttons, `setSidebarLabel` and `openView`, so they share ids too.
        let uses = contributes.sidebars.iter().filter(|s| s.id == view.id).count() + contributes.views.iter().filter(|v| v.id == view.id).count();
        check_entry("view", &view.id, &view.title, &view.icon, uses > 1)?;
        let unique = view.opens.iter().enumerate().all(|(i, o)| !view.opens[..i].contains(o));
        if view.opens.is_empty() || !unique || view.opens.iter().any(|o| !VIEW_PLACES.contains(&o.as_str())) {
            return Err(format!("view {:?} opens must list some of {VIEW_PLACES:?}, each once", view.id));
        }
        if (view.new_tab || view.multiple || view.no_project) && !view.opens.iter().any(|o| o == "tab") {
            return Err(format!("view {:?} sets newTab, multiple or noProject, which need \"tab\" in opens", view.id));
        }
        check_page(dir, "view", &view.page)?;
    }
    if contributes.project_sections.len() > 8 {
        return Err("at most 8 project sections".into());
    }
    for (i, section) in contributes.project_sections.iter().enumerate() {
        let used_twice = contributes.project_sections[..i].iter().any(|s| s.id == section.id);
        check_entry("project section", &section.id, &section.title, &section.icon, used_twice)?;
        if !section.all.is_empty() && !contributes.views.iter().any(|v| v.id == section.all && v.opens.iter().any(|o| o == "tab")) {
            return Err(format!("project section {:?} names {:?} as its all view, which isn't one of the views that open as a tab", section.id, section.all));
        }
        let sort_ok =
            |(j, o): (usize, &SectionSort)| valid_id(&o.id) && !o.label.is_empty() && o.label.len() <= 40 && !section.sorts[..j].iter().any(|p| p.id == o.id);
        if section.sorts.len() > 8 || !section.sorts.iter().enumerate().all(sort_ok) {
            return Err(format!("project section {:?} sorts: at most 8, each with its own id like a plugin id and a label up to 40 characters", section.id));
        }
    }
    if contributes.blocks.len() > MAX_BLOCKS {
        return Err(format!("at most {MAX_BLOCKS} blocks"));
    }
    for (i, block) in contributes.blocks.iter().enumerate() {
        let used_twice = contributes.blocks[..i].iter().any(|b| b.id == block.id);
        check_entry("block", &block.id, &block.title, &block.icon, used_twice)?;
        let (lo, hi) = BLOCK_MIN_HEIGHT;
        if block.min_height.is_some_and(|h| !(lo..=hi).contains(&h)) {
            return Err(format!("block {:?} minHeight must be {lo} to {hi}", block.id));
        }
        check_page(dir, "block", &block.page)?;
    }
    if let Some(settings) = &contributes.settings {
        check_page(dir, "settings", &settings.page)?;
    }
    Ok(())
}

fn check_permissions(permissions: &Permissions) -> Result<(), String> {
    if let Some(entry) = permissions.open_url.iter().find(|e| !open_url_prefix_ok(e)) {
        return Err(format!(
            "openUrl entry {entry:?} must be https:// for any page, an https URL with a path like https://example.com/, or http://localhost: for this computer"
        ));
    }
    if let Some(entry) = permissions.fetch.iter().find(|e| fetch_rule(e).is_none()) {
        return Err(format!(
            "fetch entry {entry:?} must be an https URL with a path, like https://api.example.com/, optionally after GET or POST (a POST URL may use * for one segment and end with {{field=value}})"
        ));
    }
    if let Some((name, prefix)) = permissions.secrets.iter().find(|(name, prefix)| !valid_secret_name(name) || !fetch_prefix_ok(prefix)) {
        return Err(format!(
            "secret {name:?} needs a name of letters, digits, - and _, and the https URL prefix it's sent to, like https://api.example.com/, not {prefix:?}"
        ));
    }
    let flags_ok = |e: &str| {
        let (_, flags) = exec_entry(e);
        e.matches('[').count() <= 1 && flags.is_none_or(|f| e.trim_end().ends_with(']') && f.iter().all(|w| w.starts_with('-')))
    };
    if let Some(entry) = permissions.exec.iter().find(|e| !flags_ok(e)) {
        return Err(format!(
            "exec entry {entry:?} may end with one [...] listing the flags it allows, like az boards work-item update [--id --state], or [] for none"
        ));
    }
    if let Some(entry) = permissions.post.iter().find(|e| post_entry(e).is_none()) {
        return Err(format!("post entry {entry:?} must be gh, glab or az and an API path, like gh repos/*/*/issues/*/comments"));
    }
    if let Some(entry) = permissions.panes.iter().flatten().find(|e| e.trim().is_empty() || !e.split_ascii_whitespace().all(plain_word)) {
        return Err(format!("panes entry {entry:?} must be a program and its arguments, using only letters, digits and -_./:=@%+,"));
    }
    Ok(())
}

/// A file inside the plugin folder; `None` if the path escapes it (`..`, symlinks out).
pub fn resolve(dir: &Path, relative: &str) -> Option<PathBuf> {
    let root = dir.canonicalize().ok()?;
    let path = root.join(relative.trim_start_matches('/')).canonicalize().ok()?;
    path.starts_with(&root).then_some(path)
}

pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("js" | "mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("html") => "text/html",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Popover, SettingsPage};

    #[test]
    fn a_plugin_without_its_built_main_says_so() {
        let dir = std::env::temp_dir().join(format!("wings-unbuilt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("wings-plugin.json"), r#"{ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js" }"#).unwrap();
        assert!(load(&dir).err().unwrap().contains("main.js isn't built"));
        fs::remove_dir_all(dir.join("src")).unwrap();
        assert_eq!(load(&dir).err().unwrap(), "main.js is missing");
        fs::write(dir.join("main.js"), "").unwrap();
        assert!(load(&dir).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ui_accepts_status_next_to_badges_and_refuses_unknown_kinds() {
        let dir = std::env::temp_dir().join(format!("wings-ui-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |ui: &str| format!(r#"{{ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "contributes": {{ "ui": [{ui}] }} }}"#);
        fs::write(dir.join("wings-plugin.json"), manifest(r#""badges", "status""#)).unwrap();
        assert_eq!(load(&dir).unwrap().manifest.contributes.ui, ["badges", "status"]);
        fs::write(dir.join("wings-plugin.json"), manifest(r#""strip""#)).unwrap();
        assert!(load(&dir).err().unwrap().contains("unknown ui \"strip\""));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hooks_accept_journal_and_refuse_other_events() {
        assert_eq!(HOOK_EVENTS, ["SessionStart", "PreToolUse", "UserPromptSubmit", "Journal"]);
        let dir = std::env::temp_dir().join(format!("wings-hooks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest =
            |hooks: &str| format!(r#"{{ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "contributes": {{ "hooks": [{hooks}] }} }}"#);
        fs::write(dir.join("wings-plugin.json"), manifest(r#""SessionStart", "Journal""#)).unwrap();
        assert_eq!(load(&dir).unwrap().manifest.contributes.hooks, ["SessionStart", "Journal"]);
        fs::write(dir.join("wings-plugin.json"), manifest(r#""Stop""#)).unwrap();
        assert!(load(&dir).err().unwrap().contains("unknown hook \"Stop\""));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mcp_instructions_are_capped() {
        let dir = std::env::temp_dir().join(format!("wings-instr-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |text: &str| {
            format!(r#"{{ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "contributes": {{ "mcpInstructions": "{text}" }} }}"#)
        };
        fs::write(dir.join("wings-plugin.json"), manifest(&"a".repeat(MAX_MCP_INSTRUCTIONS))).unwrap();
        assert_eq!(load(&dir).unwrap().manifest.contributes.mcp_instructions.len(), MAX_MCP_INSTRUCTIONS);
        fs::write(dir.join("wings-plugin.json"), manifest(&"a".repeat(MAX_MCP_INSTRUCTIONS + 1))).unwrap();
        assert!(load(&dir).err().unwrap().contains("mcpInstructions"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_cannot_escape_the_plugin_folder() {
        let dir = std::env::temp_dir().join(format!("wings-plugin-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        assert!(resolve(&dir, "/main.js").is_some());
        assert!(resolve(&dir, "../../etc/passwd").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn icons_are_checked_when_the_plugin_loads() {
        let dir = std::env::temp_dir().join(format!("wings-icon-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("img")).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        fs::write(dir.join("sidebar.html"), "").unwrap();
        let svg = r#"<?xml version="1.0"?><!-- lucide --><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M0 0h24"/></svg>"#;
        fs::write(dir.join("icon.svg"), svg).unwrap();
        fs::write(dir.join("img/nested.svg"), "<svg/>").unwrap();
        fs::write(dir.join("big.svg"), format!("<svg>{}</svg>", " ".repeat(ICON_MAX_BYTES as usize))).unwrap();
        fs::write(dir.join("page.svg"), "<html><svg></svg></html>").unwrap();
        fs::write(dir.join("entity.svg"), "<!DOCTYPE svg [<!ENTITY a \"b\">]><svg></svg>").unwrap();
        fs::write(dir.join("binary.svg"), [0xff, 0xfe, 0x00]).unwrap();
        fs::write(dir.join("notsvg.png"), "<svg/>").unwrap();
        let outside = dir.with_file_name(format!("{}-outside.svg", dir.file_name().unwrap().to_string_lossy()));
        fs::write(&outside, "<svg/>").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("link.svg")).unwrap();
        let manifest = |icon: Option<&str>, sidebar_icon: Option<&str>| {
            let mut sidebar = serde_json::json!({ "id": "s", "title": "S", "page": "sidebar.html" });
            if let Some(i) = sidebar_icon {
                sidebar["icon"] = i.into();
            }
            let mut m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "contributes": { "sidebars": [sidebar] } });
            if let Some(i) = icon {
                m["icon"] = i.into();
            }
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir)
        };
        let ok = manifest(Some("icon.svg"), None).unwrap();
        assert_eq!(ok.icon_svg.as_deref(), Some(svg));
        assert_eq!(ok.manifest.contributes.sidebars[0].icon, "");
        assert_eq!(manifest(Some("img/nested.svg"), Some("clock")).unwrap().icon_svg.as_deref(), Some("<svg/>"));
        assert_eq!(manifest(None, Some("clock")).unwrap().icon_svg, None);
        assert!(manifest(None, Some("not-an-icon")).err().unwrap().contains("sidebar icon"));
        let outside_path = outside.to_string_lossy().to_string();
        for (bad, error) in [
            ("../wings-icon-outside.svg", "must be a relative path"),
            (outside_path.as_str(), "must be a relative path"),
            ("/icon.svg", "must be a relative path"),
            ("img/../icon.svg", "must be a relative path"),
            ("img\\..\\icon.svg", "must be a relative path"),
            ("", "must be a relative path"),
            ("notsvg.png", "must be a relative path"),
            ("link.svg", "isn't a file in the plugin folder"),
            ("missing.svg", "isn't a file in the plugin folder"),
            ("img", "must be a relative path"),
            ("big.svg", "is over 32 KB"),
            ("page.svg", "isn't an SVG"),
            ("entity.svg", "isn't an SVG"),
            ("binary.svg", "isn't UTF-8"),
        ] {
            let e = manifest(Some(bad), None).err().unwrap_or_else(|| panic!("{bad:?} was accepted"));
            assert!(e.contains(error), "{bad:?}: {e}");
        }
        for (text, svg) in [
            ("<svg/>", true),
            ("\u{feff}\n<svg width=\"1\">", true),
            ("<svgx/>", false),
            ("<svg", false),
            ("", false),
            ("<!-- never closed <svg/>", false),
            ("<?xml <svg/>", false),
        ] {
            assert_eq!(is_svg(text), svg, "{text:?}");
        }
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn sidebar_popovers_are_checked_when_the_plugin_loads() {
        let dir = std::env::temp_dir().join(format!("wings-popover-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for file in ["main.js", "sidebar.html", "popover.html"] {
            fs::write(dir.join(file), "").unwrap();
        }
        let manifest = |popover: Option<Value>| {
            let mut sidebar = serde_json::json!({ "id": "s", "title": "S", "icon": "clock", "page": "sidebar.html" });
            if let Some(popover) = popover {
                sidebar["popover"] = popover;
            }
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "contributes": { "sidebars": [sidebar] } });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir)
        };
        let ok = manifest(Some(serde_json::json!({ "page": "popover.html", "width": 340, "height": 420 }))).unwrap();
        assert_eq!(ok.manifest.contributes.sidebars[0].popover, Some(Popover { page: "popover.html".into(), width: 340, height: 420 }));
        for (width, height) in [(240, 160), (480, 640)] {
            assert!(manifest(Some(serde_json::json!({ "page": "popover.html", "width": width, "height": height }))).is_ok(), "{width}x{height}");
        }
        for (bad, error) in [
            (serde_json::json!({ "page": "popover.html", "width": 239, "height": 420 }), "popover must be"),
            (serde_json::json!({ "page": "popover.html", "width": 481, "height": 420 }), "popover must be"),
            (serde_json::json!({ "page": "popover.html", "width": 340, "height": 159 }), "popover must be"),
            (serde_json::json!({ "page": "popover.html", "width": 340, "height": 641 }), "popover must be"),
            (serde_json::json!({ "page": "../popover.html", "width": 340, "height": 420 }), "isn't in the plugin folder"),
            (serde_json::json!({ "page": "missing.html", "width": 340, "height": 420 }), "isn't in the plugin folder"),
            (serde_json::json!({ "page": "popover.html" }), "missing field"),
        ] {
            assert!(manifest(Some(bad.clone())).err().unwrap().contains(error), "{bad}");
        }
        // A sidebar without one serializes as before, so plugins approved earlier keep their grant.
        let plain = manifest(None).unwrap().manifest.contributes.sidebars[0].clone();
        assert_eq!(plain.popover, None);
        assert!(serde_json::to_value(&plain).unwrap().get("popover").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn view_new_tab_and_multiple_need_a_tab_and_stay_out_of_older_manifests() {
        let dir = std::env::temp_dir().join(format!("wings-newtab-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for file in ["main.js", "page.html"] {
            fs::write(dir.join(file), "").unwrap();
        }
        let manifest = |extra: Value, opens: Value| {
            let mut view = serde_json::json!({ "id": "v", "title": "V", "page": "page.html", "opens": opens });
            for (k, v) in extra.as_object().unwrap() {
                view[k] = v.clone();
            }
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "contributes": { "views": [view] } });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir)
        };
        let ok = manifest(serde_json::json!({ "newTab": true, "multiple": true }), serde_json::json!(["modal", "tab"])).unwrap();
        let view = &ok.manifest.contributes.views[0];
        assert!(view.new_tab && view.multiple);
        assert_eq!(serde_json::to_value(view).unwrap()["newTab"], true);
        for extra in [serde_json::json!({ "newTab": true }), serde_json::json!({ "multiple": true }), serde_json::json!({ "noProject": true })] {
            assert!(manifest(extra, serde_json::json!(["modal"])).err().unwrap().contains("need \"tab\" in opens"));
        }
        let free = manifest(serde_json::json!({ "noProject": true }), serde_json::json!(["tab"])).unwrap().manifest.contributes.views[0].clone();
        assert!(free.no_project);
        assert_eq!(serde_json::to_value(&free).unwrap()["noProject"], true);
        // Without them a view serializes as before, so plugins approved earlier keep their grant.
        let plain = manifest(serde_json::json!({}), serde_json::json!(["tab"])).unwrap().manifest.contributes.views[0].clone();
        assert!(!plain.new_tab && !plain.multiple && !plain.no_project);
        let json = serde_json::to_value(&plain).unwrap();
        assert!(json.get("newTab").is_none() && json.get("multiple").is_none() && json.get("noProject").is_none() && json.get("button").is_none());
        assert!(plain.button);
        // `button: false` keeps a one-item page out of the title bar, and is kept when saved.
        let item =
            manifest(serde_json::json!({ "multiple": true, "button": false }), serde_json::json!(["tab"])).unwrap().manifest.contributes.views[0].clone();
        assert!(!item.button);
        assert_eq!(serde_json::to_value(&item).unwrap()["button"], false);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_sections_are_checked_and_stay_out_of_older_manifests() {
        let dir = std::env::temp_dir().join(format!("wings-sections-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |sections: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "contributes": { "projectSections": sections } });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir)
        };
        let icon = PANEL_ICONS[0].clone();
        let ok = manifest(serde_json::json!([{ "id": "prs", "title": "Pull requests", "icon": icon }, { "id": "notes", "title": "Notes" }])).unwrap();
        let sections = &ok.manifest.contributes.project_sections;
        let sorted = manifest(serde_json::json!([{ "id": "a", "title": "S", "sorts": [{ "id": "status", "label": "Status" }] }])).unwrap();
        assert_eq!(serde_json::to_value(&sorted.manifest.contributes).unwrap()["projectSections"][0]["sorts"][0]["label"], "Status");
        assert!(serde_json::to_value(&ok.manifest.contributes).unwrap()["projectSections"][0].get("sorts").is_none());
        assert_eq!((sections[0].id.as_str(), sections[1].icon.as_str()), ("prs", ""));
        assert_eq!(serde_json::to_value(&ok.manifest.contributes).unwrap()["projectSections"][0]["title"], "Pull requests");
        // A section without `all` serializes as before, so its approval stays.
        assert!(serde_json::to_value(&ok.manifest.contributes).unwrap()["projectSections"][0].get("all").is_none());
        let many: Vec<Value> = (0..9).map(|i| serde_json::json!({ "id": format!("s{i}"), "title": "S" })).collect();
        for (bad, error) in [
            (serde_json::json!(many), "at most 8"),
            (serde_json::json!([{ "id": "Bad Id", "title": "S" }]), "needs an id"),
            (serde_json::json!([{ "id": "", "title": "S" }]), "needs an id"),
            (serde_json::json!([{ "id": "a", "title": "" }]), "needs an id"),
            (serde_json::json!([{ "id": "a", "title": "x".repeat(41) }]), "needs an id"),
            (serde_json::json!([{ "id": "a", "title": "S" }, { "id": "a", "title": "T" }]), "used twice"),
            (serde_json::json!([{ "id": "a", "title": "S", "icon": "nope" }]), "icon"),
            // `all` must be a view that opens as a tab; this manifest has no views at all.
            (serde_json::json!([{ "id": "a", "title": "S", "all": "list" }]), "all view"),
            (serde_json::json!([{ "id": "a", "title": "S", "sorts": [{ "id": "x", "label": "X" }, { "id": "x", "label": "Y" }] }]), "sorts"),
            (serde_json::json!([{ "id": "a", "title": "S", "sorts": [{ "id": "x", "label": "" }] }]), "sorts"),
        ] {
            assert!(manifest(bad.clone()).err().unwrap().contains(error), "{bad}");
        }
        // Without them the contributes serialize as before, so plugins approved earlier keep their grant.
        let plain = manifest(serde_json::json!([])).unwrap();
        assert!(serde_json::to_value(&plain.manifest.contributes).unwrap().get("projectSections").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn blocks_are_checked_and_stay_out_of_older_manifests() {
        let dir = std::env::temp_dir().join(format!("wings-blocks-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for file in ["main.js", "board.html"] {
            fs::write(dir.join(file), "").unwrap();
        }
        let manifest = |blocks: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "contributes": { "blocks": blocks } });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir)
        };
        let icon = PANEL_ICONS[0].clone();
        let ok = manifest(serde_json::json!([{ "id": "board", "title": "Board", "page": "board.html", "icon": icon, "minHeight": 200 }, { "id": "other", "title": "Other", "page": "board.html" }])).unwrap();
        let blocks = &ok.manifest.contributes.blocks;
        assert_eq!((blocks[0].min_height, blocks[1].min_height, blocks[1].icon.as_str()), (Some(200), None, ""));
        let json = serde_json::to_value(&ok.manifest.contributes).unwrap();
        assert_eq!(json["blocks"][0]["minHeight"], 200);
        assert!(json["blocks"][1].get("minHeight").is_none());
        let block = |extra: Value| {
            let mut b = serde_json::json!({ "id": "a", "title": "A", "page": "board.html" });
            b.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            b
        };
        let many: Vec<Value> = (0..9).map(|i| serde_json::json!({ "id": format!("b{i}"), "title": "B", "page": "board.html" })).collect();
        assert!(manifest(serde_json::json!(many[..8])).is_ok());
        for (bad, error) in [
            (serde_json::json!(many), "at most 8"),
            (serde_json::json!([block(serde_json::json!({ "id": "Bad Id" }))]), "needs an id"),
            (serde_json::json!([block(serde_json::json!({ "title": "" }))]), "needs an id"),
            (serde_json::json!([block(serde_json::json!({ "title": "x".repeat(41) }))]), "needs an id"),
            (serde_json::json!([block(serde_json::json!({})), block(serde_json::json!({}))]), "used twice"),
            (serde_json::json!([block(serde_json::json!({ "icon": "nope" }))]), "icon"),
            (serde_json::json!([block(serde_json::json!({ "minHeight": 79 }))]), "minHeight"),
            (serde_json::json!([block(serde_json::json!({ "minHeight": 801 }))]), "minHeight"),
            (serde_json::json!([block(serde_json::json!({ "page": "missing.html" }))]), "isn't in the plugin folder"),
            (serde_json::json!([block(serde_json::json!({ "page": "../x.html" }))]), "isn't in the plugin folder"),
        ] {
            assert!(manifest(bad.clone()).err().unwrap().contains(error), "{bad}");
        }
        for h in [80, 800] {
            assert!(manifest(serde_json::json!([block(serde_json::json!({ "minHeight": h }))])).is_ok());
        }
        let plain = manifest(serde_json::json!([])).unwrap();
        assert!(serde_json::to_value(&plain.manifest.contributes).unwrap().get("blocks").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_page_is_checked_when_the_plugin_loads() {
        let dir = std::env::temp_dir().join(format!("wings-settings-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for file in ["main.js", "settings.html"] {
            fs::write(dir.join(file), "").unwrap();
        }
        let manifest = |settings: Option<Value>| {
            let mut m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "contributes": {} });
            if let Some(settings) = settings {
                m["contributes"]["settings"] = settings;
            }
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir)
        };
        let ok = manifest(Some(serde_json::json!({ "page": "settings.html" }))).unwrap();
        assert_eq!(ok.manifest.contributes.settings, Some(SettingsPage { page: "settings.html".into() }));
        for (bad, error) in [
            (serde_json::json!({ "page": "../settings.html" }), "isn't in the plugin folder"),
            (serde_json::json!({ "page": "missing.html" }), "isn't in the plugin folder"),
            (serde_json::json!({}), "missing field"),
            (serde_json::json!([{ "page": "settings.html" }]), "invalid type"),
        ] {
            assert!(manifest(Some(bad.clone())).err().unwrap().contains(error), "{bad}");
        }
        // Without one, `contributes` serializes as before, so plugins approved earlier keep their grant.
        let plain = manifest(None).unwrap().manifest.contributes;
        assert_eq!(plain.settings, None);
        assert!(serde_json::to_value(&plain).unwrap().get("settings").is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
