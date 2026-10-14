//! Spec §9 and §15 "Choreography", through the lightbox: cues start on paints, `next_wake`
//! follows them, and a cold or settled paint starts none (needs the originals).

mod common;

use std::f32::consts::FRAC_PI_4;

use common::{Clock, SETTLED, state};
use halation::beamline::half_colour;
use halation::{Easel, Lightbox, Output, SETTLE_MS, Theme};
use lumen::{
    BOARD_CELLS, BeamField, GameState, InputEvent, Piece, VISIBLE_TOOLBOX_SLOTS, board_index,
    toolbox_index,
};
use spectrograph::Game;

pub fn easel() -> Easel {
    Easel::new(1280, 800, 1.0)
}

/// A lightbox and its twin that repaints every frame whole (ruling M6).
pub fn twin(easel: Easel, reduced: bool) -> (Lightbox, Lightbox) {
    let mut whole = Lightbox::new(easel, Theme::Dark, reduced);
    whole.set_full_frames(true);
    (Lightbox::new(easel, Theme::Dark, reduced), whole)
}

/// Paints `game` at `now` on both; their pictures must match.
pub fn step_both(
    pair: &mut (Lightbox, Lightbox),
    outs: &mut (Vec<u32>, Vec<u32>),
    game: &GameState,
    now: f64,
    what: &str,
) {
    pair.0.paint(game, now, Output::Xrgb(&mut outs.0));
    pair.1.paint(game, now, Output::Xrgb(&mut outs.1));
    assert!(
        outs.0 == outs.1,
        "{what} at {now:.1} ms: an incremental frame differs from a whole one"
    );
}

/// Frames every `FRAME_MS` from `clock.now` until the incremental lightbox is idle, each checked
/// against the whole twin; then the picture must be the cold one. Returns how many frames ran.
pub fn run_out(
    pair: &mut (Lightbox, Lightbox),
    outs: &mut (Vec<u32>, Vec<u32>),
    game: &GameState,
    clock: &mut Clock,
    what: &str,
) -> usize {
    run_out_with(pair, outs, game, clock, false, what)
}

/// `run_out`, the settled frame held to a cold paint with the level sheet open when `sheet`.
pub fn run_out_with(
    pair: &mut (Lightbox, Lightbox),
    outs: &mut (Vec<u32>, Vec<u32>),
    game: &GameState,
    clock: &mut Clock,
    sheet: bool,
    what: &str,
) -> usize {
    let easel = *pair.0.easel();
    let mut frames = 0;
    while pair.0.next_wake(clock.now).is_some() {
        assert!(frames < 400, "{what}: never settles");
        let now = clock.tick();
        step_both(pair, outs, game, now, what);
        frames += 1;
    }
    assert!(
        outs.0 == common::cold_with(easel, halation::Scene::of(game), sheet),
        "{what}: the settled frame differs from a cold paint"
    );
    frames
}

#[test]
fn sweeps_equal_whole_repaints_and_settle_to_cold() {
    for (game, slot) in [(Game::C1, 0), (Game::C2, 0), (Game::C3, 0), (Game::C4, 0)] {
        let easel = easel();
        let mut st = state(game, slot, true);
        let mut pair = twin(easel, false);
        let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
        let mut clock = Clock { now: 0.0 };
        step_both(&mut pair, &mut outs, &st, clock.now, "cold");
        for k in 0..3 {
            let before = st.field().clone();
            turn_a_piece(&mut st, &pair.0);
            let now = clock.later();
            step_both(&mut pair, &mut outs, &st, now, "the turn's first frame");
            let frames = run_out(
                &mut pair,
                &mut outs,
                &st,
                &mut clock,
                &format!("{game} turn {k}"),
            );
            // A sweep runs 250–600 ms (15 frames or more); a turn that only takes light away
            // runs its 150 ms turn and 160 ms fade.
            let min = if gained_light(&before, st.field()) {
                14
            } else {
                9
            };
            assert!(
                frames >= min,
                "{game} turn {k}: {frames} frames, under {min}"
            );
        }
    }
}

#[test]
fn a_drops_sweep_on_a_phone_equals_whole_repaints_in_rgba() {
    // 780 × 1480 @2, touch, into the web's RGBA bytes: C1 level 1 solved, a piece of the solution
    // carried off to the tray and its cues run out, then dropped back where it sends light on:
    // every frame of both, against a whole repaint.
    let easel = Easel::new(780, 1480, 2.0).with_touch(true);
    let mut st = state(Game::C1, 0, true);
    let mut pair = twin(easel, false);
    let n = 780 * 1480 * 4;
    let mut outs = (vec![0u8; n], vec![0u8; n]);
    let mut clock = Clock { now: 0.0 };
    let step =
        |pair: &mut (Lightbox, Lightbox), outs: &mut (Vec<u8>, Vec<u8>), st: &GameState, now| {
            pair.0.paint(st, now, Output::Rgba(&mut outs.0));
            pair.1.paint(st, now, Output::Rgba(&mut outs.1));
            assert!(
                outs.0 == outs.1,
                "at {now:.1} ms: an incremental frame differs from a whole one"
            );
        };
    // Paints the move's first frame, then a frame every `FRAME_MS` until idle; how many.
    let mut run =
        |pair: &mut (Lightbox, Lightbox), outs: &mut (Vec<u8>, Vec<u8>), st: &GameState| {
            step(pair, outs, st, clock.later());
            let mut frames = 0;
            while pair.0.next_wake(clock.now).is_some() {
                assert!(frames < 400, "never settles");
                let now = clock.tick();
                step(pair, outs, st, now);
                frames += 1;
            }
            frames
        };
    step(&mut pair, &mut outs, &st, 0.0);
    let slot = toolbox_index(VISIBLE_TOOLBOX_SLOTS - 1);
    assert_eq!(
        st.board().cells[slot].piece,
        Piece::Empty,
        "an empty tray slot"
    );
    let cell = (0..BOARD_CELLS)
        .find(|&i| st.board().cells[i].piece.is_movable())
        .expect("a piece of the solution");
    carry(&mut st, &pair.0, cell, slot);
    let lit = st.field().clone();
    assert!(st.board().cells[slot].piece.is_movable(), "carried off");
    run(&mut pair, &mut outs, &st);
    carry(&mut st, &pair.0, slot, cell);
    assert!(gained_light(&lit, st.field()), "the drop sends light on");
    let frames = run(&mut pair, &mut outs, &st);
    assert!(frames >= 14, "a sweep's frames: {frames}");
    let cold = common::cold(easel, halation::Scene::of(&st));
    let rgb: Vec<u32> = outs
        .0
        .chunks(4)
        .map(|p| u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]))
        .collect();
    assert!(rgb == cold, "the settled frame differs from a cold paint");
}

/// Presses at cell `from`'s centre, drags to `to`'s and releases there.
fn carry(game: &mut GameState, lightbox: &Lightbox, from: usize, to: usize) {
    let ((ax, ay), (bx, by)) = (at(lightbox, from), at(lightbox, to));
    game.handle_on(
        lightbox,
        InputEvent::MouseDown {
            x: ax,
            y: ay,
            alt: false,
        },
    );
    game.handle_on(lightbox, InputEvent::MouseDragged { x: bx, y: by });
    game.handle_on(
        lightbox,
        InputEvent::MouseUp {
            x: bx,
            y: by,
            alt: false,
        },
    );
}

#[test]
fn the_sweep_grows_from_the_turned_piece() {
    // C1 level 7: the first turn (cell 47, reach 4) sends new light south down column 2 to the
    // bottom wall (reach 15). Level 1's first turn only takes light away from cells.
    let mut st = state(Game::C1, 6, true);
    let mut lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint(&st, 0.0, Output::Xrgb(&mut out));
    let old = st.field().clone();
    let cell = turn_a_piece(&mut st, &lightbox);
    lightbox.paint(&st, 1000.0, Output::Xrgb(&mut out));
    let first = out.clone();
    let end = common::cold(easel(), halation::Scene::of(&st));
    // The farthest cell that new light reaches is dark at the turn and lit once settled (a wall
    // stops light at its face, so its centre stays dark: not a wall).
    let reach = lightbox.choreography().reach();
    let fresh = |i: usize| {
        st.field().received(i) & !old.received(i) != 0 && st.board().cells[i].piece != Piece::Wall
    };
    let far = (0..BOARD_CELLS)
        .filter(|&i| fresh(i))
        .max_by_key(|&i| reach.cells[i])
        .expect("new light");
    assert!(
        reach.cells[far] > reach.cells[cell] + 2,
        "a sweep worth seeing (pick another level if not)"
    );
    let at = |px: &[u32]| {
        let c = lightbox.tableau().cell_centre(far);
        let p = px[c.y as usize * 1280 + c.x as usize];
        (p >> 16 & 0xff) + (p >> 8 & 0xff) + (p & 0xff)
    };
    assert!(
        at(&first) + 60 < at(&end),
        "the far end lights up later: {} vs {}",
        at(&first),
        at(&end)
    );
}

#[test]
fn a_change_mid_sweep_settles_to_cold() {
    let mut st = state(Game::C2, 0, true);
    let mut pair = twin(easel(), false);
    let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    turn_a_piece(&mut st, &pair.0);
    for _ in 0..6 {
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "mid-sweep");
    }
    turn_a_piece(&mut st, &pair.0);
    run_out(
        &mut pair,
        &mut outs,
        &st,
        &mut clock,
        "a second turn mid-sweep",
    );
}

#[test]
fn frames_through_two_schedules_meet() {
    // Rendering is a pure function of the state, the cues and `now` (spec §5.3): a frame at
    // t + 200 ms is the same whether 1 or 12 frames led to it.
    let mut a = state(Game::C1, 0, true);
    let mut b = state(Game::C1, 0, true);
    let (mut la, mut lb) = (
        Lightbox::new(easel(), Theme::Dark, false),
        Lightbox::new(easel(), Theme::Dark, false),
    );
    let (mut oa, mut ob) = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
    la.paint(&a, 0.0, Output::Xrgb(&mut oa));
    lb.paint(&b, 0.0, Output::Xrgb(&mut ob));
    turn_a_piece(&mut a, &la);
    turn_a_piece(&mut b, &lb);
    la.paint(&a, 1000.0, Output::Xrgb(&mut oa));
    lb.paint(&b, 1000.0, Output::Xrgb(&mut ob));
    for k in 1..12 {
        lb.paint(&b, 1000.0 + 16.0 * k as f64, Output::Xrgb(&mut ob));
    }
    la.paint(&a, 1200.0, Output::Xrgb(&mut oa));
    lb.paint(&b, 1200.0, Output::Xrgb(&mut ob));
    assert!(oa == ob);
}

#[test]
fn frames_equal_whole_repaints_at_light_scale_below_one() {
    // 2400 × 2000 @1: the board's light tile is over 1800 px, so its light renders at s < 1.
    let easel = Easel::new(2400, 2000, 1.0);
    let probe = Lightbox::new(easel, Theme::Dark, false);
    let lit = probe.tableau().board;
    assert!(
        (lit.right - lit.left) as f32 * 17.0 / 15.0 > 1800.0,
        "a scaled light tile"
    );
    let mut st = state(Game::C1, 0, true);
    let mut pair = twin(easel, false);
    let mut outs = (vec![0u32; 2400 * 2000], vec![0u32; 2400 * 2000]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    turn_a_piece(&mut st, &pair.0);
    let now = clock.later();
    step_both(&mut pair, &mut outs, &st, now, "first");
    run_out(&mut pair, &mut outs, &st, &mut clock, "scaled light");
}

#[test]
fn old_strands_fade_out() {
    let mut st = state(Game::C1, 0, true);
    let mut lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint(&st, 0.0, Output::Xrgb(&mut out));
    let before = out.clone();
    turn_a_piece(&mut st, &lightbox);
    lightbox.paint(&st, 1000.0, Output::Xrgb(&mut out));
    let end = common::cold(easel(), halation::Scene::of(&st));
    // A board pixel that was lit only by a strand the turn removed: still lit at the first
    // frame, as dark as the end picture once 160 ms have passed.
    let gone = (0..1280 * 800)
        .find(|&i| before[i] != end[i] && luma(before[i]) > luma(end[i]) + 80)
        .expect("a strand went out");
    assert!(luma(out[gone]) > luma(end[gone]) + 40, "fading, not gone");
    lightbox.paint(&st, 1200.0, Output::Xrgb(&mut out));
    assert!(luma(out[gone]) <= luma(end[gone]) + 4, "gone after 160 ms");
}

/// Whether `new` has light (a half-segment's channel) that `old` hasn't: a sweep.
fn gained_light(old: &BeamField, new: &BeamField) -> bool {
    (0..BOARD_CELLS).any(|c| (0..8).any(|s| half_colour(new, c, s) & !half_colour(old, c, s) != 0))
}

fn luma(p: u32) -> u32 {
    (p >> 16 & 0xff) + (p >> 8 & 0xff) + (p & 0xff)
}

/// Alt-clicks (turns clockwise) the first movable board piece whose turn changes the beams,
/// turning on (at most a full round) past turns that change nothing; returns its cell.
pub fn turn_a_piece(game: &mut GameState, lightbox: &Lightbox) -> usize {
    for i in 0..BOARD_CELLS {
        if !game.board().cells[i].piece.is_movable() {
            continue;
        }
        let c = lightbox.tableau().cell_centre(i);
        let (x, y) = (c.x as i32, c.y as i32);
        for _ in 0..8 {
            let before = game.field().clone();
            game.handle_on(lightbox, InputEvent::MouseDown { x, y, alt: true });
            game.handle_on(lightbox, InputEvent::MouseUp { x, y, alt: true });
            if *game.field() != before {
                return i;
            }
        }
    }
    panic!("no turn changes the beams");
}

#[test]
fn a_turn_wakes_the_lightbox_until_its_cues_end() {
    let mut game = state(Game::C1, 0, true);
    let mut lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    assert_eq!(lightbox.next_wake(0.0), None, "a cold paint starts nothing");
    turn_a_piece(&mut game, &lightbox);
    lightbox.paint(&game, 1000.0, Output::Xrgb(&mut out));
    assert!(
        lightbox.next_wake(1000.0).is_some(),
        "the turn and its sweep run"
    );
    lightbox.paint(&game, 1000.0 + SETTLE_MS, Output::Xrgb(&mut out));
    assert_eq!(lightbox.next_wake(1000.0 + SETTLE_MS), None);
}

#[test]
fn a_game_switch_forgets_every_cue() {
    let mut game = state(Game::C1, 0, true);
    let other = state(Game::C2, 0, false);
    let mut lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    turn_a_piece(&mut game, &lightbox);
    lightbox.paint(&game, 1000.0, Output::Xrgb(&mut out));
    assert!(lightbox.next_wake(1000.0).is_some(), "a cue runs");
    lightbox.paint(&other, 1016.0, Output::Xrgb(&mut out));
    assert_eq!(lightbox.next_wake(1016.0), None);
    assert_eq!(lightbox.choreography().end(), f64::NEG_INFINITY);
}

/// The pointer at `cell`'s centre.
fn at(lightbox: &Lightbox, cell: usize) -> (i32, i32) {
    let c = lightbox.tableau().cell_centre(cell);
    (c.x as i32, c.y as i32)
}

#[test]
fn a_held_piece_lifts_then_lands_or_goes_back() {
    let mut game = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    // Tray slot 0's piece picked up and held over the empty cell (8, 7): it lifts.
    let (slot, empty, other) = (toolbox_index(0), board_index(8, 7), board_index(6, 7));
    let (x, y) = at(&lightbox, slot);
    game.handle_on(&lightbox, InputEvent::MouseDown { x, y, alt: false });
    let (x, y) = at(&lightbox, empty);
    game.handle_on(&lightbox, InputEvent::MouseDragged { x, y });
    assert!(game.drag().is_some());
    lightbox.paint(&game, 1000.0, Output::Xrgb(&mut out));
    assert!(
        lightbox.choreography().moment(1030.0).lift < 1.0,
        "it rises"
    );
    // Released there, 30 ms in: it lands there, and nothing lifts any more.
    game.handle_on(&lightbox, InputEvent::MouseUp { x, y, alt: false });
    assert!(game.board().cells[empty].piece.is_movable(), "set down");
    lightbox.paint(&game, 1030.0, Output::Xrgb(&mut out));
    let m = lightbox.choreography().moment(1040.0);
    assert_eq!(m.lift, 1.0);
    assert_eq!(m.land.map(|l| (l.cell, l.from)), Some((empty, (x, y))));
    // Picked up from there, carried to (6, 7), then cancelled by a second press: it goes back.
    lightbox.paint(&game, 2000.0, Output::Xrgb(&mut out));
    game.handle_on(&lightbox, InputEvent::MouseDown { x, y, alt: false });
    let (x, y) = at(&lightbox, other);
    game.handle_on(&lightbox, InputEvent::MouseDragged { x, y });
    assert!(game.drag().is_some_and(|d| d.source == empty));
    lightbox.paint(&game, 3000.0, Output::Xrgb(&mut out));
    assert!(lightbox.choreography().moment(3000.0).lift < 1.0);
    game.handle_on(&lightbox, InputEvent::MouseDown { x, y, alt: false });
    assert!(game.drag().is_none(), "cancelled");
    lightbox.paint(&game, 3020.0, Output::Xrgb(&mut out));
    let m = lightbox.choreography().moment(3020.0);
    assert_eq!(m.lift, 1.0);
    assert_eq!(m.land.map(|l| l.cell), Some(empty), "back to its source");
    let settled = lightbox.choreography().end();
    lightbox.paint(&game, settled, Output::Xrgb(&mut out));
    assert_eq!(lightbox.next_wake(settled), None);
}

/// Presses on the first movable tray piece and drags it to `cell`'s centre (held).
fn pick_up(game: &mut GameState, lightbox: &Lightbox, cell: usize) {
    let t = lightbox.tableau();
    let from = (BOARD_CELLS..BOARD_CELLS + VISIBLE_TOOLBOX_SLOTS)
        .find(|&i| game.board().cells[i].piece.is_movable())
        .expect("a tray piece");
    let (a, b) = (t.cell_centre(from), t.cell_centre(cell));
    game.handle_on(
        lightbox,
        InputEvent::MouseDown {
            x: a.x as i32,
            y: a.y as i32,
            alt: false,
        },
    );
    game.handle_on(
        lightbox,
        InputEvent::MouseDragged {
            x: b.x as i32 + 9,
            y: b.y as i32 + 7,
        },
    );
}

#[test]
fn a_lift_rises_and_a_land_slides_into_its_cell() {
    for touch in [false, true] {
        let easel = if touch {
            Easel::new(780, 1480, 2.0).with_touch(true)
        } else {
            easel()
        };
        let n = (easel.width * easel.height) as usize;
        let mut st = state(Game::C1, 0, false);
        let mut pair = twin(easel, false);
        let mut outs = (vec![0u32; n], vec![0u32; n]);
        let mut clock = Clock { now: 0.0 };
        step_both(&mut pair, &mut outs, &st, 0.0, "cold");
        let empty = board_index(8, 7);
        pick_up(&mut st, &pair.0, empty);
        let now = clock.later();
        step_both(&mut pair, &mut outs, &st, now, "the lift starts");
        let rising = outs.0.clone();
        run_out(&mut pair, &mut outs, &st, &mut clock, "the lift");
        assert!(
            rising != outs.0,
            "touch {touch}: the first lift frame is smaller than the held piece"
        );
        let d = st.drag().unwrap();
        st.handle_on(
            &pair.0,
            InputEvent::MouseUp {
                x: d.x,
                y: d.y,
                alt: false,
            },
        );
        let now = clock.later();
        step_both(&mut pair, &mut outs, &st, now, "the land starts");
        // The cell is empty in `base` while the piece settles over it from the release point.
        let landing = pair.0.choreography().moment(now).land.expect("a land");
        assert_eq!((landing.cell, landing.from), (empty, (d.x, d.y)));
        run_out(&mut pair, &mut outs, &st, &mut clock, "the land");
    }
}

/// The largest channel difference between `a` and `b` (`0x00RRGGBB`, `width` px a row) inside
/// `r`.
fn worst_step(a: &[u32], b: &[u32], width: i32, r: halation::DeviceRect) -> u32 {
    let channel = |p: u32, s: u32| (p >> s) & 0xff;
    (r.top..r.bottom)
        .flat_map(|y| (r.left..r.right).map(move |x| (y * width + x) as usize))
        .map(|i| {
            [0, 8, 16]
                .iter()
                .map(|&s| channel(a[i], s).abs_diff(channel(b[i], s)))
                .max()
                .unwrap()
        })
        .max()
        .unwrap()
}

/// A red piece of kind `piece` facing `dir`.
fn red(piece: Piece, dir: u8) -> lumen::Cell {
    lumen::Cell {
        piece,
        colour: 1,
        dir,
    }
}

/// `game`'s empty board with a red laser at (2, 5) firing east and `pieces` placed, traced.
fn lit_board(game: Game, pieces: &[(usize, lumen::Cell)]) -> (lumen::Board, BeamField) {
    let mut board = lumen::Board::new(game);
    board.cells[board_index(2, 5)] = red(Piece::Laser, 2);
    for &(i, cell) in pieces {
        board.cells[i] = cell;
    }
    let mut field = BeamField::default();
    lumen::trace(&board, &mut field);
    (board, field)
}

/// A movable kind with a light in each game, to land: C1's prism, C2's tangler, C3's mobile
/// teleporter (its starburst and teleporter are fixed) and C4's complementor.
fn lit_kind(game: Game) -> lumen::Cell {
    let cell = match game {
        Game::C1 => red(Piece::Prism, 0),
        Game::C2 => red(Piece::Tangler, 0),
        Game::C3 => red(Piece::MobileTeleporter, 0),
        Game::C4 => red(Piece::Complementor, 0),
    };
    assert!(cell.piece.is_movable(), "{game}: a kind a hand can carry");
    cell
}

/// The two boards and fields of a carry: before (the piece at `from`) and after (at `to`).
type Carry = ((lumen::Board, BeamField), (lumen::Board, BeamField));

/// Carries `piece` from `from` to `to` on `easel` (released `off` px right and down of `to`'s
/// centre, after its lift ran out), every frame against a whole repaint, and runs the land out to
/// the cold picture. `land_only`: the held frame already shows the drop's beams, so the land is
/// all that moves. Calls `seen` with every frame from the drop on: the incremental lightbox,
/// the frame's time and its picture.
fn carry_in(
    easel: Easel,
    game: Game,
    (piece, from, to): (lumen::Cell, usize, usize),
    off: (i32, i32),
    land_only: bool,
    carry: &Carry,
    mut seen: impl FnMut(&Lightbox, f64, &[u32]),
) {
    let ((before, field_before), (after, field_after)) = carry;
    let held_field = if land_only { field_after } else { field_before };
    let n = (easel.width * easel.height) as usize;
    let mut pair = twin(easel, false);
    let mut outs = (vec![0u32; n], vec![0u32; n]);
    let mut clock = Clock { now: 0.0 };
    let paint = |pair: &mut (Lightbox, Lightbox),
                 outs: &mut (Vec<u32>, Vec<u32>),
                 scene: halation::Scene,
                 now: f64| {
        pair.0.paint_scene(scene, now, Output::Xrgb(&mut outs.0));
        pair.1.paint_scene(scene, now, Output::Xrgb(&mut outs.1));
        assert!(
            outs.0 == outs.1,
            "{game} at {now:.1} ms: an incremental frame differs from a whole one"
        );
    };
    let c = pair.0.tableau().cell_centre(to);
    let drag = lumen::Drag {
        source: from,
        piece,
        x: c.x as i32 + off.0,
        y: c.y as i32 + off.1,
    };
    let rest = halation::Scene::bare(before, field_before, game);
    let held = halation::Scene {
        drag: Some(drag),
        ..halation::Scene::bare(before, held_field, game)
    };
    let dropped = halation::Scene::bare(after, field_after, game);
    // Paints `scene` from 10 s on, a frame at a time until idle, each shown to `f`.
    let mut run = |pair: &mut (Lightbox, Lightbox),
                   outs: &mut (Vec<u32>, Vec<u32>),
                   scene: halation::Scene,
                   f: &mut dyn FnMut(&Lightbox, f64, &[u32])| {
        let mut now = clock.later();
        paint(pair, outs, scene, now);
        loop {
            f(&pair.0, now, &outs.0);
            if pair.0.next_wake(now).is_none() {
                break;
            }
            now = clock.tick();
            paint(pair, outs, scene, now);
        }
    };
    paint(&mut pair, &mut outs, rest, 0.0);
    run(&mut pair, &mut outs, held, &mut |_, _, _| {});
    let mut landed = 0;
    run(&mut pair, &mut outs, dropped, &mut |lightbox, now, out| {
        landed += usize::from(lands(lightbox, now));
        seen(lightbox, now, out);
    });
    assert!(landed >= 9, "{game}: the land's frames ({landed})");
    assert!(
        outs.0 == common::cold(easel, dropped),
        "{game}: the land settles to the cold picture"
    );
}

/// Whether a piece is landing at `now`.
fn lands(lightbox: &Lightbox, now: f64) -> bool {
    lightbox.choreography().moment(now).land.is_some()
}

/// The desktop, and the phone.
fn easels() -> [Easel; 2] {
    [easel(), Easel::new(780, 1480, 2.0).with_touch(true)]
}

#[test]
fn a_land_settles_without_a_step() {
    // Each game's lit kind carried from the tray onto (9, 5), in the laser's beam, with the drop's
    // beams already shown so only the land moves: its last frame (150 ms in, 99.98 % of the way)
    // and the settled one differ by no more than the vector body against its sprite — never by
    // its light arriving (with its glow and bloom) or its beams going over it at once.
    for game in Game::ALL {
        for easel in easels() {
            let (piece, from, to) = (lit_kind(game), toolbox_index(0), board_index(9, 5));
            let carry = (
                lit_board(game, &[(from, piece)]),
                lit_board(game, &[(to, piece)]),
            );
            assert!(carry.1.1.received(to) != 0, "{game}: lit where it lands");
            let t = Lightbox::new(easel, Theme::Dark, false).tableau().clone();
            let (c, u) = (t.cell_centre(to), t.cell as f32);
            let r = halation::DeviceRect::new(
                (c.x - 1.5 * u) as i32,
                (c.y - 1.5 * u) as i32,
                (c.x + 1.5 * u) as i32,
                (c.y + 1.5 * u) as i32,
            );
            // The last land frame, then the step to the first one after it.
            let (mut last, mut step) = (None::<Vec<u32>>, None);
            carry_in(
                easel,
                game,
                (piece, from, to),
                (5, 4),
                true,
                &carry,
                |lb, now, out| {
                    if lands(lb, now) {
                        last = Some(out.to_vec());
                    } else if let Some(before) = last.take() {
                        step = Some(worst_step(&before, out, easel.width as i32, r));
                    }
                },
            );
            let step = step.expect("a frame after the land");
            assert!(
                step <= 8,
                "{game} touch {}: the settle steps by {step}/255",
                easel.touch
            );
        }
    }
}

#[test]
fn lit_pieces_land_as_whole_repaints_in_every_game() {
    // M2: each game's lit kind carried from the tray into the beam with its sweep (and from the
    // board back onto it), released off its cell's centre: every frame of the land against a
    // whole repaint, on the desktop and the phone, settling to cold.
    for game in Game::ALL {
        for easel in easels() {
            let (piece, slot, to) = (lit_kind(game), toolbox_index(0), board_index(9, 5));
            let tray = lit_board(game, &[(slot, piece)]);
            let board = lit_board(game, &[(to, piece)]);
            let off = (if easel.touch { 13 } else { 9 }, 7);
            let carry = (tray, board.clone());
            carry_in(
                easel,
                game,
                (piece, slot, to),
                off,
                false,
                &carry,
                |_, _, _| {},
            );
            let other = board_index(11, 9);
            let carry = (board, lit_board(game, &[(other, piece)]));
            carry_in(
                easel,
                game,
                (piece, to, other),
                off,
                false,
                &carry,
                |_, _, _| {},
            );
        }
    }
}

#[test]
fn a_landing_gate_lights_when_the_sweep_reaches_it() {
    // M1: C3's red logic gate (it absorbs) moved along the laser's row from (5, 5), where it
    // stopped the beam, to (12, 5), released off its cell's centre: its new cell receives light
    // only once the sweep's front gets there, after the land. Every frame, the light tile shows
    // its light where its body is, from the old field (dark) until the front arrives and from
    // the new one after; landed, at its cell; once lit, it never goes dark again.
    let game = Game::C3;
    let gate = red(Piece::LogicGate, 0);
    let (a, b) = (board_index(5, 5), board_index(12, 5));
    let carry = (lit_board(game, &[(a, gate)]), lit_board(game, &[(b, gate)]));
    let (old, new) = (&carry.0.1, &carry.1.1);
    assert!(old.received(b) == 0 && new.received(b) != 0);
    // Each frame from the drop on: landing, the front there, and the gate's light item.
    let mut frames = Vec::new();
    carry_in(
        easel(),
        game,
        (gate, a, b),
        (9, 7),
        false,
        &carry,
        |lb, now, _| {
            let arrived = lb
                .choreography()
                .moment(now)
                .sweep
                .is_none_or(|v| v.arrived(new, b));
            let item = lb.darkroom().shown().lights[b];
            frames.push((lands(lb, now), arrived, item));
        },
    );
    for &(land, arrived, item) in &frames {
        let field = if arrived { new } else { old };
        assert!(
            item.shown && item.received == field.received(b),
            "{land} {arrived}: shown with the field the front has reached: {item:?}"
        );
        let settled = item.offset == halation::etch::Pt::default() && item.size == 1.0;
        assert_eq!(settled, !land, "where its body is: {item:?}");
    }
    assert!(
        frames.iter().any(|f| f.0 && !f.1) && frames.iter().any(|f| !f.0 && !f.1),
        "the front reaches it after the land"
    );
    let first = frames.iter().position(|f| f.2.received != 0).expect("lit");
    assert!(
        frames[first..].iter().all(|f| f.2.received != 0),
        "once lit, never dark again"
    );
}

#[test]
fn a_piece_back_in_the_tray_settles_without_a_step() {
    // A tray piece picked up, held just off its slot and the drag cancelled: it lands back in its
    // slot from a few px away (so its last frame is all but there). That frame and the settled
    // one differ by no more than the vector piece against its sprite, which the tray blits at
    // whole px: the land ends where the sprite stands. At 1366 × 768 the slot (48 px) and the
    // piece (43 px) differ by an odd count, so the sprite is half a px off the slot's centre.
    for easel in [easels().as_slice(), &[Easel::new(1366, 768, 1.0)]].concat() {
        let n = (easel.width * easel.height) as usize;
        let mut st = state(Game::C1, 0, false);
        let mut pair = twin(easel, false);
        let mut outs = (vec![0u32; n], vec![0u32; n]);
        let mut clock = Clock { now: 0.0 };
        step_both(&mut pair, &mut outs, &st, 0.0, "cold");
        let (x, y) = at(&pair.0, toolbox_index(0));
        st.handle_on(&pair.0, InputEvent::MouseDown { x, y, alt: false });
        st.handle_on(&pair.0, InputEvent::MouseDragged { x: x + 6, y: y + 5 });
        step_both(&mut pair, &mut outs, &st, clock.later(), "held");
        let source = st.drag().expect("held").source;
        assert_eq!(source, toolbox_index(0));
        st.handle_on(
            &pair.0,
            InputEvent::MouseDown {
                x: 1,
                y: 1,
                alt: false,
            },
        );
        step_both(&mut pair, &mut outs, &st, clock.later(), "cancelled");
        let r = pair.0.tableau().cell_rect(source);
        let (mut last, mut step) = (None::<Vec<u32>>, None);
        while pair.0.next_wake(clock.now).is_some() {
            let now = clock.tick();
            step_both(&mut pair, &mut outs, &st, now, "back to the tray");
            if lands(&pair.0, now) {
                last = Some(outs.0.clone());
            } else if let Some(before) = last.take() {
                step = Some(worst_step(&before, &outs.0, easel.width as i32, r));
            }
        }
        let step = step.expect("a frame after the land");
        assert!(
            step <= 8,
            "touch {}: the settle steps by {step}/255",
            easel.touch
        );
        assert!(outs.0 == common::cold(easel, halation::Scene::of(&st)));
    }
}

#[test]
fn a_release_mid_lift_lands_from_where_it_had_risen() {
    // M3: picked up and dropped on (8, 7) two frames into its 90 ms rise, on the phone (lift
    // 1.3): the land starts from the lift reached, not the full one; every frame against a whole
    // repaint, settling to cold.
    let easel = Easel::new(780, 1480, 2.0).with_touch(true);
    let n = (easel.width * easel.height) as usize;
    let mut st = state(Game::C1, 0, false);
    let mut pair = twin(easel, false);
    let mut outs = (vec![0u32; n], vec![0u32; n]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    pick_up(&mut st, &pair.0, board_index(8, 7));
    step_both(&mut pair, &mut outs, &st, clock.later(), "the lift starts");
    for _ in 0..2 {
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "rising");
    }
    let d = st.drag().unwrap();
    st.handle_on(
        &pair.0,
        InputEvent::MouseUp {
            x: d.x,
            y: d.y,
            alt: false,
        },
    );
    let now = clock.tick();
    let risen = pair.0.choreography().moment(now).lift;
    step_both(&mut pair, &mut outs, &st, now, "dropped mid-lift");
    let land = pair.0.choreography().moment(now).land.expect("a land");
    assert!(
        risen < 1.0 && land.lift == risen,
        "it lands from its lift {risen}, not {}",
        land.lift
    );
    run_out(
        &mut pair,
        &mut outs,
        &st,
        &mut clock,
        "the land from mid-lift",
    );
}

#[test]
fn a_cancelled_drag_lands_back_on_its_source() {
    let mut st = state(Game::C1, 0, false);
    let mut pair = twin(easel(), false);
    let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    pick_up(&mut st, &pair.0, board_index(8, 7));
    let now = clock.later();
    step_both(&mut pair, &mut outs, &st, now, "held");
    let source = st.drag().unwrap().source;
    st.handle_on(
        &pair.0,
        InputEvent::MouseDown {
            x: 1,
            y: 1,
            alt: false,
        },
    );
    let now = clock.later();
    step_both(&mut pair, &mut outs, &st, now, "cancelled");
    assert_eq!(
        pair.0.choreography().moment(now).land.map(|l| l.cell),
        Some(source)
    );
    run_out(&mut pair, &mut outs, &st, &mut clock, "back to the tray");
}

#[test]
fn a_land_ends_when_its_piece_is_picked_up_again_or_the_level_restarts() {
    // Dropped on (8, 7), then 3 frames into the land picked up again (held, not also landing),
    // or the level restarted (the cell empty, nothing settling into it): no ghost either way.
    for restart in [false, true] {
        let mut st = state(Game::C1, 0, false);
        let mut pair = twin(easel(), false);
        let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
        let mut clock = Clock { now: 0.0 };
        step_both(&mut pair, &mut outs, &st, 0.0, "cold");
        let cell = board_index(8, 7);
        pick_up(&mut st, &pair.0, cell);
        step_both(&mut pair, &mut outs, &st, clock.later(), "held");
        let d = st.drag().unwrap();
        st.handle_on(
            &pair.0,
            InputEvent::MouseUp {
                x: d.x,
                y: d.y,
                alt: false,
            },
        );
        step_both(&mut pair, &mut outs, &st, clock.later(), "dropped");
        for _ in 0..3 {
            let now = clock.tick();
            step_both(&mut pair, &mut outs, &st, now, "landing");
        }
        if restart {
            st.handle_on(&pair.0, InputEvent::Key(b'r'));
            assert!(st.board().cells[cell].piece == Piece::Empty, "restarted");
        } else {
            let (x, y) = at(&pair.0, cell);
            st.handle_on(&pair.0, InputEvent::MouseDown { x, y, alt: false });
            st.handle_on(
                &pair.0,
                InputEvent::MouseDragged {
                    x: x + 40,
                    y: y - 30,
                },
            );
            assert!(st.drag().is_some_and(|d| d.source == cell), "picked up");
        }
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "mid-land");
        assert_eq!(
            pair.0.choreography().moment(now).land,
            None,
            "restart {restart}: the land ends"
        );
        run_out(&mut pair, &mut outs, &st, &mut clock, "after the land");
    }
}

#[test]
fn settled_cold_and_reduced_paints_start_nothing() {
    for (reduced, now) in [(false, SETTLED), (true, 1000.0)] {
        let mut game = state(Game::C1, 0, true);
        let mut lightbox = Lightbox::new(easel(), Theme::Dark, reduced);
        let mut out = vec![0u32; 1280 * 800];
        lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
        turn_a_piece(&mut game, &lightbox);
        lightbox.paint(&game, now, Output::Xrgb(&mut out));
        assert_eq!(
            lightbox.next_wake(1000.0),
            None,
            "reduced {reduced}, now {now}"
        );
    }
}

#[test]
fn reset_resize_and_reduced_motion_forget_every_cue() {
    let mut game = state(Game::C1, 0, true);
    let mut lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    for forget in 0..3 {
        turn_a_piece(&mut game, &lightbox);
        lightbox.paint(&game, 1000.0, Output::Xrgb(&mut out));
        assert!(
            lightbox.next_wake(1000.0).is_some(),
            "case {forget}: a cue runs"
        );
        match forget {
            0 => lightbox.reset(),
            1 => lightbox.resize(easel()),
            _ => lightbox.set_reduced_motion(true),
        }
        lightbox.paint(&game, 1016.0, Output::Xrgb(&mut out));
        assert_eq!(lightbox.next_wake(1016.0), None, "case {forget}");
        lightbox.set_reduced_motion(false);
    }
}

/// A board where a red laser on row 5 is blocked by a wall at (6, 5) before a red target at
/// (12, 5), and the same board without the wall.
fn blocked(wall: bool) -> (lumen::Board, lumen::BeamField) {
    let mut b = lumen::Board::default();
    let mut put = |x: usize, y: usize, piece, colour, dir| {
        b.cells[board_index(x, y)] = lumen::Cell { piece, colour, dir }
    };
    put(2, 5, Piece::Laser, 1, 2);
    put(12, 5, Piece::Pinwheel, 1, 0);
    if wall {
        put(6, 5, Piece::Wall, 0, 0);
    }
    let mut f = BeamField::default();
    lumen::trace(&b, &mut f);
    (b, f)
}

/// The C1 scene of a board and its field.
fn scene(p: &(lumen::Board, BeamField)) -> halation::Scene<'_> {
    halation::Scene::bare(&p.0, &p.1, Game::C1)
}

#[test]
fn a_target_lit_by_a_change_bursts_then_settles() {
    let (a, b) = (blocked(true), blocked(false));
    let easel = easel();
    let mut pair = twin(easel, false);
    let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
    for (lb, out) in [(&mut pair.0, &mut outs.0), (&mut pair.1, &mut outs.1)] {
        lb.paint_scene(scene(&a), 0.0, Output::Xrgb(out));
        lb.paint_scene(scene(&b), 1000.0, Output::Xrgb(out));
    }
    let burst = pair.0.choreography().bursts()[0];
    assert_eq!(burst.cell, board_index(12, 5));
    let settled = common::cold(easel, scene(&b));
    // 30 ms into the burst its ring is at 1.2u · (0.3 + 1.1 · outExpo(0.05)) ≈ 0.747u, at
    // α 0.45 · (1 − 0.29)² ≈ 0.23: it lights pixels around the target the settled picture leaves dark.
    let t = burst.at + 30.0;
    for (lb, out) in [(&mut pair.0, &mut outs.0), (&mut pair.1, &mut outs.1)] {
        lb.paint_scene(scene(&b), t, Output::Xrgb(out));
    }
    assert!(outs.0 == outs.1, "a burst frame equals a whole one");
    let c = pair.0.tableau().cell_centre(burst.cell);
    let u = pair.0.tableau().cell as f32;
    let ring = (0..360).step_by(10).filter(|deg| {
        let a = (*deg as f32).to_radians();
        let (x, y) = (
            (c.x + a.cos() * 0.747 * u) as usize,
            (c.y + a.sin() * 0.747 * u) as usize,
        );
        luma(outs.0[y * 1280 + x]) > luma(settled[y * 1280 + x]) + 6
    });
    assert!(ring.count() >= 12, "the ring shows");
    let mut clock = Clock { now: t };
    while pair.0.next_wake(clock.now).is_some() {
        let now = clock.tick();
        for (lb, out) in [(&mut pair.0, &mut outs.0), (&mut pair.1, &mut outs.1)] {
            lb.paint_scene(scene(&b), now, Output::Xrgb(out));
        }
        assert!(outs.0 == outs.1, "at {now:.1} ms");
    }
    assert!(outs.0 == settled, "settles to the cold picture");
}

#[test]
fn a_turn_draws_its_piece_in_between_then_settles() {
    let mut st = state(Game::C1, 0, true);
    let mut pair = twin(easel(), false);
    let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    let before = outs.0.clone();
    let cell = turn_a_piece(&mut st, &pair.0);
    let now = clock.later();
    step_both(&mut pair, &mut outs, &st, now, "the turn starts");
    for _ in 0..4 {
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "mid-turn");
    }
    let mid = outs.0.clone();
    run_out(&mut pair, &mut outs, &st, &mut clock, "the turn");
    let r = pair.0.tableau().cell_rect(cell);
    let differs = |a: &[u32], b: &[u32]| {
        (r.top..r.bottom).any(|y| {
            (r.left..r.right).any(|x| a[(y * 1280 + x) as usize] != b[(y * 1280 + x) as usize])
        })
    };
    assert!(
        differs(&mid, &before) && differs(&mid, &outs.0),
        "mid-turn, the cell is neither the old nor the settled picture (its beams sweep too, so \
         this alone does not show the body turning: an_unlit_piece_turns_… does)"
    );
}

#[test]
fn an_unlit_piece_turns_through_its_angle_both_ways() {
    // No light anywhere, so only the body can move: a mirror and a complementor at (7, 7) turned
    // one step clockwise and one anticlockwise (the complementor's halves differ, so its drawing
    // turns the 45° delta, not a full round: ruling F16).
    for piece in [Piece::Mirror, Piece::Complementor] {
        for to in [3u8, 1] {
            let board = |dir| {
                let mut b = lumen::Board::default();
                b.cells[board_index(7, 7)] = lumen::Cell {
                    piece,
                    colour: 0,
                    dir,
                };
                (b, BeamField::default())
            };
            let (a, b) = (board(2), board(to));
            let easel = easel();
            let mut pair = twin(easel, false);
            let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
            let paint =
                |pair: &mut (Lightbox, Lightbox), outs: &mut (Vec<u32>, Vec<u32>), p, now| {
                    pair.0.paint_scene(scene(p), now, Output::Xrgb(&mut outs.0));
                    pair.1.paint_scene(scene(p), now, Output::Xrgb(&mut outs.1));
                    assert!(outs.0 == outs.1, "{piece:?} to {to} at {now:.1} ms");
                };
            paint(&mut pair, &mut outs, &a, 0.0);
            let before = outs.0.clone();
            paint(&mut pair, &mut outs, &b, 1000.0);
            let mut clock = Clock { now: 1000.0 };
            let mut mid = None;
            while pair.0.next_wake(clock.now).is_some() {
                let now = clock.tick();
                paint(&mut pair, &mut outs, &b, now);
                if mid.is_none() && now >= 1060.0 {
                    mid = Some((outs.0.clone(), now, pair.0.choreography().moment(now).turn));
                }
            }
            let (mid, at, turn) = mid.expect("frames mid-turn");
            // The shortest way: one step clockwise (to 3) starts a step back, anticlockwise (to
            // 1) a step on, and eases to 0 over 150 ms — never the long way round.
            let step = if to == 3 { 1.0 } else { -1.0 };
            let k = ((at - 1000.0) / halation::choreography::TURN_MS) as f32;
            let spin = -step * FRAC_PI_4 * (1.0 - halation::choreography::ease::out_cubic(k));
            let (cell, drawn) = turn.expect("the piece turns");
            assert!(
                cell == board_index(7, 7) && (drawn - spin).abs() < 1e-6,
                "{piece:?} to {to}: spin {drawn} at {at:.1} ms, not {spin}"
            );
            let after = common::cold(easel, scene(&b));
            assert!(
                outs.0 == after,
                "{piece:?} to {to}: settles to the cold picture"
            );
            let r = pair.0.tableau().cell_rect(board_index(7, 7));
            // And it is drawn at that signed spin: the cell as the darkroom paints the piece
            // posed at `spin`, not at `-spin` (the wrong way round).
            let posed = |spin: f32| {
                let tableau = pair.0.tableau();
                let palette = Theme::Dark.palette();
                let mut room = halation::darkroom::Darkroom::new(tableau, palette);
                room.set_pose(halation::darkroom::Pose {
                    turn: Some((board_index(7, 7), spin)),
                    ..halation::darkroom::Pose::SETTLED
                });
                room.paint_all(tableau, palette, &scene(&b));
                let mut px = vec![0u32; 1280 * 800];
                room.compose(r, &mut Output::Xrgb(&mut px));
                px
            };
            let in_cell = |x: &[u32], y: &[u32]| {
                (r.top..r.bottom).all(|py| {
                    (r.left..r.right).all(|px| {
                        let i = (py * 1280 + px) as usize;
                        x[i] == y[i]
                    })
                })
            };
            assert!(
                in_cell(&mid, &posed(spin)),
                "{piece:?} to {to}: drawn at its spin"
            );
            assert!(
                !in_cell(&mid, &posed(-spin)),
                "{piece:?} to {to}: not the other way round"
            );
            let count = |x: &[u32], y: &[u32]| {
                (r.top..r.bottom)
                    .flat_map(|py| (r.left..r.right).map(move |px| (py * 1280 + px) as usize))
                    .filter(|&i| x[i] != y[i])
                    .count()
            };
            assert!(
                count(&mid, &before) > 20 && count(&mid, &after) > 20,
                "{piece:?} to {to}: mid-turn is neither the old nor the new angle"
            );
            let outside = (0..1280 * 800).any(|i| {
                let (x, y) = ((i % 1280) as i32, (i / 1280) as i32);
                !((r.left..r.right).contains(&x) && (r.top..r.bottom).contains(&y))
                    && mid[i] != after[i]
            });
            assert!(
                !outside,
                "{piece:?} to {to}: the turn stays inside its cell"
            );
        }
    }
}

/// An unlit board with `pieces` (x, y, piece, dir), all colour 0.
fn unlit(pieces: &[(usize, usize, Piece, u8)]) -> (lumen::Board, BeamField) {
    let mut b = lumen::Board::default();
    for &(x, y, piece, dir) in pieces {
        b.cells[board_index(x, y)] = lumen::Cell {
            piece,
            colour: 0,
            dir,
        };
    }
    (b, BeamField::default())
}

/// Paints `p` at `now` on both lightboxes; their pictures must match.
fn paint_both(
    pair: &mut (Lightbox, Lightbox),
    outs: &mut (Vec<u32>, Vec<u32>),
    p: &(lumen::Board, BeamField),
    now: f64,
) {
    pair.0.paint_scene(scene(p), now, Output::Xrgb(&mut outs.0));
    pair.1.paint_scene(scene(p), now, Output::Xrgb(&mut outs.1));
    assert!(
        outs.0 == outs.1,
        "at {now:.1} ms: an incremental frame differs from a whole one"
    );
}

#[test]
fn a_turning_cell_given_another_piece_draws_it_at_rest() {
    // A mirror at (7, 7) turns; 30 ms in, a paste-like change of two cells puts another piece
    // there (a target, which never turns, or a filter, which does): no turn runs on, so that
    // frame is the cold picture of the new board.
    for other in [Piece::Pinwheel, Piece::Filter] {
        let (a, b) = (
            unlit(&[(7, 7, Piece::Mirror, 2)]),
            unlit(&[(7, 7, Piece::Mirror, 3)]),
        );
        let c = unlit(&[(7, 7, other, 3), (2, 2, Piece::Mirror, 0)]);
        let mut pair = twin(easel(), false);
        let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
        paint_both(&mut pair, &mut outs, &a, 0.0);
        paint_both(&mut pair, &mut outs, &b, 1000.0);
        assert!(
            pair.0.choreography().moment(1016.0).turn.is_some(),
            "a turn runs"
        );
        paint_both(&mut pair, &mut outs, &c, 1030.0);
        assert!(
            outs.0 == common::cold(easel(), scene(&c)),
            "{other:?} put on a turning cell draws at rest"
        );
        assert_eq!(pair.0.choreography().moment(1030.0).turn, None, "{other:?}");
    }
}

#[test]
fn a_second_turn_on_another_cell_settles_the_first() {
    // Mirrors at (7, 7) and (9, 9): the first turns, three frames on the second turns, and the
    // first's cell is redrawn at rest; every frame equals a whole repaint, settling to cold.
    let a = unlit(&[(7, 7, Piece::Mirror, 2), (9, 9, Piece::Mirror, 2)]);
    let b = unlit(&[(7, 7, Piece::Mirror, 3), (9, 9, Piece::Mirror, 2)]);
    let c = unlit(&[(7, 7, Piece::Mirror, 3), (9, 9, Piece::Mirror, 3)]);
    let mut pair = twin(easel(), false);
    let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
    let mut clock = Clock { now: 0.0 };
    paint_both(&mut pair, &mut outs, &a, clock.now);
    paint_both(&mut pair, &mut outs, &b, clock.later());
    for _ in 0..3 {
        let now = clock.tick();
        paint_both(&mut pair, &mut outs, &b, now);
    }
    let now = clock.tick();
    paint_both(&mut pair, &mut outs, &c, now);
    let turn = pair.0.choreography().moment(now).turn;
    assert_eq!(
        turn.map(|t| t.0),
        Some(board_index(9, 9)),
        "the second turns"
    );
    let first = pair.0.tableau().cell_rect(board_index(7, 7));
    let settled = common::cold(easel(), scene(&c));
    let at_rest = (first.top..first.bottom).all(|y| {
        (first.left..first.right).all(|x| {
            let i = (y * 1280 + x) as usize;
            outs.0[i] == settled[i]
        })
    });
    assert!(at_rest, "the first piece snaps to its settled angle");
    let mut frames = 0;
    while pair.0.next_wake(clock.now).is_some() {
        assert!(frames < 400, "never settles");
        let now = clock.tick();
        paint_both(&mut pair, &mut outs, &c, now);
        frames += 1;
    }
    assert!(outs.0 == settled, "settles to the cold picture");
}

#[test]
fn a_win_runs_packets_and_reveals_its_header_then_settles() {
    let mut st = state(Game::C1, 0, false);
    let mut pair = twin(easel(), false);
    let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    st.handle(InputEvent::Key(st.rules().cheat_key));
    assert!(st.is_won());
    let start = clock.later();
    step_both(&mut pair, &mut outs, &st, start, "the win starts");
    let settled = common::cold(easel(), halation::Scene::of(&st));
    // At 120 ms the packets are 7 cells out along the beams and the header is part-revealed
    // (outCubic(120 / 700) ≈ 0.43).
    while clock.now < start + 120.0 {
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "the win");
    }
    let board = pair.0.tableau().board;
    let header = pair.0.tableau().device(pair.0.tableau().card);
    let brighter = |r: halation::DeviceRect| {
        (r.top..r.bottom).any(|y| {
            (r.left..r.right).any(|x| {
                let i = (y * 1280 + x) as usize;
                luma(outs.0[i]) > luma(settled[i]) + 30
            })
        })
    };
    assert!(brighter(board), "a packet on a beam");
    let differs = (header.top..header.top + 60).any(|y| {
        (header.left..header.right)
            .any(|x| outs.0[(y * 1280 + x) as usize] != settled[(y * 1280 + x) as usize])
    });
    assert!(differs, "the header is mid-reveal");
    run_out(&mut pair, &mut outs, &st, &mut clock, "the win");
}

#[test]
fn the_card_is_not_repainted_once_its_header_is_whole() {
    // "YOU WIN!" (8 glyphs) is whole once outCubic(t / 700) ≥ 9.5 / 10.5, about 381 ms in:
    // from 400 ms the card is no longer damaged (the level's one packet left the board at
    // 210 ms), though the reveal runs on to 700 ms.
    let mut st = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint(&st, 0.0, Output::Xrgb(&mut out));
    st.handle(InputEvent::Key(st.rules().cheat_key));
    let mut clock = Clock { now: 0.0 };
    let start = clock.later();
    lightbox.paint(&st, start, Output::Xrgb(&mut out));
    let card = lightbox.darkroom().card_damage();
    let mut late = 0;
    while lightbox.next_wake(clock.now).is_some() {
        let now = clock.tick();
        let painted = lightbox.paint(&st, now, Output::Xrgb(&mut out));
        let on_card = painted.damage.iter().any(|r| {
            r.left < card.right && card.left < r.right && r.top < card.bottom && card.top < r.bottom
        });
        if now - start >= 400.0 {
            assert!(!on_card, "at {:.0} ms the card is damaged", now - start);
            late += 1;
        } else if now - start < 300.0 {
            assert!(on_card, "at {:.0} ms the header is mid-reveal", now - start);
        }
    }
    assert!(late > 50, "the win runs on to 1.4 s: {late} frames");
}

/// What happens 200 ms into a win.
#[derive(Clone, Copy, Debug)]
enum MidWin {
    Restart,
    OpenSheet,
    LandElsewhere,
}

#[test]
fn a_win_interrupted_mid_reveal_equals_whole_repaints_and_settles() {
    // C1 level 1 won with the cheat key on a canvas with a sheet picker; 200 ms in (packets
    // running, the header half written) it is restarted (unwon), or the level sheet opens over
    // it, or a tray piece is carried onto the board and lands there. Every frame against a
    // whole repaint, settling to the cold picture (with the sheet, if open).
    let easel = Easel::new(1600, 1200, 2.0);
    let n = (easel.width * easel.height) as usize;
    for what in [MidWin::Restart, MidWin::OpenSheet, MidWin::LandElsewhere] {
        let mut st = state(Game::C1, 0, false);
        let mut pair = twin(easel, false);
        assert!(!pair.0.has_inline_picker(), "a sheet picker");
        let mut outs = (vec![0u32; n], vec![0u32; n]);
        let mut clock = Clock { now: 0.0 };
        step_both(&mut pair, &mut outs, &st, 0.0, "cold");
        st.handle(InputEvent::Key(st.rules().cheat_key));
        let start = clock.later();
        step_both(&mut pair, &mut outs, &st, start, "the win");
        while clock.now < start + 200.0 {
            let now = clock.tick();
            step_both(&mut pair, &mut outs, &st, now, "the win");
        }
        let w = pair
            .0
            .choreography()
            .moment(clock.now)
            .win
            .expect("the win runs");
        assert!(w.reveal > 0.0 && w.reveal < 1.0, "mid-reveal: {w:?}");
        match what {
            MidWin::Restart => {
                st.handle(InputEvent::Key(b'r'));
                assert!(!st.is_won(), "restarted, unwon");
            }
            MidWin::OpenSheet => {
                assert!(pair.0.command(halation::Control::Levels));
                assert!(pair.1.command(halation::Control::Levels));
            }
            MidWin::LandElsewhere => {
                pick_up(&mut st, &pair.0, board_index(8, 7));
                let now = clock.tick();
                step_both(&mut pair, &mut outs, &st, now, "held mid-win");
                let d = st.drag().expect("held");
                let up = InputEvent::MouseUp {
                    x: d.x,
                    y: d.y,
                    alt: false,
                };
                st.handle_on(&pair.0, up);
            }
        }
        let now = clock.tick();
        let what_now = format!("{what:?} mid-win");
        step_both(&mut pair, &mut outs, &st, now, &what_now);
        let m = pair.0.choreography().moment(now);
        match what {
            MidWin::Restart => assert!(m.win.is_none(), "the unwin ends the win"),
            MidWin::OpenSheet => assert!(m.win.is_some() && m.sheet.is_some(), "{m:?}"),
            MidWin::LandElsewhere => assert!(m.land.is_some(), "it lands: {m:?}"),
        }
        let mut frames = 0;
        while pair.0.next_wake(clock.now).is_some() {
            assert!(frames < 400, "{what:?}: never settles");
            let now = clock.tick();
            step_both(&mut pair, &mut outs, &st, now, &what_now);
            frames += 1;
        }
        let sheet = pair.0.sheet_open();
        let cold = common::cold_with(easel, halation::Scene::of(&st), sheet);
        assert!(outs.0 == cold, "{what:?}: settles to the cold picture");
    }
}

#[test]
fn reduced_motion_fades_the_win_header_and_runs_no_packets() {
    let mut st = state(Game::C1, 0, false);
    let mut pair = twin(easel(), true);
    let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    st.handle(InputEvent::Key(st.rules().cheat_key));
    let start = clock.later();
    step_both(&mut pair, &mut outs, &st, start, "the win");
    let settled = common::cold(easel(), halation::Scene::of(&st));
    let now = clock.tick();
    step_both(&mut pair, &mut outs, &st, now, "the fade");
    // The card's region (its header tile's glow reaches a px past the panel's rect).
    let card = pair.0.darkroom().card_damage();
    let outside = (0..800)
        .flat_map(|y| (0..1280).map(move |x| (x, y)))
        .find(|&(x, y)| {
            let inside =
                (card.left..card.right).contains(&x) && (card.top..card.bottom).contains(&y);
            !inside && outs.0[(y * 1280 + x) as usize] != settled[(y * 1280 + x) as usize]
        });
    assert_eq!(
        outside, None,
        "only the card's header changes: no packets, no pulses (card {card:?})"
    );
    let header = card.top..card.top + 60;
    let fading = header.clone().any(|y| {
        (card.left..card.right)
            .any(|x| outs.0[(y * 1280 + x) as usize] != settled[(y * 1280 + x) as usize])
    });
    assert!(fading, "the header fades in");
    let frames = run_out(&mut pair, &mut outs, &st, &mut clock, "the reduced win");
    assert!(frames as f64 * halation::FRAME_MS <= 150.0 + halation::FRAME_MS);
}

/// Alt-clicks (turns clockwise) the first movable board piece whose one turn unwins the solved
/// level (a turn that keeps it won is turned back at once); returns its cell's centre.
fn turn_off_the_solution(game: &mut GameState, lightbox: &Lightbox) -> (i32, i32) {
    for i in 0..BOARD_CELLS {
        if !game.board().cells[i].piece.is_movable() {
            continue;
        }
        let (x, y) = at(lightbox, i);
        for alt in [true, false] {
            game.handle_on(lightbox, InputEvent::MouseDown { x, y, alt });
            game.handle_on(lightbox, InputEvent::MouseUp { x, y, alt });
            if !game.is_won() {
                return (x, y);
            }
        }
    }
    panic!("no single turn unwins the level");
}

#[test]
fn a_win_by_a_turn_runs_the_turn_its_sweep_then_the_win() {
    // C1 level 1 solved, a piece turned off the solution and its cues run out; turned back, it
    // wins: the turn, its sweep and the pending win (ruling M10(d)) run together, every frame
    // against a whole repaint, settling to cold.
    let mut st = state(Game::C1, 0, true);
    let mut pair = twin(easel(), false);
    let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    let (x, y) = turn_off_the_solution(&mut st, &pair.0);
    let now = clock.later();
    step_both(&mut pair, &mut outs, &st, now, "unwon");
    run_out(&mut pair, &mut outs, &st, &mut clock, "the unwinning turn");
    st.handle_on(&pair.0, InputEvent::MouseDown { x, y, alt: false });
    st.handle_on(&pair.0, InputEvent::MouseUp { x, y, alt: false });
    assert!(st.is_won(), "turned back, it wins");
    let now = clock.later();
    step_both(&mut pair, &mut outs, &st, now, "the winning turn");
    let m = pair.0.choreography().moment(now);
    assert!(
        m.turn.is_some() && m.win.is_some(),
        "a turn and a win: {m:?}"
    );
    let mut packets = false;
    while pair.0.next_wake(clock.now).is_some() {
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "the winning turn");
        let m = pair.0.choreography().moment(now);
        packets |= m.win.is_some_and(|w| w.packet > 0.0) && m.turn.is_none();
    }
    assert!(packets, "the packets run once the turn is done");
    assert!(
        outs.0 == common::cold(easel(), halation::Scene::of(&st)),
        "settles to the cold picture"
    );
}

/// A press and release at `(x, y)`: a turn, clockwise when `alt`.
fn click(game: &mut GameState, lightbox: &Lightbox, (x, y): (i32, i32), alt: bool) {
    game.handle_on(lightbox, InputEvent::MouseDown { x, y, alt });
    game.handle_on(lightbox, InputEvent::MouseUp { x, y, alt });
}

/// How many of the running and pending bursts sit on a target the light no longer satisfies.
fn dark_bursts(lightbox: &Lightbox, game: &GameState) -> usize {
    let cells = &game.board().cells;
    lightbox
        .choreography()
        .bursts()
        .iter()
        .filter(|b| game.field().received(b.cell) != cells[b.cell].colour)
        .count()
}

#[test]
fn a_burst_ends_when_its_target_goes_dark_again() {
    // A solved level's piece turned off the solution and its cues run out; turned back, it wins
    // and its targets' bursts wait for the sweep's front; one frame on it is turned off again.
    // No burst stays on a target the light left, running or pending; every frame against a whole
    // repaint, settling to cold.
    for (game, slot) in [(Game::C1, 0), (Game::C1, 6), (Game::C2, 0), (Game::C3, 16)] {
        let what = format!("{game} slot {slot}");
        let mut st = state(game, slot, true);
        let mut pair = twin(easel(), false);
        let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
        let mut clock = Clock { now: 0.0 };
        step_both(&mut pair, &mut outs, &st, 0.0, "cold");
        let xy = turn_off_the_solution(&mut st, &pair.0);
        let now = clock.later();
        step_both(&mut pair, &mut outs, &st, now, "off");
        run_out(&mut pair, &mut outs, &st, &mut clock, "off");
        click(&mut st, &pair.0, xy, false);
        assert!(st.is_won(), "{what}: turned back, it wins");
        let now = clock.later();
        step_both(&mut pair, &mut outs, &st, now, "back on");
        assert!(
            !pair.0.choreography().bursts().is_empty(),
            "{what}: its targets burst"
        );
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "one frame on");
        click(&mut st, &pair.0, xy, true);
        assert!(!st.is_won(), "{what}: off again");
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "off again");
        assert_eq!(
            dark_bursts(&pair.0, &st),
            0,
            "{what}: a burst on a dark target"
        );
        let mut frames = 0;
        while pair.0.next_wake(clock.now).is_some() {
            assert!(frames < 400, "{what}: never settles");
            let now = clock.tick();
            step_both(&mut pair, &mut outs, &st, now, &what);
            assert_eq!(
                dark_bursts(&pair.0, &st),
                0,
                "{what} at {now:.0} ms: a burst on a dark target"
            );
            frames += 1;
        }
        assert!(
            outs.0 == common::cold(easel(), halation::Scene::of(&st)),
            "{what}: settles to the cold picture"
        );
    }
}

#[test]
fn a_win_by_a_drop_lands_the_piece_then_runs_the_win() {
    // C1 level 1 solved, a piece of the solution carried off to the tray (painted held, then
    // landing), and back onto its cell (held, then released there): the land, the drop's sweep
    // and the win run together, every frame against a whole repaint, on the desktop and a phone.
    for easel in easels() {
        let n = (easel.width * easel.height) as usize;
        let width = easel.width as usize;
        let mut st = state(Game::C1, 0, true);
        let mut pair = twin(easel, false);
        let mut outs = (vec![0u32; n], vec![0u32; n]);
        let mut clock = Clock { now: 0.0 };
        let step = |pair: &mut (Lightbox, Lightbox),
                    outs: &mut (Vec<u32>, Vec<u32>),
                    st: &GameState,
                    now: f64| {
            pair.0.paint(st, now, Output::Xrgb(&mut outs.0));
            pair.1.paint(st, now, Output::Xrgb(&mut outs.1));
            assert!(
                outs.0 == outs.1,
                "touch {} at {now:.1} ms: an incremental frame differs from a whole one",
                easel.touch
            );
        };
        let run = |pair: &mut (Lightbox, Lightbox),
                   outs: &mut (Vec<u32>, Vec<u32>),
                   st: &GameState,
                   clock: &mut Clock| {
            while pair.0.next_wake(clock.now).is_some() {
                let now = clock.tick();
                step(pair, outs, st, now);
            }
        };
        step(&mut pair, &mut outs, &st, 0.0);
        let slot = toolbox_index(VISIBLE_TOOLBOX_SLOTS - 1);
        assert_eq!(st.board().cells[slot].piece, Piece::Empty);
        let cell = (0..BOARD_CELLS)
            .find(|&i| st.board().cells[i].piece.is_movable())
            .expect("a piece of the solution");
        // Carries the piece at `from` to `to`, painting it held and the lift run out, then
        // released (the land's first frame, at `now`).
        let carry = |pair: &mut (Lightbox, Lightbox),
                     outs: &mut (Vec<u32>, Vec<u32>),
                     st: &mut GameState,
                     clock: &mut Clock,
                     (from, to): (usize, usize)|
         -> f64 {
            let ((ax, ay), (bx, by)) = (at(&pair.0, from), at(&pair.0, to));
            let (bx, by) = (bx + 5, by + 4);
            st.handle_on(
                &pair.0,
                InputEvent::MouseDown {
                    x: ax,
                    y: ay,
                    alt: false,
                },
            );
            st.handle_on(&pair.0, InputEvent::MouseDragged { x: bx, y: by });
            let now = clock.later();
            step(pair, outs, st, now);
            run(pair, outs, st, clock);
            st.handle_on(
                &pair.0,
                InputEvent::MouseUp {
                    x: bx,
                    y: by,
                    alt: false,
                },
            );
            let now = clock.later();
            step(pair, outs, st, now);
            now
        };
        carry(&mut pair, &mut outs, &mut st, &mut clock, (cell, slot));
        assert!(!st.is_won(), "carried off, unwon");
        run(&mut pair, &mut outs, &st, &mut clock);
        let now = carry(&mut pair, &mut outs, &mut st, &mut clock, (slot, cell));
        assert!(st.is_won(), "dropped back, it wins");
        let m = pair.0.choreography().moment(now);
        assert!(
            m.land.is_some() && m.sweep.is_some() && m.win.is_some(),
            "a land, a sweep and a win: {m:?}"
        );
        // The header shows nothing of YOU WIN! until the sweep has run (M10(d)), then writes on.
        let (mut hidden, mut revealing) = (false, false);
        while pair.0.next_wake(clock.now).is_some() {
            let now = clock.tick();
            step(&mut pair, &mut outs, &st, now);
            let w = pair.0.choreography().moment(now).win;
            hidden |= w.is_some_and(|w| w.reveal == 0.0);
            revealing |= w.is_some_and(|w| w.reveal > 0.0 && w.reveal < 1.0);
        }
        assert!(hidden && revealing, "touch {}", easel.touch);
        let cold = common::cold(easel, halation::Scene::of(&st));
        let differ = outs.0.iter().zip(&cold).position(|(a, b)| a != b);
        assert!(
            differ.is_none(),
            "touch {}: settles to the cold picture (first differs at {:?})",
            easel.touch,
            differ.map(|i| (i % width, i / width))
        );
    }
}

/// Wins the level with the cheat key and goes to the next one (`=`).
fn next_level(game: &mut GameState) {
    game.handle(InputEvent::Key(game.rules().cheat_key));
    game.handle(InputEvent::Key(b'='));
}

/// Whether the board region of `a` and `b` (1280 px across) is the same.
fn same_board(f: halation::DeviceRect, a: &[u32], b: &[u32]) -> bool {
    (f.top..f.bottom).all(|y| {
        (f.left..f.right).all(|x| a[(y * 1280 + x) as usize] == b[(y * 1280 + x) as usize])
    })
}

#[test]
fn a_level_change_cross_fades_then_sweeps_and_settles() {
    for reduced in [false, true] {
        let mut st = state(Game::C2, 0, false);
        let mut pair = twin(easel(), reduced);
        let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
        let mut clock = Clock { now: 0.0 };
        step_both(&mut pair, &mut outs, &st, 0.0, "cold");
        let old = outs.0.clone();
        next_level(&mut st);
        let start = clock.later();
        step_both(&mut pair, &mut outs, &st, start, "the level changes");
        let f = pair.0.darkroom().fade_rect();
        assert!(
            same_board(f, &outs.0, &old),
            "reduced {reduced}: the first frame shows the old board (k = 0)"
        );
        let new = common::cold(easel(), halation::Scene::of(&st));
        while clock.now < start + 75.0 {
            let now = clock.tick();
            step_both(&mut pair, &mut outs, &st, now, "the fade");
        }
        let between = (f.top..f.bottom).step_by(7).any(|y| {
            (f.left..f.right).step_by(7).any(|x| {
                let i = (y * 1280 + x) as usize;
                let (a, b, m) = (luma(old[i]), luma(new[i]), luma(outs.0[i]));
                a.abs_diff(b) > 60 && m > a.min(b) + 10 && m + 10 < a.max(b)
            })
        });
        assert!(
            between,
            "reduced {reduced}: mid-fade, pixels between the two boards"
        );
        let frames = run_out(&mut pair, &mut outs, &st, &mut clock, "the level");
        if reduced {
            assert!(
                frames as f64 * halation::FRAME_MS <= 150.0 + halation::FRAME_MS,
                "a 150 ms fade, no sweep"
            );
        }
    }
}

#[test]
fn a_level_change_mid_sweep_fades_from_what_was_shown() {
    // C1 level 1 solved (level 2 open), a piece turned: 5 frames in, mid-turn and mid-sweep,
    // the next level fades in from the frame on screen.
    let mut st = state(Game::C1, 0, true);
    let mut pair = twin(easel(), false);
    let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    turn_a_piece(&mut st, &pair.0);
    for _ in 0..5 {
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "mid-sweep");
    }
    let m = pair.0.choreography().moment(clock.now);
    assert!(
        m.turn.is_some() && (m.sweep.is_some() || m.fade > 0.0),
        "mid-turn, its beams changing"
    );
    let shown = outs.0.clone();
    st.handle(InputEvent::Key(b'='));
    assert_eq!(st.slot(), 1, "on to level 2");
    let now = clock.tick();
    step_both(
        &mut pair,
        &mut outs,
        &st,
        now,
        "the level changes mid-sweep",
    );
    let f = pair.0.darkroom().fade_rect();
    assert!(same_board(f, &outs.0, &shown), "the old frame, as shown");
    run_out(&mut pair, &mut outs, &st, &mut clock, "after it");
}

#[test]
fn a_level_change_mid_win_or_mid_land_leaves_nothing_behind() {
    // C1 level 1: won with the cheat key and 200 ms into its win (packets out, the header half
    // written), or a tray piece dropped on the board and 2 frames into its land; then the next
    // level. Every frame against a whole repaint, settling to the new level's cold picture.
    for mid_land in [false, true] {
        let mut st = state(Game::C1, 0, false);
        let mut pair = twin(easel(), false);
        let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
        let mut clock = Clock { now: 0.0 };
        step_both(&mut pair, &mut outs, &st, 0.0, "cold");
        if mid_land {
            pick_up(&mut st, &pair.0, board_index(8, 7));
            let now = clock.later();
            step_both(&mut pair, &mut outs, &st, now, "held");
            let d = st.drag().expect("held");
            st.handle_on(
                &pair.0,
                InputEvent::MouseUp {
                    x: d.x,
                    y: d.y,
                    alt: false,
                },
            );
            let now = clock.later();
            step_both(&mut pair, &mut outs, &st, now, "dropped");
            for _ in 0..2 {
                let now = clock.tick();
                step_both(&mut pair, &mut outs, &st, now, "landing");
            }
            assert!(pair.0.choreography().moment(clock.now).land.is_some());
        } else {
            st.handle(InputEvent::Key(st.rules().cheat_key));
            let start = clock.later();
            step_both(&mut pair, &mut outs, &st, start, "the win");
            while clock.now < start + 200.0 {
                let now = clock.tick();
                step_both(&mut pair, &mut outs, &st, now, "the win");
            }
            assert!(pair.0.choreography().moment(clock.now).win.is_some());
        }
        next_level(&mut st);
        assert_eq!(st.slot(), 1, "on to level 2");
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "the level changes");
        let m = pair.0.choreography().moment(now);
        assert!(
            m.level.is_some() && m.win.is_none() && m.land.is_none(),
            "only the fade: level {:?}, win {:?}, land {:?}",
            m.level,
            m.win,
            m.land
        );
        let what = if mid_land { "mid-land" } else { "mid-win" };
        run_out(&mut pair, &mut outs, &st, &mut clock, what);
    }
}

#[test]
fn no_fade_under_the_open_sheet() {
    let easel = Easel::new(780, 1480, 2.0).with_touch(true);
    let mut st = state(Game::C2, 0, false);
    let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
    assert!(lightbox.command(halation::Control::Levels));
    let mut out = vec![0u32; 780 * 1480];
    lightbox.paint(&st, 0.0, Output::Xrgb(&mut out));
    next_level(&mut st);
    lightbox.paint(&st, 1000.0, Output::Xrgb(&mut out));
    assert_eq!(lightbox.choreography().moment(1000.0).level, None);
}

#[test]
fn a_level_chosen_in_the_sheet_does_not_fade() {
    // Ruling F4: a chip chosen in the open sheet closes it in the paint that opens its level; a
    // fade would start from a frame holding the scrim and the panel.
    let easel = Easel::new(780, 1480, 2.0).with_touch(true);
    let n = (easel.width * easel.height) as usize;
    let mut st = state(Game::C2, 0, false);
    let mut pair = twin(easel, false);
    let mut outs = (vec![0u32; n], vec![0u32; n]);
    let mut clock = Clock { now: 0.0 };
    let step = |pair: &mut (Lightbox, Lightbox),
                outs: &mut (Vec<u32>, Vec<u32>),
                st: &GameState,
                now: f64| {
        pair.0.paint(st, now, Output::Xrgb(&mut outs.0));
        pair.1.paint(st, now, Output::Xrgb(&mut outs.1));
        assert!(
            outs.0 == outs.1,
            "at {now:.1} ms: an incremental frame differs from a whole one"
        );
    };
    step(&mut pair, &mut outs, &st, 0.0);
    assert!(pair.0.command(halation::Control::Levels));
    assert!(pair.1.command(halation::Control::Levels));
    let now = clock.later();
    step(&mut pair, &mut outs, &st, now);
    while pair.0.next_wake(clock.now).is_some() {
        let now = clock.tick();
        step(&mut pair, &mut outs, &st, now);
    }
    // C2 opens slots 0–4 on a fresh save.
    let chip = pair.0.tableau().sheet_chip_rect(3);
    let (x, y) = ((chip.left + chip.right) / 2, (chip.top + chip.bottom) / 2);
    for event in [
        InputEvent::MouseDown { x, y, alt: false },
        InputEvent::MouseUp { x, y, alt: false },
    ] {
        let _ = pair.1.route(event.clone());
        if let halation::Route::Game(e) = pair.0.route(event) {
            st.handle_on(&pair.0, e);
        }
    }
    assert_eq!(st.slot(), 3, "the chip's level");
    let now = clock.tick();
    step(&mut pair, &mut outs, &st, now);
    assert!(!pair.0.sheet_open(), "the chip closes the sheet");
    assert_eq!(pair.0.choreography().moment(now).level, None);
    run_out(
        &mut pair,
        &mut outs,
        &st,
        &mut clock,
        "a level chosen in the sheet",
    );
}

#[test]
fn a_level_change_mid_fade_fades_on_from_the_frame_shown() {
    // C2 level 1, then level 2, then (66.7 ms into its fade) level 3: the second fade starts
    // from the half-faded frame on screen. Every frame against a whole repaint, settling to cold.
    let mut st = state(Game::C2, 0, false);
    let mut pair = twin(easel(), false);
    let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    next_level(&mut st);
    let start = clock.later();
    step_both(&mut pair, &mut outs, &st, start, "level 2");
    while clock.now < start + 60.0 {
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "level 2's fade");
    }
    let k = pair.0.choreography().moment(clock.now).level;
    assert!(k.is_some_and(|k| k > 0.0 && k < 1.0), "mid-fade: {k:?}");
    let shown = outs.0.clone();
    next_level(&mut st);
    assert_eq!(st.slot(), 2, "on to level 3");
    let now = clock.tick();
    step_both(&mut pair, &mut outs, &st, now, "level 3 mid-fade");
    let f = pair.0.darkroom().fade_rect();
    assert!(
        same_board(f, &outs.0, &shown),
        "the half-faded frame, as shown"
    );
    run_out(&mut pair, &mut outs, &st, &mut clock, "level 3");
}

#[test]
fn reduced_motion_turned_on_mid_fade_settles_at_once() {
    let mut st = state(Game::C2, 0, false);
    let mut pair = twin(easel(), false);
    let mut outs = (vec![0u32; 1280 * 800], vec![0u32; 1280 * 800]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    next_level(&mut st);
    let start = clock.later();
    step_both(&mut pair, &mut outs, &st, start, "level 2");
    while clock.now < start + 60.0 {
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "the fade");
    }
    pair.0.set_reduced_motion(true);
    pair.1.set_reduced_motion(true);
    assert_eq!(pair.0.next_wake(clock.now), None, "the fade is over");
    let now = clock.tick();
    step_both(&mut pair, &mut outs, &st, now, "reduced mid-fade");
    assert!(
        outs.0 == common::cold(easel(), halation::Scene::of(&st)),
        "the next frame is the settled level"
    );
}

#[test]
fn paints_at_a_level_changes_moment_keep_the_frame_shown_before_it() {
    // Review M4: a piece picked up on the new level and carried, painted at the same `now` as
    // the change, must not be kept as part of the old frame (it would fade out where it was).
    let mut st = state(Game::C2, 0, false);
    let mut lightbox = Lightbox::new(easel(), Theme::Dark, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint(&st, 0.0, Output::Xrgb(&mut out));
    let old = out.clone();
    next_level(&mut st);
    let now = 10_000.0;
    lightbox.paint(&st, now, Output::Xrgb(&mut out));
    let empty: Vec<usize> = (0..BOARD_CELLS)
        .filter(|&i| st.board().cells[i].piece == Piece::Empty)
        .collect();
    let (a, b) = (empty[0], empty[empty.len() - 1]);
    pick_up(&mut st, &lightbox, a);
    assert!(st.drag().is_some(), "a tray piece held over the board");
    lightbox.paint(&st, now, Output::Xrgb(&mut out));
    let to = lightbox.tableau().cell_centre(b);
    st.handle_on(
        &lightbox,
        InputEvent::MouseDragged {
            x: to.x as i32,
            y: to.y as i32,
        },
    );
    lightbox.paint(&st, now, Output::Xrgb(&mut out));
    assert!(lightbox.choreography().moment(now).level == Some(0.0));
    let r = lightbox.tableau().cell_rect(a);
    let same = (r.top..r.bottom).all(|y| {
        (r.left..r.right).all(|x| out[(y * 1280 + x) as usize] == old[(y * 1280 + x) as usize])
    });
    assert!(
        same,
        "where the piece was held, the old frame (no kept piece)"
    );
}

fn phone() -> Easel {
    Easel::new(780, 1480, 2.0).with_touch(true)
}

/// A pixel (as an index) out of every frame of the sheet's reach, 2 px from the canvas's top:
/// above the panel's shadow (12 pt down, blurred 40 pt, so reaching 28 pt above the panel),
/// which only sinks lower as the panel does. Only the scrim changes it.
fn beyond_the_sheet(lightbox: &Lightbox) -> usize {
    let t = lightbox.tableau();
    let p = t.device(t.sheet.panel);
    let shadow_top = p.top as f32 + (12.0 - 40.0) * t.px_per_pt();
    assert!(shadow_top > 3.0, "room above the sheet's shadow: {p:?}");
    let w = t.canvas().right;
    (2 * w + w / 2) as usize
}

/// The sheet panel's centre (as an index).
fn panel_centre(lightbox: &Lightbox) -> usize {
    let t = lightbox.tableau();
    let p = t.device(t.sheet.panel);
    (((p.top + p.bottom) / 2) * t.canvas().right + (p.left + p.right) / 2) as usize
}

fn toggle_both(pair: &mut (Lightbox, Lightbox), control: halation::Control) {
    for lb in [&mut pair.0, &mut pair.1] {
        assert!(lb.command(control), "{control:?}");
    }
}

#[test]
fn the_sheet_fades_and_rises_open_and_sinks_closed() {
    // Spec §9.1 "Sheet": the scrim at once; the panel fades in rising 12 pt into place (200 ms),
    // and on closing fades out sinking, its scrim gone at once. Reduced motion: a 150 ms fade.
    for reduced in [false, true] {
        let easel = phone();
        let n = 780 * 1480;
        let st = state(Game::C1, 0, false);
        let mut pair = twin(easel, reduced);
        let mut outs = (vec![0u32; n], vec![0u32; n]);
        let mut clock = Clock { now: 0.0 };
        step_both(&mut pair, &mut outs, &st, 0.0, "cold");
        let (far, centre) = (beyond_the_sheet(&pair.0), panel_centre(&pair.0));
        let bare = common::cold_with(easel, halation::Scene::of(&st), false);
        let shown = common::cold_with(easel, halation::Scene::of(&st), true);
        assert_ne!(bare[far], shown[far], "the scrim darkens it");
        toggle_both(&mut pair, halation::Control::Levels);
        let start = clock.later();
        step_both(&mut pair, &mut outs, &st, start, "the sheet opens");
        assert_eq!(
            outs.0[far], shown[far],
            "reduced {reduced}: the scrim at once"
        );
        assert_ne!(
            outs.0[centre], shown[centre],
            "reduced {reduced}: the panel not yet shown"
        );
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "opening");
        let mid = outs.0.clone();
        let frames = run_out_with(
            &mut pair,
            &mut outs,
            &st,
            &mut clock,
            true,
            "the open sheet",
        );
        assert!(
            mid != outs.0,
            "reduced {reduced}: the panel was still fading in"
        );
        let ms = if reduced { 150.0 } else { 200.0 };
        assert!(
            (frames as f64 + 1.0) * halation::FRAME_MS >= ms
                && (frames as f64) * halation::FRAME_MS <= ms + halation::FRAME_MS,
            "reduced {reduced}: {frames} frames for {ms} ms"
        );
        toggle_both(&mut pair, halation::Control::CloseSheet);
        let now = clock.later();
        step_both(&mut pair, &mut outs, &st, now, "the sheet closes");
        assert_eq!(
            outs.0[far], bare[far],
            "reduced {reduced}: the scrim lifts at once"
        );
        assert_ne!(
            outs.0[centre], bare[centre],
            "reduced {reduced}: the panel still shows"
        );
        run_out_with(
            &mut pair,
            &mut outs,
            &st,
            &mut clock,
            false,
            "the closed sheet",
        );
    }
}

#[test]
fn a_sheet_turned_back_mid_fade_equals_whole_repaints() {
    // Opened, closed 3 frames in, opened again 2 frames later: each cue goes on from where the
    // last left the panel, every frame against a whole repaint.
    let st = state(Game::C2, 0, false);
    let mut pair = twin(phone(), false);
    let mut outs = (vec![0u32; 780 * 1480], vec![0u32; 780 * 1480]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    for (control, ticks) in [
        (halation::Control::Levels, 3),
        (halation::Control::CloseSheet, 2),
        (halation::Control::Levels, 0),
    ] {
        toggle_both(&mut pair, control);
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "turned");
        for _ in 0..ticks {
            let now = clock.tick();
            step_both(&mut pair, &mut outs, &st, now, "mid-fade");
        }
    }
    run_out_with(&mut pair, &mut outs, &st, &mut clock, true, "open again");
}

#[test]
fn a_level_change_with_no_fade_ends_the_one_running() {
    // `=`, then within the 180 ms of its cross-fade the sheet opened and closed and `=` again:
    // the second change skips its fade (the close covers it), and must end the first one, or the
    // board would go on blending from the frame kept two levels back.
    let mut st = state(Game::C2, 0, false);
    let mut pair = twin(phone(), false);
    let mut outs = (vec![0u32; 780 * 1480], vec![0u32; 780 * 1480]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    next_level(&mut st);
    let start = clock.later();
    step_both(&mut pair, &mut outs, &st, start, "level 2");
    toggle_both(&mut pair, halation::Control::Levels);
    let now = clock.tick();
    step_both(&mut pair, &mut outs, &st, now, "the sheet opens");
    toggle_both(&mut pair, halation::Control::CloseSheet);
    let now = clock.tick();
    step_both(&mut pair, &mut outs, &st, now, "the sheet closes");
    assert!(
        pair.0.choreography().moment(now).level.is_some(),
        "level 2 still fading"
    );
    next_level(&mut st);
    assert_eq!(st.slot(), 2, "on to level 3");
    let now = clock.tick();
    assert!(now < start + 180.0, "within level 2's fade");
    step_both(&mut pair, &mut outs, &st, now, "level 3 mid-close");
    assert_eq!(
        pair.0.choreography().moment(now).level,
        None,
        "no level fade left running"
    );
    assert_eq!(pair.0.choreography().level_since(), None);
    run_out(&mut pair, &mut outs, &st, &mut clock, "level 3");
}

#[test]
fn a_level_change_while_the_sheet_closes_does_not_fade_from_its_panel() {
    // `=` pressed while the sheet is still fading out: a level fade would start from a frame
    // holding the half-faded panel. The close cue covers the change instead (no level fade, no
    // frame kept), and the panel goes on fading out over the new level.
    let mut st = state(Game::C2, 0, false);
    let easel = phone();
    let mut pair = twin(easel, false);
    let mut outs = (vec![0u32; 780 * 1480], vec![0u32; 780 * 1480]);
    let mut clock = Clock { now: 0.0 };
    step_both(&mut pair, &mut outs, &st, 0.0, "cold");
    let far = beyond_the_sheet(&pair.0);
    toggle_both(&mut pair, halation::Control::Levels);
    let now = clock.later();
    step_both(&mut pair, &mut outs, &st, now, "the sheet opens");
    run_out_with(&mut pair, &mut outs, &st, &mut clock, true, "open");
    toggle_both(&mut pair, halation::Control::CloseSheet);
    let now = clock.later();
    step_both(&mut pair, &mut outs, &st, now, "the sheet closes");
    for _ in 0..3 {
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, "closing");
    }
    next_level(&mut st);
    assert_eq!(st.slot(), 1, "on to level 2");
    let now = clock.tick();
    step_both(
        &mut pair,
        &mut outs,
        &st,
        now,
        "the level changes mid-close",
    );
    let m = pair.0.choreography().moment(now);
    assert_eq!(m.level, None, "no level fade");
    assert_eq!(pair.0.choreography().level_since(), None, "no frame kept");
    assert!(
        m.sheet
            .is_some_and(|p| !p.scrim && p.alpha > 0.0 && p.alpha < 1.0),
        "the panel still fading out: {:?}",
        m.sheet
    );
    let new = common::cold(easel, halation::Scene::of(&st));
    assert_eq!(outs.0[far], new[far], "no scrim");
    run_out(&mut pair, &mut outs, &st, &mut clock, "after the close");
}

/// A running cue the matrix interrupts.
#[derive(Clone, Copy, Debug)]
enum Cue {
    /// C1 level 1 solved, a piece turned off it (settled) and back: sweeping, 3 frames in.
    Sweep,
    /// C2 level 1 solved, a piece turned off it: the piece turning, 2 frames in.
    Turn,
    /// C1 level 1, a tray piece carried to an empty cell and dropped: landing, 2 frames in.
    Land,
    /// `Sweep` 50 frames in: the win's packets out.
    Win,
    /// C1 level 1 won with the cheat key (no beam moves): 20 frames in.
    CheatWin,
    /// C2 level 1 won with the cheat key, then `=`: the board cross-fading, 2 frames in.
    LevelFade,
    /// C1 level 1 won by a paste under the open level sheet: 30 frames in.
    WinUnderSheet,
}

/// What interrupts it.
#[derive(Clone, Copy, Debug)]
enum Cut {
    OpenSheet,
    CloseSheet,
    Reduced,
    /// The cue's piece turned clockwise (again).
    TurnAgain,
    Cheat,
    Paste,
    Restart,
    /// The cue's piece lifted and held 4 frames, then released there (`true`) or cancelled.
    Lift(bool),
    /// A tray piece carried onto an empty cell, held 3 frames, then dropped.
    CarryIn,
}

#[test]
fn cues_interrupted_equal_whole_repaints_and_settle_to_cold() {
    // Cue × interrupt (the final review's interleavings): on a phone (it has the level sheet),
    // every frame of each against a whole repaint, then the cold picture (the sheet as left).
    // The cue is checked to be running when it is interrupted.
    use Cue::*;
    use Cut::*;
    let matrix = [
        (Sweep, OpenSheet),
        (Turn, Cheat),
        (Turn, Restart),
        (Turn, Lift(true)),
        (Turn, TurnAgain),
        (Land, OpenSheet),
        (Land, Reduced),
        (Land, TurnAgain),
        (Win, Lift(false)),
        (CheatWin, Paste),
        (LevelFade, OpenSheet),
        (LevelFade, CarryIn),
        (WinUnderSheet, CloseSheet),
    ];
    let easel = phone();
    let n = (easel.width * easel.height) as usize;
    for (cue, cut) in matrix {
        let what = format!("{cut:?} mid-{cue:?}");
        let (game, solved) = match cue {
            Sweep | Win => (Game::C1, true),
            Turn => (Game::C2, true),
            Land | CheatWin | WinUnderSheet => (Game::C1, false),
            LevelFade => (Game::C2, false),
        };
        let mut st = state(game, 0, solved);
        let mut pair = twin(easel, false);
        let mut outs = (vec![0u32; n], vec![0u32; n]);
        let mut clock = Clock { now: 0.0 };
        step_both(&mut pair, &mut outs, &st, 0.0, "cold");
        let ticks = |pair: &mut (Lightbox, Lightbox),
                     outs: &mut (Vec<u32>, Vec<u32>),
                     st: &GameState,
                     clock: &mut Clock,
                     k: usize| {
            for _ in 0..k {
                let now = clock.tick();
                step_both(pair, outs, st, now, &what);
            }
        };
        let cell_of = |lightbox: &Lightbox, xy| (0..BOARD_CELLS).find(|&i| at(lightbox, i) == xy);
        // The cue, the cell of its piece, and how many frames it runs before the cut.
        let (cell, k) = match cue {
            Turn => {
                let xy = turn_off_the_solution(&mut st, &pair.0);
                (cell_of(&pair.0, xy), 2)
            }
            Land => {
                let to = (0..BOARD_CELLS)
                    .filter(|&i| st.board().cells[i].piece == Piece::Empty)
                    .nth(40)
                    .expect("an empty cell");
                pick_up(&mut st, &pair.0, to);
                let now = clock.later();
                step_both(&mut pair, &mut outs, &st, now, "lift");
                ticks(&mut pair, &mut outs, &st, &mut clock, 2);
                let d = st.drag().expect("held");
                let up = InputEvent::MouseUp {
                    x: d.x,
                    y: d.y,
                    alt: false,
                };
                st.handle_on(&pair.0, up);
                (Some(to), 2)
            }
            Sweep | Win => {
                let xy = turn_off_the_solution(&mut st, &pair.0);
                let now = clock.later();
                step_both(&mut pair, &mut outs, &st, now, "off");
                run_out(&mut pair, &mut outs, &st, &mut clock, "off");
                click(&mut st, &pair.0, xy, false);
                assert!(st.is_won(), "turned back, it wins");
                let k = if matches!(cue, Sweep) { 3 } else { 50 };
                (cell_of(&pair.0, xy), k)
            }
            CheatWin => {
                st.handle(InputEvent::Key(st.rules().cheat_key));
                (None, 20)
            }
            LevelFade => {
                next_level(&mut st);
                (None, 2)
            }
            WinUnderSheet => {
                toggle_both(&mut pair, halation::Control::Levels);
                let now = clock.later();
                step_both(&mut pair, &mut outs, &st, now, "open");
                run_out_with(&mut pair, &mut outs, &st, &mut clock, true, "open");
                let text = lumen::stored_solution(st.data(), 0).expect("a stored solution");
                st.handle(InputEvent::Paste(text));
                (None, 30)
            }
        };
        let now = clock.later();
        step_both(&mut pair, &mut outs, &st, now, "the cue");
        ticks(&mut pair, &mut outs, &st, &mut clock, k);
        let m = pair.0.choreography().moment(clock.now);
        let running = match cue {
            Sweep => m.sweep.is_some(),
            Turn => m.turn.is_some(),
            Land => m.land.is_some(),
            Win | CheatWin | WinUnderSheet => m.win.is_some(),
            LevelFade => m.level.is_some(),
        };
        assert!(
            running,
            "{what}: the cue runs when interrupted: sweep {}, turn {:?}, land {}, win {}, level {:?}",
            m.sweep.is_some(),
            m.turn,
            m.land.is_some(),
            m.win.is_some(),
            m.level
        );
        let piece = cell.map(|c| at(&pair.0, c));
        match cut {
            OpenSheet => toggle_both(&mut pair, halation::Control::Levels),
            CloseSheet => toggle_both(&mut pair, halation::Control::CloseSheet),
            Reduced => {
                pair.0.set_reduced_motion(true);
                pair.1.set_reduced_motion(true);
            }
            TurnAgain => {
                let cell = cell.expect("the cue's piece");
                let before = st.board().cells[cell];
                click(&mut st, &pair.0, at(&pair.0, cell), true);
                assert_ne!(st.board().cells[cell], before, "{what}: it turned");
            }
            Cheat => {
                assert!(!st.is_won(), "{what}: unwon before");
                st.handle(InputEvent::Key(st.rules().cheat_key));
                assert!(st.is_won(), "{what}");
            }
            Paste => {
                let before = st.board().clone();
                let text = lumen::stored_solution(st.data(), 0).expect("a stored solution");
                st.handle(InputEvent::Paste(text));
                assert!(
                    st.board().cells != before.cells,
                    "{what}: the paste moved pieces"
                );
            }
            Restart => {
                let before = st.board().clone();
                st.handle(InputEvent::Key(b'r'));
                assert!(
                    st.board().cells != before.cells,
                    "{what}: the restart moved pieces"
                );
            }
            Lift(drop) => {
                let (x, y) = piece.expect("the cue's piece");
                st.handle_on(&pair.0, InputEvent::MouseDown { x, y, alt: false });
                let (x, y) = (x + 60, y + 60);
                st.handle_on(&pair.0, InputEvent::MouseDragged { x, y });
                assert!(st.drag().is_some(), "{what}: held");
                ticks(&mut pair, &mut outs, &st, &mut clock, 4);
                let release = if drop {
                    InputEvent::MouseUp { x, y, alt: false }
                } else {
                    InputEvent::MouseDown { x, y, alt: false }
                };
                st.handle_on(&pair.0, release);
                assert!(st.drag().is_none(), "{what}: let go");
            }
            CarryIn => {
                let to = (0..BOARD_CELLS)
                    .filter(|&i| st.board().cells[i].piece == Piece::Empty)
                    .nth(30)
                    .expect("an empty cell");
                pick_up(&mut st, &pair.0, to);
                ticks(&mut pair, &mut outs, &st, &mut clock, 3);
                let d = st.drag().expect("held");
                let up = InputEvent::MouseUp {
                    x: d.x,
                    y: d.y,
                    alt: false,
                };
                st.handle_on(&pair.0, up);
                assert!(st.board().cells[to].piece.is_movable(), "{what}: set down");
            }
        }
        // The shells paint after every cut (reduced motion's too: its `Flag::Redraw`).
        let now = clock.tick();
        step_both(&mut pair, &mut outs, &st, now, &what);
        let sheet = pair.0.sheet_open();
        run_out_with(&mut pair, &mut outs, &st, &mut clock, sheet, &what);
    }
}
