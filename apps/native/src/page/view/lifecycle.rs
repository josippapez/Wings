//! A page view's lifecycle: opening a plugin page, starting and restarting its runtime, and what it exposes of its state.
use super::*;

impl PageView {
    /// Whether a select's menu or a combobox's options are open over the page. They are drawn above everything else
    /// and can reach past the page's own bounds, so a press on one may land outside the frame that holds the page.
    pub fn shows_menu(&self) -> bool {
        self.menu.is_some() || self.listing.get()
    }

    /// The backdrop the page asks for behind it (`backdrop: "pixels"` or `"gradient:<preset>"` on its `Page`) and its
    /// palette, when a card holds it. The page leaves its ground to the card then, which draws the backdrop where the
    /// window has one (`ui::backdrop_slot`), else its own plain ground.
    pub fn backdrop(&self) -> Option<(tree::Backdrop, theme::Palette)> {
        let drawn = self.tree.as_ref().filter(|_| self.in_card && !self.popover)?;
        let kind = drawn.children.iter().find_map(|node| match node {
            Node::Page(page) => page.backdrop,
            _ => None,
        })?;
        Some((kind, drawn.palette.unwrap_or(theme::Palette::app())))
    }

    pub fn set_visible(&self, visible: bool) {
        self.shown.set(Some(visible));
        if let Some(runtime) = &self.runtime {
            runtime.set_visible(visible);
        }
    }

    /// Whether the page has stopped and not started again yet.
    #[cfg(feature = "test-support")]
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// Loads the page from `source`, its plugin's next run after an update or restart: it stays where it shows, and
    /// starts over with the new code.
    pub fn reload_from(&mut self, source: PageOpener, cx: &mut Context<Self>) {
        self.source = source;
        self.reload(cx);
    }

    /// Starts a stopped page again now, as the Reload by its error does.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self._restart = None;
        self.restart_wait = RESTART_FIRST;
        self.restart(cx);
    }

    /// Starts the page from its beginning in place of the runtime that stopped. What the page drew and what was open
    /// or typed in it go with the old one.
    pub(super) fn restart(&mut self, cx: &mut Context<Self>) {
        let (runtime, error, events) = Self::launch(&self.source, cx);
        if let (Some(runtime), Some(shown)) = (&runtime, self.shown.get()) {
            runtime.set_visible(shown);
        }
        if let (Some(runtime), Some(told)) = (&runtime, &self.terminals_told) {
            runtime.inbox().send(Message::Terminals, 0, told.clone());
        }
        self.stopped = runtime.is_none();
        (self.runtime, self.error, self._events) = (runtime, error, events);
        self.started = Instant::now();
        self.tree = None;
        self.open.clear();
        self.menu = None;
        self.pressed_open = false;
        self.select_bounds.clear();
        self.split_narrow.clear();
        self.split_inset.clear();
        self.split_sizes.clear();
        self.split_widths.clear();
        self.split_drag = None;
        self.focused = None;
        self.suggestions_closed = false;
        self.highlight = None;
        self.fields.borrow_mut().clear();
        self.inputs.borrow_mut().clear();
        self.graphs.borrow_mut().clear();
        self.sunbursts.borrow_mut().clear();
        self.prompt_rails.borrow_mut().clear();
        self.hover_cards.borrow_mut().clear();
        self.chip_focus.borrow_mut().clear();
        self.actions_focus.borrow_mut().clear();
        self.actions_within.clear();
        self.reveal = None;
        self.dialog_shown = None;
        self.focus_before = None;
        self.popup_shown = None;
        self.popup_before = None;
        *self.context_hit.borrow_mut() = None;
        self.pointed_chars.borrow_mut().clear();
        *self.selection.borrow_mut() = None;
        self.sheet_focus.borrow_mut().clear();
        self.sheets_shown.clear();
        self.sheet_return = None;
        if self.stopped {
            self.restart_later(cx);
        }
        cx.notify();
    }

    /// Starts the stopped page again after a wait, so it comes back without a click. Not once its plugin is gone.
    pub(super) fn restart_later(&mut self, cx: &mut Context<Self>) {
        if self.source.gone() {
            return;
        }
        let wait = if self.started.elapsed() >= RESTART_RESET { RESTART_FIRST } else { self.restart_wait };
        self.restart_wait = (wait * 2).min(RESTART_MOST);
        self._restart = Some(cx.spawn(async move |view, cx| {
            cx.background_executor().timer(wait).await;
            let _ = view.update(cx, |view, cx| view.restart(cx));
        }));
    }

    /// Opens `source` as a page of the plugin `host` runs, which can talk to its main and its other pages.
    pub fn in_plugin(host: &PluginHost, source: &str, cx: &mut Context<Self>) -> Self {
        Self::start(cx, host.page(false, 0, None, None, source))
    }

    /// What the page named its sidebar's header with `setTabTitle`, in place of the sidebar's title.
    pub fn header_title(&self) -> Option<String> {
        self.source.title()
    }

    /// The page, shown in a card with a 14px corner and a 1px border (`workspace::page_card`, the sidebar's cards).
    pub fn in_card(self) -> Self {
        Self {
            in_card: true,
            ..self
        }
    }

    /// Like `in_plugin`, for a page shown in the popover under its sidebar's title bar button.
    pub fn in_plugin_popover(host: &PluginHost, source: &str, cx: &mut Context<Self>) -> Self {
        Self {
            popover: true,
            ..Self::in_plugin(host, source, cx)
        }
    }

    /// Like `in_plugin`, for a tab `wings.openView` opened with `args`, which the page reads with `viewArgs`.
    pub fn in_plugin_with_args(
        host: &PluginHost,
        source: &str,
        args: Option<&serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::start(cx, host.page(false, 0, args, None, source))
    }

    /// Like `in_plugin_with_args`, for a tab or block whose `wings.tabState`/`blockState` is `state`.
    pub fn in_plugin_with_state(
        host: &PluginHost,
        source: &str,
        args: Option<&serde_json::Value>,
        state: crate::page::plugin::FrameState,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::start(cx, host.page(false, 0, args, Some(state), source))
    }

    pub fn in_plugin_window(
        host: &PluginHost,
        window_id: u64,
        source: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::start(cx, host.page(true, window_id, None, None, source))
    }

    /// Starts the page's runtime: the runtime, or why it didn't start, and the task that takes what it sends.
    pub(super) fn launch(source: &PageOpener, cx: &mut Context<Self>) -> (Option<PageRuntime>, Option<SharedString>, Task<()>) {
        let pending = Arc::new(Mutex::new(Pending::default()));
        let (wake, woken) = async_channel::bounded::<()>(1);
        let runtime = source.open(Box::new({
            let pending = pending.clone();
            move |event| {
                {
                    let mut pending = pending.lock().unwrap();
                    match event {
                        Event::Tree(tree) => pending.tree = Some(tree),
                        Event::Error { message, fatal } => {
                            pending.error = Some(message);
                            pending.stopped |= fatal;
                        }
                    }
                }
                let _ = wake.try_send(());
            }
        }));
        let (runtime, error) = match runtime {
            Ok(runtime) => (Some(runtime), None),
            Err(error) => {
                let message = format!("the page didn't start: {error}");
                super::super::log::line(source.plugin(), source.name(), &message);
                (None, Some(message.into()))
            }
        };
        let events = cx.spawn(async move |view, cx| {
            while woken.recv().await.is_ok() {
                let taken = std::mem::take(&mut *pending.lock().unwrap());
                if view
                    .update(cx, |view, cx| {
                        let mut echo = false;
                        if let Some(tree) = taken.tree {
                            // The page sent back only what was just typed, which its fields already show: nothing to draw.
                            // An answer to an earlier key, typed before the page caught up, is an echo too: drawing the
                            // page again for each one rebuilt a long chat on most keys of someone typing at speed.
                            echo = view.tree.as_ref().is_some_and(|old| {
                                old.echoes(&tree, |id, text| {
                                    view.inputs.borrow().get(&id).is_some_and(|state| {
                                        state.field.read(cx).value() == text || state.sent.iter().any(|sent| sent == text)
                                    })
                                })
                            });
                            if echo {
                                view.echoed(&tree.children);
                            }
                            // `WINGS_PAGE_DIFFS=1`: what changed in each tree that isn't an echo of what was typed.
                            #[cfg(feature = "test-support")]
                            if !echo
                                && std::env::var_os("WINGS_PAGE_DIFFS").is_some()
                                && let Some(old) = &view.tree
                            {
                                eprintln!("[tree {}] {}", view.trees + 1, old.differences(&tree).join("\n"));
                            }
                            let mut ids = HashSet::new();
                            fn controls(nodes: &[Node], ids: &mut HashSet<u64>) {
                                for node in nodes {
                                    match node {
                                        Node::SearchField(field) => {
                                            ids.insert(field.id);
                                        }
                                        Node::Input(input) => {
                                            ids.insert(input.id);
                                        }
                                        Node::Graph(graph) => {
                                            ids.insert(graph.id);
                                        }
                                        Node::Sunburst(map) => {
                                            ids.insert(map.id);
                                        }
                                        Node::PromptRail(rail) => {
                                            ids.insert(rail.id);
                                        }
                                        Node::HoverCard(card) => {
                                            ids.insert(card.id);
                                        }
                                        Node::Chips(chips) => {
                                            ids.insert(chips.id);
                                        }
                                        _ => {}
                                    }
                                    for part in node.parts() {
                                        controls(part, ids);
                                    }
                                }
                            }
                            controls(&tree.children, &mut ids);
                            view.fields.borrow_mut().retain(|id, _| ids.contains(id));
                            let gone: Vec<gpui::WeakFocusHandle> = view
                                .inputs
                                .borrow()
                                .iter()
                                .filter(|(id, _)| !ids.contains(id))
                                .map(|(_, state)| state.field.read(cx).focus_handle(cx).downgrade())
                                .collect();
                            let mut former = view.former_inputs.borrow_mut();
                            former.retain(|field| field.upgrade().is_some());
                            former.extend(gone);
                            drop(former);
                            view.inputs.borrow_mut().retain(|id, _| ids.contains(id));
                            view.graphs.borrow_mut().retain(|id, _| ids.contains(id));
                            view.sunbursts.borrow_mut().retain(|id, _| ids.contains(id));
                            view.prompt_rails.borrow_mut().retain(|id, _| ids.contains(id));
                            view.hover_cards.borrow_mut().retain(|id, _| ids.contains(id));
                            view.chip_focus
                                .borrow_mut()
                                .retain(|(id, _), _| ids.contains(id));
                            view.keep_selection_in(&tree.children);
                            view.tree = Some(tree);
                            view.trees += 1;
                            view.error = None;
                        }
                        if let Some(error) = taken.error {
                            if taken.stopped {
                                super::super::log::line(view.source.plugin(), view.source.name(), &error);
                            }
                            view.error = Some(error.into());
                        }
                        if taken.stopped {
                            view.stopped = true;
                            view.restart_later(cx);
                        }
                        if !echo || view.error.is_some() || taken.stopped {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        (runtime, error, events)
    }

    pub(super) fn start(cx: &mut Context<Self>, source: PageOpener) -> Self {
        let (runtime, error, events) = Self::launch(&source, cx);
        let mut view = Self {
            stopped: runtime.is_none(),
            runtime,
            tree: None,
            open: HashSet::new(),
            menu: None,
            pressed_open: false,
            select_bounds: HashMap::new(),
            split_narrow: HashMap::new(),
            split_inset: HashMap::new(),
            split_sizes: HashMap::new(),
            split_widths: HashMap::new(),
            split_drag: None,
            text_cut: HashMap::new(),
            text_open: HashSet::new(),
            focused: None,
            suggestions_closed: false,
            highlight: None,
            listing: Cell::new(false),
            fields: RefCell::new(HashMap::new()),
            inputs: RefCell::new(HashMap::new()),
            graphs: RefCell::new(HashMap::new()),
            sunbursts: RefCell::new(HashMap::new()),
            prompt_rails: RefCell::new(HashMap::new()),
            hover_cards: RefCell::default(),
            viewport: Cell::new((0.0, 0.0, 1.0)),
            dialog_area: Cell::new(None),
            images_waiting: RefCell::default(),
            chip_focus: RefCell::new(HashMap::new()),
            error,
            source,
            shown: Cell::new(None),
            started: Instant::now(),
            restart_wait: RESTART_FIRST,
            _restart: None,
            popover: false,
            in_card: false,
            shell_scroll: gpui::ScrollHandle::new(),
            combobox_scroll: gpui::ScrollHandle::new(),
            follow_scrolls: RefCell::default(),
            trees: 0,
            actions_focus: RefCell::default(),
            menu_focus: cx.focus_handle(),
            menu_recovery: Default::default(),
            menu_keys: Default::default(),
            popup_keys: Default::default(),
            actions_within: HashSet::new(),
            reveal: None,
            dialog_focus: cx.focus_handle(),
            dialog_shown: None,
            focus_before: None,
            context_hit: RefCell::default(),
            pointed_chars: RefCell::default(),
            selection: RefCell::default(),
            lines: RefCell::default(),
            selection_focus: cx.focus_handle(),
            popup_focus: cx.focus_handle(),
            popup_shown: None,
            popup_before: None,
            sheet_focus: RefCell::default(),
            sheets_drawn: RefCell::default(),
            sheets_shown: Vec::new(),
            sheet_return: None,
            host_focus: None,
            gesture_at: Cell::new(None),
            pressed_with: RefCell::new(None),
            former_inputs: RefCell::default(),
            terminals: Vec::new(),
            terminals_told: None,
            terminals_covered: Cell::new(false),
            spin_in_layer: Cell::new(false),
            hovered_card: None,
            drawing_card: Cell::new(None),
            found_links: RefCell::default(),
            #[cfg(feature = "test-support")]
            shot_terminals: None,
            #[cfg(feature = "test-support")]
            shot_layers: Vec::new(),
            #[cfg(feature = "test-support")]
            renders: Cell::new(0),
            #[cfg(feature = "test-support")]
            hidden_buttons: RefCell::default(),
            #[cfg(feature = "test-support")]
            bench_hidden: RefCell::default(),
            _events: events,
        };
        if view.stopped {
            view.restart_later(cx);
        }
        // The places its spinners left in the window's layer go with it.
        let id = cx.entity_id();
        cx.on_release(move |_, _| ui::forget_view(id)).detach();
        view
    }
}
