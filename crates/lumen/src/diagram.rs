//! A text picture of a traced board, for snapshot tests and debugging.

use spectrograph::{colour_symbol, kind_symbol};

use crate::board::{BOARD_CELLS, Board, SIDE, TOOLBOX_SLOTS, toolbox_index};
use crate::piece::{DX, DY, Piece};
use crate::trace::BeamField;

/// The board as 45 lines of 45 characters, a 3 × 3 block per cell. The centre is the cell's kind
/// (`spectrograph::kind_symbol`, or `O` for a satisfied pinwheel); each of the 8 neighbours is the
/// colour drawn on that side's half-segment (`out[s] | in[s + 4]`, as `colour_symbol`), blank for
/// none. A last line lists the toolbox: kind, filter colour, dir.
pub fn beam_diagram(board: &Board, field: &BeamField) -> String {
    let mut grid = vec![[b' '; SIDE * 3]; SIDE * 3];
    for index in 0..BOARD_CELLS {
        let (x, y) = (index % SIDE * 3 + 1, index / SIDE * 3 + 1);
        let cell = board.cells[index];
        let lit = cell.piece == Piece::Pinwheel && cell.colour == field.received(index);
        grid[y][x] = if lit {
            b'O'
        } else {
            kind_symbol(cell.piece as u8) as u8
        };
        for s in 0..8 {
            let colour = field.outbound[index][s] | field.inbound[index][(s + 4) & 7];
            if colour != 0 {
                let (sx, sy) = (
                    x as isize + isize::from(DX[s]),
                    y as isize + isize::from(DY[s]),
                );
                grid[sy as usize][sx as usize] = colour_symbol(colour) as u8;
            }
        }
    }
    let mut out = String::new();
    for row in &grid {
        out.push_str(std::str::from_utf8(row).expect("ASCII").trim_end());
        out.push('\n');
    }
    out.push_str("toolbox:");
    for slot in 0..TOOLBOX_SLOTS {
        let cell = board.cells[toolbox_index(slot)];
        if cell.piece != Piece::Empty {
            out.push(' ');
            out.push(kind_symbol(cell.piece as u8));
            if cell.piece == Piece::Filter {
                out.push(colour_symbol(cell.colour));
            }
            out.push(char::from(b'0' + cell.dir));
        }
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Cell, board_index};
    use crate::piece::{BLUE, RED};
    use crate::trace::trace;

    #[test]
    fn draws_kinds_beams_and_the_toolbox() {
        let mut board = Board::default();
        board.cells[board_index(0, 0)] = Cell {
            piece: Piece::Laser,
            colour: RED,
            dir: 2,
        };
        board.cells[board_index(2, 0)] = Cell {
            piece: Piece::Pinwheel,
            colour: RED,
            dir: 0,
        };
        board.cells[toolbox_index(0)] = Cell {
            piece: Piece::Filter,
            colour: BLUE,
            dir: 0,
        };
        board.cells[toolbox_index(1)] = Cell {
            piece: Piece::Mirror,
            colour: 4,
            dir: 3,
        };
        let mut field = BeamField::default();
        trace(&board, &mut field);
        let diagram = beam_diagram(&board, &field);
        let lines: Vec<&str> = diagram.lines().collect();
        assert_eq!(lines.len(), 46);
        assert_eq!(lines[0], "");
        assert!(lines[1].starts_with(" *rr.rrOrr.rr"), "{:?}", lines[1]);
        assert_eq!(lines[4], " .  .  .  .  .  .  .  .  .  .  .  .  .  .  .");
        assert_eq!(lines[45], "toolbox: Fb0 M3");
    }
}
