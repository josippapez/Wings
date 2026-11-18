//! Claude's replies cut at their code blocks, so each block can have a Copy button of its own beside the reply's. The cut
//! follows the Markdown renderer's own rule for a fence (`wings_plugin::markdown`): a line that starts with three backticks
//! or tildes opens one, and a line that is those three again closes it. Only a fence at the start of a line is cut out: one
//! indented under a list item stays in its list, which a cut would split in two.

/// The most text one `copyText` takes: Wings refuses more than 1 MB.
pub const COPY_MAX: usize = 1_000_000;

#[derive(Debug, PartialEq)]
pub enum Part {
    /// Markdown between the code blocks.
    Text(String),
    /// A code block: the language its fence names, and what is in it.
    Code { lang: String, text: String },
}

/// `source` as its text and its code blocks, in order. Text that is only blank lines is left out.
pub fn parts(source: &str) -> Vec<Part> {
    let mut out = Vec::new();
    let mut text: Vec<&str> = Vec::new();
    let mut lines = source.lines();
    while let Some(line) = lines.next() {
        let Some(mark) = ["```", "~~~"].into_iter().find(|mark| line.starts_with(mark)) else {
            text.push(line);
            continue;
        };
        flush(&mut text, &mut out);
        let lang: String = line[3..].trim_start().chars().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '+' | '-')).collect();
        let mut code: Vec<&str> = Vec::new();
        for inner in lines.by_ref() {
            if inner.trim() == mark {
                break;
            }
            code.push(inner);
        }
        out.push(Part::Code { lang, text: code.join("\n") });
    }
    flush(&mut text, &mut out);
    out
}

fn flush(text: &mut Vec<&str>, out: &mut Vec<Part>) {
    if text.iter().any(|line| !line.trim().is_empty()) {
        out.push(Part::Text(text.join("\n")));
    }
    text.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_is_cut_at_its_code_blocks_with_their_language() {
        let reply = "Run this:\n\n```sh\ncargo test\n```\n\nThen this one:\n~~~\nplain\n~~~\nDone.";
        assert_eq!(
            parts(reply),
            [
                Part::Text("Run this:\n".into()),
                Part::Code { lang: "sh".into(), text: "cargo test".into() },
                Part::Text("\nThen this one:".into()),
                Part::Code { lang: String::new(), text: "plain".into() },
                Part::Text("Done.".into()),
            ]
        );
    }

    #[test]
    fn a_reply_without_code_is_one_piece_and_an_indented_fence_stays_in_its_list() {
        assert_eq!(parts("Just words.\n\nMore."), [Part::Text("Just words.\n\nMore.".into())]);
        let listed = "1. Install:\n   ```sh\n   npm i\n   ```\n2. Run it";
        assert_eq!(parts(listed), [Part::Text(listed.into())]);
    }

    #[test]
    fn a_fence_that_never_closes_holds_the_rest_and_blank_text_is_left_out() {
        assert_eq!(parts("```rust\nfn main() {}\n\nlet x = 1;"), [Part::Code { lang: "rust".into(), text: "fn main() {}\n\nlet x = 1;".into() }]);
        assert_eq!(parts("\n\n```\na\n```\n\n"), [Part::Code { lang: String::new(), text: "a".into() }]);
        assert_eq!(parts(""), []);
    }
}
