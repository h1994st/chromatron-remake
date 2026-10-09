//! Chromatron 4 starting toolboxes (re/notes/chromatron4/data.md D3).

mod c4_game;

use c4_game::C4;
use lumen::{
    BOARD_CELLS, Board, Piece, VISIBLE_TOOLBOX_SLOTS, move_pieces_to_toolbox, toolbox_index,
};

fn start(id: u8) -> Board {
    let mut board = Board::from_level(C4.level(id).unwrap());
    move_pieces_to_toolbox(&mut board);
    board
}

/// Toolbox slot → filter colour, for the filters in a level's starting toolbox.
fn filters(id: u8) -> Vec<(usize, u8)> {
    let board = start(id);
    (0..VISIBLE_TOOLBOX_SLOTS)
        .filter(|&s| board.cells[toolbox_index(s)].piece == Piece::Filter)
        .map(|s| (s, board.cells[toolbox_index(s)].colour))
        .collect()
}

#[test]
fn filters_of_one_level_keep_the_original_qsort_order() {
    // data.md D3: slot 7 (id 15), slot 30 (id 37) and slot 47 (id 52).
    assert_eq!(filters(15), [(0, 4), (1, 1)]);
    assert_eq!(filters(37), [(1, 1), (2, 4), (3, 2)]);
    assert_eq!(filters(52), [(5, 2), (6, 4)]);
}

#[test]
fn every_toolbox_fits_the_visible_slots() {
    let mut most = (0, 0);
    for level in C4.levels.iter() {
        let board = start(level.id);
        assert!(
            board.cells[..BOARD_CELLS]
                .iter()
                .all(|c| !c.piece.is_movable()),
            "id {}",
            level.id
        );
        let used = board.cells[toolbox_index(0)..]
            .iter()
            .filter(|c| c.piece != Piece::Empty)
            .count();
        assert!(used <= VISIBLE_TOOLBOX_SLOTS, "id {}", level.id);
        if C4.level_order.contains(&level.id) {
            most = most.max((used, level.id));
        }
    }
    assert_eq!(most, (21, 32), "slot 31 has the most pieces");
    assert_eq!(
        start(35).cells[toolbox_index(0)..toolbox_index(VISIBLE_TOOLBOX_SLOTS)]
            .iter()
            .filter(|c| c.piece != Piece::Empty)
            .count(),
        24,
        "unreachable id 35 fills the toolbox"
    );
}

#[test]
fn the_new_movable_pieces_sort_last_and_fixed_ones_stay() {
    for id in [1, 16, 27, 36, 51] {
        let board = start(id);
        let kinds: Vec<u8> = board.cells[toolbox_index(0)..]
            .iter()
            .filter(|c| c.piece != Piece::Empty)
            .map(|c| c.piece as u8)
            .collect();
        assert!(kinds.is_sorted(), "id {id}: {kinds:?}");
    }
    let count = |board: &Board, piece| {
        board.cells[..BOARD_CELLS]
            .iter()
            .filter(|c| c.piece == piece)
            .count()
    };
    for (id, piece) in [(51, Piece::OccupiedSquare), (4, Piece::Starburst)] {
        let stored = Board::from_level(C4.level(id).unwrap());
        assert!(count(&stored, piece) > 0, "id {id}");
        assert_eq!(count(&start(id), piece), count(&stored, piece), "id {id}");
    }
    assert!(
        start(16).cells[toolbox_index(0)..]
            .iter()
            .any(|c| c.piece == Piece::MobileTeleporter),
        "the mobile teleporter starts in the toolbox"
    );
}
