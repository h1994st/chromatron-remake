//! The two looks (spec §11, §13): which one the app shows, how the choice is stored, and how a
//! snapshot or probe run picks one.

/// The original's 640 × 480 screen, or halation's modern lightbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Classic,
    Modern,
}

impl Look {
    /// The look View ▸ Classic Look switches to.
    pub fn other(self) -> Look {
        match self {
            Look::Classic => Look::Modern,
            Look::Modern => Look::Classic,
        }
    }
}

/// A look's window shape waits while the window is fullscreen (spec §13): the size change
/// happens once AppKit has taken the window out of fullscreen and restored its frame
/// (`NSWindowDidExitFullScreenNotification`), not as soon as winit's `fullscreen()` clears.
#[derive(Debug, Default)]
pub struct ShapeQueue {
    /// The shape the window must take once fullscreen ends.
    pending: Option<Look>,
    /// The look the window was shaped for when the wait began.
    origin: Option<Look>,
    /// An exit from fullscreen began; AppKit's exit has not finished yet.
    leaving: bool,
}

impl ShapeQueue {
    /// The shape to apply now for a switch `from` → `to`. While fullscreen (or leaving it) it
    /// waits instead: the last switch wins, and switches that end on the look fullscreen began
    /// with cancel out (no reshape, no re-centring).
    pub fn request(&mut self, from: Look, to: Look, fullscreen: bool) -> Option<Look> {
        if fullscreen || self.leaving {
            let origin = *self.origin.get_or_insert(from);
            self.pending = (to != origin).then_some(to);
            None
        } else {
            self.pending = None;
            self.origin = None;
            Some(to)
        }
    }

    /// An exit from fullscreen began: AppKit's `NSWindowWillExitFullScreenNotification`, however
    /// it left (the green button, the system shortcut, or a menu exit winit replayed; a menu exit
    /// that begins at once comes through `exit_requested`). winit may report no fullscreen from
    /// now, but the shape still waits for `exited`.
    pub fn leaving(&mut self) {
        self.leaving = true;
    }

    /// View ▸ Exit Full Screen was requested (called right after `set_fullscreen(None)`): a
    /// leave begins only if the exit did. When the window is `still_fullscreen`, winit deferred
    /// the request behind a running transition (ruling G12): if that entry fails, winit drops
    /// the exit and nothing reaches the shell, so marking a leave here would disable Scale for
    /// ever; if it succeeds, winit replays the exit, and `NSWindowWillExitFullScreenNotification`
    /// marks the leave then.
    pub fn exit_requested(&mut self, still_fullscreen: bool) {
        if !still_fullscreen {
            self.leaving = true;
        }
    }

    /// An exit from fullscreen is still running: winit may report no fullscreen, but AppKit
    /// owns the window's frame until `exited`, so nothing may resize the window yet (classic's
    /// Scale included).
    pub fn is_leaving(&self) -> bool {
        self.leaving
    }

    /// AppKit finished leaving fullscreen: the shape that waited, once. When the window is
    /// already `still_fullscreen` (⌃⌘F during the exit; winit re-enters inside it), the shape
    /// keeps waiting, with its origin, for the next exit.
    pub fn exited(&mut self, still_fullscreen: bool) -> Option<Look> {
        self.leaving = false;
        if still_fullscreen {
            return None;
        }
        self.origin = None;
        self.pending.take()
    }
}

/// The stored `ClassicLook`: absent (never written) means modern, the default (spec §11).
pub fn look_setting(stored: Option<bool>) -> Look {
    if stored == Some(true) {
        Look::Classic
    } else {
        Look::Modern
    }
}

/// `CHROMATRON_LOOK`, read only by snapshot and present-probe runs (ruling P9): unset or empty
/// means modern.
pub fn look_from_env(value: Option<&str>) -> Result<Look, String> {
    match value.map(str::trim) {
        None | Some("") | Some("modern") => Ok(Look::Modern),
        Some("classic") => Ok(Look::Classic),
        Some(other) => Err(format!(
            "unknown look {other:?} in CHROMATRON_LOOK; expected classic or modern (lower case)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_defaults_to_modern_unless_classic_was_stored() {
        assert_eq!(look_setting(None), Look::Modern);
        assert_eq!(look_setting(Some(false)), Look::Modern);
        assert_eq!(look_setting(Some(true)), Look::Classic);
    }

    #[test]
    fn look_from_the_environment() {
        assert_eq!(look_from_env(None), Ok(Look::Modern));
        assert_eq!(look_from_env(Some(" ")), Ok(Look::Modern));
        assert_eq!(look_from_env(Some("classic")), Ok(Look::Classic));
        assert_eq!(look_from_env(Some("modern")), Ok(Look::Modern));
        assert!(
            look_from_env(Some("retro"))
                .unwrap_err()
                .contains("classic or modern")
        );
        assert!(
            look_from_env(Some("Classic"))
                .unwrap_err()
                .contains("lower case"),
            "the values are case-sensitive, and the message says so"
        );
    }

    #[test]
    fn look_other_switches() {
        assert_eq!(Look::Classic.other(), Look::Modern);
        assert_eq!(Look::Modern.other(), Look::Classic);
    }

    use Look::{Classic, Modern};

    #[test]
    fn shape_queue_applies_at_once_outside_fullscreen() {
        let mut q = ShapeQueue::default();
        assert_eq!(q.request(Modern, Classic, false), Some(Classic));
        assert_eq!(q.exited(false), None);
    }

    #[test]
    fn shape_queue_waits_for_fullscreen_to_end() {
        let mut q = ShapeQueue::default();
        assert_eq!(q.request(Modern, Classic, true), None);
        assert_eq!(q.request(Classic, Modern, true), None);
        assert_eq!(
            q.request(Modern, Classic, true),
            None,
            "the last switch wins"
        );
        assert_eq!(q.exited(false), Some(Classic));
        assert_eq!(q.exited(false), None, "once");
    }

    /// A fullscreen that never began (AppKit refused it) sends no exit; the next switch outside
    /// fullscreen takes over.
    #[test]
    fn shape_queue_drops_a_wait_overtaken_outside_fullscreen() {
        let mut q = ShapeQueue::default();
        q.request(Modern, Classic, true);
        assert_eq!(q.request(Classic, Modern, false), Some(Modern));
        assert_eq!(q.exited(false), None);
    }

    /// View ▸ Exit Full Screen clears winit's `fullscreen()` before AppKit's exit animation:
    /// the shape still waits for the exit itself.
    #[test]
    fn shape_queue_waits_for_the_exit_not_for_fullscreen_to_clear() {
        let mut q = ShapeQueue::default();
        assert_eq!(q.request(Modern, Classic, true), None);
        q.leaving();
        assert_eq!(
            q.request(Classic, Modern, false),
            None,
            "a switch during the exit animation waits too"
        );
        assert_eq!(q.request(Modern, Classic, false), None);
        assert_eq!(q.exited(false), Some(Classic));
        assert_eq!(
            q.request(Classic, Modern, false),
            Some(Modern),
            "out of fullscreen at last"
        );
    }

    /// ⌃⌘F pressed again during a ⌃⌘F exit: winit replays the enter inside AppKit's exit, so
    /// the window is fullscreen again when the exit arrives and the shape keeps waiting.
    #[test]
    fn shape_queue_keeps_waiting_when_fullscreen_is_re_entered() {
        let mut q = ShapeQueue::default();
        assert_eq!(q.request(Modern, Classic, true), None);
        q.leaving();
        assert_eq!(q.exited(true), None, "fullscreen again: no reshape");
        assert_eq!(
            q.request(Classic, Modern, true),
            None,
            "the origin is kept: back to it cancels"
        );
        assert_eq!(q.request(Modern, Classic, true), None);
        assert_eq!(q.exited(false), Some(Classic), "the real exit");
        assert_eq!(q.request(Classic, Modern, false), Some(Modern));
    }

    /// Classic's Scale waits out the exit animation too: `is_leaving` holds from the moment an
    /// exit from fullscreen begins until it arrives, re-entered or not.
    #[test]
    fn shape_queue_is_leaving_until_the_exit_arrives() {
        let mut q = ShapeQueue::default();
        assert!(!q.is_leaving());
        q.leaving();
        assert!(q.is_leaving());
        assert_eq!(q.exited(false), None);
        assert!(!q.is_leaving(), "out");
        q.leaving();
        assert_eq!(q.exited(true), None);
        assert!(
            !q.is_leaving(),
            "back in fullscreen, which guards on its own"
        );
    }

    /// Ruling G12: Exit Full Screen chosen while an entry animates is deferred by winit (the
    /// window still reports fullscreen), and dropped if that entry fails: no exit begins, so no
    /// leave is marked, and Scale stays usable once the window is windowed.
    #[test]
    fn an_exit_deferred_behind_a_failed_entry_marks_no_leave() {
        let mut q = ShapeQueue::default();
        q.exit_requested(true);
        // The entry fails: AppKit tells only winit's delegate; nothing reaches the shell.
        assert!(
            !q.is_leaving(),
            "deferred: no exit began, so Scale is usable"
        );
    }

    /// The exit winit replays after the entry succeeds begins with `windowWillExitFullScreen`
    /// (observed), and its DidExit ends the leave as before; a look switched meanwhile waits for
    /// it.
    #[test]
    fn a_replayed_exit_is_a_leave_until_it_ends() {
        let mut q = ShapeQueue::default();
        assert_eq!(q.request(Modern, Classic, true), None);
        q.exit_requested(true);
        assert!(!q.is_leaving());
        q.leaving(); // NSWindowWillExitFullScreenNotification
        assert!(q.is_leaving());
        assert_eq!(q.exited(false), Some(Classic));
        assert!(!q.is_leaving());
    }

    /// An exit that begins at once (winit reports no fullscreen right after the request) is a
    /// leave from the request on, as before.
    #[test]
    fn an_exit_that_begins_is_a_leave_at_once() {
        let mut q = ShapeQueue::default();
        q.exit_requested(false);
        assert!(q.is_leaving());
        q.exited(false);
        assert!(!q.is_leaving());
    }

    /// Switches that end on the look fullscreen began with leave the window where it was.
    #[test]
    fn shape_queue_forgets_switches_that_cancel_out() {
        let mut q = ShapeQueue::default();
        q.request(Modern, Classic, true);
        q.request(Classic, Modern, true);
        assert_eq!(q.exited(false), None, "no reshape, no re-centring");
        q.request(Classic, Modern, true);
        q.request(Modern, Classic, true);
        q.leaving();
        assert_eq!(q.exited(false), None);
    }
}
