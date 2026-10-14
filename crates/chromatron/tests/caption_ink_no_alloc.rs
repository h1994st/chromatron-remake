//! `CaptionTypesetter::ink` with every caption already set never allocates, whole or within
//! damage (spec §14): a warm frame calls no Core Text.
//! One test per binary, counting this thread's allocations only.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use chromatron::caption_typeset::{Align, CaptionLine, CaptionTypesetter, Clip};
use halation::{CaptionStyle, DeviceRect, Theme};

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

fn line(text: &str, style: CaptionStyle, align: Align, rect: DeviceRect) -> CaptionLine {
    CaptionLine {
        text: text.into(),
        rect,
        style,
        alpha: 1.0,
        align,
    }
}

#[test]
fn inking_set_captions_never_allocates() {
    let lines = [
        line(
            "A level name",
            CaptionStyle::TitleCompact,
            Align::Right,
            DeviceRect::new(300, 10, 500, 66),
        ),
        line(
            "(won)",
            CaptionStyle::Badge,
            Align::Right,
            DeviceRect::new(508, 10, 596, 66),
        ),
        line(
            "Levels",
            CaptionStyle::Pill,
            Align::Left,
            DeviceRect::new(660, 10, 748, 66),
        ),
        line(
            "Join the beams.\rThen rotate the prism.",
            CaptionStyle::Body,
            Align::Left,
            DeviceRect::new(40, 80, 780, 380),
        ),
        line(
            "freeware · silverspaceship.com",
            CaptionStyle::Credit,
            Align::Centre,
            DeviceRect::new(40, 400, 780, 436),
        ),
    ];
    let (width, height) = (800, 450);
    let mut canvas = vec![0u32; width * height];
    let palette = Theme::Dark.palette();
    let mut typesetter = CaptionTypesetter::new(2.0);
    typesetter.ink(&lines, palette, &mut canvas, width, height, Clip::Whole);
    let damage = [
        DeviceRect::new(0, 0, 400, 450),
        DeviceRect::new(350, 100, 800, 200),
    ];
    let n = allocations(|| {
        for _ in 0..10 {
            typesetter.ink(&lines, palette, &mut canvas, width, height, Clip::Whole);
            let clip = Clip::Within(&damage);
            typesetter.ink(&lines, palette, &mut canvas, width, height, clip);
        }
    });
    assert_eq!(n, 0);
    assert_eq!((typesetter.cached(), typesetter.sets()), (5, 5));
}
