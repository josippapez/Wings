//! The downloads of one Browser page, newest first, as its tabs' views tell them (`onDownload`), with the tab and group
//! each came from: the person sees all of them, an agent only its own group's. Plain data, nothing drawn.

use wings_plugin::ui::WebDownload;

/// The most downloads a page keeps; the oldest that ended go first.
const KEPT: usize = 50;

pub struct Download {
    pub event: WebDownload,
    /// The tab whose view started it, and that tab's group then: none for the person's own.
    pub tab: String,
    pub group: Option<String>,
}

#[derive(Default)]
pub struct Downloads {
    pub list: Vec<Download>,
    /// The list shows under the page.
    pub open: bool,
}

impl Downloads {
    /// What a view of `tab` (in `group`) says of a download: a new one goes first, one already listed changes where it is.
    /// True for a new one.
    pub fn heard(&mut self, tab: &str, group: Option<&str>, event: WebDownload) -> bool {
        if let Some(known) = self.list.iter_mut().find(|known| known.event.id == event.id) {
            known.event = event;
            return false;
        }
        self.list.insert(0, Download { event, tab: tab.to_owned(), group: group.map(str::to_owned) });
        while self.list.len() > KEPT {
            match self.list.iter().rposition(|download| download.event.state != "progress") {
                Some(at) => drop(self.list.remove(at)),
                None => break,
            }
        }
        true
    }

    /// The downloads of a group, or for an empty key the person's own.
    pub fn of(&self, key: &str) -> Vec<&Download> {
        self.list.iter().filter(|download| download.group.as_deref().unwrap_or_default() == key).collect()
    }

    /// How many are under way.
    pub fn running(&self) -> usize {
        self.list.iter().filter(|download| download.event.state == "progress").count()
    }

    /// Takes the ones that ended off the list, which closes when nothing is left.
    pub fn clear(&mut self) {
        self.list.retain(|download| download.event.state == "progress");
        if self.list.is_empty() {
            self.open = false;
        }
    }
}

/// `bytes` as a person reads a size: "840 B", "12 KB", "3.4 MB", "1.2 GB".
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} B"),
        _ if value < 10.0 => format!("{value:.1} {}", UNITS[unit]),
        _ => format!("{value:.0} {}", UNITS[unit]),
    }
}

/// What a download's line under its name says: how far it is, what it came to, or why it stopped.
pub fn status(event: &WebDownload) -> String {
    match event.state.as_str() {
        "progress" => match event.total {
            Some(total) if total > 0 => format!("{}% · {} of {}", event.received * 100 / total, size(event.received), size(total)),
            _ => format!("{} so far", size(event.received)),
        },
        "done" => format!("Done · {}", size(event.total.unwrap_or(event.received))),
        "canceled" => "Canceled".into(),
        _ => event.error.clone().unwrap_or_else(|| "It didn't download".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: u64, state: &str) -> WebDownload {
        WebDownload { id, state: state.into(), name: format!("file{id}.pdf"), ..WebDownload::default() }
    }

    #[test]
    fn a_download_is_listed_once_newest_first_and_changes_in_place() {
        let mut downloads = Downloads::default();
        assert!(downloads.heard("t1", None, event(1, "progress")));
        assert!(downloads.heard("t2", Some("ses-a"), event(2, "progress")));
        assert!(!downloads.heard("t1", None, event(1, "done")), "the same download again");
        let ids: Vec<(u64, &str)> = downloads.list.iter().map(|download| (download.event.id, download.event.state.as_str())).collect();
        assert_eq!(ids, [(2, "progress"), (1, "done")]);
        assert_eq!(downloads.running(), 1);
        // An agent hears only its own group's; the person's own are the ones with none.
        assert_eq!(downloads.of("ses-a").iter().map(|download| download.event.id).collect::<Vec<_>>(), [2]);
        assert_eq!(downloads.of("").iter().map(|download| download.event.id).collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn clearing_keeps_the_ones_under_way_and_the_list_is_bounded() {
        let mut downloads = Downloads { open: true, ..Downloads::default() };
        downloads.heard("t1", None, event(1, "done"));
        downloads.heard("t1", None, event(2, "progress"));
        downloads.clear();
        assert_eq!((downloads.list.len(), downloads.open), (1, true));
        downloads.heard("t1", None, event(2, "failed"));
        downloads.clear();
        assert_eq!((downloads.list.len(), downloads.open), (0, false), "nothing left closes it");
        for id in 0..80 {
            downloads.heard("t1", None, event(id, "done"));
        }
        assert_eq!(downloads.list.len(), KEPT);
        assert_eq!(downloads.list[0].event.id, 79, "the newest stay");
    }

    #[test]
    fn a_downloads_line_says_how_far_it_is_and_what_it_came_to() {
        let at = |state: &str, received: u64, total: Option<u64>| status(&WebDownload { state: state.into(), received, total, ..WebDownload::default() });
        assert_eq!(at("progress", 1_200_000, Some(2_600_000)), "46% · 1.2 MB of 2.6 MB");
        assert_eq!(at("progress", 840, None), "840 B so far");
        assert_eq!(at("done", 0, Some(48_000_000)), "Done · 48 MB");
        assert_eq!(at("canceled", 0, None), "Canceled");
        let blocked = WebDownload { state: "blocked".into(), error: Some("The page tried to download more files by itself.".into()), ..WebDownload::default() };
        assert_eq!(status(&blocked), "The page tried to download more files by itself.");
    }
}
