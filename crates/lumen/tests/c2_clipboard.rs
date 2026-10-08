//! Chromatron 2 solutions through the clipboard text format (re/notes/chromatron2/platform.md P8).

mod c2_game;

use c2_game::{C2, traced};
use lumen::{Board, apply_solution, encode_solution, is_won, move_pieces_to_toolbox};

#[test]
fn slot_7_copies_as_the_worked_example() {
    // Mirror (11,7) dir 7, bender (7,1) dir 6, multi-filter (7,7) dir 4.
    let level = C2.slot_level(6).unwrap();
    assert_eq!(level.id, 41);
    assert_eq!(
        encode_solution(&Board::from_level(level), 6).as_deref(),
        Some("2-07-OFHqye")
    );
}

#[test]
fn every_stored_solution_survives_copy_and_paste() {
    for (slot, &id) in C2.level_order.iter().enumerate() {
        let level = C2.level(id).unwrap();
        let text = encode_solution(&Board::from_level(level), slot)
            .expect("every Chromatron 2 level has < 25 pieces");
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
fn a_chromatron_1_solution_is_rejected() {
    let mut board = Board::from_level(C2.slot_level(6).unwrap());
    move_pieces_to_toolbox(&mut board);
    let before = board.clone();
    assert!(!apply_solution(&mut board, 6, "1-07-OFHqye"));
    assert_eq!(board, before);
}
