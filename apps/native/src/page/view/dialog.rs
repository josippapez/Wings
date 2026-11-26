//! A `Dialog` over the whole page.
use super::*;

impl PageView {
    /// A `Dialog` over the whole page, as Wings' own dialogs are (`ui::dialog`): its title and a close button at the top,
    /// and what it holds under them, which scrolls. Escape, a press on the backdrop and the close button send `onClose`.
    pub(super) fn dialog(&self, dialog: &tree::Dialog, cx: &mut Context<Self>) -> AnyElement {
        let id = dialog.id;
        let (backdrop, panel) = ui::dialog(
            ElementId::NamedInteger("page-dialog".into(), id),
            dialog.title.clone(),
            &self.dialog_focus,
            dialog.width,
            None,
        );
        let (on_backdrop, on_button, on_escape) =
            (dialog.on_close.clone(), dialog.on_close.clone(), dialog.on_close.clone());
        let header = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .pl(px(24.0))
            .pr(px(12.0))
            .pt(px(14.0))
            .pb(px(6.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(dialog.title.clone()),
            )
            .child(
                ui::icon_button(
                    ElementId::NamedInteger("page-dialog-close".into(), id),
                    crate::icons::Icon::X,
                    "Close",
                    ui::ButtonSize::Sm,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(handler) = &on_button {
                        view.act(handler, None);
                    }
                })),
            );
        let body = div()
            .id(ElementId::NamedInteger("page-dialog-body".into(), id))
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_y_scroll();
        let (window_w, window_h, _) = self.viewport.get();
        let area = self.dialog_area.get().unwrap_or((window_w, window_h));
        let room = DIALOG_ROOM.with(|cell| cell.replace(Some(tree::picture_room(area, dialog.width))));
        // The padding is on a child, inside what scrolls: padding on the scroll box itself is clipped away with the overflow,
        // so the last row met the panel's edge.
        let body = body.child(
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(12.0))
                .px(px(24.0))
                .pt(px(4.0))
                .pb(px(28.0))
                .children(self.nodes(&dialog.children, cx)),
        );
        DIALOG_ROOM.with(|cell| cell.set(room));
        let entity = cx.entity();
        let measure = canvas(
            move |bounds, _, cx| {
                let area = Some((f32::from(bounds.size.width), f32::from(bounds.size.height)));
                entity.update(cx, |view, cx| {
                    if view.dialog_area.replace(area) != area {
                        cx.notify();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        backdrop
            .child(measure)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |view, _, _, _| {
                    if let Some(handler) = &on_backdrop {
                        view.act(handler, None);
                    }
                }),
            )
            .child(ui::dialog_panel(
                panel
                    .flex_col()
                    // Before a field in it, which keeps Escape. A select's menu or a field's options open in the dialog
                    // close first, on their own.
                    .capture_escape({
                        let page = cx.entity();
                        move |_, window, cx| {
                            page.update(cx, |view, _| {
                                if view.shows_menu() {
                                    return false;
                                }
                                if let Some(handler) = &on_escape {
                                    view.send(handler, None);
                                }
                                window.prevent_default();
                                true
                            })
                        }
                    })
                    .children(ui::spot(|| "page-dialog".into()))
                    .child(header)
                    .child(body),
            ))
            .into_any_element()
    }
}
