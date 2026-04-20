pub mod alarm;
pub mod clock;
pub mod panel;
pub mod settings;
pub mod status;
pub mod stopwatch;
pub mod timer;

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565};

use crate::events::SystemEvent;
use super::types::{Action, Screen, ScreenId, SystemData};

/// Home-row apps, in L/R carousel order. The manager's quick-nav
/// left/right swipe cycles through this list. Keep it minimal -
/// only screens a user might want to reach by accident.
pub const HOME_APPS: &[ScreenId] = &[
    ScreenId::Clock,
    ScreenId::Status,
];

/// All apps reachable from the pull-down panel. Superset of
/// [`HOME_APPS`] - includes entries that should be reachable
/// deliberately but not through casual L/R swipes (Settings, future
/// File Explorer, etc.). The panel picker renders this list.
pub const PANEL_APPS: &[ScreenId] = &[
    ScreenId::Clock,
    ScreenId::Status,
    ScreenId::Stopwatch,
    ScreenId::Timer,
    ScreenId::Alarm,
    ScreenId::Settings,
];

/// Return the next or previous home app relative to `current`,
/// wrapping at the ends. Operates on [`HOME_APPS`] only - screens
/// that aren't home-row apps (Settings, Panel) don't participate
/// in L/R cycling and just return `current` unchanged.
pub fn cycle_home_app(current: ScreenId, forward: bool) -> ScreenId {
    let Some(idx) = HOME_APPS.iter().position(|s| *s == current) else {
        return current;
    };
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
    Stopwatch(stopwatch::StopwatchScreen),
    Timer(timer::TimerScreen),
    Alarm(alarm::AlarmScreen),
    Settings(settings::SettingsScreen),
    Panel(panel::PanelScreen),
}

impl ActiveScreen {
    /// Create a fresh screen for the given id.
    ///
    /// Note: `ScreenId::Panel` can't be constructed this way - the
    /// panel needs a `previous: ScreenId` context that plain id-based
    /// construction can't supply. Use `new_panel(previous)` instead.
    pub fn new(id: ScreenId) -> Self {
        match id {
            ScreenId::Clock => Self::Clock(clock::ClockScreen::new()),
            ScreenId::Status => Self::Status(status::StatusScreen::new()),
            ScreenId::Stopwatch => Self::Stopwatch(stopwatch::StopwatchScreen::new()),
            ScreenId::Timer => Self::Timer(timer::TimerScreen::new()),
            ScreenId::Alarm => Self::Alarm(alarm::AlarmScreen::new()),
            ScreenId::Settings => Self::Settings(settings::SettingsScreen::new()),
            ScreenId::Panel => {
                debug_assert!(false, "use ActiveScreen::new_panel(previous) for Panel");
                Self::Clock(clock::ClockScreen::new())
            }
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
            Self::Stopwatch(s) => s.render(display, data),
            Self::Timer(s) => s.render(display, data),
            Self::Alarm(s) => s.render(display, data),
            Self::Settings(s) => s.render(display, data),
            Self::Panel(s) => s.render(display, data),
        }
    }

    pub fn on_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match self {
            Self::Clock(s) => s.on_event(event, data),
            Self::Status(s) => s.on_event(event, data),
            Self::Stopwatch(s) => s.on_event(event, data),
            Self::Timer(s) => s.on_event(event, data),
            Self::Alarm(s) => s.on_event(event, data),
            Self::Settings(s) => s.on_event(event, data),
            Self::Panel(s) => s.on_event(event, data),
        }
    }

    pub fn mount(&mut self, data: &SystemData) {
        match self {
            Self::Clock(s) => s.on_mount(data),
            Self::Status(s) => s.on_mount(data),
            Self::Stopwatch(s) => s.on_mount(data),
            Self::Timer(s) => s.on_mount(data),
            Self::Alarm(s) => s.on_mount(data),
            Self::Settings(s) => s.on_mount(data),
            Self::Panel(s) => s.on_mount(data),
        }
    }

    pub fn unmount(&mut self) {
        match self {
            Self::Clock(s) => s.on_unmount(),
            Self::Status(s) => s.on_unmount(),
            Self::Stopwatch(s) => s.on_unmount(),
            Self::Timer(s) => s.on_unmount(),
            Self::Alarm(s) => s.on_unmount(),
            Self::Settings(s) => s.on_unmount(),
            Self::Panel(s) => s.on_unmount(),
        }
    }

    /// Which screen is currently active.
    pub fn id(&self) -> ScreenId {
        match self {
            Self::Clock(_) => ScreenId::Clock,
            Self::Status(_) => ScreenId::Status,
            Self::Stopwatch(_) => ScreenId::Stopwatch,
            Self::Timer(_) => ScreenId::Timer,
            Self::Alarm(_) => ScreenId::Alarm,
            Self::Settings(_) => ScreenId::Settings,
            Self::Panel(_) => ScreenId::Panel,
        }
    }

    /// Switch to a different screen.
    pub fn switch_to(&mut self, id: ScreenId, data: &SystemData) {
        self.unmount();
        *self = Self::new(id);
        self.mount(data);
    }

    /// Open the panel screen.
    pub fn open_panel(&mut self, previous: ScreenId, data: &SystemData) {
        self.unmount();
        *self = Self::new_panel(previous);
        self.mount(data);
    }
}

#[cfg(test)]
mod cycle_tests {
    use super::*;

    // HOME_APPS today is a 2-entry carousel: [Clock, Status]. All
    // cycle tests are written against that length rather than
    // hard-coded pairs so adding a new home-row entry later doesn't
    // silently break these expectations.

    #[test]
    fn forward_moves_to_next_home_app() {
        // Clock (idx 0) -> Status (idx 1).
        assert_eq!(cycle_home_app(HOME_APPS[0], true), HOME_APPS[1]);
    }

    #[test]
    fn backward_moves_to_previous_home_app() {
        // Status (idx 1) -> Clock (idx 0).
        assert_eq!(cycle_home_app(HOME_APPS[1], false), HOME_APPS[0]);
    }

    #[test]
    fn forward_wraps_at_end() {
        // Last home app cycles back to the first.
        let last = *HOME_APPS.last().unwrap();
        assert_eq!(cycle_home_app(last, true), HOME_APPS[0]);
    }

    #[test]
    fn backward_wraps_at_start() {
        // First home app cycles to the last.
        assert_eq!(cycle_home_app(HOME_APPS[0], false), *HOME_APPS.last().unwrap());
    }

    #[test]
    fn non_home_screen_returns_unchanged() {
        // Screens that aren't in HOME_APPS (Stopwatch, Timer, Alarm,
        // Settings, Panel) don't participate in L/R cycling - the
        // function returns them unchanged.
        assert_eq!(cycle_home_app(ScreenId::Stopwatch, true), ScreenId::Stopwatch);
        assert_eq!(cycle_home_app(ScreenId::Settings, false), ScreenId::Settings);
        assert_eq!(cycle_home_app(ScreenId::Panel, true), ScreenId::Panel);
    }
}
