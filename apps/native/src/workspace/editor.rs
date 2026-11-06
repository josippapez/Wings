//! "Open in <editor>" for the project on screen: the desktop's editor detection and launch (`wings-editors`), its
//! choice in Settings, Editor, and the project bar's button with the other editors one click further.

use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use wings_editors::EditorView;

use crate::{
    icons::{Icon, icon},
    theme,
    ui::{self, Variant},
};

use super::Workspace;

impl Workspace {
    /// Looks the installed editors up once, off the UI thread, as the desktop does for the app's lifetime.
    pub(super) fn find_editors(&mut self, cx: &mut Context<Self>) {
        if let Some(listed) = self.config.editors.clone() {
            self.editors = listed;
            return;
        }
        let found = cx
            .background_executor()
            .spawn(async { wings_editors::list() });
        cx.spawn(async move |this, cx| {
            let found = found.await;
            this.update(cx, |this, cx| {
                this.editors = found;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The editor chosen in Settings, else the first one found.
    pub(super) fn default_editor(&self) -> Option<&EditorView> {
        self.editors
            .iter()
            .find(|editor| Some(&editor.id) == self.prefs.editor.as_ref())
            .or(self.editors.first())
    }

    /// Opens the project on screen in the editor with this id, and says why when it can't.
    pub(super) fn open_in_editor(&mut self, id: &str, cx: &mut Context<Self>) {
        self.editor_menu = false;
        let Some(editor) = self.editors.iter().find(|editor| editor.id == id).cloned() else {
            return;
        };
        let folder = self.config.project.clone();
        let opened = cx.background_executor().spawn(async move {
            wings_editors::open(&editor.id, &folder).map_err(|e| (editor.name, e))
        });
        cx.spawn(async move |this, cx| {
            if let Err((name, error)) = opened.await {
                this.update(cx, |this, cx| {
                    this.show_notice(format!("Couldn't open {name}"), error, cx)
                })
                .ok();
            }
        })
        .detach();
        cx.notify();
    }

    /// The project bar's "Open in <editor>", and with more editors a menu of the others. Nothing without one.
    pub(super) fn editor_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let main = self.default_editor()?.clone();
        let items: Vec<AnyElement> = self
            .editors
            .iter()
            .map(|editor| {
                let id = editor.id.clone();
                ui::select_item(
                    SharedString::from(format!("open-in-{}", editor.id)),
                    editor.name.clone(),
                    editor.id == main.id,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.open_in_editor(&id, cx)))
                .into_any_element()
            })
            .collect();
        let more = self.editors.len() > 1;
        let id = main.id.clone();
        Some(
            div()
                .flex()
                .flex_col()
                .items_end()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(2.0))
                        .child(
                            ui::button(
                                "open-in-editor",
                                format!("Open in {}", main.name),
                                Variant::Ghost,
                                false,
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .text_size(px(12.0))
                                    .child(icon(Icon::Code, 14.0, theme::MUTED_FOREGROUND))
                                    .child(format!("Open in {}", main.name)),
                            )
                            .h(px(28.0))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.open_in_editor(&id, cx)),
                            ),
                        )
                        .when(more, |row| {
                            row.child(
                                ui::icon_button(
                                    "open-in-menu",
                                    Icon::ChevronDown,
                                    "Open in…",
                                    ui::ButtonSize::Sm,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.editor_menu = !this.editor_menu;
                                        cx.notify();
                                    },
                                )),
                            )
                        }),
                )
                .when(more && self.editor_menu, |column| {
                    column.child(ui::popover(
                        ui::select_menu(items)
                            .mt(px(32.0))
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.editor_menu = false;
                                cx.notify();
                            })),
                    ))
                })
                .into_any_element(),
        )
    }
}
