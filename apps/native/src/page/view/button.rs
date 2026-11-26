//! A `Button`, or a chip.
use super::*;

impl PageView {
    /// A `Button`, or a chip. In a baseline row it starts `drop` points down, as a text there does.
    pub(super) fn button(&self, button: &tree::Button, drop: f32, cx: &mut Context<Self>) -> AnyElement {
        let element = self.button_element(button, drop, cx);
        match button.on_click.clone() {
            Some(handler) => element
                .on_click(cx.listener(move |view, _, _, cx| {
                    cx.stop_propagation();
                    view.act(&handler, None);
                }))
                .into_any_element(),
            None => element.into_any_element(),
        }
    }

    /// `button` before its click is added, for a menu's button, whose click opens the menu.
    pub(in super::super) fn button_element(
        &self,
        button: &tree::Button,
        drop: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let id = ElementId::NamedInteger("page-button".into(), button.id);
        let label = button
            .label
            .clone()
            .unwrap_or_else(|| plain_text(&button.children));
        let tip = button.title.clone().or_else(|| button.shortcut.as_ref().map(|_| label.clone()));
        // plugin-ui.css's gap between a button's parts: 6px, and 4px for a chip and a small button.
        let between =
            if button.chip || matches!(button.size, tree::ButtonSize::Xs | tree::ButtonSize::Sm) {
                4.0
            } else {
                6.0
            };
        let content = if button.wrap
            && !button
                .children
                .iter()
                .all(|node| matches!(node, Node::Str { .. }))
        {
            // Each text never narrower than its longest word, as in a baseline row: GPUI took a title's narrowest as
            // its whole line. The box round them sets no least width of its own, or it would be the whole line's and
            // the title would run on past the row instead of wrapping.
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(between))
                .children(button.children.iter().map(|node| match node {
                    Node::Text(text) => self.text_element(text, Some(0.0), cx),
                    node => self.node(node, cx),
                }))
                .into_any_element()
        } else {
            self.contents(&button.children, between, cx)
        };
        let element = if button.chip {
            ui::chip(id, label, button.pressed, button.disabled, content)
        } else {
            sized(
                ui::button(id, label, variant(button.variant), button.disabled, content),
                button.size,
            )
            .when(button.round, |el| el.rounded_full())
        };
        // A `showOnHover` button in a card is drawn clear until the pointer comes onto the card, or it has the keyboard.
        let hidden = match (button.show_on_hover, self.drawing_card.get()) {
            (true, Some((card, _))) => {
                self.drawing_card.set(Some((card, true)));
                self.hovered_card != Some(card)
            }
            _ => false,
        };
        #[cfg(feature = "test-support")]
        if hidden {
            self.hidden_buttons.borrow_mut().push(button.id);
        }
        let shown = if button.disabled { 0.5 } else { 1.0 };
        let element = element
            .when(hidden, |el| {
                el.opacity(0.0)
                    .focus_visible(move |style| style.border_color(rgb(theme::palette().foreground)).opacity(shown))
            })
            // A button that holds a text cut short narrows to fit its row and cuts that text, as each desktop page
            // with one makes it do in its own CSS (Issues' linked issues and parent lanes, Disk usage's folders): an
            // issue linked by a long title ran past the properties column and the window's edge.
            .when(
                button
                    .children
                    .iter()
                    .any(|node| matches!(node, Node::Text(text) if text.truncate)),
                |el| el.flex_shrink_1().min_w_0(),
            )
            .when(button.wrap, |el| {
                el.flex_shrink_1()
                    .min_w_0()
                    .h_auto()
                    .py(px(WRAPPED_PADDING))
                    .justify_start()
                    .whitespace_normal()
            })
            .when(drop > 0.0, |el| el.mt(px(drop)))
            // While the screenshot examples check a frame for what it cuts off, a label of plain words, which a squeezed
            // button draws past its edges, says where it is drawn.
            .children(ui::cutoff::label(|| {
                let plain = button.children.iter().all(|node| matches!(node, Node::Str { .. }));
                (plain && !button.chip && !button.wrap).then(|| plain_text(&button.children))
            }))
            .when_some(button.title.clone(), |el, title| el.aria_description(title))
            // As the app's own header buttons show theirs: the shortcut beside what the tooltip says.
            .when_some(tip, |el, tip| {
                el.tooltip(ui::tooltip(tip, button.shortcut.as_ref().map(|key| key.label().into())))
            })
            .when_some(button.shortcut.as_ref(), |el, key| el.aria_keyshortcuts(key.aria()));
        let key_handler = button.on_key_down.clone();
        element.on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
            if let Some(handler) = &key_handler {
                send_shortcut(view, handler, event, window, cx);
            }
        }))
    }
}
