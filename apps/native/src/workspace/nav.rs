//! Back and forward, as the desktop's `lib/nav-history.ts`: the places you have been (a project and one of its tabs),
//! where you are, and the ones you went back from. Every change of project or tab arrives as the place on screen, so
//! nothing that navigates has to know about the history.

use std::path::PathBuf;

use gpui::{Context, Window};

use super::{Active, Workspace};

/// How many places Back remembers.
const LIMIT: usize = 50;

/// Where you are: a project and one of its tabs. Sidebars and dialogs aren't places.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Place {
    pub project: PathBuf,
    pub tab: Active,
}

#[derive(Default)]
pub(super) struct Nav {
    back: Vec<Place>,
    current: Option<Place>,
    forward: Vec<Place>,
}

impl Nav {
    /// You are now at `place`. A different place goes onto the back stack and clears forward, like a browser; the same
    /// place changes nothing, which is also what a Back or Forward looks like once it has been shown.
    pub fn visit(&mut self, place: Option<Place>) {
        let Some(place) = place else {
            return;
        };
        if self.current.as_ref() == Some(&place) {
            return;
        }
        if let Some(current) = self.current.take() {
            if self.back.last() != Some(&current) {
                self.back.push(current);
            }
            let over = self.back.len().saturating_sub(LIMIT);
            self.back.drain(..over);
        }
        self.current = Some(place);
        self.forward.clear();
    }

    /// The latest place in a stack to go to. Closed tabs and the place you're already at aren't somewhere to go.
    fn reachable(&self, back: bool, exists: impl Fn(&Place) -> bool) -> Option<usize> {
        let stack = if back { &self.back } else { &self.forward };
        stack
            .iter()
            .rposition(|place| Some(place) != self.current.as_ref() && exists(place))
    }

    pub fn can_go(&self, back: bool, exists: impl Fn(&Place) -> bool) -> bool {
        self.reachable(back, exists).is_some()
    }

    /// Back (or forward) to the latest place that still exists; the places skipped on the way are dropped. `None`
    /// when there is nowhere to go.
    pub fn go(&mut self, back: bool, exists: impl Fn(&Place) -> bool) -> Option<Place> {
        let at = self.reachable(back, exists)?;
        let current = self.current.take()?;
        let (from, to) = if back {
            (&mut self.back, &mut self.forward)
        } else {
            (&mut self.forward, &mut self.back)
        };
        let place = from[at].clone();
        from.truncate(at);
        to.push(current);
        let over = self.back.len().saturating_sub(LIMIT);
        self.back.drain(..over);
        self.current = Some(place.clone());
        Some(place)
    }
}

impl Workspace {
    /// The place on screen, or none while the welcome shows.
    pub(super) fn place(&self) -> Option<Place> {
        (!self.welcome).then(|| Place {
            project: self.config.project.clone(),
            tab: self.active,
        })
    }

    /// Whether a place can still be shown: its project and its tab are still open.
    fn place_exists(&self, place: &Place) -> bool {
        if place.project == self.config.project {
            return self.tabs().contains(&place.tab);
        }
        self.parked
            .get(&place.project)
            .is_some_and(|parked| match place.tab {
                Active::Terminal(id) => parked.terminals.iter().any(|tab| tab.id == id),
                Active::Plugin(id) => parked.plugin_tabs.iter().any(|tab| tab.id == id),
                Active::Project => parked.project_page.is_some(),
                Active::Issues => parked.issues_open,
            })
    }

    pub(super) fn can_navigate(&self, back: bool) -> bool {
        self.nav.can_go(back, |place| self.place_exists(place))
    }

    /// Back or Forward: shows the place's project, then its tab.
    pub(super) fn navigate(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mut nav = std::mem::take(&mut self.nav);
        let place = nav.go(back, |place| self.place_exists(place));
        self.nav = nav;
        let Some(Place { project, tab }) = place else {
            return;
        };
        self.switch_project(project, window, cx);
        self.show(tab, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(tab: usize) -> Place {
        Place {
            project: "p".into(),
            tab: Active::Terminal(tab),
        }
    }

    fn walk(tabs: &[usize]) -> Nav {
        let mut nav = Nav::default();
        for tab in tabs {
            nav.visit(Some(at(*tab)));
        }
        nav
    }

    fn ids(stack: &[Place]) -> Vec<usize> {
        stack
            .iter()
            .map(|place| match place.tab {
                Active::Terminal(id) => id,
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn back_and_forward_move_between_places_and_a_new_place_clears_forward() {
        let all = |_: &Place| true;
        let mut nav = walk(&[1, 2, 3]);
        assert_eq!(nav.go(true, all), Some(at(2)));
        assert_eq!((ids(&nav.back), ids(&nav.forward)), (vec![1], vec![3]));
        // The app shows the place it was sent to, which visits it: no change.
        nav.visit(Some(at(2)));
        assert_eq!(ids(&nav.forward), vec![3]);
        assert_eq!(nav.go(false, all), Some(at(3)));
        assert_eq!((ids(&nav.back), ids(&nav.forward)), (vec![1, 2], vec![]));
        nav.go(true, all);
        nav.visit(Some(at(9)));
        assert!(!nav.can_go(false, all));
        assert_eq!(ids(&nav.back), vec![1, 2]);
    }

    #[test]
    fn back_skips_and_drops_places_that_are_gone_and_keeps_the_latest_fifty() {
        let mut nav = walk(&[1, 2, 3, 4]);
        assert_eq!(nav.go(true, |place| *place != at(3)), Some(at(2)));
        assert_eq!((ids(&nav.back), ids(&nav.forward)), (vec![1], vec![4]));
        assert_eq!(nav.go(true, |_| false), None);
        let long: Vec<usize> = (0..LIMIT + 20).collect();
        let nav = walk(&long);
        assert_eq!(nav.back.len(), LIMIT);
        assert_eq!(ids(&nav.back)[0], 19);
    }
}
