//! Behaviors: the interaction logic of popups and lists, written once and composed into components, which bring their
//! own parts and look (after Vercel's UI SDK, https://ui-sdk.dev/docs/core/behaviors). One behavior per concern:
//!
//! - `position`: where a popup goes beside its anchor (`position`).
//! - `popup`: draws a popup's content there, over the window (`createPopup`).
//! - `layer`: Escape and a press outside dismiss it (`bindLayer`); `bind_escape` is the Escape half for a modal dialog.
//! - `focus_recovery`: the keyboard goes back to the trigger as it closes (`createFocusRecovery`).
//! - `focus_trap`: Tab and Shift-Tab cycle inside a modal dialog and never leave it (`trap_tab`).
//! - `reveal`: the active item scrolls into view, by a named strategy (`Reveal`).
//! - `listbox`: the arrow keys move the highlighted option (`createListbox`).
//!
//! A menu is `popup` + `layer` + `focus_recovery` + `listbox` around `ui::menu`'s box and items; a select or
//! combobox is `popup` + `layer` (+ `listbox` for a combobox, whose field keeps the keyboard) around `ui::select_menu`.

pub mod focus_recovery;
pub mod focus_trap;
pub mod layer;
pub mod listbox;
pub mod popup;
pub mod position;
pub mod reveal;

pub use focus_recovery::FocusRecovery;
pub use focus_trap::{TrapTabExt, trap_tab};
pub use layer::{DismissReason, EscapeExt, bind_escape, bind_layer, capture_escape};
pub use popup::{Popup, popup, popup_at, popup_on_parent};
pub use position::{Align, Placement, Side};
pub use reveal::{Reveal, RevealExt};
