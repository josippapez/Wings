//! The `Image` a page shows: a picture it registered, drawn at the size its box allows. Its pixels come from
//! `page::images`, which decodes it once on a thread of its own; until then, and when it can't be shown, a quiet box of
//! the same size stands in, so the page doesn't move when the picture arrives.
use super::*;
use crate::page::images::{self, Shown, Wake};
use gpui::{ObjectFit, StyledImage, img};

impl PageView {
    pub(super) fn picture(&self, picture: &tree::Picture, cx: &mut Context<Self>) -> AnyElement {
        let (window_w, window_h, scale) = self.viewport.get();
        // In a dialog, what it leaves; elsewhere, what a wide dialog over the whole window would.
        let window = DIALOG_ROOM.with(Cell::get).or_else(|| (window_w > 0.0).then(|| tree::picture_room((window_w, window_h), f32::INFINITY)));
        // The pixels it needs on its longest side: its box's, as a screen of this density has them.
        let level = (picture.max_width.max(picture.max_height) * scale.max(1.0)).ceil() as u32;
        // A render that finds it still decoding doesn't ask to be woken again: the first one's wake is enough.
        let (done, heard) = async_channel::bounded::<()>(1);
        let waiting = self.images_waiting.borrow().contains(&picture.id);
        let wake: Option<Wake> = (!waiting).then(|| Box::new(move || { let _ = done.try_send(()); }) as Wake);
        let shown = images::show(self.source.plugin(), &picture.src, level, wake);
        let (size, inside) = match &shown {
            Shown::Ready { width, height, .. } | Shown::Waiting { width, height } | Shown::Broken { width, height } => {
                (picture.size(*width, *height, window), None)
            }
            Shown::Missing => ((picture.max_width.min(160.0), picture.max_height.min(90.0)), Some("Image no longer kept")),
        };
        let inside = match &shown {
            Shown::Broken { .. } => Some("Can't show this image"),
            _ => inside,
        };
        if matches!(shown, Shown::Waiting { .. }) && !waiting {
            let id = picture.id;
            self.images_waiting.borrow_mut().insert(id);
            cx.spawn(async move |view, cx| {
                // Woken by a decode that ended, or by the picture being let go of: either way the page draws it again.
                let _ = heard.recv().await;
                let _ = view.update(cx, |view, cx| {
                    view.images_waiting.borrow_mut().remove(&id);
                    cx.notify();
                });
            })
            .detach();
        }
        let frame = div()
            .id(ElementId::NamedInteger("page-image".into(), picture.id))
            .role(Role::Image)
            .when_some(picture.label.clone(), |el, label| el.aria_label(label))
            .flex_none()
            .w(px(size.0))
            .h(px(size.1))
            .rounded(px(picture.radius))
            .overflow_hidden();
        let frame = match shown {
            Shown::Ready { decoded, .. } => frame.child(
                img(decoded.image.clone())
                    .size_full()
                    .rounded(px(picture.radius))
                    .object_fit(if picture.cover { ObjectFit::Cover } else { ObjectFit::Fill }),
            ),
            _ => frame
                .bg(rgba(0xffffff12))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.0))
                .text_color(rgb(theme::palette().muted_foreground))
                .children(inside),
        };
        match picture.on_click.clone() {
            Some(handler) => frame
                .cursor_pointer()
                .on_click(cx.listener(move |view, _, _, cx| {
                    cx.stop_propagation();
                    view.act(&handler, None);
                }))
                .into_any_element(),
            None => frame.into_any_element(),
        }
    }
}
