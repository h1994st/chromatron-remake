//! Spec §15 "Reach", on every original level of the four games, at its start and with its stored
//! solution (needs the originals): every lit channel (cell, direction, colour bit) is reached,
//! none is guessed, and each one's reach is exactly the earliest its producers give it, by lumen's
//! own rules (so `reach(n, s', k') ≤ reach(c, s, k) + 1` along every beam step, and there is a
//! step that meets it). Light sent past a crossing or joining a beam at a splitter arrives with
//! its own beam, and every move a stored solution makes lights its targets only once complete.

mod common;

use halation::Reach;
use halation::beamline::{half_colour, strand_start};
use halation::choreography::{Choreography, Observed};
use lumen::layout::STRIP_SLOTS;
use lumen::{
    BOARD_CELLS, BeamField, Board, Cell, Colour, DX, DY, GREEN, InputEvent, Piece, RED, SIDE,
    trace, transfer,
};
use spectrograph::Game;

/// Lumen's `MISPLACED_LEVEL`: Chromatron 1's level 32 (strip slot 15) stores a mirror away from
/// where it solves, so its stored solution is the only one that doesn't win.
const MISPLACED_LEVEL: u8 = 32;
const MISPLACED: (Game, usize) = (Game::C1, 15);

const BITS: [Colour; 3] = [1, 2, 4];

/// The teleport landings on `board`: cells holding a teleport target that emit light no beam
/// brought in.
fn landings(board: &Board, field: &BeamField) -> usize {
    let targets = board.rules().teleport_targets;
    (0..BOARD_CELLS)
        .filter(|&c| {
            targets.contains(&board.cells[c].piece)
                && field.outbound[c].iter().any(|&m| m != 0)
                && field.received(c) == 0
        })
        .count()
}

/// The cell one step from `cell` along `dir` (back along it when `back`), if on the board.
fn step(cell: usize, dir: usize, back: bool) -> Option<usize> {
    let sign = if back { -1 } else { 1 };
    let (x, y) = (
        (cell % SIDE) as i32 + sign * i32::from(DX[dir]),
        (cell / SIDE) as i32 + sign * i32::from(DY[dir]),
    );
    let on = |v: i32| (0..SIDE as i32).contains(&v);
    (on(x) && on(y)).then(|| y as usize * SIDE + x as usize)
}

/// When light of `bit` entering `n` travelling `s` is at its centre: one step after it left.
fn entered_at(reach: &Reach, n: usize, s: usize, bit: Colour) -> u16 {
    let up = step(n, s, true).expect("light enters from the board");
    reach.leaving(up, s, bit) + 1
}

/// The earliest reach the producers of light `bit` leaving `n` travelling `d` give it, by
/// lumen's rules: a laser's own at 0; a complementor's once all it complements has entered
/// travelling `d`; a gate's once both its inputs have `bit`; any other piece's one step after
/// light it passes on (per bit, by `transfer`; a beam an entangled twin recoloured counts by its
/// direction) entered; a teleport landing's one step after the jump's light entered its
/// teleporter.
fn produced(
    board: &Board,
    field: &BeamField,
    reach: &Reach,
    n: usize,
    d: usize,
    bit: Colour,
) -> u16 {
    let rules = board.rules();
    let cell = board.cells[n];
    let out = field.outbound[n][d];
    let mut best = u16::MAX;
    match cell.piece {
        Piece::Laser => best = 0,
        Piece::Complementor => {
            let k = field.inbound[n][d];
            if let Some(r) = BITS
                .into_iter()
                .filter(|&e| k & e != 0)
                .map(|e| entered_at(reach, n, d, e))
                .max()
            {
                best = r;
            }
        }
        Piece::LogicGate | Piece::PrimaryLogicGate => {
            let p = usize::from(cell.dir & 7);
            let (a, z) = ((p + 1) & 7, (p + 7) & 7);
            if d == p && field.inbound[n][a] & field.inbound[n][z] & bit != 0 {
                best = entered_at(reach, n, a, bit).max(entered_at(reach, n, z, bit));
            }
        }
        _ => {
            for s in 0..8 {
                for k in BITS {
                    if field.inbound[n][s] & k == 0 {
                        continue;
                    }
                    let mut tag = 0;
                    for e in transfer(rules, cell, s as u8, k, 0, &mut tag).as_slice() {
                        let lit = e.colour & out;
                        if usize::from(e.dir) == d && (lit & bit != 0 || lit == 0) {
                            best = best.min(entered_at(reach, n, s, k));
                        }
                    }
                }
            }
        }
    }
    // A jump lands on the first teleport target past its teleporter.
    let target = rules.teleport_targets.contains(&cell.piece);
    let mut at = step(n, d, true).filter(|_| target);
    while let Some(t) = at {
        let k = field.inbound[t][d];
        let teleporter = matches!(
            board.cells[t].piece,
            Piece::Teleporter | Piece::MobileTeleporter
        );
        if teleporter && k != 0 {
            for e in BITS {
                if k & e != 0 && (e == bit || out & e == 0) {
                    best = best.min(entered_at(reach, t, d, e) + 1);
                }
            }
        }
        if rules.teleport_targets.contains(&board.cells[t].piece) {
            break;
        }
        at = step(t, d, true);
    }
    best
}

/// What is wrong with `board`'s reach, if anything: a guess, a lit cell unreached, or a lit
/// channel whose reach isn't the earliest its producers give it.
fn faults(board: &Board, field: &BeamField) -> Option<String> {
    let reach = Reach::compute(board, field);
    let mut found = Vec::new();
    if reach.guessed != 0 {
        found.push(format!("{} lit cells guessed", reach.guessed));
    }
    for c in 0..BOARD_CELLS {
        let lit = field.received(c) != 0 || field.outbound[c].iter().any(|&m| m != 0);
        if lit && board.cells[c].piece != Piece::Laser && reach.cells[c] == 0 {
            found.push(format!("cell {c} lit but unreached"));
        }
        for d in 0..8 {
            for bit in BITS {
                if field.outbound[c][d] & bit == 0 {
                    continue;
                }
                let (r, want) = (
                    reach.leaving(c, d, bit),
                    produced(board, field, &reach, c, d, bit),
                );
                if r != want {
                    found.push(format!("({c}, {d}, {bit}): {r}, its producers give {want}"));
                }
            }
        }
    }
    (!found.is_empty()).then(|| {
        format!(
            "{}\n{}",
            found.join("; "),
            lumen::beam_diagram(board, field)
        )
    })
}

/// `board` with the piece at `cell` taken off, and its field.
fn without(board: &Board, cell: usize) -> (Board, BeamField) {
    let mut board = board.clone();
    board.cells[cell] = Cell::default();
    let mut field = BeamField::default();
    trace(&board, &mut field);
    (board, field)
}

/// A choreography that saw `before` become `after` (one move, `slot` unchanged) at 1000 ms.
fn observed(before: &(Board, BeamField), after: &(Board, BeamField)) -> Box<Choreography> {
    let mut choreography = Choreography::new();
    choreography.observe(
        &Observed {
            board: [&before.0, &after.0],
            field: [&before.1, &after.1],
            drag: [None, None],
            slot: [3, 3],
            won: [false, false],
            sheet: [false, false],
        },
        1000.0,
    );
    choreography
}

/// When target `t` of `after` is complete, in reach units: each bit it needs is at its centre (a
/// bit `old` already delivered at −∞), the latest of them.
fn completion(after: &(Board, BeamField), old: &BeamField, reach: &Reach, t: usize) -> f32 {
    let (board, field) = after;
    let mut complete = f32::NEG_INFINITY;
    for bit in BITS {
        if board.cells[t].colour & bit == 0 || old.received(t) & bit != 0 {
            continue;
        }
        let first = (0..8)
            .filter(|&s| field.inbound[t][s] & bit != 0)
            .map(|s| f32::from(entered_at(reach, t, s, bit)))
            .fold(f32::INFINITY, f32::min);
        complete = complete.max(first);
    }
    complete
}

/// C1's level 4 on the strip (slot 3) solved by its stored solution, from its board just before the
/// splitter at (6, 4) goes in: that move sends light down column 6 through the near target
/// (6, 5), across the laser's beam on row 7 (an empty crossing, lit long before) to the far
/// target (6, 11). The light runs down the column in that order.
#[test]
fn light_sent_down_past_a_crossing_arrives_in_order() {
    let solved = common::state(Game::C1, 3, true);
    let after = (solved.board().clone(), solved.field().clone());
    let at = |x: usize, y: usize| y * SIDE + x;
    let (splitter, near, far) = (at(6, 4), at(6, 5), at(6, 11));
    assert_eq!(after.0.cells[splitter].piece, Piece::Splitter);
    assert_eq!(after.0.cells[near].piece, Piece::Pinwheel);
    assert_eq!(after.0.cells[far].piece, Piece::Pinwheel);
    let before = without(&after.0, splitter);
    assert_eq!(before.1.received(near), 0, "dark before the move");

    let reach = Reach::compute(&after.0, &after.1);
    let diagram = lumen::beam_diagram(&after.0, &after.1);
    assert!(
        reach.cells[near] < reach.cells[far],
        "the near target first: {} vs {}\n{diagram}",
        reach.cells[near],
        reach.cells[far]
    );
    // Down the column from the splitter, each half (in from the north, then out to the south)
    // starts no earlier than the one above it.
    let mut last = f32::NEG_INFINITY;
    for y in 4..SIDE {
        let c = at(6, y);
        for side in [0, 4] {
            if half_colour(&after.1, c, side) & GREEN == 0 {
                continue;
            }
            let (start, _) = strand_start(&after.1, &reach, c, side, GREEN);
            assert!(
                start >= last,
                "(6, {y}) side {side} starts at {start}, before {last} above it\n{diagram}"
            );
            last = start;
        }
    }

    let choreography = observed(&before, &after);
    let burst = |cell| {
        choreography
            .bursts()
            .iter()
            .find(|b| b.cell == cell)
            .map(|b| b.at)
            .expect("a burst")
    };
    assert!(
        burst(near) < burst(far),
        "the near target bursts first: {} vs {}",
        burst(near),
        burst(far)
    );
}

/// C1's level 5 on the strip (slot 4) solved, from its board just before the mirror at (7, 6)
/// goes in: red from it joins green at the splitter at (7, 8), red six cells on and green eight.
/// Red's part of the yellow beams from there starts only when red gets there, and the yellow
/// targets at (7, 11) and (12, 7) burst once red completes them.
#[test]
fn light_joining_a_beam_at_a_splitter_keeps_its_own_reach() {
    let solved = common::state(Game::C1, 4, true);
    let after = (solved.board().clone(), solved.field().clone());
    let at = |x: usize, y: usize| y * SIDE + x;
    let (mirror, splitter) = (at(7, 6), at(7, 8));
    assert_eq!(after.0.cells[mirror].piece, Piece::Mirror);
    assert_eq!(after.0.cells[splitter].piece, Piece::Splitter);
    let reach = Reach::compute(&after.0, &after.1);
    let diagram = lumen::beam_diagram(&after.0, &after.1);
    for side in [2, 4] {
        assert_eq!(
            half_colour(&after.1, splitter, side),
            RED | GREEN,
            "yellow\n{diagram}"
        );
        assert_eq!(
            strand_start(&after.1, &reach, splitter, side, GREEN),
            (6.0, false)
        );
        assert_eq!(
            strand_start(&after.1, &reach, splitter, side, RED),
            (8.0, false),
            "red gets there two cells after green\n{diagram}"
        );
    }

    let before = without(&after.0, mirror);
    let choreography = observed(&before, &after);
    for (target, complete) in [(at(7, 11), 11.0), (at(12, 7), 14.0)] {
        assert_eq!(completion(&after, &before.1, &reach, target), complete);
        let burst = choreography
            .bursts()
            .iter()
            .find(|b| b.cell == target)
            .expect("a burst");
        let front = choreography.moment(burst.at).sweep.expect("a sweep").front;
        assert!(
            (front - complete).abs() < 1e-3,
            "({}, {}) bursts at {front}, complete at {complete}",
            target % SIDE,
            target / SIDE
        );
    }
}

/// Every stored solution, less each of its movable pieces in turn, then that piece put back (the
/// level's last move): every newly lit target bursts when it is complete (spec §9.1, "Burst"),
/// never before; one complete before the sweep starts bursts at its start. Its light shows it lit
/// from the burst on, not before. With the reach exact on every solved board
/// (`every_original_level_reaches_its_light`), no fresh light starts early.
#[test]
fn every_last_move_bursts_its_targets_once_complete() {
    let (mut moves, mut bursts) = (0, 0);
    let mut failures = Vec::new();
    for game in Game::ALL {
        let data = common::data(game);
        for slot in 0..data.level_order.len().min(STRIP_SLOTS) {
            if lumen::stored_solution(data, slot).is_none() {
                continue;
            }
            let solved = common::state(game, slot, (game, slot) != MISPLACED);
            let after = (solved.board().clone(), solved.field().clone());
            let reach = Reach::compute(&after.0, &after.1);
            for m in 0..BOARD_CELLS {
                if !after.0.cells[m].piece.is_movable() {
                    continue;
                }
                moves += 1;
                let before = without(&after.0, m);
                let choreography = observed(&before, &after);
                for burst in choreography.bursts() {
                    bursts += 1;
                    let complete = completion(&after, &before.1, &reach, burst.cell);
                    // A target the sweep reaches last bursts as it ends: the front just before.
                    let front = [burst.at, burst.at - 1e-6]
                        .into_iter()
                        .find_map(|t| choreography.moment(t).sweep)
                        .map_or(f32::INFINITY, |v| v.front);
                    let on_time = (front - complete).abs() < 1e-3
                        || (burst.at == 1000.0 && complete <= front);
                    if !on_time {
                        failures.push(format!(
                            "{game} slot {slot}, ({}, {}) moved: target ({}, {}) bursts at {front}, complete at {complete}",
                            m % SIDE,
                            m / SIDE,
                            burst.cell % SIDE,
                            burst.cell / SIDE
                        ));
                    }
                    // What its light shows (the playbill's `received`, by `SweepView::received`):
                    // its colour, lit, from the burst on, and not just before it.
                    let shows = |t: f64| {
                        choreography
                            .moment(t)
                            .sweep
                            .map_or(after.1.received(burst.cell), |v| {
                                v.received(&after.1, burst.cell)
                            })
                    };
                    let colour = after.0.cells[burst.cell].colour;
                    let lit_then = shows(burst.at) == colour;
                    let dark_before = burst.at == 1000.0 || shows(burst.at - 0.1) != colour;
                    if !lit_then || !dark_before {
                        failures.push(format!(
                            "{game} slot {slot}, ({}, {}) moved: target ({}, {}) bursts at {} ms showing {} (lit {colour}), {} just before",
                            m % SIDE,
                            m / SIDE,
                            burst.cell % SIDE,
                            burst.cell / SIDE,
                            burst.at,
                            shows(burst.at),
                            shows(burst.at - 0.1)
                        ));
                    }
                }
            }
        }
    }
    assert!(
        moves > 1000 && bursts > 1000,
        "{moves} moves, {bursts} bursts"
    );
    assert!(
        failures.is_empty(),
        "{} of {bursts} bursts off their completion:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn every_original_level_reaches_its_light() {
    let mut failures = Vec::new();
    let (mut boards, mut jumps) = (0, 0);
    let mut check = |what: String, board: &Board, field: &BeamField| {
        boards += 1;
        jumps += landings(board, field);
        if let Some(fault) = faults(board, field) {
            failures.push(format!("{what}: {fault}"));
        }
    };
    for game in Game::ALL {
        let data = common::data(game);
        // Every strip slot as the player meets it (pieces in the tray), then with its stored
        // solution pasted where one exists. Every stored solution wins but the misplaced one,
        // which still counts.
        for slot in 0..data.level_order.len().min(STRIP_SLOTS) {
            let mut st = common::state(game, slot, false);
            let what = format!("{game} slot {slot} (start)");
            check(what, st.board(), st.field());
            if let Some(text) = lumen::stored_solution(data, slot) {
                st.handle(InputEvent::Paste(text));
                let misplaced = (game, slot) == MISPLACED;
                if misplaced {
                    let id = data.slot_level(slot).map(|level| level.id);
                    assert_eq!(id, Some(MISPLACED_LEVEL), "{game} slot {slot}: level 32");
                }
                assert_eq!(
                    st.is_won(),
                    !misplaced,
                    "{game} slot {slot}: the stored solution wins (all but the misplaced one)"
                );
                let what = format!("{game} slot {slot} (stored solution)");
                check(what, st.board(), st.field());
            }
        }
        // Every decoded level, on the strip or not, with its pieces where its records put them.
        for level in data.levels.iter() {
            let board = Board::from_level(level);
            let mut field = BeamField::default();
            trace(&board, &mut field);
            let what = format!("{game} level {} (records)", level.id);
            check(what, &board, &field);
        }
    }
    assert!(boards > 400, "{boards} boards checked");
    assert!(jumps > 0, "some boards jump light through teleporters");
    assert!(
        failures.is_empty(),
        "{} of {boards} boards:\n{}",
        failures.len(),
        failures.join("\n").chars().take(20_000).collect::<String>()
    );
}
