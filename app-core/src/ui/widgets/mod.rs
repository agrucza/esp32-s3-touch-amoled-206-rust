//! Reusable UI widgets, grouped by kind.
//!
//! * [`containers`] - surfaces that content lives inside: rounded
//!   `card`, chamfered `chamfered_panel`, `tile`, `info_tile`, `tag_label`.
//! * [`bodies`] - content layouts drawn into a rect: `value_body`,
//!   `icon_button`, `row`.
//! * [`chrome`] - screen-level decorations: legacy `header_bar`, the
//!   Nightwatch `header`, top `status_bar`, bottom `home_indicator`,
//!   `page_scrollbar`.
//! * [`controls`] - interactive primitives: `toggle`.
//! * [`numpad`] - multi-digit entry widget for set-time / set-duration
//!   flows.

pub mod bodies;
pub mod chrome;
pub mod containers;
pub mod controls;
pub mod numpad;

pub use bodies::{icon_button, row, value_body, RowControl, ROW_H};
pub use chrome::{
    header, header_bar, header_icon_hit, home_indicator, page_scrollbar, status_bar,
    HeaderIcon, HEADER_H, HEADER_ICON_HIT_WIDTH, HOME_INDICATOR_H, STATUS_BAR_H,
};
pub use containers::{
    card, chamfered_panel, info_tile, tag_label, tile, CardStyle, NOTCH, TAG_LABEL_H,
};
pub use controls::{
    chamfered_button, slider, slider_value_from_x, toggle, ButtonVariant, SLIDER_BAR_H,
};
pub use numpad::{Numpad, NumpadAction, MAX_DIGITS};
