//! Core UI types - Screen trait, actions, and shared data.

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565};

// Re-export self-test types so screens can pull them from a single
// place alongside the other UI data structs below. `pub use` also
// brings them into local scope for `SystemData`'s field types, so
// no separate `use` is needed.
pub use crate::events::{
    NUM_SELF_TESTS, SelfTestId, SelfTestResult, SystemEvent,
};

// -- Screen IDs --------------------------------------------------------------

/// Identifies which screen to switch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenId {
    Clock,
    Status,
    /// Device settings - internally state-machined into sub-views
    /// (IMU, RTC, Power, ...) via `SettingsScreen`'s own enum, so
    /// from the outside there is only one screen id.
    Settings,
    /// The pull-down app picker. Not part of the home-row rotation
    /// (it's reached only via swipe-down-from-header) and constructed
    /// via `ActiveScreen::new_panel(previous)` because it needs
    /// context that plain `new(id)` doesn't provide.
    Panel,
}

// -- Actions -----------------------------------------------------------------

/// What a screen wants the system to do after processing an event.
///
/// Screens return an `Action` from `on_event` to tell the outer
/// navigator what to do next. The navigator is the only thing
/// allowed to mutate global system state (switch screens, signal
/// tasks, shut down) - screens never touch those directly. This
/// keeps screens portable and the control flow easy to trace.
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
    /// Run a hardware self-test. The main loop routes the id to the
    /// task that owns the underlying hardware and fires that task's
    /// command signal; the screen doesn't know which task handles it.
    /// Progress and results come back asynchronously as
    /// [`SystemEvent::SelfTestUpdated`] events.
    RunSelfTest(SelfTestId),
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
    /// Per-test latest result, indexed by [`SelfTestId`] cast to
    /// `usize`. Updated by the main loop whenever a
    /// [`SystemEvent::SelfTestUpdated`] arrives.
    pub self_tests: [SelfTestResult; NUM_SELF_TESTS],
}

// -- Screen trait -------------------------------------------------------------

/// Trait that all UI screens implement.
///
/// Screens are stateful - they can track animations, scroll positions,
/// selection state, etc. The SystemManager doesn't know or care about
/// screen internals.
///
/// ## Lifecycle
///
/// - [`on_mount`] runs once right after the screen is switched to, with
///   the current [`SystemData`] available. Use it to read initial state
///   (e.g. a diagnostics screen that kicks off a self-test, a file
///   explorer that reads the current directory) before the first render.
/// - [`on_unmount`] runs once right before the screen is swapped out
///   or dropped. Use it to release resources or persist state.
/// - [`render`] is called every frame. Must be a pure function of the
///   screen's own state plus the provided [`SystemData`] snapshot.
/// - [`on_event`] is called for every [`SystemEvent`] the main loop
///   receives while this screen is active. Returns an [`Action`]
///   telling the outer navigator what to do next.
///
/// Default implementations are provided for the lifecycle hooks so
/// screens only override what they need - `render` and usually
/// `on_event` are the only methods most screens have to provide.
///
/// [`on_mount`]: Screen::on_mount
/// [`on_unmount`]: Screen::on_unmount
/// [`render`]: Screen::render
/// [`on_event`]: Screen::on_event
pub trait Screen {
    /// Called once when this screen becomes active, before the first
    /// render. Read anything that needs to be loaded on open here.
    fn on_mount(&mut self, _data: &SystemData) {}

    /// Called once when this screen is about to be swapped out or
    /// dropped. Release resources or persist state here.
    fn on_unmount(&mut self) {}

    /// Render the screen to the display. Called every frame.
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData);

    /// Handle a system event. Return an Action to tell the manager what to do.
    fn on_event(&mut self, event: &SystemEvent, data: &SystemData) -> Action;
}
