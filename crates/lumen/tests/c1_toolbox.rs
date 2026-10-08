//! Chromatron 1 starting toolboxes (data.md D3).

mod common;

use common::C1;
use lumen::{
    BOARD_CELLS, Board, Piece, VISIBLE_TOOLBOX_SLOTS, move_pieces_to_toolbox, toolbox_index,
};

/// Filter colours in the starting toolbox of a level, in slot order.
fn toolbox_filters(id: u8) -> Vec<u8> {
    let mut board = Board::from_level(C1.level(id).unwrap());
    move_pieces_to_toolbox(&mut board);
    board.cells[toolbox_index(0)..]
        .iter()
        .filter(|c| c.piece == Piece::Filter)
        .map(|c| c.colour)
        .collect()
}

#[test]
fn filter_order_follows_the_original_qsort() {
    // data.md D3: BSD qsort gives blue, green, red for slot 15 (id 5) and blue, red, green for
    // slot 17 (id 16); a stable sort would give red, blue, green for both.
    assert_eq!(toolbox_filters(5), [1, 2, 4]);
    assert_eq!(toolbox_filters(16), [1, 4, 2]);
}

#[test]
fn playable_toolboxes_fit_the_visible_slots() {
    for &id in C1.level_order.iter() {
        let mut board = Board::from_level(C1.level(id).unwrap());
        move_pieces_to_toolbox(&mut board);
        assert_eq!(
            board.cells[..BOARD_CELLS]
                .iter()
                .filter(|c| c.piece.is_movable())
                .count(),
            0,
            "id {id}"
        );
        let used = board.cells[toolbox_index(0)..]
            .iter()
            .rposition(|c| c.piece != Piece::Empty)
            .map_or(0, |s| s + 1);
        assert!(used <= VISIBLE_TOOLBOX_SLOTS, "id {id} uses {used} slots");
    }
}
