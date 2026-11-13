//! Patches in pieces, so one huge file can't take the whole diff with it.

use std::future::Future;
use wings_plugin::task::map_limit;

/// The message Wings rejects `wings.exec` with when a program prints more than 8 MB.
pub const OUTPUT_TOO_LARGE: &str = "Output is larger than 8 MB";

/// Files per git call.
const BATCH_FILES: usize = 40;
/// A file whose own patch is bigger than this shows as "Too large to show" and the rest of the diff stays.
const MAX_FILE_BYTES: usize = 1024 * 1024;

/// The patch of `items` from `run`, one call per 40 items. A call that comes back too large is split in half and
/// tried again, and one item that is too large on its own becomes `too_large(item)`. Calls run 4 at a time, and the
/// patches come back in the order of `items`.
pub async fn batched<T, Fut>(items: Vec<T>, run: impl Fn(Vec<T>) -> Fut, too_large: impl Fn(&T) -> String) -> Result<String, String>
where
    T: Clone,
    Fut: Future<Output = Result<String, String>>,
{
    let chunks: Vec<Vec<T>> = items.chunks(BATCH_FILES).map(<[T]>::to_vec).collect();
    let fetched = map_limit(chunks, 4, async |chunk| {
        let mut out = String::new();
        // The parts still to fetch, the next one last.
        let mut parts = vec![chunk];
        while let Some(part) = parts.pop() {
            match run(part.clone()).await {
                Ok(patch) => out.push_str(&patch),
                Err(error) if !error.contains(OUTPUT_TOO_LARGE) => return Err(error),
                Err(_) if part.len() == 1 => out.push_str(&too_large(&part[0])),
                Err(_) => {
                    let half = part.len().div_ceil(2);
                    parts.push(part[half..].to_vec());
                    parts.push(part[..half].to_vec());
                }
            }
        }
        Ok(out)
    })
    .await;
    fetched.into_iter().collect()
}

/// A file entry in git's own format that says why there is no diff.
pub fn placeholder(head: &str, text: &str) -> String {
    format!("{head}@@ -1 +1 @@\n {text}\n")
}

/// Where each line of `text` that starts with `prefix` begins.
fn starts<'a>(text: &'a str, prefix: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(prefix).map(|(at, _)| at).filter(move |at| *at == 0 || text.as_bytes()[at - 1] == b'\n')
}

/// Keeps each file's patch under 1 MB and the whole under `total` bytes, by turning the biggest files into
/// "Too large to show (N MB)" entries. Binary files and files with no hunks are already small.
pub fn fit_patch(patch: &str, total: usize) -> String {
    let mut cuts: Vec<usize> = starts(patch, "diff --git ").filter(|at| *at > 0).collect();
    cuts.push(patch.len());
    let mut chunks: Vec<String> = Vec::new();
    let mut from = 0;
    for cut in cuts {
        chunks.push(patch[from..cut].to_owned());
        from = cut;
    }
    let mut sizes: Vec<usize> = chunks.iter().map(String::len).collect();
    let mut shrunk = vec![false; chunks.len()];
    let mut shrink = |i: usize, sizes: &mut Vec<usize>| {
        // The part of a file's patch before its first hunk: `diff --git`, `index`, `---` and `+++` lines.
        let head = starts(&chunks[i], "@@ ").next().map_or(chunks[i].as_str(), |at| &chunks[i][..at]);
        chunks[i] = placeholder(head, &format!("Too large to show ({:.1} MB)", sizes[i] as f64 / 1024.0 / 1024.0));
        sizes[i] = chunks[i].len();
    };
    for i in 0..sizes.len() {
        if sizes[i] > MAX_FILE_BYTES {
            shrink(i, &mut sizes);
            shrunk[i] = true;
        }
    }
    let mut sum: usize = sizes.iter().sum();
    let mut biggest: Vec<usize> = (0..sizes.len()).collect();
    biggest.sort_by_key(|i| std::cmp::Reverse(sizes[*i]));
    for i in biggest {
        if sum <= total {
            break;
        }
        let before = sizes[i];
        if !shrunk[i] {
            shrink(i, &mut sizes);
        }
        sum -= before - sizes[i];
    }
    chunks.concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use wings_plugin::task::block_on;

    #[test]
    fn a_call_that_is_too_large_is_halved_until_the_one_file_that_is_too_big_stands_alone() {
        let calls: Rc<RefCell<Vec<usize>>> = Rc::default();
        let seen = calls.clone();
        let items: Vec<u32> = (0..6).collect();
        let patch = block_on(batched(
            items,
            move |part: Vec<u32>| {
                seen.borrow_mut().push(part.len());
                async move { if part.contains(&4) && part.len() > 1 || part == [4] { Err(format!("git: {OUTPUT_TOO_LARGE}")) } else { Ok(part.iter().map(|n| format!("[{n}]")).collect()) } }
            },
            |n| format!("[{n} too large]"),
        ));
        assert_eq!(patch.unwrap(), "[0][1][2][3][4 too large][5]");
        // All six, the two halves, the half with the big file again as two and one, then its two files alone.
        assert_eq!(*calls.borrow(), [6, 3, 3, 2, 1, 1, 1]);
        let failed = block_on(batched(vec![1], |_| async { Err("fatal: bad revision".to_owned()) }, |_| String::new()));
        assert_eq!(failed.unwrap_err(), "fatal: bad revision");
    }

    #[test]
    fn a_patch_over_the_limit_loses_its_biggest_files_first_and_keeps_their_headers() {
        let file = |name: &str, lines: usize| format!("diff --git a/{name} b/{name}\n--- a/{name}\n+++ b/{name}\n@@ -1 +1,{lines} @@\n{}", "+line\n".repeat(lines));
        let patch = format!("{}{}{}", file("small", 2), file("big", 400), file("mid", 50));
        let fit = fit_patch(&patch, 700);
        assert!(fit.contains("diff --git a/big b/big\n--- a/big\n+++ b/big\n@@ -1 +1 @@\n Too large to show (0.0 MB)\n"));
        assert!(fit.contains(&file("small", 2)) && fit.contains(&file("mid", 50)));
        assert_eq!(fit_patch(&patch, usize::MAX), patch);
        assert_eq!(fit_patch("", 10), "");
    }
}
