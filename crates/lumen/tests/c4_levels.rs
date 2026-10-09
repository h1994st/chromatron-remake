//! Every Chromatron 4 level against the tracer and win check (re/notes/chromatron4/logic.md
//! "Simulator check").

mod c4_game;

use c4_game::{C4, traced};
use lumen::{BOARD_CELLS, Board, Piece, board_index, is_won};

/// All 54 stored placements win, the unreachable ids 5, 11, 35 and 46 included.
#[test]
fn every_level_is_won_from_its_stored_placement() {
    assert_eq!(C4.levels.len(), 54);
    for level in C4.levels.iter() {
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
    for level in C4.levels.iter() {
        let board = Board::from_level(level).without_movable();
        assert!(
            !is_won(&board, &traced(&board)),
            "level id {} is won with no pieces",
            level.id
        );
    }
}

/// The strip slots (1-based) whose stored placement sends light out of a `piece`.
fn slots_lighting(piece: Piece) -> Vec<usize> {
    (0..C4.level_order.len())
        .filter(|&slot| {
            let board = Board::from_level(C4.slot_level(slot).unwrap());
            let field = traced(&board);
            (0..BOARD_CELLS).any(|i| board.cells[i].piece == piece && field.outbound[i] != [0; 8])
        })
        .map(|slot| slot + 1)
        .collect()
}

/// logic.md "Simulator check": logic gates emit in slots 4, 5, 6, 15 and 50, primary logic gates
/// in slots 41 and 42.
#[test]
fn the_gates_emit_where_the_notes_say() {
    assert_eq!(slots_lighting(Piece::LogicGate), [4, 5, 6, 15, 50]);
    assert_eq!(slots_lighting(Piece::PrimaryLogicGate), [41, 42]);
}

/// logic.md L4: in slot 15 (id 29, "Color Challenge I") the complementor at (8,6) feeds the gate
/// at (8,10) in the second round, which only the second gate pass of a round sees.
#[test]
fn color_challenge_i_needs_the_second_gate_pass() {
    let level = C4.slot_level(14).unwrap();
    assert_eq!((level.id, level.name.as_ref()), (29, "Color Challenge I"));
    let board = Board::from_level(level);
    assert_eq!(board.at(8, 6).piece, Piece::Complementor);
    assert_eq!(board.at(8, 10).piece, Piece::LogicGate);
    let field = traced(&board);
    assert!(is_won(&board, &field));
    assert_ne!(field.outbound[board_index(8, 6)], [0; 8]);
    assert_ne!(field.outbound[board_index(8, 10)], [0; 8]);
}

/// logic.md "Simulator check": light crosses occupied squares in slots 26, 32 and 37, which
/// need it to pass (an absorbing occupied square fails them).
#[test]
fn light_passes_occupied_squares() {
    for slot in [26, 32, 37] {
        let board = Board::from_level(C4.slot_level(slot - 1).unwrap());
        let field = traced(&board);
        let crossed = (0..BOARD_CELLS)
            .any(|i| board.cells[i].piece == Piece::OccupiedSquare && field.outbound[i] != [0; 8]);
        assert!(crossed, "slot {slot}");
    }
}
