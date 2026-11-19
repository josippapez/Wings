//! Open a project or worktree folder, or a file at a line, in an editor: VS Code, Cursor and Zed through their command-line tools,
//! and on macOS any other editor app found in /Applications through `open -a`. No shell is involved: the
//! folder is one argument of the program Wings starts.
//!
//! The CLIs (`code`, `cursor`, `zed`) take a folder as their argument: `code [options] [paths...]` and
//! `zed [OPTIONS] [PATHS]...` (checked against their `--help` and docs). They only exist on PATH when the user
//! installed them (VS Code: "Shell Command: Install 'code' command in PATH"; Zed: its CLI install action), so
//! the app bundle is the fallback.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::Serialize;

struct Editor {
    id: &'static str,
    name: &'static str,
    /// The command-line tool, when the editor has one.
    cli: Option<&'static str>,
    /// The macOS app's name, the folder name without `.app`.
    app: &'static str,
}

const fn editor(id: &'static str, name: &'static str, cli: Option<&'static str>, app: &'static str) -> Editor {
    Editor { id, name, cli, app }
}

/// In the order the picker lists them.
const EDITORS: [Editor; 12] = [
    editor("vscode", "VS Code", Some("code"), "Visual Studio Code"),
    editor("cursor", "Cursor", Some("cursor"), "Cursor"),
    editor("zed", "Zed", Some("zed"), "Zed"),
    editor("xcode", "Xcode", None, "Xcode"),
    editor("android-studio", "Android Studio", None, "Android Studio"),
    editor("sublime", "Sublime Text", Some("subl"), "Sublime Text"),
    editor("nova", "Nova", None, "Nova"),
    editor("idea", "IntelliJ IDEA", None, "IntelliJ IDEA"),
    editor("webstorm", "WebStorm", None, "WebStorm"),
    editor("pycharm", "PyCharm", None, "PyCharm"),
    editor("goland", "GoLand", None, "GoLand"),
    editor("rustrover", "RustRover", None, "RustRover"),
];

#[derive(Debug, Clone, PartialEq)]
enum Launch {
    Cli(PathBuf),
    App(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorView {
    pub id: String,
    pub name: String,
}

/// The installed editors, with how each starts: its CLI when that's on PATH, else its app.
fn detect(find: &dyn Fn(&str) -> Option<PathBuf>, has_app: &dyn Fn(&str) -> bool) -> Vec<(&'static Editor, Launch)> {
    EDITORS
        .iter()
        .filter_map(|e| {
            let launch = e.cli.and_then(find).map(Launch::Cli).or_else(|| has_app(e.app).then_some(Launch::App(e.app)))?;
            Some((e, launch))
        })
        .collect()
}

/// The program and arguments that open `folder`.
fn argv(launch: &Launch, folder: &Path) -> (PathBuf, Vec<OsString>) {
    match launch {
        Launch::Cli(cli) => (cli.clone(), vec![folder.into()]),
        Launch::App(app) => (PathBuf::from("/usr/bin/open"), vec!["-a".into(), (*app).into(), folder.into()]),
    }
}

/// The program and arguments that open `file` at `line`: VS Code and Cursor take `--goto file:line`, Zed and Sublime Text
/// `file:line` (each CLI's documented form). An app without a CLI opens the file, at its top.
fn file_argv(id: &str, launch: &Launch, file: &Path, line: Option<u32>) -> (PathBuf, Vec<OsString>) {
    let at = |line: u32| {
        let mut at = OsString::from(file);
        at.push(format!(":{line}"));
        at
    };
    match (launch, line) {
        (Launch::Cli(cli), Some(line)) if matches!(id, "vscode" | "cursor") => (cli.clone(), vec!["--goto".into(), at(line)]),
        (Launch::Cli(cli), Some(line)) if matches!(id, "zed" | "sublime") => (cli.clone(), vec![at(line)]),
        _ => argv(launch, file),
    }
}

fn app_installed(app: &str) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let bundle = format!("{app}.app");
    Path::new("/Applications").join(&bundle).is_dir() || dirs::home_dir().is_some_and(|h| h.join("Applications").join(&bundle).is_dir())
}

fn installed() -> Vec<(&'static Editor, Launch)> {
    detect(&wings_plugins::exec::find_program, &app_installed)
}

pub fn list() -> Vec<EditorView> {
    installed().into_iter().map(|(e, _)| EditorView { id: e.id.into(), name: e.name.into() }).collect()
}

/// Opens `folder` in the editor with this id. The error is the editor's own text when it fails.
pub fn open(editor_id: &str, folder: &Path) -> Result<(), String> {
    if !folder.is_absolute() || !folder.is_dir() {
        return Err(format!("{} isn't a folder", folder.display()));
    }
    let (editor, launch) = installed().into_iter().find(|(e, _)| e.id == editor_id).ok_or("That editor isn't installed")?;
    let (program, args) = argv(&launch, folder);
    let out = Command::new(program).args(args).stdin(Stdio::null()).output().map_err(|e| format!("Couldn't start {}: {e}", editor.name))?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&out.stderr);
    Err(if text.trim().is_empty() { format!("{} exited with {}", editor.name, out.status) } else { text.trim().to_string() })
}

/// Whether an editor opens `file` as a workspace or project, whose settings and tasks it takes up, rather than as a
/// document: VS Code's and Cursor's `.code-workspace`, Sublime Text's project files, IntelliJ's `.ipr`/`.iws`, and Xcode's
/// project and workspace data (its `.xcodeproj` and `.xcworkspace` are folders, which never open as a file). The same
/// editors open the whole project folder from Wings' "Open in" button; this only keeps a click on a file a file.
fn opens_as_workspace(file: &Path) -> bool {
    let extension = file.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    matches!(
        extension.as_deref(),
        Some("code-workspace" | "sublime-project" | "sublime-workspace" | "ipr" | "iws" | "xcworkspacedata" | "pbxproj")
    )
}

/// Opens `file` in the editor with this id, at `line` where the editor can. The error is the editor's own text when it
/// fails.
pub fn open_file(editor_id: &str, file: &Path, line: Option<u32>) -> Result<(), String> {
    if !file.is_absolute() || !file.is_file() {
        return Err(format!("{} isn't a file", file.display()));
    }
    if opens_as_workspace(file) {
        return Err(format!("{} is an editor's workspace, which opens with its settings and tasks, not as a file", file.display()));
    }
    let (editor, launch) = installed().into_iter().find(|(e, _)| e.id == editor_id).ok_or("That editor isn't installed")?;
    let (program, args) = file_argv(editor.id, &launch, file, line);
    let out = Command::new(program).args(args).stdin(Stdio::null()).output().map_err(|e| format!("Couldn't start {}: {e}", editor.name))?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&out.stderr);
    Err(if text.trim().is_empty() { format!("{} exited with {}", editor.name, out.status) } else { text.trim().to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path_with(programs: &'static [&'static str]) -> impl Fn(&str) -> Option<PathBuf> {
        move |p| programs.contains(&p).then(|| PathBuf::from("/usr/local/bin").join(p))
    }

    fn ids(found: &[(&Editor, Launch)]) -> Vec<&'static str> {
        found.iter().map(|(e, _)| e.id).collect()
    }

    #[test]
    fn finds_editors_by_cli_and_by_app() {
        let found = detect(&path_with(&["code", "zed", "ls"]), &|a| a == "Xcode" || a == "Cursor");
        assert_eq!(ids(&found), ["vscode", "cursor", "zed", "xcode"]);
        assert_eq!(found[0].1, Launch::Cli(PathBuf::from("/usr/local/bin/code")));
        assert_eq!(found[1].1, Launch::App("Cursor"));
    }

    #[test]
    fn the_cli_wins_over_the_app() {
        let found = detect(&path_with(&["cursor"]), &|a| a == "Cursor");
        assert_eq!(found[0].1, Launch::Cli(PathBuf::from("/usr/local/bin/cursor")));
    }

    #[test]
    fn nothing_installed_is_an_empty_list() {
        assert!(detect(&path_with(&[]), &|_| false).is_empty());
    }

    #[test]
    fn the_folder_is_one_argument_even_with_spaces_and_quotes() {
        let folder = Path::new("/code/my app; rm -rf ~/'x'");
        let (program, args) = argv(&Launch::Cli(PathBuf::from("/usr/local/bin/zed")), folder);
        assert_eq!((program, args), (PathBuf::from("/usr/local/bin/zed"), vec![OsString::from(folder)]));
        let (program, args) = argv(&Launch::App("Visual Studio Code"), folder);
        assert_eq!(program, PathBuf::from("/usr/bin/open"));
        assert_eq!(args, vec![OsString::from("-a"), "Visual Studio Code".into(), folder.into()]);
    }

    #[test]
    fn a_file_opens_at_its_line_in_the_form_each_editor_takes() {
        let file = Path::new("/code/my app/src/cart.ts");
        let cli = |p: &str| Launch::Cli(PathBuf::from(p));
        let at = OsString::from("/code/my app/src/cart.ts:42");
        assert_eq!(file_argv("vscode", &cli("/bin/code"), file, Some(42)).1, vec![OsString::from("--goto"), at.clone()]);
        assert_eq!(file_argv("cursor", &cli("/bin/cursor"), file, Some(42)).1, vec![OsString::from("--goto"), at.clone()]);
        assert_eq!(file_argv("zed", &cli("/bin/zed"), file, Some(42)).1, vec![at.clone()]);
        assert_eq!(file_argv("sublime", &cli("/bin/subl"), file, Some(42)).1, vec![at]);
        // No line, or an app without a CLI: the file alone.
        assert_eq!(file_argv("vscode", &cli("/bin/code"), file, None).1, vec![OsString::from(file)]);
        let (program, args) = file_argv("idea", &Launch::App("IntelliJ IDEA"), file, Some(42));
        assert_eq!(program, PathBuf::from("/usr/bin/open"));
        assert_eq!(args, vec![OsString::from("-a"), "IntelliJ IDEA".into(), file.into()]);
    }

    #[test]
    fn refuses_a_workspace_file_which_an_editor_opens_with_its_settings() {
        let folder = std::env::temp_dir().join(format!("wings-editors-workspace-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        for name in ["shop.code-workspace", "shop.sublime-project", "shop.SUBLIME-WORKSPACE", "shop.ipr", "shop.iws", "contents.xcworkspacedata", "project.pbxproj"] {
            let file = folder.join(name);
            std::fs::write(&file, "{}").unwrap();
            assert!(open_file("vscode", &file, None).unwrap_err().ends_with("not as a file"), "{name}");
        }
        assert!(!opens_as_workspace(Path::new("/code/shop/settings.json")));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn refuses_a_relative_missing_or_folder_path_as_a_file() {
        assert!(open_file("vscode", Path::new("relative.ts"), None).is_err());
        assert!(open_file("vscode", Path::new("/definitely/not/here.ts"), Some(1)).is_err());
        assert!(open_file("vscode", Path::new("/"), None).is_err());
    }

    #[test]
    fn refuses_a_relative_or_missing_folder() {
        assert!(open("vscode", Path::new("relative")).is_err());
        assert!(open("vscode", Path::new("/definitely/not/here")).is_err());
    }
}
