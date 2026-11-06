//! shadcn's `Tooltip` with a `Kbd`, as the desktop's `IconButton` shows one: the button's name and its shortcut in a
//! light pill, half a second after the pointer stops on it (GPUI's delay, the desktop's `TooltipProvider delay={500}`).

use gpui::{
    AnyView, App, Context, FontWeight, IntoElement, Render, SharedString, Window, div, prelude::*,
    px, rgb, rgba,
};

use crate::theme;

pub struct Tooltip {
    label: SharedString,
    shortcut: Option<SharedString>,
}

/// What `.tooltip(..)` takes: a tooltip of `label`, with `shortcut` in a key cap when there is one.
pub fn tooltip(
    label: impl Into<SharedString>,
    shortcut: Option<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let label = label.into();
    move |_, cx| {
        let (label, shortcut) = (label.clone(), shortcut.clone());
        cx.new(|_| Tooltip { label, shortcut }).into()
    }
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // GPUI puts a tooltip's corner at the pointer, so it starts below the cursor's arrow.
        div().pt(px(20.0)).child(
            div()
                .max_w(px(320.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(12.0))
                .py(px(6.0))
                .when(self.shortcut.is_some(), |pill| pill.pr(px(6.0)))
                .rounded(px(6.0))
                .bg(rgb(theme::FOREGROUND))
                .text_size(px(12.0))
                .text_color(rgb(theme::BACKGROUND))
                .child(self.label.clone())
                .when_some(self.shortcut.clone(), |pill, keys| {
                    pill.child(
                        div()
                            .h(px(20.0))
                            .min_w(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .px(px(4.0))
                            .rounded(px(4.0))
                            .bg(rgba(theme::BACKGROUND << 8 | 0x1a))
                            .font_weight(FontWeight::MEDIUM)
                            .child(keys),
                    )
                }),
        )
    }
}
