//! Ruling G8 (lever 1): building a lightbox, and resizing it, peak at the buffers they keep, so
//! the web's high-water (wasm memory never shrinks) is the live set, not a transient. One test per
//! binary, counting this thread's live bytes only.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use halation::{Easel, Lightbox, Output, Theme};
use spectrograph::Game;

struct Tracking;

thread_local! {
    static LIVE: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
}

fn grew(n: usize) {
    let _ = LIVE.try_with(|l| {
        l.set(l.get() + n);
        let _ = PEAK.try_with(|p| p.set(p.get().max(l.get())));
    });
}

fn shrank(n: usize) {
    let _ = LIVE.try_with(|l| l.set(l.get().saturating_sub(n)));
}

unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grew(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grew(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        shrank(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        grew(size);
        shrank(layout.size());
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Tracking = Tracking;

/// The peak live bytes over `f`, past what was live before it, and what is live after it.
fn peak_and_kept(f: impl FnOnce()) -> (usize, usize) {
    let before = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(before));
    f();
    // A resize to a smaller canvas can end below `before`: saturate.
    (
        PEAK.with(Cell::get) - before,
        LIVE.with(Cell::get).saturating_sub(before),
    )
}

#[test]
fn building_a_lightbox_peaks_at_its_live_buffers() {
    const SLACK: usize = 64 << 10;
    let portrait = Easel::new(780, 1688, 2.0).with_touch(true);
    let landscape = Easel::new(1688, 780, 2.0).with_touch(true);
    for theme in Theme::ALL {
        let state = common::state(Game::C1, 0, false);
        let mut out = vec![0u32; 780 * 1688];
        let mut lightbox = None;
        let (peak, kept) = peak_and_kept(|| {
            let mut lb = Lightbox::new(portrait, theme, false);
            lb.paint(&state, f64::INFINITY, Output::Xrgb(&mut out));
            lightbox = Some(lb);
        });
        assert!(
            peak <= kept + SLACK,
            "{theme:?} new: peak {peak} over kept {kept}"
        );
        let lb = lightbox.as_mut().unwrap();
        for e in [landscape, portrait] {
            // Absolute (review M8): the peak during the resize, over what was live before it,
            // is at most the larger of the before and after sets plus the slack, so a resize
            // that holds old and new buffers at once fails.
            let before = LIVE.with(Cell::get);
            let (peak, _) = peak_and_kept(|| {
                lb.resize(e);
                lb.paint(&state, f64::INFINITY, Output::Xrgb(&mut out));
            });
            let after = LIVE.with(Cell::get);
            assert!(
                before + peak <= before.max(after) + SLACK,
                "{theme:?} resize to {e:?}: peak {} over {}",
                before + peak,
                before.max(after)
            );
        }
    }
}
