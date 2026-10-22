//! Third-party plugins: a folder with `wings-plugin.json` and web code that runs sandboxed in the UI.
//! Plugins never call Rust directly. The UI relays their requests, and everything that touches the
//! machine is checked here against the permissions the manifest declares.

use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Permissions {
    /// Commands the plugin may run: a program and the subcommand its arguments must start with, e.g.
    /// `gh pr view`. A bare program name allows any arguments.
    pub exec: Vec<String>,
    /// Claude Code transcript entry types the plugin may read, e.g. `pr-link`, or `attachment:<kind>` for one
    /// kind of attachment, e.g. `attachment:model`.
    pub transcript: Vec<String>,
    /// URL prefixes the plugin may open in the browser: https, or http to this computer (`localhost`,
    /// `127.0.0.1` or `[::1]`).
    pub open_url: Vec<String>,
    /// https URLs the plugin may call with `wings.fetch`. A bare entry is a prefix, like an API's base URL, for
    /// any method. `GET <prefix>` allows only GET and HEAD. `POST <url>` allows one POST path, where `*` stands
    /// for one path segment, with a JSON object (or no body, if nothing is pinned) and no query. Either may end with ` {field=a|b}` to
    /// pin body fields (POST only).
    pub fetch: Vec<String>,
    /// API paths the plugin may POST to through a signed-in CLI, as `<gh|glab|az> <pattern>`. `*` stands for
    /// one path segment, e.g. `gh repos/*/*/pulls/*/comments/*/replies`. `exec` refuses every write flag. An
    /// entry may end with ` {field=a|b}`, which pins a body field to one of those values, like
    /// `gh repos/*/*/pulls/*/reviews {event=APPROVE}`.
    pub post: Vec<String>,
    /// Lets the plugin open terminals in your projects and move focus to a pane. Each entry is a command it
    /// may start in a new pane, matched like `exec`; `[]` allows plain shells only. Absent, it can do neither.
    pub panes: Option<Vec<String>>,
    /// Lets the plugin show desktop notifications.
    pub notify: bool,
    /// Lets the plugin read what Claude Code last told `wings statusline`: usage limits and each session's
    /// context window and prompt cache.
    pub statusline: bool,
    /// Lets the plugin see which local TCP ports the programs in each pane listen on, with `wings.ports`.
    pub ports: bool,
    /// Lets the plugin read what an OpenCode session is doing, with `wings.agentActivity`, and hear when it
    /// changes, with `wings.onAgentActivity`.
    pub agent_activity: bool,
    /// Lets the plugin read the names and sizes of the files and folders on your disks, with `wings.diskUsage`, look
    /// for them by name, with `wings.diskFind`, and list the disks, with `wings.disks`.
    pub disk_usage: bool,
}

/// What a plugin adds to Wings, shown in the manager. The host refuses UI calls a plugin didn't declare.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Contributes {
    /// UI it draws: `badges` on pane headers, `status` under a pane's terminal, the `diff` viewer.
    pub ui: Vec<String>,
    /// Tools it offers Claude through the Wings MCP server.
    pub mcp_tools: Vec<McpTool>,
    /// A line for Claude on when to reach for those tools. Claude defers tool descriptions behind tool search
    /// and sees only tool names and the server's instructions at session start, so this is what makes it
    /// use them unprompted. Up to `MAX_MCP_INSTRUCTIONS` characters.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mcp_instructions: String,
    /// Hooks it answers with `wings.onHook`: `SessionStart` to add context to a session, `PreToolUse` to refuse a
    /// Bash command (Wings 0.1.54), `Journal` to get what an agent's change journal reports at the end of a turn,
    /// sent by `wings journal` (Wings 0.1.61).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<String>,
    /// Web pages it shows in a popover from a title bar button.
    pub panels: Vec<Panel>,
    /// Its own pages shown in the right sidebar from a title bar button.
    pub sidebars: Vec<Sidebar>,
    /// Its own pages that can open in the right sidebar, in a modal over Wings, in a window of their own or as a tab.
    pub views: Vec<View>,
    /// Lists it can show under each project in the sidebar, filled with `wings.setProjectSection`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub project_sections: Vec<ProjectSection>,
    /// Pages that can be placed on a project's Project page, each in a frame of its own.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    /// Its own page in Wings Settings, under Plugin settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<SettingsPage>,
}

/// A plugin page the user can place on a Project page. The page is an HTML file like a view's, so a view may share it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: String,
    pub title: String,
    /// One of `PANEL_ICONS`, or empty to use the plugin's own `icon`.
    #[serde(default)]
    pub icon: String,
    pub page: String,
    /// The least height in CSS pixels, within `BLOCK_MIN_HEIGHT`. Absent means `BLOCK_MIN_HEIGHT_DEFAULT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_height: Option<u32>,
}

pub const MAX_MCP_INSTRUCTIONS: usize = 500;
pub const BLOCK_MIN_HEIGHT: (u32, u32) = (80, 800);
pub const BLOCK_MIN_HEIGHT_DEFAULT: u32 = 160;
pub const MAX_BLOCKS: usize = 8;

/// A plugin's page in Wings Settings: an HTML file in the plugin, sandboxed like its sidebars.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsPage {
    pub page: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidebar {
    pub id: String,
    pub title: String,
    /// One of `PANEL_ICONS`, or empty to use the plugin's own `icon`.
    #[serde(default)]
    pub icon: String,
    /// An HTML file in the plugin. It runs sandboxed like the plugin itself, with the same `wings` object.
    pub page: String,
    /// A smaller page shown under the title bar button while you point at it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub popover: Option<Popover>,
}

/// A sidebar's hover page: another HTML file in the plugin, sized in CSS pixels within `POPOVER_WIDTH` and `POPOVER_HEIGHT`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Popover {
    pub page: String,
    pub width: u32,
    pub height: u32,
}

pub const POPOVER_WIDTH: (u32, u32) = (240, 480);
pub const POPOVER_HEIGHT: (u32, u32) = (160, 640);

/// One of the plugin's own pages, like a sidebar that may also open as a modal, a window or a tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub id: String,
    pub title: String,
    /// One of `PANEL_ICONS`, or empty to use the plugin's own `icon`.
    #[serde(default)]
    pub icon: String,
    /// An HTML file in the plugin. It runs sandboxed like the plugin itself, with the same `wings` object.
    pub page: String,
    /// Where it may open, from `VIEW_PLACES`. Its title bar button opens it in the first.
    #[serde(default = "sidebar_only")]
    pub opens: Vec<String>,
    /// The modal's size, and a window's size when it first opens, in CSS pixels.
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// Lists the view in the + menu and the command palette. Needs `tab` in `opens`.
    #[serde(default, rename = "newTab", skip_serializing_if = "std::ops::Not::not")]
    pub new_tab: bool,
    /// Opening it as a tab always opens a new tab, instead of showing the project's existing one. Needs `tab` in `opens`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multiple: bool,
    /// `false` leaves it out of the title bar's plugin buttons: a page that shows one item, opened from a row
    /// with its args, is empty from a button.
    #[serde(default = "yes", skip_serializing_if = "Clone::clone")]
    pub button: bool,
}

fn yes() -> bool {
    true
}

/// A section a plugin may add under each project in the sidebar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSection {
    pub id: String,
    pub title: String,
    /// One of `PANEL_ICONS`, or empty to use the plugin's own `icon`.
    #[serde(default)]
    pub icon: String,
    /// A view that opens as a tab and lists everything, not just the rows: the section's All button opens it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub all: String,
    /// The orders the section's header menu offers. The plugin sorts the rows itself: it hears the pick with `onSectionSort`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sorts: Vec<SectionSort>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionSort {
    pub id: String,
    pub label: String,
}

pub const VIEW_PLACES: [&str; 4] = ["sidebar", "modal", "window", "tab"];

fn sidebar_only() -> Vec<String> {
    vec!["sidebar".into()]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    pub id: String,
    pub title: String,
    /// One of `PANEL_ICONS`, or empty to use the plugin's own `icon`.
    #[serde(default)]
    pub icon: String,
    /// An https page. It runs as a normal website, with no access to Wings.
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// The icon names the UI can draw, shared with `src/lib/plugin-icons.ts`.
static PANEL_ICONS: std::sync::LazyLock<Vec<String>> =
    std::sync::LazyLock::new(|| serde_json::from_str(include_str!("../../src/lib/plugin-icons.json")).expect("plugin-icons.json"));

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema for the tool's arguments; an object schema. Defaults to taking no arguments.
    #[serde(default = "empty_schema")]
    pub input_schema: Value,
}

fn empty_schema() -> Value {
    serde_json::json!({ "type": "object" })
}

const UI_KINDS: [&str; 4] = ["badges", "diff", "rail", "status"];
/// The hooks a plugin can answer. `PreToolUse` only ever sees Bash commands. `Journal` isn't a Claude Code event: it is
/// what `wings journal` sends when an agent's change journal reports a finished turn.
pub const HOOK_EVENTS: [&str; 3] = ["SessionStart", "PreToolUse", "Journal"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub api: u32,
    /// The oldest Wings that can run it, for a plugin that uses something newer than its `api`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_wings_version: Option<String>,
    pub main: String,
    /// An SVG file in the plugin folder, like `icon.svg`: its tile in the Plugins list, and the icon of any
    /// sidebar, view or panel that doesn't name a built-in one. Wings draws it as a one-colour mask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub contributes: Contributes,
}

#[derive(Clone)]
pub struct Plugin {
    pub manifest: Manifest,
    pub dir: PathBuf,
    /// The checked contents of `manifest.icon`.
    pub icon_svg: Option<String>,
}

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
            if rest.contains('[') { return false } else { rest.find('>').map(|i| i + 1) }
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
    Ok(version_parts(env!("CARGO_PKG_VERSION")).expect("Wings' own version") >= min)
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
                Err(e) if dir.join("wings-plugin.json").exists() => eprintln!("[plugins] skipped {}: {e}", dir.display()),
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
            return Err(format!("needs Wings {min} or newer, this is {}", env!("CARGO_PKG_VERSION")));
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
    if let Some(kind) = manifest.contributes.ui.iter().find(|k| !UI_KINDS.contains(&k.as_str())) {
        return Err(format!("unknown ui {kind:?}, expected one of {UI_KINDS:?}"));
    }
    if let Some(event) = manifest.contributes.hooks.iter().find(|e| !HOOK_EVENTS.contains(&e.as_str())) {
        return Err(format!("unknown hook {event:?}, expected one of {HOOK_EVENTS:?}"));
    }
    // Up to 60 so `<plugin id>__<tool>` stays within MCP's 128 characters.
    let tool_name = |n: &str| !n.is_empty() && n.len() <= 60 && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if let Some(tool) = manifest.contributes.mcp_tools.iter().find(|t| !tool_name(&t.name)) {
        return Err(format!("MCP tool name {:?} must be up to 60 lowercase letters, digits and _", tool.name));
    }
    if let Some(tool) = manifest.contributes.mcp_tools.iter().find(|t| t.input_schema.get("type").and_then(Value::as_str) != Some("object")) {
        return Err(format!("MCP tool {:?} needs an inputSchema with \"type\": \"object\"", tool.name));
    }
    if manifest.contributes.mcp_instructions.chars().count() > MAX_MCP_INSTRUCTIONS {
        return Err(format!("mcpInstructions is up to {MAX_MCP_INSTRUCTIONS} characters"));
    }
    if manifest.contributes.panels.len() > 3 {
        return Err("at most 3 panels".into());
    }
    if manifest.contributes.sidebars.len() > 3 {
        return Err("at most 3 sidebars".into());
    }
    for sidebar in &manifest.contributes.sidebars {
        if !valid_id(&sidebar.id) || sidebar.title.is_empty() || sidebar.title.len() > 40 {
            return Err(format!("sidebar {:?} needs an id like a plugin id and a title up to 40 characters", sidebar.id));
        }
        if !sidebar.icon.is_empty() && !PANEL_ICONS.contains(&sidebar.icon) {
            return Err(format!("sidebar icon {:?} must be one of {:?}, or left out to use the plugin icon", sidebar.icon, PANEL_ICONS.as_slice()));
        }
        resolve(dir, &sidebar.page).ok_or_else(|| format!("sidebar page {:?} isn't in the plugin folder", sidebar.page))?;
        if let Some(popover) = &sidebar.popover {
            let ((w0, w1), (h0, h1)) = (POPOVER_WIDTH, POPOVER_HEIGHT);
            if !(w0..=w1).contains(&popover.width) || !(h0..=h1).contains(&popover.height) {
                return Err(format!("sidebar {:?} popover must be {w0} to {w1} wide and {h0} to {h1} high", sidebar.id));
            }
            resolve(dir, &popover.page).ok_or_else(|| format!("popover page {:?} isn't in the plugin folder", popover.page))?;
        }
    }
    if manifest.contributes.views.len() > 3 {
        return Err("at most 3 views".into());
    }
    for view in &manifest.contributes.views {
        if !valid_id(&view.id) || view.title.is_empty() || view.title.len() > 40 {
            return Err(format!("view {:?} needs an id like a plugin id and a title up to 40 characters", view.id));
        }
        // Sidebars and views share title bar buttons, `setSidebarLabel` and `openView`, so they share ids too.
        let uses = manifest.contributes.sidebars.iter().filter(|s| s.id == view.id).count() + manifest.contributes.views.iter().filter(|v| v.id == view.id).count();
        if uses > 1 {
            return Err(format!("view id {:?} is used twice", view.id));
        }
        if !view.icon.is_empty() && !PANEL_ICONS.contains(&view.icon) {
            return Err(format!("view icon {:?} must be one of {:?}, or left out to use the plugin icon", view.icon, PANEL_ICONS.as_slice()));
        }
        let unique = view.opens.iter().enumerate().all(|(i, o)| !view.opens[..i].contains(o));
        if view.opens.is_empty() || !unique || view.opens.iter().any(|o| !VIEW_PLACES.contains(&o.as_str())) {
            return Err(format!("view {:?} opens must list some of {VIEW_PLACES:?}, each once", view.id));
        }
        if (view.new_tab || view.multiple) && !view.opens.iter().any(|o| o == "tab") {
            return Err(format!("view {:?} sets newTab or multiple, which need \"tab\" in opens", view.id));
        }
        resolve(dir, &view.page).ok_or_else(|| format!("view page {:?} isn't in the plugin folder", view.page))?;
    }
    if manifest.contributes.project_sections.len() > 8 {
        return Err("at most 8 project sections".into());
    }
    for (i, section) in manifest.contributes.project_sections.iter().enumerate() {
        if !valid_id(&section.id) || section.title.is_empty() || section.title.len() > 40 {
            return Err(format!("project section {:?} needs an id like a plugin id and a title up to 40 characters", section.id));
        }
        if manifest.contributes.project_sections[..i].iter().any(|s| s.id == section.id) {
            return Err(format!("project section id {:?} is used twice", section.id));
        }
        if !section.icon.is_empty() && !PANEL_ICONS.contains(&section.icon) {
            return Err(format!("project section icon {:?} must be one of {:?}, or left out to use the plugin icon", section.icon, PANEL_ICONS.as_slice()));
        }
        if !section.all.is_empty() && !manifest.contributes.views.iter().any(|v| v.id == section.all && v.opens.iter().any(|o| o == "tab")) {
            return Err(format!("project section {:?} names {:?} as its all view, which isn't one of the views that open as a tab", section.id, section.all));
        }
        let sort_ok = |(j, o): (usize, &SectionSort)| valid_id(&o.id) && !o.label.is_empty() && o.label.len() <= 40 && !section.sorts[..j].iter().any(|p| p.id == o.id);
        if section.sorts.len() > 8 || !section.sorts.iter().enumerate().all(sort_ok) {
            return Err(format!("project section {:?} sorts: at most 8, each with its own id like a plugin id and a label up to 40 characters", section.id));
        }
    }
    if manifest.contributes.blocks.len() > MAX_BLOCKS {
        return Err(format!("at most {MAX_BLOCKS} blocks"));
    }
    for (i, block) in manifest.contributes.blocks.iter().enumerate() {
        if !valid_id(&block.id) || block.title.is_empty() || block.title.len() > 40 {
            return Err(format!("block {:?} needs an id like a plugin id and a title up to 40 characters", block.id));
        }
        if manifest.contributes.blocks[..i].iter().any(|b| b.id == block.id) {
            return Err(format!("block id {:?} is used twice", block.id));
        }
        if !block.icon.is_empty() && !PANEL_ICONS.contains(&block.icon) {
            return Err(format!("block icon {:?} must be one of {:?}, or left out to use the plugin icon", block.icon, PANEL_ICONS.as_slice()));
        }
        let (lo, hi) = BLOCK_MIN_HEIGHT;
        if block.min_height.is_some_and(|h| !(lo..=hi).contains(&h)) {
            return Err(format!("block {:?} minHeight must be {lo} to {hi}", block.id));
        }
        resolve(dir, &block.page).ok_or_else(|| format!("block page {:?} isn't in the plugin folder", block.page))?;
    }
    if let Some(settings) = &manifest.contributes.settings {
        resolve(dir, &settings.page).ok_or_else(|| format!("settings page {:?} isn't in the plugin folder", settings.page))?;
    }
    if let Some(entry) = manifest.permissions.fetch.iter().find(|e| fetch_rule(e).is_none()) {
        return Err(format!(
            "fetch entry {entry:?} must be an https URL with a path, like https://api.example.com/, optionally after GET or POST (a POST URL may use * for one segment and end with {{field=value}})"
        ));
    }
    let flags_ok = |e: &str| {
        let (_, flags) = exec_entry(e);
        e.matches('[').count() <= 1 && flags.is_none_or(|f| e.trim_end().ends_with(']') && !f.is_empty() && f.iter().all(|w| w.starts_with('-')))
    };
    if let Some(entry) = manifest.permissions.exec.iter().find(|e| !flags_ok(e)) {
        return Err(format!("exec entry {entry:?} may end with one [...] listing the flags it allows, like az boards work-item update [--id --state]"));
    }
    if let Some(entry) = manifest.permissions.post.iter().find(|e| post_entry(e).is_none()) {
        return Err(format!("post entry {entry:?} must be gh, glab or az and an API path, like gh repos/*/*/issues/*/comments"));
    }
    if let Some(entry) = manifest.permissions.panes.iter().flatten().find(|e| e.trim().is_empty() || !e.split_ascii_whitespace().all(plain_word)) {
        return Err(format!("panes entry {entry:?} must be a program and its arguments, using only letters, digits and -_./:=@%+,"));
    }
    for panel in &manifest.contributes.panels {
        if !valid_id(&panel.id) || panel.title.is_empty() || panel.title.len() > 40 {
            return Err(format!("panel {:?} needs an id like a plugin id and a title up to 40 characters", panel.id));
        }
        if !panel.icon.is_empty() && !PANEL_ICONS.contains(&panel.icon) {
            return Err(format!("panel icon {:?} must be one of {:?}, or left out to use the plugin icon", panel.icon, PANEL_ICONS.as_slice()));
        }
        if !panel.url.starts_with("https://") || tauri::Url::parse(&panel.url).is_err() {
            return Err(format!("panel url {:?} must be an https URL", panel.url));
        }
    }
    let icon_svg = manifest.icon.as_deref().map(|icon| load_icon(dir, icon)).transpose()?;
    Ok(Plugin { manifest, dir: dir.to_path_buf(), icon_svg })
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

#[derive(Debug, Serialize)]
pub struct ExecResult {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

const EXEC_TIMEOUT: Duration = Duration::from_secs(30);
/// Long enough for a browser sign-in (`az login`), short enough that nothing hangs forever.
const EXEC_TIMEOUT_MAX: Duration = Duration::from_secs(300);

pub fn exec_timeout(requested_ms: Option<u64>) -> Duration {
    requested_ms.map_or(EXEC_TIMEOUT, |ms| Duration::from_millis(ms).clamp(Duration::from_secs(1), EXEC_TIMEOUT_MAX))
}
const EXEC_OUTPUT_LIMIT: u64 = 8 * 1024 * 1024;
/// What a plugin's exec rejects with when a program prints more than `EXEC_OUTPUT_LIMIT` bytes to stdout or stderr.
pub const EXEC_OUTPUT_TOO_LARGE: &str = "Output is larger than 8 MB";

/// Gets each output line while the program runs, for prompts like a sign-in code.
pub type OnLine = Arc<dyn Fn(&str) + Send + Sync>;

/// Reads a pipe on its own thread so a chatty program can't block on a full pipe.
/// Returns the text and whether the program printed more than the limit. One byte past it is read to tell.
fn read_pipe(pipe: impl Read + Send + 'static, on_line: Option<OnLine>, overflow: Arc<AtomicBool>) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(pipe.take(EXEC_OUTPUT_LIMIT + 1));
        let (mut all, mut line) = (Vec::new(), Vec::new());
        while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
            if let Some(f) = &on_line {
                f(String::from_utf8_lossy(&line).trim_end());
            }
            all.append(&mut line);
        }
        if all.len() as u64 > EXEC_OUTPUT_LIMIT {
            overflow.store(true, Ordering::SeqCst);
        }
        String::from_utf8_lossy(&all).into_owned()
    })
}

/// An exec entry split into its command and, from a trailing `[...]`, the only flags it allows.
fn exec_entry(entry: &str) -> (&str, Option<Vec<&str>>) {
    match entry.split_once('[') {
        Some((command, flags)) => (command, Some(flags.trim_end_matches(']').split_whitespace().collect())),
        None => (entry, None),
    }
}

/// Whether a `permissions.exec` entry covers this call: `gh pr view` allows `gh pr view <url> --json ...`, and
/// `az boards work-item update [--id --state]` allows that command with no flags but those two.
fn may_exec(plugin: &Plugin, program: &str, args: &[String]) -> bool {
    plugin.manifest.permissions.exec.iter().any(|entry| {
        let (command, flags) = exec_entry(entry);
        let mut words = command.split_whitespace();
        words.next() == Some(program)
            && words.enumerate().all(|(i, word)| args.get(i).is_some_and(|a| a == word))
            // Exact names only: az would take `--stat` for `--state`, so an abbreviation counts as another flag.
            && flags.is_none_or(|flags| args.iter().filter(|a| a.starts_with('-')).all(|a| flags.contains(&a.split('=').next().unwrap_or(a))))
    })
}

/// Flags that turn an allowed command into running other commands, writing or reading files outside the
/// repo, changing data on the server, or printing a token. Refused whatever the manifest says.
fn is_unsafe_flag(program: &str, arg: &str) -> bool {
    // `abbrev`: git and az accept any unambiguous prefix of a long option, so `--upload=` means
    // `--upload-pack=`. gh and glab only take exact names.
    let (short, long, abbrev): (&[char], &[&str], bool) = match program {
        // Config overrides can point core.fsmonitor, core.sshCommand or an alias at any command.
        "git" => (
            &['c'],
            &["--config-env", "--exec-path", "--upload-pack", "--receive-pack", "--output", "--ext-diff", "--textconv", "--no-index"],
            true,
        ),
        "gh" | "glab" => (&['X', 'f', 'F', 't'], &["--method", "--field", "--raw-field", "--input", "--show-token"], false),
        // Short forms too: -f --file, -d --destination, -s --source, -m --method.
        "az" => (&['m', 'f', 'd', 's'], &["--http-method", "--method", "--in-file", "--out-file", "--output-file", "--file", "--destination", "--source"], true),
        _ => return false,
    };
    // az replaces `@path`, and the value in `key=@path` or `--flag=@path`, with that file's contents.
    if program == "az" && (arg.starts_with('@') || arg.split_once('=').is_some_and(|(_, value)| value.starts_with('@'))) {
        return true;
    }
    let name = arg.split('=').next().unwrap_or(arg);
    if let Some(option) = name.strip_prefix("--") {
        // An exact option name wins over abbreviations, so az's own `--output` isn't `--output-file`.
        let exact = program == "az" && name == "--output";
        return !option.is_empty() && !exact && long.iter().any(|flag| if abbrev { flag.starts_with(name) } else { *flag == name });
    }
    // Short flags group and carry their value attached, like `-iXDELETE` or `-ccore.fsmonitor=...`.
    name.strip_prefix('-').is_some_and(|group| group.chars().any(|c| short.contains(&c)))
}

/// Runs a command the manifest allows. No shell, so arguments can't smuggle in commands.
pub fn exec(
    plugin: &Plugin,
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    on_line: Option<OnLine>,
) -> Result<ExecResult, String> {
    let id = &plugin.manifest.id;
    if !may_exec(plugin, program, args) {
        let sub = args.iter().take_while(|a| !a.starts_with('-')).take(3).cloned().collect::<Vec<_>>().join(" ");
        return Err(format!("{id} may not run {program} {sub}"));
    }
    if let Some(flag) = args.iter().find(|a| is_unsafe_flag(program, a)) {
        return Err(format!("{id} may not pass {flag} to {program}"));
    }
    run(program, args, cwd, timeout, on_line)
}

/// Runs a program once its arguments have been checked.
fn run(program: &str, args: &[String], cwd: Option<&Path>, timeout: Duration, on_line: Option<OnLine>) -> Result<ExecResult, String> {
    let path = find_program(program).ok_or_else(|| format!("{program} is not installed or not on PATH"))?;
    let mut cmd = Command::new(path);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if program == "git" {
        // Makes git reject abbreviated options outright, on top of the prefix check above.
        cmd.env("GIT_TEST_DISALLOW_ABBREVIATED_OPTIONS", "1");
    }
    if let Some(dir) = cwd {
        if !dir.is_dir() {
            return Err(format!("{} is not a folder", dir.display()));
        }
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let overflow = Arc::new(AtomicBool::new(false));
    let out_reader = read_pipe(child.stdout.take().unwrap(), on_line.clone(), overflow.clone());
    let err_reader = read_pipe(child.stderr.take().unwrap(), on_line, overflow.clone());
    let start = Instant::now();
    let status = loop {
        // Checked before the exit status: a program cut off by the closed pipe dies by signal and would look like a failure.
        if overflow.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(EXEC_OUTPUT_TOO_LARGE.into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            return Err(format!("{program} timed out after {}s", timeout.as_secs()));
        }
        // 2 ms: at 20 ms every exec paid ~22 ms of waiting after the program had already finished (measured on echo: 25 ms -> 3 ms).
        std::thread::sleep(Duration::from_millis(2));
    };
    let (stdout, stderr) = (out_reader.join().unwrap_or_default(), err_reader.join().unwrap_or_default());
    if overflow.load(Ordering::SeqCst) {
        return Err(EXEC_OUTPUT_TOO_LARGE.into());
    }
    Ok(ExecResult { code: status.code(), stdout, stderr })
}

/// GUI apps don't get the shell's PATH on macOS and Linux, so ask the login shell once.
pub(crate) fn search_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let inherited = std::env::var("PATH").unwrap_or_default();
        #[cfg(unix)]
        {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            if let Some(path) = login_shell_path(&shell, Duration::from_secs(5)) {
                return format!("{path}:{inherited}");
            }
        }
        inherited
    })
}

/// The PATH a login shell sets up, or `None` when that takes longer than `timeout`. A profile that runs
/// something long would otherwise hold up every program lookup. A job the profile starts in the background
/// can keep the output open after the shell exits, so it's read until the closing marker, not to the end.
#[cfg(unix)]
fn login_shell_path(shell: &str, timeout: Duration) -> Option<String> {
    const MARKER: &str = "__WINGS_PATH__";
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &format!("printf '{MARKER}%s{MARKER}' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut text, mut chunk) = (Vec::new(), [0u8; 4096]);
        while let Ok(n @ 1..) = stdout.read(&mut chunk) {
            text.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&text);
            let mut parts = text.split(MARKER);
            if let (Some(_), Some(path), Some(_)) = (parts.next(), parts.next(), parts.next()) {
                let _ = tx.send(path.to_string());
                return;
            }
        }
    });
    let path = rx.recv_timeout(timeout).ok();
    let _ = child.kill();
    let _ = child.wait();
    path.filter(|p| !p.is_empty())
}

pub fn find_program(program: &str) -> Option<PathBuf> {
    if program.contains(['/', '\\']) {
        return None;
    }
    let names: Vec<String> = if cfg!(windows) { vec![format!("{program}.exe"), format!("{program}.cmd")] } else { vec![program.into()] };
    std::env::split_paths(search_path())
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|p| p.is_file())
}

/// Whether a transcript entry is of a `permissions.transcript` type. `attachment:<kind>` is one kind of
/// attachment: attachments include whole files and hook output, so most plugins want only one kind.
fn is_type(allowed: &str, entry: &Value) -> bool {
    let kind = entry.get("type").and_then(Value::as_str);
    match allowed.strip_prefix("attachment:") {
        Some(sub) => kind == Some("attachment") && entry.pointer("/attachment/type").and_then(Value::as_str) == Some(sub),
        None => kind == Some(allowed),
    }
}

/// Text every line of that type holds, so the others are skipped before they're parsed.
fn type_marker(allowed: &str) -> String {
    format!("\"type\":\"{}\"", allowed.strip_prefix("attachment:").unwrap_or(allowed))
}

/// Entries of the allowed types from a Claude Code session transcript, oldest first.
/// The most entries `last` can ask for.
pub const TRANSCRIPT_LAST_MAX: usize = 1000;

/// With `last`, only the newest that many entries, read from the end. A long session's transcript runs to tens
/// of MB, and most callers want only its latest state.
pub fn transcript_entries(plugin: &Plugin, claude_dir: &Path, session_id: &str, types: &[String], last: Option<usize>) -> Result<Vec<Value>, String> {
    let allowed = &plugin.manifest.permissions.transcript;
    if let Some(t) = types.iter().find(|t| !allowed.contains(t)) {
        return Err(format!("{} may not read {t:?} transcript entries", plugin.manifest.id));
    }
    if session_id.len() != 36 || !session_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("not a session id".into());
    }
    let file = format!("{session_id}.jsonl");
    let path = fs::read_dir(claude_dir.join("projects"))
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|d| d.path().join(&file))
        .find(|p| p.is_file())
        .ok_or("no transcript for that session")?;
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let patterns: Vec<String> = types.iter().map(|t| type_marker(t)).collect();
    let mentions = |l: &&str| patterns.iter().any(|p| l.contains(p.as_str()));
    // A nested object can say `"type":"user"` too, so the entry's own type is checked once it's parsed.
    let entry = |l: &str| serde_json::from_str::<Value>(l).ok().filter(|v| types.iter().any(|t| is_type(t, v)));
    Ok(match last {
        Some(n) => {
            let mut newest: Vec<Value> = text.lines().rev().filter(mentions).filter_map(entry).take(n.min(TRANSCRIPT_LAST_MAX)).collect();
            newest.reverse();
            newest
        }
        None => text.lines().filter(mentions).filter_map(entry).collect(),
    })
}

/// A fetch prefix has to name a host and end its host part with `/`, so `https://api.example.com` can't
/// also match `https://api.example.com.evil.net`.
fn fetch_prefix_ok(prefix: &str) -> bool {
    prefix.strip_prefix("https://").is_some_and(|rest| rest.split_once('/').is_some_and(|(host, _)| !host.is_empty() && !host.contains(['@', '*'])))
}

/// One `permissions.fetch` entry.
struct FetchRule<'a> {
    /// `None` is a bare prefix for any method.
    method: Option<&'a str>,
    target: &'a str,
    pins: Vec<Pin<'a>>,
}

/// A body field that must be one of these values: `{event=APPROVE}` or `{vote=10|5|0}`.
type Pin<'a> = (&'a str, Vec<&'a str>);

/// Splits a trailing ` {field=a|b,other=c}` off an entry. `None` if it is there but malformed.
fn split_pins(entry: &str) -> Option<(&str, Vec<Pin<'_>>)> {
    let entry = entry.trim_end();
    if !entry.ends_with('}') {
        return Some((entry, Vec::new()));
    }
    let at = entry.rfind(" {")?;
    let mut pins = Vec::new();
    for item in entry[at + 2..entry.len() - 1].split(',') {
        let (key, values) = item.split_once('=')?;
        let word = |w: &str| !w.is_empty() && w.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
        let values: Vec<&str> = values.split('|').collect();
        if !word(key) || key.contains(['-', '.']) || !values.iter().all(|v| word(v)) {
            return None;
        }
        pins.push((key, values));
    }
    (!pins.is_empty()).then_some((&entry[..at], pins))
}

/// Whether a body's top-level keys can't be read two ways. Go and .NET servers match JSON keys without regard to
/// case (Go also folds `ſ` to `s` and the Kelvin sign to `k`), so with a pinned field `{"event":"APPROVED",
/// "EVENT":"REQUEST_CHANGES"}` would pass the pin and act on the other key. A body that pins anything must use ASCII
/// keys, none repeated, none equal to another ignoring ASCII case.
fn keys_unambiguous<'a>(keys: impl IntoIterator<Item = &'a str>) -> bool {
    let mut seen = std::collections::HashSet::new();
    keys.into_iter().all(|k| k.is_ascii() && seen.insert(k.to_ascii_lowercase()))
}

/// The top-level keys of a JSON object in order, repeats included, which `serde_json::Value` would merge.
fn json_keys(text: &str) -> Option<Vec<String>> {
    struct Keys(Vec<String>);
    impl<'de> Deserialize<'de> for Keys {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> serde::de::Visitor<'de> for V {
                type Value = Keys;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("a JSON object")
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Keys, A::Error> {
                    let mut keys = Vec::new();
                    while let Some((k, _)) = map.next_entry::<String, serde::de::IgnoredAny>()? {
                        keys.push(k);
                    }
                    Ok(Keys(keys))
                }
            }
            d.deserialize_map(V)
        }
    }
    serde_json::from_str::<Keys>(text).ok().map(|k| k.0)
}

/// Whether the body's fields hold every pinned value, as text (a whole number counts as its digits).
fn pins_hold(pins: &[Pin<'_>], body: &serde_json::Map<String, Value>) -> bool {
    pins.iter().all(|(key, values)| {
        let got = match body.get(*key) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => return false,
        };
        values.contains(&got.as_str())
    })
}

fn fetch_rule(entry: &str) -> Option<FetchRule<'_>> {
    let (core, pins) = split_pins(entry)?;
    let (method, target) = match core.split_once(' ') {
        Some((m @ ("GET" | "POST"), rest)) => (Some(m), rest),
        Some(_) => return None,
        None => (None, core),
    };
    if method != Some("POST") && (!pins.is_empty() || target.contains('*')) {
        return None;
    }
    if method == Some("POST") && (target.contains(['?', '#']) || !target.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~*/:%".contains(&b))) {
        return None;
    }
    fetch_prefix_ok(target).then_some(FetchRule { method, target, pins })
}

/// Whether a `POST` rule's URL pattern covers `url`: the same segments, where `*` takes one plain segment.
fn post_url_matches(pattern: &str, url: &str) -> bool {
    let (want, got): (Vec<&str>, Vec<&str>) = (pattern.split('/').collect(), url.split('/').collect());
    want.len() == got.len() && want.iter().zip(&got).enumerate().all(|(i, (w, g))| if *w == "*" { i > 2 && post_segment_ok(g) } else { w == g })
}

/// The URL to send, if the plugin may call it with this method and body: https, no credentials, under a
/// `permissions.fetch` entry, and already in normal form. The path may only use letters, digits and `-_.~/`, with
/// no `;` or percent escapes, so no server can read it as a different path (`..;/`, `..%2F`, double encoding) than
/// the one that was checked. A `POST` entry also needs the body to be a JSON object that holds its pinned fields.
pub fn fetch_url(plugin: &Plugin, method: &str, url: &str, body: Option<&str>) -> Option<String> {
    let parsed = tauri::Url::parse(url).ok()?;
    let normal = parsed.as_str();
    let plain_path = parsed.path().bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~/".contains(&b));
    let ok = parsed.scheme() == "https"
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.fragment().is_none()
        && normal == url
        && plain_path;
    if !ok {
        return None;
    }
    let method = method.to_ascii_uppercase();
    let allowed = plugin.manifest.permissions.fetch.iter().filter_map(|e| fetch_rule(e)).any(|rule| match rule.method {
        None => normal.starts_with(rule.target),
        Some("GET") => matches!(method.as_str(), "GET" | "HEAD") && normal.starts_with(rule.target),
        _ => {
            method == "POST"
                && parsed.query().is_none()
                && post_url_matches(rule.target, normal)
                && match body.filter(|b| !b.is_empty()) {
                    // A POST that pins nothing may send no body, like Bitbucket's approve.
                    None => rule.pins.is_empty(),
                    Some(b) => {
                        b.len() <= POST_BODY_LIMIT
                            && (rule.pins.is_empty() || json_keys(b).is_some_and(|k| keys_unambiguous(k.iter().map(String::as_str))))
                            && matches!(serde_json::from_str::<Value>(b), Ok(Value::Object(o)) if pins_hold(&rule.pins, &o))
                    }
                }
        }
    });
    allowed.then(|| normal.to_string())
}

/// Whether a pinned `POST` rule covers this call. Its body was checked as JSON, so it must go out as JSON: with a
/// form or UTF-16 content type, `{"event":"APPROVED","x":"&event=REQUEST_CHANGES&"}` reads as the other event.
pub fn fetch_needs_json(plugin: &Plugin, method: &str, url: &str) -> bool {
    method.eq_ignore_ascii_case("POST")
        && plugin.manifest.permissions.fetch.iter().filter_map(|e| fetch_rule(e)).any(|rule| rule.method == Some("POST") && !rule.pins.is_empty() && post_url_matches(rule.target, url))
}

/// The only headers a plugin may set. Anything else could route a request, and the token on it, to a
/// different site or path behind the same server (`Host`, `X-Forwarded-*`, `X-HTTP-Host-Override`).
pub fn fetch_header_ok(name: &str) -> bool {
    ["accept", "accept-language", "content-type", "if-none-match", "if-modified-since", "cache-control"].contains(&name.to_ascii_lowercase().as_str())
}

/// `permissions.post` entry → (program, pattern, pinned body fields), if it's well formed.
fn post_entry(entry: &str) -> Option<(&str, &str, Vec<Pin<'_>>)> {
    let (entry, pins) = split_pins(entry)?;
    let (program, pattern) = entry.split_once(' ')?;
    // `?` and `=` for a fixed query like `?api-version=7.1`. A `*` only matches a whole plain segment.
    let chars_ok = !pattern.is_empty() && pattern.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~*/:%?=".contains(&b));
    let shape_ok = match program {
        "gh" | "glab" => !pattern.starts_with('/') && !pattern.contains(':'),
        "az" => pattern.strip_prefix("https://").is_some_and(|rest| rest.split('/').next().is_some_and(|host| !host.is_empty() && !host.contains('*'))),
        _ => false,
    };
    (chars_ok && shape_ok).then_some((program, pattern, pins))
}

/// A URL segment matched by `*`. Percent escapes stay, since GitLab project paths (`g%2Fp`) and Azure project
/// names (`My%20Project`) need them, but no decoding of it may step out of its place (`%2e%2e`, `%252e`, `%5c`).
fn post_segment_ok(segment: &str) -> bool {
    if segment.is_empty() || !segment.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~%".contains(&b)) {
        return false;
    }
    let bytes = segment.as_bytes();
    let mut decoded = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = segment.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok());
            let Some(byte) = hex else { return false };
            decoded.push(byte);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8_lossy(&decoded);
    !decoded.contains(['%', '\\', '?', '#']) && !decoded.split('/').any(|part| part == "." || part == "..")
}

/// Whether a `permissions.post` entry covers a POST by `program` to `url`.
fn may_post(plugin: &Plugin, program: &str, url: &str, fields: &serde_json::Map<String, Value>) -> bool {
    let parts: Vec<&str> = url.split('/').collect();
    plugin.manifest.permissions.post.iter().filter_map(|e| post_entry(e)).any(|(p, pattern, pins)| {
        let want: Vec<&str> = pattern.split('/').collect();
        p == program
            && want.len() == parts.len()
            && want.iter().zip(&parts).all(|(w, part)| if *w == "*" { post_segment_ok(part) } else { w == part })
            && (pins.is_empty() || keys_unambiguous(fields.keys().map(String::as_str)))
            && pins_hold(&pins, fields)
    })
}

/// A POST a plugin asks for: its fields become the JSON body. `host` is the GitLab server for `glab`, and the
/// GitHub Enterprise Server for `gh` (absent or `github.com` is github.com).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostRequest {
    pub program: String,
    pub url: String,
    #[serde(default)]
    pub host: Option<String>,
    pub fields: serde_json::Map<String, Value>,
}

/// The resource `az rest` gets a token for when it calls Azure DevOps.
const AZURE_DEVOPS_RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";
const POST_BODY_LIMIT: usize = 64 * 1024;

fn hostname_ok(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && !host.starts_with(['-', '.']) && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

/// The CLI arguments for a POST the manifest allows. Wings builds them itself, so the plugin never picks a flag.
fn post_args(plugin: &Plugin, request: &PostRequest) -> Result<Vec<String>, String> {
    let PostRequest { program, url, host, fields } = request;
    let id = &plugin.manifest.id;
    if !may_post(plugin, program, url, fields) {
        return Err(format!("{id} may not post to {program} {url}"));
    }
    let key_ok = |k: &str| !k.is_empty() && k.len() <= 40 && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if fields.is_empty() || fields.len() > 10 {
        return Err("a post needs 1 to 10 fields".into());
    }
    if let Some((key, _)) = fields.iter().find(|(k, v)| !key_ok(k) || !(v.is_string() || v.is_i64())) {
        return Err(format!("post field {key:?} must be a name of letters, digits and _ with a string or whole number value"));
    }
    let body = Value::Object(fields.clone()).to_string();
    if body.len() > POST_BODY_LIMIT {
        return Err("a post is limited to 64 KB".into());
    }
    let mut args: Vec<String> = vec!["api".into()];
    match program.as_str() {
        "gh" | "glab" => {
            if program == "glab" {
                let host = host.as_deref().filter(|h| hostname_ok(h)).ok_or("a glab post needs the GitLab host name")?;
                args.extend(["--hostname".into(), host.into()]);
            } else if let Some(host) = host.as_deref().filter(|h| *h != "github.com") {
                if !hostname_ok(host) {
                    return Err("a gh post's host must be a plain host name".into());
                }
                args.extend(["--hostname".into(), host.into()]);
            }
            args.extend(["--method".into(), "POST".into(), url.clone()]);
            // `-f` sends the text as it is: only `-F` reads `@file` or fills in placeholders, and a number can't start with either.
            for (key, value) in fields {
                match value.as_str() {
                    Some(text) => args.extend(["-f".into(), format!("{key}={text}")]),
                    None => args.extend(["-F".into(), format!("{key}={value}")]),
                }
            }
        }
        // A JSON object starts with `{`, so az never reads it as `@file`.
        "az" => args = ["rest", "--method", "post", "--resource", AZURE_DEVOPS_RESOURCE, "--url", url, "--body", &body, "-o", "json"].map(String::from).to_vec(),
        _ => unreachable!("may_post only matches gh, glab and az"),
    }
    Ok(args)
}

/// The hosts `gh` is signed in to, from `gh auth status --json hosts`.
fn parse_gh_hosts(json: &str) -> Vec<String> {
    serde_json::from_str::<Value>(json).ok().and_then(|v| v.get("hosts")?.as_object().map(|h| h.keys().cloned().collect())).unwrap_or_default()
}

/// The hosts `glab` is set up for: each block of `glab auth status --all` starts with its host name, unindented.
fn parse_glab_hosts(text: &str) -> Vec<String> {
    text.lines().filter(|l| !l.starts_with(char::is_whitespace) && hostname_ok(l.trim())).map(|l| l.trim().to_string()).collect()
}

/// Looks the CLI's signed-in hosts up, and keeps the answer for a minute.
fn signed_in_hosts(program: &str) -> Result<Vec<String>, String> {
    static CACHE: std::sync::Mutex<Vec<(String, std::time::Instant, Vec<String>)>> = std::sync::Mutex::new(Vec::new());
    let mut cache = CACHE.lock().unwrap();
    if let Some((_, at, hosts)) = cache.iter().find(|(p, ..)| p == program) {
        if at.elapsed() < Duration::from_secs(60) {
            return Ok(hosts.clone());
        }
    }
    let args: Vec<String> = (if program == "gh" { &["auth", "status", "--json", "hosts"][..] } else { &["auth", "status", "--all"][..] }).iter().map(|a| a.to_string()).collect();
    let out = run(program, &args, None, EXEC_TIMEOUT, None)?;
    // glab writes its report to stderr when a token is bad, so read both.
    let hosts = if program == "gh" { parse_gh_hosts(&out.stdout) } else { parse_glab_hosts(&format!("{}\n{}", out.stdout, out.stderr)) };
    cache.retain(|(p, ..)| p != program);
    cache.push((program.to_string(), std::time::Instant::now(), hosts.clone()));
    Ok(hosts)
}

/// A CLI sends its token to whatever `--hostname` names (`gh` sends GH_ENTERPRISE_TOKEN to any host but github.com), so a
/// plugin may only name a host the CLI is already set up for. github.com is gh's own default.
fn host_is_signed_in(program: &str, host: Option<&str>, hosts: &dyn Fn(&str) -> Result<Vec<String>, String>) -> Result<(), String> {
    let Some(host) = host.filter(|h| !(program == "gh" && *h == "github.com")) else { return Ok(()) };
    if hosts(program)?.iter().any(|h| h.eq_ignore_ascii_case(host)) {
        Ok(())
    } else {
        Err(format!("{program} isn't signed in to {host}, so Wings won't send a request there."))
    }
}

/// Sends a POST the manifest allows, like a reply to a review comment, as you through the CLI's own sign-in.
pub fn post(plugin: &Plugin, request: &PostRequest) -> Result<ExecResult, String> {
    post_checked(plugin, request, &signed_in_hosts)
}

fn post_checked(plugin: &Plugin, request: &PostRequest, hosts: &dyn Fn(&str) -> Result<Vec<String>, String>) -> Result<ExecResult, String> {
    let args = post_args(plugin, request)?;
    host_is_signed_in(&request.program, request.host.as_deref(), hosts)?;
    run(&request.program, &args, None, EXEC_TIMEOUT, None)
}

pub fn may_open_url(plugin: &Plugin, url: &str) -> bool {
    (url.starts_with("https://") || is_loopback_http(url)) && plugin.manifest.permissions.open_url.iter().any(|prefix| url.starts_with(prefix))
}

/// An http URL that can only reach this computer: host `localhost`, `127.0.0.1` or `[::1]`, no credentials,
/// and already in the normal form the browser would read, so no backslash or odd escape can move the host.
fn is_loopback_http(url: &str) -> bool {
    tauri::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "http"
            && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
            && u.username().is_empty()
            && u.password().is_none()
            && u.as_str() == url
    })
}

/// Whether the plugin may see which local ports the programs in each pane listen on.
pub fn may_read_ports(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.ports
}

/// Whether the plugin may read what an OpenCode session is doing.
pub fn may_read_agent_activity(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.agent_activity
}

/// Whether the plugin may scan the disks for the names and sizes of their files and folders, and list them.
pub fn may_scan_disks(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.disk_usage
}

/// A word of a pane command. Pane commands are typed into your shell, so words can't hold spaces, quotes,
/// backslashes, `$`, globs or operators; single-quoted, such a word reads the same in sh, bash, zsh and fish.
fn plain_word(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./:=@%+,".contains(&b))
}

/// Whether the plugin may read what Claude Code told `wings statusline`.
pub fn may_read_statusline(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.statusline
}

/// Whether the plugin may open terminals and move focus between panes.
pub fn may_use_panes(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.panes.is_some()
}

const PANE_COMMAND_MAX: usize = 1024;
/// The agent CLIs whose flags a plugin may pass only when its `panes` entry names them.
const AGENT_PROGRAMS: [&str; 2] = ["claude", "opencode"];
/// `openPane` `args` together: room for a prompt that names an issue and says what to do with it.
const PANE_ARGS_MAX: usize = 4096;

/// One `args` value as a single shell word: single-quoted, a `'` inside written as `'\''`.
fn shell_quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// The line to type into a new pane's shell for `command`, or `None` for a plain shell. Like `exec`, an entry
/// in `permissions.panes` is a program and the words the command must start with. Words past the entry
/// come from the plugin, so they're single-quoted. `args` go after them as one quoted word each, so a value
/// can hold spaces, like a prompt; one that starts with `-` is refused, since it would read as a flag.
pub fn pane_input(plugin: &Plugin, command: Option<&str>, args: &[String]) -> Result<Option<String>, String> {
    let id = &plugin.manifest.id;
    let Some(declared) = &plugin.manifest.permissions.panes else {
        return Err(format!("{id} may not open panes"));
    };
    let Some(command) = command else {
        return if args.is_empty() { Ok(None) } else { Err("openPane args need a command".into()) };
    };
    // Split on spaces only, so a tab or newline is refused rather than read as a word break.
    let words: Vec<&str> = command.split(' ').filter(|w| !w.is_empty()).collect();
    if words.is_empty() || command.len() > PANE_COMMAND_MAX || !words.iter().all(|w| plain_word(w)) {
        return Err(format!("A pane command is up to 1 KB of words using only letters, digits and -_./:=@%+, not {command:?}"));
    }
    let entry = declared
        .iter()
        .map(|e| e.split_ascii_whitespace().collect::<Vec<_>>())
        .filter(|e| words.starts_with(e))
        .max_by_key(Vec::len)
        .ok_or_else(|| format!("{id} may not start {}", words.iter().take(3).copied().collect::<Vec<_>>().join(" ")))?;
    let program = words[0];
    if let Some(flag) = words[1..].iter().find(|w| is_unsafe_flag(program, w)) {
        return Err(format!("{id} may not pass {flag} to {program}"));
    }
    // An agent's flags set its permissions, tools and config (claude --dangerously-skip-permissions, --mcp-config,
    // --settings; opencode --auto, --server), so a plugin passes only the ones its entry names. And one value at most,
    // a session id or a prompt: a second word reads as a subcommand (`opencode --session <id> uninstall`).
    if AGENT_PROGRAMS.contains(&program) {
        if let Some(flag) = words[entry.len()..].iter().find(|w| w.starts_with('-')) {
            return Err(format!("{id} may not pass {flag} to {program}"));
        }
        if words.len() - entry.len() + args.len() > 1 {
            return Err(format!("{id} may pass {program} one value after {}, not more", entry.join(" ")));
        }
    }
    if args.iter().map(String::len).sum::<usize>() > PANE_ARGS_MAX {
        return Err(format!("openPane args are up to {} KB together", PANE_ARGS_MAX / 1024));
    }
    // Typed into an interactive shell: a newline would run what follows, and other control characters edit the line.
    if let Some(arg) = args.iter().find(|a| a.is_empty() || a.starts_with('-') || a.chars().any(char::is_control)) {
        return Err(format!("openPane args must be non-empty, not start with -, and have no control characters, not {arg:?}"));
    }
    let extra = words[entry.len()..].iter().map(|w| format!("'{w}'")).chain(args.iter().map(|a| shell_quote(a)));
    Ok(Some(entry.iter().map(|w| w.to_string()).chain(extra).collect::<Vec<_>>().join(" ")))
}

/// The folder a plugin's pane may start in: inside one of your projects once `..` and symlinks are resolved,
/// so neither can lead out of it. Returns which project it belongs to and the resolved folder. The project
/// on screen (`current`) wins when it holds the folder, so a split stays in its tab's project; otherwise the
/// deepest project that holds it.
pub fn pane_cwd(cwd: &Path, projects: &[PathBuf], current: Option<usize>) -> Result<(usize, PathBuf), String> {
    let dir = cwd.canonicalize().ok().filter(|d| d.is_dir()).ok_or_else(|| format!("{} is not a folder", cwd.display()))?;
    let roots: Vec<Option<PathBuf>> = projects.iter().map(|p| p.canonicalize().ok()).collect();
    let holds = |i: &usize| roots[*i].as_ref().is_some_and(|root| dir.starts_with(root));
    let depth = |i: &usize| roots[*i].as_ref().map_or(0, |root| root.components().count());
    let project = current.filter(holds).or_else(|| (0..roots.len()).filter(holds).max_by_key(depth));
    project.map(|i| (i, dir)).ok_or_else(|| format!("{} isn't inside any of your Wings projects", cwd.display()))
}

/// Takes the grant `open_pane` left for a tab in `space_id` starting in `dir`, a folder outside that project, so
/// `pane_create` starts it there. Plugin panes never get one.
pub fn take_grant(grants: &mut Vec<(String, PathBuf)>, space_id: &str, dir: &Path) -> Option<PathBuf> {
    let dir = dir.canonicalize().ok()?;
    let at = grants.iter().position(|(s, d)| s == space_id && *d == dir)?;
    Some(grants.remove(at).1)
}

/// Notifications a plugin may show in any minute: enough for a burst of its own updates, too few to flood
/// Notification Center.
pub const NOTIFY_LIMIT: usize = 20;
const NOTIFY_WINDOW: Duration = Duration::from_secs(60);
const NOTIFY_TITLE_MAX: usize = 64;
const NOTIFY_BODY_MAX: usize = 256;

/// When each plugin showed its recent notifications, for the rate limit.
#[derive(Default)]
pub struct NotifyLog(HashMap<String, Vec<Instant>>);

impl NotifyLog {
    /// Counts a notification at `now` if the plugin is under the limit, else says how long until it can.
    pub fn allow(&mut self, plugin_id: &str, now: Instant) -> Result<(), Duration> {
        let sent = self.0.entry(plugin_id.to_string()).or_default();
        sent.retain(|t| now.duration_since(*t) < NOTIFY_WINDOW);
        if sent.len() >= NOTIFY_LIMIT {
            return Err(NOTIFY_WINDOW - now.duration_since(sent[0]));
        }
        sent.push(now);
        Ok(())
    }
}

pub(crate) fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max - 1).chain(['…']).collect()
}

/// The title and body to show, cut to length. The plugin's name leads the title, so a plugin can't pass
/// for Wings or Claude.
pub fn notification(plugin: &Plugin, title: &str, body: &str) -> Result<(String, String), String> {
    let id = &plugin.manifest.id;
    if !plugin.manifest.permissions.notify {
        return Err(format!("{id} may not show notifications"));
    }
    let title = title.trim();
    if title.is_empty() {
        return Err("A notification needs a title".into());
    }
    Ok((format!("{}: {}", plugin.manifest.name, clip(title, NOTIFY_TITLE_MAX)), clip(body.trim(), NOTIFY_BODY_MAX)))
}

/// A transcript entry Claude just wrote, for `wings.onTranscript`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEvent {
    pub session_id: String,
    pub pane_id: String,
    pub entry: Value,
    /// The running plugins whose `permissions.transcript` has the entry's type. Only they get it.
    pub plugins: Vec<String>,
}

/// The event for a line a transcript appended, if it's a JSON entry of a type one of `running` may read.
pub fn transcript_event(running: &[Plugin], line: crate::tail::Line) -> Option<TranscriptEvent> {
    let text = std::str::from_utf8(&line.bytes).ok()?;
    // Most lines are large tool output of a type no plugin reads, so look before parsing.
    let types = || running.iter().flat_map(|p| &p.manifest.permissions.transcript);
    if !types().any(|t| text.contains(&type_marker(t))) {
        return None;
    }
    let entry: Value = serde_json::from_str(text).ok()?;
    let plugins: Vec<String> =
        running.iter().filter(|p| p.manifest.permissions.transcript.iter().any(|t| is_type(t, &entry))).map(|p| p.manifest.id.clone()).collect();
    (!plugins.is_empty()).then_some(TranscriptEvent { session_id: line.session_id, pane_id: line.pane_id, entry, plugins })
}

/// An OpenCode session did something, for `wings.onAgentActivity`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActivityEvent {
    pub session_id: String,
    /// The running plugins whose `permissions.agentActivity` is on. Only they get it.
    pub plugins: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn fake_shell(name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("wings-shell-{name}-{}", std::process::id()));
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

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
        assert_eq!(HOOK_EVENTS, ["SessionStart", "PreToolUse", "Journal"]);
        let dir = std::env::temp_dir().join(format!("wings-hooks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |hooks: &str| format!(r#"{{ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "contributes": {{ "hooks": [{hooks}] }} }}"#);
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
        let manifest = |text: &str| format!(r#"{{ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "contributes": {{ "mcpInstructions": "{text}" }} }}"#);
        fs::write(dir.join("wings-plugin.json"), manifest(&"a".repeat(MAX_MCP_INSTRUCTIONS))).unwrap();
        assert_eq!(load(&dir).unwrap().manifest.contributes.mcp_instructions.len(), MAX_MCP_INSTRUCTIONS);
        fs::write(dir.join("wings-plugin.json"), manifest(&"a".repeat(MAX_MCP_INSTRUCTIONS + 1))).unwrap();
        assert!(load(&dir).err().unwrap().contains("mcpInstructions"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_plugin_in_the_repo_loads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let mut checked = 0;
        for entry in std::fs::read_dir(&root).unwrap() {
            let dir = entry.unwrap().path();
            if dir.join("wings-plugin.json").exists() {
                load(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
                checked += 1;
            }
        }
        assert!(checked > 0);
    }

    #[test]
    #[cfg(unix)]
    fn login_shell_path_gives_up_on_a_slow_profile() {
        let quick = fake_shell("quick", "printf '__WINGS_PATH__/fake/bin__WINGS_PATH__'");
        assert_eq!(login_shell_path(quick.to_str().unwrap(), Duration::from_secs(5)).as_deref(), Some("/fake/bin"));
        // A job left running in the background keeps the output open after the shell is done.
        let background = fake_shell("background", "sleep 30 &\nprintf '__WINGS_PATH__/fake/bin__WINGS_PATH__'");
        let start = Instant::now();
        assert_eq!(login_shell_path(background.to_str().unwrap(), Duration::from_secs(5)).as_deref(), Some("/fake/bin"));
        assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
        let hung = fake_shell("hung", "sleep 30");
        let start = Instant::now();
        assert_eq!(login_shell_path(hung.to_str().unwrap(), Duration::from_millis(300)), None);
        assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
        for path in [quick, background, hung] {
            let _ = fs::remove_file(path);
        }
    }

    fn plugin(dir: &Path, exec: &[&str], transcript: &[&str]) -> Plugin {
        Plugin {
            manifest: Manifest {
                id: "t".into(),
                name: "T".into(),
                version: "0".into(),
                description: String::new(),
                api: 1,
                min_wings_version: None,
                main: "main.js".into(),
                icon: None,
                permissions: Permissions {
                    exec: exec.iter().map(|s| s.to_string()).collect(),
                    transcript: transcript.iter().map(|s| s.to_string()).collect(),
                    open_url: vec!["https://github.com/".into()],
                    fetch: vec!["https://api.example.com/v1/".into()],
                    ..Default::default()
                },
                contributes: Contributes::default(),
            },
            dir: dir.to_path_buf(),
            icon_svg: None,
        }
    }

    #[test]
    fn fetch_only_reaches_declared_https_prefixes() {
        let p = plugin(Path::new("/tmp"), &[], &[]);
        assert_eq!(fetch_url(&p, "GET", "https://api.example.com/v1/timers", None).as_deref(), Some("https://api.example.com/v1/timers"));
        for url in [
            "http://api.example.com/v1/timers",
            "https://api.example.com/v2/x",
            "https://api.example.com.evil.net/v1/",
            "https://user:pw@api.example.com/v1/",
            "not a url",
            // Paths a server might decode or normalise to somewhere outside /v1/.
            "https://api.example.com/v1/../admin",
            "https://api.example.com/v1/..%2Fadmin",
            "https://api.example.com/v1/%2e%2e/admin",
            "https://api.example.com/v1\\..\\admin",
            "https://API.example.com/v1/x",
            "https://api.example.com/v1/..;/admin",
            "https://api.example.com/v1/%252e%252e%252fadmin",
            "https://api.example.com/v1/a%20b",
            "https://api.example.com/v1/x#frag",
        ] {
            assert!(fetch_url(&p, "GET", url, None).is_none(), "{url}");
        }
        assert_eq!(fetch_url(&p, "GET", "https://api.example.com/v1/time-entries?from=2026-10-05&to=2026-10-08", None).as_deref(), Some("https://api.example.com/v1/time-entries?from=2026-10-05&to=2026-10-08"));
        for name in ["Accept", "content-type", "If-None-Match"] {
            assert!(fetch_header_ok(name), "{name}");
        }
        for name in [
            "Host", "x-forwarded-host", "X-Forwarded", "Forwarded", "Cookie", "Transfer-Encoding", "Authorization", "X-Real-IP",
            "X-HTTP-Host-Override", "X-HTTP-Method-Override", "X-Override-URL", "X-Api-Key",
        ] {
            assert!(!fetch_header_ok(name), "{name}");
        }
        assert!(fetch_prefix_ok("https://api.example.com/"));
        assert!(!fetch_prefix_ok("https://api.example.com") && !fetch_prefix_ok("http://x.com/") && !fetch_prefix_ok("https:///x"));
    }

    #[test]
    fn exec_rejects_output_over_the_limit_without_waiting_for_the_timeout() {
        let p = plugin(Path::new("/tmp"), &["yes", "echo"], &[]);
        let start = Instant::now();
        // `yes` never stops on its own: the host has to cut it off, long before the 30 s timeout.
        let err = exec(&p, "yes", &[], None, EXEC_TIMEOUT, None).unwrap_err();
        assert_eq!(err, EXEC_OUTPUT_TOO_LARGE);
        assert!(start.elapsed() < Duration::from_secs(10), "took {:?}", start.elapsed());
        assert!(exec(&p, "echo", &["fine".into()], None, EXEC_TIMEOUT, None).is_ok());
    }

    #[test]
    fn exec_needs_the_declared_subcommand() {
        let p = plugin(Path::new("/tmp"), &["echo hi", "git remote get-url"], &[]);
        assert_eq!(exec(&p, "echo", &["hi".into(), "there".into()], None, EXEC_TIMEOUT, None).unwrap().stdout, "hi there\n");
        assert!(exec(&p, "echo", &["bye".into()], None, EXEC_TIMEOUT, None).unwrap_err().contains("may not run echo bye"));
        assert!(exec(&p, "git", &["status".into()], None, EXEC_TIMEOUT, None).unwrap_err().contains("may not run git status"));
        let missing = Path::new("/no/such/folder");
        assert!(exec(&p, "echo", &["hi".into()], Some(missing), EXEC_TIMEOUT, None).unwrap_err().contains("is not a folder"));
    }

    #[test]
    fn flags_that_run_code_or_write_are_refused() {
        for (program, arg) in [
            ("git", "-c"),
            ("git", "-ccore.fsmonitor=touch x"),
            ("git", "--upload-pack=sh"),
            ("git", "--output=/tmp/x"),
            ("git", "--no-index"),
            ("git", "--upload=sh"),
            ("git", "--outp=/tmp/x"),
            ("gh", "-XDELETE"),
            ("gh", "-iXDELETE"),
            ("glab", "-pfquery=mutation"),
            ("az", "--http-meth=POST"),
            ("az", "--out-file=/tmp/x"),
            ("az", "--out-f"),
            ("az", "@/etc/passwd"),
            ("az", "--body=@/etc/passwd"),
            ("az", "project=@/etc/passwd"),
            ("az", "--output-file=/tmp/x"),
            ("az", "-mPOST"),
            ("az", "-f/etc/passwd"),
            ("az", "-d"),
            ("az", "--source"),
            ("gh", "--method=POST"),
            ("gh", "-fquery=mutation"),
            ("glab", "--show-token"),
            ("glab", "-t"),
            ("az", "--http-method"),
        ] {
            assert!(is_unsafe_flag(program, arg), "{program} {arg}");
        }
        for (program, arg) in [
            ("git", "-C"),
            ("git", "--cached"),
            ("git", "--quiet"),
            ("git", "--no-ext-diff"),
            ("git", "--no-color"),
            ("gh", "--json"),
            ("gh", "-i"),
            ("glab", "--paginate"),
            ("glab", "--raw"),
            ("az", "-o"),
            ("az", "--output"),
            ("az", "--source-branch"),
        ] {
            assert!(!is_unsafe_flag(program, arg), "{program} {arg}");
        }
        // The escape this guards against: a config override that runs a shell command.
        let p = plugin(Path::new("/tmp"), &["git"], &[]);
        let args = ["-c".into(), "core.fsmonitor=touch /tmp/wings-pwned".into(), "status".into()];
        assert!(exec(&p, "git", &args, None, EXEC_TIMEOUT, None).unwrap_err().contains("may not pass -c"));
    }

    #[test]
    fn pr_tracker_calls_pass_its_own_manifest() {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugins/pr-tracker"));
        let p = load(dir).unwrap();
        assert!(p.manifest.contributes.mcp_tools.iter().any(|t| t.name == "failing_checks"));
        let calls: &[(&str, &[&str])] = &[
            ("git", &["remote", "get-url", "origin"]),
            ("git", &["branch", "--show-current"]),
            ("git", &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]),
            ("git", &["fetch", "--quiet", "origin", "main", "feature/x"]),
            ("git", &["diff", "--no-color", "--no-ext-diff", "origin/main...origin/feature/x"]),
            ("gh", &["pr", "list", "--head", "x", "--state", "all", "--limit", "1", "--json", "url,number"]),
            ("gh", &["pr", "view", "https://github.com/o/r/pull/1", "--json", "comments,reviews"]),
            ("gh", &["pr", "diff", "https://github.com/o/r/pull/1"]),
            ("gh", &["api", "repos/o/r/pulls/1/comments", "--paginate", "--slurp"]),
            ("gh", &["api", "repos/o/r/actions/jobs/123"]),
            ("gh", &["api", "repos/o/r/actions/jobs/123/logs", "--allow-escape-sequences"]),
            ("gh", &["auth", "login", "--web", "--clipboard", "--hostname", "github.com"]),
            ("glab", &["api", "--hostname", "gitlab.com", "--paginate", "projects/g%2Fp/merge_requests/1/discussions?per_page=100"]),
            ("glab", &["api", "--hostname", "gitlab.com", "projects/1/jobs/123/trace"]),
            ("glab", &["mr", "diff", "1", "--raw", "--repo", "https://gitlab.com/g/p"]),
            ("glab", &["auth", "status", "--hostname", "gitlab.com"]),
            ("glab", &["auth", "login", "--web", "--hostname", "gitlab.com"]),
            ("az", &["repos", "pr", "list", "--organization", "https://dev.azure.com/o", "--source-branch", "x", "-o", "json"]),
            ("az", &["repos", "pr", "policy", "list", "--id", "1", "-o", "json"]),
            ("az", &["devops", "invoke", "--area", "git", "--resource", "pullRequestThreads", "--api-version", "7.1"]),
            ("az", &["devops", "invoke", "--area", "build", "--resource", "timeline", "--route-parameters", "project=My Project", "buildId=123", "--organization", "https://dev.azure.com/o", "--api-version", "7.1", "-o", "json"]),
            ("az", &["devops", "invoke", "--area", "build", "--resource", "logs", "--route-parameters", "project=My Project", "buildId=123", "logId=4", "--organization", "https://dev.azure.com/o", "--api-version", "7.1", "-o", "json"]),
            ("az", &["login", "--allow-no-subscriptions", "--output", "none"]),
            ("az", &["repos", "pr", "set-vote", "--id", "42", "--vote", "approve-with-suggestions", "--organization", "https://dev.azure.com/o", "-o", "json"]),
        ];
        for (program, args) in calls {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            assert!(may_exec(&p, program, &args), "{program} {args:?} not declared");
            assert!(!args.iter().any(|a| is_unsafe_flag(program, a)), "{program} {args:?} refused");
        }
        assert!(!may_exec(&p, "gh", &["auth".into(), "token".into()]));
        // az's own file and method flags still can't ride along on a vote.
        let vote = |extra: &[&str]| -> Vec<String> { ["repos", "pr", "set-vote", "--id", "1", "--vote", "reset"].iter().chain(extra).map(|a| a.to_string()).collect() };
        assert!(may_exec(&p, "az", &vote(&[])));
        assert!(!may_exec(&p, "az", &vote(&["--detect"])), "a flag that isn't listed");
        assert!(vote(&["--file", "x"]).iter().any(|a| is_unsafe_flag("az", a)));
        assert!(is_unsafe_flag("az", "@/etc/passwd"));
        let posts = [
            ("gh", "repos/o/r/pulls/1/comments/2/replies", None),
            ("gh", "repos/o/r/pulls/1/comments/2/replies", Some("ghe.example.com")),
            ("glab", "projects/g%2Fp/merge_requests/1/approve", Some("gitlab.com")),
            ("glab", "projects/g%2Fp/merge_requests/1/discussions/ab12/notes", Some("gitlab.com")),
            ("az", "https://dev.azure.com/o/My%20Project/_apis/git/repositories/Repo.Name/pullRequests/1/threads/44590/comments?api-version=7.1", None),
        ];
        for (program, url, host) in posts {
            assert!(post_args(&p, &post_request(program, url, host)).is_ok(), "{program} {url} not declared");
        }
        // Approving on GitHub is pinned to the APPROVE event, on github.com and on an Enterprise host.
        let review = |event: &str, host: Option<&str>| {
            let mut r = post_request("gh", "repos/o/r/pulls/1/reviews", host);
            r.fields = serde_json::json!({ "event": event }).as_object().unwrap().clone();
            r
        };
        assert!(post_args(&p, &review("APPROVE", None)).is_ok());
        assert!(post_args(&p, &review("APPROVE", Some("ghe.example.com"))).is_ok());
        assert!(post_args(&p, &review("REQUEST_CHANGES", None)).is_err());
        assert!(post_args(&p, &review("COMMENT", None)).is_err());
        let bb = "https://api.bitbucket.org/2.0/repositories/ws/repo/pullrequests/4";
        let reply = Some(r#"{"content":{"raw":"Thanks"},"parent":{"id":12}}"#);
        assert!(fetch_url(&p, "GET", &format!("{bb}/comments?pagelen=100"), None).is_some());
        assert!(fetch_url(&p, "POST", &format!("{bb}/comments"), reply).is_some());
        assert!(fetch_url(&p, "POST", &format!("{bb}/approve"), None).is_some());
        for method in ["PUT", "DELETE", "PATCH"] {
            assert!(fetch_url(&p, method, &format!("{bb}/comments"), reply).is_none(), "{method}");
        }
        assert!(fetch_url(&p, "POST", &format!("{bb}/merge"), Some("{}")).is_none());
        assert!(fetch_url(&p, "DELETE", &format!("{bb}/approve"), None).is_none());
        let cb = "https://codeberg.org/api/v1/repos/o/r";
        assert!(fetch_url(&p, "GET", &format!("{cb}/pulls/4.diff"), None).is_some());
        assert!(fetch_url(&p, "POST", &format!("{cb}/issues/4/comments"), Some(r#"{"body":"> quoted\n\nThanks"}"#)).is_some());
        assert!(fetch_url(&p, "POST", &format!("{cb}/pulls/4/reviews"), Some(r#"{"event":"APPROVED"}"#)).is_some());
        assert!(fetch_url(&p, "POST", &format!("{cb}/pulls/4/reviews"), Some(r#"{"event":"REQUEST_CHANGES","body":"no"}"#)).is_none());
        assert!(fetch_url(&p, "POST", &format!("{cb}/pulls/4/merge"), Some(r#"{"Do":"merge"}"#)).is_none());
    }

    #[test]
    fn pr_tracker_replies_on_azure_at_dev_azure_com_only() {
        let p = load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugins/pr-tracker"))).unwrap();
        let tail = "_apis/git/repositories/repo/pullRequests/7/threads/44590/comments?api-version=7.1";
        let reply = |address: &str| post_args(&p, &post_request("az", &format!("{address}/{tail}"), None));
        // An organization on {org}.visualstudio.com is reachable at dev.azure.com too, so the plugin posts there.
        assert!(reply("https://dev.azure.com/org/proj").is_ok());
        // The legacy address can't be covered: the host of an `az` entry has no `*`, and no organization is known in advance.
        assert!(reply("https://org.visualstudio.com/proj").is_err());
        assert!(reply("https://org.visualstudio.com/DefaultCollection/proj").is_err());
        assert!(post_entry("az https://*.visualstudio.com/*/_apis/git/repositories/*/pullRequests/*/threads/*/comments?api-version=7.1").is_none());
    }

    #[test]
    fn changes_calls_pass_its_own_manifest() {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugins/changes"));
        let p = load(dir).unwrap();
        assert!(p.manifest.contributes.mcp_tools.iter().any(|t| t.name == "changed_files"));
        assert!(p.manifest.contributes.mcp_tools.iter().any(|t| t.name == "recent_commits"));
        assert!(p.manifest.contributes.views.iter().any(|v| v.id == "graph" && v.opens == ["window", "modal"]));
        let color = ["--no-color", "--no-ext-diff", "--no-textconv", "--no-relative"];
        let diff = |extra: &[&str]| -> Vec<String> { ["diff", "abc123"].iter().chain(extra).chain(&color).map(|a| a.to_string()).collect() };
        let grep = |extra: &[&str]| -> Vec<String> {
            ["grep", "--untracked"].iter().chain(extra).chain(&["--no-color", "-e", "", "--", ":(literal)new file.txt"]).map(|a| a.to_string()).collect()
        };
        let log_format = ["-z", "--no-color", "--decorate=full", "--format=%H%x00%P%x00%an%x00%at%x00%ct%x00%D%x00%s"];
        let log = |extra: &[&str]| -> Vec<String> { ["log"].iter().chain(&log_format).chain(extra).map(|a| a.to_string()).collect() };
        let show = |extra: &[&str]| -> Vec<String> {
            ["show", "--format=", "--diff-merges=first-parent", "-M"].iter().chain(&color).chain(extra).chain(&["abc123", "--"]).map(|a| a.to_string()).collect()
        };
        let calls: Vec<Vec<String>> = vec![
            log(&["--max-count=51", "--skip=50"]),
            log(&["--max-count=51", "--skip=0", "--all"]),
            log(&["--regexp-ignore-case", "--fixed-strings", "--max-count=101", "--grep=fix the -c flag", "--all"]),
            log(&["--regexp-ignore-case", "--fixed-strings", "--max-count=101", "--author=someone"]),
            log(&["--max-count=1", "abc123", "--"]),
            ["log", "--all", "--topo-order"].iter().chain(&log_format).chain(&["--max-count=10000"]).map(|a| a.to_string()).collect(),
            show(&["--numstat", "-z"]),
            show(&["--patch"]),
            ["rev-parse", "--verify", "--quiet", "abc123^{commit}"].map(String::from).to_vec(),
            ["rev-parse", "--show-toplevel"].map(String::from).to_vec(),
            ["rev-parse", "--abbrev-ref", "origin/HEAD"].map(String::from).to_vec(),
            ["rev-parse", "--verify", "--quiet", "main^{commit}"].map(String::from).to_vec(),
            ["branch", "--show-current"].map(String::from).to_vec(),
            ["merge-base", "HEAD", "origin/main"].map(String::from).to_vec(),
            ["ls-files", "--others", "--exclude-standard", "-z"].map(String::from).to_vec(),
            diff(&["--numstat", "-z", "-M"]),
            diff(&["--name-status", "-z", "-M"]),
            diff(&["-M", "--", ":(literal)src/a.ts"]),
            grep(&["-I", "-z", "--count"]),
            grep(&["-I", "-z", "--no-line-number", "--no-column"]),
        ];
        for args in &calls {
            assert!(may_exec(&p, "git", args), "git {args:?} not declared");
            assert!(!args.iter().any(|a| is_unsafe_flag("git", a)), "git {args:?} refused");
        }
        // Read-only: git grep can't open a pager, and branch can't delete or rename.
        assert!(!may_exec(&p, "git", &["grep".into(), "-Ovi".into(), "x".into()]));
        assert!(!may_exec(&p, "git", &["branch".into(), "--show-current".into(), "-D".into(), "x".into()]));
        assert!(!may_exec(&p, "git", &["symbolic-ref".into(), "HEAD".into(), "refs/heads/x".into()]));
        // log and show can't write a file, run a diff or textconv driver, or take flags outside their lists.
        for args in [["log", "--output=x"], ["log", "--ext-diff"], ["show", "--output=x"], ["show", "--textconv"], ["log", "--follow"], ["show", "--stat"]] {
            let args: Vec<String> = args.map(String::from).to_vec();
            assert!(!may_exec(&p, "git", &args), "git {args:?} allowed");
        }
    }

    #[test]
    fn worktrees_calls_pass_its_own_manifest() {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugins/worktrees"));
        let p = load(dir).unwrap();
        assert!(p.manifest.permissions.panes.as_ref().is_some_and(|panes| panes.is_empty()), "plain shells only");
        let calls: &[&[&str]] = &[
            &["rev-parse", "--show-toplevel"],
            &["rev-parse", "--abbrev-ref", "origin/HEAD"],
            &["rev-parse", "--verify", "--quiet", "origin/main^{commit}"],
            &["worktree", "list", "--porcelain"],
            &["worktree", "remove", "--", "/Users/me/dev/app-feature"],
            &["worktree", "prune", "--verbose"],
            &["status", "--porcelain"],
            &["log", "--max-count=1", "--format=%ct"],
            &["branch", "--list", "--merged=origin/main", "--format=%(refname:short)"],
            &["branch", "--list", "--format=%(refname:short)%09%(upstream:track)"],
            &["branch", "-d", "feature/x"],
        ];
        for args in calls {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            assert!(may_exec(&p, "git", &args), "git {args:?} not declared");
            assert!(!args.iter().any(|a| is_unsafe_flag("git", a)), "git {args:?} refused");
        }
        // Nothing can force a removal, delete an unmerged branch, or change anything else.
        for args in [
            &["worktree", "remove", "--force", "/p"][..],
            &["worktree", "remove", "-f", "/p"],
            &["worktree", "remove", "-f", "-f", "/p"],
            &["worktree", "remove", "-ff", "/p"],
            &["worktree", "remove", "--forc", "/p"],
            &["worktree", "remove", "/p", "--force"],
            &["worktree", "prune", "--expire=now"],
            &["worktree", "add", "/p"],
            &["worktree", "move", "/a", "/b"],
            &["worktree", "lock", "/p"],
            &["branch", "-D", "x"],
            &["branch", "-d", "-D", "x"],
            &["branch", "-d", "--force", "x"],
            &["branch", "-df", "x"],
            &["branch", "--delete", "x"],
            &["branch", "newbranch"],
            &["branch", "--list", "-D", "x"],
            &["branch", "--list", "--set-upstream-to=origin/x"],
            &["status", "--porcelain", "--ignore-submodules"],
            &["symbolic-ref", "HEAD", "refs/heads/x"],
            &["config", "user.name", "x"],
            &["reset", "--hard"],
        ] {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            assert!(!may_exec(&p, "git", &args), "git {args:?} allowed");
        }
    }

    #[test]
    fn work_item_calls_pass_its_own_manifest() {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugins/work-item"));
        let manifest: Manifest = serde_json::from_str(&fs::read_to_string(dir.join("wings-plugin.json")).unwrap()).unwrap();
        let p = Plugin { manifest, dir: dir.to_path_buf(), icon_svg: None };
        let org = "https://dev.azure.com/o";
        let calls: &[(&str, &[&str])] = &[
            ("git", &["remote", "get-url", "origin"]),
            ("git", &["branch", "--show-current"]),
            ("az", &["boards", "work-item", "show", "--id", "123456", "--organization", org, "-o", "json"]),
            (
                "az",
                &[
                    "devops", "invoke", "--area", "wit", "--resource", "workItemTypeStates", "--route-parameters",
                    "project=My Project", "type=Tech Story", "--api-version", "7.1", "--organization", org, "-o", "json",
                ],
            ),
            ("az", &["boards", "work-item", "update", "--id", "123456", "--state", "Ready for Peer Review/QA", "--organization", org, "-o", "json"]),
            ("az", &["login", "--allow-no-subscriptions", "--output", "none"]),
            // The updates check: reads only.
            (
                "az",
                &[
                    "boards", "query", "--project", "My Project", "--wiql",
                    "SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = 'My Project' AND [System.AssignedTo] = @Me", "--organization", org, "-o", "json",
                ],
            ),
            (
                "az",
                &[
                    "devops", "invoke", "--area", "wit", "--resource", "updates", "--api-version", "7.1", "--route-parameters", "project=My Project", "id=123456",
                    "--query-parameters", "$skip=200", "--organization", org, "-o", "json",
                ],
            ),
            (
                "az",
                &[
                    "devops", "invoke", "--area", "git", "--resource", "pullRequests", "--api-version", "7.1", "--route-parameters", "project=My Project",
                    "--query-parameters", "searchCriteria.reviewerId=00000000-0000-4000-8000-000000000000", "searchCriteria.status=active", "$top=100", "--organization", org, "-o", "json",
                ],
            ),
            (
                "az",
                &[
                    "devops", "invoke", "--area", "git", "--resource", "pullRequestThreads", "--api-version", "7.1", "--route-parameters", "project=My Project",
                    "repositoryId=r1", "pullRequestId=7", "--organization", org, "-o", "json",
                ],
            ),
        ];
        for (program, args) in calls {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            assert!(may_exec(&p, program, &args), "{program} {args:?} not declared");
            assert!(!args.iter().any(|a| is_unsafe_flag(program, a)), "{program} {args:?} refused");
        }
        assert!(!p.manifest.permissions.notify);
        // The query entry only takes the flags it lists, and no other `boards` subcommand but the declared ones.
        let query = |extra: &[&str]| -> Vec<String> { ["boards", "query"].iter().chain(extra).map(|a| a.to_string()).collect() };
        assert!(may_exec(&p, "az", &query(&["--wiql", "SELECT 1"])));
        assert!(!may_exec(&p, "az", &query(&["--path", "Shared Queries/x"])));
        // The update entry can't read a file into a field or the state, and can't reach other commands.
        let update = |extra: &[&str]| -> Vec<String> { ["boards", "work-item", "update", "--id", "1"].iter().chain(extra).map(|a| a.to_string()).collect() };
        for extra in [&["--state", "@/etc/passwd"][..], &["--fields", "System.Title=@/etc/passwd"], &["--description=@notes.md"], &["-f", "x=1"], &["-d", "x"]] {
            assert!(update(extra).iter().any(|a| is_unsafe_flag("az", a)), "{extra:?}");
        }
        assert!(!may_exec(&p, "az", &["boards".into(), "work-item".into(), "delete".into(), "--id".into(), "1".into()]));
        assert!(!may_exec(&p, "az", &["rest".into()]));
        assert!(p.manifest.permissions.post.is_empty());
        // Only the flags its entry lists: state, not the title, fields or anything else.
        assert!(may_exec(&p, "az", &update(&["--state=Done"])));
        for extra in [&["--title", "x"][..], &["--fields", "System.Title=x"], &["--assigned-to", "x"], &["--stat", "Done"], &["--discussion", "x"]] {
            assert!(!may_exec(&p, "az", &update(extra)), "{extra:?}");
        }
    }

    #[test]
    fn exec_flag_lists_are_checked_on_load() {
        let dir = std::env::temp_dir().join(format!("wings-flags-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |entry: &str| {
            fs::write(dir.join("wings-plugin.json"), serde_json::json!({ "id": "t", "name": "T", "version": "0", "api": 1, "main": "main.js", "permissions": { "exec": [entry] } }).to_string()).unwrap();
            load(&dir)
        };
        assert!(manifest("az boards work-item update [--id --state]").is_ok());
        assert!(manifest("git branch --show-current").is_ok());
        for bad in ["az x [--id", "az x [id]", "az x []", "az x [--id] y", "az x [--a] [--b]"] {
            assert!(manifest(bad).is_err(), "{bad}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    fn post_request(program: &str, url: &str, host: Option<&str>) -> PostRequest {
        let fields = serde_json::json!({ "body": "Thanks, fixed" }).as_object().unwrap().clone();
        PostRequest { program: program.into(), url: url.into(), host: host.map(String::from), fields }
    }

    #[test]
    fn post_only_reaches_declared_paths() {
        let fields = serde_json::Map::new();
        let mut p = plugin(Path::new("/tmp"), &["gh api"], &[]);
        p.manifest.permissions.post = vec!["gh repos/*/*/pulls/*/comments/*/replies".into(), "glab projects/*/merge_requests/*/discussions/*/notes".into()];
        assert!(may_post(&p, "gh", "repos/o/r/pulls/1/comments/2/replies", &fields));
        for url in [
            "repos/o/r/pulls/1/comments/2/replies/x",
            "repos/o/r/pulls/1/comments/replies",
            "repos/o/r/pulls/1/comments/../replies",
            "repos/o/r/pulls/1/comments/%2e%2e/replies",
            "repos/o/r/pulls/1/comments/%252e/replies",
            "repos/o/r/pulls/1/comments/%5c/replies",
            "repos/o/r/pulls/1/comments/2?x=1/replies",
            "repos/o/r/pulls/1/comments/a%2F..%2Fb/replies",
        ] {
            assert!(!may_post(&p, "gh", url, &fields), "{url}");
        }
        assert!(!may_post(&p, "glab", "repos/o/r/pulls/1/comments/2/replies", &fields));
        assert!(may_post(&p, "glab", "projects/g%2Fp/merge_requests/1/discussions/ab12/notes", &fields));
        // An exec permission for `gh api` still can't write.
        assert!(is_unsafe_flag("gh", "--method"));
    }

    #[test]
    fn post_builds_the_command_itself() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.post = vec!["gh repos/*/*/pulls/*/comments/*/replies".into(), "glab projects/*/notes".into(), "az https://dev.azure.com/*/threads".into()];
        let args = post_args(&p, &post_request("gh", "repos/o/r/pulls/1/comments/2/replies", None)).unwrap();
        assert_eq!(args, ["api", "--method", "POST", "repos/o/r/pulls/1/comments/2/replies", "-f", "body=Thanks, fixed"]);
        let mut at_file = post_request("gh", "repos/o/r/pulls/1/comments/2/replies", None);
        at_file.fields.insert("body".into(), "@/etc/passwd".into());
        assert_eq!(post_args(&p, &at_file).unwrap()[5], "body=@/etc/passwd", "sent as text with -f");
        assert!(post_args(&p, &post_request("glab", "projects/1/notes", None)).is_err(), "glab needs a host");
        assert!(post_args(&p, &post_request("glab", "projects/1/notes", Some("-x"))).is_err());
        let az = post_args(&p, &post_request("az", "https://dev.azure.com/o/threads", None)).unwrap();
        assert_eq!(az[..7], ["rest", "--method", "post", "--resource", AZURE_DEVOPS_RESOURCE, "--url", "https://dev.azure.com/o/threads"]);
        assert_eq!(az[8], r#"{"body":"Thanks, fixed"}"#);
        let mut bad = post_request("gh", "repos/o/r/pulls/1/comments/2/replies", None);
        bad.fields.insert("--x".into(), "1".into());
        assert!(post_args(&p, &bad).is_err());
        bad.fields = serde_json::Map::new();
        assert!(post_args(&p, &bad).is_err());
    }

    #[test]
    fn a_post_entry_can_pin_body_fields() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.post = vec!["gh repos/*/*/pulls/*/reviews {event=APPROVE}".into(), "az https://dev.azure.com/*/reviewers/*/vote?api-version=7.1 {vote=10|5|0|-5|-10}".into()];
        let fields = |v: serde_json::Value| v.as_object().unwrap().clone();
        let url = "repos/o/r/pulls/1/reviews";
        assert!(may_post(&p, "gh", url, &fields(serde_json::json!({ "event": "APPROVE" }))));
        assert!(may_post(&p, "gh", url, &fields(serde_json::json!({ "event": "APPROVE", "body": "Looks good" }))));
        for bad in [
            serde_json::json!({ "event": "REQUEST_CHANGES" }),
            serde_json::json!({ "event": "approve" }),
            serde_json::json!({ "event": "APPROVE|COMMENT" }),
            serde_json::json!({ "body": "no event" }),
            serde_json::json!({ "event": 1 }),
        ] {
            assert!(!may_post(&p, "gh", url, &fields(bad.clone())), "{bad}");
        }
        let vote = "https://dev.azure.com/o/reviewers/me/vote?api-version=7.1";
        assert!(may_post(&p, "az", vote, &fields(serde_json::json!({ "vote": -5 }))));
        assert!(!may_post(&p, "az", vote, &fields(serde_json::json!({ "vote": 7 }))));
        // The pin is part of the entry, so an entry with a malformed pin is refused on load.
        for entry in ["gh repos/x {event=APPROVE", "gh repos/x {}", "gh repos/x {event}", "gh repos/x {event=}", "gh repos/x {ev-ent=A}", "gh repos/x {event=A B}"] {
            assert!(post_entry(entry).is_none(), "{entry}");
        }
        assert!(post_entry("gh repos/x {event=APPROVE}").is_some());
    }

    #[test]
    fn a_gh_post_names_an_enterprise_host() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.post = vec!["gh repos/*/*/pulls/*/comments/*/replies".into()];
        let url = "repos/o/r/pulls/1/comments/2/replies";
        let args = post_args(&p, &post_request("gh", url, Some("ghe.example.com"))).unwrap();
        assert_eq!(args, ["api", "--hostname", "ghe.example.com", "--method", "POST", url, "-f", "body=Thanks, fixed"]);
        assert_eq!(post_args(&p, &post_request("gh", url, Some("github.com"))).unwrap()[1], "--method");
        assert!(post_args(&p, &post_request("gh", url, Some("-x"))).is_err());
        assert!(post_args(&p, &post_request("gh", url, Some("a b"))).is_err());
    }

    #[test]
    fn fetch_writes_need_their_own_pinned_entry() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.fetch = vec![
            "GET https://api.example.com/2.0/".into(),
            "POST https://api.example.com/2.0/repos/*/*/pulls/*/comments".into(),
            "POST https://api.example.com/2.0/repos/*/*/pulls/*/approve".into(),
            "POST https://api.example.com/v1/repos/*/*/pulls/*/reviews {event=APPROVED}".into(),
        ];
        let comments = "https://api.example.com/2.0/repos/w/r/pulls/3/comments";
        let body = Some(r#"{"content":{"raw":"Thanks"},"parent":{"id":7}}"#);
        assert!(fetch_url(&p, "GET", "https://api.example.com/2.0/repos/w/r/pulls/3", None).is_some());
        assert!(fetch_url(&p, "head", "https://api.example.com/2.0/user", None).is_some());
        assert!(fetch_url(&p, "POST", comments, body).is_some());
        // A GET prefix doesn't let a write through, anywhere under it.
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            assert!(fetch_url(&p, method, "https://api.example.com/2.0/repos/w/r/pulls/3/merge", Some("{}")).is_none(), "{method}");
        }
        // Only POST, only that path, no query, a JSON object body.
        assert!(fetch_url(&p, "PUT", comments, body).is_none());
        assert!(fetch_url(&p, "DELETE", comments, None).is_none());
        assert!(fetch_url(&p, "POST", "https://api.example.com/2.0/repos/w/r/pulls/3/comments/9", body).is_none());
        assert!(fetch_url(&p, "POST", "https://api.example.com/2.0/repos/w/r/pulls/3/comments?x=1", body).is_none());
        assert!(fetch_url(&p, "POST", "https://api.example.com/2.0/repos/w/r/pulls/../comments", body).is_none());
        assert!(fetch_url(&p, "POST", "https://api.example.com/2.0/repos/w/r/pulls/%2e%2e/comments", body).is_none());
        assert!(fetch_url(&p, "POST", comments, Some("not json")).is_none());
        assert!(fetch_url(&p, "POST", comments, Some("[1]")).is_none());
        let huge = format!(r#"{{"content":"{}"}}"#, "x".repeat(POST_BODY_LIMIT));
        assert!(fetch_url(&p, "POST", comments, Some(&huge)).is_none());
        // No body is fine where nothing is pinned (Bitbucket's approve), but not where a field is.
        assert!(fetch_url(&p, "POST", "https://api.example.com/2.0/repos/w/r/pulls/3/approve", None).is_some());
        let review = "https://api.example.com/v1/repos/o/r/pulls/3/reviews";
        assert!(fetch_url(&p, "POST", review, Some(r#"{"event":"APPROVED"}"#)).is_some());
        assert!(fetch_url(&p, "POST", review, Some(r#"{"event":"REQUEST_CHANGES","body":"x"}"#)).is_none());
        assert!(fetch_url(&p, "POST", review, Some(r#"{"body":"x"}"#)).is_none());
        assert!(fetch_url(&p, "POST", review, None).is_none());
        // A host's own path can't be reached by a `*` (the host is literal).
        assert!(fetch_url(&p, "POST", "https://evil.example.com/2.0/repos/w/r/pulls/3/comments", body).is_none());
    }

    #[test]
    fn fetch_entries_are_checked_on_load() {
        for entry in [
            "https://api.example.com/",
            "GET https://api.example.com/2.0/",
            "POST https://api.example.com/x/*/y",
            "POST https://api.example.com/x/*/reviews {event=APPROVED}",
        ] {
            assert!(fetch_rule(entry).is_some(), "{entry}");
        }
        for entry in [
            "PUT https://api.example.com/x",
            "DELETE https://api.example.com/",
            "GET https://api.example.com/x/*/y",
            "GET https://api.example.com/ {a=b}",
            "https://api.example.com/*/y",
            "POST https://*/x",
            "POST https://api.example.com/x?y=1",
            "POST https://api.example.com {a=b}",
            "POST https://api.example.com/x {a=}",
            "POST http://api.example.com/x",
            "GET",
        ] {
            assert!(fetch_rule(entry).is_none(), "{entry}");
        }
    }

    #[test]
    fn a_pinned_post_goes_out_as_json_only() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.fetch = vec![
            "POST https://api.example.com/v1/repos/*/*/pulls/*/reviews {event=APPROVED}".into(),
            "POST https://api.example.com/v1/repos/*/*/issues/*/comments".into(),
        ];
        assert!(fetch_needs_json(&p, "POST", "https://api.example.com/v1/repos/o/r/pulls/1/reviews"));
        assert!(fetch_needs_json(&p, "post", "https://api.example.com/v1/repos/o/r/pulls/1/reviews"));
        // An unpinned rule has nothing to read two ways, and other methods carry no checked body.
        assert!(!fetch_needs_json(&p, "POST", "https://api.example.com/v1/repos/o/r/issues/1/comments"));
        assert!(!fetch_needs_json(&p, "GET", "https://api.example.com/v1/repos/o/r/pulls/1/reviews"));
    }

    #[test]
    fn a_pinned_body_cannot_hide_a_second_key() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.post = vec!["gh repos/*/*/pulls/*/reviews {event=APPROVE}".into()];
        p.manifest.permissions.fetch = vec!["POST https://api.example.com/v1/repos/*/*/pulls/*/reviews {event=APPROVED}".into()];
        let fields = |v: serde_json::Value| v.as_object().unwrap().clone();
        let url = "repos/o/r/pulls/1/reviews";
        for bad in [
            serde_json::json!({ "event": "APPROVE", "EVENT": "REQUEST_CHANGES" }),
            serde_json::json!({ "event": "APPROVE", "Event": "REQUEST_CHANGES" }),
            serde_json::json!({ "event": "APPROVE", "ſ": "x" }),
            serde_json::json!({ "event": "APPROVE", "\u{212A}ey": "x" }),
        ] {
            assert!(!may_post(&p, "gh", url, &fields(bad.clone())), "{bad}");
        }
        assert!(may_post(&p, "gh", url, &fields(serde_json::json!({ "event": "APPROVE", "body": "ok" }))));
        let review = "https://api.example.com/v1/repos/o/r/pulls/1/reviews";
        for bad in [
            r#"{"event":"APPROVED","EVENT":"REQUEST_CHANGES"}"#,
            r#"{"event":"REQUEST_CHANGES","event":"APPROVED"}"#,
            r#"{"event":"APPROVED","event":"REQUEST_CHANGES"}"#,
            r#"{"event":"APPROVED","ſ":"x"}"#,
            r#"{"event":"APPROVED","\u212Aey":"x"}"#,
            r#"{"ev\u0065nt":"APPROVED","EVENT":"REQUEST_CHANGES"}"#,
        ] {
            assert!(fetch_url(&p, "POST", review, Some(bad)).is_none(), "{bad}");
        }
        assert!(fetch_url(&p, "POST", review, Some(r#"{"event":"APPROVED","body":"ok"}"#)).is_some());
        // An entry that pins nothing is not affected.
        p.manifest.permissions.fetch = vec!["POST https://api.example.com/v1/c/*/comments".into()];
        assert!(fetch_url(&p, "POST", "https://api.example.com/v1/c/1/comments", Some(r#"{"a":1,"A":2}"#)).is_some());
    }

    #[test]
    fn a_post_rule_is_only_a_post_to_exactly_its_path() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.fetch = vec!["POST https://api.example.com/v1/repos/*/pulls/*/comments".into()];
        let ok = "https://api.example.com/v1/repos/o/pulls/3/comments";
        assert!(fetch_url(&p, "POST", ok, Some("{}")).is_some());
        for method in ["HEAD", "OPTIONS", "GET", "TRACE"] {
            assert!(fetch_url(&p, method, ok, Some("{}")).is_none(), "{method}");
        }
        assert!(fetch_url(&p, "POST", &format!("{ok}/"), Some("{}")).is_none(), "trailing slash");
        assert!(fetch_url(&p, "POST", "https://api.example.com/v1/repos/o%2Fx/pulls/3/comments", Some("{}")).is_none(), "%2F in a segment");
        assert!(fetch_url(&p, "POST", "https://api.example.com/v1/repos/o/pulls/3/comments/", Some("{}")).is_none());
    }

    #[test]
    fn a_cli_post_only_names_a_host_it_is_signed_in_to() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.post = vec!["gh repos/*/*/pulls/*/comments/*/replies".into(), "glab projects/*/notes".into()];
        let signed_in = |program: &str| -> Result<Vec<String>, String> { Ok(if program == "gh" { vec!["github.com".into(), "ghe.example.com".into()] } else { vec!["gitlab.example.com".into()] }) };
        assert!(host_is_signed_in("gh", None, &signed_in).is_ok());
        assert!(host_is_signed_in("gh", Some("github.com"), &|_| Err("gh not looked up for its default host".into())).is_ok());
        assert!(host_is_signed_in("gh", Some("GHE.example.com"), &signed_in).is_ok());
        let refused = host_is_signed_in("gh", Some("attacker.example"), &signed_in).unwrap_err();
        assert!(refused.contains("isn't signed in to attacker.example"), "{refused}");
        assert!(host_is_signed_in("glab", Some("gitlab.example.com"), &signed_in).is_ok());
        assert!(host_is_signed_in("glab", Some("gitlab.com"), &signed_in).is_err());
        // post_checked stops before running anything.
        let url = "repos/o/r/pulls/1/comments/2/replies";
        let err = post_checked(&p, &post_request("gh", url, Some("attacker.example")), &signed_in).unwrap_err();
        assert!(err.contains("isn't signed in"), "{err}");
        assert!(post_checked(&p, &post_request("gh", url, Some("a b")), &signed_in).is_err());
        assert_eq!(parse_gh_hosts(r#"{"hosts":{"github.com":[{"state":"success"}],"ghe.example.com":[]}}"#).len(), 2);
        assert!(parse_gh_hosts("not json").is_empty());
        let text = "gitlab.com\n  x gitlab.com: API call failed\n  ! No token found\n\ngitlab.example.com\n  ✓ Logged in to gitlab.example.com\n\n   ERROR  \n\n  X could not authenticate";
        assert_eq!(parse_glab_hosts(text), ["gitlab.com", "gitlab.example.com"]);
    }

    #[test]
    fn post_entries_are_checked_on_load() {
        for entry in ["gh repos/*/replies", "glab projects/*/notes", "az https://dev.azure.com/*/comments"] {
            assert!(post_entry(entry).is_some(), "{entry}");
        }
        for entry in ["curl https://x/", "gh /repos/x", "gh https://api.github.com/x", "az dev.azure.com/x", "az https://*/x", "gh repos/x y", "gh "] {
            assert!(post_entry(entry).is_none(), "{entry}");
        }
    }

    #[test]
    fn exec_only_runs_declared_programs_without_a_shell() {
        let p = plugin(Path::new("/tmp"), &["echo"], &[]);
        let out = exec(&p, "echo", &["hi; rm -rf /".into()], None, EXEC_TIMEOUT, None).unwrap();
        assert_eq!(out.stdout.trim(), "hi; rm -rf /");
        assert!(exec(&p, "ls", &[], None, EXEC_TIMEOUT, None).unwrap_err().contains("may not run"));
        assert!(exec(&p, "/bin/echo", &[], None, EXEC_TIMEOUT, None).is_err());
    }

    #[test]
    fn exec_streams_lines_from_both_pipes() {
        let p = plugin(Path::new("/tmp"), &["sh"], &[]);
        let lines = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = lines.clone();
        let on_line: OnLine = Arc::new(move |l| sink.lock().unwrap().push(l.to_string()));
        let out = exec(&p, "sh", &["-c".into(), "echo out; echo err >&2".into()], None, EXEC_TIMEOUT, Some(on_line)).unwrap();
        let mut got = lines.lock().unwrap().clone();
        got.sort();
        assert_eq!(got, ["err", "out"]);
        assert_eq!((out.stdout.as_str(), out.stderr.as_str()), ("out\n", "err\n"));
    }

    #[test]
    fn exec_timeout_is_capped() {
        assert_eq!(exec_timeout(None), EXEC_TIMEOUT);
        assert_eq!(exec_timeout(Some(10)), Duration::from_secs(1));
        assert_eq!(exec_timeout(Some(3_600_000)), EXEC_TIMEOUT_MAX);
        let p = plugin(Path::new("/tmp"), &["sleep"], &[]);
        assert!(exec(&p, "sleep", &["5".into()], None, Duration::from_secs(1), None).unwrap_err().contains("timed out"));
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
    fn transcript_reads_only_declared_types() {
        let claude = std::env::temp_dir().join(format!("wings-tr-{}", std::process::id()));
        let id = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
        fs::create_dir_all(claude.join("projects/-x")).unwrap();
        fs::write(
            claude.join(format!("projects/-x/{id}.jsonl")),
            "{\"type\":\"user\",\"message\":{}}\n{\"type\":\"pr-link\",\"prNumber\":7,\"prUrl\":\"https://github.com/a/b/pull/7\"}\n",
        )
        .unwrap();
        let p = plugin(&claude, &[], &["pr-link"]);
        let entries = transcript_entries(&p, &claude, id, &["pr-link".into()], None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["prNumber"], 7);
        assert!(transcript_entries(&p, &claude, id, &["user".into()], None).is_err());
        assert!(transcript_entries(&p, &claude, "../../x", &["pr-link".into()], None).is_err());
        let _ = fs::remove_dir_all(&claude);
    }

    #[test]
    fn transcript_last_gives_the_newest_entries_in_order() {
        let claude = std::env::temp_dir().join(format!("wings-tr-last-{}", std::process::id()));
        let id = "4c21b0c5-8113-4f55-9493-fa156c3fa369";
        fs::create_dir_all(claude.join("projects/-x")).unwrap();
        // The assistant entry mentions `"type":"user"` inside its content, which mustn't make it a user entry.
        let lines = [
            r#"{"type":"user","n":1}"#,
            r#"{"type":"assistant","n":2,"message":{"content":[{"type":"user"}]}}"#,
            r#"{"type":"user","n":3}"#,
            "not json",
            r#"{"type":"user","n":4}"#,
        ];
        fs::write(claude.join(format!("projects/-x/{id}.jsonl")), lines.join("\n")).unwrap();
        let p = plugin(&claude, &[], &["user"]);
        let n = |entries: Vec<Value>| entries.iter().map(|e| e["n"].as_i64().unwrap()).collect::<Vec<_>>();
        assert_eq!(n(transcript_entries(&p, &claude, id, &["user".into()], Some(2)).unwrap()), [3, 4]);
        assert_eq!(n(transcript_entries(&p, &claude, id, &["user".into()], Some(10)).unwrap()), [1, 3, 4]);
        assert_eq!(n(transcript_entries(&p, &claude, id, &["user".into()], None).unwrap()), [1, 3, 4]);
        let _ = fs::remove_dir_all(&claude);
    }

    #[test]
    fn transcript_events_go_only_to_plugins_that_read_the_type() {
        let reader = |id: &str, types: &[&str]| {
            let mut p = plugin(Path::new("/tmp"), &[], types);
            p.manifest.id = id.into();
            p
        };
        let running = [reader("timeline", &["user", "assistant"]), reader("cost", &["assistant", "cost-state"]), reader("pr", &["pr-link"])];
        let event = |text: &str| {
            let line = crate::tail::Line { pane_id: "p1".into(), session_id: "s1".into(), bytes: text.as_bytes().to_vec() };
            transcript_event(&running, line)
        };
        let e = event(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#).unwrap();
        assert_eq!((e.plugins.as_slice(), e.pane_id.as_str(), e.session_id.as_str()), (["timeline".to_string(), "cost".to_string()].as_slice(), "p1", "s1"));
        assert_eq!(e.entry["message"]["content"][0]["text"], "hi");
        assert_eq!(event(r#"{"type":"cost-state","costUSD":1.5}"#).unwrap().plugins, ["cost"]);
        assert_eq!(event(r#"{"type":"pr-link","prNumber":7}"#).unwrap().plugins, ["pr"]);
        // A type nobody reads, even when a nested block names a type someone does, and lines that aren't entries.
        assert!(event(r#"{"type":"attachment","content":[{"type":"user"}]}"#).is_none());
        for line in [r#"{"type":"user""#, "not json", r#"["type","user"]"#, r#"{"kind":"user","x":{"type":"user"}}"#, r#"{"type":7}"#] {
            assert!(event(line).is_none(), "{line}");
        }
        let line = |bytes: &[u8]| crate::tail::Line { pane_id: "p1".into(), session_id: "s1".into(), bytes: bytes.to_vec() };
        assert!(transcript_event(&running, line(b"{\"type\":\"user\",\"x\":\"\xff\"}")).is_none());
        assert!(transcript_event(&[], line(b"{\"type\":\"user\"}")).is_none());
    }

    #[test]
    fn attachment_kinds_narrow_what_a_plugin_reads() {
        let claude = std::env::temp_dir().join(format!("wings-tr-att-{}", std::process::id()));
        let id = "5c21b0c5-8113-4f55-9493-fa156c3fa369";
        fs::create_dir_all(claude.join("projects/-x")).unwrap();
        let lines = [
            r#"{"type":"attachment","attachment":{"type":"model","identity":{"modelId":"claude-opus-5-5[1m]"}}}"#,
            r#"{"type":"attachment","attachment":{"type":"file","content":"secret source"}}"#,
            // Says `"type":"model"` inside, but isn't a model attachment.
            r#"{"type":"user","message":{"content":[{"type":"model"}]}}"#,
            r#"{"type":"attachment","attachment":{"type":"deferred_tools_delta","failedMcpServers":[]}}"#,
        ];
        fs::write(claude.join(format!("projects/-x/{id}.jsonl")), lines.join("\n")).unwrap();
        let p = plugin(&claude, &[], &["attachment:model", "attachment:deferred_tools_delta"]);
        let kinds = |entries: Vec<Value>| entries.iter().map(|e| e["attachment"]["type"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let both = ["attachment:model".into(), "attachment:deferred_tools_delta".into()];
        assert_eq!(kinds(transcript_entries(&p, &claude, id, &both, None).unwrap()), ["model", "deferred_tools_delta"]);
        assert_eq!(kinds(transcript_entries(&p, &claude, id, &["attachment:model".into()], Some(5)).unwrap()), ["model"]);
        // One kind doesn't allow every attachment, or another kind.
        assert!(transcript_entries(&p, &claude, id, &["attachment".into()], None).is_err());
        assert!(transcript_entries(&p, &claude, id, &["attachment:file".into()], None).is_err());
        // A plain `attachment` permission still reads them all.
        let all = plugin(&claude, &[], &["attachment"]);
        assert_eq!(transcript_entries(&all, &claude, id, &["attachment".into()], None).unwrap().len(), 3);
        let _ = fs::remove_dir_all(&claude);

        let mut narrow = plugin(Path::new("/tmp"), &[], &["attachment:model"]);
        narrow.manifest.id = "narrow".into();
        let mut wide = plugin(Path::new("/tmp"), &[], &["attachment"]);
        wide.manifest.id = "wide".into();
        let running = [narrow, wide];
        let event = |text: &str| transcript_event(&running, crate::tail::Line { pane_id: "p1".into(), session_id: "s1".into(), bytes: text.as_bytes().to_vec() });
        assert_eq!(event(lines[0]).unwrap().plugins, ["narrow", "wide"]);
        assert_eq!(event(lines[1]).unwrap().plugins, ["wide"]);
        assert!(event(lines[2]).is_none());
    }

    #[test]
    fn statusline_needs_its_permission() {
        let dir = std::env::temp_dir().join(format!("wings-statusline-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |permissions: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": permissions });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir).unwrap()
        };
        assert!(!may_read_statusline(&manifest(serde_json::json!({}))));
        assert!(!may_read_statusline(&manifest(serde_json::json!({ "transcript": ["assistant"] }))));
        assert!(may_read_statusline(&manifest(serde_json::json!({ "statusline": true }))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_url_needs_https_and_a_declared_prefix() {
        let p = plugin(Path::new("/tmp"), &[], &[]);
        assert!(may_open_url(&p, "https://github.com/a/b/pull/1"));
        assert!(!may_open_url(&p, "https://evil.example/"));
        assert!(!may_open_url(&p, "file:///etc/passwd"));
    }

    #[test]
    fn open_url_allows_http_only_to_this_computer() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.open_url.push("http://localhost:".into());
        p.manifest.permissions.open_url.push("http://127.0.0.1:".into());
        assert!(may_open_url(&p, "http://localhost:5173/"));
        assert!(may_open_url(&p, "http://localhost:8765/a/b?c=d"));
        assert!(may_open_url(&p, "http://127.0.0.1:3000/"));
        for url in [
            "http://localhost.evil.com:80/",
            "http://localhost:80@evil.com/",
            "http://localhost:80\\@evil.com/",
            "http://evil.com:80/",
            "http://localhost@evil.com/",
            "http://[::1]:5173/",
            "https://localhost:5173/",
            "http://LOCALHOST:5173/",
        ] {
            assert!(!may_open_url(&p, url), "{url}");
        }
        // Not declared: a plugin with only https prefixes can't open localhost.
        assert!(!may_open_url(&plugin(Path::new("/tmp"), &[], &[]), "http://localhost:5173/"));
        // A broad prefix still can't reach past this computer.
        p.manifest.permissions.open_url.push("http://".into());
        assert!(!may_open_url(&p, "http://example.com/"));
        assert!(may_open_url(&p, "http://[::1]:5173/"));
    }

    #[test]
    fn ports_need_their_permission() {
        let dir = std::env::temp_dir().join(format!("wings-ports-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |permissions: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": permissions });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir).unwrap()
        };
        assert!(!may_read_ports(&manifest(serde_json::json!({}))));
        assert!(!may_read_ports(&manifest(serde_json::json!({ "exec": ["lsof"], "statusline": true }))));
        assert!(may_read_ports(&manifest(serde_json::json!({ "ports": true }))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn agent_activity_needs_its_permission() {
        let dir = std::env::temp_dir().join(format!("wings-activity-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |permissions: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": permissions });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir).unwrap()
        };
        assert!(!may_read_agent_activity(&manifest(serde_json::json!({}))));
        assert!(!may_read_agent_activity(&manifest(serde_json::json!({ "transcript": ["assistant"], "statusline": true }))));
        assert!(may_read_agent_activity(&manifest(serde_json::json!({ "agentActivity": true }))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn disk_usage_needs_its_permission() {
        let dir = std::env::temp_dir().join(format!("wings-diskusage-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |permissions: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": permissions });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir).unwrap()
        };
        assert!(!may_scan_disks(&manifest(serde_json::json!({}))));
        assert!(!may_scan_disks(&manifest(serde_json::json!({ "exec": ["df"], "ports": true }))));
        assert!(may_scan_disks(&manifest(serde_json::json!({ "diskUsage": true }))));
        let _ = fs::remove_dir_all(&dir);
    }

    fn with_panes(panes: Option<&[&str]>) -> Plugin {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.panes = panes.map(|list| list.iter().map(|s| s.to_string()).collect());
        p
    }

    #[test]
    fn pane_commands_must_be_declared() {
        let p = with_panes(Some(&["lazygit", "npm run dev", "claude --resume", "git log"]));
        let input = |command: Option<&str>| pane_input(&p, command, &[]);
        assert_eq!(input(None).unwrap(), None);
        assert_eq!(input(Some("lazygit")).unwrap().as_deref(), Some("lazygit"));
        assert_eq!(input(Some("  npm   run dev ")).unwrap().as_deref(), Some("npm run dev"));
        // Words the plugin adds past the declared entry are quoted.
        assert_eq!(input(Some("npm run dev --port=3000")).unwrap().as_deref(), Some("npm run dev '--port=3000'"));
        let id = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
        assert_eq!(input(Some(&format!("claude --resume {id}"))).unwrap(), Some(format!("claude --resume '{id}'")));

        for undeclared in ["rm -rf /", "npm run build", "npm", "claude", "lazygitx", "/usr/bin/lazygit"] {
            assert!(input(Some(undeclared)).unwrap_err().contains("may not start"), "{undeclared}");
        }
        // Nothing a shell would read as more than plain words: operators, quotes, expansions, globs, newlines.
        for smuggled in [
            "lazygit; rm -rf ~", "lazygit && curl x", "lazygit | sh", "lazygit $(id)", "lazygit `id`", "lazygit 'a'",
            "lazygit a\\", "lazygit ~", "lazygit *", "lazygit >x", "lazygit\nrm", "lazygit\r", "npm run dev #", "",
        ] {
            assert!(input(Some(smuggled)).unwrap_err().contains("only letters"), "{smuggled:?}");
        }
        assert!(input(Some(&format!("lazygit {}", "a".repeat(1100)))).is_err());
        // The same flags `exec` refuses.
        assert!(input(Some("git log -ccore.pager=sh")).unwrap_err().contains("may not pass"));
        // Claude takes only the flags the entry names; a prompt word is fine.
        let claude = with_panes(Some(&["claude", "claude --resume"]));
        assert_eq!(pane_input(&claude, Some("claude Continue-issue-A-1"), &[]).unwrap().as_deref(), Some("claude 'Continue-issue-A-1'"));
        assert_eq!(pane_input(&claude, Some(&format!("claude --resume {id}")), &[]).unwrap(), Some(format!("claude --resume '{id}'")));
        // `args` keep their spaces as one quoted word, a quote inside escaped; flags and control characters are refused.
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            pane_input(&claude, Some("claude"), &args(&["Continue issue A-1. It's in review"])).unwrap().as_deref(),
            Some("claude 'Continue issue A-1. It'\\''s in review'")
        );
        for bad in [&["--dangerously-skip-permissions"][..], &["line\nrm -rf ~"], &["tab\there"], &[""]] {
            assert!(pane_input(&claude, Some("claude"), &args(bad)).is_err(), "{bad:?}");
        }
        assert!(pane_input(&claude, None, &args(&["x"])).is_err());
        assert!(pane_input(&claude, Some("claude"), &args(&[&"a".repeat(5000)])).is_err());
        for flag in ["claude --dangerously-skip-permissions", "claude --permission-mode=bypassPermissions", "claude x --mcp-config=a.json", "claude --resume x --settings=s.json", "claude -p x"] {
            assert!(pane_input(&claude, Some(flag), &[]).unwrap_err().contains("may not pass"), "{flag}");
        }
        // opencode the same: its prompt and session, but not --auto (approves every permission) or --server.
        let opencode = with_panes(Some(&["opencode --prompt", "opencode --session"]));
        assert_eq!(pane_input(&opencode, Some("opencode --session ses_abc12345"), &[]).unwrap().as_deref(), Some("opencode --session 'ses_abc12345'"));
        assert_eq!(pane_input(&opencode, Some("opencode --prompt"), &args(&["Continue issue A-1."])).unwrap().as_deref(), Some("opencode --prompt 'Continue issue A-1.'"));
        for flag in ["opencode --prompt --auto", "opencode --session ses_abc12345 --server=http://x", "opencode --session x -c"] {
            assert!(pane_input(&opencode, Some(flag), &[]).unwrap_err().contains("may not pass"), "{flag}");
        }
        // A second value would be read as a subcommand, so an agent gets one.
        assert!(pane_input(&opencode, Some("opencode --session ses_abc12345 uninstall"), &[]).unwrap_err().contains("one value"));
        assert!(pane_input(&opencode, Some("opencode --session ses_abc12345"), &args(&["uninstall"])).unwrap_err().contains("one value"));
        assert!(pane_input(&opencode, Some("opencode --prompt"), &args(&["hi", "uninstall"])).unwrap_err().contains("one value"));
        assert!(pane_input(&claude, Some("claude mcp remove"), &[]).unwrap_err().contains("one value"));

        // `[]` allows plain shells only; no `panes` allows nothing, not even focus.
        let shells = with_panes(Some(&[]));
        assert_eq!(pane_input(&shells, None, &[]).unwrap(), None);
        assert!(pane_input(&shells, Some("lazygit"), &[]).unwrap_err().contains("may not start"));
        assert!(may_use_panes(&shells));
        let none = with_panes(None);
        assert!(pane_input(&none, None, &[]).unwrap_err().contains("may not open panes"));
        assert!(!may_use_panes(&none));
    }

    #[test]
    fn pane_entries_are_checked_when_the_plugin_loads() {
        let dir = std::env::temp_dir().join(format!("wings-panes-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |panes: &[&str]| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": { "panes": panes } });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir)
        };
        assert_eq!(manifest(&["lazygit", "npm run dev"]).unwrap().manifest.permissions.panes.unwrap(), ["lazygit", "npm run dev"]);
        for bad in ["npm run dev && curl x | sh", "", " ", "echo $HOME"] {
            assert!(manifest(&[bad]).err().unwrap().contains("panes entry"), "{bad:?}");
        }
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
        for (text, svg) in [("<svg/>", true), ("\u{feff}\n<svg width=\"1\">", true), ("<svgx/>", false), ("<svg", false), ("", false), ("<!-- never closed <svg/>", false), ("<?xml <svg/>", false)] {
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
        for extra in [serde_json::json!({ "newTab": true }), serde_json::json!({ "multiple": true })] {
            assert!(manifest(extra, serde_json::json!(["modal"])).err().unwrap().contains("need \"tab\" in opens"));
        }
        // Without them a view serializes as before, so plugins approved earlier keep their grant.
        let plain = manifest(serde_json::json!({}), serde_json::json!(["tab"])).unwrap().manifest.contributes.views[0].clone();
        assert!(!plain.new_tab && !plain.multiple);
        let json = serde_json::to_value(&plain).unwrap();
        assert!(json.get("newTab").is_none() && json.get("multiple").is_none() && json.get("button").is_none());
        assert!(plain.button);
        // `button: false` keeps a one-item page out of the title bar, and is kept when saved.
        let item = manifest(serde_json::json!({ "multiple": true, "button": false }), serde_json::json!(["tab"])).unwrap().manifest.contributes.views[0].clone();
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

    #[test]
    fn a_grant_is_for_one_tab_in_one_project_and_folder() {
        let dir = std::env::temp_dir().canonicalize().unwrap();
        let mut grants = vec![("s1".to_string(), dir.clone())];
        assert_eq!(take_grant(&mut grants, "s2", &dir), None);
        assert_eq!(take_grant(&mut grants, "s1", Path::new("/")), None);
        assert_eq!(take_grant(&mut grants, "s1", &dir), Some(dir.clone()));
        assert_eq!(take_grant(&mut grants, "s1", &dir), None, "used up");
    }

    #[test]
    fn pane_cwd_must_be_inside_a_project() {
        // Canonical, since macOS temp folders sit behind the /var symlink.
        let root = std::env::temp_dir().join(format!("wings-cwd-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        for dir in ["app/src", "app/nested/deep", "app2", "outside"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("app/file.txt"), "").unwrap();
        std::os::unix::fs::symlink(root.join("outside"), root.join("app/link-out")).unwrap();
        let projects = [root.join("app"), root.join("app/nested"), root.join("app2")];
        let cwd = |dir: &str, current| pane_cwd(&root.join(dir), &projects, current);

        assert_eq!(cwd("app", None).unwrap(), (0, root.join("app")));
        assert_eq!(cwd("app/src", None).unwrap(), (0, root.join("app/src")));
        // The deepest project holds it, unless the project on screen does.
        assert_eq!(cwd("app/nested/deep", None).unwrap().0, 1);
        assert_eq!(cwd("app/nested/deep", Some(0)).unwrap().0, 0);
        assert_eq!(cwd("app/src", Some(2)).unwrap().0, 0);

        for outside in ["outside", "app/../outside", "app/link-out", "app/src/../../outside", "."] {
            assert!(cwd(outside, Some(0)).unwrap_err().contains("isn't inside"), "{outside}");
        }
        // `app2` only shares a prefix with `app`; with app2 gone it's in no project.
        assert!(pane_cwd(&root.join("app2"), &projects[..2], None).unwrap_err().contains("isn't inside"));
        assert!(pane_cwd(Path::new("/"), &projects, None).unwrap_err().contains("isn't inside"));
        assert!(cwd("app/missing", None).unwrap_err().contains("is not a folder"));
        assert!(cwd("app/file.txt", None).unwrap_err().contains("is not a folder"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn notifications_need_permission_are_cut_to_length_and_rate_limited() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        assert!(notification(&p, "Done", "").unwrap_err().contains("may not show notifications"));
        p.manifest.permissions.notify = true;
        assert_eq!(notification(&p, " Build passed ", "main is green").unwrap(), ("T: Build passed".into(), "main is green".into()));
        assert!(notification(&p, "  ", "body").unwrap_err().contains("needs a title"));
        let (title, body) = notification(&p, &"t".repeat(500), &"ü".repeat(5000)).unwrap();
        assert_eq!((title.chars().count(), body.chars().count()), (3 + NOTIFY_TITLE_MAX, NOTIFY_BODY_MAX));
        assert!(body.ends_with('…'));

        let mut log = NotifyLog::default();
        let start = Instant::now();
        for i in 0..NOTIFY_LIMIT as u64 {
            assert!(log.allow("a", start + Duration::from_secs(i)).is_ok());
        }
        assert_eq!(log.allow("a", start + Duration::from_secs(10)), Err(Duration::from_secs(50)));
        // Each plugin has its own budget, and a refused call doesn't use any.
        assert!(log.allow("b", start + Duration::from_secs(10)).is_ok());
        assert!(log.allow("a", start + Duration::from_secs(59)).is_err());
        assert!(log.allow("a", start + Duration::from_secs(60)).is_ok());
        assert!(log.allow("a", start + Duration::from_secs(60)).is_err());
    }
}
