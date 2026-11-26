//! A viewer's start and its layout choices: wrap, tree, split, dock and the file list.
use super::*;

impl Viewer {
    pub fn new(view: View, host: Arc<PluginHost>, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| {
            // Unframed: the row around it draws the frame and the search icon, and clips it.
            TextField::new(
                Role::SearchInput,
                "Filter files",
                "Filter files",
                px(400.0),
                cx,
            )
        });
        let filter_events = cx.subscribe(&filter, |this, _, event: &FieldEvent, cx| {
            if matches!(event, FieldEvent::Changed) {
                this.relist(cx);
                cx.notify();
            }
        });
        let viewer = cx.entity().downgrade();
        let opener: Opener = Rc::new(move |url, _, cx| {
            let _ = viewer.update(cx, |this, cx| this.open_link(url.to_owned(), cx));
        });
        let stack = stacked::Stack::new(view.update.focus.is_some());
        let mut this = Self {
            view,
            host,
            selected: 0,
            rows: Vec::new(),
            split_choice: None,
            docked: false,
            can_dock: false,
            back: false,
            dock_files: true,
            wrap: false,
            tree: false,
            collapsed: HashSet::new(),
            list_rows: Vec::new(),
            filter,
            _filter_events: filter_events,
            wrapped: ListState::new(0, ListAlignment::Top, px(200.0)),
            split_x: px(0.0),
            widest: 0,
            counts: Vec::new(),
            colors: HashMap::new(),
            coloring: None,
            code_width: 0.0,
            scroll: UniformListScrollHandle::new(),
            files_scroll: UniformListScrollHandle::new(),
            horizontal: gpui::ScrollHandle::new(),
            focus: cx.focus_handle(),
            files_focus: cx.focus_handle().tab_stop(true),
            files_typeahead: Default::default(),
            code_focus: cx.focus_handle().tab_stop(true),
            refresh_focus: cx.focus_handle().tab_stop(true),
            close_focus: cx.focus_handle().tab_stop(true),
            refreshing: false,
            refresh_error: None,
            refresh_task: None,
            timeout: None,
            threads: Vec::new(),
            discussion: Vec::new(),
            discussion_open: false,
            discussion_list: ListState::new(0, ListAlignment::Top, px(400.0)),
            file_comments: Vec::new(),
            notes: Vec::new(),
            replies: HashMap::new(),
            compose: None,
            drafts: Vec::new(),
            next_draft: -1,
            draft_errors: HashMap::new(),
            #[cfg(feature = "test-support")]
            sent_comments: Vec::new(),
            bodies: RefCell::default(),
            opener,
            link_error: None,
            stack,
            editor: None,
            #[cfg(feature = "test-support")]
            rendered_rows: std::cell::Cell::new(0),
        };
        this.rebuild(cx);
        this
    }

    /// Wrap and the folder tree as last chosen.
    pub fn set_layout(&mut self, wrap: bool, tree: bool, cx: &mut Context<Self>) {
        self.wrap = wrap;
        self.tree = tree;
        self.rebuild(cx);
    }

    pub fn set_split(&mut self, split: bool, cx: &mut Context<Self>) {
        self.split_choice = Some(split);
        self.rebuild(cx);
        cx.notify();
    }

    /// Side by side until you pick, except docked: a dock is usually too narrow for two columns, as the desktop's
    /// `splitChoice ?? !dock`.
    pub fn split(&self) -> bool {
        self.split_choice.unwrap_or(!self.docked)
    }

    /// Beside its tab (`docked`), or filling the window, where `can_dock` offers to go back beside the tab.
    pub fn set_dock(&mut self, docked: bool, can_dock: bool, cx: &mut Context<Self>) {
        let split = self.split();
        self.docked = docked;
        self.can_dock = can_dock;
        if self.split() != split {
            self.rebuild(cx);
        }
        cx.notify();
    }

    /// Shown in a sidebar's place, with a Back button.
    pub fn set_back(&mut self, back: bool, cx: &mut Context<Self>) {
        self.back = back;
        cx.notify();
    }

    /// Whether the docked file list shows, as last chosen.
    pub fn set_dock_files(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.dock_files = shown;
        cx.notify();
    }

    pub(super) fn choose_dock_files(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.set_dock_files(shown, cx);
        cx.emit(Event::DockFiles(shown));
    }

    pub(super) fn set_wrap(&mut self, wrap: bool, cx: &mut Context<Self>) {
        self.wrap = wrap;
        self.rebuild(cx);
        cx.emit(Event::Layout {
            wrap,
            tree: self.tree,
        });
        cx.notify();
    }

    pub(super) fn set_tree(&mut self, tree: bool, cx: &mut Context<Self>) {
        self.tree = tree;
        self.relist(cx);
        cx.emit(Event::Layout {
            wrap: self.wrap,
            tree,
        });
        cx.notify();
    }

    pub(super) fn toggle_folder(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(path) {
            self.collapsed.insert(path.to_owned());
        }
        self.relist(cx);
        cx.notify();
    }

    /// The file list's rows for the filter and the layout.
    pub(super) fn relist(&mut self, cx: &App) {
        let filter = self.filter.read(cx).text.clone();
        self.list_rows = self
            .view
            .update
            .patch
            .as_ref()
            .map_or_else(Vec::new, |diff| {
                list_rows(&diff.files, &filter, self.tree, &self.collapsed)
            });
    }

    /// The listed files, in the order the list shows them.
    pub(super) fn listed_files(&self) -> Vec<usize> {
        self.list_rows
            .iter()
            .filter_map(|row| match row {
                ListRow::File { index, .. } => Some(*index),
                ListRow::Folder { .. } => None,
            })
            .collect()
    }
}
