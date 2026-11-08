//! A diff's syntax colours, as the desktop's `diff-viewer.tsx` gets them from @pierre/diffs: Shiki with the
//! `pierre-dark` theme, the language its `getFiletypeFromFileName` picks from the file's name, and each hunk's old lines
//! and new lines highlighted apart, each from the hunk's first line, since a patch holds no more of the file. The
//! grammars are syntect's and bat's (two-face), TextMate ones like Shiki's, so the theme's scope rules apply as they are.

use std::{ops::Range, sync::OnceLock};

use syntect::{
    easy::HighlightLines,
    highlighting::{
        Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
    },
    parsing::{SyntaxReference, SyntaxSet},
};

use super::patch::{DiffFile, DiffLineKind};

/// The text colour where no rule of the theme applies: its `editor.foreground`.
pub const FOREGROUND: u32 = 0xfafafa;
/// A line this long or longer, in UTF-16 units, is drawn plain, as @pierre/diffs' `tokenizeMaxLineLength`.
const MAX_LINE: usize = 1000;
/// A side of a file with more lines than this is drawn plain, as @pierre/diffs' `tokenizeMaxLength`.
const MAX_LINES: usize = 100_000;

/// A stretch of a line in one colour: where it ends, in bytes of the line's text, and its colour as 0xrrggbb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub end: usize,
    pub color: u32,
    pub italic: bool,
}

/// A line's colours on the old side of its hunk and on the new side. A removed line has only the old, an added one
/// only the new, an unchanged one both, as each side is highlighted on its own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineColors {
    pub old: Vec<Span>,
    pub new: Vec<Span>,
}

/// Each line's colours, by hunk and line, as `DiffFile::hunks`. Empty for a file in a language with no grammar.
pub type FileColors = Vec<Vec<LineColors>>;

/// The grammar @pierre/diffs would pick for the file at `path`, by syntect's name for it, or None for plain text.
/// As its `getFiletypeFromFileName`: the whole name, then the last two extensions, then the last one, and then the
/// same for the file's name without its folders.
fn language(path: &str) -> Option<&'static str> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let by_name = |name: &str| {
        let compound = name
            .rsplit_once('.')
            .and_then(|(rest, _)| rest.rfind('.'))
            .map(|dot| &name[dot + 1..]);
        let ext = name.rsplit_once('.').map(|(_, ext)| ext);
        grammar(name)
            .or_else(|| compound.and_then(grammar))
            .or_else(|| ext.and_then(grammar))
    };
    by_name(path).or_else(|| by_name(name))
}

/// The grammar for each of @pierre/diffs' `EXTENSION_TO_FILE_FORMAT` keys, for the languages two-face has. JavaScript
/// takes the TypeScript grammars: Shiki's JavaScript grammar is made from the same source as its TypeScript one, so
/// they give the same scopes, which syntect's own JavaScript grammar doesn't.
const GRAMMARS: &[(&str, &[&str])] = &[
    (
        "TypeScript",
        &["ts", "mts", "cts", "component.ts", "js", "mjs", "cjs"],
    ),
    ("TypeScriptReact", &["tsx", "jsx"]),
    ("Rust", &["rs"]),
    ("CSS", &["css"]),
    ("SCSS", &["scss"]),
    ("Less", &["less"]),
    ("JSON", &["json", "jsonc", "json5"]),
    ("Markdown", &["md", "markdown"]),
    ("YAML", &["yaml", "yml"]),
    ("Bourne Again Shell (bash)", &["sh", "bash", "zsh"]),
    ("C#", &["cs"]),
    ("TOML", &["toml"]),
    ("HTML", &["html", "htm", "component.html"]),
    ("XML", &["xml", "xsl", "xslt"]),
    ("Python", &["py", "pyw", "pyi"]),
    ("Go", &["go"]),
    ("Java", &["java"]),
    ("Kotlin", &["kt", "kts"]),
    ("Swift", &["swift"]),
    ("C", &["c"]),
    ("C++", &["cpp", "hpp", "cc", "cxx", "hh"]),
    ("Ruby", &["rb", "Gemfile", "Rakefile"]),
    ("PHP", &["php"]),
    ("SQL", &["sql"]),
    ("Dockerfile", &["Dockerfile", "dockerfile"]),
    ("Makefile", &["Makefile", "makefile", "mk"]),
    ("Diff", &["diff", "patch"]),
    ("INI", &["ini", "cfg"]),
    ("Lua", &["lua"]),
    ("Vue Component", &["vue"]),
    ("Svelte", &["svelte"]),
];

fn grammar(key: &str) -> Option<&'static str> {
    GRAMMARS
        .iter()
        .find(|(_, keys)| keys.contains(&key))
        .map(|(name, _)| *name)
}

/// Scopes that bat's grammars name otherwise than VS Code's, which Shiki runs, each with the colour pierre-dark gives
/// VS Code's name. Each names its innermost scope in full, to outweigh the theme's broader rule for it.
const RENAMED: &[(&str, u32)] = &[
    // JSON keys: VS Code's `support.type.property-name.json`.
    ("meta.mapping.key.json string.quoted.double.json", 0xff855e),
    (
        "meta.mapping.key.json punctuation.definition.string.begin.json",
        0xff855e,
    ),
    (
        "meta.mapping.key.json punctuation.definition.string.end.json",
        0xff855e,
    ),
    // CSS custom properties: `variable.css`.
    ("support.type.custom-property", 0xffa359),
    (
        "support.type.custom-property.css punctuation.definition.custom-property.css",
        0xffa359,
    ),
    // A number's decimal point is part of the number.
    ("constant.numeric punctuation.separator.decimal", 0x68cdf2),
    // TOML keys and table names: `variable.other.key.toml` and `entity.other.attribute-name.table.toml`.
    ("meta.tag.key.toml entity.name.tag.toml", 0xffa359),
    ("entity.name.table.toml", 0x9d6afb),
    // Markdown link text, inline code and headings.
    ("meta.image.inline.description.markdown", 0x9d6afb),
    ("meta.link.inline.description.markdown", 0x9d6afb),
    ("markup.raw.inline.markdown", 0x5ecc71),
    (
        "markup.raw.inline.markdown punctuation.definition.raw",
        0x5ecc71,
    ),
    (
        "markup.heading punctuation.definition.heading.begin.markdown",
        0xff855e,
    ),
    // Shell: a command's arguments are `string.unquoted.argument.shell`, its options `constant.other.option`.
    ("meta.function-call.arguments.shell", 0x5ecc71),
    ("variable.parameter.option.shell", 0xffd452),
    (
        "variable.parameter.option.shell punctuation.definition.parameter.shell",
        0xffd452,
    ),
    ("punctuation.definition.variable.shell", 0xffa359),
    // Rust: `fn` is a keyword, parameters are `variable.other.rust`, and a format string's `{name}` is part of the
    // string. Names the grammar leaves unscoped are `rust_names`'.
    ("storage.type.function.rust", 0xff678d),
    ("variable.parameter.rust", 0xffa359),
    ("variable.annotation.rust", 0xfafafa),
    (
        "string.quoted punctuation.definition.string.begin.rust",
        0x636363,
    ),
    (
        "string.quoted punctuation.definition.string.end.rust",
        0x636363,
    ),
    (
        "string.quoted.double.rust constant.other.placeholder.rust",
        0x5ecc71,
    ),
    // C#: `using` paths, built-in types (`keyword.type`), members and parameters (uncoloured), attributes (types).
    ("meta.path.cs", 0xffab16),
    ("storage.type.cs", 0xff678d),
    ("variable.other.member.cs", 0xfafafa),
    ("variable.parameter.cs", 0xfafafa),
    ("variable.annotation.cs", 0xd568ea),
];

/// The grammars and the theme, loaded the first time a diff is highlighted, off the window's thread.
fn assets() -> &'static (SyntaxSet, Theme) {
    static ASSETS: OnceLock<(SyntaxSet, Theme)> = OnceLock::new();
    ASSETS.get_or_init(|| (two_face::syntax::extra_newlines(), theme()))
}

/// The desktop's theme, @pierre/theme 2.0.0's `pierre-dark.json`, as a syntect theme. syntect has no `>` (a scope right
/// under the one before), so those rules match any scope under it, which the few such rules (JSON keys and values)
/// don't tell apart.
fn theme() -> Theme {
    let json: serde_json::Value =
        serde_json::from_str(include_str!("pierre-dark.json")).expect("pierre-dark.json is JSON");
    let color = |hex: &str| {
        let hex = u32::from_str_radix(hex.trim_start_matches('#'), 16).ok()?;
        Some(Color {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
            a: 0xff,
        })
    };
    // Of two rules as specific, VS Code's takes the later and syntect the first, so syntect gets them last first.
    let scopes = json["tokenColors"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .filter_map(|rule| {
            let scope = match &rule["scope"] {
                serde_json::Value::String(scope) => scope.clone(),
                serde_json::Value::Array(scopes) => scopes
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => return None,
            };
            let settings = &rule["settings"];
            Some(ThemeItem {
                scope: scope.replace(" > ", " ").parse::<ScopeSelectors>().ok()?,
                style: StyleModifier {
                    foreground: settings["foreground"].as_str().and_then(color),
                    background: None,
                    font_style: settings["fontStyle"].as_str().map(|style| {
                        if style.contains("italic") {
                            FontStyle::ITALIC
                        } else {
                            FontStyle::empty()
                        }
                    }),
                },
            })
        });
    let renamed = RENAMED.iter().map(|(scope, rgb)| ThemeItem {
        scope: scope.parse().expect("RENAMED holds scope selectors"),
        style: StyleModifier {
            foreground: color(&format!("{rgb:06x}")),
            ..StyleModifier::default()
        },
    });
    let scopes = renamed.chain(scopes).collect();
    Theme {
        name: Some("pierre-dark".into()),
        author: None,
        // Text no rule colours comes out see-through, for `Side::line` to tell it apart; it's drawn in `FOREGROUND`.
        settings: ThemeSettings {
            foreground: Some(Color {
                a: 0,
                ..Color::WHITE
            }),
            ..ThemeSettings::default()
        },
        scopes,
    }
}

/// The colours of each line of `file`, or none when its language has no grammar or a side has more than `MAX_LINES`.
/// Slow on a big file: run it off the window's thread.
pub fn file_colors(file: &DiffFile) -> FileColors {
    let path = file
        .new_path
        .as_deref()
        .or(file.old_path.as_deref())
        .unwrap_or_default();
    let Some(name) = language(path) else {
        return FileColors::new();
    };
    let (old, new) =
        file.hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .fold((0, 0), |(old, new), line| match line.kind {
                DiffLineKind::Added => (old, new + 1),
                DiffLineKind::Deleted => (old + 1, new),
                DiffLineKind::Context => (old + 1, new + 1),
            });
    if old.max(new) > MAX_LINES {
        return FileColors::new();
    }
    let (syntaxes, theme) = assets();
    let Some(syntax) = syntaxes.find_syntax_by_name(name) else {
        return FileColors::new();
    };
    file.hunks
        .iter()
        .map(|hunk| {
            let mut lines = vec![LineColors::default(); hunk.lines.len()];
            for old in [true, false] {
                let mut side = Side::new(syntax, syntaxes, theme);
                side.rust = name == "Rust";
                for (at, line) in hunk.lines.iter().enumerate() {
                    let colors = &mut lines[at];
                    match (line.kind, old) {
                        (DiffLineKind::Added, true) | (DiffLineKind::Deleted, false) => {}
                        (_, true) => colors.old = side.line(&line.text),
                        (_, false) => colors.new = side.line(&line.text),
                    }
                }
            }
            lines
        })
        .collect()
}

/// One side of a hunk, highlighted a line at a time from its first.
struct Side<'a> {
    lines: HighlightLines<'a>,
    syntaxes: &'a SyntaxSet,
    text: String,
    /// Unscoped names take `rust_names`' colours.
    rust: bool,
}

impl<'a> Side<'a> {
    fn new(syntax: &SyntaxReference, syntaxes: &'a SyntaxSet, theme: &'a Theme) -> Self {
        Self {
            lines: HighlightLines::new(syntax, theme),
            syntaxes,
            text: String::new(),
            rust: false,
        }
    }

    /// The next line's colours. A line of `MAX_LINE` or more is left plain, and, as in Shiki, the lines after it carry
    /// on from the state before it.
    fn line(&mut self, text: &str) -> Vec<Span> {
        if text.encode_utf16().count() >= MAX_LINE {
            return Vec::new();
        }
        // The grammars are the kind that match the end of a line, as Shiki's do.
        self.text.clear();
        self.text.push_str(text);
        self.text.push('\n');
        let Ok(styled) = self.lines.highlight_line(&self.text, self.syntaxes) else {
            return Vec::new();
        };
        let mut spans = Vec::new();
        let mut end = 0;
        for (style, piece) in styled {
            let start = end;
            // The added newline isn't the line's.
            end = (end + piece.len()).min(text.len());
            let Color { r, g, b, a } = style.foreground;
            if a == 0 && self.rust {
                rust_names(text, start..end, &mut spans);
            } else {
                let color = if a == 0 {
                    FOREGROUND
                } else {
                    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
                };
                let italic = style.font_style.contains(FontStyle::ITALIC);
                push(&mut spans, end, color, italic);
            }
        }
        spans
    }
}

/// Ends the line's last stretch at `end`, or starts one there in a new colour.
fn push(spans: &mut Vec<Span>, end: usize, color: u32, italic: bool) {
    if end == spans.last().map_or(0, |last| last.end) {
        return;
    }
    match spans.last_mut() {
        Some(last) if (last.color, last.italic) == (color, italic) => last.end = end,
        _ => spans.push(Span { end, color, italic }),
    }
}

/// The colours of `text[range]`, which bat's Rust grammar leaves unscoped. VS Code's, which Shiki runs, tells names
/// apart by how they're written and what's around them: `name(` and `name!` are calls, `name::` a module, a
/// capitalised name a type, `.name` and `::name` plain, and any other a variable. What isn't a name is punctuation.
fn rust_names(text: &str, range: Range<usize>, spans: &mut Vec<Span>) {
    let mut at = range.start;
    while at < range.end {
        let rest = &text[at..range.end];
        let word = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        if word == 0 {
            let c = rest.chars().next().unwrap_or(' ');
            at += c.len_utf8();
            push(
                spans,
                at,
                if c.is_whitespace() {
                    FOREGROUND
                } else {
                    0x636363
                },
                false,
            );
            continue;
        }
        let name = &rest[..word];
        let before = text[..at].trim_end();
        let after = text[at + word..].trim_start();
        let color = if name.starts_with(|c: char| c.is_ascii_digit()) {
            FOREGROUND
        } else if after.starts_with('(') || (after.starts_with('!') && !after.starts_with("!=")) {
            0x9d6afb
        } else if after.starts_with("::") && name.starts_with(char::is_lowercase) {
            0xffab16
        } else if name.starts_with(char::is_uppercase) {
            0xd568ea
        } else if before.ends_with('.') || before.ends_with("::") {
            FOREGROUND
        } else {
            0xffa359
        };
        at += word;
        push(spans, at, color, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::patch::parse_patch;

    /// The colours of an added file's lines, with each stretch's text.
    fn added(path: &str, text: &str) -> Vec<Vec<(String, u32)>> {
        let lines: Vec<&str> = text.lines().collect();
        let patch = format!(
            "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n{}",
            lines.len(),
            lines
                .iter()
                .map(|line| format!("+{line}\n"))
                .collect::<String>()
        );
        let diff = parse_patch(&patch).unwrap();
        let colors = file_colors(&diff.files[0]);
        lines
            .iter()
            .enumerate()
            .map(|(at, line)| {
                let mut start = 0;
                colors
                    .first()
                    .map_or(&[][..], |hunk| &hunk[at].new)
                    .iter()
                    .map(|span| {
                        let piece = line[start..span.end].to_owned();
                        start = span.end;
                        (piece, span.color)
                    })
                    .filter(|(piece, _)| !piece.trim().is_empty())
                    .collect()
            })
            .collect()
    }

    /// What colour `piece` has on line `line`, where it's a stretch of its own or part of one.
    fn color_of(colors: &[Vec<(String, u32)>], line: usize, piece: &str) -> u32 {
        colors[line]
            .iter()
            .find(|(text, _)| text.contains(piece))
            .unwrap_or_else(|| panic!("no {piece:?} in {:?}", colors[line]))
            .1
    }

    #[test]
    fn the_language_comes_from_the_file_name_as_on_the_desktop() {
        for (path, grammar) in [
            ("src/components/App.tsx", Some("TypeScriptReact")),
            ("src/lib/terminal.ts", Some("TypeScript")),
            ("scripts/run.mjs", Some("TypeScript")),
            ("src/main.jsx", Some("TypeScriptReact")),
            ("apps/native/src/diff/rows.rs", Some("Rust")),
            ("src/index.css", Some("CSS")),
            ("package.json", Some("JSON")),
            ("README.md", Some("Markdown")),
            (".github/workflows/ci.yml", Some("YAML")),
            ("scripts/build.sh", Some("Bourne Again Shell (bash)")),
            ("src/Services/RetryPolicy.cs", Some("C#")),
            ("apps/native/Cargo.toml", Some("TOML")),
            // The whole name, in a folder or not, and the last two extensions.
            ("Dockerfile", Some("Dockerfile")),
            ("docker/Dockerfile", Some("Dockerfile")),
            ("src/app/app.component.ts", Some("TypeScript")),
            // What the desktop shows as plain text.
            ("LICENSE", None),
            ("notes.txt", None),
            ("a.b/file", None),
            ("Program.csproj", None),
        ] {
            assert_eq!(language(path), grammar, "{path}");
        }
        let (syntaxes, _) = assets();
        for (name, _) in GRAMMARS {
            assert!(syntaxes.find_syntax_by_name(name).is_some(), "no {name}");
        }
    }

    /// The colours the desktop's Shiki gives these lines with pierre-dark.
    #[test]
    fn tokens_take_the_desktops_colours() {
        let ts = added(
            "src/load.ts",
            "export async function load(path: string): Promise<Config> {\n  const timeout = 30; // seconds\n  return { path, timeout, name: \"client\" };\n}",
        );
        assert_eq!(color_of(&ts, 0, "export"), 0xff678d);
        assert_eq!(color_of(&ts, 0, "load"), 0x9d6afb);
        assert_eq!(color_of(&ts, 0, "Promise"), 0xd568ea);
        assert_eq!(color_of(&ts, 1, "30"), 0x68cdf2);
        assert_eq!(color_of(&ts, 1, "// seconds"), 0x737373);
        assert_eq!(color_of(&ts, 2, "return"), 0xff678d);
        assert_eq!(color_of(&ts, 2, "\"client\""), 0x5ecc71);
        let rust = added(
            "src/retry.rs",
            "pub fn backoff(attempt: u32) -> u64 {\n    // Doubles each time.\n    let delay = std::time::Duration::from_millis(base.max);\n    200 * 2u64.pow(attempt)\n}",
        );
        assert_eq!(color_of(&rust, 0, "pub"), 0xff678d);
        assert_eq!(color_of(&rust, 0, "backoff"), 0x9d6afb);
        assert_eq!(color_of(&rust, 0, "attempt"), 0xffa359);
        assert_eq!(color_of(&rust, 0, "u32"), 0xd568ea);
        assert_eq!(color_of(&rust, 1, "// Doubles"), 0x737373);
        // Names bat's grammar leaves unscoped, as VS Code's tells them apart.
        assert_eq!(color_of(&rust, 2, "delay"), 0xffa359);
        assert_eq!(color_of(&rust, 2, "std"), 0xffab16);
        assert_eq!(color_of(&rust, 2, "Duration"), 0xd568ea);
        assert_eq!(color_of(&rust, 2, "from_millis"), 0x9d6afb);
        assert_eq!(color_of(&rust, 2, "base"), 0xffa359);
        assert_eq!(color_of(&rust, 2, "max"), 0xfafafa);
        assert_eq!(color_of(&rust, 3, "200"), 0x68cdf2);
        assert_eq!(color_of(&rust, 3, "pow"), 0x9d6afb);
        let json = added(
            "package.json",
            "{\n  \"name\": \"wings\",\n  \"private\": true\n}",
        );
        assert_eq!(color_of(&json, 1, "\"wings\""), 0x5ecc71);
        assert_eq!(color_of(&json, 2, "true"), 0x68cdf2);
        // No grammar: no colours, so the line takes the theme's text colour.
        assert!(added("notes.txt", "plain words")[0].is_empty());
    }

    #[test]
    fn each_side_of_a_hunk_is_highlighted_on_its_own() {
        // The old side opens a comment the unchanged line is in; on the new side that line is code.
        let patch = "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1,2 +1,2 @@\n-/* the old note\n+let a = 1;\n value = 2; */\n";
        let diff = parse_patch(patch).unwrap();
        let colors = file_colors(&diff.files[0]);
        let lines = &colors[0];
        assert!(lines[0].new.is_empty() && lines[1].old.is_empty());
        assert_eq!(lines[2].old[0].color, 0x737373);
        assert_ne!(lines[2].new[0].color, 0x737373);
    }

    #[test]
    fn very_long_lines_and_very_big_files_are_left_plain_as_on_the_desktop() {
        let long = format!("let a = \"{}\";", "x".repeat(MAX_LINE));
        let colors = added("a.ts", &format!("{long}\nlet b = 2;"));
        assert!(colors[0].is_empty());
        // The line after it is highlighted from where the grammar was before it.
        assert_eq!(color_of(&colors, 1, "2"), 0x68cdf2);
        let big = format!(
            "diff --git a/a.rs b/a.rs\nnew file mode 100644\n--- /dev/null\n+++ b/a.rs\n@@ -0,0 +1,{} @@\n{}",
            MAX_LINES + 1,
            "+x\n".repeat(MAX_LINES + 1)
        );
        assert!(file_colors(&parse_patch(&big).unwrap().files[0]).is_empty());
    }
}
