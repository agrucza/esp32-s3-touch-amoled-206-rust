//! Settings screen - device configuration and diagnostics, organised
//! by hardware subsystem.
//!
//! Uses the **internal state machine** pattern (see module docs on
//! `ui::types::Screen`): one [`SettingsScreen`] struct holds a
//! [`SettingsView`] enum that tracks which sub-view is currently
//! shown. Tapping a row in the Index sub-view switches `view` to
//! the corresponding device sub-view; tapping the back chevron in
//! a device sub-view switches `view` back to `Index`. None of this
//! is visible outside the screen - from `ActiveScreen`'s point of
//! view there is exactly one `ScreenId::Settings`.
//!
//! Adding a new device sub-view (RTC, Power, Touch, ...) is:
//! 1. Add a variant to [`SettingsView`].
//! 2. Add an entry to [`INDEX_ROWS`] so it appears in the Index.
//! 3. Add render + event match arms for the new variant.
//!
//! No changes to `ScreenId`, no global navigation plumbing, no
//! lifecycle hook rewiring.
//!
//! Visual style matches the widget-layer "All Bookings" reference -
//! all content is built from [`card`] + [`value_body`] + [`header_bar`].

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::Rgb565,
};
use heapless::String;
use core::fmt::Write;

use crate::ui::layout;
use crate::ui::theme;
use crate::ui::types::{
    Action, Screen, SelfTestId, SelfTestResult, SystemData, SystemEvent,
};
use crate::ui::widgets::{card, header_bar, value_body, CardStyle, HeaderIcon};

// -- View enum ---------------------------------------------------------------

/// Which sub-view the Settings screen is currently showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsView {
    /// Top-level index list of device sub-views.
    Index,
    /// QMI8658 6-axis IMU sub-view: self-tests and (future) config.
    Imu,
}

// -- Index row metadata ------------------------------------------------------

/// One row in the Settings Index list. Each row has a descriptive
/// label (shown as the card's small-label), a specific chip/device
/// identifier (shown as the card's value), and the sub-view it
/// opens on tap.
struct IndexRow {
    label: &'static str,
    chip: &'static str,
    target: SettingsView,
}

const INDEX_ROWS: &[IndexRow] = &[
    IndexRow {
        label: "6-AXIS IMU",
        chip: "QMI8658",
        target: SettingsView::Imu,
    },
    // Future:
    // IndexRow { label: "REAL-TIME CLOCK", chip: "PCF85063", target: SettingsView::Rtc },
    // IndexRow { label: "POWER MGMT",      chip: "AXP2101",  target: SettingsView::Power },
    // IndexRow { label: "TOUCH",           chip: "FT3168",   target: SettingsView::Touch },
    // IndexRow { label: "DISPLAY",         chip: "CO5300",   target: SettingsView::Display },
];

// -- IMU sub-view test list --------------------------------------------------

/// One row in the IMU sub-view. Each entry is a self-test card: the
/// label shown at the top of the card, the [`SelfTestId`] that
/// identifies it in the self-test array, and the physical unit used
/// to format its 3-axis result.
struct ImuTestRow {
    label: &'static str,
    id: SelfTestId,
    unit: &'static str,
}

const IMU_TESTS: &[ImuTestRow] = &[
    ImuTestRow {
        label: "ACCEL SELF-TEST",
        id: SelfTestId::ImuAccel,
        unit: "mg",
    },
    ImuTestRow {
        label: "GYRO SELF-TEST",
        id: SelfTestId::ImuGyro,
        unit: "dps",
    },
];

// -- SettingsScreen ----------------------------------------------------------

pub struct SettingsScreen {
    view: SettingsView,
}

impl SettingsScreen {
    pub fn new() -> Self {
        Self { view: SettingsView::Index }
    }
}

// -- Screen impl -------------------------------------------------------------

impl Screen for SettingsScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        match self.view {
            SettingsView::Index => self.render_index(display),
            SettingsView::Imu => self.render_imu(display, data),
        }
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &SystemData) -> Action {
        if matches!(event, SystemEvent::PowerButtonLong) {
            return Action::Shutdown;
        }

        match self.view {
            SettingsView::Index => self.index_event(event),
            SettingsView::Imu => self.imu_event(event),
        }
    }
}

// -- Index sub-view ----------------------------------------------------------

impl SettingsScreen {
    fn render_index<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Close,
            "SETTINGS",
            theme::AMBER,
        );

        for (i, row) in INDEX_ROWS.iter().enumerate() {
            let rect = layout::content_card_rect(i);
            card(display, rect, CardStyle::DEFAULT);
            value_body(display, rect, row.label, row.chip, theme::TEXT_WHITE);
        }
    }

    fn index_event(&mut self, event: &SystemEvent) -> Action {
        match event {
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                Action::Back
            }
            SystemEvent::Tap { x, y } => {
                for (i, row) in INDEX_ROWS.iter().enumerate() {
                    if layout::content_card_rect(i)
                        .contains(Point::new(*x as i32, *y as i32))
                    {
                        self.view = row.target;
                        return Action::Redraw;
                    }
                }
                Action::None
            }
            _ => Action::None,
        }
    }
}

// -- IMU sub-view ------------------------------------------------------------

impl SettingsScreen {
    fn render_imu<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "6-AXIS IMU",
            theme::AMBER,
        );

        for (i, test) in IMU_TESTS.iter().enumerate() {
            let rect = layout::content_card_rect(i);
            let result = data.self_tests[test.id as usize];

            let (value_buf, value_color, dot) = format_result(&result, test.unit);
            let style = match dot {
                Some(color) => CardStyle::DEFAULT.with_status_dot(color),
                None => CardStyle::DEFAULT,
            };

            // Dimmed while running - prevents double-taps and gives
            // visible feedback that the test is in flight.
            if matches!(result, SelfTestResult::Running) {
                card(display, rect, dimmed(style));
            } else {
                card(display, rect, style);
            }

            value_body(display, rect, test.label, value_buf.as_str(), value_color);
        }
    }

    fn imu_event(&mut self, event: &SystemEvent) -> Action {
        match event {
            // Back chevron tap: return to Index.
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            // Swipe right to go back (common back gesture).
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Index;
                Action::Redraw
            }

            SystemEvent::Tap { x, y } => {
                for (i, test) in IMU_TESTS.iter().enumerate() {
                    if !layout::content_card_rect(i)
                        .contains(Point::new(*x as i32, *y as i32))
                    {
                        continue;
                    }
                    return Action::RunSelfTest(test.id);
                }
                Action::None
            }

            SystemEvent::SelfTestUpdated { .. } => Action::Redraw,

            _ => Action::None,
        }
    }
}

// -- Result formatting -------------------------------------------------------

/// Format a [`SelfTestResult`] into a display string, a value color,
/// and an optional status-dot color for the card.
///
/// Returns a fixed-size `heapless::String` so there's no allocation
/// and the caller can render it without lifetimes.
fn format_result(
    result: &SelfTestResult,
    unit: &'static str,
) -> (String<32>, Rgb565, Option<Rgb565>) {
    let mut buf: String<32> = String::new();
    match result {
        SelfTestResult::NotRun => {
            let _ = buf.push_str("--");
            (buf, theme::TEXT_MUTED, None)
        }
        SelfTestResult::Running => {
            let _ = buf.push_str("RUNNING");
            (buf, theme::TEXT_DIM, Some(theme::AMBER))
        }
        SelfTestResult::PassAxes3(v) => {
            let _ = write!(&mut buf, "{} {} {} {}", v[0], v[1], v[2], unit);
            (buf, theme::TEXT_WHITE, Some(theme::GREEN))
        }
        SelfTestResult::FailAxes3(v) => {
            let _ = write!(&mut buf, "{} {} {} {}", v[0], v[1], v[2], unit);
            (buf, theme::RED, Some(theme::RED))
        }
        SelfTestResult::Error(_) => {
            let _ = buf.push_str("ERROR");
            (buf, theme::RED, Some(theme::RED))
        }
    }
}

/// Return a dimmed copy of a card style - used while a test is
/// running so the card reads as disabled. Uses [`theme::TEXT_MUTED`]
/// as the background so the panel shifts noticeably darker without
/// vanishing into the screen bg.
fn dimmed(mut style: CardStyle) -> CardStyle {
    style.bg = theme::TEXT_MUTED;
    style
}
