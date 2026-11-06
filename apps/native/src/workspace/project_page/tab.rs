//! The project page as a tab of the shown project: opened from the sidebar row, closed with the tab's X or Cmd-W, and
//! kept while another tab shows or another project does (`ProjectPageTab` in the desktop app).

use std::path::PathBuf;

use gpui::{AppContext, Context, Window};

use crate::installed;

use super::{
    super::{Active, Workspace},
    ProjectPage,
};

impl Workspace {
    /// Shows `project`'s page, opening its tab if it isn't there yet.
    pub fn open_project_page(
        &mut self,
        project: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.switch_project(project, window, cx);
        let Some(info) = self.project_info.clone() else {
            self.show_notice(
                "Wings".into(),
                "The project page needs the desktop app's data folder, which wasn't found.".into(),
                cx,
            );
            return;
        };
        if self.project_page.is_none() {
            self.project_page = Some(self.new_project_page(info, cx));
        }
        self.show(Active::Project, window, cx);
    }

    /// The shown project's page, with the blocks of the plugins running now.
    fn new_project_page(
        &self,
        info: super::InfoStore,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<ProjectPage> {
        let path = self.config.project.clone();
        let entry = self.projects.iter().find(|entry| entry.path == path);
        let name = entry.map_or_else(
            || path.to_string_lossy().into_owned(),
            |entry| entry.name.clone(),
        );
        let branch = entry.and_then(|entry| entry.branch.clone());
        let blocks = self.plugins.iter().flat_map(installed::blocks).collect();
        cx.new(|cx| ProjectPage::new(&path, &name, branch.as_deref(), info, blocks, cx))
    }

    /// A plugin with blocks started or stopped: the open page starts over with the blocks that run now. The pages of
    /// projects not on screen close, so none keeps a stopped plugin's blocks running.
    pub(in crate::workspace) fn reload_project_pages(&mut self, cx: &mut Context<Self>) {
        for parked in self.parked.values_mut() {
            parked.project_page = None;
            if parked.active == Active::Project {
                parked.active = parked
                    .terminals
                    .first()
                    .map_or(Active::Issues, |tab| Active::Terminal(tab.id));
            }
        }
        if self.project_page.is_some()
            && let Some(info) = self.project_info.clone()
        {
            self.project_page = Some(self.new_project_page(info, cx));
        }
    }

    /// Closes the tab. The page and its blocks stop with it.
    pub(in crate::workspace) fn close_project_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_page.take().is_some() {
            let position = self.terminals.len() + self.plugin_tabs.len();
            self.after_closing(Active::Project, position, window, cx);
        }
    }

    /// Tells the page whether it is on screen, so only a shown page looks for changes.
    pub(in crate::workspace) fn sync_project_page(&mut self, cx: &mut Context<Self>) {
        let shown = self.active == Active::Project;
        if let Some(page) = &self.project_page {
            page.update(cx, |page, cx| page.set_visible(shown, cx));
        }
        for parked in self.parked.values() {
            if let Some(page) = &parked.project_page {
                page.update(cx, |page, cx| page.set_visible(false, cx));
            }
        }
    }
}
