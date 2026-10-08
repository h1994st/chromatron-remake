//! The beam tracer (logic.md L3–L4: `retrace_beams` @ 0x3ea4, `trace_laser` @ 0x3a58).

use crate::board::{BOARD_CELLS, Board, SIDE, board_index};
use crate::piece::{Colour, DX, DY, Dir, Piece};
use crate::transfer::transfer;

/// Beams one generation can hold; further pushes are dropped (`push_beam` @ 0x34ec).
pub const MAX_BEAMS: usize = 32;
/// Generations traced per laser before giving up (`li r30, 0x3ff`).
pub const MAX_GENERATIONS: u32 = 1023;

/// `doppler_fwd_map` @ 0x8098 and `doppler_back_map` @ 0x8084, indexed by a one-bit colour mask.
const FWD_MAP: [Colour; 5] = [0, 4, 1, 0, 2];
const BACK_MAP: [Colour; 5] = [0, 2, 4, 0, 1];

/// Colour masks crossing each board cell, by travel direction (cell bytes +4..+19, data.md D2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeamField {
    /// Beams that entered the cell, by the direction they were travelling.
    pub inbound: [[Colour; 8]; BOARD_CELLS],
    /// Beams that left the cell, by the direction they were travelling.
    pub outbound: [[Colour; 8]; BOARD_CELLS],
}

impl Default for BeamField {
    fn default() -> BeamField {
        BeamField {
            inbound: [[0; 8]; BOARD_CELLS],
            outbound: [[0; 8]; BOARD_CELLS],
        }
    }
}

impl BeamField {
    /// Everything that entered cell `index`, from any direction: what a pinwheel there sees.
    pub fn received(&self, index: usize) -> Colour {
        self.inbound[index].iter().fold(0, |acc, &c| acc | c)
    }

    fn clear(&mut self) {
        self.inbound = [[0; 8]; BOARD_CELLS];
        self.outbound = [[0; 8]; BOARD_CELLS];
    }
}

/// "A beam of `colour` in cell (x, y) that will leave it travelling `dir`".
#[derive(Clone, Copy, Debug, Default)]
struct Beam {
    x: u8,
    y: u8,
    dir: Dir,
    colour: Colour,
    tag: i32,
}

#[derive(Clone, Copy)]
struct BeamList {
    beams: [Beam; MAX_BEAMS],
    len: usize,
}

impl BeamList {
    const fn new() -> BeamList {
        BeamList {
            beams: [Beam {
                x: 0,
                y: 0,
                dir: 0,
                colour: 0,
                tag: 0,
            }; MAX_BEAMS],
            len: 0,
        }
    }

    fn push(&mut self, beam: Beam) {
        if self.len < MAX_BEAMS {
            self.beams[self.len] = beam;
            self.len += 1;
        }
    }
}

/// Recomputes `field` from scratch for every laser on `board`. Allocation-free.
pub fn trace(board: &Board, field: &mut BeamField) {
    field.clear();
    for index in 0..BOARD_CELLS {
        let cell = board.cells[index];
        if cell.piece == Piece::Laser {
            trace_laser(
                board,
                field,
                (index % SIDE) as u8,
                (index / SIDE) as u8,
                cell.dir,
                cell.colour,
            );
        }
    }
}

fn on_board(x: u8, y: u8) -> bool {
    usize::from(x) < SIDE && usize::from(y) < SIDE
}

fn step(x: u8, y: u8, dir: Dir) -> (u8, u8) {
    let d = usize::from(dir);
    (x.wrapping_add_signed(DX[d]), y.wrapping_add_signed(DY[d]))
}

/// Traces one laser and returns how many generations ran (at most `MAX_GENERATIONS`).
fn trace_laser(
    board: &Board,
    field: &mut BeamField,
    x: u8,
    y: u8,
    dir: Dir,
    colour: Colour,
) -> u32 {
    let mut cur = BeamList::new();
    let mut next = BeamList::new();
    let mut next_tag = 1;
    next.push(Beam {
        x,
        y,
        dir,
        colour,
        tag: 0,
    });
    let mut generations = 0;
    loop {
        generations += 1;
        std::mem::swap(&mut cur, &mut next);
        next.len = 0;
        for beam in &cur.beams[..cur.len] {
            field.outbound[board_index(usize::from(beam.x), usize::from(beam.y))]
                [usize::from(beam.dir)] |= beam.colour;
            let (x, y) = step(beam.x, beam.y, beam.dir);
            if on_board(x, y) {
                enter_cell(
                    board,
                    field,
                    &mut next,
                    &mut next_tag,
                    Beam { x, y, ..*beam },
                );
            }
        }
        entangle(board, &mut next);
        if next.len == 0 || generations == MAX_GENERATIONS {
            return generations;
        }
    }
}

fn enter_cell(
    board: &Board,
    field: &mut BeamField,
    next: &mut BeamList,
    next_tag: &mut i32,
    beam: Beam,
) {
    let index = board_index(usize::from(beam.x), usize::from(beam.y));
    field.inbound[index][usize::from(beam.dir)] |= beam.colour;
    let cell = board.cells[index];
    if cell.piece == Piece::Teleporter {
        if let Some((x, y)) = teleport_target(board, beam.x, beam.y, beam.dir) {
            next.push(Beam { x, y, ..beam });
        }
        return;
    }
    for e in transfer(cell, beam.dir, beam.colour, beam.tag, next_tag).as_slice() {
        next.push(Beam {
            dir: e.dir,
            colour: e.colour,
            tag: e.tag,
            ..beam
        });
    }
}

/// The next teleporter along `dir`, skipping every other cell (walls included).
fn teleport_target(board: &Board, mut x: u8, mut y: u8, dir: Dir) -> Option<(u8, u8)> {
    loop {
        (x, y) = step(x, y, dir);
        if !on_board(x, y) {
            return None;
        }
        if board.at(usize::from(x), usize::from(y)).piece == Piece::Teleporter {
            return Some((x, y));
        }
    }
}

/// After each generation: a tagged beam that just left a doppler gets the doppler's shift, and
/// every other beam with its tag (its twin) the opposite shift, in place.
fn entangle(board: &Board, next: &mut BeamList) {
    for i in 0..next.len {
        let beam = next.beams[i];
        if beam.tag == 0 || !on_board(beam.x, beam.y) {
            continue;
        }
        let cell = board.at(usize::from(beam.x), usize::from(beam.y));
        if cell.piece != Piece::Doppler {
            continue;
        }
        let forward = beam.dir.wrapping_sub(cell.dir) & 7 == 2;
        for j in 0..next.len {
            if next.beams[j].tag == beam.tag {
                let map = if (i == j) == forward {
                    &FWD_MAP
                } else {
                    &BACK_MAP
                };
                let colour = &mut next.beams[j].colour;
                *colour = map.get(usize::from(*colour)).copied().unwrap_or(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Cell;
    use crate::piece::{BLUE, GREEN, RED, WHITE};

    fn board(pieces: &[(usize, usize, Piece, Colour, Dir)]) -> Board {
        let mut board = Board::default();
        for &(x, y, piece, colour, dir) in pieces {
            board.cells[board_index(x, y)] = Cell { piece, colour, dir };
        }
        board
    }

    fn traced(board: &Board) -> BeamField {
        let mut field = BeamField::default();
        trace(board, &mut field);
        field
    }

    #[test]
    fn a_laser_beam_crosses_the_board_and_leaves_it() {
        let field = traced(&board(&[(2, 7, Piece::Laser, RED, 2)]));
        assert_eq!(
            field.outbound[board_index(2, 7)][2],
            RED,
            "starts in the laser's own cell"
        );
        assert_eq!(
            field.inbound[board_index(2, 7)],
            [0; 8],
            "the laser cell is not entered"
        );
        for x in 3..15 {
            assert_eq!(field.inbound[board_index(x, 7)][2], RED, "x={x}");
            assert_eq!(field.outbound[board_index(x, 7)][2], RED, "x={x}");
        }
        assert_eq!(field.received(board_index(2, 6)), 0);
    }

    #[test]
    fn a_lone_beam_runs_one_generation_per_cell() {
        let lone = board(&[(2, 7, Piece::Laser, RED, 2)]);
        let mut field = BeamField::default();
        assert_eq!(
            trace_laser(&lone, &mut field, 2, 7, 2, RED),
            13,
            "(2,7) to (14,7), then off"
        );
    }

    #[test]
    fn walls_and_other_lasers_absorb() {
        let field = traced(&board(&[
            (0, 0, Piece::Laser, RED, 2),
            (5, 0, Piece::Wall, 0, 0),
            (0, 5, Piece::Laser, BLUE, 2),
            (5, 5, Piece::Laser, GREEN, 6),
        ]));
        assert_eq!(field.inbound[board_index(5, 0)][2], RED);
        assert_eq!(field.outbound[board_index(5, 0)], [0; 8]);
        assert_eq!(field.received(board_index(6, 0)), 0);
        assert_eq!(
            field.received(board_index(5, 5)),
            BLUE,
            "lasers absorb but record what enters"
        );
        assert_eq!(field.received(board_index(0, 5)), GREEN);
    }

    #[test]
    fn pinwheels_see_beams_from_every_laser_and_do_not_block() {
        let field = traced(&board(&[
            (0, 4, Piece::Laser, RED, 2),
            (4, 0, Piece::Laser, BLUE, 4),
            (4, 4, Piece::Pinwheel, RED | BLUE, 0),
        ]));
        assert_eq!(field.received(board_index(4, 4)), RED | BLUE);
        assert_eq!(
            field.inbound[board_index(9, 4)][2],
            RED,
            "the beam goes on through the pinwheel"
        );
    }

    #[test]
    fn teleporter_jumps_over_walls_to_the_next_teleporter() {
        let field = traced(&board(&[
            (0, 3, Piece::Laser, GREEN, 2),
            (3, 3, Piece::Teleporter, 0, 0),
            (5, 3, Piece::Wall, 0, 0),
            (9, 3, Piece::Teleporter, 0, 0),
        ]));
        assert_eq!(field.inbound[board_index(3, 3)][2], GREEN);
        assert_eq!(
            field.outbound[board_index(3, 3)],
            [0; 8],
            "nothing leaves the entry teleporter"
        );
        assert_eq!(field.received(board_index(5, 3)), 0, "the wall is skipped");
        assert_eq!(
            field.inbound[board_index(9, 3)],
            [0; 8],
            "the target is not entered"
        );
        assert_eq!(field.outbound[board_index(9, 3)][2], GREEN);
        assert_eq!(field.inbound[board_index(10, 3)][2], GREEN);
    }

    #[test]
    fn teleporter_without_a_target_swallows_the_beam() {
        let field = traced(&board(&[
            (0, 3, Piece::Laser, GREEN, 2),
            (3, 3, Piece::Teleporter, 0, 0),
        ]));
        assert_eq!(field.received(board_index(4, 3)), 0);
    }

    #[test]
    fn a_mirror_loop_runs_until_the_generation_limit() {
        // The laser feeds a ring of a splitter and three mirrors, where one beam circles forever;
        // the trace stops after exactly 1023 generations.
        let ring = board(&[
            (0, 2, Piece::Laser, WHITE, 2),
            (2, 2, Piece::Splitter, 0, 1),
            (6, 2, Piece::Mirror, 0, 7),
            (6, 6, Piece::Mirror, 0, 1),
            (2, 6, Piece::Mirror, 0, 3),
        ]);
        let mut field = BeamField::default();
        assert_eq!(
            trace_laser(&ring, &mut field, 0, 2, 2, WHITE),
            MAX_GENERATIONS
        );
        assert_eq!(
            field.inbound[board_index(4, 6)][6],
            WHITE,
            "the loop is lit"
        );
        assert_eq!(field.inbound[board_index(2, 4)][0], WHITE);
    }

    #[test]
    fn a_generation_holds_at_most_32_beams() {
        let mut list = BeamList::new();
        for i in 0..40 {
            list.push(Beam {
                x: i,
                ..Beam::default()
            });
        }
        assert_eq!(list.len, MAX_BEAMS);
        assert_eq!(list.beams[MAX_BEAMS - 1].x, 31, "later pushes are dropped");
    }

    #[test]
    fn an_entangled_twin_takes_the_opposite_doppler_shift() {
        // Red laser → tangler (pair N/S, tag 1) → the north beam crosses a forward doppler three
        // cells out; its twin, three cells south, turns from red to blue in the same generation.
        let field = traced(&board(&[
            (0, 7, Piece::Laser, RED, 2),
            (5, 7, Piece::Tangler, 0, 0),
            (5, 4, Piece::Doppler, 0, 6),
            (5, 1, Piece::Pinwheel, GREEN, 0),
            (5, 12, Piece::Pinwheel, BLUE, 0),
        ]));
        assert_eq!(
            field.outbound[board_index(5, 4)][0],
            GREEN,
            "forward shift on the beam itself"
        );
        assert_eq!(field.received(board_index(5, 1)), GREEN);
        assert_eq!(
            field.inbound[board_index(5, 10)][4],
            RED,
            "the twin entered its cell still red"
        );
        assert_eq!(
            field.outbound[board_index(5, 10)][4],
            BLUE,
            "and left it blue"
        );
        assert_eq!(field.received(board_index(5, 12)), BLUE);
    }

    #[test]
    fn a_45_degree_splitter_hit_collapses_entanglement() {
        // As above, but the north beam goes through a splitter at 45° before the doppler: its
        // tag is cleared, so the doppler shifts it normally and the twin stays red.
        let field = traced(&board(&[
            (0, 7, Piece::Laser, RED, 2),
            (5, 7, Piece::Tangler, 0, 0),
            (5, 6, Piece::Splitter, 0, 1),
            (5, 4, Piece::Doppler, 0, 6),
            (5, 12, Piece::Pinwheel, RED, 0),
        ]));
        assert_eq!(field.outbound[board_index(5, 4)][0], GREEN);
        assert_eq!(field.received(board_index(5, 12)), RED);
    }

    #[test]
    fn retracing_starts_from_a_clean_field() {
        let mut field = traced(&board(&[(0, 0, Piece::Laser, RED, 2)]));
        trace(&board(&[(0, 0, Piece::Laser, BLUE, 4)]), &mut field);
        assert_eq!(field.received(board_index(5, 0)), 0);
        assert_eq!(field.received(board_index(0, 5)), BLUE);
    }
}
