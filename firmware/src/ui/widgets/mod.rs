//! Reusable UI widgets.
//!
//! Three layers, each in its own sub-module:
//!
//! * [`containers`] - visual wrappers (cards, panels). Define the
//!   visual frame a piece of content lives inside. Never drawn
//!   content of their own beyond their own decoration.
//! * [`bodies`] - content layouts (label + value, icon + label, etc.)
//!   that draw into a `Rectangle`. Can be composed with a container
//!   by drawing both into the same rect, or used standalone on a
//!   bare rect without any wrapping panel.
//! * [`chrome`] - screen-level decorations (header bar, nav hints).
//!   Things that belong to a screen's outer frame rather than its
//!   content blocks.
//!
//! The split is about *reusability tier*, not widget kind. Every
//! card container is the same card; every body helper works inside
//! any container; every chrome piece works on any screen.
//!
//! Typical usage inside a screen's `render`:
//!
//! ```ignore
//! use crate::ui::widgets::{card, value_body, CardStyle};
//! use embedded_graphics::{geometry::{Point, Size}, primitives::Rectangle};
//!
//! let r = Rectangle::new(Point::new(35, 120), Size::new(340, 80));
//! card(display, r, CardStyle::DEFAULT);
//! value_body(display, r, "ACCEL X", "721 mg", theme::TEXT_WHITE);
//! ```

pub mod containers;
pub mod bodies;
pub mod chrome;

pub use containers::{card, CardStyle};
pub use bodies::value_body;
pub use chrome::{header_bar, HeaderIcon, HEADER_ICON_HIT_WIDTH};
