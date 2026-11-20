//! The `Code` a page sends: lines of code in one box. The page names the language and the app colours the lines, so a
//! page ships no highlighter and a block of thousands of lines is one node.

use serde_json::{Map, Value};

use super::{Tone, str_prop};

/// The most characters of code one node keeps. The tree as a whole is held to `MAX_JSON_BYTES`.
pub const MAX_CHARS: usize = 400_000;
/// The longest language name kept, in characters.
const LANGUAGE_CHARS: usize = 40;

#[derive(Clone, Debug, PartialEq)]
pub struct Code {
    pub id: u64,
    pub text: String,
    /// What the fence of a Markdown code block names (`rust`, `ts`), or a file's extension. None when the page gave
    /// none, or it isn't a plain name.
    pub language: Option<String>,
    /// `tone: "success"` or `"danger"`: the code is a run of added or removed lines of a diff, drawn as a tinted band in
    /// place of a card. Another tone is drawn in its colour the same way.
    pub tone: Option<Tone>,
    /// `variant: "band"`: a diff's unchanged lines, drawn flush like a tinted band but untinted, for the same card.
    pub band: bool,
}

pub(super) fn parse(id: u64, props: &Map<String, Value>, tone: Option<Tone>) -> Code {
    let text = str_prop(props, "code").unwrap_or_default();
    let text = match text.char_indices().nth(MAX_CHARS) {
        Some((end, _)) => &text[..end],
        None => text,
    };
    let language = str_prop(props, "language")
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.chars().count() <= LANGUAGE_CHARS)
        .filter(|name| {
            name.chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '+' | '#' | '-' | '.'))
        })
        .map(str::to_owned);
    Code {
        id,
        text: text.to_owned(),
        language,
        tone,
        band: str_prop(props, "variant") == Some("band"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parsed(props: Value) -> Code {
        parse(1, props.as_object().unwrap(), None)
    }

    #[test]
    fn a_code_node_keeps_its_text_and_a_plain_language_name() {
        let code = parsed(json!({ "code": "let a = 1;\nlet b = 2;", "language": " rust " }));
        assert_eq!(code.text, "let a = 1;\nlet b = 2;");
        assert_eq!(code.language.as_deref(), Some("rust"));
        assert_eq!(
            parsed(json!({ "code": "x", "language": "c++" }))
                .language
                .as_deref(),
            Some("c++")
        );
    }

    #[test]
    fn no_language_or_one_that_is_not_a_name_leaves_the_code_plain() {
        assert_eq!(parsed(json!({ "code": "x" })).language, None);
        assert_eq!(
            parsed(json!({ "code": "x", "language": "" })).language,
            None
        );
        assert_eq!(
            parsed(json!({ "code": "x", "language": "a b" })).language,
            None
        );
        assert_eq!(
            parsed(json!({ "code": "x", "language": "x".repeat(41) })).language,
            None
        );
        assert_eq!(parsed(json!({ "code": "x", "language": 3 })).language, None);
    }

    #[test]
    fn a_tone_makes_it_a_band() {
        assert_eq!(
            parse(
                1,
                json!({ "code": "x" }).as_object().unwrap(),
                Some(Tone::Danger)
            )
            .tone,
            Some(Tone::Danger)
        );
        assert_eq!(parsed(json!({ "code": "x" })).tone, None);
    }

    #[test]
    fn code_past_the_limit_is_cut_at_a_character() {
        let long = "é".repeat(MAX_CHARS + 5);
        assert_eq!(
            parsed(json!({ "code": long })).text.chars().count(),
            MAX_CHARS
        );
        assert_eq!(parsed(json!({})).text, "");
    }
}
