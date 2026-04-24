//! Settings screen - device configuration and diagnostics, organised
//! by hardware subsystem.
//!
//! Uses the **internal state machine** pattern: one [`SettingsScreen`]
//! struct holds a [`SettingsView`] enum that tracks which sub-view is
//! currently shown. Tapping a row in the Index sub-view switches
//! `view` to the corresponding sub-view; tapping the back chevron
//! returns to Index.
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

use crate::ui::{fonts, layout, theme};
use crate::ui::types::{
    Action, Screen, SelfTestId, SelfTestResult, SystemData, SystemEvent,
};
use crate::ui::widgets::{
    card, header_bar, value_body, CardStyle, HeaderIcon, Numpad, NumpadAction, MAX_DIGITS,
};

// -- View enum ---------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsView {
    Index,
    Imu,
    Clock,
    TimeEntry,
    DateEntry,
    /// Storage sub-index. Routes to the storage leaves below.
    Storage,
    StorageFlash,
    StorageSd,
    StorageRestoreFlash,
    StorageFactoryReset,
}

// -- Index row metadata ------------------------------------------------------

struct IndexRow {
    label: &'static str,
    value_fn: fn(&SystemData) -> String<20>,
    target: SettingsView,
}

fn clock_value(data: &SystemData) -> String<20> {
    let mut buf = String::new();
    let _ = write!(buf, "{:02}:{:02}:{:02}", data.time.hour, data.time.minute, data.time.second);
    buf
}

fn imu_value(_data: &SystemData) -> String<20> {
    let mut buf = String::new();
    let _ = buf.push_str("QMI8658");
    buf
}

fn storage_value(data: &SystemData) -> String<20> {
    // Summary shown on the top-level settings index: "<files> / <size>K".
    let mut buf = String::new();
    let _ = write!(
        buf,
        "{} / {}K",
        data.storage.files,
        data.storage.total_bytes / 1024,
    );
    buf
}

// -- Storage sub-index rows --------------------------------------------------
//
// Same IndexRow pattern as the top-level settings index, one level
// deeper. Each row taps into a storage leaf view.

fn storage_flash_value(data: &SystemData) -> String<20> {
    let mut buf = String::new();
    let _ = write!(
        buf,
        "{} FILES / {}K",
        data.storage.files,
        data.storage.total_bytes / 1024,
    );
    buf
}

fn storage_sd_value(data: &SystemData) -> String<20> {
    let mut buf = String::new();
    let _ = buf.push_str(if data.storage.sd_online { "ONLINE" } else { "NOT PRESENT" });
    buf
}

fn storage_reset_value(_data: &SystemData) -> String<20> {
    String::new()
}

fn storage_restore_value(data: &SystemData) -> String<20> {
    let mut buf = String::new();
    let _ = buf.push_str(if data.storage.sd_online { "" } else { "SD NOT PRESENT" });
    buf
}

const STORAGE_INDEX_ROWS: &[IndexRow] = &[
    IndexRow {
        label: "FLASH",
        value_fn: storage_flash_value,
        target: SettingsView::StorageFlash,
    },
    IndexRow {
        label: "SD CARD",
        value_fn: storage_sd_value,
        target: SettingsView::StorageSd,
    },
    IndexRow {
        label: "RESTORE FROM SD",
        value_fn: storage_restore_value,
        target: SettingsView::StorageRestoreFlash,
    },
    IndexRow {
        label: "FACTORY RESET",
        value_fn: storage_reset_value,
        target: SettingsView::StorageFactoryReset,
    },
];

const INDEX_ROWS: &[IndexRow] = &[
    IndexRow {
        label: "CLOCK",
        value_fn: clock_value,
        target: SettingsView::Clock,
    },
    IndexRow {
        label: "6-AXIS IMU",
        value_fn: imu_value,
        target: SettingsView::Imu,
    },
    IndexRow {
        label: "STORAGE",
        value_fn: storage_value,
        target: SettingsView::Storage,
    },
];

// -- IMU sub-view test list --------------------------------------------------

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

// -- Numpad time label Y (same as timer) -------------------------------------

const NUMPAD_TIME_Y: i32 = 90;

// -- SettingsScreen ----------------------------------------------------------

pub struct SettingsScreen {
    view: SettingsView,
    numpad: Numpad,
}

impl SettingsScreen {
    pub fn new() -> Self {
        Self {
            view: SettingsView::Index,
            numpad: Numpad::new(6),
        }
    }
}

// -- Screen impl -------------------------------------------------------------

impl Screen for SettingsScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        match self.view {
            SettingsView::Index => self.render_index(display, data),
            SettingsView::Imu => self.render_imu(display, data),
            SettingsView::Clock => self.render_clock(display, data),
            SettingsView::TimeEntry => self.render_time_entry(display, data),
            SettingsView::DateEntry => self.render_date_entry(display, data),
            SettingsView::Storage => self.render_storage_index(display, data),
            SettingsView::StorageFlash => self.render_storage_flash(display, data),
            SettingsView::StorageSd => self.render_storage_sd(display, data),
            SettingsView::StorageRestoreFlash => self.render_storage_restore(display, data),
            SettingsView::StorageFactoryReset => self.render_storage_factory_reset(display, data),
        }
    }

    fn on_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        if matches!(event, SystemEvent::PowerButtonLong) {
            return Action::Shutdown;
        }

        match self.view {
            SettingsView::Index => self.index_event(event),
            SettingsView::Imu => self.imu_event(event),
            SettingsView::Clock => self.clock_event(event, data),
            SettingsView::TimeEntry => self.time_entry_event(event, data),
            SettingsView::DateEntry => self.date_entry_event(event, data),
            SettingsView::Storage => self.storage_index_event(event),
            SettingsView::StorageFlash => self.storage_flash_event(event),
            SettingsView::StorageSd => self.storage_sd_event(event),
            SettingsView::StorageRestoreFlash => self.storage_restore_event(event, data),
            SettingsView::StorageFactoryReset => self.storage_factory_reset_event(event),
        }
    }
}

// -- Index sub-view ----------------------------------------------------------

impl SettingsScreen {
    fn render_index<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData,
    ) {
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
            let val = (row.value_fn)(data);
            value_body(display, rect, row.label, val.as_str(), theme::TEXT_WHITE);
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

// -- Clock sub-view (time + date cards) --------------------------------------

impl SettingsScreen {
    fn render_clock<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "CLOCK",
            theme::AMBER,
        );

        // Time card.
        let rect = layout::content_card_rect(0);
        card(display, rect, CardStyle::DEFAULT);
        let mut time_buf: String<12> = String::new();
        let _ = write!(time_buf, "{:02}:{:02}:{:02}",
            data.time.hour, data.time.minute, data.time.second);
        value_body(display, rect, "TIME", time_buf.as_str(), theme::TEXT_WHITE);

        // Date card.
        let rect = layout::content_card_rect(1);
        card(display, rect, CardStyle::DEFAULT);
        let mut date_buf: String<12> = String::new();
        let _ = write!(date_buf, "{:02}.{:02}.{:04}",
            data.time.day, data.time.month, data.time.year);
        value_body(display, rect, "DATE", date_buf.as_str(), theme::TEXT_WHITE);
    }

    fn clock_event(&mut self, event: &SystemEvent, data: &SystemData) -> Action {
        match event {
            // Keep the display fresh.
            SystemEvent::TimeUpdated { .. } => Action::Redraw,

            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                if layout::content_card_rect(0)
                    .contains(Point::new(*x as i32, *y as i32))
                {
                    // Open time numpad, pre-fill with current time.
                    let t = &data.time;
                    self.numpad = Numpad::new(6);
                    self.numpad.prefill(&[
                        t.hour / 10, t.hour % 10,
                        t.minute / 10, t.minute % 10,
                        t.second / 10, t.second % 10,
                    ]);
                    self.view = SettingsView::TimeEntry;
                    Action::Redraw
                } else if layout::content_card_rect(1)
                    .contains(Point::new(*x as i32, *y as i32))
                {
                    // Open date numpad, pre-fill with current date.
                    let t = &data.time;
                    self.numpad = Numpad::new(8);
                    self.numpad.prefill(&[
                        t.day / 10, t.day % 10,
                        t.month / 10, t.month % 10,
                        (t.year / 1000) as u8, ((t.year / 100) % 10) as u8,
                        ((t.year / 10) % 10) as u8, (t.year % 10) as u8,
                    ]);
                    self.view = SettingsView::DateEntry;
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            _ => Action::None,
        }
    }
}

// -- Time entry numpad -------------------------------------------------------

impl SettingsScreen {
    fn render_time_entry<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, _data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "SET TIME",
            theme::AMBER,
        );

        // HH:MM:SS label from digits.
        let p = pad_digits(&self.numpad.digits, 6);
        let mut buf: String<12> = String::new();
        let _ = write!(buf, "{}{}{}{}{}{}{}{}",
            p[0], p[1], ':', p[2], p[3], ':', p[4], p[5]);
        fonts::draw_centered(
            display, &fonts::value(),
            &buf,
            theme::SCREEN_W as i32 / 2, NUMPAD_TIME_Y,
            theme::AMBER,
        );

        self.numpad.render(display);
    }

    fn time_entry_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                self.numpad = Numpad::new(6);
                self.view = SettingsView::Clock;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                if let Some(action) = self.numpad.hit_test(*x, *y) {
                    match action {
                        NumpadAction::Confirm => {
                            let p = pad_digits(&self.numpad.digits, 6);
                            let h = p[0] * 10 + p[1];
                            let m = p[2] * 10 + p[3];
                            let s = p[4] * 10 + p[5];
                            // Validate.
                            if h < 24 && m < 60 && s < 60 {
                                self.numpad = Numpad::new(6);
                                self.view = SettingsView::Clock;
                                return Action::SetTime {
                                    year: data.time.year,
                                    month: data.time.month,
                                    day: data.time.day,
                                    hour: h,
                                    minute: m,
                                    second: s,
                                };
                            }
                            // Invalid - just redraw (user can see the bad value).
                            Action::Redraw
                        }
                        other => {
                            if self.numpad.apply(other) {
                                Action::Redraw
                            } else {
                                Action::None
                            }
                        }
                    }
                } else {
                    Action::None
                }
            }
            _ => Action::None,
        }
    }
}

// -- Date entry numpad -------------------------------------------------------

impl SettingsScreen {
    fn render_date_entry<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, _data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "SET DATE",
            theme::AMBER,
        );

        // DD.MM.YYYY label from digits.
        let p = pad_digits(&self.numpad.digits, 8);
        let mut buf: String<12> = String::new();
        let _ = write!(buf, "{}{}.{}{}.{}{}{}{}",
            p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]);
        fonts::draw_centered(
            display, &fonts::value(),
            &buf,
            theme::SCREEN_W as i32 / 2, NUMPAD_TIME_Y,
            theme::AMBER,
        );

        self.numpad.render(display);
    }

    fn date_entry_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                self.numpad = Numpad::new(6);
                self.view = SettingsView::Clock;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                if let Some(action) = self.numpad.hit_test(*x, *y) {
                    match action {
                        NumpadAction::Confirm => {
                            let p = pad_digits(&self.numpad.digits, 8);
                            let d = p[0] * 10 + p[1];
                            let m = p[2] * 10 + p[3];
                            let y = p[4] as u16 * 1000 + p[5] as u16 * 100
                                  + p[6] as u16 * 10 + p[7] as u16;
                            // Basic validation.
                            if d >= 1 && d <= 31 && m >= 1 && m <= 12 && y >= 2000 && y <= 2099 {
                                self.numpad = Numpad::new(6);
                                self.view = SettingsView::Clock;
                                return Action::SetTime {
                                    year: y,
                                    month: m,
                                    day: d,
                                    hour: data.time.hour,
                                    minute: data.time.minute,
                                    second: data.time.second,
                                };
                            }
                            Action::Redraw
                        }
                        other => {
                            if self.numpad.apply(other) {
                                Action::Redraw
                            } else {
                                Action::None
                            }
                        }
                    }
                } else {
                    Action::None
                }
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
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
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

// -- Storage sub-views -------------------------------------------------------
//
// Two-level hierarchy:
//
//   Settings → Storage (sub-index) → { Flash | SD Card | Factory Reset }
//
// The sub-index mirrors the top-level settings index layout (one
// row per leaf). Each leaf is a focused view for its single
// concern. Back navigation from a leaf returns to the Storage
// sub-index; back from the sub-index returns to the Settings index.

impl SettingsScreen {
    // -- Storage sub-index ---------------------------------------------------

    fn render_storage_index<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "STORAGE",
            theme::AMBER,
        );

        for (i, row) in STORAGE_INDEX_ROWS.iter().enumerate() {
            let rect = layout::content_card_rect(i);
            card(display, rect, CardStyle::DEFAULT);
            let val = (row.value_fn)(data);
            value_body(display, rect, row.label, val.as_str(), theme::TEXT_WHITE);
        }
    }

    fn storage_index_event(&mut self, event: &SystemEvent) -> Action {
        match event {
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                let pt = Point::new(*x as i32, *y as i32);
                for (i, row) in STORAGE_INDEX_ROWS.iter().enumerate() {
                    if layout::content_card_rect(i).contains(pt) {
                        self.view = row.target;
                        return Action::Redraw;
                    }
                }
                Action::None
            }
            SystemEvent::StorageUsageUpdated { .. } => Action::Redraw,
            _ => Action::None,
        }
    }

    // -- Flash leaf (read-only info) -----------------------------------------

    fn render_storage_flash<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "FLASH",
            theme::AMBER,
        );

        let rect = layout::content_card_rect(0);
        card(display, rect, CardStyle::DEFAULT);
        let mut buf: String<32> = String::new();
        let _ = write!(
            buf,
            "{} FILES / {} KB",
            data.storage.files,
            data.storage.total_bytes / 1024,
        );
        value_body(display, rect, "USAGE", buf.as_str(), theme::TEXT_WHITE);
    }

    fn storage_flash_event(&mut self, event: &SystemEvent) -> Action {
        match event {
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::StorageUsageUpdated { .. } => Action::Redraw,
            _ => Action::None,
        }
    }

    // -- SD card leaf (status + tap to probe) --------------------------------

    fn render_storage_sd<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "SD CARD",
            theme::AMBER,
        );

        // Card 0: status line. Green dot when online, amber when
        // not. Read-only; the button below triggers the probe.
        let status_rect = layout::content_card_rect(0);
        let (dot, status_text, status_color) = if data.storage.sd_online {
            (theme::GREEN, "ONLINE",      theme::TEXT_WHITE)
        } else {
            (theme::AMBER, "NOT PRESENT", theme::AMBER)
        };
        card(display, status_rect, CardStyle::DEFAULT.with_status_dot(dot));
        value_body(display, status_rect, "STATUS", status_text, status_color);

        // Card 1: tap target to (re-)probe.
        let probe_rect = layout::content_card_rect(1);
        card(display, probe_rect, CardStyle::DEFAULT);
        let probe_text = if data.storage.sd_online { "TAP TO REPROBE" } else { "TAP TO INITIALIZE" };
        value_body(display, probe_rect, "PROBE", probe_text, theme::TEXT_WHITE);
    }

    fn storage_sd_event(&mut self, event: &SystemEvent) -> Action {
        match event {
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                let pt = Point::new(*x as i32, *y as i32);
                if layout::content_card_rect(1).contains(pt) {
                    return Action::InitSd;
                }
                Action::None
            }
            SystemEvent::StorageUsageUpdated { .. } => Action::Redraw,
            _ => Action::None,
        }
    }

    // -- Restore-from-SD leaf (destructive, gated on SD online) --------------

    fn render_storage_restore<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "RESTORE FROM SD",
            theme::AMBER,
        );

        // Card 0: summary of what happens. Always present so the
        // user has context whether SD is online or not.
        let warn_rect = layout::content_card_rect(0);
        card(display, warn_rect, CardStyle::DEFAULT.with_status_dot(theme::AMBER));
        value_body(
            display,
            warn_rect,
            "OVERWRITES",
            "FLASH CONFIG + REBOOT",
            theme::AMBER,
        );

        // Card 1: confirm target. Disabled (dimmed, different label)
        // when the SD mirror isn't online - matches the InitSd row's
        // gating idea but flipped: here offline is the blocker.
        let confirm_rect = layout::content_card_rect(1);
        if data.storage.sd_online {
            card(display, confirm_rect, CardStyle::DEFAULT.with_status_dot(theme::AMBER));
            value_body(
                display,
                confirm_rect,
                "CONFIRM",
                "TAP TO RESTORE",
                theme::AMBER,
            );
        } else {
            card(display, confirm_rect, dimmed(CardStyle::DEFAULT));
            value_body(
                display,
                confirm_rect,
                "CONFIRM",
                "SD NOT PRESENT",
                theme::TEXT_MUTED,
            );
        }
    }

    fn storage_restore_event(
        &mut self,
        event: &SystemEvent,
        data: &SystemData,
    ) -> Action {
        match event {
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                if !data.storage.sd_online {
                    return Action::None;
                }
                let pt = Point::new(*x as i32, *y as i32);
                if layout::content_card_rect(1).contains(pt) {
                    // No bounce-back - the manager will software-
                    // reset shortly after this returns, so the
                    // next frame never draws. Leaving `view` on
                    // StorageRestoreFlash is fine.
                    return Action::RestoreFromSd;
                }
                Action::None
            }
            SystemEvent::StorageUsageUpdated { .. } => Action::Redraw,
            _ => Action::None,
        }
    }

    // -- Factory reset leaf (destructive) ------------------------------------

    fn render_storage_factory_reset<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        _data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "FACTORY RESET",
            theme::RED,
        );

        // Card 0: warning summary (read-only).
        let warn_rect = layout::content_card_rect(0);
        card(display, warn_rect, CardStyle::DEFAULT.with_status_dot(theme::RED));
        value_body(
            display,
            warn_rect,
            "WARNING",
            "WIPES CONFIG + LOGS",
            theme::RED,
        );

        // Card 1: confirmation tap target.
        let confirm_rect = layout::content_card_rect(1);
        card(display, confirm_rect, CardStyle::DEFAULT.with_status_dot(theme::RED));
        value_body(
            display,
            confirm_rect,
            "CONFIRM",
            "TAP TO WIPE",
            theme::RED,
        );
    }

    fn storage_factory_reset_event(&mut self, event: &SystemEvent) -> Action {
        match event {
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                let pt = Point::new(*x as i32, *y as i32);
                if layout::content_card_rect(1).contains(pt) {
                    // Bounce back to Storage sub-index on confirm
                    // so the user sees the refreshed usage counts
                    // land naturally.
                    self.view = SettingsView::Storage;
                    return Action::FactoryReset;
                }
                Action::None
            }
            _ => Action::None,
        }
    }
}

// -- Helpers -----------------------------------------------------------------

/// Pad a digit slice to `len` digits, right-aligned with leading zeros.
fn pad_digits(digits: &[u8], len: usize) -> [u8; MAX_DIGITS] {
    let mut p = [0u8; MAX_DIGITS];
    let offset = len.saturating_sub(digits.len());
    for (i, &d) in digits.iter().enumerate() {
        if offset + i < 8 {
            p[offset + i] = d;
        }
    }
    p
}


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

fn dimmed(mut style: CardStyle) -> CardStyle {
    style.bg = theme::TEXT_MUTED;
    style
}
