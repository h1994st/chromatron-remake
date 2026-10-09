//! Chromatron 4 solutions through the clipboard text format (re/notes/chromatron4/platform.md P8).

mod c4_game;

use c4_game::{C4, traced};
use lumen::{
    BOARD_CELLS, Board, Piece, apply_solution, encode_solution, is_won, move_pieces_to_toolbox,
};

#[test]
fn slot_29_copies_as_the_worked_example() {
    // Pot Luck: mirror (7,1) dir 5, doppler (6,7) dir 7.
    let level = C4.slot_level(28).unwrap();
    assert_eq!((level.id, level.name.as_ref()), (8, "Pot Luck"));
    assert_eq!(
        encode_solution(&Board::from_level(level), 28).as_deref(),
        Some("4-29-BROA")
    );
}

#[test]
fn every_stored_solution_survives_copy_and_paste() {
    for (slot, &id) in C4.level_order.iter().enumerate() {
        let level = C4.level(id).unwrap();
        let text = encode_solution(&Board::from_level(level), slot)
            .expect("every Chromatron 4 level has < 25 pieces");
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
    let mut board = Board::from_level(C4.slot_level(28).unwrap());
    move_pieces_to_toolbox(&mut board);
    let before = board.clone();
    for text in ["3-29-BROA", "2-29-BROA", "1-29-BROA"] {
        assert!(!apply_solution(&mut board, 28, text), "{text}");
        assert_eq!(board, before);
    }
}

#[test]
fn nothing_can_be_pasted_onto_an_occupied_square() {
    // Slot 26 (id 51) introduces the occupied square. Point its first copied piece (the first
    // letter pair) at one: the paste is refused and the board is left alone.
    let slot = 25;
    let solved = Board::from_level(C4.slot_level(slot).unwrap());
    let occupied = (0..BOARD_CELLS)
        .find(|&i| solved.cells[i].piece == Piece::OccupiedSquare)
        .unwrap();
    let base52 = |d: usize| (b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ")[d] as char;
    let text = encode_solution(&solved, slot).unwrap();
    let bad = format!(
        "{}{}{}{}",
        &text[..5],
        base52(occupied / 52),
        base52(occupied % 52),
        &text[7..]
    );
    let mut board = solved;
    move_pieces_to_toolbox(&mut board);
    let before = board.clone();
    assert!(!apply_solution(&mut board, slot, &bad), "{bad}");
    assert_eq!(board, before);
}
