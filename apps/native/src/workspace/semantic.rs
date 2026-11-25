//! Semantic search of past sessions in the app (`wings_agents::semantic`): its indexer started on the sessions the
//! history reads, told when a key is pressed so it pauses, the sidebar's Settings row filling while it indexes, and its
//! switch in Settings, Agents.

use std::{sync::Arc, time::Duration};

use gpui::{AnyElement, App, Context, Subscription, Task, TextStyle, Window, div, prelude::*, px, relative, rgba};
use wings_agents::semantic::{Progress, Semantic, State, Status};
use wings_spaces::claude_accounts;

use super::Workspace;
#[cfg(feature = "test-support")]
use crate::workspace::NotifyWorkspace;
use crate::{host::SessionSource, theme, ui, ui::live};

/// How often the Settings row's fill moves on while it indexes: its layer draws it again this often, and nothing else.
const STEP: Duration = Duration::from_secs(1);
const HINT: &str = "Finds a past session from what it was about, not only its words, with a small model on this Mac.";

/// The app's side of the semantic index.
#[derive(Default)]
pub(super) struct Search {
    semantic: Option<Arc<Semantic>>,
    /// The indexer's progress, which the Settings row's fill reads as it draws. At rest with no index, as in tests.
    pub(super) progress: Arc<Progress>,
    _typing: Option<Subscription>,
    _changes: Option<Task<()>>,
}

impl Workspace {
    /// Starts the indexer, when the app reads every account's past sessions and has a folder for the index.
    pub(super) fn start_semantic(&mut self, cx: &mut Context<Self>) {
        let (Some(dir), Some(SessionSource::Accounts(default))) = (self.config.semantic.clone(), self.config.sessions.clone())
        else {
            return;
        };
        let Some(helper) = std::env::current_exe().ok().map(|exe| exe.with_file_name("wings-embed")) else { return };
        let semantic = Semantic::open(dir, helper);
        self.sessions.set_semantic(semantic.clone());
        let (tell, told) = async_channel::unbounded::<()>();
        semantic.start(
            move || claude_accounts::dirs_with_default(default.clone()),
            move || {
                let _ = tell.try_send(());
            },
        );
        let typing = semantic.clone();
        self.search._typing = Some(cx.intercept_keystrokes(move |_, _, _| typing.typed()));
        // Only when what shows changes: a round starts or ends, it fails or is turned off. Never at a step.
        self.search._changes = Some(cx.spawn(async move |this, cx| {
            while told.recv().await.is_ok() {
                if this.update(cx, |this, cx| this.redraw_sidebar(cx)).is_err() {
                    break;
                }
            }
        }));
        self.search.progress = semantic.progress();
        self.search.semantic = Some(semantic);
    }

    fn set_semantic(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(semantic) = &self.search.semantic {
            semantic.set_enabled(on);
        }
        self.redraw_sidebar(cx);
    }

    /// The fill of the sidebar's Settings row while a round worth showing runs, its background filling from the left
    /// as far as the round is, and what the row's tooltip says. Its layer draws it and moves it on, so the sidebar
    /// draws only when it comes and goes.
    pub(super) fn indexing_fill(&self) -> Option<(AnyElement, impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static)> {
        self.search.progress.shown()?;
        let progress = self.search.progress.clone();
        let fill = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(live::slot(live::Kind::Progress, "semantic-indexing", IndexingFill(progress.clone())))
            .into_any_element();
        let tooltip = ui::tooltip_with(move || {
            let (done, total) = progress.counts();
            let label = format!("Indexing past sessions · {done} of {total}");
            #[cfg(test)]
            TOLD.with_borrow_mut(|told| told.push(label.clone()));
            label.into()
        });
        Some((fill, tooltip))
    }

    /// Settings, Agents: the switch, and what the index is doing.
    pub(super) fn semantic_card(&self, cx: &mut Context<Self>) -> Option<Vec<AnyElement>> {
        let status = self.search.semantic.as_ref()?.status();
        let on = status.enabled;
        Some(vec![ui::setting_row(
            "Search past sessions by meaning",
            format!("{HINT} {}", status_line(&status)),
            status.error.clone().filter(|_| status.state == State::Failed).map(Into::into),
            ui::switch("semantic-search", on).on_click(cx.listener(move |this, _, _, cx| this.set_semantic(!on, cx))),
        )])
    }

    /// Settings, Agents with an index that is `412 of 929` sessions in, for screenshots: its model never runs.
    #[cfg(feature = "test-support")]
    pub fn sample_semantic_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = std::env::temp_dir().join("wings-native-shot-semantic");
        let semantic = Semantic::with(dir, 0, Box::new(|_| Err("no model in screenshots".into())));
        self.search.progress = semantic.progress();
        self.search.semantic = Some(semantic);
        self.show_semantic_progress(412, 929, cx);
        self.open_settings(window, cx);
        if let Some(dialog) = &mut self.settings {
            dialog.section = super::settings::Section::Agents;
        }
        cx.notify_workspace();
    }

    /// Shows the Settings row's fill `done` of `total` sessions in, as an indexing round would, for screenshots and tests.
    #[cfg(feature = "test-support")]
    pub fn show_semantic_progress(&mut self, done: u32, total: u32, cx: &mut Context<Self>) {
        self.search.progress.set(State::Indexing, done, total);
        self.redraw_sidebar(cx);
    }
}

/// What Settings says the index is doing.
fn status_line(status: &Status) -> String {
    let size = |bytes: u64| format!("{:.0} MB", bytes as f64 / 1e6);
    match status.state {
        State::Off => "Off: searches match words only.".into(),
        State::Downloading => format!("Downloading the model ({}) once…", size(wings_agents::semantic::model::MODEL.size)),
        State::Indexing if status.total >= wings_agents::semantic::SHOW_AT => {
            // No break inside the count.
            format!("Indexing past sessions · {}\u{a0}of\u{a0}{}.", status.done, status.total)
        }
        State::Failed => "It will try again in a few minutes.".into(),
        _ if status.sessions == 0 => "Nothing indexed yet.".into(),
        _ => format!("{} sessions indexed, {}.", status.sessions, size(status.bytes)),
    }
}

/// The Settings row's indexing fill: its rounded background, as wide as the share of the round done. The layer draws it
/// over the row, so it is the hover's faint white and the row's text reads through it.
struct IndexingFill(Arc<Progress>);

#[cfg(test)]
thread_local! {
    /// How many times the fill was drawn, and the share it showed last.
    static DRAWN: std::cell::Cell<(usize, f32)> = const { std::cell::Cell::new((0, 0.0)) };
    /// What the Settings row's tooltip said each time it showed.
    static TOLD: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl live::Draw for IndexingFill {
    fn pace(&self, _: &Window, _: &App) -> Option<Duration> {
        self.0.shown().map(|_| STEP)
    }

    fn draw(&self, _: Duration, _: &TextStyle, _: &Window, _: &App) -> AnyElement {
        let share = self.0.shown().unwrap_or(1.0);
        #[cfg(test)]
        DRAWN.with(|drawn| drawn.set((drawn.get().0 + 1, share)));
        // A clip as wide as the share over the row's whole rounded shape, so the front edge is straight.
        div()
            .h_full()
            .w(relative(share))
            .overflow_hidden()
            .children(crate::ui::cutoff::clips_on_purpose())
            .child(div().h_full().w(relative(1.0 / share.max(0.001))).rounded(px(theme::RADIUS)).bg(rgba(theme::HOVER)))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::open_config;

    #[test]
    fn the_indexing_fill_moves_on_in_its_layer_and_the_sidebar_never_draws_for_a_step() {
        let (app, mut window) = open_config(|_| {});
        window.draw();
        let sidebar = |window: &gpui::TestAppWindow<Workspace>| {
            window.read(|w, cx| w.parts.as_ref().expect("the pieces").sidebar.read(cx).drawn)
        };
        window.update(|w, _, cx| w.show_semantic_progress(100, 929, cx));
        window.draw();
        assert!(live::placed("semantic-indexing").is_some(), "the fill is on Settings");
        let (before, (fills, _)) = (sidebar(&window), DRAWN.with(|d| d.get()));
        let progress = window.read(|w, _| w.search.progress.clone());
        for done in 101..=110 {
            progress.set(State::Indexing, done, 929);
            app.advance_clock(STEP);
            app.run_until_parked();
        }
        // A step the layer's clock did not run in time (a loaded machine) is drawn by the next one: the fill shows the
        // share as it stands once a step has passed with no change.
        app.advance_clock(STEP);
        app.run_until_parked();
        let (drawn, share) = DRAWN.with(|d| d.get());
        assert_eq!(sidebar(&window), before, "the sidebar kept its drawing through ten steps");
        assert!(drawn - fills >= 9, "the layer drew the fill {} times in ten steps", drawn - fills);
        assert!((share - 110.0 / 929.0).abs() < 1e-6, "it shows how far it is now");

        // Done: the fill goes, which draws the sidebar once.
        progress.set(State::Ready, 929, 929);
        window.update(|w, _, cx| w.redraw_sidebar(cx));
        window.draw();
        assert_eq!(sidebar(&window), before + 1);
        assert!(window.read(|w, _| w.indexing_fill().is_none()));
    }

    #[test]
    fn pointing_at_settings_while_it_indexes_says_how_far_it_is() {
        use gpui::{Modifiers, MouseMoveEvent, point};
        let (app, mut window) = open_config(|_| {});
        window.draw();
        window.update(|w, _, cx| w.show_semantic_progress(412, 929, cx));
        window.draw();
        let at = crate::ui::spot_at("open-settings").expect("the Settings row").center();
        for position in [point(px(700.0), px(400.0)), at, point(at.x + px(1.0), at.y)] {
            window.simulate_event(MouseMoveEvent { position, pressed_button: None, modifiers: Modifiers::default() });
            window.draw();
        }
        app.advance_clock(Duration::from_millis(600));
        app.run_until_parked();
        window.draw();
        assert_eq!(TOLD.with_borrow(|told| told.last().cloned()).as_deref(), Some("Indexing past sessions · 412 of 929"));
    }

    #[test]
    fn the_tooltip_counts_on_while_it_shows() {
        let (app, mut window) = open_config(|_| {});
        window.draw();
        window.update(|w, _, cx| w.show_semantic_progress(412, 929, cx));
        window.draw();
        assert!(hover_until_tooltip(&app, &mut window, "open-settings"), "the tooltip shows");
        let progress = window.read(|w, _| w.search.progress.clone());
        progress.set(State::Indexing, 413, 929);
        app.advance_clock(STEP);
        app.run_until_parked();
        window.draw();
        assert!(tooltip_shown(&mut window), "still shown");
        assert_eq!(TOLD.with_borrow(|told| told.last().cloned()).as_deref(), Some("Indexing past sessions · 413 of 929"));
    }

    /// Points at `spot` from elsewhere and waits for its tooltip; whether the window then draws one.
    fn hover_until_tooltip(app: &gpui::TestApp, window: &mut gpui::TestAppWindow<Workspace>, spot: &str) -> bool {
        use gpui::{Modifiers, MouseMoveEvent, point};
        let at = crate::ui::spot_at(spot).expect(spot).center();
        for position in [point(px(700.0), px(400.0)), at, point(at.x + px(1.0), at.y)] {
            window.simulate_event(MouseMoveEvent { position, pressed_button: None, modifiers: Modifiers::default() });
            window.draw();
        }
        app.advance_clock(Duration::from_millis(600));
        app.run_until_parked();
        window.draw();
        tooltip_shown(window)
    }

    fn tooltip_shown(window: &mut gpui::TestAppWindow<Workspace>) -> bool {
        window.update(|_, window, _| crate::ui::tooltip_shown_in(window.window_handle().window_id()))
    }

    #[test]
    fn pointing_anywhere_on_the_settings_row_shows_its_tooltip_while_on_it() {
        use gpui::{Modifiers, MouseMoveEvent, point};
        let (app, mut window) = open_config(|_| {});
        window.draw();
        window.update(|w, _, cx| w.show_semantic_progress(412, 929, cx));
        window.draw();
        // Its middle, well away from the foot of the row.
        assert!(hover_until_tooltip(&app, &mut window, "open-settings"), "the tooltip shows from the middle of the row");
        let row = crate::ui::spot_at("open-settings").unwrap();
        for position in [
            point(row.left() + px(12.0), row.center().y),
            point(row.right() - px(12.0), row.center().y),
            point(row.center().x, row.top() + px(3.0)),
        ] {
            window.simulate_event(MouseMoveEvent { position, pressed_button: None, modifiers: Modifiers::default() });
            window.draw();
            app.advance_clock(Duration::from_millis(100));
            app.run_until_parked();
            window.draw();
            assert!(tooltip_shown(&mut window), "still shown at {position:?}, on the row");
        }
    }

    #[test]
    fn the_fill_goes_with_a_closed_sidebar() {
        let (app, mut window) = open_config(|_| {});
        window.draw();
        window.update(|w, _, cx| w.show_semantic_progress(100, 929, cx));
        window.draw();
        crate::workspace::tests::press(&mut window, "b");
        assert!(!window.read(|w, _| w.sidebar));
        for _ in 0..5 {
            app.advance_clock(STEP);
            app.run_until_parked();
            window.draw();
        }
        let before = DRAWN.with(|d| d.get().0);
        for _ in 0..3 {
            app.advance_clock(STEP);
            app.run_until_parked();
            window.draw();
        }
        assert_eq!(DRAWN.with(|d| d.get().0), before, "nothing draws the fill where the sidebar was");
    }

    #[test]
    fn a_catch_up_of_a_few_sessions_shows_nothing() {
        let (_app, mut window) = open_config(|_| {});
        window.update(|w, _, cx| w.show_semantic_progress(0, wings_agents::semantic::SHOW_AT - 1, cx));
        assert!(window.read(|w, _| w.indexing_fill().is_none()));
    }
}
