//! Every Chromatron 2 level against the tracer and win check (re/notes/chromatron2/logic.md
//! "Simulator check").

mod c2_game;

use c2_game::{C2, traced};
use lumen::{Board, Piece, board_index, is_won};

/// Unlike Chromatron 1, every stored placement wins, the two unreachable ids (24, 52) included.
#[test]
fn every_level_is_won_from_its_stored_placement() {
    assert_eq!(C2.levels.len(), 52);
    for level in C2.levels.iter() {
        let board = Board::from_level(level);
        assert!(
            is_won(&board, &traced(&board)),
            "level id {} is not won",
            level.id
        );
    }
}

#[test]
fn no_level_is_won_without_its_movable_pieces() {
    for level in C2.levels.iter() {
        let board = Board::from_level(level).without_movable();
        assert!(
            !is_won(&board, &traced(&board)),
            "level id {} is won with no pieces",
            level.id
        );
    }
}

/// logic.md L4: in slot 49 (id 37, "Feedback Eliminator") the complementor at (7,7) gets red
/// heading E, its cyan comes back into the same input, and it shuts down: the intended solution.
#[test]
fn the_feedback_eliminator_wins_by_shutting_its_complementor_down() {
    let level = C2.slot_level(48).unwrap();
    assert_eq!(level.id, 37);
    let board = Board::from_level(level);
    let complementor = board_index(7, 7);
    assert_eq!(board.cells[complementor].piece, Piece::Complementor);
    let field = traced(&board);
    assert!(is_won(&board, &field));
    assert_ne!(
        field.inbound[complementor][2], 0,
        "it receives light heading E"
    );
    assert_eq!(field.outbound[complementor], [0; 8], "and sends none on");
}
