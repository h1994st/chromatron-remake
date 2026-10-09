//! Every Chromatron 3 level against the tracer and win check (re/notes/chromatron3/logic.md
//! "Simulator check").

mod c3_game;

use c3_game::{C3, traced};
use lumen::{BOARD_CELLS, Board, Piece, board_index, is_won};

/// Unreachable ids 32 and 53 are unfinished drafts (hint "blah"): each leaves a pinwheel dark.
const DRAFTS: [u8; 2] = [32, 53];

#[test]
fn every_level_but_the_two_drafts_is_won_from_its_stored_placement() {
    assert_eq!(C3.levels.len(), 78);
    for level in C3.levels.iter() {
        let board = Board::from_level(level);
        let won = is_won(&board, &traced(&board));
        assert_eq!(won, !DRAFTS.contains(&level.id), "level id {}", level.id);
    }
}

#[test]
fn the_drafts_each_leave_one_pinwheel_unlit() {
    for (id, (x, y)) in [(32, (9, 5)), (53, (12, 0))] {
        let board = Board::from_level(C3.level(id).unwrap());
        assert_eq!(board.at(x, y).piece, Piece::Pinwheel, "id {id}");
        assert_eq!(traced(&board).received(board_index(x, y)), 0, "id {id}");
    }
}

#[test]
fn no_level_is_won_without_its_movable_pieces() {
    for level in C3.levels.iter() {
        let board = Board::from_level(level).without_movable();
        assert!(
            !is_won(&board, &traced(&board)),
            "level id {} is won with no pieces",
            level.id
        );
    }
}

/// The level ids whose stored placement sends light out of a `piece`.
fn ids_lighting(piece: Piece) -> Vec<u8> {
    C3.levels
        .iter()
        .filter(|level| {
            let board = Board::from_level(level);
            let field = traced(&board);
            (0..BOARD_CELLS).any(|i| board.cells[i].piece == piece && field.outbound[i] != [0; 8])
        })
        .map(|level| level.id)
        .collect()
}

/// logic.md "Simulator check": gates emit in unreachable ids 1, 2, 3, 10 and 14, starbursts are
/// hit in ids 4–9 and 26, and beams jump onto mobile teleporters in ids 16, 17, 18, 20 and 21.
#[test]
fn the_unreachable_drafts_exercise_types_16_to_18() {
    assert_eq!(ids_lighting(Piece::LogicGate), [1, 2, 3, 10, 14]);
    assert_eq!(ids_lighting(Piece::Starburst), [4, 5, 6, 7, 8, 9, 26]);
    assert_eq!(ids_lighting(Piece::MobileTeleporter), [16, 17, 18, 20, 21]);
}
