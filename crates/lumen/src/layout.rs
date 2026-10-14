//! Screen geometry in window pixels, origin top-left, y down (platform.md P2–P3, logic.md L7).

use crate::board::{BOARD_CELLS, SIDE, VISIBLE_TOOLBOX_SLOTS, board_index, toolbox_index};

pub const FRAME_WIDTH: usize = 640;
pub const FRAME_HEIGHT: usize = 480;
/// Strip slots shown (2 rows of 25).
pub const STRIP_SLOTS: usize = 50;
pub const LAST_SLOT: usize = STRIP_SLOTS - 1;

const CELL: i32 = 24;
const BOARD_LEFT: i32 = 48;
const BOARD_TOP: i32 = 18;
const TOOLBOX_LEFT: i32 = 448;
const TOOLBOX_TOP: i32 = 8;
const TOOLBOX_PITCH: i32 = 26;
const TOOLBOX_COLUMNS: usize = 6;
const STRIP_LEFT: i32 = 10;
const STRIP_TOP: i32 = 410;
const STRIP_PITCH: i32 = 20;
const STRIP_COLUMNS: usize = 25;

/// A rectangle `[left, right) × [top, bottom)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }
}

pub const FULL_FRAME: Rect = Rect::new(0, 0, FRAME_WIDTH as i32, FRAME_HEIGHT as i32);
/// Text box rects, `draw_text_box(str, left, top, right, bottom)` in `paint_window`.
pub const YOU_WIN_RECT: Rect = Rect::new(330, 385, 400, 420);
pub const WON_RECT: Rect = Rect::new(350, 385, 400, 420);
pub const HINT_RECT: Rect = Rect::new(450, 125, 620, 475);
pub const FREEWARE_RECT: Rect = Rect::new(0, 450, 100, 480);
pub const MORE_LEVELS_RECT: Rect = Rect::new(350, 450, 465, 480);
pub const SITE_URL_RECT: Rect = Rect::new(470, 450, 640, 480);
/// Chromatron 2 on (re/notes/chromatron2/platform.md P3), in play (`game_mode == 2`).
pub const LEVEL_NAME_RECT: Rect = Rect::new(50, 385, 300, 420);
pub const THANK_YOU_RECT: Rect = Rect::new(0, 450, 400, 480);
/// The 15 × 15 board: every pixel in it hits a board cell, and no pixel outside it does.
pub const BOARD_RECT: Rect = Rect::new(
    BOARD_LEFT,
    BOARD_TOP,
    BOARD_LEFT + CELL * SIDE as i32,
    BOARD_TOP + CELL * SIDE as i32,
);
/// The visible toolbox: its 6 × 4 slots on their 26 px pitch, with the 2 px gap on every side.
/// Every toolbox hit lies inside it.
pub const TOOLBOX_RECT: Rect = Rect::new(
    TOOLBOX_LEFT - (TOOLBOX_PITCH - CELL),
    TOOLBOX_TOP - (TOOLBOX_PITCH - CELL),
    TOOLBOX_LEFT + TOOLBOX_PITCH * TOOLBOX_COLUMNS as i32,
    TOOLBOX_TOP + TOOLBOX_PITCH * (VISIBLE_TOOLBOX_SLOTS / TOOLBOX_COLUMNS) as i32,
);

/// Top-left pixel of the 24 × 24 sprite of cell `index` (a board cell or a visible toolbox slot).
pub fn cell_origin(index: usize) -> (i32, i32) {
    if index < BOARD_CELLS {
        let (x, y) = ((index % SIDE) as i32, (index / SIDE) as i32);
        (BOARD_LEFT + CELL * x, BOARD_TOP + CELL * y)
    } else {
        let slot = index - BOARD_CELLS;
        let (col, row) = (
            (slot % TOOLBOX_COLUMNS) as i32,
            (slot / TOOLBOX_COLUMNS) as i32,
        );
        (
            TOOLBOX_LEFT + TOOLBOX_PITCH * col,
            TOOLBOX_TOP + TOOLBOX_PITCH * row,
        )
    }
}

/// Top-left pixel of the number for strip slot `slot`.
pub fn strip_origin(slot: usize) -> (i32, i32) {
    let (col, row) = ((slot % STRIP_COLUMNS) as i32, (slot / STRIP_COLUMNS) as i32);
    (
        STRIP_LEFT + STRIP_PITCH * col,
        STRIP_TOP + STRIP_PITCH * row,
    )
}

/// What a window pixel is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A board cell or a visible toolbox slot, as a cell index.
    Cell(usize),
    /// A strip slot, 0..50.
    Strip(usize),
    Nothing,
}

/// The original's hit test (`handle_mouse` @ 0x50d4), C integer division and all: board first,
/// then toolbox, then strip.
pub fn hit_test(x: i32, y: i32) -> Hit {
    let in_range = |v: i32, n: usize| (0..n as i32).contains(&v);
    let (col, row) = (x / CELL - 2, (y + 30) / CELL - 2);
    if in_range(col, SIDE) && in_range(row, SIDE) {
        return Hit::Cell(board_index(col as usize, row as usize));
    }
    let (col, row) = ((x - 422) / TOOLBOX_PITCH - 1, (y + 18) / TOOLBOX_PITCH - 1);
    if in_range(col, TOOLBOX_COLUMNS) && in_range(row, VISIBLE_TOOLBOX_SLOTS / TOOLBOX_COLUMNS) {
        return Hit::Cell(toolbox_index(col as usize + TOOLBOX_COLUMNS * row as usize));
    }
    let (col, row) = ((x + 10) / STRIP_PITCH - 1, (y - 390) / STRIP_PITCH - 1);
    if in_range(col, STRIP_COLUMNS) && in_range(row, STRIP_SLOTS / STRIP_COLUMNS) {
        return Hit::Strip(col as usize + STRIP_COLUMNS * row as usize);
    }
    Hit::Nothing
}

/// A pixel geometry for pointer input: what each pixel is over, and how far a press must move to
/// become a drag. Classic is the original window (`ClassicFloorplan`); the modern look supplies
/// its own. Coordinates are the floorplan's pixels.
pub trait Floorplan {
    /// What pixel (`x`, `y`) is over.
    fn hit(&self, x: i32, y: i32) -> Hit;
    /// A drag starts once the pointer is more than this many pixels from the press on an axis.
    fn drag_slop(&self) -> i32;
}

/// The original 640 × 480 window: `hit_test` and a 1 px slop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClassicFloorplan;

impl Floorplan for ClassicFloorplan {
    fn hit(&self, x: i32, y: i32) -> Hit {
        hit_test(x, y)
    }

    fn drag_slop(&self) -> i32 {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_classic_floorplan_is_the_original_hit_test() {
        let plan = ClassicFloorplan;
        assert_eq!(plan.drag_slop(), 1);
        for y in -(FRAME_HEIGHT as i32)..2 * FRAME_HEIGHT as i32 {
            for x in -(FRAME_WIDTH as i32)..2 * FRAME_WIDTH as i32 {
                assert_eq!(plan.hit(x, y), hit_test(x, y), "({x}, {y})");
            }
        }
        for (x, y) in [
            (-0x10000, -0x10000),
            (0x10000, 0x10000),
            (-0x10000, 420),
            (100, 0x10000),
        ] {
            assert_eq!(plan.hit(x, y), hit_test(x, y), "({x}, {y})");
        }
    }

    #[test]
    fn board_cells_sit_on_a_24_px_grid() {
        assert_eq!(cell_origin(board_index(0, 0)), (48, 18));
        assert_eq!(cell_origin(board_index(14, 14)), (384, 354));
        assert_eq!(hit_test(48, 18), Hit::Cell(board_index(0, 0)));
        assert_eq!(hit_test(407, 377), Hit::Cell(board_index(14, 14)));
        assert_eq!(hit_test(47, 18), Hit::Nothing);
        assert_eq!(hit_test(408, 200), Hit::Nothing);
        assert_eq!(hit_test(200, 378), Hit::Nothing);
    }

    #[test]
    fn toolbox_slots_sit_on_a_26_px_pitch_and_their_gaps_hit_too() {
        assert_eq!(cell_origin(toolbox_index(0)), (448, 8));
        assert_eq!(cell_origin(toolbox_index(23)), (578, 86));
        assert_eq!(hit_test(448, 8), Hit::Cell(toolbox_index(0)));
        assert_eq!(
            hit_test(473, 33),
            Hit::Cell(toolbox_index(0)),
            "the 2 px gap belongs to the slot"
        );
        assert_eq!(hit_test(603, 111), Hit::Cell(toolbox_index(23)));
        assert_eq!(hit_test(604, 50), Hit::Nothing);
        assert_eq!(hit_test(500, 112), Hit::Nothing);
    }

    #[test]
    fn strip_numbers_sit_on_a_20_px_pitch() {
        assert_eq!(strip_origin(0), (10, 410));
        assert_eq!(strip_origin(49), (490, 430));
        assert_eq!(hit_test(10, 410), Hit::Strip(0));
        assert_eq!(hit_test(509, 449), Hit::Strip(49));
        assert_eq!(hit_test(9, 420), Hit::Nothing);
        assert_eq!(hit_test(100, 450), Hit::Nothing);
    }

    #[test]
    fn the_board_rect_is_exactly_the_board_cells() {
        assert_eq!(BOARD_RECT, Rect::new(48, 18, 408, 378));
        for y in 0..FRAME_HEIGHT as i32 {
            for x in 0..FRAME_WIDTH as i32 {
                let inside = (BOARD_RECT.left..BOARD_RECT.right).contains(&x)
                    && (BOARD_RECT.top..BOARD_RECT.bottom).contains(&y);
                let on_board = matches!(hit_test(x, y), Hit::Cell(i) if i < BOARD_CELLS);
                assert_eq!(inside, on_board, "({x}, {y})");
            }
        }
    }

    #[test]
    fn the_toolbox_rect_holds_every_slot_and_every_toolbox_hit() {
        assert_eq!(TOOLBOX_RECT, Rect::new(446, 6, 604, 112));
        let inside = |x: i32, y: i32| {
            (TOOLBOX_RECT.left..TOOLBOX_RECT.right).contains(&x)
                && (TOOLBOX_RECT.top..TOOLBOX_RECT.bottom).contains(&y)
        };
        for slot in 0..VISIBLE_TOOLBOX_SLOTS {
            let (x, y) = cell_origin(toolbox_index(slot));
            assert!(
                inside(x, y) && inside(x + CELL - 1, y + CELL - 1),
                "slot {slot}"
            );
        }
        for y in 0..FRAME_HEIGHT as i32 {
            for x in 0..FRAME_WIDTH as i32 {
                if matches!(hit_test(x, y), Hit::Cell(i) if i >= BOARD_CELLS) {
                    assert!(inside(x, y), "({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn negative_coordinates_hit_nothing() {
        assert_eq!(hit_test(-30, -30), Hit::Nothing);
        assert_eq!(hit_test(-5, 420), Hit::Nothing);
    }
}
