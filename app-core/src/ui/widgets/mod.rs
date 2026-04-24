//! Reusable UI widgets.
//!
//! Four layers:
//!
//! * [`containers`] - rounded-card visual wrappers. Used by the
//!   stopwatch / timer / alarm / status / numpad screens.
//! * [`bodies`] - content layouts (label + value, icon + label) that
//!   draw into a `Rectangle`. Composable with any container.
//! * [`chrome`] - legacy screen-level decorations (rounded header
//!   bar, vertical page scrollbar) for the card-style screens.
//! * [`nightwatch`] - sharp HUD-panel vocabulary for the watch face,
//!   app grid, and settings screens: chamfered hex outlines, hanging
//!   tag labels, red-hairline headers, toggles, 1-row dividers.
//!
//! Screens pick exactly one chrome style - card or nightwatch - to
//! avoid mixing rounded and sharp on the same surface.

pub mod containers;
pub mod bodies;
pub mod chrome;
pub mod nightwatch;
pub mod numpad;

pub use containers::{card, CardStyle};
pub use bodies::{icon_button, value_body};
pub use chrome::{header_bar, page_scrollbar, HeaderIcon, HEADER_ICON_HIT_WIDTH};
pub use nightwatch::{
    chamfered_panel, header, header_icon_hit, row, tile, toggle, tag_label,
    RowControl, NOTCH, HEADER_H, ROW_H, TAG_LABEL_H,
};
pub use numpad::{Numpad, NumpadAction, MAX_DIGITS};
