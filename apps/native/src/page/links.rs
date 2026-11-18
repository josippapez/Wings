//! Links in a page's text to what Wings' plugins open, like an issue key: the rules of the plugins that run, which the
//! workspace keeps current, and what a click on one opens. Only a click opens one: a page never can.

use std::{rc::Rc, sync::Arc};

use gpui::{App, Global, Window};
use wings_plugins::links::Rule;

/// What a click on a link opens: the plugin and link that found it, and its text.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkTarget {
    pub plugin: String,
    pub link: String,
    pub text: String,
}

/// The links of the plugins that run, and what opens one. Without it a page shows no links.
pub struct PageLinks {
    pub rules: Arc<Vec<Rule>>,
    /// Goes up each time `rules` change, so a page finds its links again.
    pub generation: u64,
    pub open: Rc<dyn Fn(LinkTarget, &mut Window, &mut App)>,
}

impl Global for PageLinks {}
