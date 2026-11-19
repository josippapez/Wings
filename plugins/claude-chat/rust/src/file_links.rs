//! File references in Claude's replies (`src/cart.ts`, `src/cart.ts:42`, `src/cart.ts#L42`, in backticks or plain) as links.
//! A reference is a link only when it resolves, inside the chat's folder, to a file the session changed: Wings has no call
//! that opens a file in an editor, and the diff viewer is the closest place to show one, at the line.

/// A file a reply names, by its full path, and the line when it says one.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub path: String,
    pub line: Option<u32>,
}

/// A stretch of a reply's text: words, or a reference with the words as written.
#[derive(Debug, PartialEq)]
pub enum Piece {
    Text(String),
    File(String, Reference),
}

/// `text` as a reference to one of `known` (full paths): relative to `cwd` or full, with `:line`, `:line:col` or `#L<line>`
/// after it. None for anything else, and for a path that leaves `cwd`.
pub fn resolve(cwd: &str, text: &str, known: &[String]) -> Option<Reference> {
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let (path, line) = split_line(text);
    let joined = if path.starts_with('/') { path.to_owned() } else { format!("{}/{path}", cwd.trim_end_matches('/')) };
    let full = normal(&joined)?;
    let inside = full.strip_prefix(cwd.trim_end_matches('/')).is_some_and(|rest| rest.starts_with('/'));
    (inside && known.contains(&full)).then_some(Reference { path: full, line })
}

/// The path and the line of `src/cart.ts:42:7` or `src/cart.ts#L42`.
fn split_line(text: &str) -> (&str, Option<u32>) {
    if let Some((path, line)) = text.rsplit_once("#L")
        && let Ok(line) = line.parse()
    {
        return (path, Some(line));
    }
    let number = |part: &str| part.parse::<u32>().ok();
    if let Some((rest, last)) = text.rsplit_once(':')
        && let Some(last) = number(last)
    {
        return match rest.rsplit_once(':') {
            // `path:line:col`
            Some((path, line)) if number(line).is_some() => (path, number(line)),
            _ => (rest, Some(last)),
        };
    }
    (text, None)
}

/// The path without `.` and `..` parts. None when `..` would go above the root.
fn normal(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/').filter(|part| !part.is_empty() && *part != ".") {
        if part == ".." {
            parts.pop()?;
        } else {
            parts.push(part);
        }
    }
    Some(format!("/{}", parts.join("/")))
}

/// `text` cut into its words and the references among them, each word's wrapping punctuation (backticks, quotes, brackets,
/// a closing comma or full stop) left in the text around it.
pub fn pieces(cwd: &str, text: &str, known: &[String]) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut plain = String::new();
    let mut flush = |out: &mut Vec<Piece>, plain: &mut String| {
        if !plain.is_empty() {
            out.push(Piece::Text(std::mem::take(plain)));
        }
    };
    let mut rest = text;
    while !rest.is_empty() {
        let space = rest.starts_with(char::is_whitespace);
        let end = rest.find(|c: char| c.is_whitespace() != space).unwrap_or(rest.len());
        let (word, after) = rest.split_at(end);
        rest = after;
        if space {
            plain.push_str(word);
            continue;
        }
        let head = word.len() - word.trim_start_matches(['(', '[', '{', '"', '\'', '`']).len();
        let core = word[head..].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\'', '`']);
        // A file name has an extension or a folder, which keeps most words from being looked up.
        match Some(core).filter(|core| core.contains(['/', '.'])).and_then(|core| resolve(cwd, core, known)) {
            Some(reference) => {
                plain.push_str(&word[..head]);
                flush(&mut out, &mut plain);
                out.push(Piece::File(core.to_owned(), reference));
                plain.push_str(&word[head + core.len()..]);
            }
            None => plain.push_str(word),
        }
    }
    flush(&mut out, &mut plain);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CWD: &str = "/work/shop";

    fn known() -> Vec<String> {
        vec!["/work/shop/src/cart.ts".into(), "/work/shop/api/limits.ts".into()]
    }

    #[test]
    fn a_path_in_the_folder_with_or_without_a_line_is_a_reference() {
        let at = |line| Some(Reference { path: "/work/shop/src/cart.ts".into(), line });
        assert_eq!(resolve(CWD, "src/cart.ts", &known()), at(None));
        assert_eq!(resolve(CWD, "src/cart.ts:42", &known()), at(Some(42)));
        assert_eq!(resolve(CWD, "src/cart.ts:42:7", &known()), at(Some(42)));
        assert_eq!(resolve(CWD, "src/cart.ts#L42", &known()), at(Some(42)));
        assert_eq!(resolve(CWD, "./src/../src/cart.ts", &known()), at(None));
        assert_eq!(resolve(CWD, "/work/shop/src/cart.ts:3", &known()), at(Some(3)));
    }

    #[test]
    fn a_path_outside_the_folder_or_not_changed_by_the_session_is_not() {
        assert_eq!(resolve(CWD, "../other/src/cart.ts", &known()), None);
        assert_eq!(resolve(CWD, "/etc/passwd", &known()), None);
        assert_eq!(resolve(CWD, "/work/shop2/src/cart.ts", &["/work/shop2/src/cart.ts".into()]), None);
        assert_eq!(resolve(CWD, "src/other.ts", &known()), None);
        assert_eq!(resolve(CWD, "../../../../etc/passwd", &known()), None);
        assert_eq!(resolve(CWD, "src/cart.ts and more", &known()), None);
    }

    #[test]
    fn words_around_a_reference_stay_text() {
        assert_eq!(
            pieces(CWD, "See `src/cart.ts:42`, and (api/limits.ts). Not other.ts.", &known()),
            [
                Piece::Text("See `".into()),
                Piece::File("src/cart.ts:42".into(), Reference { path: "/work/shop/src/cart.ts".into(), line: Some(42) }),
                Piece::Text("`, and (".into()),
                Piece::File("api/limits.ts".into(), Reference { path: "/work/shop/api/limits.ts".into(), line: None }),
                Piece::Text("). Not other.ts.".into()),
            ]
        );
        assert_eq!(pieces(CWD, "nothing here", &known()), [Piece::Text("nothing here".into())]);
    }
}
