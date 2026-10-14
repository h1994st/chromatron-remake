//! `lay_out` on warmed buffers never allocates, credits joined included (spec §14).
//! One test per binary, counting this thread's allocations only.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use chromatron::caption_typeset::{CaptionLines, lay_out};
use halation::{Caption, CaptionRole, CaptionStyle, DeviceRect};
use lumen::TextRole;

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

fn allocations(f: impl FnOnce()) -> usize {
    let before = COUNT.with(Cell::get);
    f();
    COUNT.with(Cell::get) - before
}

fn cap(
    role: TextRole,
    text: &'static str,
    rect: DeviceRect,
    style: CaptionStyle,
) -> Caption<'static> {
    Caption {
        role: CaptionRole::Text(role),
        text,
        rect,
        style,
        alpha: 1.0,
    }
}

#[test]
fn laying_out_warmed_buffers_never_allocates() {
    let credits = DeviceRect::new(600, 700, 980, 718);
    let name = DeviceRect::new(300, 10, 500, 38);
    let badge = DeviceRect::new(504, 10, 548, 38);
    let captions = [
        cap(
            TextRole::LevelName,
            "A level name",
            name,
            CaptionStyle::TitleCompact,
        ),
        cap(TextRole::Won, "(won)", badge, CaptionStyle::Badge),
        cap(
            TextRole::Hint,
            "Join the beams",
            credits,
            CaptionStyle::Body,
        ),
        cap(
            TextRole::Freeware,
            "Freeware",
            credits,
            CaptionStyle::Credit,
        ),
        cap(
            TextRole::ThankYou,
            "Thank you",
            credits,
            CaptionStyle::Credit,
        ),
        cap(
            TextRole::MoreLevels,
            "More levels",
            credits,
            CaptionStyle::Credit,
        ),
    ];
    let mut out = CaptionLines::new();
    lay_out(&captions, 1000, 2.0, |_, _| 30.0, &mut out);
    let n = allocations(|| {
        for _ in 0..10 {
            lay_out(&captions, 1000, 2.0, |_, _| 30.0, &mut out);
        }
    });
    assert_eq!(n, 0);
    assert_eq!(out[3].text, "Freeware · Thank you · More levels");
}
