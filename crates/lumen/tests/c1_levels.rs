//! Every Chromatron 1 level against the tracer and win check (logic.md "Simulator check").

mod common;

use common::{C1, MISPLACED_LEVEL, solved_board, traced};
use lumen::{Board, is_won};

#[test]
fn every_level_is_won_from_its_stored_placement() {
    for level in C1.levels.iter() {
        let board = solved_board(level);
        assert!(
            is_won(&board, &traced(&board)),
            "level id {} is not won",
            level.id
        );
    }
}

#[test]
fn level_32_is_not_won_as_stored() {
    let board = Board::from_level(C1.level(MISPLACED_LEVEL).unwrap());
    assert!(!is_won(&board, &traced(&board)));
}

/// logic.md's simulator check covers the 50 playable levels; this runs all 62 ids, and the 12
/// unreachable ones hold too.
#[test]
fn no_level_is_won_without_its_movable_pieces() {
    for level in C1.levels.iter() {
        let board = solved_board(level).without_movable();
        assert!(
            !is_won(&board, &traced(&board)),
            "level id {} is won with no pieces",
            level.id
        );
    }
}
