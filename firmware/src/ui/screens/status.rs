//! Status screen - paginated sensor view in the modern smartwatch
//! style. Each page presents its data as a stack of "value cards":
//! filled rounded rectangles with a small grey label centered on top
//! and a large bold white value centered below (matches the concept's
//! "All Bookings" list pattern).
//!
//! Pages (swipe up/down to navigate):
//! - Page 0: ACCEL  (X / Y / Z value cards)
//! - Page 1: GYRO   (X / Y / Z value cards)
//! - Page 2: ENV    (TEMP and TOUCH cards)

use core::fmt::Write;

use embedded_graphics::{
    draw_target::DrawTarget,
    pixelcolor::Rgb565,
};

use crate::events::{SwipeDir, SwipeRegion, SystemEvent};
use crate::ui::{fonts, primitives, theme};
use crate::ui::types::{Action, Screen, SystemData};

const PAGE_COUNT: u8 = 3;
const PAGE_TITLES: [&str; PAGE_COUNT as usize] = [
    "ACCELEROMETER",
    "GYROSCOPE",
    "ENVIRONMENT",
];

pub struct StatusScreen {
    page: u8,
}

impl StatusScreen {
    pub fn new() -> Self { Self { page: 0 } }
}

impl Screen for StatusScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        let w = theme::SCREEN_W as i32;

        // Page title at the top - small grey headline.
        let title = PAGE_TITLES[self.page as usize];
        fonts::draw_centered(
            display, &fonts::headline(),
            title, w / 2, 80,
            theme::TEXT_DIM,
        );

        // Page-specific cards.
        match self.page {
            0 => render_axes_cards(display, data.motion.accel_x, data.motion.accel_y, data.motion.accel_z),
            1 => render_axes_cards(display, data.motion.gyro_x, data.motion.gyro_y, data.motion.gyro_z),
            2 => render_env_cards(display, data),
            _ => {}
        }

        // Vertical scrollbar on the right, spanning the full
        // bezel-safe content band.
        primitives::scrollbar_v(
            display,
            SCROLLBAR_X, theme::CONTENT_TOP, SCROLLBAR_W, theme::CONTENT_H,
            PAGE_COUNT as usize,
            self.page as usize,
            theme::AMBER,
            theme::AMBER_DIM,
        );
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,
            // ENV page shows live touch coords - redraw while a finger
            // is down so the TOUCH card tracks the drag.
            SystemEvent::TouchPressed { .. } if self.page == 2 => Action::Redraw,
            SystemEvent::TouchReleased if self.page == 2 => Action::Redraw,
            SystemEvent::Swipe { dir: SwipeDir::Up, region: SwipeRegion::Content } => {
                self.page = (self.page + 1) % PAGE_COUNT;
                Action::Redraw
            }
            SystemEvent::Swipe { dir: SwipeDir::Down, region: SwipeRegion::Content } => {
                self.page = (self.page + PAGE_COUNT - 1) % PAGE_COUNT;
                Action::Redraw
            }
            _ => Action::None,
        }
    }
}

// Card layout constants - shared by all status pages so the cards
// line up consistently between page transitions.
const CARD_X: i32 = 35;
const CARD_W: i32 = 340;
const CARD_H: i32 = 80;
const CARD_GAP: i32 = 12;
const FIRST_CARD_Y: i32 = 120;

// Vertical page-indicator scrollbar, sitting to the right of the
// card stack. Thin enough to stay clear of the bezel arc on the
// right edge (screen is 410 wide, bezel corner radius is 98).
const SCROLLBAR_W: i32 = 4;
const SCROLLBAR_X: i32 = theme::SCREEN_W as i32 - 18;

/// Filled rounded value card: small grey label centered on top, big
/// bold white value centered below. Matches the reference's
/// "All Bookings" list tile pattern.
fn draw_value_card<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32, w: i32, h: i32,
    label: &str, value: &str,
) {
    primitives::rounded_panel(
        display, x, y, w, h, theme::CARD_RADIUS,
        Some(theme::PANEL_BG), None,
    );

    // Label uses body (14 px) instead of caption (10 px) - at caption
    // size the label was visually lost next to the big value.
    fonts::draw_centered(
        display, &fonts::body(),
        label, x + w / 2, y + 16,
        theme::TEXT_DIM,
    );

    fonts::draw_centered(
        display, &fonts::value(),
        value, x + w / 2, y + 38,
        theme::TEXT_WHITE,
    );
}

/// Three stacked value cards for X / Y / Z axes.
fn render_axes_cards<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    vx: i16, vy: i16, vz: i16,
) {
    let mut buf: heapless::String<8> = heapless::String::new();
    let mut y = FIRST_CARD_Y;

    write!(buf, "{}", vx).ok();
    draw_value_card(display, CARD_X, y, CARD_W, CARD_H, "X", &buf);
    buf.clear();
    y += CARD_H + CARD_GAP;

    write!(buf, "{}", vy).ok();
    draw_value_card(display, CARD_X, y, CARD_W, CARD_H, "Y", &buf);
    buf.clear();
    y += CARD_H + CARD_GAP;

    write!(buf, "{}", vz).ok();
    draw_value_card(display, CARD_X, y, CARD_W, CARD_H, "Z", &buf);
}

/// Two value cards for the environment page (TEMP and TOUCH).
fn render_env_cards<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, data: &SystemData,
) {
    let mut y = FIRST_CARD_Y;

    let temp_c = data.motion.temp_raw / 256;
    let mut temp_buf: heapless::String<12> = heapless::String::new();
    let _ = write!(temp_buf, "{} C", temp_c);
    draw_value_card(display, CARD_X, y, CARD_W, CARD_H, "TEMP", &temp_buf);
    y += CARD_H + CARD_GAP;

    let mut touch_buf: heapless::String<24> = heapless::String::new();
    let touch_value: &str = match (data.touch.x, data.touch.y) {
        (Some(tx), Some(ty)) => {
            let _ = write!(touch_buf, "{}, {}", tx, ty);
            touch_buf.as_str()
        }
        _ => "NO CONTACT",
    };
    draw_value_card(display, CARD_X, y, CARD_W, CARD_H, "TOUCH", touch_value);
}
