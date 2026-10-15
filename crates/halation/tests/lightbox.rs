//! The lightbox façade on real levels: cold and incremental paints, damage, the drag overlay,
//! hit testing through `Floorplan`, and memory.

mod common;

use common::{SETTLED, cold, state};
use halation::{Control, DeviceRect, Easel, Lightbox, MAX_DAMAGE, Output, Scene, Theme};
use lumen::{
    BOARD_CELLS, Floorplan, GameState, Hit, InputEvent, Piece, SIDE, VISIBLE_TOOLBOX_SLOTS,
    board_index, toolbox_index,
};
use spectrograph::Game;

const EASEL: (u32, u32, f32) = (800, 600, 1.0);

fn easel() -> Easel {
    Easel::new(EASEL.0, EASEL.1, EASEL.2)
}

/// The reference 1280 × 800 at @1: Beside with an inline picker.
fn easel_1280() -> Easel {
    Easel::new(1280, 800, 1.0)
}

fn canvas(easel: Easel) -> Vec<u32> {
    vec![0u32; (easel.width * easel.height) as usize]
}

fn full(easel: Easel) -> DeviceRect {
    DeviceRect::new(0, 0, easel.width as i32, easel.height as i32)
}

/// Drags the first tray piece to the first empty board cell through `plan`; returns that cell.
fn drag_first_piece(
    state: &mut GameState,
    plan: &impl Floorplan,
    tableau: &halation::Tableau,
) -> Option<usize> {
    let slot = (0..VISIBLE_TOOLBOX_SLOTS)
        .map(toolbox_index)
        .find(|&i| state.board().cells[i].piece.is_movable())?;
    let target = (0..BOARD_CELLS).find(|&i| state.board().cells[i].piece == Piece::Empty)?;
    let (a, b) = (tableau.cell_centre(slot), tableau.cell_centre(target));
    state.handle_on(
        plan,
        InputEvent::MouseDown {
            x: a.x as i32,
            y: a.y as i32,
            alt: false,
        },
    );
    state.handle_on(
        plan,
        InputEvent::MouseDragged {
            x: b.x as i32,
            y: b.y as i32,
        },
    );
    state.handle_on(
        plan,
        InputEvent::MouseUp {
            x: b.x as i32,
            y: b.y as i32,
            alt: false,
        },
    );
    Some(target)
}

#[test]
fn the_first_paint_is_whole_and_an_unchanged_paint_is_empty() {
    let game = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let mut out = canvas(easel());
    assert_eq!(
        lightbox.paint(&game, 0.0, Output::Xrgb(&mut out)).damage,
        [full(easel())]
    );
    assert!(
        lightbox
            .paint(&game, 16.0, Output::Xrgb(&mut out))
            .damage
            .is_empty()
    );
    assert_eq!(lightbox.next_wake(16.0), None);
    lightbox.reset();
    assert_eq!(
        lightbox.paint(&game, 32.0, Output::Xrgb(&mut out)).damage,
        [full(easel())],
        "reset makes it cold"
    );
    let other = state(Game::C2, 0, false);
    assert_eq!(
        lightbox.paint(&other, 48.0, Output::Xrgb(&mut out)).damage,
        [full(easel())],
        "a game change is cold"
    );
}

#[test]
fn the_floorplan_is_the_tableaus() {
    let lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let c = lightbox.tableau().cell_centre(toolbox_index(0));
    assert_eq!(
        lightbox.hit(c.x as i32, c.y as i32),
        Hit::Cell(toolbox_index(0))
    );
    assert_eq!(lightbox.hit(-1, -1), Hit::Nothing);
    assert_eq!(lightbox.drag_slop(), 1);
}

#[test]
fn a_drag_repaints_only_what_it_touches() {
    let mut game = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let mut out = canvas(easel());
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    let tableau = lightbox.tableau().clone();
    let slot = toolbox_index(0);
    let (a, b) = (tableau.cell_centre(slot), tableau.cell_centre(0));
    game.handle_on(
        &lightbox,
        InputEvent::MouseDown {
            x: a.x as i32,
            y: a.y as i32,
            alt: false,
        },
    );
    game.handle_on(
        &lightbox,
        InputEvent::MouseDragged {
            x: b.x as i32,
            y: b.y as i32,
        },
    );
    // Settled: the drag's own damage, with no lift rising (Task 8 animates it).
    let damage = lightbox
        .paint(&game, SETTLED, Output::Xrgb(&mut out))
        .damage
        .to_vec();
    assert!(
        !damage.is_empty() && !damage.contains(&full(easel())),
        "{damage:?}"
    );
    game.handle_on(
        &lightbox,
        InputEvent::MouseDragged {
            x: b.x as i32 + 5,
            y: b.y as i32 + 3,
        },
    );
    let moved = lightbox
        .paint(&game, SETTLED, Output::Xrgb(&mut out))
        .damage
        .to_vec();
    assert_eq!(moved.len(), 2, "the old and new lifted boxes: {moved:?}");
    let at = b.y as usize * EASEL.0 as usize + b.x as usize + 5;
    let mut resting = Scene::of(&game);
    resting.drag = None;
    assert_ne!(
        out[at],
        cold(easel(), resting)[at],
        "the lifted piece is drawn"
    );
    assert!(
        out == cold(easel(), Scene::of(&game)),
        "and the picture equals a cold paint"
    );
}

#[test]
fn a_risen_drag_repaints_only_its_boxes() {
    // As a shell paints a drag: picked up at 16 ms, painted again at 216 ms (the lift has risen
    // and its boxes are recomposed), then moved and painted at 232 ms: just the old and new
    // lifted boxes.
    let mut game = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let mut out = canvas(easel());
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    let tableau = lightbox.tableau().clone();
    let (a, b) = (
        tableau.cell_centre(toolbox_index(0)),
        tableau.cell_centre(0),
    );
    let (ax, ay, bx, by) = (a.x as i32, a.y as i32, b.x as i32, b.y as i32);
    game.handle_on(
        &lightbox,
        InputEvent::MouseDown {
            x: ax,
            y: ay,
            alt: false,
        },
    );
    game.handle_on(&lightbox, InputEvent::MouseDragged { x: bx, y: by });
    lightbox.paint(&game, 16.0, Output::Xrgb(&mut out));
    lightbox.paint(&game, 216.0, Output::Xrgb(&mut out));
    assert_eq!(lightbox.next_wake(216.0), None, "risen");
    game.handle_on(
        &lightbox,
        InputEvent::MouseDragged {
            x: bx + 5,
            y: by + 3,
        },
    );
    let moved = lightbox
        .paint(&game, 232.0, Output::Xrgb(&mut out))
        .damage
        .to_vec();
    assert_eq!(moved.len(), 2, "the old and new lifted boxes: {moved:?}");
    assert!(
        out == cold(easel(), Scene::of(&game)),
        "and the picture equals a cold paint"
    );
}

/// Drags tray slot 0's piece over `cell` (off its centre, see `off_centre`) and holds it there.
fn hold_over(game: &mut GameState, lightbox: &Lightbox, cell: usize) {
    let a = lightbox.tableau().cell_centre(toolbox_index(0));
    game.handle_on(
        lightbox,
        InputEvent::MouseDown {
            x: a.x as i32,
            y: a.y as i32,
            alt: false,
        },
    );
    carry(game, lightbox, off_centre(lightbox, cell));
    assert!(game.drag().is_some());
}

/// A point inside board `cell`, 0.35u right of and below its centre, as a hand holds a piece: the
/// drop ring (around the cell's centre) then reaches past the lifted piece's box, so its own
/// damage is what repaints it.
fn off_centre(lightbox: &Lightbox, cell: usize) -> (i32, i32) {
    let t = lightbox.tableau();
    let (c, d) = (t.cell_centre(cell), 0.35 * t.cell as f32);
    ((c.x + d) as i32, (c.y + d) as i32)
}

/// The drop ring's top point for a drag held over `cell`: 0.62u above its centre, clear of the
/// lifted piece and its shadow (and of C1 level 1's beam along row 7).
fn ring_point(lightbox: &Lightbox, cell: usize) -> usize {
    let t = lightbox.tableau();
    let c = t.cell_centre(cell);
    let (x, y) = (c.x as usize, (c.y - 0.62 * t.cell as f32).round() as usize);
    y * lightbox.easel().width as usize + x
}

/// `cell`'s centre in whole device px.
fn centre_px(lightbox: &Lightbox, cell: usize) -> (i32, i32) {
    let c = lightbox.tableau().cell_centre(cell);
    (c.x as i32, c.y as i32)
}

/// Carries the held piece to (`x`, `y`).
fn carry(game: &mut GameState, lightbox: &Lightbox, (x, y): (i32, i32)) {
    game.handle_on(lightbox, InputEvent::MouseDragged { x, y });
}

/// Releases the pointer at (`x`, `y`).
fn release(game: &mut GameState, lightbox: &Lightbox, (x, y): (i32, i32)) {
    game.handle_on(lightbox, InputEvent::MouseUp { x, y, alt: false });
}

/// An incremental paint of `game` that must equal a fresh lightbox's cold paint.
fn frame(lightbox: &mut Lightbox, game: &GameState, out: &mut [u32], what: &str) {
    let easel = *lightbox.easel();
    lightbox.paint(game, SETTLED, Output::Xrgb(out));
    assert!(
        out == cold(easel, Scene::of(game)).as_slice(),
        "{what}: differs from a cold paint"
    );
}

/// The dashed outline's four solid corner arcs (at 45°) for a cell of `u` px around `cell`'s
/// centre: each must have brightened some pixel of its 3 × 3 neighbourhood by more than 8 against
/// `bare` (the same picture without the outline).
fn corners_lit(lightbox: &Lightbox, cell: usize, u: f32, out: &[u32], bare: &[u32]) -> usize {
    let c = lightbox.tableau().cell_centre(cell);
    let width = lightbox.easel().width as i32;
    // The arcs' centres are 0.43u − 0.16u from the cell's centre; their radius is 0.16u.
    let d = (0.43 - 0.16 + 0.16 * std::f32::consts::FRAC_1_SQRT_2) * u;
    [(-d, -d), (d, -d), (d, d), (-d, d)]
        .iter()
        .filter(|&&(dx, dy)| {
            let (x, y) = ((c.x + dx) as i32, (c.y + dy) as i32);
            (-1..=1).any(|j| {
                (-1..=1).any(|i| {
                    let k = ((y + j) * width + x + i) as usize;
                    (out[k] & 0xff) > (bare[k] & 0xff) + 8
                })
            })
        })
        .count()
}

/// An occupied board cell that is not a wall, away from the border and ≥ 3 cells from `away`.
fn mid_board_piece(game: &GameState, away: usize) -> usize {
    let (ax, ay) = ((away % SIDE) as i32, (away / SIDE) as i32);
    (0..BOARD_CELLS)
        .find(|&i| {
            let (x, y) = ((i % SIDE) as i32, (i / SIDE) as i32);
            let piece = game.board().cells[i].piece;
            (2..SIDE as i32 - 2).contains(&x)
                && (2..SIDE as i32 - 2).contains(&y)
                && !matches!(piece, Piece::Empty | Piece::Wall)
                && ((x - ax).abs() >= 3 || (y - ay).abs() >= 3)
        })
        .expect("a piece mid-board")
}

#[test]
fn a_held_piece_shows_its_drop_ring_over_an_empty_cell() {
    let easel = easel_1280();
    let target = board_index(8, 7);
    let mut game = state(Game::C1, 0, false);
    assert_eq!(game.board().cells[target].piece, Piece::Empty);
    let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
    let mut out = canvas(easel);
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    let plain = out.clone();
    hold_over(&mut game, &lightbox, target);
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    let ring = ring_point(&lightbox, target);
    let at = |px: &[u32]| px[ring] & 0xff;
    assert!(
        at(&out) > at(&plain) + 60,
        "the ring at 0.62u: {} vs {}",
        at(&out),
        at(&plain)
    );
    assert!(
        out == cold(easel, Scene::of(&game)),
        "mid-drag equals a cold paint"
    );
    // The tray slot left behind (a tray-source drag) shows the dashed outline: against the slot
    // simply emptied, all four of its solid corner arcs brightened.
    let t = lightbox.tableau();
    let mut emptied = game.board().clone();
    emptied.cells[toolbox_index(0)].piece = Piece::Empty;
    let bare = cold(
        easel,
        Scene {
            board: &emptied,
            drag: None,
            ..Scene::of(&game)
        },
    );
    let u = t.tray_piece_px() as f32;
    assert_eq!(
        corners_lit(&lightbox, toolbox_index(0), u, &out, &bare),
        4,
        "a dashed outline where the piece was"
    );
    let same = |r: DeviceRect, a: &[u32], b: &[u32]| {
        (r.top..r.bottom).all(|y| {
            (r.left..r.right).all(|x| a[(y * 1280 + x) as usize] == b[(y * 1280 + x) as usize])
        })
    };
    let next_slot = t.cell_rect(toolbox_index(1));
    assert!(same(next_slot, &out, &bare), "only that slot's outline");
    // Carried on to another empty cell, the ring moves with it; dropped, it goes.
    let other = board_index(6, 7);
    assert_eq!(game.board().cells[other].piece, Piece::Empty);
    let (x, y) = off_centre(&lightbox, other);
    game.handle_on(&lightbox, InputEvent::MouseDragged { x, y });
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert_eq!(out[ring], plain[ring], "the old ring is gone");
    let moved = ring_point(&lightbox, other);
    assert!(
        (out[moved] & 0xff) > (plain[moved] & 0xff) + 60,
        "a new one"
    );
    assert!(out == cold(easel, Scene::of(&game)), "moved equals cold");
    game.handle_on(&lightbox, InputEvent::MouseUp { x, y, alt: false });
    assert_ne!(game.board().cells[other].piece, Piece::Empty, "it landed");
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert!(out == cold(easel, Scene::of(&game)), "the drop equals cold");
}

/// An empty tray slot is a drop target too: its ring is sized by the tray piece, and a drag
/// carried from the board to it and back paints like cold paints.
#[test]
fn an_empty_tray_slot_is_a_drop_target() {
    let easel = easel_1280();
    let mut game = state(Game::C1, 0, false);
    let slot = (0..VISIBLE_TOOLBOX_SLOTS)
        .map(toolbox_index)
        .find(|&i| game.board().cells[i].piece == Piece::Empty)
        .unwrap();
    let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
    let mut out = canvas(easel);
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    let plain = out.clone();
    hold_over(&mut game, &lightbox, board_index(8, 7));
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    let t = lightbox.tableau().clone();
    let c = t.cell_centre(slot);
    // Off the slot's centre, inside it.
    let d = 0.3 * t.tray_piece_px() as f32;
    let (x, y) = ((c.x + d) as i32, (c.y + d) as i32);
    assert_eq!(lightbox.hit(x, y), Hit::Cell(slot));
    game.handle_on(&lightbox, InputEvent::MouseDragged { x, y });
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert!(out == cold(easel, Scene::of(&game)), "over the slot");
    let top = (c.y - 0.62 * t.tray_piece_px() as f32).round() as usize * 1280 + c.x as usize;
    assert!(
        (out[top] & 0xff) > (plain[top] & 0xff) + 60,
        "the slot's ring at 0.62 of a tray piece"
    );
    carry(
        &mut game,
        &lightbox,
        off_centre(&lightbox, board_index(6, 7)),
    );
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert!(out == cold(easel, Scene::of(&game)), "back over the board");
    assert_eq!(out[top], plain[top], "the slot's ring is gone");
}

/// A piece dragged onto the board and back into tray slot 10 lands in lumen's slot 10 in both of
/// Beside's tray shapes, 6×4 (row 1, column 4) and a short touch landscape's 8×3 (row 1, column
/// 2), and every incremental paint equals a cold one.
#[test]
fn a_piece_drops_into_the_tray_in_both_shapes() {
    for (easel, cols) in [
        (easel_1280(), 6),
        (Easel::new(2223, 963, 3.0).with_touch(true), 8),
    ] {
        let mut game = state(Game::C1, 0, false);
        let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
        assert_eq!(lightbox.tableau().tray.cols, cols);
        let mut out = canvas(easel);
        lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
        let t = lightbox.tableau().clone();
        let cell = drag_first_piece(&mut game, &lightbox, &t).expect("a tray piece");
        let piece = game.board().cells[cell].piece;
        lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
        assert!(out == cold(easel, Scene::of(&game)), "{cols}: on the board");
        let slot = toolbox_index(10);
        assert_eq!(game.board().cells[slot].piece, Piece::Empty);
        let (a, b) = (t.cell_centre(cell), t.cell_centre(slot));
        let (a, b) = ((a.x as i32, a.y as i32), (b.x as i32, b.y as i32));
        game.handle_on(
            &lightbox,
            InputEvent::MouseDown {
                x: a.0,
                y: a.1,
                alt: false,
            },
        );
        carry(&mut game, &lightbox, b);
        lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
        assert!(out == cold(easel, Scene::of(&game)), "{cols}: over slot 10");
        release(&mut game, &lightbox, b);
        assert_eq!(game.board().cells[slot].piece, piece, "{cols}: in slot 10");
        assert_eq!(game.board().cells[cell].piece, Piece::Empty);
        lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
        assert!(
            out == cold(easel, Scene::of(&game)),
            "{cols}: back in the tray"
        );
    }
}

/// C1 level 1 at 1280 × 800 painted once, its picture, and tray slot 0's piece held over the
/// empty cell (8, 7) (ring showing, equal to a cold paint).
fn holding() -> (GameState<'static>, Lightbox, Vec<u32>, Vec<u32>) {
    let easel = easel_1280();
    let mut game = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
    let mut out = canvas(easel);
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    let plain = out.clone();
    hold_over(&mut game, &lightbox, EMPTY);
    frame(&mut lightbox, &game, &mut out, "held over an empty cell");
    let ring = ring_point(&lightbox, EMPTY);
    assert!((out[ring] & 0xff) > (plain[ring] & 0xff) + 60, "its ring");
    (game, lightbox, out, plain)
}

/// An empty cell of C1 level 1, mid-board.
const EMPTY: usize = 7 * SIDE + 8;

/// Carried from an empty cell onto a mid-board piece, the ring goes (none over the piece), and a
/// release there does not land: the piece goes back to the tray.
#[test]
fn over_an_occupied_cell_the_ring_goes_and_a_release_does_not_land() {
    let (mut game, mut lightbox, mut out, plain) = holding();
    let occupied = mid_board_piece(&game, EMPTY);
    let over = off_centre(&lightbox, occupied);
    carry(&mut game, &lightbox, over);
    frame(&mut lightbox, &game, &mut out, "over a piece");
    let (old, none) = (
        ring_point(&lightbox, EMPTY),
        ring_point(&lightbox, occupied),
    );
    assert_eq!(out[old], plain[old], "the old ring is gone");
    // 0.62u above the piece: clear of the lifted tile and its shadow, as it was.
    assert_eq!(out[none], plain[none], "no ring over an occupied cell");
    let board = game.board().clone();
    release(&mut game, &lightbox, over);
    assert!(
        game.drag().is_none() && game.board().cells == board.cells,
        "no landing"
    );
    frame(&mut lightbox, &game, &mut out, "the release");
    assert!(out == plain, "the piece is back in its slot");
}

/// Carried off the board (into the canvas's margin), the ring goes; released there, the piece
/// goes back.
#[test]
fn off_the_board_the_ring_goes() {
    let (mut game, mut lightbox, mut out, plain) = holding();
    assert_eq!(lightbox.hit(3, 3), Hit::Nothing);
    carry(&mut game, &lightbox, (3, 3));
    assert!(game.drag().is_some());
    frame(&mut lightbox, &game, &mut out, "off the board");
    let old = ring_point(&lightbox, EMPTY);
    assert_eq!(out[old], plain[old], "the ring is gone");
    release(&mut game, &lightbox, (3, 3));
    frame(&mut lightbox, &game, &mut out, "released off the board");
    assert!(out == plain, "the piece is back in its slot");
}

/// A press while a piece is held cancels the drag (lumen: its release was lost): the piece is
/// back, and the ring and lifted piece are gone.
#[test]
fn a_press_while_holding_cancels_the_drag() {
    let (mut game, mut lightbox, mut out, plain) = holding();
    let (x, y) = centre_px(&lightbox, board_index(6, 7));
    game.handle_on(&lightbox, InputEvent::MouseDown { x, y, alt: false });
    assert!(game.drag().is_none(), "cancelled");
    frame(&mut lightbox, &game, &mut out, "the cancel");
    assert!(out == plain, "as before the drag");
}

/// Carried back over its own (empty-looking) source, the piece shows no ring there, and a
/// release puts it back.
#[test]
fn a_release_over_its_source_puts_the_piece_back() {
    let (mut game, mut lightbox, mut out, plain) = holding();
    let slot = toolbox_index(0);
    carry(&mut game, &lightbox, centre_px(&lightbox, slot));
    frame(&mut lightbox, &game, &mut out, "over its source");
    let t = lightbox.tableau();
    let c = t.cell_centre(slot);
    let top = (c.y - 0.62 * t.tray_piece_px() as f32).round() as usize * 1280 + c.x as usize;
    assert_eq!(out[top], plain[top], "no ring over its own source");
    release(&mut game, &lightbox, centre_px(&lightbox, slot));
    frame(&mut lightbox, &game, &mut out, "released over its source");
    assert!(out == plain, "the piece is back in its slot");
}

/// A piece lifted from the board leaves its cell's floor with a dashed outline; dropped on an
/// empty cell, it lands.
#[test]
fn a_board_piece_lifted_leaves_a_dashed_cell_and_lands() {
    let (mut game, mut lightbox, mut out, _) = holding();
    // First set it down on a mid-board cell no beam crosses, far from (8, 7).
    let quiet = (0..BOARD_CELLS)
        .find(|&i| {
            let (x, y) = (i % SIDE, i / SIDE);
            (2..SIDE - 2).contains(&x)
                && (2..5).contains(&y)
                && game.board().cells[i].piece == Piece::Empty
                && game.field().outbound[i].iter().all(|&m| m == 0)
        })
        .expect("a quiet empty cell");
    let at = centre_px(&lightbox, quiet);
    carry(&mut game, &lightbox, at);
    release(&mut game, &lightbox, at);
    assert!(game.board().cells[quiet].piece.is_movable(), "set down");
    frame(&mut lightbox, &game, &mut out, "set down");
    // Then lift it from there to (8, 7).
    game.handle_on(
        &lightbox,
        InputEvent::MouseDown {
            x: at.0,
            y: at.1,
            alt: false,
        },
    );
    carry(&mut game, &lightbox, off_centre(&lightbox, EMPTY));
    assert_eq!(game.drag().map(|d| d.source), Some(quiet));
    frame(&mut lightbox, &game, &mut out, "a board piece held");
    let mut emptied = game.board().clone();
    emptied.cells[quiet].piece = Piece::Empty;
    let bare = cold(
        easel_1280(),
        Scene {
            board: &emptied,
            drag: None,
            ..Scene::of(&game)
        },
    );
    let u = lightbox.tableau().cell as f32;
    assert_eq!(
        corners_lit(&lightbox, quiet, u, &out, &bare),
        4,
        "a dashed outline on its cell"
    );
    release(&mut game, &lightbox, off_centre(&lightbox, EMPTY));
    assert!(game.board().cells[EMPTY].piece.is_movable(), "it landed");
    assert_eq!(game.board().cells[quiet].piece, Piece::Empty);
    frame(&mut lightbox, &game, &mut out, "the landing");
}

/// On a touch easel the piece lifts by 1.3 (a larger tile and shadow) and its ring still shows.
#[test]
fn a_touch_lift_shows_its_ring() {
    let easel = Easel::new(780, 1480, 2.0).with_touch(true);
    let mut game = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
    let mut out = canvas(easel);
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    let plain = out.clone();
    hold_over(&mut game, &lightbox, EMPTY);
    frame(&mut lightbox, &game, &mut out, "a touch hold");
    let ring = ring_point(&lightbox, EMPTY);
    assert!(
        (out[ring] & 0xff) > (plain[ring] & 0xff) + 60,
        "the ring at 0.62u, clear of the 1.3 tile"
    );
    let other = board_index(6, 7);
    carry(&mut game, &lightbox, off_centre(&lightbox, other));
    frame(&mut lightbox, &game, &mut out, "a touch carry");
    assert_eq!(out[ring], plain[ring], "the old ring is gone");
    release(&mut game, &lightbox, off_centre(&lightbox, other));
    assert!(game.board().cells[other].piece.is_movable(), "it landed");
    frame(&mut lightbox, &game, &mut out, "a touch drop");
}

/// With the level sheet open the board hits nothing (it is not a drop target), so a piece held
/// over an empty cell when the sheet opens shows no ring under it.
#[test]
fn no_ring_under_the_open_sheet() {
    // 1024 × 768 has a sheet picker.
    let easel = Easel::new(1024, 768, 1.0);
    let target = board_index(8, 7);
    let mut game = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
    assert!(!lightbox.has_inline_picker());
    let sheet_only = common::cold_with(easel, Scene::of(&game), true);
    let mut out = canvas(easel);
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    hold_over(&mut game, &lightbox, target);
    assert!(lightbox.command(Control::Levels));
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert!(out == common::cold_with(easel, Scene::of(&game), true));
    let ring = ring_point(&lightbox, target);
    assert_eq!(out[ring], sheet_only[ring], "no ring under the sheet");
    // Carried under the sheet to another empty cell, still none: only the lifted boxes repaint.
    let other = board_index(6, 7);
    carry(&mut game, &lightbox, off_centre(&lightbox, other));
    let damage = lightbox
        .paint(&game, SETTLED, Output::Xrgb(&mut out))
        .damage
        .len();
    assert_eq!(damage, 2, "the old and new lifted boxes only");
    let ring = ring_point(&lightbox, other);
    assert_eq!(out[ring], sheet_only[ring], "no ring under the sheet");
    assert!(out == common::cold_with(easel, Scene::of(&game), true));
}

#[test]
fn queued_changes_paint_like_a_cold_paint() {
    let mut game = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let mut out = canvas(easel());
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    let tableau = lightbox.tableau().clone();
    for _ in 0..3 {
        drag_first_piece(&mut game, &lightbox, &tableau);
    }
    let painted = lightbox
        .paint(&game, SETTLED, Output::Xrgb(&mut out))
        .damage
        .len();
    assert!(painted <= MAX_DAMAGE);
    assert!(
        out == cold(easel(), Scene::of(&game)),
        "whatever the damage, the picture is right"
    );
}

/// What a random walk covered (see `walk`).
#[derive(Default)]
struct Covered {
    chosen: usize,
    wins: usize,
    sheets: usize,
    keyed_under: usize,
    closes: usize,
    resized_open: usize,
    ringed: usize,
    landed: usize,
    cancels: usize,
    /// Theme switches (walks with `flips` only).
    flips: usize,
}

#[test]
fn incremental_paints_equal_cold_paints() {
    // Several walks, judged on what they cover together, so a reshuffle of one walk's arms can't
    // silently stop covering a case. The drag cases also have their own scripted tests above.
    let mut all = Covered::default();
    for seed in [4, 88, 149, 164, 268] {
        let c = walk(seed, false);
        all.chosen += c.chosen;
        all.wins += c.wins;
        all.sheets += c.sheets;
        all.keyed_under += c.keyed_under;
        all.closes += c.closes;
        all.resized_open += c.resized_open;
        all.ringed += c.ringed;
        all.landed += c.landed;
        all.cancels += c.cancels;
    }
    for (count, what) in [
        (all.chosen, "a level opened through an inline chip"),
        (all.wins, "a level won mid-walk"),
        (all.sheets, "the sheet opened mid-walk"),
        (all.keyed_under, "a level changed under the open sheet"),
        (all.closes, "the sheet closed"),
        (all.resized_open, "a resize with the sheet open"),
        (all.ringed, "a mid-drag frame over a drop target"),
        (all.landed, "a held piece dropped where it landed"),
        (
            all.cancels,
            "a held piece cancelled by a press or a level change",
        ),
    ] {
        assert!(count > 0, "no walk had {what}");
    }
}

#[test]
fn incremental_paints_equal_cold_paints_across_theme_changes() {
    let mut flips = 0;
    for seed in [7, 91, 230] {
        flips += walk(seed, true).flips;
    }
    assert!(flips >= 3, "the walks switched theme");
}

/// 60 random steps from `seed` (wins, presses, drags held and dropped, inline chips, the sheet,
/// game switches and resizes), each frame painted incrementally and checked against a cold paint.
/// With `flips`, a second sequence (so the first one's steps are untouched) switches the theme
/// before one step in ten (ruling L10), and the cold paint is in the theme switched to.
fn walk(start: u32, flips: bool) -> Covered {
    let mut seed = start;
    let mut next = |n: u32| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed % n
    };
    let mut flip_seed = start ^ 0x5eed;
    let mut flip = move || {
        flip_seed ^= flip_seed << 13;
        flip_seed ^= flip_seed >> 17;
        flip_seed ^= flip_seed << 5;
        flip_seed.is_multiple_of(10)
    };
    // Starts at 1280 × 800, the one easel here with an inline picker (and the brand).
    let mut easel = easel_1280();
    let mut theme = Theme::Dark;
    let mut lightbox = Lightbox::new(easel, theme, false);
    let mut games = Game::ALL.map(|g| state(g, 0, false));
    let mut current = 0;
    let mut out = canvas(easel);
    let mut c = Covered::default();
    for step in 0..60 {
        if flips && flip() {
            theme = if theme == Theme::Dark {
                Theme::Light
            } else {
                Theme::Dark
            };
            lightbox.set_theme(theme);
            c.flips += 1;
        }
        let tableau = lightbox.tableau().clone();
        let game = &mut games[current];
        let (held, was) = (game.drag().is_some(), current);
        let mut released = false;
        let before = game.board().cells;
        match next(10) {
            0 | 1 => {
                drag_first_piece(game, &lightbox, &tableau);
            }
            2 => {
                let movable = (0..BOARD_CELLS)
                    .filter(|&i| game.board().cells[i].piece.is_movable())
                    .nth(next(3) as usize);
                if let Some(i) = movable {
                    let c = tableau.cell_centre(i);
                    let alt = next(2) == 0;
                    game.handle_on(
                        &lightbox,
                        InputEvent::MouseDown {
                            x: c.x as i32,
                            y: c.y as i32,
                            alt,
                        },
                    );
                    game.handle_on(
                        &lightbox,
                        InputEvent::MouseUp {
                            x: c.x as i32,
                            y: c.y as i32,
                            alt,
                        },
                    );
                }
            }
            3 => {
                game.handle(InputEvent::Key(game.rules().cheat_key));
                game.handle(InputEvent::Key(b'='));
                c.keyed_under += usize::from(lightbox.sheet_open());
            }
            4 => current = next(4) as usize,
            5 => {
                // A level opened through an inline chip, when there is one.
                if tableau.picker.is_some() {
                    let r = tableau.chip_rect(next(5) as usize);
                    let (x, y) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
                    let was = game.slot();
                    game.handle_on(&lightbox, InputEvent::MouseDown { x, y, alt: false });
                    game.handle_on(&lightbox, InputEvent::MouseUp { x, y, alt: false });
                    c.chosen += usize::from(game.slot() != was);
                }
            }
            6 => {
                // A win (and Next ▶ when the next level is open) mid-walk.
                if let Some(text) = lumen::stored_solution(game.data(), game.slot()) {
                    game.handle(InputEvent::Paste(text));
                    c.wins += usize::from(game.is_won());
                }
            }
            7 => {
                // The level sheet, where the picker is one (an inline picker keeps it shut).
                let open = lightbox.sheet_open();
                lightbox.command(if open {
                    Control::CloseSheet
                } else {
                    Control::Levels
                });
                c.sheets += usize::from(lightbox.sheet_open());
                c.closes += usize::from(open && !lightbox.sheet_open());
            }
            8 => {
                // A piece in the air over a random cell, so later steps paint mid-drag frames: a
                // held piece is dropped where it is, carried to another cell, or replaced by a new
                // press (which cancels it), as other arms' presses do.
                let choice = if held { next(3) } else { 2 };
                if let (0, Some(d)) = (choice, game.drag()) {
                    let up = InputEvent::MouseUp {
                        x: d.x,
                        y: d.y,
                        alt: false,
                    };
                    game.handle_on(&lightbox, up);
                    released = true;
                } else {
                    let to = tableau.cell_centre(next(225) as usize);
                    let (x, y) = (to.x as i32, to.y as i32);
                    let from = (0..VISIBLE_TOOLBOX_SLOTS)
                        .map(toolbox_index)
                        .find(|&i| game.board().cells[i].piece.is_movable());
                    if let (2, Some(from)) = (choice, from) {
                        let c = tableau.cell_centre(from);
                        let down = InputEvent::MouseDown {
                            x: c.x as i32,
                            y: c.y as i32,
                            alt: false,
                        };
                        game.handle_on(&lightbox, down);
                    }
                    game.handle_on(&lightbox, InputEvent::MouseDragged { x, y });
                }
            }
            _ => {
                // Beside (only 1280 × 800 keeps an inline picker), Stacked and compact Beside.
                easel = [
                    Easel::new(800, 600, 1.0),
                    Easel::new(1024, 768, 1.0),
                    Easel::new(1600, 1200, 2.0),
                    Easel::new(780, 1480, 2.0).with_touch(true),
                    Easel::new(1400, 740, 2.0).with_touch(true),
                    easel_1280(),
                ][next(6) as usize];
                c.resized_open += usize::from(lightbox.sheet_open());
                lightbox.resize(easel);
                out = canvas(easel);
            }
        }
        let game = &games[current];
        if current == was {
            match (held, game.drag()) {
                // The held piece over an empty cell other than its own: a drop target shows.
                (_, Some(d)) => {
                    let target = matches!(lightbox.hit(d.x, d.y),
                        Hit::Cell(i) if i != d.source && game.board().cells[i].piece == Piece::Empty);
                    c.ringed += usize::from(!lightbox.sheet_open() && target);
                }
                // A release that moved the piece; one over an occupied cell changes nothing.
                (true, None) if released => {
                    c.landed += usize::from(game.board().cells != before);
                }
                (true, None) => c.cancels += 1,
                (false, None) => {}
            }
        }
        lightbox.paint(game, SETTLED, Output::Xrgb(&mut out));
        assert!(
            out == common::cold_with_in(theme, easel, Scene::of(game), lightbox.sheet_open()),
            "seed {start}, step {step} ({theme:?}): incremental differs from cold"
        );
    }
    c
}

/// The won card of C1 level 1 (solved with its stored solution) at 1280 × 800.
#[test]
fn winning_repaints_the_card_and_shows_next() {
    let mut game = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel_1280(), Theme::Dark, false);
    let mut out = canvas(easel_1280());
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    let before = out.clone();
    let text = lumen::stored_solution(common::data(Game::C1), 0).unwrap();
    game.handle(InputEvent::Paste(text));
    assert!(game.is_won());
    let damage = lightbox
        .paint(&game, SETTLED, Output::Xrgb(&mut out))
        .damage
        .to_vec();
    let t = lightbox.tableau().clone();
    let next = t.device(t.card_parts.next);
    assert!(
        damage.iter().any(|d| d.left <= next.left
            && d.right >= next.right
            && d.top <= next.top
            && d.bottom >= next.bottom),
        "{damage:?}"
    );
    let changed = (next.top..next.bottom).any(|y| {
        (next.left..next.right)
            .any(|x| out[(y * 1280 + x) as usize] != before[(y * 1280 + x) as usize])
    });
    assert!(changed, "Next ▶ appeared");
    assert!(
        out == cold(easel_1280(), Scene::of(&game)),
        "and the picture equals a cold paint"
    );
}

/// Every game's win text is lettered (ruling 7), and like "LEVEL 50" it fits the header's
/// lettering tile and its key, so a cold paint keeps the lettered header instead of lettering it
/// again.
#[test]
fn every_games_win_text_is_letterable() {
    use halation::darkroom::{HEADER_LETTERING_W, LETTERING_TEXT};
    use halation::lettering::{letterable, width};
    for game in Game::ALL {
        let you_win = common::data(game).ui.you_win.as_ref();
        assert!(
            letterable(you_win),
            "{game}: {you_win:?} (else the card typesets it: ruling 7)"
        );
        assert!(
            you_win.len() <= LETTERING_TEXT,
            "{game}: {you_win:?} is keyed"
        );
        let w = width(you_win, 0.3) * 18.0;
        assert!(
            w <= HEADER_LETTERING_W,
            "{game}: {you_win:?} is {w} pt wide"
        );
    }
    for slot in 0..lumen::layout::STRIP_SLOTS {
        let mut buf = [0u8; 9];
        let label = halation::lettering::level_label(slot, &mut buf);
        let w = width(label, 0.3) * 13.0;
        assert!(w <= HEADER_LETTERING_W, "{label} is {w} pt wide");
    }
}

#[test]
fn a_level_change_repaints_the_picker() {
    let mut game = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel_1280(), Theme::Dark, false);
    let mut out = canvas(easel_1280());
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    let picker = lightbox
        .tableau()
        .device(lightbox.tableau().picker.unwrap().rect);
    game.handle(InputEvent::Key(game.rules().cheat_key));
    game.handle(InputEvent::Key(b'='));
    let damage = lightbox
        .paint(&game, SETTLED, Output::Xrgb(&mut out))
        .damage
        .to_vec();
    assert!(
        damage
            .iter()
            .any(|d| d.left <= picker.left && d.right >= picker.right && d.top <= picker.top),
        "{damage:?}"
    );
    assert!(out == cold(easel_1280(), Scene::of(&game)));
}

#[test]
fn the_brand_lights_the_top_of_the_column() {
    let game = state(Game::C2, 0, false);
    let out = cold(easel_1280(), Scene::of(&game));
    let t = halation::Tableau::arrange(&easel_1280());
    let brand = t.device(t.brand.unwrap());
    let brightest = (brand.top..brand.bottom)
        .flat_map(|y| (brand.left..brand.right).map(move |x| (x, y)))
        .map(|(x, y)| out[(y * 1280 + x) as usize] & 0xff)
        .max()
        .unwrap();
    assert!(brightest > 200, "the wordmark's white core: {brightest}");
}

/// Review I4: a cold paint without a reshape (a level change, `reset`) adds the brand's light
/// once, exactly as a fresh lightbox does — where the brand's bloom overlaps the board region.
#[test]
fn a_reset_paint_equals_a_fresh_one_under_the_brand() {
    let game = state(Game::C2, 0, false);
    let mut lightbox = Lightbox::new(easel_1280(), Theme::Dark, false);
    let mut out = canvas(easel_1280());
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    lightbox.reset();
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert!(out == cold(easel_1280(), Scene::of(&game)));
}

/// The brand's lettering is kept across a resize that only moves it and across level changes,
/// and lettered again for another game or size: each paint equals a fresh lightbox's.
#[test]
fn a_kept_brand_paints_like_a_fresh_one() {
    let mut lightbox = Lightbox::new(Easel::new(2560, 1600, 2.0), Theme::Dark, false);
    for (easel, game, slot) in [
        (Easel::new(2560, 1600, 2.0), Game::C2, 0),
        (Easel::new(2561, 1600, 2.0), Game::C2, 0),
        (Easel::new(2560, 1600, 2.0), Game::C2, 3),
        (Easel::new(2560, 1600, 2.0), Game::C3, 3),
        (Easel::new(2451, 1671, 1.75), Game::C3, 3),
        (Easel::new(2450, 1671, 1.75), Game::C4, 0),
        (Easel::new(1280, 800, 1.0), Game::C4, 0),
    ] {
        if lightbox.easel() != &easel {
            lightbox.resize(easel);
        }
        let game = state(game, slot, false);
        let mut out = canvas(easel);
        lightbox.reset();
        lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
        assert!(
            out == cold(easel, Scene::of(&game)),
            "{easel:?} {}: differs from a fresh paint",
            game.data().game
        );
    }
}

/// A press and release at `cell`'s centre: a rotation (clockwise with `alt`).
fn click(state: &mut GameState, plan: &impl Floorplan, tableau: &halation::Tableau, cell: usize) {
    let c = tableau.cell_centre(cell);
    let (x, y) = (c.x as i32, c.y as i32);
    for event in [
        InputEvent::MouseDown { x, y, alt: false },
        InputEvent::MouseUp { x, y, alt: false },
    ] {
        state.handle_on(plan, event);
    }
}

/// An empty board cell, preferring one beside a wall (whose art reaches into it) or else the
/// last empty one.
fn drop_target(state: &GameState) -> Option<usize> {
    let cells = &state.board().cells;
    let wall = |x: i32, y: i32| {
        (0..SIDE as i32).contains(&x)
            && (0..SIDE as i32).contains(&y)
            && cells[y as usize * SIDE + x as usize].piece == Piece::Wall
    };
    let mut empty = (0..BOARD_CELLS).filter(|&i| cells[i].piece == Piece::Empty);
    empty
        .clone()
        .find(|&i| {
            let (x, y) = ((i % SIDE) as i32, (i / SIDE) as i32);
            [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)]
                .iter()
                .any(|&(dx, dy)| wall(x + dx, y + dy))
        })
        .or_else(|| empty.next_back())
}

#[test]
fn board_edits_paint_like_cold_paints() {
    // Pick up from a board cell, move, drop beside a wall, rotate both ways: each frame equals a
    // cold paint. One easel's width is not a multiple of 4.
    let mut checked = 0;
    for game in Game::ALL {
        for slot in [0, 20, 41] {
            // Solved, so pieces stand on the board.
            let solved = lumen::stored_solution(common::data(game), slot).is_some();
            for easel in [Easel::new(1280, 800, 1.0), Easel::new(2451, 1673, 1.75)] {
                let mut state = state(game, slot, solved);
                let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
                let mut out = canvas(easel);
                lightbox.paint(&state, 0.0, Output::Xrgb(&mut out));
                let tableau = lightbox.tableau().clone();
                let Some(source) =
                    (0..BOARD_CELLS).find(|&i| state.board().cells[i].piece.is_movable())
                else {
                    continue;
                };
                let target = drop_target(&state).expect("an empty cell");
                let (a, b) = (tableau.cell_centre(source), tableau.cell_centre(target));
                let mut frame = |state: &GameState, lightbox: &mut Lightbox, what: &str| {
                    lightbox.paint(state, SETTLED, Output::Xrgb(&mut out));
                    assert!(
                        out == cold(easel, Scene::of(state)),
                        "{game} slot {slot} {easel:?}: the {what} frame differs from cold"
                    );
                    checked += 1;
                };
                let (x, y) = (a.x as i32, a.y as i32);
                state.handle_on(&lightbox, InputEvent::MouseDown { x, y, alt: false });
                state.handle_on(&lightbox, InputEvent::MouseDragged { x: x + 9, y: y + 7 });
                assert!(state.drag().is_some(), "{game} slot {slot}: a drag");
                frame(&state, &mut lightbox, "pickup");
                let (x, y) = (b.x as i32, b.y as i32);
                state.handle_on(&lightbox, InputEvent::MouseDragged { x, y });
                frame(&state, &mut lightbox, "moved");
                state.handle_on(&lightbox, InputEvent::MouseUp { x, y, alt: false });
                frame(&state, &mut lightbox, "drop");
                click(&mut state, &lightbox, &tableau, target);
                frame(&state, &mut lightbox, "rotate");
                let c = tableau.cell_centre(target);
                let (x, y) = (c.x as i32, c.y as i32);
                state.handle_on(&lightbox, InputEvent::MouseDown { x, y, alt: true });
                state.handle_on(&lightbox, InputEvent::MouseUp { x, y, alt: true });
                frame(&state, &mut lightbox, "clockwise rotate");
            }
        }
    }
    assert!(checked >= 80, "{checked} frames checked");
}

#[test]
fn tiny_canvases_paint() {
    let game = state(Game::C3, 0, false);
    for easel in [
        Easel::new(1, 1, 1.0),
        Easel::new(300, 200, 1.0),
        Easel::new(5, 900, 3.0),
    ] {
        let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
        let mut px = vec![0u8; (easel.width * easel.height * 4) as usize];
        lightbox.paint(&game, 0.0, Output::Rgba(&mut px));
    }
}

#[test]
fn memory_stays_within_budget() {
    const MB: usize = 1 << 20;
    for theme in Theme::ALL {
        let bytes = |easel: Easel| Lightbox::new(easel, theme, false).buffer_bytes();
        // The web's own RGBA canvas counts too (spec §14).
        let web =
            |w: u32, h: u32, scale: f32| bytes(Easel::new(w, h, scale)) + (w * h * 4) as usize;
        let phone = bytes(Easel::new(780, 1688, 2.0).with_touch(true)) + 780 * 1688 * 4;
        assert!(
            phone <= 20 * MB,
            "{theme:?}: phone 390 × 844 @2 (Stacked): {} MiB",
            phone / MB
        );
        assert!(
            web(2450, 1671, 1.75) <= 96 * MB,
            "{theme:?}: web desktop 1400 × 955 @1.75: {} MiB",
            web(2450, 1671, 1.75) / MB
        );
        assert!(
            web(2880, 1910, 1.0) <= 96 * MB,
            "{theme:?}: web at the 5.5 Mpx limit: {} MiB",
            web(2880, 1910, 1.0) / MB
        );
        assert!(
            bytes(Easel::new(2560, 1600, 2.0)) <= 64 * MB,
            "{theme:?}: macOS 2560 × 1600"
        );
        assert!(
            bytes(Easel::new(3456, 2234, 2.0)) <= 110 * MB,
            "{theme:?}: macOS 3456 × 2234"
        );
    }
}

#[test]
fn memory_stays_within_budget_across_resizes() {
    const MB: usize = 1 << 20;
    for theme in Theme::ALL {
        // A lightbox made at `sizes[0]` and resized through the rest, ending at the last size.
        let bytes = |sizes: &[Easel]| {
            let mut lightbox = Lightbox::new(sizes[0], theme, false);
            for &easel in &sizes[1..] {
                lightbox.resize(easel);
            }
            lightbox.buffer_bytes()
        };
        let web = Easel::new(2880, 1910, 1.0);
        let rgba = (web.width * web.height * 4) as usize;
        for (path, what) in [
            (
                [Easel::new(2860, 1900, 1.0), web],
                "a window grown by a few px",
            ),
            ([Easel::new(1400, 900, 1.0), web], "grown from small"),
        ] {
            let end = path[path.len() - 1];
            let total = bytes(&path) + (end.width * end.height * 4) as usize;
            assert!(
                total <= 96 * MB,
                "{theme:?}: web {what}, ending {}×{}: {:.1} MiB",
                end.width,
                end.height,
                total as f64 / MB as f64
            );
        }
        // A much smaller canvas gives the memory back.
        let small = Easel::new(1400, 900, 1.0);
        let (shrunk, fresh) = (bytes(&[web, small]), bytes(&[small]));
        assert!(
            shrunk <= fresh + fresh / 8,
            "{theme:?}: shrunk to 1400 × 900: {:.1} MiB, fresh {:.1} MiB",
            shrunk as f64 / MB as f64,
            fresh as f64 / MB as f64
        );
        let shrunk_then_grown = bytes(&[web, Easel::new(1400, 900, 1.0), web]) + rgba;
        assert!(
            shrunk_then_grown <= 96 * MB,
            "{theme:?}: web shrunk then grown: {:.1} MiB",
            shrunk_then_grown as f64 / MB as f64
        );
        let mac = Easel::new(3456, 2234, 2.0);
        for path in [
            vec![Easel::new(3440, 2220, 2.0), mac],
            vec![mac, Easel::new(1600, 1000, 2.0), mac],
            vec![Easel::new(2560, 1600, 2.0), mac],
        ] {
            let total = bytes(&path);
            assert!(
                total <= 110 * MB,
                "{theme:?}: macOS {path:?}: {:.1} MiB",
                total as f64 / MB as f64
            );
        }
        let grown = bytes(&[Easel::new(2540, 1590, 2.0), Easel::new(2560, 1600, 2.0)]);
        assert!(
            grown <= 64 * MB,
            "{theme:?}: macOS grown to 2560 × 1600: {:.1} MiB",
            grown as f64 / MB as f64
        );
    }
}

/// Ruling P4 (phase 3): a rect the shell drew over is recomposed at the next paint, which then
/// equals the clean frame; until then a paint leaves the shell's drawing alone.
#[test]
fn an_invalidated_rect_is_recomposed_at_the_next_paint() {
    let game = state(Game::C1, 0, false);
    let easel = Easel::new(800, 600, 1.0);
    let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
    let mut out = vec![0u32; 800 * 600];
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    let clean = out.clone();
    let scribble = DeviceRect::new(500, 40, 560, 70);
    for y in 40..70 {
        out[y * 800 + 500..y * 800 + 560].fill(0x00ff_00ff);
    }
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert!(out != clean, "an idle paint recomposes nothing");
    lightbox.invalidate(scribble);
    lightbox.invalidate(DeviceRect::new(790, 590, 900, 700)); // clipped to the canvas
    let damage = lightbox
        .paint(&game, SETTLED, Output::Xrgb(&mut out))
        .damage
        .to_vec();
    assert!(damage.contains(&scribble), "{damage:?}");
    assert!(
        damage.iter().all(|r| r.right <= 800 && r.bottom <= 600),
        "{damage:?}"
    );
    assert!(out == clean, "the scribble is gone");
    let damage = lightbox
        .paint(&game, SETTLED, Output::Xrgb(&mut out))
        .damage
        .len();
    assert_eq!(damage, 0, "an invalidation is used once");
}

/// More invalidated rects than `MAX_DAMAGE` merge, and every scribbled pixel is still
/// recomposed.
#[test]
fn more_invalidations_than_max_damage_still_recompose_every_rect() {
    let game = state(Game::C1, 0, false);
    let easel = Easel::new(800, 600, 1.0);
    let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
    let mut out = vec![0u32; 800 * 600];
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    let clean = out.clone();
    let count = MAX_DAMAGE as i32 + 4;
    for k in 0..count {
        // Disjoint, far enough apart that no two touch.
        let (left, top) = (20 + 60 * k, 30 + 40 * k);
        let scribble = DeviceRect::new(left, top, left + 12, top + 9);
        for y in top..top + 9 {
            let row = y as usize * 800;
            out[row + left as usize..row + left as usize + 12].fill(0x00ff_00ff);
        }
        lightbox.invalidate(scribble);
    }
    let damage = lightbox
        .paint(&game, SETTLED, Output::Xrgb(&mut out))
        .damage
        .len();
    assert!(damage <= MAX_DAMAGE, "{damage} rects");
    assert!(out == clean, "every scribble is gone");
}

/// `reset` then `invalidate`: one cold paint covers the rect too, and the invalidation is not
/// left over for the paint after it.
#[test]
fn a_reset_then_an_invalidation_is_one_cold_paint() {
    let game = state(Game::C1, 0, false);
    let easel = Easel::new(800, 600, 1.0);
    let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
    let mut out = vec![0u32; 800 * 600];
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    let clean = out.clone();
    for y in 40..70 {
        out[y * 800 + 500..y * 800 + 560].fill(0x00ff_00ff);
    }
    lightbox.reset();
    lightbox.invalidate(DeviceRect::new(500, 40, 560, 70));
    assert_eq!(
        lightbox
            .paint(&game, SETTLED, Output::Xrgb(&mut out))
            .damage,
        [full(easel)]
    );
    assert!(out == clean, "the scribble is gone");
    assert_eq!(
        lightbox
            .paint(&game, SETTLED, Output::Xrgb(&mut out))
            .damage
            .len(),
        0,
        "nothing is left over"
    );
}
