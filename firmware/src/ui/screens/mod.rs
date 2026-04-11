pub mod clock;
pub mod corner_test;
pub mod panel;
pub mod status;

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565};

use crate::events::SystemEvent;
use super::types::{Action, Screen, ScreenId, SystemData};

/// Home-row apps, in L/R carousel order. This is the canonical app
/// list - both the manager's quick-nav L/R swipe and the panel's app
/// picker draw from this slice. Adding a new app here wires it into
/// both pieces of navigation automatically.
pub const HOME_APPS: &[ScreenId] = &[
    ScreenId::Clock,
    ScreenId::Status,
];

/// Return the next or previous home app relative to `current`,
/// wrapping at the ends.
pub fn cycle_home_app(current: ScreenId, forward: bool) -> ScreenId {
    let idx = HOME_APPS.iter().position(|s| *s == current).unwrap_or(0);
    let len = HOME_APPS.len();
    let next = if forward {
        (idx + 1) % len
    } else {
        (idx + len - 1) % len
    };
    HOME_APPS[next]
}

/// Enum-based screen dispatch - avoids dynamic dispatch and heap allocation.
///
/// Add new screen variants here as they're created.
pub enum ActiveScreen {
    Clock(clock::ClockScreen),
    Status(status::StatusScreen),
    CornerTest(corner_test::CornerTestScreen),
    Panel(panel::PanelScreen),
}

impl ActiveScreen {
    /// Create a fresh screen for the given id.
    ///
    /// Note: `ScreenId::Panel` can't be constructed this way - the
    /// panel needs a `previous: ScreenId` context that plain id-based
    /// construction can't supply. Use `new_panel(previous)` instead.
    /// If `ScreenId::Panel` is passed here we fall back to Clock
    /// rather than panicking.
    pub fn new(id: ScreenId) -> Self {
        match id {
            ScreenId::Clock => Self::Clock(clock::ClockScreen::new()),
            ScreenId::Status => Self::Status(status::StatusScreen::new()),
            ScreenId::CornerTest => Self::CornerTest(corner_test::CornerTestScreen::new()),
            ScreenId::Panel => Self::Clock(clock::ClockScreen::new()),
        }
    }

    /// Create the panel screen, remembering which screen it should
    /// return to on close.
    pub fn new_panel(previous: ScreenId) -> Self {
        Self::Panel(panel::PanelScreen::new(previous))
    }

    pub fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        match self {
            Self::Clock(s) => s.render(display, data),
            Self::Status(s) => s.render(display, data),
            Self::CornerTest(s) => s.render(display, data),
            Self::Panel(s) => s.render(display, data),
        }
    }

    pub fn on_event(&mut self, event: &SystemEvent, data: &SystemData) -> Action {
        match self {
            Self::Clock(s) => s.on_event(event, data),
            Self::Status(s) => s.on_event(event, data),
            Self::CornerTest(s) => s.on_event(event, data),
            Self::Panel(s) => s.on_event(event, data),
        }
    }

    /// Which screen is currently active.
    pub fn id(&self) -> ScreenId {
        match self {
            Self::Clock(_) => ScreenId::Clock,
            Self::Status(_) => ScreenId::Status,
            Self::CornerTest(_) => ScreenId::CornerTest,
            Self::Panel(_) => ScreenId::Panel,
        }
    }

    /// Switch to a different screen.
    pub fn switch_to(&mut self, id: ScreenId) {
        *self = Self::new(id);
    }
}
