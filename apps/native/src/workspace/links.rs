//! The links plugins declare in `contributes.links`, in the pages' text: the rules of the plugins that run, which the
//! pages read as a global, and what a click on one opens. A link opens only the plugin's own view, or a URL its `openUrl`
//! allows, and only when it is clicked.

use std::{rc::Rc, sync::Arc};

use gpui::{App, Context, Window};
use wings_plugins::{
    fetch::may_open_url,
    links::{Rule, url_of},
};

use crate::page::{
    links::{LinkTarget, PageLinks},
    plugin::Env,
};

use super::Workspace;

/// What a click on a link does, as its plugin declares it.
#[derive(Debug, PartialEq)]
enum Opens {
    View { view: String, arg: String },
    /// The address in its normal form, while the plugin's `openUrl` allows it.
    Url(Option<String>),
}

impl Workspace {
    /// Gives the pages the links of the plugins that run, each narrowed to the words its plugin set, and draws them again.
    pub(super) fn refresh_links(&mut self, cx: &mut Context<Self>) {
        self.set_page_links(cx);
        cx.refresh_windows();
    }

    /// `refresh_links` without drawing again, for the window as it opens, when nothing has drawn yet.
    pub(super) fn set_page_links(&mut self, cx: &mut Context<Self>) {
        let words = self.plugin_env.link_words.lock().unwrap().clone();
        let rules: Vec<Rule> = self
            .plugins
            .iter()
            .flat_map(|installed| {
                let words = &words;
                installed.plugin.manifest.contributes.links.iter().filter_map(move |link| {
                    let narrowed = words.get(&(installed.id.clone(), link.id.clone())).cloned();
                    Rule::new(&installed.id, link, narrowed)
                })
            })
            .collect();
        let generation = cx.try_global::<PageLinks>().map_or(0, |links| links.generation) + 1;
        let workspace = cx.weak_entity();
        cx.set_global(PageLinks {
            rules: Arc::new(rules),
            generation,
            open: Rc::new(move |target: LinkTarget, window: &mut Window, cx: &mut App| {
                let _ = workspace.update(cx, |workspace, cx| workspace.open_link(&target, window, cx));
            }),
        });
    }

    /// What a click on `target` opens, while its plugin runs and declares the link.
    fn link_opens(&self, target: &LinkTarget) -> Option<Opens> {
        let installed = self.plugins.iter().find(|installed| installed.id == target.plugin)?;
        let link = installed.plugin.manifest.contributes.links.iter().find(|link| link.id == target.link)?;
        Some(match link.url {
            true => Opens::Url(url_of(&target.text).filter(|url| may_open_url(&installed.plugin, url))),
            false => Opens::View { view: link.view.clone(), arg: link.arg.clone() },
        })
    }

    /// A click on a link in a page: its plugin's view opens as a tab with the text, or the address it names in the browser.
    pub(super) fn open_link(&mut self, target: &LinkTarget, window: &mut Window, cx: &mut Context<Self>) {
        match self.link_opens(target) {
            Some(Opens::View { view, arg }) => {
                let mut args = serde_json::Map::new();
                args.insert(arg, target.text.clone().into());
                self.open_plugin_view(&target.plugin, &view, Some(serde_json::Value::Object(args)), window, cx);
            }
            Some(Opens::Url(Some(url))) => {
                if let Err(error) = self.plugin_env.open_url(&url) {
                    self.show_notice("Couldn't open the link".into(), error, cx);
                }
            }
            Some(Opens::Url(None)) | None => {}
        }
    }
}
