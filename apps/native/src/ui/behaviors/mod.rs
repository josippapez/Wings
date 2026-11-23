//! Behaviors: the interaction logic of popups and lists, written once and composed into components, which bring their
//! own parts and look (after Vercel's UI SDK, https://ui-sdk.dev/docs/core/behaviors). One behavior per concern:
//!
//! - `position`: where a popup goes beside its anchor (`position`).
//! - `popup`: draws a popup's content there, over the window (`createPopup`).
//! - `layer`: Escape and a press outside dismiss it (`bindLayer`); `bind_escape` is the Escape half for a modal dialog.
//! - `focus_recovery`: the keyboard goes back to the trigger as it closes (`createFocusRecovery`).
//! - `focus_trap`: Tab and Shift-Tab cycle inside a modal dialog and never leave it (`trap_tab`).
//! - `drag`: a press becomes a drag past a threshold, and Escape cancels it.
//! - `menu_keys`: Up, Down, Home, End, a typed letter and Enter in an open menu (`MenuKeys`).
//! - `truncation`: text cut off by an ellipsis shows all of itself in a tooltip (`cut_text`).
//! - `reveal`: the active item scrolls into view, by a named strategy (`Reveal`).
//! - `listbox`: the arrow keys move the highlighted option (`createListbox`); `clamp_after` for rows that stop at the
//!   ends and page.
//!
//! A menu is `popup` + `layer` + `focus_recovery` + `listbox` around `ui::menu`'s box and items; a select or
//! combobox is `popup` + `layer` (+ `listbox` for a combobox, whose field keeps the keyboard) around `ui::select_menu`.

pub mod drag;
pub mod focus_recovery;
pub mod focus_trap;
pub mod layer;
pub mod listbox;
pub mod menu_keys;
pub mod popup;
pub mod position;
pub mod reveal;
pub mod truncation;

pub use focus_recovery::FocusRecovery;
pub use focus_trap::{TrapTabExt, cycle_until, trap_tab};
pub use menu_keys::{MenuKey, MenuKeys};
pub use layer::{DismissReason, EscapeExt, bind_escape, bind_layer, capture_escape};
pub use popup::{Popup, popup, popup_at, popup_on_parent};
pub use position::{Align, Placement, Side};
pub use truncation::cut_text;
pub use reveal::{Reveal, RevealExt};
