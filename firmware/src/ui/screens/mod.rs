pub mod clock;
pub mod corner_test;
pub mod status;

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565};

use crate::events::SystemEvent;
use super::types::{Action, Screen, ScreenId, SystemData};

/// Enum-based screen dispatch - avoids dynamic dispatch and heap allocation.
///
/// Add new screen variants here as they're created.
pub enum ActiveScreen {
    Clock(clock::ClockScreen),
    Status(status::StatusScreen),
    CornerTest(corner_test::CornerTestScreen),
}

impl ActiveScreen {
    /// Create a fresh screen for the given id.
    pub fn new(id: ScreenId) -> Self {
        match id {
            ScreenId::Clock => Self::Clock(clock::ClockScreen::new()),
            ScreenId::Status => Self::Status(status::StatusScreen::new()),
            ScreenId::CornerTest => Self::CornerTest(corner_test::CornerTestScreen::new()),
        }
    }

    pub fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        match self {
            Self::Clock(s) => s.render(display, data),
            Self::Status(s) => s.render(display, data),
            Self::CornerTest(s) => s.render(display, data),
        }
    }

    pub fn on_event(&mut self, event: &SystemEvent, data: &SystemData) -> Action {
        match self {
            Self::Clock(s) => s.on_event(event, data),
            Self::Status(s) => s.on_event(event, data),
            Self::CornerTest(s) => s.on_event(event, data),
        }
    }

    /// Which screen is currently active.
    pub fn id(&self) -> ScreenId {
        match self {
            Self::Clock(_) => ScreenId::Clock,
            Self::Status(_) => ScreenId::Status,
            Self::CornerTest(_) => ScreenId::CornerTest,
        }
    }

    /// Switch to a different screen.
    pub fn switch_to(&mut self, id: ScreenId) {
        *self = Self::new(id);
    }
}
