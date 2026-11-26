//! A `Text`, as a line, wrapped or expandable.
use super::*;

impl PageView {
    /// A `Text`. In a baseline row it has a `drop`: its first line starts that many points down, onto the row's
    /// baseline, and it narrows to its longest word and wraps when the row is short of room, as a CSS flex item does
    /// even when it doesn't grow. GPUI takes a text's narrowest as its whole line, so an issue's long title ran over the
    /// key and the Edit after it.
    pub(super) fn text_element(
        &self,
        text: &tree::Text,
        drop: Option<f32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if text.expandable && text.truncate {
            return self.expandable_text(text, drop, cx);
        }
        // CSS breaks a line between words, and lets a word wider than its box run over it; GPUI breaks inside a
        // word when it has to. A text that grows in a row, squeezed by the buttons beside it, would come apart a
        // letter or two a line, so it is never narrower than its longest word, as in the desktop. A mono text that
        // grows is a line of code or of a log, which the desktop sets in a box that scrolls sideways and the app
        // can't scroll: it breaks inside a word wider than its box instead, or a log's "=====" rule and a long link
        // ran past the card's edge.
        let code = text.mono && text.grow;
        // A line that pulses or counts the seconds moves in the window's layer, so its steps never draw the page.
        let in_layer = (text.shimmer || text.since.is_some()) && self.spin_in_layer.get();
        let page = cx.entity_id();
        let least = ((text.grow || drop.is_some()) && !text.truncate && !code).then(|| {
            let size = match text.size {
                TextSize::Small => scaled(12.0),
                TextSize::Tiny => scaled(11.0),
                TextSize::Large => scaled(20.0),
                TextSize::Field => scaled(14.0),
                TextSize::Normal => scaled(13.0),
            };
            let weight = match text.weight {
                Weight::Normal => FontWeight::NORMAL,
                Weight::Medium => FontWeight::MEDIUM,
                Weight::Strong => FontWeight::SEMIBOLD,
            };
            longest_word(&plain_text(&text.children), size, weight, text.mono, cx)
        });
        let element = div()
            .when_some(drop.filter(|drop| *drop > 0.0), |el, drop| el.pt(px(drop)))
            .when(text.truncate || code, |el| el.min_w_0())
            .when_some(least, |el, least| el.min_w(px(least)))
            .when(text.grow, |el| el.flex_1())
            .when(text.truncate, |el| el.truncate())
            // Shrinking is shared by width, so a long folder before a long file name cut both: the name hardly shrinks.
            .when(text.shrink_last, |mut el| {
                el.style().flex_shrink = Some(0.001);
                el
            })
            .when(text.clamp, three_lines)
            .when(text.mono, |el| el.font_family(DEFAULT_FONT_FAMILY))
            .map(|el| match text.size {
                TextSize::Normal => el,
                TextSize::Small => el.text_size(px(scaled(12.0))),
                TextSize::Tiny => el.text_size(px(scaled(11.0))),
                TextSize::Large => el.text_size(px(scaled(20.0))).line_height(px(scaled(26.0))),
                TextSize::Field => el.text_size(px(scaled(14.0))).line_height(relative(1.5)),
            })
            .map(|el| match text.weight {
                Weight::Normal => el,
                Weight::Medium => el.font_weight(FontWeight::MEDIUM),
                Weight::Strong => el.font_weight(FontWeight::SEMIBOLD),
            })
            .when_some(text.tone, |el, tone| el.text_color(rgb(tone_color(tone))))
            // A text with nothing in it takes no line, as an empty span takes none in CSS: PR tracker's "A
            // token is saved." left a blank line over the field while it was still asking.
            .when(
                text.children
                    .iter()
                    .any(|node| !matches!(node, Node::Str { text, .. } if text.is_empty())),
                |el| {
                    let content = match text.since {
                        Some(since) => {
                            let took = Node::Str { id: text.id, text: format!(" · {}", took(since, now_ms())) };
                            self.inline(&[text.children.clone(), vec![took]].concat(), cx)
                        }
                        None if text.links || LINKED.with(Cell::get) => self
                            .linked_text(&text.children, text.id, cx)
                            .unwrap_or_else(|| self.inline_of(&text.children, true, cx)),
                        None => self.inline_of(&text.children, true, cx),
                    };
                    if !in_layer {
                        return el.child(content);
                    }
                    // The window's layer draws the words over their own, which keep their room, selection and screen
                    // reader in no colour, and pulses them and counts their `since` up without drawing the page again.
                    let words = plain_text(&text.children);
                    let since = text.since;
                    let said = move || -> SharedString {
                        match since {
                            Some(since) => format!("{words} · {}", took(since, now_ms())).into(),
                            None => words.clone().into(),
                        }
                    };
                    let name = format!("shimmer-{page}-{}", text.id);
                    el.child(
                        div()
                            .relative()
                            .child(div().text_color(gpui::transparent_black()).child(content))
                            .map(|el| match text.shimmer {
                                true => el.child(ui::shimmer_slot(name, since.is_some(), said)),
                                false => el.child(ui::count_slot(name, said)),
                            }),
                    )
                },
            );
        // A shimmer pulses on the spinners' clock, 10 frames a second while the window is in front: in the window's
        // layer for it (`ui::view_spinner_layer`), or in the page, each of whose frames then draws the page again, which
        // counts a `since` up too. Without a shimmer, a second's tick does that.
        let element = match (text.shimmer, text.since) {
            _ if in_layer => element.into_any_element(),
            (true, since) => ui::shimmer(element, since.is_some()).into_any_element(),
            (false, Some(_)) => element
                .with_animation(
                    ElementId::NamedInteger("page-since".into(), text.id),
                    Animation::new(std::time::Duration::from_secs(1)).repeat().with_max_fps(1.0),
                    |el, _| el,
                )
                .into_any_element(),
            (false, None) => element.into_any_element(),
        };
        if text.on_click.is_none() && text.title.is_none() {
            return element;
        }
        // The box round it narrows and grows as the text would, or a title cut short with a tooltip ran its whole
        // length under the buttons beside it, as Claude chat's header title did in a narrow tab.
        let element = div()
            .when(text.truncate || code, |el| el.min_w_0())
            .when(text.grow, |el| el.flex_1())
            // The box is what shares the row's width, so it keeps its own as the text does: with a tooltip, the name still shrank first.
            .when(text.shrink_last, |mut el| {
                el.style().flex_shrink = Some(0.001);
                el
            })
            .child(element);
        // A tooltip needs an id to hang on.
        let element = element
            .id(ElementId::NamedInteger("page-text".into(), text.id))
            .when_some(text.title.clone(), |el, title| {
                el.aria_description(title.clone())
                    .tooltip(ui::tooltip(title, None))
            });
        match text.on_click.clone() {
            Some(handler) => element
                .cursor_pointer()
                .text_color(rgb(text.tone.map_or(theme::palette().info, tone_color)))
                .on_click(cx.listener(move |view, _, _, _| view.act(&handler, None)))
                .into_any_element(),
            None => element.into_any_element(),
        }
    }

    /// A text cut short with More after it while it doesn't fit, and Less once opened: the queued messages over Claude
    /// chat's message box guessed from their length and offered More for a line that fit. The text's whole line is
    /// measured against the width it is drawn at, and a new width that cuts it, or stops cutting it, draws again.
    pub(super) fn expandable_text(&self, text: &tree::Text, drop: Option<f32>, cx: &mut Context<Self>) -> AnyElement {
        let id = text.id;
        let open = self.text_open.contains(&id);
        let said = plain_text(&text.children);
        // GPUI breaks a line at every newline even in a text that doesn't wrap, so a cut text of many lines showed them all: a
        // pasted log queued over Claude chat's message box ran down the page. Cut short, it is its first line.
        let first = said.trim_end().contains('\n').then(|| said.lines().next().unwrap_or_default().to_owned());
        let children = match (&first, open) {
            (Some(first), false) => vec![Node::Str { id: text.id, text: first.clone() }],
            _ => text.children.clone(),
        };
        let shown = tree::Text { truncate: !open, expandable: false, grow: false, children, ..text.clone() };
        let (size, weight) = text_font(text);
        let whole = line_width(&said, size, weight, text.mono, cx);
        let lines = said.trim_end().contains('\n');
        let entity = cx.entity();
        let measure = canvas(
            move |bounds, _, cx| {
                let cut = lines || whole > bounds.size.width.as_f32() + 0.5;
                entity.update(cx, |view, cx| {
                    if view.text_cut.insert(id, cut) != Some(cut) {
                        cx.notify();
                    }
                })
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let cut = self.text_cut.get(&id).copied().unwrap_or(false);
        let label = if open { "Less" } else { "More" };
        let toggle = sized(
            ui::button(ElementId::NamedInteger("page-text-more".into(), id), label, ui::Variant::Ghost, false, label),
            tree::ButtonSize::Sm,
        )
        .on_click(cx.listener(move |view, _, _, cx| {
            cx.stop_propagation();
            if !view.text_open.remove(&id) {
                view.text_open.insert(id);
            }
            cx.notify();
        }));
        // The first line is centred in a button's height, as the toggle is and as the buttons beside it in a row are, so a row
        // that starts its items at the top has Less, Send now and the cross all on the message's first line.
        let line = match text.size {
            TextSize::Normal => scaled(13.0 * 1.45),
            TextSize::Small => scaled(12.0 * 1.45),
            TextSize::Tiny => scaled(11.0 * 1.45),
            TextSize::Large => scaled(26.0),
            TextSize::Field => scaled(14.0 * 1.5),
        };
        let top = ((SM_BUTTON_HEIGHT - line) / 2.0).max(0.0);
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_start()
            .gap(px(4.0))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .pt(px(top))
                    // Opened, a long message scrolls in a box of its own, so it never pushes the page off the screen.
                    .child(match open {
                        true => div()
                            .id(ElementId::NamedInteger("page-text-open".into(), id))
                            .max_h(px(OPEN_TEXT_HEIGHT))
                            .overflow_y_scroll()
                            .child(self.text_element(&shown, drop, cx))
                            .into_any_element(),
                        false => self.text_element(&shown, drop, cx),
                    })
                    .child(measure),
            )
            .when(cut || open, |el| {
                el.child(div().flex_none().h(px(SM_BUTTON_HEIGHT)).flex().items_center().child(toggle))
            })
            .into_any_element()
    }
}
