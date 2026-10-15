//! Ruling G13: a growing live resize on macOS reuses its buffers. Natively, `make_room` takes
//! 1/8 more room when it grows a buffer it already had, so a window dragged larger reallocates
//! each canvas-sized buffer every ~12% of growth, not on every step. One test per binary,
//! counting this thread's allocations only.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use halation::{Easel, Lightbox, Output, Theme};
use spectrograph::Game;

struct Counting;

thread_local! { static COUNT: Cell<usize> = const { Cell::new(0) }; }

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// A third of the count measured with no slack (review M14): 2 466 dark and 1 624 light, so
/// the light one's third, 1 624 / 3. With the slack: 242 dark, 220 light.
const RED_THIRD: usize = 541;

#[test]
fn a_growing_live_resize_reallocates_rarely() {
    for theme in Theme::ALL {
        let state = common::state(Game::C1, 41, false);
        let mut lightbox = Lightbox::new(Easel::new(1600, 1000, 2.0), theme, false);
        let mut out = vec![0u32; 2400 * 1500];
        let before = COUNT.with(Cell::get);
        // 100 steps of +8 × +5 px, as a window corner dragged out: the canvas grows 2.2×.
        for k in 0..100u32 {
            let e = Easel::new(1600 + 8 * k, 1000 + 5 * k, 2.0);
            lightbox.resize(e);
            let n = (e.width * e.height) as usize;
            lightbox.paint(&state, f64::INFINITY, Output::Xrgb(&mut out[..n]));
        }
        let allocations = COUNT.with(Cell::get) - before;
        assert!(
            allocations < RED_THIRD,
            "{theme:?}: {allocations} allocations over 100 growing steps"
        );
    }
}
