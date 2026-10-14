//! `playbill`: what the board's light tile shows in one frame, as data: the beam runs (and,
//! from the sweep, the pieces of half-segments being drawn), the glints, and each cell's piece
//! light. Two frames' playbills are compared item by item, so the darkroom redraws light only
//! where an item changed (ruling M2). Fixed arrays; no allocation after `new`.

use lumen::{BOARD_CELLS, Cell, Colour, Piece};

use crate::beamline::{Beamline, Glint, Partial, Run, SweepView};
use crate::darkroom::{Pose, Scene};
use crate::etch::Pt;

/// One cell's piece light as drawn: whether it shows (not empty, not held), the piece (a piece
/// swapped in place is a change), what it receives, whether it emits, its spin mid-turn, and
/// where it is: its offset from the cell's centre (device px) and its size in cells, (0, 0) and 1
/// but while its piece lands.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellLight {
    pub shown: bool,
    pub cell: Cell,
    pub received: Colour,
    pub emitting: bool,
    pub spin: f32,
    pub offset: Pt,
    pub size: f32,
}

pub struct Playbill {
    pub line: Box<Beamline>,
    pub lights: [CellLight; BOARD_CELLS],
}

/// An item in one frame's playbill that the other frame lacks.
#[derive(Clone, Copy, Debug)]
pub enum Change {
    Run(Run),
    Partial(Partial),
    Glint(Glint),
    /// A cell's piece light, as one of the frames draws it.
    Light(usize, CellLight),
}

impl Playbill {
    pub fn new() -> Box<Playbill> {
        Box::new(Playbill {
            line: Beamline::new(),
            lights: [CellLight::default(); BOARD_CELLS],
        })
    }

    /// Empties it: no runs, no glints, no piece lights (the bill of a frame that showed nothing).
    pub(crate) fn clear(&mut self) {
        self.line.clear();
        self.lights = [CellLight::default(); BOARD_CELLS];
    }

    /// The frame `scene` shows mid-sweep at `view` (settled with none) and at `pose` (the held
    /// piece shows no light; a turning piece's light is at its spin, a landing piece's where its
    /// body is). A cell's piece light shows the old field until the front reaches it.
    pub fn build(&mut self, scene: &Scene, view: Option<&SweepView>, pose: &Pose) {
        let hidden = scene.drag.map(|d| d.source);
        self.line.rebuild_at(scene.board, scene.field, hidden, view);
        for i in 0..BOARD_CELLS {
            let cell = scene.board.cells[i];
            let shown = cell.piece != Piece::Empty && hidden != Some(i);
            let (offset, size) = match pose.landing {
                Some((c, offset, size)) if c == i => (offset, size),
                _ => (Pt::default(), 1.0),
            };
            let field = match view {
                Some(v) if !v.arrived(scene.field, i) => v.old,
                _ => scene.field,
            };
            self.lights[i] = if shown {
                CellLight {
                    shown,
                    cell,
                    received: field.received(i),
                    emitting: field.outbound[i][usize::from(cell.dir & 7)] != 0,
                    spin: pose.turn.filter(|t| t.0 == i).map_or(0.0, |t| t.1),
                    offset,
                    size,
                }
            } else {
                CellLight::default()
            };
        }
    }

    /// Calls `f` with every item one frame has and the other lacks: runs and partials by walk
    /// order, glints by cell, piece lights by cell (each frame's that shows, since a landing
    /// piece's light moves). Equal items at the same place are skipped.
    pub fn changes(&self, last: &Playbill, mut f: impl FnMut(Change)) {
        merge(
            self.line.runs(),
            last.line.runs(),
            |r| r.order,
            |r| f(Change::Run(*r)),
        );
        merge(
            self.line.partials(),
            last.line.partials(),
            |p| p.order,
            |p| f(Change::Partial(*p)),
        );
        merge(
            self.line.glints(),
            last.line.glints(),
            |g| g.cell as u32,
            |g| f(Change::Glint(*g)),
        );
        for i in 0..BOARD_CELLS {
            let (now, then) = (self.lights[i], last.lights[i]);
            if now != then {
                for cl in [now, then] {
                    if cl.shown {
                        f(Change::Light(i, cl));
                    }
                }
            }
        }
    }
}

/// Calls `f` with each item of `a` or `b` that the other lacks at its key, both sorted by `key`
/// (strictly increasing): one pass.
fn merge<T: PartialEq>(a: &[T], b: &[T], key: impl Fn(&T) -> u32, mut f: impl FnMut(&T)) {
    let (mut i, mut j) = (0, 0);
    loop {
        match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) if key(x) == key(y) => {
                if x != y {
                    f(x);
                    f(y);
                }
                i += 1;
                j += 1;
            }
            (Some(x), Some(y)) if key(x) < key(y) => {
                f(x);
                i += 1;
            }
            (Some(_), Some(y)) => {
                f(y);
                j += 1;
            }
            (Some(x), None) => {
                f(x);
                i += 1;
            }
            (None, Some(y)) => {
                f(y);
                j += 1;
            }
            (None, None) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beamline::Reach;
    use lumen::{BeamField, Board, Cell, board_index, trace};
    use spectrograph::Game;

    fn scene_parts(pieces: &[(usize, usize, Piece, u8, u8)]) -> (Board, BeamField) {
        let mut b = Board::default();
        for &(x, y, piece, colour, dir) in pieces {
            b.cells[board_index(x, y)] = Cell { piece, colour, dir };
        }
        let mut field = BeamField::default();
        trace(&b, &mut field);
        (b, field)
    }

    #[test]
    fn a_bill_compared_with_itself_has_no_changes() {
        let (b, f) = scene_parts(&[(2, 5, Piece::Laser, 7, 2), (9, 5, Piece::Mirror, 0, 1)]);
        let (mut one, mut two) = (Playbill::new(), Playbill::new());
        one.build(&Scene::bare(&b, &f, Game::C1), None, &Pose::SETTLED);
        two.build(&Scene::bare(&b, &f, Game::C1), None, &Pose::SETTLED);
        let mut n = 0;
        one.changes(&two, |_| n += 1);
        assert_eq!(n, 0);
    }

    #[test]
    fn mid_sweep_a_cell_shows_its_old_light_until_the_front_arrives() {
        // A blue laser east onto a blue target at (8, 5), lit from nothing.
        let (b, f) = scene_parts(&[(2, 5, Piece::Laser, 1, 2), (8, 5, Piece::Pinwheel, 1, 0)]);
        let (dark, reach) = (BeamField::default(), Reach::compute(&b, &f));
        let target = board_index(8, 5);
        let at = |front| SweepView {
            old: &dark,
            reach: &reach,
            front,
        };
        let (mut mid, mut settled) = (Playbill::new(), Playbill::new());
        let scene = Scene::bare(&b, &f, Game::C1);
        mid.build(&scene, Some(&at(3.25)), &Pose::SETTLED);
        settled.build(&scene, None, &Pose::SETTLED);
        assert_eq!(mid.lights[target].received, 0, "not reached: dark");
        assert_eq!(settled.lights[target].received, 1);
        assert!(!mid.line.partials().is_empty(), "the front is mid-half");
        let mut partials = 0;
        mid.changes(&settled, |c| {
            partials += usize::from(matches!(c, Change::Partial(_)))
        });
        assert_eq!(
            partials,
            mid.line.partials().len(),
            "every piece is a change"
        );
        mid.build(&scene, Some(&at(6.0)), &Pose::SETTLED);
        assert_eq!(mid.lights[target].received, 1, "reached");
    }

    #[test]
    fn a_landing_piece_shows_its_light_where_it_is_and_settles_to_its_cell() {
        // The blue target at (8, 5) landing 3 px right of and 2 px above its cell at 1.05 cells:
        // its light shows there, from the old (dark) field until the front reaches it; landed,
        // it is the settled item exactly, and the change names both places.
        let (b, f) = scene_parts(&[(2, 5, Piece::Laser, 1, 2), (8, 5, Piece::Pinwheel, 1, 0)]);
        let (dark, reach) = (BeamField::default(), Reach::compute(&b, &f));
        let target = board_index(8, 5);
        let at = |front| SweepView {
            old: &dark,
            reach: &reach,
            front,
        };
        let landing = Pose {
            landing: Some((target, Pt { x: 3.0, y: -2.0 }, 1.05)),
            ..Pose::SETTLED
        };
        let scene = Scene::bare(&b, &f, Game::C1);
        let (mut flying, mut settled) = (Playbill::new(), Playbill::new());
        flying.build(&scene, Some(&at(3.25)), &landing);
        let item = flying.lights[target];
        assert!(item.shown, "a landing piece's light shows");
        assert_eq!((item.offset, item.size), (Pt { x: 3.0, y: -2.0 }, 1.05));
        assert_eq!(item.received, 0, "not reached: the old field's");
        flying.build(&scene, Some(&at(6.0)), &landing);
        assert_eq!(flying.lights[target].received, 1, "reached");
        settled.build(&scene, None, &Pose::SETTLED);
        let rest = settled.lights[target];
        assert_eq!(
            (rest.offset, rest.size),
            (Pt::default(), 1.0),
            "settled: its cell's"
        );
        assert_eq!(
            CellLight {
                offset: rest.offset,
                size: rest.size,
                ..flying.lights[target]
            },
            rest,
            "only its place differs"
        );
        let mut places = Vec::new();
        flying.changes(&settled, |c| {
            if let Change::Light(i, cl) = c {
                places.push((i, cl.offset, cl.size));
            }
        });
        assert_eq!(
            places,
            [
                (target, Pt { x: 3.0, y: -2.0 }, 1.05),
                (target, Pt::default(), 1.0)
            ],
            "both places are redrawn"
        );
    }

    #[test]
    fn a_turned_mirror_changes_its_runs_and_light_and_nothing_else() {
        let (b1, f1) = scene_parts(&[
            (2, 5, Piece::Laser, 7, 2),
            (9, 5, Piece::Mirror, 0, 1),
            (2, 12, Piece::Laser, 1, 2),
        ]);
        let (b2, f2) = scene_parts(&[
            (2, 5, Piece::Laser, 7, 2),
            (9, 5, Piece::Mirror, 0, 3),
            (2, 12, Piece::Laser, 1, 2),
        ]);
        let (mut one, mut two) = (Playbill::new(), Playbill::new());
        one.build(&Scene::bare(&b1, &f1, Game::C1), None, &Pose::SETTLED);
        two.build(&Scene::bare(&b2, &f2, Game::C1), None, &Pose::SETTLED);
        let (mut runs, mut lights) = (Vec::new(), Vec::new());
        two.changes(&one, |c| match c {
            Change::Run(r) => runs.push(r),
            Change::Light(i, _) => lights.push(i),
            Change::Glint(_) | Change::Partial(_) => {}
        });
        assert!(lights.contains(&board_index(9, 5)), "the mirror's light");
        assert!(!runs.is_empty(), "its beams");
        // The second laser's row (y = 12) is the same in both: none of its runs changed.
        assert!(
            runs.iter().all(|r| r.from.1 != 25 || r.to.1 != 25),
            "row 12's run is unchanged: {runs:?}"
        );
    }
}
