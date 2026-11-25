//! The dark palette. Plugin pages get the same colours from the `tokens:start` block in
//! packages/plugin-ui/plugin-ui.css, so a change here goes there too.

pub const BACKGROUND: u32 = 0x0f0e12;
/// The desktop's `--glass`, 58%, over the window's vibrancy, behind the sidebar and title bar.
pub const GLASS: u32 = 0x1a171c94;
pub const SURFACE: u32 = 0x17151b;
pub const FOREGROUND: u32 = 0xece9e4;
pub const MUTED_FOREGROUND: u32 = 0x9d978f;
/// The redesign's `--fg-2`: a pane's title and the project bar's buttons, between muted and foreground.
pub const SECONDARY_FOREGROUND: u32 = 0xb9b3aa;
/// The redesign's `--line-2`: an outlined button's border.
pub const LINE_STRONG: u32 = 0xffffff24;
pub const BORDER: u32 = 0xffffff14;
pub const HOVER: u32 = 0xffffff0f;
/// `--pressed`, and hover over something already tinted.
pub const HOVER_STRONG: u32 = 0xffffff1a;
pub const DANGER: u32 = 0xf2786a;
/// `--wings-info`, the plugin pages' blue.
pub const INFO: u32 = 0x9cc0ff;
/// `--popover` at the find bar's 95%.
pub const POPOVER: u32 = 0x201d23f2;
pub const HAIRLINE_STRONG: u32 = 0xffffff29;
pub const RADIUS: f32 = 10.0;
/// Agent states, the desktop's `--working`, `--blocked`, `--done` and `--idle`.
pub const WORKING: u32 = 0xe9b65f;
pub const BLOCKED: u32 = 0xf2786a;
pub const DONE: u32 = 0x7dd3a7;
pub const IDLE: u32 = 0x8a857e;
/// The Claude spark's brand orange, as in the desktop's `PaneIcon`.
pub const CLAUDE: u32 = 0xd97757;

/// The colours a plugin page may paint in place of the app's, from a mod's theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// The window's ground, darker than a page, under a pane floating over one; what a page lies on; and a dialog or
    /// popover over it.
    pub background: u32,
    pub surface: u32,
    pub dialog: u32,
    pub popover: u32,
    pub foreground: u32,
    pub muted_foreground: u32,
    pub info: u32,
    pub danger: u32,
    pub done: u32,
    pub working: u32,
    /// The page's ground from one colour to another, at an angle in degrees as in CSS. None lies on `surface`.
    pub ground: Option<(u32, u32, f32)>,
}

impl Palette {
    pub const APP: Palette = Palette {
        background: BACKGROUND,
        surface: SURFACE,
        dialog: crate::ui::DIALOG,
        popover: POPOVER,
        foreground: FOREGROUND,
        muted_foreground: MUTED_FOREGROUND,
        info: INFO,
        danger: DANGER,
        done: DONE,
        working: WORKING,
        ground: None,
    };
}

thread_local! {
    static PAINTING: std::cell::Cell<Palette> = const { std::cell::Cell::new(Palette::APP) };
}

/// The palette what is drawn now paints in: a page's own while it builds its elements, the app's otherwise.
pub fn palette() -> Palette {
    PAINTING.with(std::cell::Cell::get)
}

/// Paints in `palette` until the guard drops. The colours are read as elements are built, so it holds for a render.
pub fn painting(palette: Palette) -> PaintingGuard {
    PaintingGuard(PAINTING.with(|painting| painting.replace(palette)))
}

pub struct PaintingGuard(Palette);

impl Drop for PaintingGuard {
    fn drop(&mut self) {
        PAINTING.with(|painting| painting.set(self.0));
    }
}
