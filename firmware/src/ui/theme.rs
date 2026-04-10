//! Deus Ex: Mankind Divided inspired palette and layout constants.
//!
//! Dominant amber on true-black with white data values. Teal for the
//! battery accent, green for success, red for warnings. Pure black
//! background keeps AMOLED pixels off when idle.

use embedded_graphics::pixelcolor::Rgb565;

// -- Palette -----------------------------------------------------------------

/// Background: true black so inactive AMOLED pixels are fully off.
pub const BG:         Rgb565 = Rgb565::new(0, 0, 0);
/// Subtle panel fill for overlays (barely above pure black).
pub const PANEL_BG:   Rgb565 = Rgb565::new(2, 4, 2);
/// Primary accent: Jensen amber. Used for labels, headings, brackets.
pub const AMBER:      Rgb565 = Rgb565::new(29, 40, 0);
/// Highlighted amber for active/selected elements.
pub const AMBER_HI:   Rgb565 = Rgb565::new(31, 48, 2);
/// Dim amber for inactive borders, dividers, empty bars.
pub const AMBER_DIM:  Rgb565 = Rgb565::new(12, 16, 0);
/// Primary white for data values and selected rows.
pub const TEXT_WHITE: Rgb565 = Rgb565::new(31, 63, 31);
/// Secondary/dim text.
pub const TEXT_DIM:   Rgb565 = Rgb565::new(16, 32, 16);
/// Sparse teal accent used for the battery meter.
pub const TEAL:       Rgb565 = Rgb565::new(8, 48, 28);
/// Dim teal for the battery meter trough.
pub const TEAL_DIM:   Rgb565 = Rgb565::new(2, 14, 8);
/// Success state ("ACCESS GRANTED").
pub const GREEN:      Rgb565 = Rgb565::new(0, 48, 0);
/// Warning/danger state.
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
// still draw into the corner zones, but content placed there needs
// to stay horizontally centered enough to clear the rounded bezel.

/// Full-width-safe content band starts here.
pub const CONTENT_TOP: i32 = CORNER_R;
/// Full-width-safe content band ends here.
pub const CONTENT_BOTTOM: i32 = (SCREEN_H as i32) - CORNER_R;
/// Full-width-safe content band height (306 px).
pub const CONTENT_H: i32 = CONTENT_BOTTOM - CONTENT_TOP;

/// Side margin for content area.
pub const MARGIN: i32 = 8;
/// Default corner radius for rounded panels and cards.
pub const CARD_RADIUS: u32 = 12;
