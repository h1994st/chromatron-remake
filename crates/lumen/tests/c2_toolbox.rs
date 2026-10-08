//! Chromatron 2 starting toolboxes (re/notes/chromatron2/data.md D3).

mod c2_game;

use c2_game::C2;
use lumen::{
    BOARD_CELLS, Board, Piece, VISIBLE_TOOLBOX_SLOTS, move_pieces_to_toolbox, toolbox_index,
};

fn start(id: u8) -> Board {
    let mut board = Board::from_level(C2.level(id).unwrap());
    move_pieces_to_toolbox(&mut board);
    board
}

#[test]
fn playable_toolboxes_fit_the_visible_slots_and_level_50_fills_them() {
    for &id in C2.level_order.iter() {
        let board = start(id);
        assert!(
            board.cells[..BOARD_CELLS]
                .iter()
                .all(|c| !c.piece.is_movable()),
            "id {id}"
        );
        let used = board.cells[toolbox_index(0)..]
            .iter()
            .rposition(|c| c.piece != Piece::Empty)
            .map_or(0, |s| s + 1);
        assert!(used <= VISIBLE_TOOLBOX_SLOTS, "id {id} uses {used} slots");
    }
    let last = start(6);
    assert!(
        last.cells[toolbox_index(0)..toolbox_index(VISIBLE_TOOLBOX_SLOTS)]
            .iter()
            .all(|c| c.piece != Piece::Empty)
    );
}

#[test]
fn twisters_stay_on_the_board_and_new_pieces_sort_last() {
    // Slot 46 (id 48) has multi-filters and complementors; slot 10 (id 10) twisters.
    let board = start(48);
    let kinds: Vec<u8> = board.cells[toolbox_index(0)..]
        .iter()
        .filter(|c| c.piece != Piece::Empty)
        .map(|c| c.piece as u8)
        .collect();
    assert!(kinds.is_sorted(), "{kinds:?}");
    assert_eq!(kinds.last(), Some(&(Piece::Complementor as u8)));
    assert!(kinds.contains(&(Piece::MultiFilter as u8)));
    let twisters = |board: &Board| {
        board.cells[..BOARD_CELLS]
            .iter()
            .filter(|c| c.piece == Piece::Twister)
            .count()
    };
    let level_10 = Board::from_level(C2.level(10).unwrap());
    assert!(twisters(&level_10) > 0);
    assert_eq!(twisters(&start(10)), twisters(&level_10));
}
