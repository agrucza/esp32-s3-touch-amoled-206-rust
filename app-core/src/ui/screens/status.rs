//! Status screen - paginated sensor view using the unified layout
//! system (header bar + card stack).
//!
//! Each page presents its data as a stack of value cards using the
//! shared `card` + `value_body` widgets. The header bar shows a
//! Close icon on the left and the current page title on the right.
//!
//! Pages (swipe up/down to navigate):
//! - Page 0: ACCEL  (X / Y / Z value cards)
//! - Page 1: GYRO   (X / Y / Z value cards)
//! - Page 2: ENV    (TEMP and TOUCH cards)
//!
//! Navigation: tapping the X icon in the header closes the screen
//! and returns via `Action::Back`. Swipe-down-to-close is not used
//! here because up/down swipes are reserved for page navigation.

use core::fmt::Write;

use embedded_graphics::{
    draw_target::DrawTarget,
    pixelcolor::Rgb565,
};

use crate::events::{SwipeDir, SwipeRegion, SystemEvent};
use crate::ui::{layout, theme};
use crate::ui::types::{Action, Screen, SystemData};
use crate::ui::widgets::{card, header_bar, page_scrollbar, value_body, CardStyle, HeaderIcon};

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
        // -- Header bar (X close left, page title right) ------------------
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Close,
            PAGE_TITLES[self.page as usize],
            theme::AMBER,
        );

        // -- Page-specific cards ------------------------------------------
        match self.page {
            0 => render_axes_cards(display, data.motion.accel_x, data.motion.accel_y, data.motion.accel_z),
            1 => render_axes_cards(display, data.motion.gyro_x, data.motion.gyro_y, data.motion.gyro_z),
            2 => render_env_cards(display, data),
            _ => {}
        }

        // -- Page scrollbar -----------------------------------------------
        page_scrollbar(display, PAGE_COUNT as usize, self.page as usize);
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &mut SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,

            // Accel/gyro pages show live motion data.
            SystemEvent::MotionUpdated { .. } if self.page < 2 => Action::Redraw,

            // Header icon (X): close and return to previous screen.
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                Action::Back
            }

            // ENV page shows live touch coords.
            SystemEvent::TouchPressed { .. } if self.page == 2 => Action::Redraw,
            SystemEvent::TouchReleased if self.page == 2 => Action::Redraw,

            // Up/down content swipes cycle pages.
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

// -- Card rendering helpers --------------------------------------------------

/// Three stacked value cards for X / Y / Z axes.
fn render_axes_cards<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    vx: i16, vy: i16, vz: i16,
) {
    let labels = ["X", "Y", "Z"];
    let values = [vx, vy, vz];
    let mut buf: heapless::String<8> = heapless::String::new();

    for (i, (label, val)) in labels.iter().zip(values.iter()).enumerate() {
        buf.clear();
        write!(buf, "{}", val).ok();
        let rect = layout::content_card_rect(i);
        card(display, rect, CardStyle::DEFAULT);
        value_body(display, rect, label, &buf, theme::TEXT_WHITE);
    }
}

/// Two value cards for the environment page (TEMP and TOUCH).
fn render_env_cards<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, data: &SystemData,
) {
    let temp_c = data.motion.temp_raw / 256;
    let mut temp_buf: heapless::String<12> = heapless::String::new();
    let _ = write!(temp_buf, "{} C", temp_c);
    let rect = layout::content_card_rect(0);
    card(display, rect, CardStyle::DEFAULT);
    value_body(display, rect, "TEMP", &temp_buf, theme::TEXT_WHITE);

    let mut touch_buf: heapless::String<24> = heapless::String::new();
    let touch_value: &str = match (data.touch.x, data.touch.y) {
        (Some(tx), Some(ty)) => {
            let _ = write!(touch_buf, "{}, {}", tx, ty);
            touch_buf.as_str()
        }
        _ => "NO CONTACT",
    };
    let rect = layout::content_card_rect(1);
    card(display, rect, CardStyle::DEFAULT);
    value_body(display, rect, "TOUCH", touch_value, theme::TEXT_WHITE);
}
