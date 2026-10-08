//! What each piece does to a beam that enters its cell (logic.md L3, `beam_enter_cell` @ 0x3568).

use crate::board::Cell;
use crate::piece::{BLUE, Colour, Dir, GREEN, Piece, RED, turn};

/// One beam leaving a cell: direction, colour mask and entanglement tag (0 = not entangled).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Emission {
    pub dir: Dir,
    pub colour: Colour,
    pub tag: i32,
}

/// Most beams one entry can produce: a white beam through a tangler gives 3 pairs.
pub const MAX_EMISSIONS: usize = 6;

/// The beams one entry produces, in the order the original pushes them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Emissions {
    items: [Emission; MAX_EMISSIONS],
    len: usize,
}

impl Emissions {
    pub fn as_slice(&self) -> &[Emission] {
        &self.items[..self.len]
    }

    fn push(&mut self, dir: Dir, colour: Colour, tag: i32) {
        self.items[self.len] = Emission { dir, colour, tag };
        self.len += 1;
    }
}

/// Prism outputs per colour bit, in push order (red, green, blue): `Some(k)` sends that bit out
/// at `p + k`, indexed by `r` (logic.md L3 prism table).
const PRISM: [(Colour, [Option<u8>; 8]); 3] = [
    (
        RED,
        [None, Some(1), Some(2), None, None, Some(5), Some(6), None],
    ),
    (
        GREEN,
        [Some(1), None, Some(3), None, None, Some(4), None, Some(6)],
    ),
    (
        BLUE,
        [Some(6), None, Some(4), None, None, Some(3), None, Some(1)],
    ),
];

/// Doppler "forward" shift, per bit: red → green → blue → red.
pub fn doppler_forward(c: Colour) -> Colour {
    (c & RED) >> 1 | (c & GREEN) >> 1 | (c & BLUE) << 2
}

/// Doppler "backward" shift, the inverse: red → blue → green → red.
pub fn doppler_backward(c: Colour) -> Colour {
    (c & RED) >> 2 | (c & GREEN) << 1 | (c & BLUE) << 1
}

/// The beams that leave `cell` when a beam travelling `d` with colour `c` and tag `t` enters it.
/// `next_tag` is the trace's `entangle_counter`. Teleporters are handled by the tracer (they need
/// the board) and produce nothing here.
pub fn transfer(cell: Cell, d: Dir, c: Colour, t: i32, next_tag: &mut i32) -> Emissions {
    let mut out = Emissions::default();
    let p = cell.dir;
    let r = d.wrapping_sub(p) & 7;
    let mirrored = p.wrapping_mul(2).wrapping_sub(d) & 7;
    match cell.piece {
        Piece::Empty | Piece::Pinwheel => out.push(d, c, t),
        Piece::Wall | Piece::Laser | Piece::Teleporter => {}
        Piece::Mirror => {
            if (1..=3).contains(&r) {
                out.push(mirrored, c, t);
            }
        }
        Piece::Bender => {
            if r <= 3 {
                out.push(turn(mirrored, 7), c, t);
            }
        }
        Piece::Filter => {
            let passed = c & cell.colour;
            if (r == 2 || r == 6) && passed != 0 {
                out.push(d, passed, t);
            }
        }
        Piece::Prism => {
            for (bit, outputs) in PRISM {
                if c & bit != 0
                    && let Some(k) = outputs[usize::from(r)]
                {
                    out.push(turn(p, k), bit, t);
                }
            }
        }
        Piece::Doppler => match r {
            2 => out.push(d, if t != 0 { c } else { doppler_forward(c) }, t),
            6 => out.push(d, if t != 0 { c } else { doppler_backward(c) }, t),
            _ => {}
        },
        Piece::Splitter => match r {
            0 | 4 => {}
            2 | 6 => out.push(d, c, t),
            _ => {
                out.push(d, c, 0);
                out.push(mirrored, c, 0);
            }
        },
        Piece::Tangler => {
            if r == 2 {
                for bit in [BLUE, GREEN, RED] {
                    if c & bit != 0 {
                        out.push(p & 7, bit, *next_tag);
                        out.push(turn(p, 4), bit, *next_tag);
                        *next_tag += 1;
                    }
                }
            }
        }
        Piece::Conduit => {
            if r == 0 || r == 4 {
                out.push(d, c, t);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::WHITE;

    fn cell(piece: Piece, colour: Colour, dir: Dir) -> Cell {
        Cell { piece, colour, dir }
    }

    /// (dir, colour, tag) triples for a beam entering `cell`, with the tag counter at 5.
    fn run(cell: Cell, d: Dir, c: Colour, t: i32) -> Vec<(Dir, Colour, i32)> {
        let mut next_tag = 5;
        transfer(cell, d, c, t, &mut next_tag)
            .as_slice()
            .iter()
            .map(|e| (e.dir, e.colour, e.tag))
            .collect()
    }

    /// Checks a one-output rule written as logic.md writes it: for each r, `Some(k)` = out at p + k.
    fn check_turns(piece: Piece, table: [Option<u8>; 8]) {
        for p in 0..8 {
            for r in 0..8 {
                let d = turn(p, r);
                let expected: Vec<_> = table[usize::from(r)]
                    .map(|k| (turn(p, k), RED, 3))
                    .into_iter()
                    .collect();
                assert_eq!(
                    run(cell(piece, 0, p), d, RED, 3),
                    expected,
                    "{piece:?} p={p} r={r}"
                );
            }
        }
    }

    #[test]
    fn a_raw_dir_of_8_or_9_acts_as_dir_0_or_1() {
        // An original save can hold dir 8 or 9 from an unchecked paste (platform.md P8).
        let movable = [
            Piece::Mirror,
            Piece::Bender,
            Piece::Filter,
            Piece::Prism,
            Piece::Doppler,
        ];
        for piece in movable.into_iter().chain([Piece::Splitter, Piece::Tangler]) {
            for p in [8, 9] {
                for d in 0..8 {
                    assert_eq!(
                        run(cell(piece, GREEN, p), d, WHITE, 0),
                        run(cell(piece, GREEN, p & 7), d, WHITE, 0),
                        "{piece:?} p={p} d={d}"
                    );
                }
            }
        }
    }

    #[test]
    fn empty_and_pinwheel_pass_everything() {
        for piece in [Piece::Empty, Piece::Pinwheel] {
            for d in 0..8 {
                assert_eq!(run(cell(piece, 4, 3), d, WHITE, 2), [(d, WHITE, 2)]);
            }
        }
    }

    #[test]
    fn walls_lasers_and_teleporters_emit_nothing_here() {
        for piece in [Piece::Wall, Piece::Laser, Piece::Teleporter] {
            for d in 0..8 {
                assert_eq!(run(cell(piece, 4, 0), d, WHITE, 0), []);
            }
        }
    }

    #[test]
    fn mirror_reflects_from_one_side() {
        check_turns(
            Piece::Mirror,
            [None, Some(7), Some(6), Some(5), None, None, None, None],
        );
    }

    #[test]
    fn level_1_mirror_turns_east_into_north() {
        assert_eq!(run(cell(Piece::Mirror, 4, 1), 2, RED, 0), [(0, RED, 0)]);
    }

    #[test]
    fn bender_reflects_at_a_half_step() {
        check_turns(
            Piece::Bender,
            [Some(7), Some(6), Some(5), Some(4), None, None, None, None],
        );
    }

    #[test]
    fn conduit_passes_along_its_axis() {
        for p in 0..8 {
            for r in 0..8 {
                let d = turn(p, r);
                let expected: Vec<_> = if r == 0 || r == 4 {
                    vec![(d, RED, 1)]
                } else {
                    vec![]
                };
                assert_eq!(
                    run(cell(Piece::Conduit, 0, p), d, RED, 1),
                    expected,
                    "p={p} r={r}"
                );
            }
        }
    }

    #[test]
    fn filter_masks_colour_on_its_axis() {
        for p in 0..8 {
            for r in 0..8 {
                let d = turn(p, r);
                let expected: Vec<_> = if r == 2 || r == 6 {
                    vec![(d, GREEN, 4)]
                } else {
                    vec![]
                };
                assert_eq!(
                    run(cell(Piece::Filter, GREEN, p), d, WHITE, 4),
                    expected,
                    "p={p} r={r}"
                );
            }
        }
        assert_eq!(
            run(cell(Piece::Filter, GREEN, 0), 2, RED | BLUE, 0),
            [],
            "nothing left, no beam"
        );
    }

    #[test]
    fn prism_fans_white_out_and_merges_it_back() {
        // r = 2: red straight, green +45°, blue +90°.
        assert_eq!(
            run(cell(Piece::Prism, 0, 0), 2, WHITE, 7),
            [(2, RED, 7), (3, GREEN, 7), (4, BLUE, 7)]
        );
        // r = 5 from p = 0: red straight on, green −45°, blue −90°.
        assert_eq!(
            run(cell(Piece::Prism, 0, 0), 5, WHITE, 0),
            [(5, RED, 0), (4, GREEN, 0), (3, BLUE, 0)]
        );
        // The fan run backwards leaves at p + 6.
        assert_eq!(
            run(cell(Piece::Prism, 0, 1), turn(1, 6), RED, 0),
            [(turn(1, 6), RED, 0)]
        );
        assert_eq!(
            run(cell(Piece::Prism, 0, 1), turn(1, 7), GREEN, 0),
            [(turn(1, 6), GREEN, 0)]
        );
        assert_eq!(
            run(cell(Piece::Prism, 0, 1), turn(1, 0), BLUE, 0),
            [(turn(1, 6), BLUE, 0)]
        );
        // Yellow at r = 1 keeps only its red.
        assert_eq!(
            run(cell(Piece::Prism, 0, 0), 1, RED | GREEN, 0),
            [(1, RED, 0)]
        );
    }

    #[test]
    fn prism_matches_the_full_table() {
        let table = [
            (
                RED,
                [None, Some(1), Some(2), None, None, Some(5), Some(6), None],
            ),
            (
                GREEN,
                [Some(1), None, Some(3), None, None, Some(4), None, Some(6)],
            ),
            (
                BLUE,
                [Some(6), None, Some(4), None, None, Some(3), None, Some(1)],
            ),
        ];
        for (bit, outputs) in table {
            check_turns_colour(bit, outputs);
        }
    }

    fn check_turns_colour(bit: Colour, table: [Option<u8>; 8]) {
        for p in 0..8 {
            for r in 0..8 {
                let expected: Vec<_> = table[usize::from(r)]
                    .map(|k| (turn(p, k), bit, 0))
                    .into_iter()
                    .collect();
                assert_eq!(
                    run(cell(Piece::Prism, 0, p), turn(p, r), bit, 0),
                    expected,
                    "bit {bit} p={p} r={r}"
                );
            }
        }
    }

    #[test]
    fn doppler_shifts_colour_forward_and_back() {
        assert_eq!([RED, GREEN, BLUE].map(doppler_forward), [GREEN, BLUE, RED]);
        assert_eq!([RED, GREEN, BLUE].map(doppler_backward), [BLUE, RED, GREEN]);
        assert_eq!(doppler_forward(RED | GREEN), GREEN | BLUE);
        assert_eq!(doppler_forward(WHITE), WHITE);
        for p in 0..8 {
            for r in 0..8 {
                let d = turn(p, r);
                let expected: Vec<_> = match r {
                    2 => vec![(d, GREEN, 0)],
                    6 => vec![(d, BLUE, 0)],
                    _ => vec![],
                };
                assert_eq!(
                    run(cell(Piece::Doppler, 0, p), d, RED, 0),
                    expected,
                    "p={p} r={r}"
                );
            }
        }
    }

    #[test]
    fn doppler_leaves_entangled_beams_to_the_entanglement_pass() {
        assert_eq!(run(cell(Piece::Doppler, 0, 0), 2, RED, 9), [(2, RED, 9)]);
        assert_eq!(run(cell(Piece::Doppler, 0, 0), 6, RED, 9), [(6, RED, 9)]);
    }

    #[test]
    fn splitter_passes_head_on_and_splits_at_45_degrees() {
        for p in 0..8 {
            let at = |r: u8| run(cell(Piece::Splitter, 0, p), turn(p, r), RED, 3);
            assert_eq!(at(0), [], "parallel");
            assert_eq!(at(4), [], "parallel");
            assert_eq!(at(2), [(turn(p, 2), RED, 3)], "head-on keeps the tag");
            assert_eq!(at(6), [(turn(p, 6), RED, 3)], "head-on keeps the tag");
            assert_eq!(at(1), [(turn(p, 1), RED, 0), (turn(p, 7), RED, 0)]);
            assert_eq!(at(3), [(turn(p, 3), RED, 0), (turn(p, 5), RED, 0)]);
            assert_eq!(at(5), [(turn(p, 5), RED, 0), (turn(p, 3), RED, 0)]);
            assert_eq!(at(7), [(turn(p, 7), RED, 0), (turn(p, 1), RED, 0)]);
        }
    }

    #[test]
    fn tangler_emits_a_fresh_tagged_pair_per_colour_bit() {
        let mut next_tag = 5;
        let out = transfer(cell(Piece::Tangler, 0, 1), 3, WHITE, 2, &mut next_tag);
        let got: Vec<_> = out
            .as_slice()
            .iter()
            .map(|e| (e.dir, e.colour, e.tag))
            .collect();
        assert_eq!(
            got,
            [
                (1, BLUE, 5),
                (5, BLUE, 5),
                (1, GREEN, 6),
                (5, GREEN, 6),
                (1, RED, 7),
                (5, RED, 7)
            ]
        );
        assert_eq!(next_tag, 8);
        for r in [0, 1, 3, 4, 5, 6, 7] {
            assert_eq!(
                run(cell(Piece::Tangler, 0, 1), turn(1, r), RED, 0),
                [],
                "r={r}"
            );
        }
    }
}
