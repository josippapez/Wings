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
            let path = self.config.project.clone();
            let entry = self.projects.iter().find(|entry| entry.path == path);
            let name = entry.map_or_else(
                || path.to_string_lossy().into_owned(),
                |entry| entry.name.clone(),
            );
            let branch = entry.and_then(|entry| entry.branch.clone());
            let blocks = self.plugins.iter().flat_map(installed::blocks).collect();
            self.project_page = Some(
                cx.new(|cx| ProjectPage::new(&path, &name, branch.as_deref(), info, blocks, cx)),
            );
        }
        self.show(Active::Project, window, cx);
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
