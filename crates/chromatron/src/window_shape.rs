//! The window each look opens (spec §13): classic's fixed 640 × 480 pt window exactly as today,
//! and modern's resizable one (default 1280 × 800 pt within 90% of the screen, minimum 800 × 600);
//! the modern easel for a window size; the theme for the window's appearance; and AppKit's
//! word that the window has left fullscreen, which a look's shape waits for.

use std::ptr::NonNull;

use block2::RcBlock;
use halation::{Easel, Theme};
use lumen::{FRAME_HEIGHT, FRAME_WIDTH};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSScreen, NSWindowDidExitFullScreenNotification};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue};
use winit::dpi::{LogicalSize, PhysicalSize, Size};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowButtons};

use crate::look_switch::Look;

/// The modern window's minimum content size (pt, D6).
pub const MODERN_MIN_PT: (f64, f64) = (800.0, 600.0);
/// The modern window's default content size (pt).
pub const MODERN_DEFAULT_PT: (f64, f64) = (1280.0, 800.0);
/// The share of the screen's visible frame the default modern window may take.
pub const SCREEN_SHARE: f64 = 0.9;
/// The classic window's content size at launch (pt): 640 × 480, one pt per pixel of the
/// original's frame.
const CLASSIC_PT: (f64, f64) = (FRAME_WIDTH as f64, FRAME_HEIGHT as f64);

/// How a look's window is set up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowShape {
    pub size: LogicalSize<f64>,
    pub min_size: Option<LogicalSize<f64>>,
    pub resizable: bool,
    pub buttons: WindowButtons,
}

/// The default modern content size (pt) for a screen whose visible frame is `visible` (pt):
/// 1280 × 800, each side at most 90% of the screen's (whole pt), never under 800 × 600. A
/// missing or broken frame gives the default.
pub fn modern_size(visible: Option<(f64, f64)>) -> (f64, f64) {
    let (dw, dh) = MODERN_DEFAULT_PT;
    let (w, h) = visible
        .filter(|&(w, h)| w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0)
        .map_or((dw, dh), |(w, h)| {
            (
                (w * SCREEN_SHARE).floor().min(dw),
                (h * SCREEN_SHARE).floor().min(dh),
            )
        });
    (w.max(MODERN_MIN_PT.0), h.max(MODERN_MIN_PT.1))
}

/// The window for `look` on a screen whose visible frame is `visible` (pt).
pub fn shape_for(look: Look, visible: Option<(f64, f64)>) -> WindowShape {
    match look {
        Look::Classic => WindowShape {
            size: LogicalSize::new(CLASSIC_PT.0, CLASSIC_PT.1),
            min_size: None,
            resizable: false,
            buttons: WindowButtons::CLOSE | WindowButtons::MINIMIZE,
        },
        Look::Modern => {
            let (w, h) = modern_size(visible);
            WindowShape {
                size: LogicalSize::new(w, h),
                min_size: Some(LogicalSize::new(MODERN_MIN_PT.0, MODERN_MIN_PT.1)),
                resizable: true,
                buttons: WindowButtons::all(),
            }
        }
    }
}

/// Reshapes an open window (a look switch, spec §13). Shrinking to classic needs the minimum
/// gone first; growing to modern sets it once the size is asked for. On macOS winit 0.30.13's
/// `request_inner_size` always returns `None` (it calls `setContentSize` and a `Resized`
/// follows), so the caller lays out at `inner_size()` itself and again on that `Resized`.
pub fn apply_shape(window: &Window, shape: &WindowShape) {
    if shape.min_size.is_none() {
        window.set_min_inner_size(None::<Size>);
    }
    window.set_resizable(shape.resizable);
    window.set_enabled_buttons(shape.buttons);
    let _ = window.request_inner_size(shape.size);
    if let Some(min) = shape.min_size {
        window.set_min_inner_size(Some(min));
    }
}

/// The window's `NSWindow`; `None` when winit has no AppKit handle for it.
fn ns_window(window: &Window) -> Option<Retained<AnyObject>> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: winit's AppKit handle is a live NSView, used on the main thread; `window` is
    // NSView's own method and takes no arguments.
    unsafe {
        let view: &AnyObject = handle.ns_view.cast().as_ref();
        msg_send![view, window]
    }
}

/// Centres the window on its screen (`-[NSWindow center]`). winit can't, and AppKit cascades a
/// new window instead. A look switch resizes the content with its top-left corner fixed, so a
/// window that grows extends down and to the right and could run off the screen's bottom edge.
pub fn centre_window(window: &Window) {
    if let Some(ns_window) = ns_window(window) {
        // SAFETY: `center` is NSWindow's own method, takes no arguments, on the main thread.
        let _: () = unsafe { msg_send![&*ns_window, center] };
    }
}

/// Calls `on_exit` on the main queue each time AppKit has finished taking the window out of
/// fullscreen (`NSWindowDidExitFullScreenNotification`, after winit has restored its frame and
/// style), whichever way it left: View ▸ Exit Full Screen, the green button or the system's
/// shortcut. The observer is removed when this is dropped.
pub struct FullscreenExitObserver {
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

impl FullscreenExitObserver {
    /// The marker proves the main thread. `on_exit` may still run after the observer is
    /// dropped, so it must tolerate a closed event loop (ignore a failed send). `None` when the
    /// window has no `NSWindow`.
    pub fn install(
        _main: MainThreadMarker,
        window: &Window,
        on_exit: impl Fn() + 'static,
    ) -> Option<FullscreenExitObserver> {
        let ns_window = ns_window(window)?;
        let block = RcBlock::new(move |_: NonNull<NSNotification>| on_exit());
        // SAFETY: the name is an immutable NSString constant; the object filter is this
        // window's NSWindow (the center keeps no reference to it); the main queue runs the
        // block on the main thread, where the event loop and its proxy live.
        let token = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSWindowDidExitFullScreenNotification),
                Some(&ns_window),
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        Some(FullscreenExitObserver { token })
    }
}

impl Drop for FullscreenExitObserver {
    fn drop(&mut self) {
        // SAFETY: `token` is the observer `addObserverForName…` returned on this center.
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(self.token.as_ref()) };
    }
}

/// The main screen's visible frame (pt): the screen less the menu bar and the Dock. `None` off
/// the main thread or without a screen.
pub fn visible_frame() -> Option<(f64, f64)> {
    let mtm = MainThreadMarker::new()?;
    let frame = NSScreen::mainScreen(mtm)?.visibleFrame();
    Some((frame.size.width, frame.size.height))
}

/// The modern easel for a window of `size` device px at `scale_factor` px per pt (spec §13: no
/// safe area, never touch).
pub fn easel_for(size: PhysicalSize<u32>, scale_factor: f64) -> Easel {
    Easel::new(size.width, size.height, scale_factor as f32)
}

/// The modern look's theme for the window's effective appearance (spec §11). Only the dark
/// theme exists until phase 5, which returns `Theme::Light` for `Some(Light)` here.
pub fn theme_for(_appearance: Option<winit::window::Theme>) -> Theme {
    Theme::Dark
}

#[cfg(test)]
mod tests {
    use super::*;
    use halation::{Arrangement, Tableau};

    #[test]
    fn the_modern_window_is_1280_by_800_within_90_percent_of_the_screen() {
        // A 14" MacBook Pro's visible frame, a 13" Air's, a 1024 × 768 display, a tiny one.
        assert_eq!(modern_size(Some((1512.0, 945.0))), (1280.0, 800.0));
        assert_eq!(modern_size(Some((1440.0, 875.0))), (1280.0, 787.0));
        assert_eq!(modern_size(Some((1024.0, 743.0))), (921.0, 668.0));
        assert_eq!(
            modern_size(Some((800.0, 600.0))),
            (800.0, 600.0),
            "never under the minimum"
        );
        assert_eq!(modern_size(None), (1280.0, 800.0));
        assert_eq!(modern_size(Some((f64::NAN, 900.0))), (1280.0, 800.0));
        assert_eq!(modern_size(Some((0.0, 0.0))), (1280.0, 800.0));
    }

    #[test]
    fn each_look_has_its_window() {
        let modern = shape_for(Look::Modern, Some((1512.0, 945.0)));
        assert_eq!(modern.size, LogicalSize::new(1280.0, 800.0));
        assert_eq!(modern.min_size, Some(LogicalSize::new(800.0, 600.0)));
        assert!(modern.resizable);
        assert_eq!(modern.buttons, WindowButtons::all(), "zoom enabled");
        // Classic is exactly today's window (spec §13).
        let classic = shape_for(Look::Classic, Some((1512.0, 945.0)));
        assert_eq!(classic.size, LogicalSize::new(640.0, 480.0));
        assert_eq!(classic.min_size, None);
        assert!(!classic.resizable);
        assert_eq!(
            classic.buttons,
            WindowButtons::CLOSE | WindowButtons::MINIMIZE
        );
    }

    #[test]
    fn easels_follow_the_backing_scale() {
        let easel = easel_for(PhysicalSize::new(2560, 1600), 2.0);
        assert_eq!(
            (easel.width, easel.height, easel.px_per_pt),
            (2560, 1600, 2.0)
        );
        assert!(!easel.touch);
        assert_eq!(easel.room(), (1280.0, 800.0));
        let t = Tableau::arrange(&easel);
        assert_eq!(t.arrangement, Arrangement::Beside);
        assert!(
            t.picker.is_some(),
            "the default window has an inline picker"
        );
        assert_eq!(
            easel_for(PhysicalSize::new(800, 600), 1.0).room(),
            (800.0, 600.0)
        );
    }

    /// Ruling P2: a tall window is Stacked, and can be compact (D6 corrected).
    #[test]
    fn a_tall_window_is_stacked_and_may_be_compact() {
        let t = Tableau::arrange(&easel_for(PhysicalSize::new(1600, 1400), 2.0));
        assert_eq!(t.arrangement, Arrangement::Stacked);
        assert!(t.compact && t.credits.is_none());
        let t = Tableau::arrange(&easel_for(PhysicalSize::new(1600, 1200), 2.0));
        assert_eq!(
            t.arrangement,
            Arrangement::Beside,
            "the minimum window is Beside"
        );
        assert!(!t.compact && t.picker.is_none());
    }

    /// Phase 5 maps `Light` to `Theme::Light` here, and changes this test.
    #[test]
    fn every_appearance_is_dark_until_phase_5() {
        assert_eq!(theme_for(None), Theme::Dark);
        assert_eq!(theme_for(Some(winit::window::Theme::Dark)), Theme::Dark);
        assert_eq!(theme_for(Some(winit::window::Theme::Light)), Theme::Dark);
    }
}
