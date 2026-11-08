//! The dark palette from apps/desktop/src/index.css, shared with plugin-tokens.mjs.

pub const BACKGROUND: u32 = 0x0f0e12;
/// The desktop's `--glass`, 58%, over the window's vibrancy, behind the sidebar and title bar.
pub const GLASS: u32 = 0x1a171c94;
pub const SURFACE: u32 = 0x17151b;
pub const FOREGROUND: u32 = 0xece9e4;
pub const MUTED_FOREGROUND: u32 = 0x9d978f;
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
