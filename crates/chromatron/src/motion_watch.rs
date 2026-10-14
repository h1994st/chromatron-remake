//! Reduced motion on macOS (spec §9.3): `NSWorkspace.accessibilityDisplayShouldReduceMotion`,
//! read at launch, on every `Focused(true)` and when the system posts
//! `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2_app_kit::{NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification};
use objc2_foundation::{NSNotification, NSOperationQueue};

/// Where the reduced-motion setting comes from (the system, or a test's fake).
pub trait MotionSetting {
    fn reduce_motion(&self) -> bool;
}

/// The system's Accessibility ▸ Display ▸ Reduce motion.
pub struct SystemMotion;

impl MotionSetting for SystemMotion {
    fn reduce_motion(&self) -> bool {
        NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }
}

/// The last value read, so only a change reaches the lightbox.
pub struct MotionWatch<S> {
    source: S,
    current: bool,
}

impl<S: MotionSetting> MotionWatch<S> {
    pub fn new(source: S) -> MotionWatch<S> {
        let current = source.reduce_motion();
        MotionWatch { source, current }
    }

    pub fn current(&self) -> bool {
        self.current
    }

    /// Reads the setting again: the new value when it changed, else `None`.
    pub fn refresh(&mut self) -> Option<bool> {
        let now = self.source.reduce_motion();
        (now != self.current).then(|| {
            self.current = now;
            now
        })
    }
}

/// Calls `on_change` on the main queue whenever the system's display accessibility options
/// change; the observer is removed when this is dropped.
pub struct MotionObserver {
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

impl MotionObserver {
    /// The marker proves the main thread. `on_change` runs on the main queue and may still run
    /// after the observer is dropped, so it must tolerate a closed event loop (ignore a failed
    /// send).
    pub fn install(_main: MainThreadMarker, on_change: impl Fn() + 'static) -> MotionObserver {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let block = RcBlock::new(move |_: NonNull<NSNotification>| on_change());
        // SAFETY: the name is an immutable NSString constant; no object filter; the main queue
        // runs the block on the main thread, where the event loop and its proxy live.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        MotionObserver { token }
    }
}

impl Drop for MotionObserver {
    fn drop(&mut self) {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        // SAFETY: `token` is the observer `addObserverForName…` returned on this center.
        unsafe { center.removeObserver(self.token.as_ref()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    struct Fake(Rc<Cell<bool>>);

    impl MotionSetting for Fake {
        fn reduce_motion(&self) -> bool {
            self.0.get()
        }
    }

    #[test]
    fn the_watch_reports_only_changes() {
        let setting = Rc::new(Cell::new(false));
        let mut watch = MotionWatch::new(Fake(setting.clone()));
        assert!(!watch.current());
        assert_eq!(watch.refresh(), None, "nothing changed");
        setting.set(true);
        assert_eq!(watch.refresh(), Some(true));
        assert!(watch.current());
        assert_eq!(watch.refresh(), None, "reported once");
        setting.set(false);
        assert_eq!(watch.refresh(), Some(false));
    }

    #[test]
    fn the_watch_starts_from_the_setting() {
        let watch = MotionWatch::new(Fake(Rc::new(Cell::new(true))));
        assert!(watch.current());
    }

    /// A smoke check only (it fails only by panicking); the `MotionWatch` tests above are the
    /// plumbing's coverage.
    #[test]
    fn the_system_setting_reads_without_panicking() {
        // Whatever this Mac's setting is, reading it neither panics nor blocks.
        let _ = SystemMotion.reduce_motion();
    }
}
