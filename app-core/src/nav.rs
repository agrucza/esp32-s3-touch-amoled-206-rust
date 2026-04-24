//! Navigation stack.
//!
//! LIFO of screen IDs the user can return to via `Action::Back`.
//! Pushed when the user navigates *into* a new screen, popped on
//! `Back`.
//!
//! The Quick Access and App Drawer overlays are never pushed - each
//! one replaces the current screen when launching an app, and the
//! pre-overlay screen already sits below it on the stack.

use crate::ui::types::ScreenId;
use heapless::Vec;

/// Maximum depth of the navigation stack.
///
/// Realistically the deepest chain today is
/// `Clock -> Panel -> App -> Panel -> App`, but the panel
/// replaces-top so the stack never actually contains more than
/// one "real" entry per visited screen. Four slots leaves
/// generous headroom; push failures beyond this degrade
/// gracefully to "Back returns to Clock".
pub const NAV_STACK_DEPTH: usize = 4;

/// Fixed-size navigation stack. Methods mirror the subset of
/// `heapless::Vec` semantics that the manager actually uses.
#[derive(Debug, Default, Clone)]
pub struct NavStack {
    inner: Vec<ScreenId, NAV_STACK_DEPTH>,
}

impl NavStack {
    pub const fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Push a screen. Silently drops the push if the stack is
    /// full - consistent with the current behaviour via
    /// `let _ = self.nav_stack.push(...)`.
    pub fn push(&mut self, screen: ScreenId) {
        let _ = self.inner.push(screen);
    }

    /// Pop the top screen, or return `ScreenId::Clock` if the
    /// stack is empty. Clock is always a safe landing: it's the
    /// default first screen on boot and nothing navigates below
    /// it.
    pub fn pop_or_home(&mut self) -> ScreenId {
        self.inner.pop().unwrap_or(ScreenId::Clock)
    }

    /// Peek the top screen without popping, or return
    /// `ScreenId::Clock` if the stack is empty. Used by the overlay
    /// open path to reuse the pre-overlay entry when the user
    /// switches directly between the two overlays.
    pub fn peek_or_home(&self) -> ScreenId {
        self.inner.last().copied().unwrap_or(ScreenId::Clock)
    }

    /// Current depth (for diagnostics / tests).
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Empty flag (for diagnostics / tests).
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stack_pops_home() {
        let mut s = NavStack::new();
        assert_eq!(s.pop_or_home(), ScreenId::Clock);
    }

    #[test]
    fn push_then_pop_round_trips() {
        let mut s = NavStack::new();
        s.push(ScreenId::Settings);
        assert_eq!(s.pop_or_home(), ScreenId::Settings);
        // Second pop after emptying hits the home fallback.
        assert_eq!(s.pop_or_home(), ScreenId::Clock);
    }

    #[test]
    fn lifo_order() {
        let mut s = NavStack::new();
        s.push(ScreenId::Clock);
        s.push(ScreenId::Settings);
        s.push(ScreenId::Timer);
        assert_eq!(s.pop_or_home(), ScreenId::Timer);
        assert_eq!(s.pop_or_home(), ScreenId::Settings);
        assert_eq!(s.pop_or_home(), ScreenId::Clock);
        assert_eq!(s.pop_or_home(), ScreenId::Clock); // empty fallback
    }

    #[test]
    fn overflow_drops_silently() {
        let mut s = NavStack::new();
        for _ in 0..NAV_STACK_DEPTH + 3 {
            s.push(ScreenId::Clock);
        }
        assert_eq!(s.len(), NAV_STACK_DEPTH);
    }
}
