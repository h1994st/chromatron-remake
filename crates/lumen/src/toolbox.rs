//! Moving a level's pieces into the toolbox at level start (`move_pieces_to_toolbox` @ 0x54f8,
//! data.md D3).

use std::cmp::Ordering;

use crate::board::{BOARD_CELLS, Board, Cell, TOOLBOX_SLOTS, toolbox_index};
use crate::bsd_qsort::bsd_qsort;
use crate::piece::Piece;

/// Moves every movable board piece, in row-major order, into the first free toolbox slot with
/// `dir = 0`, leaving the board cell's colour and dir bytes behind. Then sorts the filled slots by
/// kind with the original's `qsort`. A level with more pieces than slots (only the unreachable ids
/// 50 and 51) keeps the rest on the board instead of overrunning memory as the original does.
pub fn move_pieces_to_toolbox(board: &mut Board) {
    let mut next = 0;
    for index in 0..BOARD_CELLS {
        if !board.cells[index].piece.is_movable() {
            continue;
        }
        while next < TOOLBOX_SLOTS && board.cells[toolbox_index(next)].piece != Piece::Empty {
            next += 1;
        }
        if next == TOOLBOX_SLOTS {
            break;
        }
        board.cells[toolbox_index(next)] = Cell {
            dir: 0,
            ..board.cells[index]
        };
        board.cells[index].piece = Piece::Empty;
    }
    let filled = (next + 1).min(TOOLBOX_SLOTS);
    bsd_qsort(
        &mut board.cells[toolbox_index(0)..toolbox_index(filled)],
        &mut by_kind,
    );
}

/// The comparator at 0x54e8: `a.type − b.type`, nothing else.
fn by_kind(a: &Cell, b: &Cell) -> Ordering {
    (a.piece as u8).cmp(&(b.piece as u8))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board_index;
    use crate::piece::{BLUE, GREEN, RED};

    fn place(board: &mut Board, x: usize, y: usize, piece: Piece, colour: u8, dir: u8) {
        board.cells[board_index(x, y)] = Cell { piece, colour, dir };
    }

    #[test]
    fn pieces_move_in_row_major_order_and_are_sorted_by_kind() {
        let mut board = Board::default();
        place(&mut board, 5, 0, Piece::Splitter, 4, 3);
        place(&mut board, 1, 1, Piece::Mirror, 4, 1);
        place(&mut board, 0, 2, Piece::Bender, 2, 5);
        place(&mut board, 3, 3, Piece::Pinwheel, RED, 0);
        move_pieces_to_toolbox(&mut board);
        let toolbox: Vec<Cell> = board.cells[toolbox_index(0)..toolbox_index(4)].to_vec();
        assert_eq!(
            toolbox,
            [
                Cell {
                    piece: Piece::Mirror,
                    colour: 4,
                    dir: 0
                },
                Cell {
                    piece: Piece::Bender,
                    colour: 2,
                    dir: 0
                },
                Cell {
                    piece: Piece::Splitter,
                    colour: 4,
                    dir: 0
                },
                Cell::default(),
            ]
        );
        assert_eq!(board.at(3, 3).piece, Piece::Pinwheel, "fixed pieces stay");
        assert_eq!(
            board.at(5, 0),
            Cell {
                piece: Piece::Empty,
                colour: 4,
                dir: 3
            },
            "colour and dir bytes stay"
        );
    }

    #[test]
    fn equal_kinds_keep_the_bsd_qsort_order() {
        // Seven filters (red, then blue/green alternating) and a mirror. By hand: med3 picks slot 4
        // as the pivot and swaps it to the front, every filter compares equal to it, and the
        // final vecswap trades slot 0 with slot 7. A stable sort would keep red first.
        let mut board = Board::default();
        for (x, colour) in [RED, BLUE, GREEN, BLUE, GREEN, BLUE, GREEN]
            .into_iter()
            .enumerate()
        {
            place(&mut board, x, 0, Piece::Filter, colour, 0);
        }
        place(&mut board, 7, 0, Piece::Mirror, 0, 0);
        move_pieces_to_toolbox(&mut board);
        let colours: Vec<u8> = (0..8)
            .map(|s| board.cells[toolbox_index(s)].colour)
            .collect();
        assert_eq!(board.cells[toolbox_index(0)].piece, Piece::Mirror);
        assert_eq!(colours[1..], [BLUE, GREEN, BLUE, RED, BLUE, GREEN, GREEN]);
    }

    #[test]
    fn a_level_with_no_pieces_leaves_the_toolbox_empty() {
        let mut board = Board::default();
        place(&mut board, 0, 0, Piece::Wall, 0, 0);
        move_pieces_to_toolbox(&mut board);
        assert!(
            board.cells[BOARD_CELLS..]
                .iter()
                .all(|c| *c == Cell::default())
        );
    }

    #[test]
    fn overflowing_pieces_stay_on_the_board() {
        let mut board = Board::default();
        for i in 0..70 {
            place(&mut board, i % 15, i / 15, Piece::Mirror, 0, 1);
        }
        move_pieces_to_toolbox(&mut board);
        assert_eq!(
            board.cells[BOARD_CELLS..]
                .iter()
                .filter(|c| c.piece == Piece::Mirror)
                .count(),
            TOOLBOX_SLOTS
        );
        assert_eq!(
            board.cells[..BOARD_CELLS]
                .iter()
                .filter(|c| c.piece == Piece::Mirror)
                .count(),
            10
        );
    }
}
