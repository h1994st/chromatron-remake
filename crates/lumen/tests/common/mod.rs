//! Shared helpers for tests that need the original Chromatron 1 data.

#![allow(dead_code)]

use std::sync::LazyLock;

use lumen::{BeamField, Board, Cell, Piece, board_index, trace};
use spectrograph::{Game, GameData, Level};

/// Chromatron 1's data, decoded once per test binary from `chromatron_mac/`.
pub static C1: LazyLock<GameData> =
    LazyLock::new(|| spectrograph::decode(Game::C1, &Game::C1.read_original().unwrap()).unwrap());

/// Level id 32 (slot 16) stores its mirror at (12,2) dir 6, but is solved with it at (12,4)
/// dir 0 (data.md D3, logic.md "Simulator check").
pub const MISPLACED_LEVEL: u8 = 32;

/// A level with every movable piece at its stored position, corrected for level 32.
pub fn solved_board(level: &Level) -> Board {
    let mut board = Board::from_level(level);
    if level.id == MISPLACED_LEVEL {
        let mirror = board.cells[board_index(12, 2)];
        assert_eq!(
            mirror,
            Cell {
                piece: Piece::Mirror,
                colour: mirror.colour,
                dir: 6
            }
        );
        board.cells[board_index(12, 2)] = Cell::default();
        board.cells[board_index(12, 4)] = Cell { dir: 0, ..mirror };
    }
    board
}

pub fn traced(board: &Board) -> BeamField {
    let mut field = BeamField::default();
    trace(board, &mut field);
    field
}
