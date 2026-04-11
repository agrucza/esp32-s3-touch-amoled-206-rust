//! Modern smartwatch palette: warm amber on pure black, with white as
//! the primary text color and grey for secondary labels. Inspired by
//! contemporary travel/utility watch UI concepts (filled rounded
//! cards, amber pills, soft hierarchy).

use embedded_graphics::pixelcolor::Rgb565;

// -- Palette -----------------------------------------------------------------

/// Pure black so AMOLED pixels stay off.
pub const BG:         Rgb565 = Rgb565::new(0, 0, 0);
/// Subtle dark grey for filled cards / panel surfaces - barely above
/// black but visibly raised against the BG.
pub const PANEL_BG:   Rgb565 = Rgb565::new(4, 8, 4);
/// Warm orange-amber (~#FFAA00). The hero accent color.
pub const AMBER:      Rgb565 = Rgb565::new(31, 42, 0);
/// Dim amber for inactive borders, dim bar troughs.
pub const AMBER_DIM:  Rgb565 = Rgb565::new(12, 18, 0);
/// Primary text color: pure white for titles and key data values.
pub const TEXT_WHITE: Rgb565 = Rgb565::new(31, 63, 31);
/// Secondary text: medium grey (~#808080) for labels and captions.
pub const TEXT_DIM:   Rgb565 = Rgb565::new(16, 32, 16);
/// Tertiary text: darker grey for inactive/placeholder labels.
pub const TEXT_MUTED: Rgb565 = Rgb565::new(10, 20, 10);
/// Notification green (reserved for future notification dots).
#[allow(dead_code)]
pub const GREEN:      Rgb565 = Rgb565::new(0, 50, 10);
/// Warning red.
pub const RED:        Rgb565 = Rgb565::new(31, 0, 2);

// -- Screen geometry ---------------------------------------------------------

pub const SCREEN_W: u16 = 410;
pub const SCREEN_H: u16 = 502;

/// Bezel rounded-corner radius. No content should land outside this inset
/// from each corner.
pub const CORNER_R: i32 = 98;

// -- Layout zones ------------------------------------------------------------
//
// These describe the bezel-safe content band. Full-screen apps may
// still draw into the corner zones, but content placed there needs to
// stay horizontally centered enough to clear the rounded bezel.

/// Full-width-safe content band starts here.
pub const CONTENT_TOP: i32 = CORNER_R;
/// Full-width-safe content band ends here.
pub const CONTENT_BOTTOM: i32 = (SCREEN_H as i32) - CORNER_R;
/// Full-width-safe content band height (306 px).
pub const CONTENT_H: i32 = CONTENT_BOTTOM - CONTENT_TOP;

/// Side margin for content area.
pub const MARGIN: i32 = 8;
/// Default corner radius for rounded panels and cards.
pub const CARD_RADIUS: u32 = 16;

/// Depth (in pixels) of the system-gesture edge zone at the top and
/// bottom of the display. A swipe whose *start* y lands within this
/// many pixels of the top or bottom screen edge is classified as an
/// edge gesture (system-level, e.g. pull-down-to-open-panel) rather
/// than a content gesture. Kept deliberately tighter than the bezel
/// corner radius so only gestures actually starting near the edge
/// qualify - accidental brushes in the middle of the screen should
/// be classified as content.
pub const EDGE_GESTURE_ZONE: i32 = 48;
