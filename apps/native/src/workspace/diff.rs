use super::Workspace;
use crate::diff::{
    View,
    viewer::{Event, Viewer},
};
use gpui::{AppContext, Context, Focusable, Window};

impl Workspace {
    pub(super) fn open_diff_view(
        &mut self,
        view: View,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self
            .plugins
            .iter()
            .find(|installed| installed.id == view.plugin_id)
            .map(|installed| installed.host.clone())
        else {
            return;
        };
        self.plugin_ui.diff = None;
        window.activate_window();
        let (wrap, tree) = (self.prefs.diff_wrap, self.prefs.diff_tree);
        let viewer = cx.new(|cx| {
            let mut viewer = Viewer::new(view, host, cx);
            viewer.set_layout(wrap, tree, cx);
            viewer
        });
        let events = cx.subscribe_in(&viewer, window, |this, _, event, window, cx| match event {
            Event::Close => this.close_diff_view(window, cx),
            Event::Layout { wrap, tree } => this.set_prefs(
                |prefs| {
                    prefs.diff_wrap = *wrap;
                    prefs.diff_tree = *tree;
                },
                cx,
            ),
        });
        viewer.focus_handle(cx).focus(window, cx);
        self.plugin_ui.diff = Some((viewer, events));
        cx.notify();
    }

    pub(super) fn close_diff_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.plugin_ui.diff = None;
        self.show(self.active, window, cx);
    }
}
