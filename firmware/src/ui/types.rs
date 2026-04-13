//! Core UI types - Screen trait, actions, and shared data.

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565};

use crate::events::SystemEvent;

// -- Screen IDs --------------------------------------------------------------

/// Identifies which screen to switch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenId {
    Clock,
    Status,
    /// The pull-down app picker. Not part of the home-row rotation
    /// (it's reached only via swipe-down-from-header) and constructed
    /// via `ActiveScreen::new_panel(previous)` because it needs
    /// context that plain `new(id)` doesn't provide.
    Panel,
    // Future: Sensors, Settings, ...
}

// -- Actions -----------------------------------------------------------------

/// What a screen wants the system to do after processing an event.
///
/// `SwitchScreen` is currently unused but stays as part of the screen
/// API - screens may want to programmatically navigate (e.g., a
/// settings screen returning to Clock, an alarm firing jumping to a
/// timer screen).
#[allow(dead_code)] // SwitchScreen is reserved for programmatic nav
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing to do.
    None,
    /// Screen content changed, request a display refresh.
    Redraw,
    /// Switch to a different screen.
    SwitchScreen(ScreenId),
    /// Request system shutdown.
    Shutdown,
}

// -- System data snapshot ----------------------------------------------------

// The per-peripheral data structs live alongside the task that
// produces them, so each task module is a self-contained unit
// (hardware state + emitted data type). We just re-export them
// here so screens can `use crate::ui::types::{TimeData, PowerData,
// MotionData, TouchData, SystemData}` from one place.
pub use crate::system::tasks::imu::MotionData;
pub use crate::system::tasks::power::PowerData;
pub use crate::system::tasks::rtc::TimeData;
pub use crate::system::tasks::touch::TouchData;

/// Read-only snapshot of system state, passed to screens each frame.
///
/// Organised by source peripheral so each task owns exactly one
/// sub-struct. Adding a new field means extending one group and
/// teaching one event handler to keep it up to date - no changes
/// to the screen trait, no unrelated refactors.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemData {
    pub time: TimeData,
    pub power: PowerData,
    pub motion: MotionData,
    pub touch: TouchData,
    pub tick_count: u32,
}

// -- Screen trait -------------------------------------------------------------

/// Trait that all UI screens implement.
///
/// Screens are stateful - they can track animations, scroll positions,
/// selection state, etc. The SystemManager doesn't know or care about
/// screen internals.
pub trait Screen {
    /// Render the screen to the display. Called every frame.
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData);

    /// Handle a system event. Return an Action to tell the manager what to do.
    fn on_event(&mut self, event: &SystemEvent, data: &SystemData) -> Action;
}
