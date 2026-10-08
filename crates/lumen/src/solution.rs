//! The clipboard solution text, "Copy Level" / "Paste Level" (platform.md P8, `encode_solution`
//! C1 @ 0x5914, C2 @ 0x5c2c; `decode_solution` C1 @ 0x5ad4, C2 @ 0x5dec). Text only: the
//! pasteboard is the shell's job. Only the game digit differs between games.

use std::cmp::Ordering;

use crate::board::{BOARD_CELLS, Board, CELLS, Cell, VISIBLE_TOOLBOX_SLOTS};
use crate::bsd_qsort::bsd_qsort;
use crate::piece::Piece;

/// Copy gives up at this many pieces (`2·n + 6 >= 55`).
const MAX_COPIED_PIECES: usize = 25;

/// One movable piece as `encode_solution` collects it: {cell index, dir, type, colour}.
#[derive(Clone, Copy, Debug)]
struct Entry {
    cell: usize,
    dir: u8,
    piece: Piece,
    colour: u8,
}

/// Every movable piece in cell order, sorted with the original's qsort by (type, colour).
fn sorted_pieces(board: &Board) -> Vec<Entry> {
    let mut pieces: Vec<Entry> = (0..CELLS)
        .filter(|&i| board.cells[i].piece.is_movable())
        .map(|i| Entry {
            cell: i,
            dir: board.cells[i].dir,
            piece: board.cells[i].piece,
            colour: board.cells[i].colour,
        })
        .collect();
    bsd_qsort(&mut pieces, &mut by_kind_then_colour);
    pieces
}

/// The comparator at 0x58a0: type, then colour, both unsigned.
fn by_kind_then_colour(a: &Entry, b: &Entry) -> Ordering {
    (a.piece as u8, a.colour).cmp(&(b.piece as u8, b.colour))
}

/// `base52_char` @ 0x58e4: 0–25 → 'a'–'z', 26–51 → 'A'–'Z'.
fn base52_char(d: usize) -> char {
    let d = d as u8;
    char::from(if d < 26 { b'a' + d } else { b'A' + d - 26 })
}

fn base52_value(c: u8) -> Option<usize> {
    match c {
        b'a'..=b'z' => Some(usize::from(c - b'a')),
        b'A'..=b'Z' => Some(usize::from(c - b'A') + 26),
        _ => None,
    }
}

/// "G-NN-" (G = the board's game digit) plus two letters per movable piece (board and toolbox),
/// or `None` with 25 or more pieces, when the original leaves the clipboard alone. `slot` is
/// 0-based.
///
/// Boards restored from original saves may hold dir 8 or 9, for which the copied text is not
/// pasteable (faithful to the original).
pub fn encode_solution(board: &Board, slot: usize) -> Option<String> {
    let pieces = sorted_pieces(board);
    if pieces.len() >= MAX_COPIED_PIECES {
        return None;
    }
    let digit = board.rules().clipboard_digit;
    let mut text = format!("{}-{:02}-", char::from(digit), slot + 1);
    for p in &pieces {
        let v = p.cell + CELLS * usize::from(p.dir);
        text.push(base52_char(v / 52));
        text.push(base52_char(v % 52));
    }
    Some(text)
}

/// Pastes `text` onto `board` for strip slot `slot` (0-based); it must start with the board's
/// game digit. Returns false, leaving the board
/// untouched, for anything the original rejects, and also for what it lets through but a port
/// should reject: non-letters, a dir above 7, two pieces on one cell, or a toolbox slot past 23.
pub fn apply_solution(board: &mut Board, slot: usize, text: &str) -> bool {
    let mut s = text.as_bytes();
    while let [b' ', rest @ ..] = s {
        s = rest;
    }
    let [
        game,
        b'-',
        tens @ b'0'..=b'9',
        ones @ b'0'..=b'9',
        b'-',
        rest @ ..,
    ] = s
    else {
        return false;
    };
    if *game != board.rules().clipboard_digit
        || usize::from((tens - b'0') * 10 + (ones - b'0')) != slot + 1
    {
        return false;
    }
    if rest.first() == Some(&b' ') {
        return false;
    }
    let mut pairs = rest;
    while let [head @ .., b' '] = pairs {
        pairs = head;
    }
    let pieces = sorted_pieces(board);
    if pairs.len() != 2 * pieces.len() {
        return false;
    }
    let mut targets: Vec<(usize, u8)> = Vec::with_capacity(pieces.len());
    for pair in pairs.as_chunks::<2>().0 {
        let (Some(high), Some(low)) = (base52_value(pair[0]), base52_value(pair[1])) else {
            return false;
        };
        let v = high * 52 + low;
        let (cell, dir) = (v % CELLS, v / CELLS);
        let target = board.cells[cell].piece;
        let fixed = target != Piece::Empty && !target.is_movable();
        let hidden = cell >= BOARD_CELLS + VISIBLE_TOOLBOX_SLOTS;
        if fixed || hidden || dir > 7 || targets.iter().any(|&(c, _)| c == cell) {
            return false;
        }
        targets.push((cell, dir as u8));
    }
    for p in &pieces {
        board.cells[p.cell].piece = Piece::Empty;
    }
    for (p, &(cell, dir)) in pieces.iter().zip(&targets) {
        board.cells[cell] = Cell {
            piece: p.piece,
            colour: p.colour,
            dir,
        };
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{board_index, toolbox_index};
    use crate::piece::{BLUE, RED};
    use spectrograph::Game;

    fn level_1_solved() -> Board {
        let mut board = Board::default();
        board.cells[board_index(8, 2)] = Cell {
            piece: Piece::Pinwheel,
            colour: RED,
            dir: 6,
        };
        board.cells[board_index(2, 7)] = Cell {
            piece: Piece::Laser,
            colour: RED,
            dir: 2,
        };
        board.cells[board_index(8, 7)] = Cell {
            piece: Piece::Mirror,
            colour: RED,
            dir: 1,
        };
        board
    }

    fn level_1_start() -> Board {
        let mut board = level_1_solved();
        board.cells[toolbox_index(0)] = Cell {
            dir: 0,
            ..board.cells[board_index(8, 7)]
        };
        board.cells[board_index(8, 7)].piece = Piece::Empty;
        board
    }

    #[test]
    fn copies_the_worked_example() {
        assert_eq!(
            encode_solution(&level_1_solved(), 0).as_deref(),
            Some("1-01-hI")
        );
        assert_eq!(
            encode_solution(&level_1_start(), 0).as_deref(),
            Some("1-01-er"),
            "toolbox slot 0 is cell 225"
        );
    }

    #[test]
    fn the_worked_example_round_trips() {
        let mut board = level_1_start();
        assert!(apply_solution(&mut board, 0, "1-01-hI"));
        assert_eq!(
            board.at(8, 7),
            Cell {
                piece: Piece::Mirror,
                colour: RED,
                dir: 1
            }
        );
        assert_eq!(board.cells[toolbox_index(0)].piece, Piece::Empty);
        assert_eq!(encode_solution(&board, 0).as_deref(), Some("1-01-hI"));
    }

    #[test]
    fn spaces_around_the_text_are_allowed() {
        let mut board = level_1_start();
        assert!(apply_solution(&mut board, 0, "  1-01-hI  "));
        assert_eq!(board, level_1_solved_with_toolbox_noise());
    }

    /// Pasting clears only the type byte of the old cell; colour and dir stay (platform.md P8).
    fn level_1_solved_with_toolbox_noise() -> Board {
        let mut board = level_1_solved();
        board.cells[toolbox_index(0)] = Cell {
            piece: Piece::Empty,
            colour: RED,
            dir: 0,
        };
        board
    }

    #[test]
    fn rejects_without_touching_the_board() {
        for text in [
            "1-02-hI",   // another level
            "2-01-hI",   // another game
            "1-01- hI",  // space right after the prefix
            "1-01-hI\n", // a trailing newline is not stripped
            "1-01-h",    // too short
            "1-01-hIhI", // too long
            "1-01-h1",   // not a letter
            "1-01-ZZ",   // v = 2703: dir 9
            "1-01-eV",   // v = 255: toolbox slot 30, never drawn
            "1-01-cd",   // v = 107: the laser at (2,7)
            "1-01-aM",   // v = 38: the pinwheel at (8,2)
            "",
        ] {
            let mut board = level_1_start();
            assert!(!apply_solution(&mut board, 0, text), "{text:?}");
            assert_eq!(board, level_1_start(), "{text:?}");
        }
    }

    #[test]
    fn any_empty_cell_is_a_valid_target() {
        let mut board = level_1_start();
        assert!(apply_solution(&mut board, 0, "1-01-aa"));
        assert_eq!(board.at(0, 0).piece, Piece::Mirror);
    }

    #[test]
    fn two_pieces_cannot_share_a_cell() {
        let mut board = Board::default();
        board.cells[toolbox_index(0)] = Cell {
            piece: Piece::Mirror,
            colour: 0,
            dir: 0,
        };
        board.cells[toolbox_index(1)] = Cell {
            piece: Piece::Mirror,
            colour: 0,
            dir: 0,
        };
        assert!(!apply_solution(&mut board, 4, "1-05-aaaa"));
        assert!(apply_solution(&mut board, 4, "1-05-aaab"));
    }

    #[test]
    fn pairs_are_matched_to_pieces_by_type_then_colour() {
        let mut board = Board::default();
        board.cells[toolbox_index(0)] = Cell {
            piece: Piece::Filter,
            colour: RED,
            dir: 0,
        };
        board.cells[toolbox_index(1)] = Cell {
            piece: Piece::Mirror,
            colour: 0,
            dir: 0,
        };
        board.cells[toolbox_index(2)] = Cell {
            piece: Piece::Filter,
            colour: BLUE,
            dir: 0,
        };
        // Sorted: mirror, blue filter, red filter → cells 0, 1, 2.
        assert!(apply_solution(&mut board, 0, "1-01-aaabac"));
        assert_eq!(board.at(0, 0).piece, Piece::Mirror);
        assert_eq!(
            board.at(1, 0),
            Cell {
                piece: Piece::Filter,
                colour: BLUE,
                dir: 0
            }
        );
        assert_eq!(
            board.at(2, 0),
            Cell {
                piece: Piece::Filter,
                colour: RED,
                dir: 0
            }
        );
    }

    #[test]
    fn copy_gives_up_at_25_pieces() {
        let mut board = Board::default();
        for slot in 0..24 {
            board.cells[toolbox_index(slot)] = Cell {
                piece: Piece::Mirror,
                colour: 0,
                dir: 0,
            };
        }
        assert!(encode_solution(&board, 0).is_some());
        board.cells[0] = Cell {
            piece: Piece::Mirror,
            colour: 0,
            dir: 0,
        };
        assert_eq!(encode_solution(&board, 0), None);
    }

    #[test]
    fn a_chromatron_2_board_copies_and_pastes_with_its_own_digit() {
        let mut solved = level_1_solved();
        solved.game = Game::C2;
        assert_eq!(encode_solution(&solved, 0).as_deref(), Some("2-01-hI"));
        let mut start = level_1_start();
        start.game = Game::C2;
        let before = start.clone();
        assert!(!apply_solution(&mut start, 0, "1-01-hI"), "C1's text");
        assert_eq!(start, before);
        assert!(apply_solution(&mut start, 0, "2-01-hI"));
        assert_eq!(start.at(8, 7).piece, Piece::Mirror);
    }
}
