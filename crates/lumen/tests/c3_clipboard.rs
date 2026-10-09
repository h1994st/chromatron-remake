//! Chromatron 3 solutions through the clipboard text format (re/notes/chromatron3/platform.md P8).

mod c3_game;

use c3_game::{C3, traced};
use lumen::{Board, apply_solution, encode_solution, is_won, move_pieces_to_toolbox};

#[test]
fn slots_1_and_17_copy_as_the_worked_examples() {
    // Slot 17: mirror (0,7) dir 5, mirror (14,7) dir 1, dual-reflector (7,7) dir 1.
    for (slot, text) in [(0, "3-01-oCcmcO"), (16, "3-17-hODwhH")] {
        let level = C3.slot_level(slot).unwrap();
        assert_eq!(
            encode_solution(&Board::from_level(level), slot).as_deref(),
            Some(text)
        );
    }
}

#[test]
fn every_stored_solution_survives_copy_and_paste() {
    for (slot, &id) in C3.level_order.iter().enumerate() {
        let level = C3.level(id).unwrap();
        let text = encode_solution(&Board::from_level(level), slot)
            .expect("every Chromatron 3 level has < 25 pieces");
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

#[test]
fn solutions_of_other_games_are_rejected() {
    let mut board = Board::from_level(C3.slot_level(16).unwrap());
    move_pieces_to_toolbox(&mut board);
    let before = board.clone();
    for text in ["2-17-hODwhH", "4-17-hODwhH", "1-17-hODwhH"] {
        assert!(!apply_solution(&mut board, 16, text), "{text}");
        assert_eq!(board, before);
    }
}
