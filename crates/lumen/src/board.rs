//! The board: 15 × 15 cells plus a 60-slot toolbox, as in the original's `board` (data.md D2).

use spectrograph::{Game, Level};

use crate::piece::{Colour, Dir, Piece};
use crate::rule_set::RuleSet;

pub const SIDE: usize = 15;
pub const BOARD_CELLS: usize = SIDE * SIDE;
/// Toolbox slots that exist (board rows 15–18).
pub const TOOLBOX_SLOTS: usize = 60;
/// Toolbox slots that are drawn and can be clicked (6 × 4).
pub const VISIBLE_TOOLBOX_SLOTS: usize = 24;
/// Every cell: board cells 0–224 row-major, then toolbox slot s at 225 + s.
pub const CELLS: usize = BOARD_CELLS + TOOLBOX_SLOTS;

/// The three bytes of a cell the game keeps: `type`, `colour`, `dir`. Bytes that a kind ignores
/// are kept as-is, because they are copied into the save file (data.md D2 "data noise").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub piece: Piece,
    pub colour: Colour,
    pub dir: Dir,
}

/// A board of one game: the game decides what its cell bytes mean and how beams behave
/// (`Board::rules`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    pub cells: [Cell; CELLS],
    pub game: Game,
}

/// An empty Chromatron 1 board.
impl Default for Board {
    fn default() -> Board {
        Board::new(Game::C1)
    }
}

/// Cell index of board position (x, y).
pub fn board_index(x: usize, y: usize) -> usize {
    y * SIDE + x
}

/// Cell index of toolbox slot `slot`.
pub fn toolbox_index(slot: usize) -> usize {
    BOARD_CELLS + slot
}

impl Board {
    /// An empty board of `game`.
    pub fn new(game: Game) -> Board {
        Board {
            cells: [Cell::default(); CELLS],
            game,
        }
    }

    /// The rules of the board's game.
    pub fn rules(&self) -> &'static RuleSet {
        RuleSet::of(self.game)
    }

    /// `load_level_data` @ 0x69fc: an empty board of the level's game with every record placed,
    /// movable pieces at their stored (solution) positions.
    pub fn from_level(level: &Level) -> Board {
        let mut board = Board::new(level.game);
        let rules = board.rules();
        for r in level.records.iter() {
            let piece = rules
                .piece(r.kind)
                .expect("spectrograph rejects kinds the game doesn't have");
            board.cells[board_index(usize::from(r.x), usize::from(r.y))] = Cell {
                piece,
                colour: r.colour,
                dir: r.dir,
            };
        }
        board
    }

    pub fn at(&self, x: usize, y: usize) -> Cell {
        self.cells[board_index(x, y)]
    }

    /// Movable pieces anywhere (board and toolbox).
    pub fn movable_count(&self) -> usize {
        self.cells.iter().filter(|c| c.piece.is_movable()).count()
    }

    /// The same board with every movable piece taken off (toolbox included).
    pub fn without_movable(&self) -> Board {
        let mut board = self.clone();
        for cell in board.cells.iter_mut().filter(|c| c.piece.is_movable()) {
            *cell = Cell::default();
        }
        board
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectrograph::{Game, Record};
    use std::borrow::Cow;

    fn level(records: &[Record]) -> Level {
        Level {
            game: Game::C1,
            id: 1,
            records: Cow::Owned(records.to_vec()),
            name: Cow::Borrowed(""),
            hint: Cow::Borrowed(""),
        }
    }

    #[test]
    fn records_land_on_their_cells_with_raw_bytes() {
        let board = Board::from_level(&level(&[
            Record {
                kind: 2,
                dir: 2,
                colour: 4,
                x: 2,
                y: 7,
            },
            Record {
                kind: 1,
                dir: 3,
                colour: 5,
                x: 14,
                y: 14,
            },
        ]));
        assert_eq!(
            board.at(2, 7),
            Cell {
                piece: Piece::Laser,
                colour: 4,
                dir: 2
            }
        );
        assert_eq!(
            board.at(14, 14),
            Cell {
                piece: Piece::Wall,
                colour: 5,
                dir: 3
            }
        );
        assert_eq!(
            board
                .cells
                .iter()
                .filter(|c| c.piece != Piece::Empty)
                .count(),
            2
        );
    }

    #[test]
    fn a_board_belongs_to_its_levels_game() {
        assert_eq!(Board::default().game, Game::C1);
        assert_eq!(Board::default().rules(), &RuleSet::C1);
        let mut c2 = level(&[Record {
            kind: 3,
            dir: 1,
            colour: 4,
            x: 3,
            y: 4,
        }]);
        c2.game = Game::C2;
        let board = Board::from_level(&c2);
        assert_eq!(board.game, Game::C2);
        assert_eq!(board.rules(), &RuleSet::C2);
        assert_eq!(board.at(3, 4).piece, Piece::Mirror);
        assert_eq!(Board::new(Game::C2).cells, Board::default().cells);
    }

    #[test]
    fn indices_put_the_toolbox_after_the_board() {
        assert_eq!(board_index(0, 0), 0);
        assert_eq!(board_index(14, 14), 224);
        assert_eq!(toolbox_index(0), 225);
        assert_eq!(toolbox_index(59), CELLS - 1);
    }

    #[test]
    fn movable_pieces_can_be_counted_and_removed() {
        let board = Board::from_level(&level(&[
            Record {
                kind: 3,
                dir: 1,
                colour: 4,
                x: 8,
                y: 7,
            },
            Record {
                kind: 10,
                dir: 6,
                colour: 4,
                x: 8,
                y: 2,
            },
        ]));
        assert_eq!(board.movable_count(), 1);
        let bare = board.without_movable();
        assert_eq!(bare.movable_count(), 0);
        assert_eq!(bare.at(8, 2).piece, Piece::Pinwheel);
    }
}
