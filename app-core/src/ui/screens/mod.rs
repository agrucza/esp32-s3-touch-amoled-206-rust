pub mod alarm;
pub mod app_drawer;
pub mod clock;
pub mod quick_access;
pub mod settings;
pub mod stopwatch;
pub mod timer;

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565};

use crate::events::SystemEvent;
use super::types::{Action, Screen, ScreenId, SystemData};

/// Enum-based screen dispatch - avoids dynamic dispatch and heap allocation.
///
/// Add new screen variants here as they're created.
pub enum ActiveScreen {
    Clock(clock::ClockScreen),
    Stopwatch(stopwatch::StopwatchScreen),
    Timer(timer::TimerScreen),
    Alarm(alarm::AlarmScreen),
    Settings(settings::SettingsScreen),
    /// Pull-down Quick Access overlay. Reached via swipe-down-from-top.
    QuickAccess(quick_access::QuickAccessScreen),
    /// Pull-up App Drawer. Reached via swipe-up-from-bottom and via
    /// tapping the watch face.
    AppDrawer(app_drawer::AppDrawerScreen),
}

impl ActiveScreen {
    /// Create a fresh screen for the given id.
    ///
    /// Note: `ScreenId::QuickAccess` and `ScreenId::AppDrawer` can't
    /// be constructed this way - both overlays need a `previous:
    /// ScreenId` context that plain id-based construction can't
    /// supply. Use `new_quick_access(previous)` or
    /// `new_app_drawer(previous)` instead.
    pub fn new(id: ScreenId) -> Self {
        match id {
            ScreenId::Clock => Self::Clock(clock::ClockScreen::new()),
            ScreenId::Stopwatch => Self::Stopwatch(stopwatch::StopwatchScreen::new()),
            ScreenId::Timer => Self::Timer(timer::TimerScreen::new()),
            ScreenId::Alarm => Self::Alarm(alarm::AlarmScreen::new()),
            ScreenId::Settings => Self::Settings(settings::SettingsScreen::new()),
            ScreenId::QuickAccess => {
                debug_assert!(false,
                    "use ActiveScreen::new_quick_access(previous) for QuickAccess");
                Self::Clock(clock::ClockScreen::new())
            }
            ScreenId::AppDrawer => {
                debug_assert!(false,
                    "use ActiveScreen::new_app_drawer(previous) for AppDrawer");
                Self::Clock(clock::ClockScreen::new())
            }
        }
    }

    /// Create the Quick Access overlay, remembering which screen it
    /// should return to on close.
    pub fn new_quick_access(previous: ScreenId) -> Self {
        Self::QuickAccess(quick_access::QuickAccessScreen::new(previous))
    }

    /// Create the App Drawer overlay, remembering which screen it
    /// should return to on close.
    pub fn new_app_drawer(previous: ScreenId) -> Self {
        Self::AppDrawer(app_drawer::AppDrawerScreen::new(previous))
    }

    pub fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        match self {
            Self::Clock(s) => s.render(display, data),
            Self::Stopwatch(s) => s.render(display, data),
            Self::Timer(s) => s.render(display, data),
            Self::Alarm(s) => s.render(display, data),
            Self::Settings(s) => s.render(display, data),
            Self::QuickAccess(s) => s.render(display, data),
            Self::AppDrawer(s) => s.render(display, data),
        }
    }

    pub fn on_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match self {
            Self::Clock(s) => s.on_event(event, data),
            Self::Stopwatch(s) => s.on_event(event, data),
            Self::Timer(s) => s.on_event(event, data),
            Self::Alarm(s) => s.on_event(event, data),
            Self::Settings(s) => s.on_event(event, data),
            Self::QuickAccess(s) => s.on_event(event, data),
            Self::AppDrawer(s) => s.on_event(event, data),
        }
    }

    pub fn mount(&mut self, data: &SystemData) {
        match self {
            Self::Clock(s) => s.on_mount(data),
            Self::Stopwatch(s) => s.on_mount(data),
            Self::Timer(s) => s.on_mount(data),
            Self::Alarm(s) => s.on_mount(data),
            Self::Settings(s) => s.on_mount(data),
            Self::QuickAccess(s) => s.on_mount(data),
            Self::AppDrawer(s) => s.on_mount(data),
        }
    }

    pub fn unmount(&mut self) {
        match self {
            Self::Clock(s) => s.on_unmount(),
            Self::Stopwatch(s) => s.on_unmount(),
            Self::Timer(s) => s.on_unmount(),
            Self::Alarm(s) => s.on_unmount(),
            Self::Settings(s) => s.on_unmount(),
            Self::QuickAccess(s) => s.on_unmount(),
            Self::AppDrawer(s) => s.on_unmount(),
        }
    }

    /// Which screen is currently active.
    pub fn id(&self) -> ScreenId {
        match self {
            Self::Clock(_) => ScreenId::Clock,
            Self::Stopwatch(_) => ScreenId::Stopwatch,
            Self::Timer(_) => ScreenId::Timer,
            Self::Alarm(_) => ScreenId::Alarm,
            Self::Settings(_) => ScreenId::Settings,
            Self::QuickAccess(_) => ScreenId::QuickAccess,
            Self::AppDrawer(_) => ScreenId::AppDrawer,
        }
    }

    /// Switch to a different screen. Constructs a fresh instance of
    /// the target screen and runs its mount hook. Not valid for the
    /// overlay screens; use `open_quick_access` / `open_app_drawer`.
    pub fn switch_to(&mut self, id: ScreenId, data: &SystemData) {
        self.unmount();
        *self = Self::new(id);
        self.mount(data);
    }

    /// Open the Quick Access overlay.
    pub fn open_quick_access(&mut self, previous: ScreenId, data: &SystemData) {
        self.unmount();
        *self = Self::new_quick_access(previous);
        self.mount(data);
    }

    /// Open the App Drawer overlay.
    pub fn open_app_drawer(&mut self, previous: ScreenId, data: &SystemData) {
        self.unmount();
        *self = Self::new_app_drawer(previous);
        self.mount(data);
    }
}
