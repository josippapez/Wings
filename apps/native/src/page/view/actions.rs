//! What a page does with presses, shortcuts and picks, and the presses the tests and shots make.
use super::*;

impl PageView {
    /// The terminals the agent of this page's tab opened, by pane, which its `Terminal` nodes may draw, and the JSON list
    /// the page hears them as (`terminals`). Nothing is sent when it is what the page heard last.
    pub fn set_terminals(&mut self, panes: Vec<usize>, json: String, cx: &mut Context<Self>) {
        if self.terminals_told.as_deref() == Some(json.as_str()) {
            return;
        }
        if let Some(runtime) = &self.runtime {
            runtime.inbox().send(Message::Terminals, 0, json.clone());
        }
        self.terminals = panes;
        self.terminals_told = Some(json);
        cx.notify();
    }

    /// Draws the page's terminals over it with `draw`, as the workspace does, for the screenshots of a page alone.
    #[cfg(feature = "test-support")]
    pub fn draw_terminals_for_shots(
        &mut self,
        draw: impl Fn(&mut gpui::App) -> Vec<(SharedString, AnyElement)> + 'static,
    ) {
        self.shot_terminals = Some(Rc::new(draw));
    }

    /// Puts the page on screen as the workspace does and draws the layers of what moves in it over it, for the screenshots
    /// of a page alone: a hover card's cue shows only then.
    #[cfg(feature = "test-support")]
    pub fn draw_layers_for_shots(&mut self, cx: &mut Context<Self>) {
        use ui::live::Kind;
        // A view of its own for each, as the workspace's are, so a step of one draws it and not the page.
        self.shot_layers = [Kind::ViewSpinners, Kind::Pulses, Kind::Counts, Kind::Progress]
            .map(|kind| cx.new(|_| ui::live::Layer::new(kind)))
            .to_vec();
    }

    /// Where a `Terminal` shows: the workspace draws the terminal over this place, in the `TERMINALS` layer, so what it
    /// prints never draws the page again. One the tab wasn't handed draws a line saying so, and one under the page's own
    /// dialog or sheet is left out, as it would draw over them.
    pub(super) fn terminal(&self, terminal: &tree::Terminal) -> AnyElement {
        let Some(pane) = embeddable(&self.terminals, &terminal.pane) else {
            return div()
                .text_size(px(12.0))
                .text_color(rgb(theme::palette().muted_foreground))
                .child("Only a terminal this tab's agent opened shows here.")
                .into_any_element();
        };
        let place = match terminal.fill {
            true => div().w_full().flex_1().min_h_0(),
            false => div().w_full().flex_none().h(px(terminal.height)),
        };
        if self.terminals_covered.get() {
            return place.into_any_element();
        }
        place.child(ui::slot(TERMINALS, terminal_slot(pane), None)).into_any_element()
    }

    /// Where a `WebView` shows: the system's web engine draws the page over this place (`crate::web`), so nothing the web
    /// page does draws this page again. A plugin whose manifest doesn't ask for `webView` gets a line saying so.
    pub(super) fn web_view(&self, web: &tree::WebView, cx: &mut Context<Self>) -> AnyElement {
        if self.web_profile.is_none() || web.name.is_empty() {
            return div()
                .text_size(px(12.0))
                .text_color(rgb(theme::palette().muted_foreground))
                .child("Web pages show here only for a plugin that asks to show them, with a name for each.")
                .into_any_element();
        }
        let place = match web.fill {
            true => div().w_full().flex_1().min_h_0(),
            false => div().w_full().flex_none().h(px(web.height)),
        };
        place.child(crate::web::slot(cx.entity_id(), web.name.clone().into())).into_any_element()
    }

    /// The page sent a new tree: its web views are the `WebView`s in it, and each hears what its node's `onState` is.
    pub(super) fn sync_web(&mut self, nodes: &[Node], cx: &mut Context<Self>) {
        let Some(profile) = self.web_profile.clone() else { return };
        fn webs<'a>(nodes: &'a [Node], into: &mut Vec<&'a tree::WebView>) {
            for node in nodes {
                if let Node::WebView(web) = node {
                    into.push(web);
                }
                for part in node.parts() {
                    webs(part, into);
                }
            }
        }
        let mut found = Vec::new();
        webs(nodes, &mut found);
        found.retain(|web| !web.name.is_empty());
        if found.is_empty() && self.web_told.is_none() {
            return;
        }
        self.web_handlers = found
            .iter()
            .filter_map(|web| Some((SharedString::from(web.name.clone()), web.on_state.clone()?)))
            .collect();
        let told = self.web_told.get_or_insert_with(|| {
            let (told, hear) = async_channel::unbounded::<(SharedString, crate::web::State)>();
            self._web_events = Some(cx.spawn(async move |view, cx| {
                while let Ok((name, state)) = hear.recv().await {
                    let payload = serde_json::to_value(&state).unwrap_or_default();
                    let sent = view.update(cx, |view, _| {
                        if let Some(handler) = view.web_handlers.get(&name) {
                            view.send(handler, Some(&payload));
                        }
                    });
                    if sent.is_err() {
                        break;
                    }
                }
            }));
            told
        });
        let specs: Vec<crate::web::Spec> = found
            .iter()
            .map(|web| crate::web::Spec {
                name: web.name.clone().into(),
                url: web.url.clone(),
                keep_data: web.keep_data,
                command: web.command.clone(),
            })
            .collect();
        crate::web::update(cx.entity_id(), &profile, &specs, told);
    }

    /// The links in the text of `id`, found once for each text and set of links rather than each time the page draws.
    pub(super) fn found_links(&self, id: u64, text: &str, rules: &[wings_plugins::links::Rule], generation: u64) -> Rc<Vec<wings_plugins::links::Found>> {
        let mut cache = self.found_links.borrow_mut();
        // A long chat's texts come and go: a bound on what is kept, past which it starts over.
        if cache.generation != generation || cache.by_text.len() > 4096 {
            cache.generation = generation;
            cache.by_text.clear();
        }
        if let Some((seen, found)) = cache.by_text.get(&id)
            && seen.as_str() == text
        {
            return found.clone();
        }
        let found = Rc::new(wings_plugins::links::find(text, rules));
        cache.by_text.insert(id, (text.to_owned(), found.clone()));
        found
    }

    pub(super) fn send(&self, handler: &str, payload: Option<&serde_json::Value>) {
        if let Some(runtime) = &self.runtime {
            runtime.dispatch(handler, payload);
        }
    }

    /// `send` for a click, a key press or a pick, which lets the page copy right after.
    pub(super) fn act(&self, handler: &str, payload: Option<&serde_json::Value>) {
        self.gesture_at.set(Some(Instant::now()));
        if let Some(runtime) = &self.runtime {
            runtime.dispatch_gesture(handler, payload);
        }
    }

    /// Clicks the button whose `shortcut` is `keystroke`, and says whether one had it. While a dialog is open only its
    /// buttons count, as only they can be clicked.
    pub fn press_shortcut(&self, keystroke: &gpui::Keystroke) -> bool {
        let mods = keystroke.modifiers;
        let Some(tree) = self.tree.as_ref().filter(|_| mods.platform || mods.control) else {
            return false;
        };
        let nodes = dialog_in(&tree.children).map_or(&tree.children[..], |dialog| &dialog.children[..]);
        match shortcut_in(nodes, keystroke) {
            Some(handler) => {
                self.act(&handler, None);
                true
            }
            None => false,
        }
    }

    /// Sends the page's `onTextScale` the `step` (`bigger`, `smaller` or `reset`), and says whether it has one.
    pub fn scale_text(&self, step: &str) -> bool {
        let Some(handler) = self.tree.as_ref().and_then(|tree| text_scale_in(&tree.children)) else {
            return false;
        };
        self.act(&handler, Some(&serde_json::json!({ "step": step })));
        true
    }

    /// Whether a click or a key of yours has reached the page.
    #[cfg(test)]
    pub(in super::super) fn acted(&self) -> bool {
        self.gesture_at.get().is_some()
    }

    /// The tab that shows this page: while it has the keyboard, as it does once you open the tab, the page may move it
    /// into a field of its own.
    pub fn set_host_focus(&mut self, focus: gpui::FocusHandle) {
        self.host_focus = Some(focus);
    }

    /// Whether the page may move the keyboard into itself, as a dialog, a sheet or a field with `autoFocus` does: when
    /// the keyboard is in the page or on its tab already, nothing has it, or you just clicked or typed in the page. A
    /// page never takes it from a terminal or another page you are typing in.
    pub(in super::super) fn may_take_keyboard(&self, window: &gpui::Window, cx: &gpui::App) -> bool {
        let focused = window.focused(cx);
        // Right after you used the page, while the keyboard is where it was as you pressed in it: a click in a terminal
        // since then takes it back.
        let recent = self.gesture_at.get().is_some_and(|at| at.elapsed() < GESTURE_TAKES_KEYBOARD);
        if recent && self.pressed_with.borrow().as_ref().is_some_and(|before| *before == focused) {
            return true;
        }
        let Some(focused) = focused else {
            return true;
        };
        // What can hold the keyboard in the page: its fields, menus, chips, action rows, dialog and sheets. Compared by
        // handle, as a window that hasn't drawn yet can't say what holds what.
        self.host_focus.as_ref() == Some(&focused)
            || focused == self.dialog_focus
            || focused == self.popup_focus
            || focused == self.menu_focus
            || self.inputs.borrow().values().any(|state| state.field.read(cx).focus_handle(cx) == focused)
            || self.former_inputs.borrow().iter().any(|field| *field == focused)
            || self.sheet_focus.borrow().values().any(|handle| *handle == focused)
            || self.chip_focus.borrow().values().any(|handle| *handle == focused)
            || self.actions_focus.borrow().values().any(|handle| *handle == focused)
    }

    pub(super) fn toggle_disclosure(&mut self, id: u64, cx: &mut Context<Self>) {
        if !self.open.remove(&id) {
            self.open.insert(id);
        }
        cx.notify();
    }

    pub(super) fn toggle_menu(&mut self, id: u64, cx: &mut Context<Self>) {
        let was_open = std::mem::take(&mut self.pressed_open) || self.menu == Some(id);
        self.menu = (!was_open).then_some(id);
        // A row of the menu that opened its levels starts shut.
        self.open.remove(&id);
        cx.notify();
    }

    /// The person picked an open `Menu`'s item, whose click is `handler`, by a click or Enter: the menu closes, the
    /// keyboard goes back to its button, and the page hears the click.
    pub(super) fn pick_menu_item(&mut self, handler: &str, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.menu_recovery.restore(&self.menu_focus, window, cx);
        self.act(handler, None);
        cx.notify();
    }

    /// The person picked `value`: the page hears it as the select's `onChange` does, with `{"value": ...}`.
    pub(super) fn pick(&mut self, on_change: Option<&str>, value: &str, cx: &mut Context<Self>) {
        self.menu = None;
        if let Some(handler) = on_change {
            self.act(handler, Some(&serde_json::json!({ "value": value })));
        }
        cx.notify();
    }

    /// The person picked a combobox's option, whose chip's click is `handler`: the page hears that click, and the list
    /// goes away until the field changes again.
    pub(super) fn pick_suggestion(&mut self, handler: &str, cx: &mut Context<Self>) {
        self.suggestions_closed = true;
        self.highlight = None;
        self.act(handler, None);
        cx.notify();
    }

    /// Presses the `nth` button labelled `label`, by its text or an icon button's name, as a click would. False when
    /// the page has no such button.
    #[cfg(feature = "test-support")]
    pub fn press(&self, label: &str, nth: usize) -> bool {
        fn find<'a>(nodes: &'a [Node], label: &str, found: &mut Vec<&'a str>) {
            for node in nodes {
                if let Node::Button(button) = node
                    && (plain_text(&button.children).trim() == label
                        || button.label.as_deref() == Some(label))
                    && let Some(handler) = &button.on_click
                {
                    found.push(handler);
                }
                for part in node.parts() {
                    find(part, label, found);
                }
            }
        }
        let mut found = Vec::new();
        if let Some(tree) = &self.tree {
            find(&tree.children, label, &mut found);
        }
        found.get(nth).is_some_and(|handler| {
            self.act(handler, None);
            true
        })
    }

    /// Clicks the first `Image` whose label starts with `label`. False when the page has none that takes a click.
    #[cfg(feature = "test-support")]
    pub fn press_image(&self, label: &str) -> bool {
        fn find<'a>(nodes: &'a [Node], label: &str) -> Option<&'a str> {
            nodes.iter().find_map(|node| match node {
                Node::Image(picture) if picture.label.as_deref().is_some_and(|name| name.starts_with(label)) => picture.on_click.as_deref(),
                node => node.parts().into_iter().find_map(|part| find(part, label)),
            })
        }
        let handler = self.tree.as_ref().and_then(|tree| find(&tree.children, label));
        handler.is_some_and(|handler| {
            self.act(handler, None);
            true
        })
    }

    /// Opens or closes the first disclosure whose summary starts with `summary`, as a click on its line would. False when
    /// the page has none.
    /// Presses the More of the first expandable text that is cut short, as a click on it would. False when none is.
    #[cfg(feature = "test-support")]
    pub fn press_more(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.text_cut.iter().filter(|(_, cut)| **cut).map(|(id, _)| *id).min() else { return false };
        self.text_open.insert(id);
        cx.notify();
        true
    }

    #[cfg(feature = "test-support")]
    pub fn press_disclosure(&mut self, summary: &str, cx: &mut Context<Self>) -> bool {
        fn find<'a>(nodes: &'a [Node], summary: &str) -> Option<&'a tree::Disclosure> {
            nodes.iter().find_map(|node| match node {
                Node::Disclosure(disclosure) if plain_text(&disclosure.summary).trim().starts_with(summary) => Some(disclosure),
                node => node.parts().into_iter().find_map(|part| find(part, summary)),
            })
        }
        let Some(disclosure) = self.tree.as_ref().and_then(|tree| find(&tree.children, summary)).cloned() else {
            return false;
        };
        let open = disclosure.open.unwrap_or_else(|| self.open.contains(&disclosure.id));
        match &disclosure.on_open_change {
            Some(handler) => self.act(handler, Some(&serde_json::json!({ "open": !open }))),
            None => self.toggle_disclosure(disclosure.id, cx),
        }
        true
    }

    /// Opens the `nth` menu whose button is labelled `label`, by its text or its name, as a click on it would. False
    /// when the page has no such menu.
    #[cfg(feature = "test-support")]
    pub fn open_menu(&mut self, label: &str, nth: usize, cx: &mut Context<Self>) -> bool {
        fn find(nodes: &[Node], label: &str, found: &mut Vec<u64>) {
            for node in nodes {
                if let Node::Menu(menu) = node
                    && let Some(Node::Button(button)) = menu.trigger.first()
                    && (plain_text(&button.children).trim() == label
                        || button.label.as_deref() == Some(label))
                {
                    found.push(menu.id);
                }
                for part in node.parts() {
                    find(part, label, found);
                }
            }
        }
        let mut found = Vec::new();
        if let Some(tree) = &self.tree {
            find(&tree.children, label, &mut found);
        }
        let Some(id) = found.get(nth).copied() else {
            return false;
        };
        self.menu = Some(id);
        cx.notify();
        true
    }

    /// Presses the `nth` row that opens (`aria-expanded`) and shows `text`, as a click would. False when the page has none.
    #[cfg(feature = "test-support")]
    pub fn press_row(&self, text: &str, nth: usize) -> bool {
        fn find<'a>(nodes: &'a [Node], text: &str, found: &mut Vec<&'a str>) {
            for node in nodes {
                if let Node::ListButton(row) = node
                    && row.expanded.is_some()
                    && plain_text(&row.button.children).contains(text)
                    && let Some(handler) = &row.button.on_click
                {
                    found.push(handler);
                }
                for part in node.parts() {
                    find(part, text, found);
                }
            }
        }
        let mut found = Vec::new();
        if let Some(tree) = &self.tree {
            find(&tree.children, text, &mut found);
        }
        found.get(nth).is_some_and(|handler| {
            self.act(handler, None);
            true
        })
    }

    /// Clicks the tick of prompt `index` on the first prompt rail, as a click there would. False when the page has none.
    #[cfg(feature = "test-support")]
    pub fn press_rail(&self, index: usize) -> bool {
        fn find(nodes: &[Node]) -> Option<&str> {
            nodes.iter().find_map(|node| match node {
                Node::PromptRail(rail) => rail.on_select.as_deref(),
                other => other.parts().into_iter().find_map(find),
            })
        }
        let handler = self.tree.as_ref().and_then(|tree| find(&tree.children));
        handler.is_some_and(|handler| {
            self.act(handler, Some(&serde_json::json!({ "index": index })));
            true
        })
    }

    /// Presses the first `ListButton` that shows `text`, as a click would. False when the page has none.
    #[cfg(feature = "test-support")]
    pub fn press_list_button(&self, text: &str) -> bool {
        fn find<'a>(nodes: &'a [Node], text: &str) -> Option<&'a str> {
            nodes.iter().find_map(|node| match node {
                Node::ListButton(row) if plain_text(&row.button.children).contains(text) => {
                    row.button.on_click.as_deref()
                }
                other => other.parts().into_iter().find_map(|part| find(part, text)),
            })
        }
        let handler = self
            .tree
            .as_ref()
            .and_then(|tree| find(&tree.children, text));
        handler.is_some_and(|handler| {
            self.act(handler, None);
            true
        })
    }

    /// Picks the chip of `Chips` that shows `label`, as a click would. False when the page has no such chip.
    #[cfg(feature = "test-support")]
    pub fn press_chip(&self, label: &str) -> bool {
        fn find(nodes: &[Node], label: &str) -> Option<(String, String)> {
            nodes.iter().find_map(|node| match node {
                Node::Chips(chips) => chips
                    .options
                    .iter()
                    .find(|option| option.label.starts_with(label))
                    .zip(chips.on_change.clone())
                    .map(|(option, handler)| (handler, option.value.clone())),
                other => other.parts().into_iter().find_map(|part| find(part, label)),
            })
        }
        let found = self
            .tree
            .as_ref()
            .and_then(|tree| find(&tree.children, label));
        found.is_some_and(|(handler, value)| {
            self.act(&handler, Some(&serde_json::json!({ "value": value })));
            true
        })
    }

    /// Turns on the `Chip` or the `Checkbox` that shows `label`, as a click on it would, unless it's on already, or presses
    /// the icon `Button` named `label`. False when the page has no such chip, tick or button.
    #[cfg(feature = "test-support")]
    pub fn turn_on_chip(&self, label: &str) -> bool {
        /// Whether it is on, and what a click sends: nothing for a chip, the new state for a tick.
        fn find(nodes: &[Node], label: &str) -> Option<(bool, Option<String>, Option<serde_json::Value>)> {
            nodes.iter().find_map(|node| match node {
                Node::Button(button)
                    if button.chip && plain_text(&button.children).starts_with(label) =>
                {
                    Some((button.pressed, button.on_click.clone(), None))
                }
                Node::Checkbox(tick) if plain_text(&tick.children).starts_with(label) => {
                    Some((tick.checked, tick.on_change.clone(), Some(serde_json::json!({ "checked": true }))))
                }
                // An icon button, by the name it is read out by, like a header's New issue.
                Node::Button(button) if !button.chip && button.label.as_deref() == Some(label) => {
                    Some((false, button.on_click.clone(), None))
                }
                other => other.parts().into_iter().find_map(|part| find(part, label)),
            })
        }
        let found = self
            .tree
            .as_ref()
            .and_then(|tree| find(&tree.children, label));
        found.is_some_and(|(on, handler, payload)| {
            if let (false, Some(handler)) = (on, handler) {
                self.act(&handler, payload.as_ref());
            }
            true
        })
    }

    /// The id of the select named `label`, or showing it, while the page has one.
    #[cfg(feature = "test-support")]
    pub fn select_named(&self, label: &str) -> Option<u64> {
        fn find(nodes: &[Node], label: &str) -> Option<u64> {
            nodes.iter().find_map(|node| match node {
                Node::Select(select) if select.label == label || select.shown() == label => {
                    Some(select.id)
                }
                other => other.parts().into_iter().find_map(|part| find(part, label)),
            })
        }
        self.tree.as_ref().and_then(|tree| find(&tree.children, label))
    }

    /// Opens the menu of the select named `label`, or showing it, as a click on it would. False when the page has no
    /// such select.
    #[cfg(feature = "test-support")]
    pub fn open_select(&mut self, label: &str, cx: &mut Context<Self>) -> bool {
        let found = self.select_named(label);
        if let Some(id) = found {
            self.menu = Some(id);
            cx.notify();
        }
        found.is_some()
    }

    /// Tab and Shift-Tab go to the page's next and previous control, as in a browser, and round to its first and last
    /// instead of leaving it: a popover's page shares the window with the panes behind it.
    pub(super) fn tab(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let mods = event.keystroke.modifiers;
        if event.keystroke.key != "tab" || mods.number_of_modifiers() != u8::from(mods.shift) {
            return;
        }
        let page = cx.entity_id().to_string();
        let dialog = self.dialog_shown.is_some().then(|| self.dialog_focus.clone());
        let inside = |window: &Window, cx: &gpui::App| match &dialog {
            // A dialog keeps the keyboard in it, as a modal does.
            Some(dialog) => dialog.contains_focused(window, cx),
            None => window
                .context_stack()
                .iter()
                .any(|context| context.get(PAGE_CONTEXT).is_some_and(|id| **id == page)),
        };
        crate::ui::behaviors::cycle_until(inside, mods.shift, window, cx);
        window.prevent_default();
        cx.stop_propagation();
    }
}
