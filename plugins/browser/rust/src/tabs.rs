//! The browser tabs of one Browser page and the groups the agents' tabs sit in. Plain data and rules, nothing drawn and no host
//! calls: `browser_page.rs` draws it and acts on it, `agent_page.rs` does what the agent tools ask of it.
//!
//! A tab is either the person's own (no group) or in the group of the agent that opened it. The group's key is the agent's
//! session id (or its pane), so each agent finds its tab group again and never touches another's or the person's.

use crate::{extension::Staged, history::Entry};
use serde_json::{Value, json};
use wings_plugin::ui::WebState;

/// The most commands a tab keeps sending: more than a step and a click between two draws.
pub const COMMANDS_KEPT: usize = 16;

pub struct Tab {
    pub id: String,
    /// The key of the group it is in: none for the person's own.
    pub group: Option<String>,
    /// Where the view starts, once.
    pub start: String,
    /// A tab that has no address yet: the start page is shown in place of the view until one is chosen.
    pub blank: bool,
    pub state: WebState,
    /// The commands sent lately, oldest first, which the view runs once each by `token`: two sent before the next tree
    /// (an agent's step and a click) both run.
    pub commands: Vec<Value>,
    /// How many times the view's state changed.
    pub updates: u64,
    /// What is in the address field while it is being edited, in place of the page's address.
    pub typed: Option<String>,
    /// History as of when typing began, for the suggestions.
    pub history: Vec<Entry>,
    /// The page is a Chrome Web Store or Firefox add-ons page, so its extension can be added.
    pub store: bool,
    pub staged: Option<Staged>,
    pub added: Option<String>,
    /// The page last put in history.
    pub recorded: (String, String),
    /// The token of the last click of an extension's button, whose refusal shows in the red line.
    pub opening: Option<u64>,
    /// The address "Clear data for" was chosen on, while its dialog asks.
    pub clearing_site: Option<String>,
    /// When an agent last used it or the person last chose it (the clock's milliseconds): none until the first idle check, which
    /// starts its count, so a tab restored at launch isn't closed at once.
    pub used: Option<f64>,
    /// The view Wings made for the window another tab's page opened, which this tab shows (`WebPopup::popup`), and that tab.
    pub popup: Option<u64>,
    pub opened_by: Option<String>,
    /// The address of the last window its page opened by itself, which wasn't opened.
    pub blocked: Option<String>,
}

impl Tab {
    fn new(id: String, group: Option<String>, url: Option<&str>) -> Self {
        Self {
            id,
            group,
            start: url.unwrap_or_default().to_owned(),
            blank: url.is_none(),
            state: WebState::default(),
            commands: Vec::new(),
            updates: 0,
            typed: None,
            history: Vec::new(),
            store: false,
            staged: None,
            added: None,
            recorded: Default::default(),
            opening: None,
            clearing_site: None,
            used: None,
            popup: None,
            opened_by: None,
            blocked: None,
        }
    }

    /// What names the tab: its title, else its address, else "New tab".
    pub fn label(&self) -> String {
        [self.state.title.as_str(), self.state.url.as_str(), if self.blank { "" } else { self.start.as_str() }]
            .into_iter()
            .find(|text| !text.is_empty())
            .unwrap_or("New tab")
            .to_owned()
    }

    /// The address the tab is at, or starts at.
    pub fn url(&self) -> &str {
        if self.state.url.is_empty() { &self.start } else { &self.state.url }
    }
}

pub struct Group {
    pub key: String,
    pub name: String,
    pub collapsed: bool,
    /// The tab the group's agent works in.
    pub active: Option<String>,
}

pub struct Tabs {
    pub list: Vec<Tab>,
    /// The tab the person sees.
    pub active: String,
    pub groups: Vec<Group>,
    /// The person's own tab they had last, which a call with no group works in.
    own_active: Option<String>,
    next: u64,
    /// One counter for all the tabs, so a result maps back to the tab it was asked of.
    token: u64,
    /// The address and group the page was last opened with, so a restart doesn't open them again.
    pub args_seen: String,
}

impl Default for Tabs {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl Tabs {
    /// The page's first tab: at `url`, or blank, in `group` (key and name) or the person's own.
    pub fn new(url: Option<&str>, group: Option<(&str, &str)>) -> Self {
        let mut tabs = Self { list: Vec::new(), active: String::new(), groups: Vec::new(), own_active: None, next: 1, token: 0, args_seen: String::new() };
        let id = tabs.open(group, url);
        tabs.active = id;
        tabs
    }

    pub fn get(&self, id: &str) -> Option<&Tab> {
        self.list.iter().find(|tab| tab.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Tab> {
        self.list.iter_mut().find(|tab| tab.id == id)
    }

    pub fn active_tab(&self) -> &Tab {
        self.get(&self.active).unwrap_or(&self.list[0])
    }

    pub fn active_mut(&mut self) -> &mut Tab {
        let id = self.active.clone();
        let at = self.list.iter().position(|tab| tab.id == id).unwrap_or(0);
        &mut self.list[at]
    }

    pub fn group(&self, key: &str) -> Option<&Group> {
        self.groups.iter().find(|group| group.key == key)
    }

    pub fn group_name(&self, tab: &Tab) -> Option<&str> {
        self.group(tab.group.as_deref()?).map(|group| group.name.as_str())
    }

    /// The tabs of a group, or for an empty key the person's own.
    pub fn of(&self, key: &str) -> Vec<&Tab> {
        self.list.iter().filter(|tab| tab.group.as_deref().unwrap_or_default() == key).collect()
    }

    /// The ids in the order the strip shows them: each group's tabs, then the person's own.
    pub fn order(&self) -> Vec<String> {
        let grouped = self.groups.iter().flat_map(|group| self.of(&group.key));
        grouped.chain(self.of("")).map(|tab| tab.id.clone()).collect()
    }

    /// The tab a call from `key` works in: its group's active tab, or for an empty key (the person, the CLI) the person's own tab. None
    /// for a key that has no group yet.
    pub fn target(&self, key: &str) -> Option<String> {
        if key.is_empty() {
            let own = self.own_active.as_ref().filter(|id| self.get(id).is_some_and(|tab| tab.group.is_none()));
            let found = own.cloned().or_else(|| self.of("").first().map(|tab| tab.id.clone()));
            return found.or_else(|| Some(self.active_tab().id.clone()));
        }
        let group = self.group(key)?;
        let own = group.active.as_ref().filter(|id| self.get(id).is_some_and(|tab| tab.group.as_deref() == Some(key)));
        own.cloned().or_else(|| self.of(key).last().map(|tab| tab.id.clone()))
    }

    pub fn ensure_group(&mut self, key: &str, name: &str) {
        match self.groups.iter_mut().find(|group| group.key == key) {
            Some(group) => {
                if !name.is_empty() {
                    group.name = name.to_owned();
                }
            }
            None => self.groups.push(Group { key: key.to_owned(), name: name.to_owned(), collapsed: false, active: None }),
        }
    }

    /// A new tab at `url` (blank for none) in `group`, which is made when it isn't there. It becomes the group's tab. The person
    /// sees it when it is their own, when the tab they see shows no web page, or when it is in the group they are looking at.
    pub fn open(&mut self, group: Option<(&str, &str)>, url: Option<&str>) -> String {
        let id = format!("t{}", self.next);
        self.next += 1;
        if let Some((key, name)) = group {
            self.ensure_group(key, name);
        }
        self.list.push(Tab::new(id.clone(), group.map(|(key, _)| key.to_owned()), url));
        match group {
            Some((key, _)) => {
                if let Some(group) = self.groups.iter_mut().find(|group| group.key == key) {
                    group.active = Some(id.clone());
                }
                self.follow(&id);
            }
            None => self.activate(&id),
        }
        id
    }

    /// A tab for the window the page of `opener` opened at `url`: in the opener's group, after the opener and the tabs it
    /// opened before, showing `popup`, the view Wings made for it, or else loading `url`. Unless it opens in the background
    /// it becomes its group's tab, and the person sees it where they see the opener. None when the opener is gone.
    pub fn open_from(&mut self, opener: &str, url: &str, popup: Option<u64>, background: bool) -> Option<String> {
        let at = self.list.iter().position(|tab| tab.id == opener)?;
        let after = self.list[at + 1..].iter().take_while(|tab| tab.opened_by.as_deref() == Some(opener)).count();
        let group = self.list[at].group.clone();
        let id = format!("t{}", self.next);
        self.next += 1;
        let mut tab = Tab::new(id.clone(), group.clone(), Some(url));
        (tab.popup, tab.opened_by) = (popup, Some(opener.to_owned()));
        self.list.insert(at + 1 + after, tab);
        if !background {
            let shown = self.active == opener;
            self.select(&id);
            if shown {
                self.activate(&id);
            }
        }
        Some(id)
    }

    /// Shows `id` to the person and keeps it as the tab of its group (or their own).
    pub fn activate(&mut self, id: &str) {
        let Some(group) = self.get(id).map(|tab| tab.group.clone()) else { return };
        self.active = id.to_owned();
        match group {
            Some(key) => {
                if let Some(group) = self.groups.iter_mut().find(|group| group.key == key) {
                    group.active = Some(id.to_owned());
                    group.collapsed = false;
                }
            }
            None => self.own_active = Some(id.to_owned()),
        }
    }

    /// An agent's tab came forward: the person sees it only when what they see is no web page, or is in its own group. Their own
    /// tabs never move for an agent.
    pub fn follow(&mut self, id: &str) {
        let Some(tab) = self.get(id) else { return };
        let (group, seen) = (tab.group.clone(), self.active_tab());
        if seen.id != id && (seen.blank || (group.is_some() && seen.group == group)) {
            self.activate(id);
        }
    }

    /// Makes `id` the tab of its group without showing it to the person unless `follow` allows.
    pub fn select(&mut self, id: &str) -> bool {
        let Some(group) = self.get(id).map(|tab| tab.group.clone()) else { return false };
        match &group {
            Some(key) => {
                if let Some(group) = self.groups.iter_mut().find(|group| &group.key == key) {
                    group.active = Some(id.to_owned());
                }
            }
            None => self.own_active = Some(id.to_owned()),
        }
        self.follow(id);
        true
    }

    /// Closes `id`; the tab next to it takes its place when it was the one shown, and a page left with no tab gets a blank one. A
    /// group with no tab left goes. Gives the tokens of the commands the tab still had, whose answers will not come.
    pub fn close(&mut self, id: &str) -> Vec<u64> {
        let Some(at) = self.list.iter().position(|tab| tab.id == id) else { return Vec::new() };
        let before = self.order();
        let place = before.iter().position(|other| other == id).unwrap_or(0);
        let gone = self.list.remove(at);
        let tokens = gone.commands.iter().filter_map(|command| command["token"].as_u64()).collect();
        if let Some(key) = &gone.group {
            if self.of(key).is_empty() {
                self.groups.retain(|group| &group.key != key);
            }
        }
        if self.list.is_empty() {
            let blank = self.open(None, None);
            self.active = blank;
            return tokens;
        }
        let after = self.order();
        // A window a page opened gives way to its opener, as a sign-in's window does once it is done.
        let opener = gone.opened_by.filter(|opener| self.get(opener).is_some());
        let next = opener.unwrap_or_else(|| after.get(place.min(after.len().saturating_sub(1))).cloned().unwrap_or_default());
        if self.own_active.as_deref() == Some(id) {
            self.own_active = self.of("").last().map(|tab| tab.id.clone());
        }
        if self.active == id {
            self.activate(&next);
        }
        tokens
    }

    /// Notes that `id` is in use at `now`, which keeps it from the idle check.
    pub fn touch(&mut self, id: &str, now: f64) {
        if let Some(tab) = self.get_mut(id) {
            tab.used = Some(now);
        }
    }

    /// Whether an agent has a tab, which is when the idle check has anything to do.
    pub fn has_agent_tabs(&self) -> bool {
        self.list.iter().any(|tab| tab.group.is_some())
    }

    /// The agents' tabs nobody has used for more than `limit_ms` at `now`: the person's own tabs and the one on screen never.
    /// The tab on screen counts as used now, so it starts its count when the person looks away. A tab with no time yet starts its
    /// count now. Closing them is up to the caller, since a close has to tell whoever waits on the tab.
    pub fn idle(&mut self, now: f64, limit_ms: f64) -> Vec<String> {
        let shown = self.active.clone();
        let mut idle = Vec::new();
        for tab in &mut self.list {
            if tab.id == shown {
                tab.used = Some(now);
            }
            let used = *tab.used.get_or_insert(now);
            if tab.group.is_some() && tab.id != shown && now - used > limit_ms {
                idle.push(tab.id.clone());
            }
        }
        idle
    }

    /// Sends the tab's view a command made from a new token, which the view runs once, and gives the token.
    pub fn run(&mut self, id: &str, make: impl FnOnce(u64) -> Value) -> Option<u64> {
        self.get(id)?;
        self.token += 1;
        let token = self.token;
        let tab = self.get_mut(id)?;
        tab.commands.push(make(token));
        // The view has run all but the newest few by the time the page draws again.
        let extra = tab.commands.len().saturating_sub(COMMANDS_KEPT);
        tab.commands.drain(..extra);
        Some(token)
    }

    pub fn send(&mut self, id: &str, action: &str, url: Option<&str>) {
        let Some(tab) = self.get_mut(id) else { return };
        // The view isn't there yet: an address starts it there, and nothing else has a page to act on.
        if tab.blank {
            if let ("load", Some(url)) = (action, url) {
                tab.blank = false;
                tab.start = url.to_owned();
            }
            return;
        }
        self.run(id, |token| wings_plugin::ui::web_command(action, url, token));
    }

    /// The tab a command's token was sent to, while the tab still keeps the command.
    pub fn tab_of_token(&self, token: u64) -> Option<&str> {
        self.list.iter().find(|tab| tab.commands.iter().any(|command| command["token"].as_u64() == Some(token))).map(|tab| tab.id.as_str())
    }

    /// What is kept with the Wings tab: the tabs' addresses and titles, the groups and the tab shown.
    pub fn snapshot(&self) -> Value {
        let tabs: Vec<Value> = self.list.iter().map(|tab| json!({ "id": tab.id, "url": if tab.blank { "" } else { tab.url() }, "title": tab.state.title, "group": tab.group })).collect();
        let groups: Vec<Value> = self.groups.iter().map(|group| json!({ "key": group.key, "name": group.name, "collapsed": group.collapsed, "active": group.active })).collect();
        json!({ "tabs": tabs, "groups": groups, "active": self.active, "own": self.own_active, "next": self.next, "args": self.args_seen })
    }

    /// Takes the tabs `saved` kept in place of these. False, and nothing changed, when it has none.
    pub fn restore(&mut self, saved: &Value) -> bool {
        let text = |value: &Value, key: &str| value[key].as_str().unwrap_or_default().to_owned();
        let mut list: Vec<Tab> = Vec::new();
        for kept in saved["tabs"].as_array().into_iter().flatten() {
            let id = text(kept, "id");
            if id.is_empty() || list.iter().any(|tab| tab.id == id) {
                continue;
            }
            let url = text(kept, "url");
            let mut tab = Tab::new(id, kept["group"].as_str().map(str::to_owned), (!url.is_empty()).then_some(url.as_str()));
            tab.state.url = url;
            tab.state.title = text(kept, "title");
            list.push(tab);
        }
        if list.is_empty() {
            return false;
        }
        let groups: Vec<Group> = saved["groups"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|group| list.iter().any(|tab| tab.group.as_deref() == group["key"].as_str()))
            .map(|group| Group { key: text(group, "key"), name: text(group, "name"), collapsed: group["collapsed"] == true, active: group["active"].as_str().map(str::to_owned) })
            .collect();
        // A tab in a group that wasn't kept is the person's own.
        for tab in &mut list {
            if tab.group.as_ref().is_some_and(|key| !groups.iter().any(|group| &group.key == key)) {
                tab.group = None;
            }
        }
        let numbered = list.iter().filter_map(|tab| tab.id.strip_prefix('t')?.parse::<u64>().ok()).max().unwrap_or(0) + 1;
        self.next = saved["next"].as_u64().unwrap_or(0).max(numbered);
        self.active = saved["active"].as_str().filter(|id| list.iter().any(|tab| tab.id == *id)).map_or_else(|| list[0].id.clone(), str::to_owned);
        self.own_active = saved["own"].as_str().filter(|id| list.iter().any(|tab| tab.id == *id)).map(str::to_owned);
        self.args_seen = text(saved, "args");
        self.list = list;
        self.groups = groups;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: (&str, &str) = ("ses-a", "Fix login bug");
    const B: (&str, &str) = ("ses-b", "Write docs");

    fn ids(tabs: &Tabs) -> Vec<String> {
        tabs.order()
    }

    #[test]
    fn a_page_starts_with_one_blank_tab_of_the_persons_own() {
        let tabs = Tabs::default();
        assert_eq!((tabs.list.len(), tabs.active_tab().blank, tabs.active_tab().group.clone()), (1, true, None));
        assert_eq!(tabs.active, "t1");
    }

    #[test]
    fn tabs_open_get_activated_and_close_with_the_neighbour_taking_over() {
        let mut tabs = Tabs::new(Some("https://a.com/"), None);
        let b = tabs.open(None, Some("https://b.com/"));
        let c = tabs.open(None, None);
        assert_eq!((tabs.active.as_str(), ids(&tabs)), (c.as_str(), vec!["t1".to_owned(), "t2".into(), "t3".into()]), "a tab of the person's own is shown when opened");
        tabs.activate("t1");
        assert_eq!(tabs.active, "t1");
        tabs.activate(&b);
        tabs.close(&b);
        assert_eq!(tabs.active, "t3", "the next one takes the place of the active one that closed");
        tabs.close("t3");
        assert_eq!(tabs.active, "t1", "and the one before when it was the last");
        tabs.activate("t1");
        // Closing one that isn't shown leaves the shown one.
        let d = tabs.open(None, Some("https://d.com/"));
        tabs.activate("t1");
        tabs.close(&d);
        assert_eq!(tabs.active, "t1");
    }

    #[test]
    fn closing_the_last_tab_leaves_one_blank_tab() {
        let mut tabs = Tabs::new(Some("https://a.com/"), None);
        tabs.close("t1");
        assert_eq!(tabs.list.len(), 1);
        assert!(tabs.active_tab().blank && tabs.active_tab().group.is_none());
        assert_eq!(tabs.active, tabs.list[0].id);
    }

    #[test]
    fn an_agents_tab_is_in_its_own_group_and_never_moves_the_persons_tab() {
        let mut tabs = Tabs::new(Some("https://mine.com/"), None);
        let a = tabs.open(Some(A), Some("https://a.com/"));
        assert_eq!(tabs.active, "t1", "the person's tab stays");
        let b = tabs.open(Some(B), Some("https://b.com/"));
        assert_eq!((tabs.target("ses-a"), tabs.target("ses-b"), tabs.target("")), (Some(a.clone()), Some(b), Some("t1".into())));
        assert_eq!(tabs.target("ses-none"), None);
        assert_eq!(tabs.group_name(tabs.get(&a).unwrap()), Some("Fix login bug"));
        assert_eq!(tabs.active, "t1");
    }

    #[test]
    fn a_new_tab_in_the_group_the_person_watches_comes_forward_and_one_in_another_group_does_not() {
        let mut tabs = Tabs::new(Some("https://mine.com/"), None);
        let a1 = tabs.open(Some(A), Some("https://a.com/"));
        tabs.activate(&a1);
        let a2 = tabs.open(Some(A), Some("https://a.com/2"));
        assert_eq!(tabs.active, a2, "the person is looking at that group");
        let b = tabs.open(Some(B), Some("https://b.com/"));
        assert_eq!(tabs.active, a2, "another group's tab stays behind");
        assert!(tabs.select(&b));
        assert_eq!(tabs.active, a2);
        assert_eq!(tabs.target("ses-b"), Some(b));
    }

    #[test]
    fn an_agents_tab_shows_when_the_person_sees_no_web_page() {
        let mut tabs = Tabs::default();
        let a = tabs.open(Some(A), Some("https://a.com/"));
        assert_eq!(tabs.active, a, "the blank start page gives way");
    }

    #[test]
    fn closing_an_agents_last_tab_ends_its_group() {
        let mut tabs = Tabs::new(Some("https://mine.com/"), None);
        let a1 = tabs.open(Some(A), Some("https://a.com/"));
        let a2 = tabs.open(Some(A), Some("https://a.com/2"));
        tabs.close(&a2);
        assert_eq!(tabs.target("ses-a"), Some(a1.clone()), "the group's tab falls back to one that is left");
        tabs.close(&a1);
        assert!(tabs.group("ses-a").is_none() && tabs.target("ses-a").is_none());
        assert_eq!(tabs.list.len(), 1);
    }

    #[test]
    fn idle_agent_tabs_are_found_after_the_limit_never_the_shown_tab_or_the_persons_own() {
        const LIMIT: f64 = 30.0 * 60_000.0;
        let mut tabs = Tabs::new(Some("https://mine.com/"), None);
        let a = tabs.open(Some(A), Some("https://a.com/"));
        let b = tabs.open(Some(B), Some("https://b.com/"));
        let own = tabs.open(None, Some("https://own.com/"));
        tabs.activate("t1");
        assert!(tabs.has_agent_tabs());
        // The first check starts every count, so nothing is idle yet.
        assert!(tabs.idle(1000.0, LIMIT).is_empty());
        // Just short of the limit, and one agent's tab used since.
        assert!(tabs.idle(1000.0 + LIMIT, LIMIT).is_empty());
        tabs.touch(&b, 1000.0 + LIMIT);
        assert_eq!(tabs.idle(2000.0 + LIMIT, LIMIT), [a.clone()], "only the tab nobody used, not the person's own ones");
        assert!(!tabs.idle(2000.0 + LIMIT, LIMIT).contains(&own));
        // The tab shown is never closed, and counts from when the person looks away.
        tabs.activate(&a);
        assert!(tabs.idle(3000.0 + LIMIT, LIMIT).is_empty());
        tabs.activate("t1");
        assert!(tabs.idle(4000.0 + LIMIT, LIMIT).is_empty(), "it was shown a moment ago");
        assert_eq!(tabs.idle(4000.0 + 2.0 * LIMIT, LIMIT), [a, b]);
        assert!(!Tabs::default().has_agent_tabs());
    }

    #[test]
    fn tokens_are_one_count_for_all_tabs_and_a_result_maps_to_its_tab() {
        let mut tabs = Tabs::new(Some("https://a.com/"), None);
        let b = tabs.open(Some(A), Some("https://b.com/"));
        let one = tabs.run("t1", |token| json!({ "token": token })).unwrap();
        let two = tabs.run(&b, |token| json!({ "token": token })).unwrap();
        let three = tabs.run("t1", |token| json!({ "token": token })).unwrap();
        assert_eq!((one, two, three), (1, 2, 3));
        assert_eq!((tabs.tab_of_token(1), tabs.tab_of_token(2), tabs.tab_of_token(3)), (Some("t1"), Some(b.as_str()), Some("t1")));
        assert_eq!(tabs.tab_of_token(9), None);
        assert_eq!(tabs.close(&b), vec![2], "what the closed tab was still asked is given back");
        assert_eq!(tabs.run("nope", |token| json!({ "token": token })), None);
    }

    /// A window a page opens is a tab after its opener (and the ones it opened before), in its group, shown where the
    /// opener was shown; a background one stays behind.
    #[test]
    fn a_window_a_page_opens_is_a_tab_beside_its_opener_in_its_group() {
        let mut tabs = Tabs::new(Some("https://mine.com/"), None);
        tabs.open(None, Some("https://other.com/"));
        tabs.activate("t1");
        let sign_in = tabs.open_from("t1", "https://login.example/", Some(4), false).unwrap();
        assert_eq!(ids(&tabs), ["t1", "t3", "t2"], "next to its opener, not at the end");
        assert_eq!(tabs.active, sign_in, "the person saw the opener, so they see it");
        assert_eq!(tabs.get(&sign_in).map(|tab| (tab.popup, tab.start.as_str(), tab.blank)), Some((Some(4), "https://login.example/", false)));
        tabs.activate("t1");
        let behind = tabs.open_from("t1", "https://docs.example/", None, true).unwrap();
        assert_eq!(ids(&tabs), ["t1", "t3", "t4", "t2"], "after the tab it opened before");
        assert_eq!((tabs.active.as_str(), tabs.get(&behind).unwrap().popup), ("t1", None), "a background tab stays behind");
        // An agent's page opens a window: in its group, and its group's tab, without moving the person.
        let agent = tabs.open(Some(A), Some("https://a.com/"));
        let popup = tabs.open_from(&agent, "https://login.example/", Some(5), false).unwrap();
        assert_eq!((tabs.get(&popup).unwrap().group.as_deref(), tabs.target("ses-a")), (Some("ses-a"), Some(popup.clone())));
        assert_eq!(tabs.active, "t1");
        assert_eq!(tabs.open_from("t99", "https://x.example/", None, false), None, "its opener closed");
        // Closing a window that is shown shows its opener again, not the tab beside it.
        tabs.activate(&sign_in);
        tabs.close(&sign_in);
        assert_eq!(tabs.active, "t1");
    }

    #[test]
    fn tabs_groups_and_the_shown_tab_are_kept_and_come_back() {
        let mut tabs = Tabs::new(Some("https://mine.com/"), None);
        tabs.get_mut("t1").unwrap().state.title = "Mine".into();
        let a = tabs.open(Some(A), Some("https://a.com/"));
        tabs.open(Some(B), Some("https://b.com/"));
        tabs.open(None, None);
        tabs.groups[1].collapsed = true;
        tabs.args_seen = "x".into();
        tabs.select(&a);
        let kept = tabs.snapshot();
        let mut back = Tabs::default();
        assert!(back.restore(&kept));
        assert_eq!(back.snapshot(), kept);
        assert_eq!(ids(&back), ids(&tabs));
        assert_eq!((back.groups[1].collapsed, back.groups[0].name.as_str(), back.active.as_str()), (true, "Fix login bug", tabs.active.as_str()));
        assert_eq!(back.get("t1").unwrap().label(), "Mine");
        assert!(back.list.last().unwrap().blank, "a blank tab comes back blank");
        // The next tab doesn't reuse an id.
        let new = back.open(None, None);
        assert!(!ids(&tabs).contains(&new));
        assert!(!Tabs::default().restore(&json!({})) && !Tabs::default().restore(&Value::Null));
    }
}
