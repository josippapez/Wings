//! The performance readout: a small card pinned in a corner of the window with the app's own CPU use, the window frames
//! drawn per second and the views that rendered most in the last second, so the user can check them without `top` or a
//! profile. It is off until the "Performance Readout" command turns it on, and while off nothing is counted or sampled.
//!
//! On, the cost is one render of the card a second, and only when a number changed. The card is a view of its own kept as
//! last drawn (`.cached`), so its update draws none of the workspace's pieces (`workspace::parts`), and the counts are a
//! thread-local tally that a render or a frame adds one to.
//!
//! An agent reads the same numbers over a few seconds with the `performance` tool (`Readout::begin_measure`): a second
//! tally, kept apart so the card's own once-a-second update is not disturbed, counts only for that window, with or
//! without the card shown.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    panic::Location,
    time::{Duration, Instant},
};

use gpui::{
    Context, FontWeight, IntoElement, Render, StyleRefinement, Styled, Task, Window, canvas, div,
    prelude::*, px, rgb, rgba,
};

use crate::theme;

/// How often the card updates.
const PERIOD: Duration = Duration::from_secs(1);
/// The views it lists.
const LISTED: usize = 4;
const WIDTH: f32 = 190.0;
/// Room for the two numbers, the heading and the listed views (14 each, 1 apart), the heading's 3 of margin, the card's
/// 8 of padding top and bottom and its border, so the cached card never changes size.
const HEIGHT: f32 = 15.0 * (3 + LISTED) as f32 + 3.0 + 16.0 + 2.0;
const MARGIN: f32 = 10.0;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static FRAMES: Cell<u32> = const { Cell::new(0) };
    static RENDERS: RefCell<HashMap<&'static str, u32>> = RefCell::new(HashMap::new());
    /// A measurement for an agent is running, and its own tally.
    static MEASURING: Cell<bool> = const { Cell::new(false) };
    static MEASURED_FRAMES: Cell<u32> = const { Cell::new(0) };
    static MEASURED_RENDERS: RefCell<HashMap<&'static str, u32>> = RefCell::new(HashMap::new());
    /// The places that notified the workspace during the measurement, with how many times each did.
    static MEASURED_CAUSES: RefCell<HashMap<&'static Location<'static>, u32>> = RefCell::new(HashMap::new());
}

/// A view called `name` rendered. Does nothing while the readout is off and no measurement runs.
pub fn render(name: &'static str) {
    if ON.get() {
        RENDERS.with_borrow_mut(|renders| *renders.entry(name).or_default() += 1);
    }
    if MEASURING.get() {
        MEASURED_RENDERS.with_borrow_mut(|renders| *renders.entry(name).or_default() += 1);
    }
}

/// The workspace was notified from `at` (`workspace::NotifyWorkspace`). Does nothing while no measurement runs.
pub fn notified(at: &'static Location<'static>) {
    if MEASURING.get() {
        MEASURED_CAUSES.with_borrow_mut(|causes| *causes.entry(at).or_default() += 1);
    }
}

/// A source location as `workspace/sidebar/mod.rs:123`, from the `src` folder of the app.
fn place(at: &Location<'_>) -> String {
    let file = at.file().replace('\\', "/");
    let file = file.split_once("/src/").map_or(file.as_str(), |(_, rest)| rest);
    let file = file.strip_prefix("src/").unwrap_or(file);
    format!("{file}:{}", at.line())
}

/// The window drew a frame. Does nothing while the readout is off and no measurement runs.
fn frame() {
    if ON.get() {
        FRAMES.set(FRAMES.get() + 1);
    }
    if MEASURING.get() {
        MEASURED_FRAMES.set(MEASURED_FRAMES.get() + 1);
    }
}

/// Counts the window's draws: its prepaint runs once in each draw of the window. Put it in the workspace's root only
/// while the readout is on; it asks for no frame.
pub fn frame_counter() -> impl IntoElement {
    canvas(|_, _, _| frame(), |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .w(px(0.0))
        .h(px(0.0))
}

/// How the card is placed in the window, for a `.cached` view.
pub fn placement() -> StyleRefinement {
    StyleRefinement::default()
        .absolute()
        .right(px(MARGIN))
        .bottom(px(MARGIN))
        .w(px(WIDTH))
        .h(px(HEIGHT))
}

/// This process's CPU time so far, user and system.
#[cfg(unix)]
fn cpu_time() -> Duration {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `getrusage` fills the struct it is given; RUSAGE_SELF cannot fail with a valid pointer.
    let usage = unsafe {
        if libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) != 0 {
            return Duration::ZERO;
        }
        usage.assume_init()
    };
    let time = |t: libc::timeval| Duration::new(t.tv_sec.max(0) as u64, (t.tv_usec.max(0) as u32) * 1000);
    time(usage.ru_utime) + time(usage.ru_stime)
}

#[cfg(not(unix))]
fn cpu_time() -> Duration {
    Duration::ZERO
}

/// What a measurement over a few seconds found, for the `performance` tool.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    /// The real time it covers.
    pub seconds: f64,
    /// Percent of one core.
    pub cpu_percent: f64,
    pub frames: u32,
    /// Every view that rendered, with how many renders, most first.
    pub renders: Vec<(&'static str, u32)>,
    /// Every place that notified the workspace, which redraws all its pieces, with how many times, most first.
    pub causes: Vec<(String, u32)>,
    /// The card was on during it, so its own update is in these numbers.
    pub card_shown: bool,
}

/// What the card shows.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Shown {
    /// Percent of one core.
    pub cpu: u32,
    pub frames: u32,
    /// The views that rendered most, with how many renders each, most first.
    pub views: Vec<(&'static str, u32)>,
}

pub struct Readout {
    on: bool,
    last: Option<(Instant, Duration)>,
    shown: Shown,
    task: Option<Task<()>>,
    /// A measurement for an agent has begun, and where its window starts once it is settled.
    measuring: bool,
    baseline: Option<(Instant, Duration)>,
    /// How many times it rendered, for tests of what its update draws.
    #[cfg(test)]
    pub(crate) drawn: usize,
}

impl Readout {
    /// Off. Make it before the window's first draw: GPUI only redraws a view that is older than the frame it is first
    /// drawn in.
    pub fn new() -> Self {
        Self {
            on: false,
            last: None,
            shown: Shown::default(),
            task: None,
            measuring: false,
            baseline: None,
            #[cfg(test)]
            drawn: 0,
        }
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    pub fn shown(&self) -> &Shown {
        &self.shown
    }

    /// The window has to count its frames: the card is on, or a measurement runs.
    pub fn counting(&self) -> bool {
        self.on || self.measuring
    }

    pub fn is_measuring(&self) -> bool {
        self.measuring
    }

    /// Starts counting for a measurement, with or without the card. It counts nothing yet: the window needs a draw to put
    /// the frame counter in, so `restart_measure` starts the window once that has settled.
    pub fn begin_measure(&mut self) -> Result<(), String> {
        if self.measuring {
            return Err("A performance measurement is already running. Try again when it is done.".into());
        }
        self.measuring = true;
        self.baseline = None;
        MEASURING.set(true);
        MEASURED_FRAMES.set(0);
        MEASURED_RENDERS.with_borrow_mut(HashMap::clear);
        MEASURED_CAUSES.with_borrow_mut(HashMap::clear);
        Ok(())
    }

    /// Starts the measured window now: the counts and the CPU time so far are left behind.
    pub fn restart_measure(&mut self) -> Result<(), String> {
        if !self.measuring {
            return Err("No performance measurement is running".into());
        }
        MEASURED_FRAMES.set(0);
        MEASURED_RENDERS.with_borrow_mut(HashMap::clear);
        MEASURED_CAUSES.with_borrow_mut(HashMap::clear);
        self.baseline = Some((Instant::now(), cpu_time()));
        Ok(())
    }

    /// Ends the measurement, whether or not its window started, and puts the readout as it was before it.
    pub fn end_measure(&mut self) -> Result<Measured, String> {
        let (now, cpu) = (Instant::now(), cpu_time());
        let baseline = self.baseline.take();
        let was = std::mem::replace(&mut self.measuring, false);
        MEASURING.set(false);
        let frames = MEASURED_FRAMES.replace(0);
        let mut renders: Vec<_> = MEASURED_RENDERS.with_borrow_mut(|renders| renders.drain().collect());
        renders.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let mut causes: Vec<(String, u32)> = MEASURED_CAUSES.with_borrow_mut(|causes| {
            let mut by_place = HashMap::<String, u32>::new();
            for (at, count) in causes.drain() {
                *by_place.entry(place(at)).or_default() += count;
            }
            by_place.into_iter().collect()
        });
        causes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let (Some((at, before)), true) = (baseline, was) else {
            return Err("The measurement never started".into());
        };
        let seconds = now.duration_since(at).as_secs_f64().max(0.001);
        Ok(Measured {
            seconds,
            cpu_percent: cpu.saturating_sub(before).as_secs_f64() / seconds * 100.0,
            frames,
            renders,
            causes,
            card_shown: self.on,
        })
    }

    /// Turns the readout on, which starts the counts and the once-a-second update, or off, which stops both.
    pub fn set_on(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.on == on {
            return;
        }
        self.on = on;
        ON.set(on);
        FRAMES.set(0);
        RENDERS.with_borrow_mut(HashMap::clear);
        self.shown = Shown::default();
        if on {
            self.last = Some((Instant::now(), cpu_time()));
            self.task = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(PERIOD).await;
                    if this.update(cx, |this, cx| this.sample(cx)).is_err() {
                        break;
                    }
                }
            }));
        } else {
            self.last = None;
            self.task = None;
        }
        cx.notify();
    }

    /// One second's numbers; the card renders again only when they differ from what it shows.
    fn sample(&mut self, cx: &mut Context<Self>) {
        let (now, cpu) = (Instant::now(), cpu_time());
        let Some((at, before)) = self.last.replace((now, cpu)) else {
            return;
        };
        // The real time between the samples, which a busy main thread can stretch past a second.
        let seconds = now.duration_since(at).as_secs_f64().max(0.001);
        let frames = FRAMES.replace(0);
        let mut views: Vec<_> = RENDERS.with_borrow_mut(|renders| renders.drain().collect());
        views.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        views.truncate(LISTED);
        let shown = Shown {
            cpu: (cpu.saturating_sub(before).as_secs_f64() / seconds * 100.0).round() as u32,
            frames: (f64::from(frames) / seconds).round() as u32,
            views: views
                .into_iter()
                .map(|(name, count)| (name, (f64::from(count) / seconds).round().max(1.0) as u32))
                .collect(),
        };
        if shown != self.shown {
            self.shown = shown;
            cx.notify();
        }
    }
}

impl Render for Readout {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.drawn += 1;
        }
        render("readout");
        let line = |label: String, value: String| {
            div()
                .flex()
                .justify_between()
                .child(div().text_color(rgb(theme::muted_foreground())).child(label))
                .child(div().text_color(rgb(theme::foreground())).child(value))
        };
        let shown = &self.shown;
        div()
            .size_full()
            .px(px(10.0))
            .py(px(8.0))
            .flex()
            .flex_col()
            .gap(px(1.0))
            .rounded(px(theme::RADIUS))
            .border_1()
            .border_color(rgba(theme::border()))
            .bg(rgba(theme::popover()))
            .text_size(px(11.0))
            .line_height(px(14.0))
            .font_family(crate::terminal_view::DEFAULT_FONT_FAMILY)
            .child(line("CPU".into(), format!("{}%", shown.cpu)))
            .child(line("Frames".into(), format!("{}/s", shown.frames)))
            .child(
                div()
                    .mt(px(3.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(theme::muted_foreground()))
                    .child("Renders/s"),
            )
            .children(shown.views.iter().map(|(name, count)| line((*name).into(), count.to_string())))
    }
}

/// What is counted so far: frames, and renders of all views. For tests of what is, and isn't, counted.
#[cfg(all(test, feature = "test-support"))]
pub(crate) fn counted() -> (u32, u32) {
    (FRAMES.get(), RENDERS.with_borrow(|renders| renders.values().sum()))
}
