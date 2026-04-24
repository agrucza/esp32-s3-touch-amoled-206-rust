//! Numeric keypad widget - a 3x4 grid of digit buttons with
//! backspace and confirm, plus a digit buffer.
//!
//! Used by the timer (duration entry) and settings (time/date
//! entry). The widget handles rendering and hit testing. The
//! caller decides what the digits mean (duration, time, date).

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Line, PrimitiveStyle, Triangle},
    Drawable,
};

use crate::ui::{fonts, primitives, theme};

// -- Layout constants --------------------------------------------------------

/// Default top of the numpad button grid.
const DEFAULT_TOP: i32 = 150;
/// Button width.
const BTN_W: i32 = 90;
/// Button height.
const BTN_H: i32 = 52;
/// Horizontal gap between buttons.
const BTN_GAP_X: i32 = 10;
/// Vertical gap between buttons.
const BTN_GAP_Y: i32 = 8;
/// Corner radius for digit buttons.
const BTN_RADIUS: u32 = 12;
/// Total grid width: 3 buttons + 2 gaps.
const GRID_W: i32 = 3 * BTN_W + 2 * BTN_GAP_X;
/// Left edge of the grid (horizontally centered).
const GRID_X: i32 = (theme::SCREEN_W as i32 - GRID_W) / 2;

/// The 4x3 button labels. Row-major order.
const BUTTON_LABELS: [[&str; 3]; 4] = [
    ["1", "2", "3"],
    ["4", "5", "6"],
    ["7", "8", "9"],
    ["",  "0", ""],
];

// -- Public types ------------------------------------------------------------

/// Result of a numpad tap.
#[derive(Debug, Clone, Copy)]
pub enum NumpadAction {
    /// A digit 0-9 was tapped.
    Digit(u8),
    /// Backspace was tapped.
    Backspace,
    /// Confirm/OK was tapped.
    Confirm,
}

// -- Numpad struct -----------------------------------------------------------

/// Maximum number of digits the buffer can hold.
pub const MAX_DIGITS: usize = 8;

/// Reusable numeric keypad with a digit buffer.
pub struct Numpad {
    /// Entered digits, left-to-right. Interpreted right-to-left
    /// (calculator style) by the caller.
    pub digits: heapless::Vec<u8, MAX_DIGITS>,
    /// Maximum number of digits accepted.
    max_digits: usize,
    /// Top Y of the first button row.
    top_y: i32,
}

impl Numpad {
    /// Create a new numpad accepting up to `max_digits` digits.
    pub fn new(max_digits: usize) -> Self {
        Self {
            digits: heapless::Vec::new(),
            max_digits: max_digits.min(MAX_DIGITS),
            top_y: DEFAULT_TOP,
        }
    }

    /// Set a custom top Y for the button grid. Use this when there
    /// is extra content (e.g. day selector) between the header and
    /// the numpad.
    pub fn with_top(mut self, top_y: i32) -> Self {
        self.top_y = top_y;
        self
    }

    /// Clear all entered digits.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.digits.clear();
    }

    /// Pre-fill with digits from a slice, stripping leading zeros.
    /// Use this when opening the numpad with an existing value.
    pub fn prefill(&mut self, raw: &[u8]) {
        self.digits.clear();
        let mut started = false;
        for &d in raw {
            if d != 0 { started = true; }
            if started {
                if self.digits.len() >= self.max_digits { break; }
                let _ = self.digits.push(d);
            }
        }
    }

    /// Render the button grid (no header, no label - the caller
    /// draws those).
    pub fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D) {
        for row in 0..4 {
            for col in 0..3 {
                let (bx, by) = button_origin(self.top_y, row, col);
                let label = BUTTON_LABELS[row][col];

                if !label.is_empty() {
                    // Digit button: rounded rect border + label.
                    primitives::rounded_panel(
                        display,
                        bx, by, BTN_W, BTN_H, BTN_RADIUS,
                        None, Some(theme::SIGNAL),
                    );
                    let rect = embedded_graphics::primitives::Rectangle::new(
                        Point::new(bx, by),
                        Size::new(BTN_W as u32, BTN_H as u32),
                    );
                    fonts::draw_centered_in_rect(
                        display, &fonts::value(),
                        label, rect,
                        theme::SIGNAL,
                    );
                } else if row == 3 && col == 0 {
                    // Backspace: glyph only, no border.
                    draw_backspace_glyph(display, bx + BTN_W / 2, by + BTN_H / 2);
                } else if row == 3 && col == 2 {
                    // Confirm: glyph only, no border.
                    draw_confirm_glyph(display, bx + BTN_W / 2, by + BTN_H / 2);
                }
            }
        }
    }

    /// Hit-test a tap against the grid. Returns the action if a
    /// button was tapped, or `None` if the tap missed.
    pub fn hit_test(&self, x: u16, y: u16) -> Option<NumpadAction> {
        let px = x as i32;
        let py = y as i32;

        for row in 0..4 {
            for col in 0..3 {
                let (bx, by) = button_origin(self.top_y, row, col);
                if px >= bx && px < bx + BTN_W && py >= by && py < by + BTN_H {
                    return match BUTTON_LABELS[row][col] {
                        "" if row == 3 && col == 0 => Some(NumpadAction::Backspace),
                        "" if row == 3 && col == 2 => Some(NumpadAction::Confirm),
                        "" => None,
                        s => {
                            let d = s.as_bytes()[0] - b'0';
                            Some(NumpadAction::Digit(d))
                        }
                    };
                }
            }
        }
        None
    }

    /// Process a tap action, updating the digit buffer. Returns
    /// `true` if the buffer changed (caller should redraw).
    /// Does NOT handle Confirm - the caller decides what to do.
    pub fn apply(&mut self, action: NumpadAction) -> bool {
        match action {
            NumpadAction::Digit(d) => {
                if self.digits.len() < self.max_digits {
                    let _ = self.digits.push(d);
                    true
                } else {
                    false
                }
            }
            NumpadAction::Backspace => {
                self.digits.pop().is_some()
            }
            NumpadAction::Confirm => false,
        }
    }

    /// Pad the digit buffer to `max_digits` width (right-aligned)
    /// and return as an array.
    #[allow(dead_code)]
    pub fn padded(&self) -> [u8; MAX_DIGITS] {
        let mut p = [0u8; MAX_DIGITS];
        let offset = self.max_digits.saturating_sub(self.digits.len());
        for (i, &d) in self.digits.iter().enumerate() {
            p[offset + i] = d;
        }
        p
    }
}

// -- Private helpers ---------------------------------------------------------

/// Top-left corner of the button at (row, col).
fn button_origin(top_y: i32, row: usize, col: usize) -> (i32, i32) {
    let x = GRID_X + col as i32 * (BTN_W + BTN_GAP_X);
    let y = top_y + row as i32 * (BTN_H + BTN_GAP_Y);
    (x, y)
}

/// Small back chevron for the backspace button.
fn draw_backspace_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32,
) {
    let half = 8;
    let style = PrimitiveStyle::with_stroke(theme::SIGNAL, 2);
    Line::new(
        Point::new(cx + half, cy - half),
        Point::new(cx - half, cy),
    ).into_styled(style).draw(display).ok();
    Line::new(
        Point::new(cx - half, cy),
        Point::new(cx + half, cy + half),
    ).into_styled(style).draw(display).ok();
}

/// Small play/confirm triangle for the confirm button.
fn draw_confirm_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32,
) {
    let r = 8;
    Triangle::new(
        Point::new(cx - r / 2, cy - r),
        Point::new(cx - r / 2, cy + r),
        Point::new(cx + r, cy),
    )
    .into_styled(PrimitiveStyle::with_fill(theme::SIGNAL))
    .draw(display).ok();
}
