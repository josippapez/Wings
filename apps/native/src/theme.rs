//! The dark palettes. Plugin pages get the same colours from the `tokens:start` block in
//! packages/plugin-ui/plugin-ui.css, so a change to Wings' own palette goes there too.
//!
//! A theme is a set of `Colors`. The one in use is a thread-local, as `PAINTING` is: the colours are read as elements are
//! built, on the window's thread, so a change applies on the next draw (`cx.refresh_windows()`) and nothing draws per
//! frame for it. Terminals keep their own palette (`wings_terminal::grid`), which a theme leaves alone.

use std::cell::Cell;

pub const RADIUS: f32 = 10.0;

/// A theme's colours, the roles the app paints with. `hover`, `border` and the like are white at a low alpha on every
/// dark theme, so they are not in here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    pub background: u32,
    /// The window's glass, 58% over the window's vibrancy, behind the sidebar and title bar.
    pub glass: u32,
    /// The same ground with no see-through, for "Transparent sidebar and title bar" turned off.
    pub solid: u32,
    pub surface: u32,
    pub dialog: u32,
    pub popover: u32,
    pub foreground: u32,
    pub muted_foreground: u32,
    pub secondary_foreground: u32,
    pub info: u32,
    pub danger: u32,
    pub working: u32,
    pub done: u32,
    pub idle: u32,
    pub claude: u32,
}

/// What the Appearance setting chooses between.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Wings,
    Midnight,
    Graphite,
    Nord,
    Forest,
    Dusk,
}

impl Theme {
    pub const ALL: [Theme; 6] =
        [Theme::Wings, Theme::Midnight, Theme::Graphite, Theme::Nord, Theme::Forest, Theme::Dusk];

    pub fn label(self) -> &'static str {
        match self {
            Theme::Wings => "Wings",
            Theme::Midnight => "Midnight",
            Theme::Graphite => "Graphite",
            Theme::Nord => "Nord",
            Theme::Forest => "Forest",
            Theme::Dusk => "Dusk",
        }
    }

    /// The name saved in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Theme::Wings => "wings",
            Theme::Midnight => "midnight",
            Theme::Graphite => "graphite",
            Theme::Nord => "nord",
            Theme::Forest => "forest",
            Theme::Dusk => "dusk",
        }
    }

    /// The theme a saved name stands for; none, or one this version doesn't know, is Wings.
    pub fn from_key(key: Option<&str>) -> Theme {
        Theme::ALL.into_iter().find(|theme| Some(theme.key()) == key).unwrap_or_default()
    }

    pub const fn colors(self) -> Colors {
        match self {
            // The look the app always had.
            Theme::Wings => Colors {
                background: 0x0f0e12,
                glass: 0x1a171c,
                solid: 0x15131a,
                surface: 0x17151b,
                dialog: 0x201d23,
                popover: 0x201d23,
                foreground: 0xece9e4,
                muted_foreground: 0x9d978f,
                secondary_foreground: 0xb9b3aa,
                info: 0x9cc0ff,
                danger: 0xf2786a,
                working: 0xe9b65f,
                done: 0x7dd3a7,
                idle: 0x8a857e,
                claude: 0xd97757,
            },
            Theme::Midnight => Colors {
                background: 0x0a0e18,
                glass: 0x141a2a,
                solid: 0x0e1322,
                surface: 0x111726,
                dialog: 0x192033,
                popover: 0x192033,
                foreground: 0xe6ebf5,
                muted_foreground: 0x98a3ba,
                secondary_foreground: 0xb5bfd3,
                info: 0x8ab4ff,
                danger: 0xff7b72,
                working: 0xf0c060,
                done: 0x6fdba0,
                idle: 0x7f8aa3,
                claude: 0xd97757,
            },
            Theme::Graphite => Colors {
                background: 0x101010,
                glass: 0x1c1c1c,
                solid: 0x151515,
                surface: 0x1a1a1a,
                dialog: 0x242424,
                popover: 0x242424,
                foreground: 0xececec,
                muted_foreground: 0x9a9a9a,
                secondary_foreground: 0xb8b8b8,
                info: 0x8fbfff,
                danger: 0xf27a6e,
                working: 0xe5b95c,
                done: 0x7ccf9f,
                idle: 0x888888,
                claude: 0xd97757,
            },
            Theme::Nord => Colors {
                background: 0x242933,
                glass: 0x2e3440,
                solid: 0x272c36,
                surface: 0x2b303b,
                dialog: 0x363d4b,
                popover: 0x363d4b,
                foreground: 0xe5e9f0,
                muted_foreground: 0x9fabbe,
                secondary_foreground: 0xbcc4d3,
                info: 0x88c0d0,
                danger: 0xee949d,
                working: 0xebcb8b,
                done: 0xa3be8c,
                idle: 0x8791a3,
                claude: 0xd08770,
            },
            Theme::Forest => Colors {
                background: 0x0b110e,
                glass: 0x16201b,
                solid: 0x101912,
                surface: 0x121b17,
                dialog: 0x1b2822,
                popover: 0x1b2822,
                foreground: 0xe4ebe5,
                muted_foreground: 0x93a39a,
                secondary_foreground: 0xb2c0b7,
                info: 0x8cc5e8,
                danger: 0xf2786a,
                working: 0xe3b75f,
                done: 0x7fdc9d,
                idle: 0x82938a,
                claude: 0xd97757,
            },
            Theme::Dusk => Colors {
                background: 0x0f0b18,
                glass: 0x1e1830,
                solid: 0x150f24,
                surface: 0x171126,
                dialog: 0x221a34,
                popover: 0x221a34,
                foreground: 0xede8f5,
                muted_foreground: 0xa59bb8,
                secondary_foreground: 0xc0b7d0,
                info: 0xa5b4ff,
                danger: 0xf27d8d,
                working: 0xf0b866,
                done: 0x7dd9b0,
                idle: 0x8f86a3,
                claude: 0xe08a6a,
            },
        }
    }
}

thread_local! {
    static THEME: Cell<(Theme, bool)> = const { Cell::new((Theme::Wings, true)) };
}

/// Paints in `theme` from the next draw, with the sidebar and title bar on the window's glass or, `transparent` off, on a
/// solid ground of the theme's.
pub fn set(theme: Theme, transparent: bool) {
    THEME.with(|current| current.set((theme, transparent)));
}

pub fn current() -> Theme {
    THEME.with(Cell::get).0
}

pub fn transparent() -> bool {
    THEME.with(Cell::get).1
}

pub fn colors() -> Colors {
    current().colors()
}

/// What the window lies on behind the sidebar and title bar, with its alpha: the glass, or the theme's solid ground.
pub fn glass() -> u32 {
    let colors = colors();
    if transparent() { colors.glass << 8 | 0x94 } else { colors.solid << 8 | 0xff }
}

pub fn background() -> u32 { colors().background }
pub fn surface() -> u32 { colors().surface }
pub fn dialog() -> u32 { colors().dialog }
/// `--popover` at the find bar's 95%.
pub fn popover() -> u32 { colors().popover << 8 | 0xf2 }
pub fn foreground() -> u32 { colors().foreground }
pub fn muted_foreground() -> u32 { colors().muted_foreground }
/// The redesign's `--fg-2`: a pane's title and the project bar's buttons, between muted and foreground.
pub fn secondary_foreground() -> u32 { colors().secondary_foreground }
/// `--wings-info`, the plugin pages' blue.
pub fn info() -> u32 { colors().info }
pub fn danger() -> u32 { colors().danger }
/// Agent states, the desktop's `--working`, `--blocked`, `--done` and `--idle`.
pub fn working() -> u32 { colors().working }
pub fn blocked() -> u32 { colors().danger }
pub fn done() -> u32 { colors().done }
pub fn idle() -> u32 { colors().idle }
/// The Claude spark's brand orange, as in the desktop's `PaneIcon`.
pub fn claude() -> u32 { colors().claude }
/// The redesign's `--line-2`: an outlined button's border.
pub fn line_strong() -> u32 { 0xffffff24 }
pub fn border() -> u32 { 0xffffff14 }
pub fn hover() -> u32 { 0xffffff0f }
/// `--pressed`, and hover over something already tinted.
pub fn hover_strong() -> u32 { 0xffffff1a }
pub fn hairline_strong() -> u32 { 0xffffff29 }

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
    /// The palette of a theme.
    pub fn of(theme: Theme) -> Palette {
        let colors = theme.colors();
        Palette {
            background: colors.background,
            surface: colors.surface,
            dialog: colors.dialog,
            popover: colors.popover << 8 | 0xf2,
            foreground: colors.foreground,
            muted_foreground: colors.muted_foreground,
            info: colors.info,
            danger: colors.danger,
            done: colors.done,
            working: colors.working,
            ground: None,
        }
    }

    /// The app's own palette in the theme in use, which a page paints in unless a mod's theme gives it another.
    pub fn app() -> Palette {
        Palette::of(current())
    }
}

thread_local! {
    static PAINTING: std::cell::Cell<Option<Palette>> = const { std::cell::Cell::new(None) };
}

/// The palette what is drawn now paints in: a page's own while it builds its elements, the app's otherwise.
pub fn palette() -> Palette {
    PAINTING.with(std::cell::Cell::get).unwrap_or_else(Palette::app)
}

/// Paints in `palette` until the guard drops. The colours are read as elements are built, so it holds for a render.
pub fn painting(palette: Palette) -> PaintingGuard {
    PaintingGuard(PAINTING.with(|painting| painting.replace(Some(palette))))
}

pub struct PaintingGuard(Option<Palette>);

impl Drop for PaintingGuard {
    fn drop(&mut self) {
        PAINTING.with(|painting| painting.set(self.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.x contrast of two opaque colours.
    fn contrast(a: u32, b: u32) -> f32 {
        let luminance = |c: u32| {
            let channel = |shift: u32| {
                let v = ((c >> shift) & 0xff) as f32 / 255.0;
                if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
        };
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn the_palette_follows_the_theme_and_a_page_s_own_wins() {
        set(Theme::Wings, true);
        assert_eq!(palette(), Palette::of(Theme::Wings));
        assert_eq!(foreground(), 0xece9e4, "the look the app always had");
        set(Theme::Nord, true);
        assert_eq!(palette().surface, Theme::Nord.colors().surface);
        assert_ne!(palette(), Palette::of(Theme::Wings));
        assert_eq!(muted_foreground(), Theme::Nord.colors().muted_foreground);
        let own = Palette { foreground: 0x123456, ..Palette::app() };
        {
            let _page = painting(own);
            assert_eq!(palette().foreground, 0x123456);
        }
        assert_eq!(palette(), Palette::of(Theme::Nord), "back to the app's once the page is built");
        set(Theme::Wings, true);
    }

    #[test]
    fn glass_is_see_through_only_while_transparency_is_on() {
        set(Theme::Forest, true);
        assert_eq!(glass(), Theme::Forest.colors().glass << 8 | 0x94);
        set(Theme::Forest, false);
        assert_eq!(glass(), Theme::Forest.colors().solid << 8 | 0xff);
        set(Theme::Wings, true);
    }

    #[test]
    fn a_theme_is_saved_by_name_and_an_unknown_one_is_wings() {
        for theme in Theme::ALL {
            assert_eq!(Theme::from_key(Some(theme.key())), theme);
        }
        assert_eq!(Theme::from_key(None), Theme::Wings);
        assert_eq!(Theme::from_key(Some("neon")), Theme::Wings);
    }

    #[test]
    fn every_theme_keeps_its_text_readable_on_its_grounds() {
        for theme in Theme::ALL {
            let c = theme.colors();
            for ground in [c.background, c.surface, c.dialog, c.solid] {
                for (name, text, least) in [
                    ("foreground", c.foreground, 7.0),
                    ("secondary", c.secondary_foreground, 4.5),
                    ("muted", c.muted_foreground, 4.5),
                    ("info", c.info, 4.5),
                    ("danger", c.danger, 4.5),
                    ("working", c.working, 4.5),
                    ("done", c.done, 4.5),
                    ("idle", c.idle, 3.0),
                ] {
                    let ratio = contrast(text, ground);
                    assert!(ratio >= least, "{} {name} {text:06x} on {ground:06x} is {ratio:.2}", theme.label());
                }
            }
        }
    }
}
