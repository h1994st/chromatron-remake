//! `beamline`: what the board's beams look like as geometry (spec §7.3). The half-segment —
//! cell `i`, side `s`, colour `outbound[i][s] | inbound[i][(s + 4) & 7]` (classic's
//! `draw_cell_beams`) — is the unit; collinear neighbours of one colour merge into runs. Glints
//! mark crossings (classic's centre-dot rule) and bounces. `Reach` (§9.2) is each lit cell's
//! distance from a laser along the beams. Fixed arrays: no allocation after `new`.

use lumen::{BOARD_CELLS, BeamField, Board, Colour, DX, DY, Piece, RuleSet, SIDE};

pub const MAX_RUNS: usize = BOARD_CELLS * 8;

/// The colour of half-segment (`cell`, `side`).
pub fn half_colour(field: &BeamField, cell: usize, side: usize) -> Colour {
    field.outbound[cell][side] | field.inbound[cell][(side + 4) & 7]
}

/// A straight stretch of one colour. Ends are in half-cell units from the board's top-left:
/// cell (x, y)'s centre is (2x + 1, 2y + 1), its sides' midpoints and corners one unit away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub from: (i16, i16),
    pub to: (i16, i16),
    pub mask: Colour,
    pub from_end: End,
    pub to_end: End,
    /// Its walk position: line `(forward, start)` and the half-segment it opens on,
    /// `((forward · BOARD_CELLS + start) << 6) | step`; strictly increasing along `runs()`, so two
    /// frames' runs merge in one pass.
    pub order: u32,
}

/// Where a run's end is drawn: at its point, or cut back from a cell centre to a laser's
/// aperture or a wall's slab face (drawing only; the field still runs to the centre).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum End {
    #[default]
    Open,
    Laser,
    Wall,
}

impl End {
    /// How far short of the end point, in u, the line's end sits. The aperture is 0.25u out.
    /// The wall's 0.8u slab face is 0.4u out (about 0.52u along a diagonal, at its rounded
    /// corner); the line ends a round cap (0.032u) further out, so its tip touches the face.
    pub fn inset(self, diagonal: bool) -> f32 {
        match (self, diagonal) {
            (End::Open, _) => 0.0,
            (End::Laser, _) => 0.25,
            (End::Wall, false) => 0.432,
            (End::Wall, true) => 0.56,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlintKind {
    Crossing,
    Bounce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glint {
    pub cell: usize,
    pub mask: Colour,
    pub kind: GlintKind,
}

/// Pieces of half-segments a sweep is drawing: fractions `from`..`to` of half (`cell`, `side`)
/// (0 at the cell's centre, 1 at its edge point, already trimmed at an aperture or a wall face),
/// with the bits over it. `order` is the half's walk position × 4 + the piece's index, so two
/// frames' pieces merge in one pass.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Partial {
    pub order: u32,
    pub cell: u16,
    pub side: u8,
    pub from: f32,
    pub to: f32,
    pub mask: Colour,
}

pub const MAX_PARTIALS: usize = BOARD_CELLS * 8;

/// What one half-segment shows mid-sweep: whole in `steady` (it merges into runs), or as up to
/// four `pieces` (then `steady` is 0); and where each fresh channel's head is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HalfState {
    pub steady: Colour,
    pub pieces: [(f32, f32, Colour); 4],
    pub len: usize,
    pub heads: [(f32, Colour); 3],
    pub head_len: usize,
}

/// The fraction of half (`cell`, `side`) cut at the cell's centre: to a laser's aperture on the
/// side it emits from, to a wall's face (as the runs' ends are cut, `End::inset`).
pub fn centre_trim(board: &Board, field: &BeamField, cell: usize, side: usize) -> f32 {
    let diagonal = side % 2 == 1;
    let half = if diagonal {
        std::f32::consts::FRAC_1_SQRT_2
    } else {
        0.5
    };
    (end_at(board, field, cell, side).inset(diagonal) / half).min(1.0)
}

/// Where a run ends at `cell`'s centre on `side`: at a laser's aperture on its emitting side, at
/// a wall's face (never through the wall).
fn end_at(board: &Board, field: &BeamField, cell: usize, side: usize) -> End {
    match board.cells[cell].piece {
        Piece::Laser if field.outbound[cell][side] != 0 => End::Laser,
        Piece::Wall => End::Wall,
        _ => End::Open,
    }
}

impl SweepView<'_> {
    /// Whether the front has reached `cell`: always, unless light new to it enters or leaves it.
    pub fn arrived(&self, new: &BeamField, cell: usize) -> bool {
        let lit = |f: &BeamField| f.received(cell) | f.outbound[cell].iter().fold(0, |m, &c| m | c);
        lit(new) & !lit(self.old) == 0 || self.front >= f32::from(self.reach.cells[cell])
    }

    /// What half (`cell`, `side`) of `new` shows at this front (spec §9.1 "Beams", per channel):
    /// bits in both fields are steady; a fresh bit is drawn from where its strand starts
    /// (`strand_start`) over the part the front has passed; bits only in the old field aren't in
    /// the light at all (they fade over the output: `flourish`).
    pub fn half(&self, new: &BeamField, cell: usize, side: usize) -> HalfState {
        let (now, then) = (
            half_colour(new, cell, side),
            half_colour(self.old, cell, side),
        );
        let mut state = HalfState {
            steady: now & then,
            pieces: [(0.0, 0.0, 0); 4],
            len: 0,
            heads: [(0.0, 0); 3],
            head_len: 0,
        };
        let mut spans = [(0u8, 0f32, 0f32); 3];
        let mut m = 0;
        let mut cuts = [0f32, 1.0, 0.0, 0.0, 0.0];
        let mut n = 2;
        for bit in [1u8, 2, 4] {
            if now & !then & bit == 0 {
                continue;
            }
            let (start, inward) = strand_start(new, self.reach, cell, side, bit);
            let passed = (self.front - start) * 2.0;
            let g = passed.clamp(0.0, 1.0);
            if g >= 1.0 {
                state.steady |= bit;
                // The front exactly at this half's far end: the head sits there, or a frame
                // landing on a half boundary would show no head at all.
                if passed == 1.0 {
                    state.heads[state.head_len] = (if inward { 0.0 } else { 1.0 }, bit);
                    state.head_len += 1;
                }
                continue;
            }
            if g <= 0.0 {
                continue;
            }
            let (a, b) = if inward { (1.0 - g, 1.0) } else { (0.0, g) };
            let head = if inward { a } else { b };
            spans[m] = (bit, a, b);
            m += 1;
            cuts[n] = head;
            n += 1;
            state.heads[state.head_len] = (head, bit);
            state.head_len += 1;
        }
        if m == 0 {
            return state;
        }
        cuts[..n].sort_unstable_by(f32::total_cmp);
        for w in cuts[..n].windows(2) {
            let (a, b) = (w[0], w[1]);
            if b <= a {
                continue;
            }
            let mid = 0.5 * (a + b);
            let mask = spans[..m]
                .iter()
                .filter(|s| s.1 <= mid && mid <= s.2)
                .fold(state.steady, |k, s| k | s.0);
            if mask == 0 {
                continue;
            }
            match state.pieces[..state.len].last_mut() {
                Some(last) if last.2 == mask && last.1 == a => last.1 = b,
                _ => {
                    state.pieces[state.len] = (a, b, mask);
                    state.len += 1;
                }
            }
        }
        state.steady = 0;
        state
    }
}

/// Calls `f(cell, side, at, bit)` for each moving head of the sweep at `view`, `at` the head's
/// fraction of its half, never inside a trimmed centre.
pub fn for_each_head(
    view: &SweepView,
    board: &Board,
    field: &BeamField,
    mut f: impl FnMut(usize, usize, f32, Colour),
) {
    for cell in 0..BOARD_CELLS {
        for side in 0..8 {
            if half_colour(field, cell, side) & !half_colour(view.old, cell, side) == 0 {
                continue;
            }
            let h = view.half(field, cell, side);
            if h.head_len == 0 {
                continue;
            }
            let trim = centre_trim(board, field, cell, side);
            for &(at, bit) in &h.heads[..h.head_len] {
                f(cell, side, at.max(trim), bit);
            }
        }
    }
}

pub struct Beamline {
    runs: [Run; MAX_RUNS],
    run_len: usize,
    glints: [Glint; BOARD_CELLS],
    glint_len: usize,
    partials: [Partial; MAX_PARTIALS],
    partial_len: usize,
}

impl Beamline {
    pub fn new() -> Box<Beamline> {
        Box::new(Beamline {
            runs: [Run::default(); MAX_RUNS],
            run_len: 0,
            glints: [Glint {
                cell: 0,
                mask: 0,
                kind: GlintKind::Crossing,
            }; BOARD_CELLS],
            glint_len: 0,
            partials: [Partial::default(); MAX_PARTIALS],
            partial_len: 0,
        })
    }

    pub fn runs(&self) -> &[Run] {
        &self.runs[..self.run_len]
    }

    pub fn glints(&self) -> &[Glint] {
        &self.glints[..self.glint_len]
    }

    /// The pieces of half-segments a sweep is drawing, in walk order (none when settled).
    pub fn partials(&self) -> &[Partial] {
        &self.partials[..self.partial_len]
    }

    /// Recomputes runs and glints, settled. `hidden` is the cell a piece is being dragged out
    /// of: beams still reflect there (as classic), but it gets no bounce glint.
    pub fn rebuild(&mut self, board: &Board, field: &BeamField, hidden: Option<usize>) {
        self.rebuild_at(board, field, hidden, None);
    }

    /// Recomputes runs, glints and partials as a sweep at `view` shows `field` (settled with
    /// none): halves the front has passed merge into runs, halves it is crossing are drawn in
    /// pieces, and a cell gets its glint once the front reaches it.
    pub fn rebuild_at(
        &mut self,
        board: &Board,
        field: &BeamField,
        hidden: Option<usize>,
        view: Option<&SweepView>,
    ) {
        self.run_len = 0;
        self.glint_len = 0;
        self.partial_len = 0;
        for_each_line(|forward, start| {
            let (dx, dy) = (i32::from(DX[forward]), i32::from(DY[forward]));
            self.walk_line(board, field, view, start, (dx, dy), forward, forward + 4);
        });
        for cell in 0..BOARD_CELLS {
            if view.is_some_and(|v| !v.arrived(field, cell)) {
                continue;
            }
            if let Some(glint) = glint_at(board, field, cell, hidden) {
                self.glints[self.glint_len] = glint;
                self.glint_len += 1;
            }
        }
    }

    /// One line of cells from the edge cell `start`: back half, front half, next cell's back
    /// half, … (at most 30 halves, so `step` < 64). Mid-sweep a half's mask is what the sweep
    /// shows whole of it, and its pieces become partials.
    #[allow(clippy::too_many_arguments)]
    fn walk_line(
        &mut self,
        board: &Board,
        field: &BeamField,
        view: Option<&SweepView>,
        start: usize,
        (dx, dy): (i32, i32),
        forward: usize,
        back: usize,
    ) {
        let (mut x, mut y) = ((start % SIDE) as i32, (start / SIDE) as i32);
        let line = ((forward * BOARD_CELLS + start) as u32) << 6;
        let mut step = 0u32;
        let mut open: Option<Run> = None;
        while on_board(x, y) {
            let cell = (y as usize) * SIDE + x as usize;
            let centre = ((2 * x + 1) as i16, (2 * y + 1) as i16);
            for (side, from, to) in [
                (back, (centre.0 - dx as i16, centre.1 - dy as i16), centre),
                (
                    forward,
                    centre,
                    (centre.0 + dx as i16, centre.1 + dy as i16),
                ),
            ] {
                let (mask, pieces) = match view {
                    None => (half_colour(field, cell, side), None),
                    Some(v) => {
                        let h = v.half(field, cell, side);
                        (h.steady, (h.len > 0).then_some(h))
                    }
                };
                if let Some(h) = pieces {
                    let trim = centre_trim(board, field, cell, side);
                    let base = (line | step) << 2;
                    for (k, &(from, to, bits)) in h.pieces[..h.len].iter().enumerate() {
                        let from = from.max(trim);
                        if from < to && self.partial_len < MAX_PARTIALS {
                            self.partials[self.partial_len] = Partial {
                                order: base | k as u32,
                                cell: cell as u16,
                                side: side as u8,
                                from,
                                to,
                                mask: bits,
                            };
                            self.partial_len += 1;
                        }
                    }
                }
                let end = end_at(board, field, cell, side);
                let starts_here = end != End::Open && side == forward;
                match open.as_mut() {
                    Some(run) if run.mask == mask && mask != 0 && !starts_here => run.to = to,
                    _ => {
                        if let Some(run) = open.take() {
                            self.push(run);
                        }
                        if mask != 0 {
                            open = Some(Run {
                                from,
                                to,
                                mask,
                                from_end: if starts_here { end } else { End::Open },
                                to_end: End::Open,
                                order: line | step,
                            });
                        }
                    }
                }
                step += 1;
                if end != End::Open
                    && side == back
                    && let Some(mut run) = open.take()
                {
                    run.to_end = end;
                    self.push(run);
                }
            }
            (x, y) = (x + dx, y + dy);
        }
        if let Some(run) = open {
            self.push(run);
        }
    }

    fn push(&mut self, run: Run) {
        self.runs[self.run_len] = run;
        self.run_len += 1;
    }

    /// Empties it: no runs, no glints, no partials.
    pub(crate) fn clear(&mut self) {
        self.run_len = 0;
        self.glint_len = 0;
        self.partial_len = 0;
    }

    #[cfg(test)]
    pub(crate) fn push_run(&mut self, run: Run) {
        self.push(run);
    }

    #[cfg(test)]
    pub(crate) fn push_partial(&mut self, p: Partial) {
        self.partials[self.partial_len] = p;
        self.partial_len += 1;
    }

    #[cfg(test)]
    pub(crate) fn push_glint(&mut self, g: Glint) {
        self.glints[self.glint_len] = g;
        self.glint_len += 1;
    }
}

pub(crate) fn on_board(x: i32, y: i32) -> bool {
    (0..SIDE as i32).contains(&x) && (0..SIDE as i32).contains(&y)
}

/// Calls `f(forward, start)` for every line of cells across the board: each of the four forward
/// directions (`DX`/`DY` 0–3; its back is `forward + 4`) from each edge cell `start` that has no
/// cell behind it. Every half-segment lies on exactly one line.
pub(crate) fn for_each_line(mut f: impl FnMut(usize, usize)) {
    for forward in 0..4 {
        let (dx, dy) = (i32::from(DX[forward]), i32::from(DY[forward]));
        for start in 0..BOARD_CELLS {
            let (sx, sy) = ((start % SIDE) as i32, (start / SIDE) as i32);
            if !on_board(sx - dx, sy - dy) {
                f(forward, start);
            }
        }
    }
}

/// Spec §9.2: each cell's distance, in cells, from a laser along the beams. A sweep's front
/// reaches a strand at `at + (reach − r_min) / speed`. `guessed` counts lit cells no laser or
/// teleporter accounts for (given 0); the original levels have none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reach {
    pub cells: [u16; BOARD_CELLS],
    pub guessed: u16,
}

impl Default for Reach {
    fn default() -> Reach {
        Reach {
            cells: [0; BOARD_CELLS],
            guessed: 0,
        }
    }
}

/// Not reached yet.
const FAR: u16 = u16::MAX;

fn lit_out(field: &BeamField, cell: usize) -> bool {
    field.outbound[cell].iter().any(|&m| m != 0)
}

impl Reach {
    /// A BFS from the lasers (0), with the rules of `board`'s game. Its edges are the lit
    /// neighbour steps `c → n = c + DIR[s]` with `outbound[c][s] & inbound[n][s] ≠ 0`, and the
    /// teleport jumps: a dequeued teleporter that took light travelling `s` reaches the next
    /// teleport target along `s` (lumen's tracer jumps it there) where that light leaves. So
    /// `reach[n] ≤ reach[c] + 1` along every edge. A cell that emits light and is still
    /// unreached takes 0 (counted in `guessed`), lowest index first, and the search goes on from
    /// it. Fixed arrays.
    pub fn compute(board: &Board, field: &BeamField) -> Reach {
        let rules = board.rules();
        let mut search = Search::new();
        let mut guessed = 0;
        for c in 0..BOARD_CELLS {
            if board.cells[c].piece == Piece::Laser && lit_out(field, c) {
                search.reach(c, 0);
            }
        }
        // Every cell below `unguessed` is reached or emits nothing.
        let mut unguessed = 0;
        loop {
            while let Some(c) = search.next() {
                let next = search.cells[c] + 1;
                let (x, y) = ((c % SIDE) as i32, (c / SIDE) as i32);
                // As lumen's `enter_cell`: either kind of teleporter jumps what enters it.
                let teleporter = matches!(
                    board.cells[c].piece,
                    Piece::Teleporter | Piece::MobileTeleporter
                );
                for s in 0..8 {
                    let (nx, ny) = (x + i32::from(DX[s]), y + i32::from(DY[s]));
                    if on_board(nx, ny) {
                        let n = ny as usize * SIDE + nx as usize;
                        if field.outbound[c][s] & field.inbound[n][s] != 0 {
                            search.reach(n, next);
                        }
                    }
                    let entered = field.inbound[c][s];
                    if teleporter
                        && entered != 0
                        && let Some(n) = teleport_landing(board, rules, c, s)
                        && field.outbound[n][s] & entered != 0
                    {
                        search.reach(n, next);
                    }
                }
            }
            let Some(c) =
                (unguessed..BOARD_CELLS).find(|&c| search.cells[c] == FAR && lit_out(field, c))
            else {
                break;
            };
            unguessed = c + 1;
            guessed += 1;
            search.reach(c, 0);
        }
        Reach {
            cells: search.cells.map(|r| if r == FAR { 0 } else { r }),
            guessed,
        }
    }
}

/// `Reach::compute`'s breadth-first frontier. Every cell is queued at most once, when first
/// reached, so `BOARD_CELLS` slots hold the whole search.
struct Search {
    cells: [u16; BOARD_CELLS],
    queue: [u16; BOARD_CELLS],
    head: usize,
    tail: usize,
}

impl Search {
    fn new() -> Search {
        Search {
            cells: [FAR; BOARD_CELLS],
            queue: [0; BOARD_CELLS],
            head: 0,
            tail: 0,
        }
    }

    /// Gives `cell` reach `r` and queues it, unless it is already reached.
    fn reach(&mut self, cell: usize, r: u16) {
        if self.cells[cell] == FAR {
            self.cells[cell] = r;
            self.queue[self.tail] = cell as u16;
            self.tail += 1;
        }
    }

    fn next(&mut self) -> Option<usize> {
        (self.head < self.tail).then(|| {
            self.head += 1;
            usize::from(self.queue[self.head - 1])
        })
    }
}

/// Where bit `bit` of half-segment (`cell`, `side`) starts being drawn by a sweep, in reach
/// units (cells from the lasers): light leaving the cell starts at its centre at the cell's
/// reach; light entering it starts at its edge half a cell after the neighbour it comes from.
/// `true` when it enters.
pub fn strand_start(
    field: &BeamField,
    reach: &Reach,
    cell: usize,
    side: usize,
    bit: Colour,
) -> (f32, bool) {
    if field.outbound[cell][side] & bit != 0 {
        return (f32::from(reach.cells[cell]), false);
    }
    let (x, y) = (
        (cell % SIDE) as i32 + i32::from(DX[side]),
        (cell / SIDE) as i32 + i32::from(DY[side]),
    );
    let from = if on_board(x, y) {
        reach.cells[y as usize * SIDE + x as usize]
    } else {
        reach.cells[cell]
    };
    (f32::from(from) + 0.5, true)
}

/// A sweep at one moment: the field it started from, the new field's reach, and its front
/// (reach units). Task 6 gives it the half-segment geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepView<'c> {
    pub old: &'c BeamField,
    pub reach: &'c Reach,
    pub front: f32,
}

/// lumen's `teleport_target`: the next cell from `cell` along `dir` holding one of the game's
/// teleport targets, skipping every other cell (walls included).
fn teleport_landing(board: &Board, rules: &RuleSet, cell: usize, dir: usize) -> Option<usize> {
    let (mut x, mut y) = ((cell % SIDE) as i32, (cell / SIDE) as i32);
    loop {
        (x, y) = (x + i32::from(DX[dir]), y + i32::from(DY[dir]));
        if !on_board(x, y) {
            return None;
        }
        let n = y as usize * SIDE + x as usize;
        if rules.teleport_targets.contains(&board.cells[n].piece) {
            return Some(n);
        }
    }
}

fn glint_at(board: &Board, field: &BeamField, cell: usize, hidden: Option<usize>) -> Option<Glint> {
    let (inbound, outbound) = (&field.inbound[cell], &field.outbound[cell]);
    let (mut axes, mut dot) = (0u8, 0u8);
    for s in 0..8 {
        // Classic's `draw_cell_beams`: only drawn sides count, and a side counts as straight
        // when in == out, both zero included.
        if half_colour(field, cell, s) == 0 {
            continue;
        }
        if inbound[s] == outbound[s] {
            axes |= 1 << (s & 3);
            dot |= inbound[s];
        }
    }
    if dot != 0 && axes.count_ones() >= 2 {
        return Some(Glint {
            cell,
            mask: dot,
            kind: GlintKind::Crossing,
        });
    }
    let piece = board.cells[cell].piece;
    if matches!(piece, Piece::Empty | Piece::Laser | Piece::Wall) || hidden == Some(cell) {
        return None;
    }
    let bounce = (0..8)
        .filter(|&s| inbound[s] == 0)
        .fold(0, |m, s| m | outbound[s]);
    (bounce != 0).then_some(Glint {
        cell,
        mask: bounce,
        kind: GlintKind::Bounce,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen::{Cell, board_index, trace};

    fn board(pieces: &[(usize, usize, Piece, u8, u8)]) -> (Board, BeamField) {
        let mut b = Board::default();
        for &(x, y, piece, colour, dir) in pieces {
            b.cells[board_index(x, y)] = Cell { piece, colour, dir };
        }
        let mut field = BeamField::default();
        trace(&b, &mut field);
        (b, field)
    }

    /// Marks every half-segment a run covers, walking it in half-cell steps.
    fn painted(line: &Beamline) -> Vec<[Colour; 8]> {
        let mut out = vec![[0u8; 8]; BOARD_CELLS];
        for run in line.runs() {
            let (dx, dy) = (
                (run.to.0 - run.from.0).signum(),
                (run.to.1 - run.from.1).signum(),
            );
            let side = DX
                .iter()
                .zip(DY.iter())
                .position(|(&x, &y)| i16::from(x) == dx && i16::from(y) == dy)
                .unwrap();
            let (mut x, mut y) = run.from;
            while (x, y) != run.to {
                let (nx, ny) = (x + dx, y + dy);
                // The half from (x, y) to (nx, ny) belongs to the cell whose centre is one of them.
                let (centre, s) = if x % 2 == 1 && y % 2 == 1 {
                    ((x, y), side)
                } else {
                    ((nx, ny), (side + 4) & 7)
                };
                let cell = board_index((centre.0 / 2) as usize, (centre.1 / 2) as usize);
                out[cell][s] |= run.mask;
                (x, y) = (nx, ny);
            }
        }
        out
    }

    #[test]
    fn runs_cover_exactly_classics_half_segments() {
        let (b, field) = board(&[
            (2, 7, Piece::Laser, 4, 2),
            (8, 7, Piece::Mirror, 4, 1),
            (8, 2, Piece::Pinwheel, 4, 0),
            (1, 1, Piece::Laser, 2, 3),
            (6, 1, Piece::Laser, 1, 4),
        ]);
        let mut line = Beamline::new();
        line.rebuild(&b, &field, None);
        let drawn = painted(&line);
        for (cell, sides) in drawn.iter().enumerate() {
            for (s, &colour) in sides.iter().enumerate() {
                assert_eq!(colour, half_colour(&field, cell, s), "cell {cell} side {s}");
            }
        }
        assert!(
            line.runs()
                .iter()
                .any(|r| r.from_end == End::Laser || r.to_end == End::Laser),
            "a laser's run is trimmed to its aperture"
        );
    }

    #[test]
    fn runs_come_in_walk_order() {
        let (b, field) = board(&[
            (0, 0, Piece::Laser, 7, 3),
            (7, 7, Piece::Splitter, 0, 1),
            (12, 2, Piece::Mirror, 0, 2),
        ]);
        let mut line = Beamline::new();
        line.rebuild(&b, &field, None);
        assert!(
            line.runs().windows(2).all(|w| w[0].order < w[1].order),
            "orders strictly increase"
        );
    }

    #[test]
    fn a_straight_beam_is_one_run_per_colour_stretch() {
        let (b, field) = board(&[(0, 4, Piece::Laser, 4, 2), (14, 4, Piece::Wall, 0, 0)]);
        let mut line = Beamline::new();
        line.rebuild(&b, &field, None);
        let red: Vec<&Run> = line.runs().iter().filter(|r| r.mask == 4).collect();
        assert_eq!(red.len(), 1, "{red:?}");
        assert_eq!(red[0].from, (1, 9));
        assert_eq!((red[0].from_end, red[0].to_end), (End::Laser, End::Wall));
    }

    #[test]
    fn beams_stop_at_a_walls_face_and_never_run_through_it() {
        // Red from the west and from the east, both into the wall at (7, 4).
        let (b, field) = board(&[
            (0, 4, Piece::Laser, 4, 2),
            (14, 4, Piece::Laser, 4, 6),
            (7, 4, Piece::Wall, 0, 0),
        ]);
        let mut line = Beamline::new();
        line.rebuild(&b, &field, None);
        let red: Vec<&Run> = line.runs().iter().filter(|r| r.mask == 4).collect();
        assert_eq!(red.len(), 2, "{red:?}");
        assert_eq!((red[0].to, red[0].to_end), ((15, 9), End::Wall));
        assert_eq!((red[1].from, red[1].from_end), ((15, 9), End::Wall));
        assert!(End::Wall.inset(false) > 0.4 && End::Wall.inset(false) < 0.45);
    }

    #[test]
    fn glints_follow_classics_centre_dot_and_bounces() {
        let (b, field) = board(&[
            (0, 5, Piece::Laser, 4, 2),
            (5, 0, Piece::Laser, 1, 4),
            (10, 5, Piece::Mirror, 4, 1),
        ]);
        let mut line = Beamline::new();
        line.rebuild(&b, &field, None);
        let at = |x, y| {
            line.glints()
                .iter()
                .find(|g| g.cell == board_index(x, y))
                .copied()
        };
        assert_eq!(
            at(5, 5).map(|g| (g.kind, g.mask)),
            Some((GlintKind::Crossing, 5)),
            "red across blue"
        );
        assert_eq!(
            at(10, 5).map(|g| g.kind),
            Some(GlintKind::Bounce),
            "the mirror turns red"
        );
        assert_eq!(at(3, 5), None, "a straight beam has no glint");
        line.rebuild(&b, &field, Some(board_index(10, 5)));
        assert!(
            line.glints().iter().all(|g| g.cell != board_index(10, 5)),
            "no bounce glint on a dragged piece's cell"
        );
    }

    #[test]
    fn an_absorbed_beam_beside_a_straight_one_still_glints_as_in_classic() {
        // Red runs east into a red filter turned so it passes; blue runs south into the same
        // cell edge-on and is absorbed. Classic draws the dot (the absorbed side has in == out == 0).
        let (b, field) = board(&[
            (0, 5, Piece::Laser, 4, 2),
            (5, 0, Piece::Laser, 1, 4),
            (5, 5, Piece::Filter, 4, 0),
        ]);
        let mut line = Beamline::new();
        line.rebuild(&b, &field, None);
        let at = line
            .glints()
            .iter()
            .find(|g| g.cell == board_index(5, 5))
            .copied();
        assert_eq!(
            at.map(|g| g.kind),
            Some(GlintKind::Crossing),
            "{:?} {:?}",
            field.inbound[board_index(5, 5)],
            field.outbound[board_index(5, 5)]
        );
    }

    /// `reach[n] ≤ reach[c] + 1` along every lit edge `c → n` between neighbours.
    fn assert_edges_hold(field: &BeamField, reach: &Reach) {
        for c in 0..BOARD_CELLS {
            for s in 0..8 {
                let (x, y) = (
                    (c % SIDE) as i32 + i32::from(DX[s]),
                    (c / SIDE) as i32 + i32::from(DY[s]),
                );
                if !on_board(x, y) {
                    continue;
                }
                let n = y as usize * SIDE + x as usize;
                if field.outbound[c][s] & field.inbound[n][s] != 0 {
                    assert!(reach.cells[n] <= reach.cells[c] + 1, "{c} → {n}");
                }
            }
        }
    }

    #[test]
    fn reach_counts_cells_from_the_laser() {
        // A blue laser at (2, 5) aiming east (dir 2) at a blue target at (8, 5).
        let (b, field) = board(&[(2, 5, Piece::Laser, 1, 2), (8, 5, Piece::Pinwheel, 1, 0)]);
        let reach = Reach::compute(&b, &field);
        let at = |x: usize, y: usize| reach.cells[board_index(x, y)];
        assert_eq!(at(2, 5), 0, "the laser");
        assert_eq!(at(3, 5), 1);
        assert_eq!(at(8, 5), 6, "the target, six cells on");
        assert_eq!(at(2, 6), 0, "an unlit cell");
        assert_eq!(reach.guessed, 0);
    }

    #[test]
    fn reach_never_grows_by_more_than_one_along_an_edge() {
        let (b, field) = board(&[
            (0, 0, Piece::Laser, 7, 3),
            (7, 7, Piece::Splitter, 0, 1),
            (12, 2, Piece::Mirror, 0, 2),
        ]);
        let reach = Reach::compute(&b, &field);
        assert_edges_hold(&field, &reach);
        assert_eq!(reach.guessed, 0);
    }

    #[test]
    fn a_teleport_landing_takes_its_teleporters_reach_plus_one() {
        // Laser at (1, 7) east; teleporters at (4, 7) and (10, 7): the beam jumps 4 → 10.
        let (b, field) = board(&[
            (1, 7, Piece::Laser, 2, 2),
            (4, 7, Piece::Teleporter, 0, 0),
            (10, 7, Piece::Teleporter, 0, 0),
        ]);
        let reach = Reach::compute(&b, &field);
        let at = |x: usize| reach.cells[board_index(x, 7)];
        assert_eq!(at(4), 3);
        assert_eq!(at(7), 0, "jumped over, unlit");
        assert_eq!(at(10), at(4) + 1, "the landing");
        assert_eq!(at(11), at(10) + 1, "and on from it");
        assert_eq!(reach.guessed, 0);
    }

    #[test]
    fn reach_relaxes_where_a_teleported_beam_joins_a_longer_path() {
        // Green east from (0, 7) jumps (2, 7) → (5, 7) and runs on through (8, 7), which blue,
        // south from (8, 0), reaches only seven cells on. The jump is an edge in the search, so
        // (8, 7) takes the shorter reach and every edge holds.
        let (b, field) = board(&[
            (0, 7, Piece::Laser, 2, 2),
            (2, 7, Piece::Teleporter, 0, 0),
            (5, 7, Piece::Teleporter, 0, 0),
            (8, 0, Piece::Laser, 1, 4),
        ]);
        let reach = Reach::compute(&b, &field);
        let at = |x: usize, y: usize| reach.cells[board_index(x, y)];
        assert_eq!(at(5, 7), 3, "the landing");
        assert_eq!(at(8, 7), 6, "by the jump, not the blue beam's 7");
        assert_eq!(at(8, 6), 6, "blue, six cells on");
        assert_eq!(at(8, 8), 7, "reach is per cell: one on from (8, 7)");
        assert_edges_hold(&field, &reach);
        assert_eq!(reach.guessed, 0);
    }

    #[test]
    fn reach_guesses_zero_for_light_no_laser_or_teleporter_accounts_for() {
        // A hand-made field: cell (3, 3) emits east with no source; (4, 3) takes it on.
        let b = Board::default();
        let mut field = BeamField::default();
        field.outbound[board_index(3, 3)][2] = 4;
        field.inbound[board_index(4, 3)][2] = 4;
        let reach = Reach::compute(&b, &field);
        assert_eq!(reach.guessed, 1);
        assert_eq!(reach.cells[board_index(3, 3)], 0);
        assert_eq!(reach.cells[board_index(4, 3)], 1);
    }

    #[test]
    fn a_half_mid_sweep_is_drawn_in_pieces_and_a_swept_one_is_steady() {
        let old = BeamField::default();
        let (b, new) = board(&[(2, 5, Piece::Laser, 7, 2)]);
        let reach = Reach::compute(&b, &new);
        let cell = board_index(5, 5);
        let east = 2;
        let start = f32::from(reach.cells[cell]);
        let view = |front| SweepView {
            old: &old,
            reach: &reach,
            front,
        };
        let mid = view(start + 0.25).half(&new, cell, east);
        assert_eq!((mid.steady, mid.len), (0, 1), "half drawn: one piece");
        assert_eq!(
            mid.pieces[0],
            (0.0, 0.5, 7),
            "from the centre to half way, white"
        );
        assert_eq!(mid.heads[..mid.head_len].len(), 3, "a head per channel");
        let done = view(start + 0.5).half(&new, cell, east);
        assert_eq!(
            (done.steady, done.len),
            (7, 0),
            "swept: steady, merged into runs"
        );
        let before = view(start - 1.0).half(&new, cell, east);
        assert_eq!((before.steady, before.len), (0, 0), "not reached: nothing");
    }

    #[test]
    fn a_front_on_a_half_boundary_keeps_its_heads_at_the_swept_halfs_end() {
        // White east from (2, 5), the front exactly at the end of (5, 5)'s east half: that half
        // is swept and the next one not begun, and the heads sit where they meet, once each.
        let old = BeamField::default();
        let (b, new) = board(&[(2, 5, Piece::Laser, 7, 2)]);
        let reach = Reach::compute(&b, &new);
        let cell = board_index(5, 5);
        let view = SweepView {
            old: &old,
            reach: &reach,
            front: f32::from(reach.cells[cell]) + 0.5,
        };
        let mut heads = Vec::new();
        for_each_head(&view, &b, &new, |cell, side, at, bit| {
            heads.push((cell, side, at, bit))
        });
        assert_eq!(
            heads,
            [(cell, 2, 1.0, 1), (cell, 2, 1.0, 2), (cell, 2, 1.0, 4)]
        );
        assert_eq!(
            view.half(&new, cell, 2).steady,
            7,
            "the swept half stays steady"
        );
        // Half a cell on, at the end of (6, 5)'s inbound west half: the heads sit at its far
        // end (its centre, never inside the trimmed centre).
        let next = board_index(6, 5);
        let view = SweepView {
            front: f32::from(reach.cells[cell]) + 1.0,
            ..view
        };
        let mut heads = Vec::new();
        for_each_head(&view, &b, &new, |cell, side, at, bit| {
            heads.push((cell, side, at, bit))
        });
        let trim = centre_trim(&b, &new, next, 6);
        assert_eq!(
            heads,
            [(next, 6, trim, 1), (next, 6, trim, 2), (next, 6, trim, 4)]
        );
        assert_eq!(view.half(&new, next, 6).heads[0], (0.0, 1), "at its start");
    }

    #[test]
    fn with_no_view_or_a_passed_front_the_runs_are_the_settled_ones() {
        let (b, new) = board(&[(2, 5, Piece::Laser, 7, 2), (9, 5, Piece::Splitter, 0, 1)]);
        let reach = Reach::compute(&b, &new);
        let old = BeamField::default();
        let (mut settled, mut passed) = (Beamline::new(), Beamline::new());
        settled.rebuild(&b, &new, None);
        passed.rebuild_at(
            &b,
            &new,
            None,
            Some(&SweepView {
                old: &old,
                reach: &reach,
                front: 99.0,
            }),
        );
        assert_eq!(settled.runs(), passed.runs());
        assert_eq!(settled.glints(), passed.glints());
        assert!(passed.partials().is_empty());
    }

    #[test]
    fn a_half_entering_a_cell_is_drawn_from_its_edge_and_trimmed_at_a_laser() {
        // White east from (2, 5): cell (3, 5)'s west half takes light entering from the laser,
        // drawn inward from its edge; the laser's own east half is cut at its aperture.
        let old = BeamField::default();
        let (b, new) = board(&[(2, 5, Piece::Laser, 7, 2)]);
        let reach = Reach::compute(&b, &new);
        let view = SweepView {
            old: &old,
            reach: &reach,
            front: 0.75,
        };
        let west = view.half(&new, board_index(3, 5), 6);
        assert_eq!(
            (west.len, west.pieces[0]),
            (1, (0.5, 1.0, 7)),
            "edge inward"
        );
        assert_eq!(west.heads[0].0, 0.5);
        let laser = board_index(2, 5);
        assert_eq!(
            centre_trim(&b, &new, laser, 2),
            0.5,
            "the aperture at 0.25u"
        );
        assert_eq!(centre_trim(&b, &new, laser, 6), 0.0, "it emits east only");
        // A quarter cell on, the drawn part of the laser's half is still under its aperture: no
        // partial, and the heads sit on the aperture.
        let mut line = Beamline::new();
        let early = SweepView {
            front: 0.25,
            ..view
        };
        line.rebuild_at(&b, &new, None, Some(&early));
        assert!(line.partials().is_empty(), "{:?}", line.partials());
        let mut heads = Vec::new();
        for_each_head(&early, &b, &new, |cell, side, at, bit| {
            heads.push((cell, side, at, bit))
        });
        assert_eq!(
            heads,
            [(laser, 2, 0.5, 1), (laser, 2, 0.5, 2), (laser, 2, 0.5, 4)]
        );
        // Mid-sweep past a splitter, both branches' pieces come in walk order.
        let (b, new) = board(&[(2, 5, Piece::Laser, 7, 2), (9, 5, Piece::Splitter, 0, 1)]);
        let reach = Reach::compute(&b, &new);
        let front = f32::from(reach.cells[board_index(9, 5)]) + 1.25;
        let mid = SweepView {
            old: &old,
            reach: &reach,
            front,
        };
        line.rebuild_at(&b, &new, None, Some(&mid));
        assert!(line.partials().len() >= 2, "{:?}", line.partials());
        assert!(
            line.partials().windows(2).all(|w| w[0].order < w[1].order),
            "orders strictly increase"
        );
    }
}
