//! Spec §14 for the macOS shell: once its captions are set, a modern frame (paint, re-ink inside
//! the damage, routing, the wake) allocates nothing: a drag over the card and its landing, a cold
//! frame re-inking every caption (Chromatron 1's joined credits among them), a level change's
//! cross-fade frames, and the level sheet opening, open, fading out and the captions coming back.
//! Only a caption change that sets new text (a level change's first frame) may allocate.
//! One test per binary, counting this thread's allocations only.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use chromatron::look_switch::Look;
use chromatron::modern_stage::ModernStage;
use chromatron::press_gate::{PressGate, place, route};
use chromatron::viewport::Viewport;
use halation::{Control, Easel, Theme};
use lumen::{BOARD_CELLS, GameState, InputEvent, Piece, RuleSet, SaveData, VISIBLE_TOOLBOX_SLOTS};

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

/// A pointer event at physical (`x`, `y`) through the gate, as the shell does: the cursor moves,
/// is placed in the modern look's units and routed. `kind` builds the event from that point.
fn pointer(
    gate: &mut PressGate,
    stage: &mut ModernStage,
    state: &mut GameState<'static>,
    (x, y): (i32, i32),
    kind: fn(i32, i32) -> InputEvent,
) {
    gate.moved(f64::from(x), f64::from(y));
    let (x, y) = place(Look::Modern, &Viewport::fit(1, 1), gate.cursor());
    route(Some(stage), state, kind(x, y));
}

fn down(x: i32, y: i32) -> InputEvent {
    InputEvent::MouseDown { x, y, alt: false }
}

fn drag(x: i32, y: i32) -> InputEvent {
    InputEvent::MouseDragged { x, y }
}

fn up(x: i32, y: i32) -> InputEvent {
    InputEvent::MouseUp { x, y, alt: false }
}

/// Paints frame by frame from `now` (60 fps) until nothing animates; the frames painted.
fn settle(
    stage: &mut ModernStage,
    state: &GameState<'static>,
    px: &mut [u32],
    mut now: f64,
) -> (usize, f64) {
    for frames in 1..=400 {
        stage.paint(state, now, px);
        match stage.next_wake(now) {
            Some(at) => now = at,
            None => return (frames, now),
        }
    }
    panic!("still animating after 400 frames");
}

#[test]
fn a_warm_modern_frame_never_allocates() {
    let data = chromatron::CHROMATRON_1.data;
    // Slot 1 of Chromatron 1: its hint and the joined credits ("freeware · the site").
    let mut state = GameState::new(data, SaveData::fresh(RuleSet::of(data.game)));
    let easel = Easel::new(1280, 800, 1.0);
    let mut stage = ModernStage::new(easel, Theme::Dark, false);
    let mut px = vec![0u32; 1280 * 800];
    stage.paint(&state, 0.0, &mut px);
    let t = stage.lightbox().tableau().clone();
    let slot = (BOARD_CELLS..BOARD_CELLS + VISIBLE_TOOLBOX_SLOTS)
        .find(|&i| state.board().cells[i].piece != Piece::Empty)
        .expect("a tray piece");
    let from = t.cell_centre(slot);
    let (fx, fy) = (from.x as i32, from.y as i32);
    let card = t.device(t.card);
    let mut now = 16.7;

    let onto = t.cell_centre(0);
    let mut held = 0;

    // A held piece lifted onto the board (a drag starts only over a cell), then carried across
    // the card (its lift, then plain drag frames), released there and sliding home.
    let mut gate = PressGate::default();
    let n = allocations(|| {
        gate.press();
        pointer(&mut gate, &mut stage, &mut state, (fx, fy), down);
        let to = (onto.x as i32, onto.y as i32);
        pointer(&mut gate, &mut stage, &mut state, to, drag);
        let y = (card.top + card.bottom) / 2;
        for k in 0..12 {
            let x = card.left + (card.right - card.left) * k / 12;
            pointer(&mut gate, &mut stage, &mut state, (x, y), drag);
            stage.paint(&state, now, &mut px);
            held += usize::from(state.drag().is_some());
            let _ = stage.next_wake(now);
            now += 16.7;
        }
        assert!(gate.release());
        pointer(&mut gate, &mut stage, &mut state, (card.right - 1, y), up);
        now = settle(&mut stage, &state, &mut px, now).1;
    });
    assert_eq!(n, 0, "a drag over the card and its landing");
    assert_eq!(held, 12, "the piece was held over the card");

    // A cold frame (a look switch's): every caption re-inked inside the whole-canvas damage.
    let n = allocations(|| {
        stage.reset();
        stage.paint(&state, now, &mut px);
    });
    assert_eq!(n, 0, "a cold frame re-inking the captions");

    // A level change: its first frame sets the new card's text (allowed, spec §14); the
    // cross-fade and sweep frames after it are warm.
    state.goto_slot(1);
    now += 16.7;
    stage.paint(&state, now, &mut px);
    let mut frames = 0;
    let n = allocations(|| {
        let next = stage.next_wake(now).expect("a level change animates");
        frames = settle(&mut stage, &state, &mut px, next).0;
    });
    assert_eq!(n, 0, "a level change's {frames} animation frames");
    assert!(frames > 2, "{frames} frames");

    // The level sheet (800 × 600 pt @2: the picker is a sheet) opening, open, fading out, and the
    // captions inked again from the cache once the fade ends.
    let big = Easel::new(1600, 1200, 2.0);
    let mut sheet_stage = ModernStage::new(big, Theme::Dark, false);
    let mut px = vec![0u32; 1600 * 1200];
    sheet_stage.paint(&state, now, &mut px);
    let shown = sheet_stage.caption_rects().count();
    assert!(shown > 0 && !sheet_stage.has_inline_picker());
    let n = allocations(|| {
        assert!(sheet_stage.command(Control::Levels));
        now = settle(&mut sheet_stage, &state, &mut px, now + 16.7).1;
        assert!(sheet_stage.command(Control::CloseSheet));
        settle(&mut sheet_stage, &state, &mut px, now + 16.7);
    });
    assert_eq!(n, 0, "the sheet opening and closing");
    assert_eq!(
        sheet_stage.caption_rects().count(),
        shown,
        "the captions came back"
    );

    // A compact card (800 × 700 pt @2, ruling P2) of a level solved earlier: its name widens to
    // the "(won)" badge's text, the one layout that calls `measure` (Core Text). A drag across
    // the card and its landing after the first paint; a layout per frame would show here.
    let data = chromatron::CHROMATRON_2.data;
    let mut state = GameState::new(data, SaveData::fresh(RuleSet::of(data.game)));
    let solution = lumen::stored_solution(data, 0).expect("a stored solution");
    state.handle(InputEvent::Paste(solution));
    assert!(state.is_won());
    state.handle(InputEvent::Key(b'r'));
    assert!(!state.is_won(), "restarted, solved earlier");
    let compact = Easel::new(1600, 1400, 2.0);
    let mut stage = ModernStage::new(compact, Theme::Dark, false);
    let mut px = vec![0u32; 1600 * 1400];
    stage.paint(&state, 0.0, &mut px);
    let t = stage.lightbox().tableau().clone();
    assert!(t.card_parts.name.is_none(), "a compact card");
    assert!(stage.caption_rects().count() > 0);
    let from = (BOARD_CELLS..BOARD_CELLS + VISIBLE_TOOLBOX_SLOTS)
        .find(|&i| state.board().cells[i].piece.is_movable())
        .map(|i| t.cell_centre(i))
        .expect("a movable tray piece");
    let onto = t.cell_centre(0);
    let card = t.device(t.card);
    let mut held = 0;
    let mut now = 16.7;
    let mut gate = PressGate::default();
    let n = allocations(|| {
        gate.press();
        let at = (from.x as i32, from.y as i32);
        pointer(&mut gate, &mut stage, &mut state, at, down);
        let to = (onto.x as i32, onto.y as i32);
        pointer(&mut gate, &mut stage, &mut state, to, drag);
        let y = (card.top + card.bottom) / 2;
        for k in 0..8 {
            let x = card.left + (card.right - card.left) * k / 8;
            pointer(&mut gate, &mut stage, &mut state, (x, y), drag);
            stage.paint(&state, now, &mut px);
            held += usize::from(state.drag().is_some());
            let _ = stage.next_wake(now);
            now += 16.7;
        }
        assert!(gate.release());
        pointer(&mut gate, &mut stage, &mut state, (card.right - 1, y), up);
        settle(&mut stage, &state, &mut px, now);
    });
    assert_eq!(n, 0, "a compact card's drag and landing");
    assert_eq!(held, 8, "the piece was held over the compact card");
}
