//! The beam tracer (logic.md L3–L4: `retrace_beams` C1 @ 0x3ea4, C2 @ 0x415c, C3 @ 0x4258,
//! C4 @ 0x405c; `trace_laser` C1 @ 0x3a58, C2 @ 0x39e0), with the complementor pass of
//! Chromatron 2 on and the gate pass of Chromatron 3 on.

use crate::board::{BOARD_CELLS, Board, SIDE, board_index};
use crate::piece::{BLUE, Colour, DX, DY, Dir, GREEN, Piece, RED, WHITE, turn};
use crate::rule_set::{Passes, RuleSet};
use crate::transfer::transfer;

/// Most beams a generation holds in any game (Chromatron 4's `push_beam` @ 0x3020); the others
/// hold 32 (`RuleSet::max_beams`). Further pushes are dropped.
pub const MAX_BEAMS: usize = 128;
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

/// One generation's beams, in fixed storage for the largest cap; `cap` is the game's.
#[derive(Clone, Copy)]
struct BeamList {
    beams: [Beam; MAX_BEAMS],
    len: usize,
    cap: usize,
}

impl BeamList {
    const fn new(cap: usize) -> BeamList {
        BeamList {
            beams: [Beam {
                x: 0,
                y: 0,
                dir: 0,
                colour: 0,
                tag: 0,
            }; MAX_BEAMS],
            len: 0,
            cap: if cap < MAX_BEAMS { cap } else { MAX_BEAMS },
        }
    }

    fn push(&mut self, beam: Beam) {
        if self.len < self.cap {
            self.beams[self.len] = beam;
            self.len += 1;
        }
    }
}

/// Recomputes `field` from scratch for every laser on `board`, then lets the gates and
/// complementors emit until nothing changes (`retrace_beams`). Allocation-free: the passes work in
/// two fixed-size arrays on the stack.
pub fn trace(board: &Board, field: &mut BeamField) {
    let complementors = (0..BOARD_CELLS).any(|i| board.cells[i].piece == Piece::Complementor);
    let gates = board.rules().passes == Passes::GatesAroundComplementors
        && (0..BOARD_CELLS).any(|i| {
            matches!(
                board.cells[i].piece,
                Piece::LogicGate | Piece::PrimaryLogicGate
            )
        });
    // `clear_complementor_flags`: cell +3, "shut down", for this retrace only.
    let mut shut = [false; BOARD_CELLS];
    // `snapshot_cell_inputs`: cell +20..+27, the inputs before the latest emission.
    let mut snapshot = [[0; 8]; BOARD_CELLS];
    loop {
        field.clear();
        trace_all_lasers(board, field);
        if !complementors && !gates {
            return;
        }
        loop {
            snapshot.copy_from_slice(&field.inbound);
            let mut changed = gates && emit_gates(board, field);
            emit_complementors(board, field, &shut);
            changed |= complementor_new_input(board, field, &snapshot, &shut);
            changed |= gates && emit_gates(board, field);
            if !changed {
                break;
            }
        }
        if !shut_down_complementors(board, field, &snapshot, &mut shut) {
            return;
        }
    }
}

/// `trace_all_lasers`: every laser, row by row.
fn trace_all_lasers(board: &Board, field: &mut BeamField) {
    for index in 0..BOARD_CELLS {
        let cell = board.cells[index];
        if cell.piece == Piece::Laser {
            let (x, y) = xy(index);
            trace_laser(board, field, x, y, cell.dir, cell.colour);
        }
    }
}

fn xy(index: usize) -> (u8, u8) {
    ((index % SIDE) as u8, (index / SIDE) as u8)
}

/// The board cells that hold a complementor not shut down, row by row.
fn live_complementors<'a>(
    board: &'a Board,
    shut: &'a [bool; BOARD_CELLS],
) -> impl Iterator<Item = usize> + 'a {
    (0..BOARD_CELLS).filter(|&i| board.cells[i].piece == Piece::Complementor && !shut[i])
}

/// `emit_complementors` (logic.md L3): each live complementor sends the complement of every input
/// whose direction has its `dir`'s parity on in that direction, as a new laser trace from its own
/// cell. Inputs are read live, so an emission can feed a later complementor in the same pass.
/// White has no complement.
fn emit_complementors(board: &Board, field: &mut BeamField, shut: &[bool; BOARD_CELLS]) {
    for index in live_complementors(board, shut) {
        let parity = board.cells[index].dir & 1;
        for d in 0..8u8 {
            let k = field.inbound[index][usize::from(d)];
            if k != 0 && k != WHITE && d & 1 == parity {
                let (x, y) = xy(index);
                trace_laser(board, field, x, y, d, k ^ WHITE);
            }
        }
    }
}

/// `emit_gates` (C3 @ 0x4198) / `emit_logic_gates` (C4 @ 0x3f1c), row by row: a gate pointing
/// `p` sends the colour bits its two inputs share (the beams that entered travelling `p + 1` and
/// `p − 1`) and that it hasn't sent yet, as a new laser trace from its own cell in direction `p`.
/// A primary logic gate sends only when the shared colour is exactly one primary. Inputs are read
/// live. Returns whether any gate emitted. A raw dir of 8 or 9 (from an original save) acts as
/// `dir & 7`. The original reads past `out[]` and also starts its `trace_laser` in the raw dir,
/// reading `dx`/`dy` past their tables (dir 8 steps west, dir 9 steps (-1, +2)).
fn emit_gates(board: &Board, field: &mut BeamField) -> bool {
    let mut fired = false;
    for index in 0..BOARD_CELLS {
        let cell = board.cells[index];
        let primary = match cell.piece {
            Piece::LogicGate => false,
            Piece::PrimaryLogicGate => true,
            _ => continue,
        };
        let p = cell.dir & 7;
        let inputs = &field.inbound[index];
        let shared = inputs[usize::from(turn(p, 1))] & inputs[usize::from(turn(p, 7))];
        if primary && !matches!(shared, BLUE | GREEN | RED) {
            continue;
        }
        let k = shared & !field.outbound[index][usize::from(p)];
        if k != 0 {
            let (x, y) = xy(index);
            trace_laser(board, field, x, y, p, k);
            fired = true;
        }
    }
    fired
}

/// `complementor_new_input`: some live complementor got an input, in any of the 8 directions,
/// that it didn't have at the snapshot.
fn complementor_new_input(
    board: &Board,
    field: &BeamField,
    snapshot: &[[Colour; 8]; BOARD_CELLS],
    shut: &[bool; BOARD_CELLS],
) -> bool {
    live_complementors(board, shut)
        .any(|i| (0..8).any(|d| field.inbound[i][d] != 0 && snapshot[i][d] == 0))
}

/// `shut_down_complementors`: shuts every live complementor whose input changed after it had
/// already emitted for it (typically its own complement fed back), and says whether any was.
/// The original's exception for a snapshot of white and an input of black can't happen, because
/// inputs only grow, but is kept.
fn shut_down_complementors(
    board: &Board,
    field: &BeamField,
    snapshot: &[[Colour; 8]; BOARD_CELLS],
    shut: &mut [bool; BOARD_CELLS],
) -> bool {
    let mut any = false;
    for i in 0..BOARD_CELLS {
        if board.cells[i].piece != Piece::Complementor || shut[i] {
            continue;
        }
        let contradicted = (0..8).any(|d| {
            let (was, now) = (snapshot[i][d], field.inbound[i][d]);
            was != now && was != 0 && !(was == WHITE && now == 0)
        });
        if contradicted {
            shut[i] = true;
            any = true;
        }
    }
    any
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
    let rules = board.rules();
    let mut lists = [
        BeamList::new(rules.max_beams),
        BeamList::new(rules.max_beams),
    ];
    let [mut cur, mut next] = lists.each_mut();
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
                    rules,
                    board,
                    field,
                    next,
                    &mut next_tag,
                    Beam { x, y, ..*beam },
                );
            }
        }
        entangle(board, next);
        if next.len == 0 || generations == MAX_GENERATIONS {
            return generations;
        }
    }
}

fn enter_cell(
    rules: &RuleSet,
    board: &Board,
    field: &mut BeamField,
    next: &mut BeamList,
    next_tag: &mut i32,
    beam: Beam,
) {
    let index = board_index(usize::from(beam.x), usize::from(beam.y));
    field.inbound[index][usize::from(beam.dir)] |= beam.colour;
    let cell = board.cells[index];
    if matches!(cell.piece, Piece::Teleporter | Piece::MobileTeleporter) {
        if let Some((x, y)) = teleport_target(rules, board, beam.x, beam.y, beam.dir) {
            next.push(Beam { x, y, ..beam });
        }
        return;
    }
    for e in transfer(rules, cell, beam.dir, beam.colour, beam.tag, next_tag).as_slice() {
        next.push(Beam {
            dir: e.dir,
            colour: e.colour,
            tag: e.tag,
            ..beam
        });
    }
}

/// The next cell along `dir` that holds one of the game's teleport targets, skipping every other
/// cell (walls included).
fn teleport_target(
    rules: &RuleSet,
    board: &Board,
    mut x: u8,
    mut y: u8,
    dir: Dir,
) -> Option<(u8, u8)> {
    loop {
        (x, y) = step(x, y, dir);
        if !on_board(x, y) {
            return None;
        }
        let piece = board.at(usize::from(x), usize::from(y)).piece;
        if rules.teleport_targets.contains(&piece) {
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
    use spectrograph::Game;

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
        let mut list = BeamList::new(RuleSet::C1.max_beams);
        for i in 0..40 {
            list.push(Beam {
                x: i,
                ..Beam::default()
            });
        }
        assert_eq!(list.len, 32);
        assert_eq!(list.beams[31].x, 31, "later pushes are dropped");
    }

    #[test]
    fn a_chromatron_4_generation_holds_128_beams() {
        let mut list = BeamList::new(RuleSet::C4.max_beams);
        for i in 0..200 {
            list.push(Beam {
                x: i,
                ..Beam::default()
            });
        }
        assert_eq!((list.len, MAX_BEAMS), (128, 128));
        assert_eq!(list.beams[127].x, 127, "later pushes are dropped");
        assert_eq!(BeamList::new(500).cap, MAX_BEAMS, "never past the storage");
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

    const CYAN: Colour = GREEN | BLUE;

    /// A Chromatron 2 board.
    fn c2_board(pieces: &[(usize, usize, Piece, Colour, Dir)]) -> Board {
        Board {
            game: Game::C2,
            ..board(pieces)
        }
    }

    #[test]
    fn a_complementor_sends_the_complement_on_in_the_same_direction() {
        let field = traced(&c2_board(&[
            (0, 5, Piece::Laser, RED, 2),
            (5, 5, Piece::Complementor, 4, 0),
        ]));
        let at = board_index(5, 5);
        assert_eq!(field.inbound[at][2], RED);
        assert_eq!(field.outbound[at][2], CYAN, "drawn from its own cell");
        assert_eq!(field.inbound[board_index(6, 5)][2], CYAN);
        assert_eq!(field.received(board_index(14, 5)), CYAN);
    }

    #[test]
    fn complements_of_every_colour_and_none_for_white() {
        for (colour, complement) in [(RED, CYAN), (GREEN, RED | BLUE), (BLUE, RED | GREEN)] {
            for (input, output) in [(colour, complement), (complement, colour)] {
                let field = traced(&c2_board(&[
                    (0, 5, Piece::Laser, input, 2),
                    (5, 5, Piece::Complementor, 4, 0),
                ]));
                assert_eq!(field.received(board_index(6, 5)), output, "{input}");
            }
        }
        let field = traced(&c2_board(&[
            (0, 5, Piece::Laser, WHITE, 2),
            (5, 5, Piece::Complementor, 4, 0),
        ]));
        assert_eq!(field.outbound[board_index(5, 5)], [0; 8]);
    }

    #[test]
    fn even_dirs_take_orthogonal_beams_and_odd_dirs_diagonal_ones() {
        let orthogonal = |dir| {
            traced(&c2_board(&[
                (0, 5, Piece::Laser, RED, 2),
                (5, 5, Piece::Complementor, 4, dir),
            ]))
            .received(board_index(6, 5))
        };
        assert_eq!(orthogonal(2), CYAN);
        assert_eq!(orthogonal(9), 0, "dir 9 is odd: absorbed");
        let diagonal = |dir| {
            traced(&c2_board(&[
                (0, 0, Piece::Laser, RED, 3),
                (4, 4, Piece::Complementor, 4, dir),
            ]))
            .received(board_index(5, 5))
        };
        assert_eq!(diagonal(1), CYAN);
        assert_eq!(diagonal(0), 0, "dir 0 is even: absorbed");
    }

    #[test]
    fn a_complement_starts_untagged_like_any_laser_beam() {
        // A tangler's north beam is complemented (red → cyan), then a forward doppler shifts it
        // as an ordinary beam (cyan → magenta); its south twin is not touched.
        let field = traced(&c2_board(&[
            (0, 7, Piece::Laser, RED, 2),
            (5, 7, Piece::Tangler, 0, 0),
            (5, 5, Piece::Complementor, 4, 0),
            (5, 3, Piece::Doppler, 0, 6),
            (5, 12, Piece::Pinwheel, RED, 0),
        ]));
        assert_eq!(field.outbound[board_index(5, 3)][0], RED | BLUE);
        assert_eq!(field.received(board_index(5, 12)), RED);
    }

    #[test]
    fn a_complement_can_feed_the_next_complementor_in_the_same_pass() {
        for (first, second) in [(3, 6), (6, 3)] {
            // Beam E: the first complementor in row order emits before the second reads. Beam W
            // (mirrored board): the second is reached only on the next inner round.
            let field = if first < second {
                traced(&c2_board(&[
                    (0, 0, Piece::Laser, RED, 2),
                    (first, 0, Piece::Complementor, 4, 0),
                    (second, 0, Piece::Complementor, 4, 0),
                ]))
            } else {
                traced(&c2_board(&[
                    (14, 0, Piece::Laser, RED, 6),
                    (first + 5, 0, Piece::Complementor, 4, 0),
                    (second + 5, 0, Piece::Complementor, 4, 0),
                ]))
            };
            let beyond = if first < second { 9 } else { 0 };
            assert_eq!(
                field.received(board_index(beyond, 0)),
                RED,
                "{first}, {second}"
            );
        }
    }

    /// Red enters a complementor heading E; its cyan goes round four pieces and comes back into the
    /// same input through a splitter, which turns that input white.
    fn feedback_loop() -> Board {
        c2_board(&[
            (0, 5, Piece::Laser, RED, 2),
            (2, 5, Piece::Splitter, 0, 3),
            (5, 5, Piece::Complementor, 4, 0),
            (8, 5, Piece::Mirror, 0, 1),
            (8, 2, Piece::Mirror, 0, 7),
            (2, 2, Piece::Mirror, 0, 5),
        ])
    }

    #[test]
    fn a_complementor_fed_its_own_complement_shuts_down() {
        let board = feedback_loop();
        let mut field = BeamField::default();
        for _ in 0..2 {
            trace(&board, &mut field);
            let at = board_index(5, 5);
            assert_eq!(field.inbound[at][2], RED, "it still absorbs");
            assert_eq!(field.outbound[at], [0; 8], "and emits nothing");
            assert_eq!(field.received(board_index(6, 5)), 0);
            assert_eq!(field.received(board_index(8, 3)), 0, "the loop is dark");
        }
    }

    #[test]
    fn without_its_feedback_the_same_complementor_works() {
        let mut board = feedback_loop();
        board.cells[board_index(8, 2)] = Cell::default();
        let field = traced(&board);
        assert_eq!(field.outbound[board_index(5, 5)][2], CYAN);
        assert_eq!(field.received(board_index(8, 0)), CYAN);
    }

    /// A board of `game`.
    fn board_of(game: Game, pieces: &[(usize, usize, Piece, Colour, Dir)]) -> Board {
        Board {
            game,
            ..board(pieces)
        }
    }

    const MAGENTA: Colour = RED | BLUE;
    const YELLOW: Colour = RED | GREEN;

    #[test]
    fn teleporters_of_both_kinds_pair_with_each_other() {
        for game in [Game::C3, Game::C4] {
            for (entry, target) in [
                (Piece::Teleporter, Piece::MobileTeleporter),
                (Piece::MobileTeleporter, Piece::Teleporter),
                (Piece::MobileTeleporter, Piece::MobileTeleporter),
            ] {
                let field = traced(&board_of(
                    game,
                    &[
                        (0, 3, Piece::Laser, GREEN, 2),
                        (3, 3, entry, 4, 0),
                        (5, 3, Piece::Wall, 0, 0),
                        (9, 3, target, 4, 7),
                    ],
                ));
                assert_eq!(field.received(board_index(5, 3)), 0, "{game:?}");
                assert_eq!(
                    field.outbound[board_index(9, 3)][2],
                    GREEN,
                    "{game:?} {entry:?} → {target:?}"
                );
                assert_eq!(field.inbound[board_index(10, 3)][2], GREEN);
            }
        }
    }

    #[test]
    fn a_starburst_sends_four_beams_of_the_other_parity() {
        let field = traced(&board_of(
            Game::C3,
            &[(0, 7, Piece::Laser, RED, 2), (7, 7, Piece::Starburst, 4, 0)],
        ));
        let at = board_index(7, 7);
        assert_eq!(field.inbound[at][2], RED);
        assert_eq!(field.outbound[at], [0, RED, 0, RED, 0, RED, 0, RED]);
        assert_eq!(
            field.received(board_index(8, 7)),
            0,
            "nothing goes straight on"
        );
        assert_eq!(field.received(board_index(10, 4)), RED);
        assert_eq!(field.received(board_index(4, 10)), RED);
    }

    #[test]
    fn chromatron_4_keeps_beams_that_chromatron_3_drops() {
        // Five starbursts multiply one beam past 32 per generation: Chromatron 3 drops the extra
        // pushes, Chromatron 4 (128 beams) keeps them and lights more of the board.
        let pieces = [
            (0, 7, Piece::Laser, RED, 2),
            (4, 7, Piece::Starburst, 4, 0),
            (6, 5, Piece::Starburst, 4, 0),
            (6, 9, Piece::Starburst, 4, 0),
            (2, 5, Piece::Starburst, 4, 0),
            (2, 9, Piece::Starburst, 4, 0),
        ];
        let c3 = traced(&board_of(Game::C3, &pieces));
        let c4 = traced(&board_of(Game::C4, &pieces));
        for (x, y) in [(13, 9), (14, 9), (2, 14)] {
            let at = board_index(x, y);
            assert_eq!((c3.received(at), c4.received(at)), (0, RED), "({x},{y})");
        }
        assert!((0..BOARD_CELLS).all(|i| c3.received(i) == 0 || c4.received(i) == RED));
    }

    /// A gate at (7,7) pointing N, fed from the SW (beam heading NE) and the SE (beam heading NW).
    fn gate_board(game: Game, gate: Piece, from_sw: Colour, from_se: Colour) -> Board {
        let mut pieces = vec![(7, 7, gate, 4, 0), (7, 2, Piece::Pinwheel, 0, 0)];
        if from_sw != 0 {
            pieces.push((4, 10, Piece::Laser, from_sw, 1));
        }
        if from_se != 0 {
            pieces.push((10, 10, Piece::Laser, from_se, 7));
        }
        board_of(game, &pieces)
    }

    #[test]
    fn a_logic_gate_sends_on_the_colour_both_inputs_share() {
        // logic.md L3 (C4 slot 6): magenta AND yellow is red, sent straight ahead from the gate.
        for game in [Game::C3, Game::C4] {
            let field = traced(&gate_board(game, Piece::LogicGate, MAGENTA, YELLOW));
            let at = board_index(7, 7);
            assert_eq!(
                (field.inbound[at][1], field.inbound[at][7]),
                (MAGENTA, YELLOW)
            );
            assert_eq!(field.outbound[at], [RED, 0, 0, 0, 0, 0, 0, 0], "{game:?}");
            assert_eq!(field.received(board_index(7, 2)), RED);
            for (sw, se) in [(MAGENTA, 0), (0, YELLOW), (RED, BLUE)] {
                let field = traced(&gate_board(game, Piece::LogicGate, sw, se));
                assert_eq!(field.outbound[at], [0; 8], "{game:?} {sw} {se}");
            }
        }
    }

    #[test]
    fn a_gate_ignores_beams_from_other_sides() {
        let field = traced(&board_of(
            Game::C4,
            &[
                (7, 7, Piece::LogicGate, 4, 0),
                (0, 7, Piece::Laser, WHITE, 2),
                (7, 14, Piece::Laser, WHITE, 0),
                (7, 2, Piece::Pinwheel, 0, 0),
            ],
        ));
        assert_eq!(field.outbound[board_index(7, 7)], [0; 8]);
        assert_eq!(field.received(board_index(8, 7)), 0, "it absorbs");
    }

    #[test]
    fn a_primary_logic_gate_sends_only_a_single_primary() {
        let at = board_index(7, 7);
        for (sw, se, out) in [
            (MAGENTA, YELLOW, RED),
            (CYAN, YELLOW, GREEN),
            (WHITE, BLUE, BLUE),
            (WHITE, WHITE, 0),
            (MAGENTA, MAGENTA, 0),
            (YELLOW, CYAN | RED, 0),
        ] {
            let field = traced(&gate_board(Game::C4, Piece::PrimaryLogicGate, sw, se));
            assert_eq!(field.outbound[at][0], out, "{sw} & {se}");
            assert_eq!(field.received(board_index(7, 2)), out);
        }
        let plain = traced(&gate_board(Game::C4, Piece::LogicGate, WHITE, WHITE));
        assert_eq!(
            plain.outbound[at][0], WHITE,
            "a plain gate sends any colour"
        );
    }

    #[test]
    fn a_gate_fed_its_own_output_settles() {
        // The gate's red goes N, W, then SE into a splitter that merges it into the gate's own
        // NE-bound input: the input doesn't change, so the pass ends with one emission.
        let board = board_of(
            Game::C4,
            &[
                (3, 11, Piece::Laser, RED, 1),
                (10, 10, Piece::Laser, YELLOW, 7),
                (5, 9, Piece::Splitter, 0, 2),
                (7, 7, Piece::LogicGate, 4, 0),
                (7, 5, Piece::Mirror, 0, 7),
                (1, 5, Piece::Bender, 0, 5),
            ],
        );
        let field = traced(&board);
        let at = board_index(7, 7);
        assert_eq!(field.inbound[at][1], RED);
        assert_eq!(field.outbound[at], [RED, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            field.inbound[board_index(5, 9)][3],
            RED,
            "the loop is closed"
        );
        assert_eq!(traced(&board), field, "and the same every time");
    }

    #[test]
    fn a_gate_with_a_raw_dir_of_8_or_9_acts_as_dir_0_or_1() {
        let mut board = gate_board(Game::C4, Piece::LogicGate, MAGENTA, YELLOW);
        board.cells[board_index(7, 7)].dir = 8;
        assert_eq!(traced(&board).received(board_index(7, 2)), RED);
    }

    /// The first gate pass of a round runs before the complementors: a green gate output merges
    /// (through a splitter) with red into a complementor's east-bound input, which therefore
    /// sends only blue. Without that first pass the complementor would also send red's cyan.
    fn gate_before_complementor() -> Board {
        board_of(
            Game::C3,
            &[
                (3, 4, Piece::Laser, GREEN, 3),
                (3, 10, Piece::Laser, GREEN, 1),
                (6, 7, Piece::LogicGate, 4, 2),
                (8, 0, Piece::Laser, RED, 4),
                (8, 7, Piece::Splitter, 0, 3),
                (10, 7, Piece::Complementor, 4, 0),
                (13, 7, Piece::Pinwheel, BLUE, 0),
            ],
        )
    }

    #[test]
    fn gates_emit_before_the_complementors_in_each_round() {
        let board = gate_before_complementor();
        let field = traced(&board);
        assert_eq!(field.inbound[board_index(10, 7)][2], YELLOW);
        assert_eq!(
            field.outbound[board_index(10, 7)][2],
            BLUE,
            "not cyan | blue"
        );
        assert!(crate::win::is_won(&board, &field));
    }

    /// The second gate pass of a round runs after the complementors: a complementor's cyan reaches a
    /// gate that then sends green on, with no complementor input new (C4 logic.md L4, slot 15).
    fn complementor_before_gate(game: Game) -> Board {
        board_of(
            game,
            &[
                (0, 7, Piece::Laser, RED, 2),
                (3, 7, Piece::Complementor, 4, 0),
                (6, 7, Piece::LogicGate, 4, 1),
                (6, 12, Piece::Laser, GREEN, 0),
                (9, 4, Piece::Pinwheel, GREEN, 0),
            ],
        )
    }

    #[test]
    fn gates_emit_again_after_the_complementors() {
        for game in [Game::C3, Game::C4] {
            let board = complementor_before_gate(game);
            let field = traced(&board);
            assert_eq!(field.inbound[board_index(6, 7)][2], CYAN);
            assert_eq!(field.outbound[board_index(6, 7)][1], GREEN, "{game:?}");
            assert!(crate::win::is_won(&board, &field), "{game:?}");
        }
    }

    /// The second gate pass of a round counts as a change: in round 1 the complementor's cyan
    /// reaches gate G (6,7) only in the second pass, and G's green (NE) lands in the gate G0 (9,4)
    /// on an earlier row, which can only emit in round 2 (green N to the pinwheel at (9,1)). If the
    /// second pass did not count, the rounds would stop after round 1.
    #[test]
    fn the_second_gate_pass_counts_as_a_change() {
        for game in [Game::C3, Game::C4] {
            let board = board_of(
                game,
                &[
                    (0, 7, Piece::Laser, RED, 2),
                    (3, 7, Piece::Complementor, 4, 0),
                    (6, 7, Piece::LogicGate, 4, 1),
                    (6, 12, Piece::Laser, GREEN, 0),
                    (9, 4, Piece::LogicGate, 4, 0),
                    (12, 7, Piece::Laser, WHITE, 7),
                    (9, 1, Piece::Pinwheel, GREEN, 0),
                ],
            );
            let field = traced(&board);
            assert_eq!(field.outbound[board_index(9, 4)][0], GREEN, "{game:?}");
            assert!(crate::win::is_won(&board, &field), "{game:?}");
        }
    }

    /// Three gates, each fed by the next one down: G1 (row 10) feeds G2 (row 7), which feeds G3
    /// (row 4). Row-major order reaches G3 before G2 and G2 before G1, so G3 only emits in the
    /// second round: the rounds repeat because a gate emitted, with no complementor at all.
    fn gate_chain() -> Board {
        board_of(
            Game::C4,
            &[
                (4, 13, Piece::Laser, WHITE, 1),
                (10, 13, Piece::Laser, WHITE, 7),
                (7, 10, Piece::LogicGate, 4, 0),
                (3, 7, Piece::Laser, WHITE, 2),
                (7, 7, Piece::LogicGate, 4, 1),
                (13, 7, Piece::Laser, WHITE, 7),
                (10, 4, Piece::LogicGate, 4, 0),
                (10, 1, Piece::Pinwheel, WHITE, 0),
            ],
        )
    }

    #[test]
    fn rounds_repeat_while_a_gate_emits() {
        let board = gate_chain();
        let field = traced(&board);
        assert_eq!(field.outbound[board_index(7, 10)][0], WHITE);
        assert_eq!(field.outbound[board_index(7, 7)][1], WHITE);
        assert_eq!(field.outbound[board_index(10, 4)][0], WHITE);
        assert!(crate::win::is_won(&board, &field));
    }
}
