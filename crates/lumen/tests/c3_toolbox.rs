//! Chromatron 3 starting toolboxes (re/notes/chromatron3/data.md D3).

mod c3_game;

use c3_game::C3;
use lumen::{
    BOARD_CELLS, Board, Piece, VISIBLE_TOOLBOX_SLOTS, move_pieces_to_toolbox, toolbox_index,
};

fn start(id: u8) -> Board {
    let mut board = Board::from_level(C3.level(id).unwrap());
    move_pieces_to_toolbox(&mut board);
    board
}

fn toolbox_kinds(board: &Board) -> Vec<u8> {
    board.cells[toolbox_index(0)..]
        .iter()
        .filter(|c| c.piece != Piece::Empty)
        .map(|c| c.piece as u8)
        .collect()
}

#[test]
fn every_toolbox_fits_the_visible_slots_and_slots_48_and_50_fill_them() {
    for level in C3.levels.iter() {
        let board = start(level.id);
        assert!(
            board.cells[..BOARD_CELLS]
                .iter()
                .all(|c| !c.piece.is_movable()),
            "id {}",
            level.id
        );
        assert!(toolbox_kinds(&board).len() <= VISIBLE_TOOLBOX_SLOTS);
    }
    for id in [38, 57, 53] {
        assert_eq!(
            toolbox_kinds(&start(id)).len(),
            VISIBLE_TOOLBOX_SLOTS,
            "id {id}"
        );
    }
}

#[test]
fn the_new_pieces_sort_after_the_older_ones() {
    // Slot 17 (id 55): two mirrors, then the dual-reflector; unreachable id 18: mobile teleporters.
    assert_eq!(
        toolbox_kinds(&start(55)),
        [3, 3, Piece::DualReflector as u8]
    );
    for id in [1, 18, 44, 53] {
        let kinds = toolbox_kinds(&start(id));
        assert!(kinds.is_sorted(), "id {id}: {kinds:?}");
    }
    assert!(toolbox_kinds(&start(18)).contains(&(Piece::MobileTeleporter as u8)));
    assert!(toolbox_kinds(&start(1)).contains(&(Piece::LogicGate as u8)));
}

#[test]
fn starbursts_stay_on_the_board() {
    let level = C3.level(4).unwrap();
    let starbursts = |board: &Board| {
        board.cells[..BOARD_CELLS]
            .iter()
            .filter(|c| c.piece == Piece::Starburst)
            .count()
    };
    let stored = Board::from_level(level);
    assert!(starbursts(&stored) > 0);
    assert_eq!(starbursts(&start(4)), starbursts(&stored));
}
