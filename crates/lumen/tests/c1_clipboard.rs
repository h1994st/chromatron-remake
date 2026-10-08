//! Chromatron 1 solutions through the clipboard text format (platform.md P8).

mod common;

use common::{C1, solved_board, traced};
use lumen::{Board, apply_solution, encode_solution, is_won, move_pieces_to_toolbox};

#[test]
fn level_1_copies_as_the_worked_example() {
    assert_eq!(
        encode_solution(&Board::from_level(C1.level(1).unwrap()), 0).as_deref(),
        Some("1-01-hI")
    );
}

#[test]
fn every_stored_solution_survives_copy_and_paste() {
    for (slot, &id) in C1.level_order.iter().enumerate() {
        let level = C1.level(id).unwrap();
        let text =
            encode_solution(&solved_board(level), slot).expect("playable levels have < 25 pieces");
        let mut board = Board::from_level(level);
        move_pieces_to_toolbox(&mut board);
        assert!(
            apply_solution(&mut board, slot, &text),
            "slot {}: {text}",
            slot + 1
        );
        assert!(is_won(&board, &traced(&board)), "slot {}: {text}", slot + 1);
        assert_eq!(
            encode_solution(&board, slot),
            Some(text),
            "slot {}",
            slot + 1
        );
    }
}
