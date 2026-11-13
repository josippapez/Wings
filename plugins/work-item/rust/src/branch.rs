//! The work item id a branch name carries, like `feature/1234-login`.

/// The length of a date or a version at the start of `s`: `2026-10-13`, `2026.10`, `v1.2.3`. A date starts with a 19xx or
/// 20xx year, so `feature/1234-2-login` keeps its id.
fn date_or_version(s: &[u8]) -> Option<usize> {
    let digits = |from: usize, most: usize| s[from.min(s.len())..].iter().take(most).take_while(|b| b.is_ascii_digit()).count();
    let is_sep = |at: usize| s.get(at).is_some_and(|b| matches!(b, b'-' | b'.' | b'/'));
    if s.len() >= 4 && (s.starts_with(b"19") || s.starts_with(b"20")) && s[2].is_ascii_digit() && s[3].is_ascii_digit() && is_sep(4) {
        let month = digits(5, 2);
        if month > 0 {
            let mut end = 5 + month;
            if is_sep(end) && digits(end + 1, 2) > 0 {
                end += 1 + digits(end + 1, 2);
            }
            return Some(end);
        }
    }
    let from = usize::from(s.first().is_some_and(|b| matches!(b, b'v' | b'V')));
    let mut end = from + digits(from, usize::MAX);
    if end == from {
        return None;
    }
    let mut dots = 0;
    while s.get(end) == Some(&b'.') && digits(end + 1, usize::MAX) > 0 {
        end += 1 + digits(end + 1, usize::MAX);
        dots += 1;
    }
    (dots > 0).then_some(end)
}

/// `branch` with each date and version made a space: their numbers aren't work item ids.
fn without_dates(branch: &str) -> String {
    let bytes = branch.as_bytes();
    let mut out = String::new();
    let mut at = 0;
    while at < bytes.len() {
        match date_or_version(&bytes[at..]) {
            Some(len) => {
                out.push(' ');
                at += len;
            }
            None => {
                let next = branch[at..].chars().next().map_or(1, char::len_utf8);
                out.push_str(&branch[at..at + next]);
                at += next;
            }
        }
    }
    out
}

/// The first run of 3+ digits that is a whole path segment or followed by `-`: `feature/1234-login` is 1234.
pub fn work_item_id(branch: &str) -> Option<String> {
    let s = without_dates(branch);
    let bytes = s.as_bytes();
    (0..=bytes.len()).find_map(|at| {
        let from = if at == 0 && bytes.first().is_some_and(u8::is_ascii_digit) { 0 } else if matches!(bytes.get(at), Some(b'/' | b'_' | b'-')) { at + 1 } else { return None };
        let len = bytes[from..].iter().take_while(|b| b.is_ascii_digit()).count();
        let end = from + len;
        (len >= 3 && matches!(bytes.get(end), None | Some(b'-' | b'/'))).then(|| s[from..end].to_owned())
    })
}

/// Whether a branch name carries the work item's id as a token of its own, at least 3 digits: `feature/1234-login` has
/// 1234, `hotfix/2026-10-13` and `v1.2.3` have none.
pub fn branch_has_id(branch: &str, id: u64) -> bool {
    if id < 100 {
        return false;
    }
    let (s, id) = (without_dates(branch), id.to_string());
    s.match_indices(&id).any(|(at, _)| {
        let before = s[..at].chars().next_back();
        let after = s[at + id.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphanumeric()) && !after.is_some_and(|c| c.is_ascii_alphanumeric())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_has_id_matches_the_id_as_a_whole_number() {
        assert!(branch_has_id("feature/1234-login", 1234));
        assert!(branch_has_id("origin/bugfix_1234", 1234));
        assert!(!branch_has_id("feature/12345-login", 1234));
        assert!(!branch_has_id("feature/123-login", 1234));
        assert!(!branch_has_id("feature/a1234", 1234));
        assert!(!branch_has_id("feature/12-x", 12));
        assert!(!branch_has_id("hotfix/2026-10-13", 2026));
        assert!(!branch_has_id("hotfix/2026-10-13", 10));
        assert!(!branch_has_id("release/2026.10", 2026));
        assert!(!branch_has_id("release/v1.200.3", 200));
        assert!(branch_has_id("hotfix/2026-10-13-4521-fix", 4521));
    }

    // The badge reads a branch with work_item_id and the section with branch_has_id, and both have to name the same items.
    #[test]
    fn work_item_id_and_branch_has_id_agree_on_which_branches_name_a_work_item() {
        let cases = [
            ("feature/1234-login", Some("1234")),
            ("bug/5678", Some("5678")),
            ("origin/bugfix_1234", Some("1234")),
            ("hotfix/2026-10-13-4521-fix", Some("4521")),
            ("feature/1234-2-factor-auth", Some("1234")),
            ("feature/12345-12-login", Some("12345")),
            ("hotfix/2026-10-13", None),
            ("release/2026.10", None),
            ("release/v1.200.3", None),
            ("v1.2.3", None),
            ("main", None),
        ];
        for (branch, id) in cases {
            assert_eq!(work_item_id(branch).as_deref(), id, "{branch}");
            let numbers: Vec<u64> = branch.split(|c: char| !c.is_ascii_digit()).filter(|n| !n.is_empty()).filter_map(|n| n.parse().ok()).collect();
            let named: Vec<u64> = numbers.into_iter().filter(|n| branch_has_id(branch, *n)).collect();
            assert_eq!(named, id.map(|id| vec![id.parse::<u64>().unwrap()]).unwrap_or_default(), "{branch}");
        }
    }
}
