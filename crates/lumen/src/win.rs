//! The win condition (logic.md L5, `pinwheel_satisfied` @ 0x3ecc, `check_win` @ 0x4494).

use crate::board::{BOARD_CELLS, Board};
use crate::piece::Piece;
use crate::trace::BeamField;

/// True when every pinwheel receives exactly its colour: the OR of everything entering it
/// equals its mask, so a black (0) pinwheel must receive nothing at all.
pub fn is_won(board: &Board, field: &BeamField) -> bool {
    (0..BOARD_CELLS).all(|i| {
        let cell = board.cells[i];
        cell.piece != Piece::Pinwheel || cell.colour == field.received(i)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Cell, board_index};
    use crate::piece::{BLUE, Colour, GREEN, RED, WHITE};
    use crate::trace::trace;

    fn won(pieces: &[(usize, usize, Piece, Colour, u8)]) -> bool {
        let mut board = Board::default();
        for &(x, y, piece, colour, dir) in pieces {
            board.cells[board_index(x, y)] = Cell { piece, colour, dir };
        }
        let mut field = BeamField::default();
        trace(&board, &mut field);
        is_won(&board, &field)
    }

    #[test]
    fn a_pinwheel_needs_exactly_its_colour() {
        assert!(won(&[
            (0, 0, Piece::Laser, RED, 2),
            (5, 0, Piece::Pinwheel, RED, 0)
        ]));
        assert!(
            !won(&[
                (0, 0, Piece::Laser, WHITE, 2),
                (5, 0, Piece::Pinwheel, RED, 0)
            ]),
            "too much"
        );
        assert!(
            !won(&[
                (0, 0, Piece::Laser, RED, 2),
                (5, 0, Piece::Pinwheel, RED | GREEN, 0)
            ]),
            "too little"
        );
        assert!(
            !won(&[
                (0, 0, Piece::Laser, RED, 4),
                (5, 0, Piece::Pinwheel, RED, 0)
            ]),
            "nothing"
        );
    }

    #[test]
    fn white_can_come_from_several_beams() {
        assert!(won(&[
            (0, 5, Piece::Laser, RED | GREEN, 2),
            (5, 0, Piece::Laser, BLUE, 4),
            (5, 5, Piece::Pinwheel, WHITE, 0),
        ]));
    }

    #[test]
    fn a_black_pinwheel_must_stay_dark() {
        assert!(won(&[
            (0, 0, Piece::Laser, RED, 4),
            (5, 0, Piece::Pinwheel, 0, 0)
        ]));
        assert!(!won(&[
            (0, 0, Piece::Laser, RED, 2),
            (5, 0, Piece::Pinwheel, 0, 0)
        ]));
    }

    #[test]
    fn every_pinwheel_counts() {
        assert!(!won(&[
            (0, 0, Piece::Laser, RED, 2),
            (5, 0, Piece::Pinwheel, RED, 0),
            (5, 5, Piece::Pinwheel, BLUE, 0),
        ]));
    }
}
