//! Spec §15 "Reach", on every original level of the four games, at its start and with its stored
//! solution (needs the originals): every lit cell is reached, no landing is guessed, and
//! `reach[n] ≤ reach[c] + 1` along every lit edge.

mod common;

use halation::Reach;
use lumen::layout::STRIP_SLOTS;
use lumen::{BOARD_CELLS, BeamField, Board, DX, DY, InputEvent, Piece, SIDE, trace};
use spectrograph::Game;

/// Lumen's `MISPLACED_LEVEL`: Chromatron 1's level 32 (strip slot 15) stores a mirror away from
/// where it solves, so its stored solution is the only one that doesn't win.
const MISPLACED_LEVEL: u8 = 32;
const MISPLACED: (Game, usize) = (Game::C1, 15);

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

/// What is wrong with `board`'s reach, if anything.
fn faults(board: &Board, field: &BeamField) -> Option<String> {
    let reach = Reach::compute(board, field);
    let mut found = Vec::new();
    if reach.guessed != 0 {
        found.push(format!("{} lit cells guessed", reach.guessed));
    }
    for c in 0..BOARD_CELLS {
        let lit = field.received(c) != 0 || field.outbound[c].iter().any(|&m| m != 0);
        let laser = board.cells[c].piece == Piece::Laser;
        if lit && !laser && reach.cells[c] == 0 {
            found.push(format!("cell {c} lit but unreached"));
        }
        for s in 0..8 {
            let (x, y) = (
                (c % SIDE) as i32 + i32::from(DX[s]),
                (c / SIDE) as i32 + i32::from(DY[s]),
            );
            if !(0..SIDE as i32).contains(&x) || !(0..SIDE as i32).contains(&y) {
                continue;
            }
            let n = y as usize * SIDE + x as usize;
            if field.outbound[c][s] & field.inbound[n][s] != 0
                && reach.cells[n] > reach.cells[c] + 1
            {
                found.push(format!(
                    "{c} → {n}: {} after {}",
                    reach.cells[n], reach.cells[c]
                ));
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
        failures.join("\n")
    );
}
