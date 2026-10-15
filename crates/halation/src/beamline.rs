//! `beamline`: what the board's beams look like as geometry (spec §7.3). The half-segment —
//! cell `i`, side `s`, colour `outbound[i][s] | inbound[i][(s + 4) & 7]` (classic's
//! `draw_cell_beams`) — is the unit; collinear neighbours of one colour merge into runs. Glints
//! mark crossings (classic's centre-dot rule) and bounces, never on a target. `Reach` (§9.2) is how far light has
//! come from a laser along its own beams, per half-beam. Fixed arrays: no allocation after `new`.

use lumen::{BOARD_CELLS, BeamField, Board, Colour, DX, DY, Dir, Piece, RuleSet, SIDE, transfer};

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
/// with the bits over it. `order` is the half's walk position × 8 + the piece's index, so two
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

/// Most moving heads on one half mid-sweep: two strands (one each way) per channel.
pub const MAX_HALF_HEADS: usize = 6;
/// Most pieces of one half mid-sweep: one between each two of its ends and heads.
pub const MAX_HALF_PIECES: usize = MAX_HALF_HEADS + 1;

/// What one half-segment shows mid-sweep: whole in `steady` (it merges into runs), or as up to
/// `MAX_HALF_PIECES` `pieces` (then `steady` is 0); and where each fresh strand's head is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HalfState {
    pub steady: Colour,
    pub pieces: [(f32, f32, Colour); MAX_HALF_PIECES],
    pub len: usize,
    pub heads: [(f32, Colour); MAX_HALF_HEADS],
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
    /// Whether the front has reached `cell`: always, unless light new to one of its halves enters
    /// or leaves it; then once the earliest such light is at its centre.
    pub fn arrived(&self, new: &BeamField, cell: usize) -> bool {
        fresh_arrival(self.old, new, self.reach, cell).is_none_or(|at| self.front >= at)
    }

    /// What `cell` shows it receives at this front, per bit: what both fields deliver, and each
    /// bit new to it once that bit's light is at its centre. A target's arcs fill as each colour
    /// lands, and its core lights at completion, as it bursts. Bits it loses are gone from the
    /// sweep's start, as their strands fade (`flourish`).
    pub fn received(&self, new: &BeamField, cell: usize) -> Colour {
        let (then, now) = (self.old.received(cell), new.received(cell));
        let mut shown = then & now;
        for bit in BITS {
            if now & !then & bit != 0
                && self
                    .reach
                    .received_at(new, cell, bit)
                    .is_none_or(|at| self.front >= at)
            {
                shown |= bit;
            }
        }
        shown
    }

    /// What half (`cell`, `side`) of `new` shows at this front (spec §9.1 "Beams", per channel):
    /// bits in both fields are steady; a fresh bit is drawn from where its strands start
    /// (`strands`) over the part the front has passed (a bit crossing the half both ways from
    /// both ends: whole once either strand is, or once the two meet); bits only in the old field
    /// aren't in the light at all (they fade over the output: `flourish`).
    pub fn half(&self, new: &BeamField, cell: usize, side: usize) -> HalfState {
        let (now, then) = (
            half_colour(new, cell, side),
            half_colour(self.old, cell, side),
        );
        let mut state = HalfState {
            steady: now & then,
            pieces: [(0.0, 0.0, 0); MAX_HALF_PIECES],
            len: 0,
            heads: [(0.0, 0); MAX_HALF_HEADS],
            head_len: 0,
        };
        let mut spans = [(0u8, 0f32, 0f32); MAX_HALF_HEADS];
        let mut m = 0;
        let mut cuts = [0f32; MAX_HALF_HEADS + 2];
        cuts[1] = 1.0;
        let mut n = 2;
        // Second (inbound) strands' heads go after every first strand's, so a half's heads at
        // one point stay side by side (`flourish` merges them in `Over`).
        let mut seconds = [(0f32, 0u8); BITS.len()];
        let mut second_len = 0;
        let swept = |passed: f32| passed.clamp(0.0, 1.0);
        for bit in BITS {
            if now & !then & bit == 0 {
                continue;
            }
            // How far (half lengths) the front has passed each strand's start, and its way.
            let passed = |(start, inward): (f32, bool)| ((self.front - start) * 2.0, inward);
            let pair = strands(new, self.reach, cell, side, bit).map(|strand| strand.map(passed));
            // Whole once either strand is (the parts are clamped, so the sum is at least the
            // larger), or once the two meet.
            let whole = pair.iter().flatten().map(|&(p, _)| swept(p)).sum::<f32>() >= 1.0;
            // Past whole: a strand that ended before this frame.
            let ended = pair.iter().flatten().any(|&(p, _)| p > 1.0);
            for (k, strand) in pair.into_iter().enumerate() {
                let Some((p, inward)) = strand else {
                    continue;
                };
                let head = if whole {
                    // The front exactly at this half's far end: the head sits there, or a frame
                    // landing on a half boundary would show no head at all; not on a half the
                    // other strand had already lit whole.
                    if p != 1.0 || ended {
                        continue;
                    }
                    if inward { 0.0 } else { 1.0 }
                } else {
                    let g = swept(p);
                    if g <= 0.0 {
                        continue;
                    }
                    let (a, b) = if inward { (1.0 - g, 1.0) } else { (0.0, g) };
                    let head = if inward { a } else { b };
                    spans[m] = (bit, a, b);
                    m += 1;
                    cuts[n] = head;
                    n += 1;
                    head
                };
                if k == 0 {
                    state.heads[state.head_len] = (head, bit);
                    state.head_len += 1;
                } else {
                    seconds[second_len] = (head, bit);
                    second_len += 1;
                }
            }
            if whole {
                state.steady |= bit;
            }
        }
        for &head in &seconds[..second_len] {
            state.heads[state.head_len] = head;
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

/// When light that `new` adds to a half of `cell` (bits not in `old`) first reaches its centre,
/// in reach units; none when no half of it gains light.
pub fn fresh_arrival(old: &BeamField, new: &BeamField, reach: &Reach, cell: usize) -> Option<f32> {
    let mut first = f32::INFINITY;
    for side in 0..8 {
        let fresh = half_colour(new, cell, side) & !half_colour(old, cell, side);
        for bit in [1u8, 2, 4] {
            if fresh & bit != 0 {
                for (start, inward) in strands(new, reach, cell, side, bit).into_iter().flatten() {
                    first = first.min(if inward { start + 0.5 } else { start });
                }
            }
        }
    }
    (first < f32::INFINITY).then_some(first)
}

/// When target `cell` of colour `colour` is complete in `new`, in reach units: each bit it needs
/// is at its centre (a bit `old` already delivered counts as −∞), the latest of them; none when
/// every bit it needs was already there (lit only by losing light).
pub fn completion(
    old: &BeamField,
    new: &BeamField,
    reach: &Reach,
    cell: usize,
    colour: Colour,
) -> Option<f32> {
    let had = old.received(cell);
    BITS.into_iter()
        .filter(|&bit| colour & !had & bit != 0)
        .map(|bit| reach.received_at(new, cell, bit).unwrap_or(f32::INFINITY))
        .reduce(f32::max)
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
                    let base = (line | step) << 3;
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

/// Spec §9.2: how far, in cells, light has come from a laser along its own beams, per channel
/// (cell, direction, colour bit). `leaving` is each lit outbound channel's: light of that bit
/// leaving `cell` travelling `dir` starts at the cell's centre then. Light entering a cell takes
/// the reach of the channel it left its neighbour by, and is at the centre one cell on. `cells`
/// is the earliest light at each cell's centre (0 when unlit). A sweep's front reaches a strand
/// at `at + (reach − r_min) / speed`. `guessed` counts lit cells no laser or teleporter accounts
/// for (given 0); the original levels have none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reach {
    pub cells: [u16; BOARD_CELLS],
    leaving: [Channels; BOARD_CELLS],
    pub guessed: u16,
}

/// One cell's reach per direction and colour bit (blue, green, red).
type Channels = [[u16; 3]; 8];

impl Default for Reach {
    fn default() -> Reach {
        Reach {
            cells: [0; BOARD_CELLS],
            leaving: [[[0; 3]; 8]; BOARD_CELLS],
            guessed: 0,
        }
    }
}

/// Not reached yet.
const FAR: u16 = u16::MAX;

/// The colour bits, in channel order.
const BITS: [Colour; 3] = [1, 2, 4];

/// The channel of colour bit `bit` (one of `BITS`).
fn channel(bit: Colour) -> usize {
    bit.trailing_zeros() as usize
}

impl Reach {
    /// A BFS over channels from the lasers' lit outbound channels (0), with the rules of
    /// `board`'s game. Light of bit `k` leaving `c` travelling `s` at reach `r` enters
    /// `n = c + DIR[s]` at `r + 1`, and there leaves as lumen sends it, at `r + 1`:
    /// - most pieces by the channels lumen's `transfer` gives bit `k` (those lit in `field`; one
    ///   an entangled twin recoloured after it entered counts by its direction: every lit bit
    ///   that way);
    /// - a teleporter jumps it (as lumen's `enter_cell`): it leaves the next teleport target
    ///   along `s` one step later, `r + 2`;
    /// - a complementor sends the complement of what enters it travelling `s` on along `s`,
    ///   once all of that has entered;
    /// - a gate sends bit `k` on along its direction once both its inputs have it.
    ///
    /// So along every beam step `reach(n, s', k') ≤ reach(c, s, k) + 1`, and every channel has a
    /// step that meets it. A cell that emits light no beam accounts for takes 0 on those
    /// channels (counted in `guessed`), lowest index first, and the search goes on from it.
    /// Fixed arrays.
    pub fn compute(board: &Board, field: &BeamField) -> Reach {
        let rules = board.rules();
        let mut search = Search::new();
        let mut guessed = 0;
        for c in 0..BOARD_CELLS {
            if board.cells[c].piece == Piece::Laser {
                search.seed(field, c);
            }
        }
        // Every cell below `unguessed` has each lit outbound channel reached.
        let mut unguessed = 0;
        loop {
            while let Some(step) = search.next() {
                match step {
                    Step::Leaving(c, s, k) => {
                        search.enter(board, rules, field, c, s, k);
                    }
                    Step::Jumping(t, s, k) => {
                        // The jump lands one step after the light entered the teleporter.
                        let r = search.jumping[t][s][k] + 1;
                        if let Some(n) = teleport_landing(board, rules, t, s) {
                            search.leave_by(field, n, s, BITS[k], r);
                        }
                    }
                }
            }
            let Some(c) = (unguessed..BOARD_CELLS).find(|&c| {
                (0..8).any(|s| {
                    BITS.iter().enumerate().any(|(k, &bit)| {
                        field.outbound[c][s] & bit != 0 && search.leaving[c][s][k] == FAR
                    })
                })
            }) else {
                break;
            };
            unguessed = c + 1;
            guessed += 1;
            search.seed(field, c);
        }
        let leaving = search
            .leaving
            .map(|dirs| dirs.map(|bits| bits.map(|r| if r == FAR { 0 } else { r })));
        let mut cells = [0; BOARD_CELLS];
        for (c, at) in cells.iter_mut().enumerate() {
            let mut first = FAR;
            for s in 0..8 {
                for (k, &bit) in BITS.iter().enumerate() {
                    if field.outbound[c][s] & bit != 0 {
                        first = first.min(leaving[c][s][k]);
                    }
                    if field.inbound[c][s] & bit != 0
                        && let Some(r) = entering(&leaving, c, s, k)
                    {
                        first = first.min(r + 1);
                    }
                }
            }
            *at = if first == FAR { 0 } else { first };
        }
        Reach {
            cells,
            leaving,
            guessed,
        }
    }

    /// The reach of light `bit` leaving `cell` travelling `dir` (0 when none does).
    pub fn leaving(&self, cell: usize, dir: usize, bit: Colour) -> u16 {
        self.leaving[cell][dir][channel(bit)]
    }

    /// When light `bit` of `field` first reaches `cell`'s centre from one of its sides, in reach
    /// units; none when none enters it.
    pub fn received_at(&self, field: &BeamField, cell: usize, bit: Colour) -> Option<f32> {
        (0..8)
            .filter(|&s| field.inbound[cell][s] & bit != 0)
            .filter_map(|s| entering(&self.leaving, cell, s, channel(bit)))
            .min()
            .map(|r| f32::from(r) + 1.0)
    }
}

/// The reach of light of channel `k` entering `cell` travelling `dir`: that of the channel it
/// left its neighbour by (none off the board, which a traced field never lights).
fn entering(leaving: &[Channels; BOARD_CELLS], cell: usize, dir: usize, k: usize) -> Option<u16> {
    let (x, y) = (
        (cell % SIDE) as i32 - i32::from(DX[dir]),
        (cell / SIDE) as i32 - i32::from(DY[dir]),
    );
    on_board(x, y).then(|| leaving[y as usize * SIDE + x as usize][dir][k])
}

/// A channel on `Reach::compute`'s frontier: light leaving a cell, or light inside a teleporter
/// about to jump; cell, direction, channel.
#[derive(Clone, Copy)]
enum Step {
    Leaving(usize, usize, usize),
    Jumping(usize, usize, usize),
}

/// Queue slots: every channel queued at most once as leaving and once as jumping.
const QUEUE: usize = 2 * 3 * 8 * BOARD_CELLS;

/// `Reach::compute`'s breadth-first frontier over channels. A channel's first reach is its
/// least: every step is one, so the queue stays in reach order, and every channel of one reach
/// is set before the first of them is taken (a gate or complementor sees all it waits for).
struct Search {
    leaving: [Channels; BOARD_CELLS],
    jumping: [Channels; BOARD_CELLS],
    /// `((cell × 8 + dir) × 3 + channel) × 2`, + 1 for a jump.
    queue: [u16; QUEUE],
    head: usize,
    tail: usize,
}

impl Search {
    fn new() -> Search {
        Search {
            leaving: [[[FAR; 3]; 8]; BOARD_CELLS],
            jumping: [[[FAR; 3]; 8]; BOARD_CELLS],
            queue: [0; QUEUE],
            head: 0,
            tail: 0,
        }
    }

    fn push(&mut self, cell: usize, dir: usize, k: usize, jump: bool) {
        self.queue[self.tail] = (((cell * 8 + dir) * 3 + k) * 2 + usize::from(jump)) as u16;
        self.tail += 1;
    }

    /// Gives each channel of `bits` leaving `cell` travelling `dir` reach `r` and queues it,
    /// unless reached.
    fn leave(&mut self, cell: usize, dir: usize, bits: Colour, r: u16) {
        for (k, &bit) in BITS.iter().enumerate() {
            if bits & bit != 0 && self.leaving[cell][dir][k] == FAR {
                self.leaving[cell][dir][k] = r;
                self.push(cell, dir, k, false);
            }
        }
    }

    /// Light `bits` sent on from `cell` travelling `dir` at `r`: the lit ones, or, when an
    /// entangled twin was recoloured after it entered (none of them lit that way), every lit
    /// bit that way.
    fn leave_by(&mut self, field: &BeamField, cell: usize, dir: usize, bits: Colour, r: u16) {
        let out = field.outbound[cell][dir];
        let lit = bits & out;
        self.leave(cell, dir, if lit != 0 { lit } else { out }, r);
    }

    /// Every lit outbound channel of `cell` not yet reached starts at 0.
    fn seed(&mut self, field: &BeamField, cell: usize) {
        for dir in 0..8 {
            self.leave(cell, dir, field.outbound[cell][dir], 0);
        }
    }

    /// Whether light of channel `k` entering `n` travelling `dir` is at its centre by `r` (it
    /// left the neighbour before `r`).
    fn entered_by(&self, n: usize, dir: usize, k: usize, r: u16) -> bool {
        entering(&self.leaving, n, dir, k).is_some_and(|e| e < r)
    }

    /// Light of channel `k` leaving `c` travelling `s` enters the next cell, one step on, and
    /// leaves it as that cell's piece sends it.
    fn enter(
        &mut self,
        board: &Board,
        rules: &RuleSet,
        field: &BeamField,
        c: usize,
        s: usize,
        k: usize,
    ) {
        let (x, y) = (
            (c % SIDE) as i32 + i32::from(DX[s]),
            (c / SIDE) as i32 + i32::from(DY[s]),
        );
        let bit = BITS[k];
        if !on_board(x, y) {
            return;
        }
        let n = y as usize * SIDE + x as usize;
        let entered = field.inbound[n][s];
        if entered & bit == 0 {
            return;
        }
        let r = self.leaving[c][s][k] + 1;
        let cell = board.cells[n];
        match cell.piece {
            Piece::Teleporter | Piece::MobileTeleporter => {
                if self.jumping[n][s][k] == FAR {
                    self.jumping[n][s][k] = r;
                    self.push(n, s, k, true);
                }
            }
            // lumen's `emit_complementors`: the complement of what entered travelling `s` goes
            // on along `s`, once all of it has entered.
            Piece::Complementor => {
                let all = (0..3).all(|j| entered & BITS[j] == 0 || self.entered_by(n, s, j, r));
                if all {
                    self.leave(n, s, field.outbound[n][s], r);
                }
            }
            // `emit_gates`: a gate pointing `p` sends on the bits both its inputs (light
            // travelling `p ± 1`) share, once both have them.
            Piece::LogicGate | Piece::PrimaryLogicGate => {
                let p = usize::from(cell.dir & 7);
                let other = match s {
                    _ if s == (p + 1) & 7 => (p + 7) & 7,
                    _ if s == (p + 7) & 7 => (p + 1) & 7,
                    _ => return,
                };
                if self.entered_by(n, other, k, r) {
                    self.leave(n, p, field.outbound[n][p] & bit, r);
                }
            }
            _ => {
                // The tag only picks a doppler's colour and numbers a tangler's pairs: the
                // directions don't depend on it.
                let mut tag = 0;
                for e in transfer(rules, cell, s as Dir, bit, 0, &mut tag).as_slice() {
                    let dir = usize::from(e.dir & 7);
                    if field.outbound[n][dir] != 0 {
                        self.leave_by(field, n, dir, e.colour, r);
                    }
                }
            }
        }
    }

    fn next(&mut self) -> Option<Step> {
        (self.head < self.tail).then(|| {
            let v = usize::from(self.queue[self.head]);
            self.head += 1;
            let (k, dir, cell) = (v / 2 % 3, v / 6 % 8, v / 48);
            if v % 2 == 1 {
                Step::Jumping(cell, dir, k)
            } else {
                Step::Leaving(cell, dir, k)
            }
        })
    }
}

/// Where bit `bit` of half-segment (`cell`, `side`) starts being drawn by a sweep, in reach
/// units (cells from the lasers): light leaving the cell starts at its centre at its channel's
/// reach; light entering it starts at its edge half a cell after the channel it left its
/// neighbour by. `true` when it enters.
pub fn strand_start(
    field: &BeamField,
    reach: &Reach,
    cell: usize,
    side: usize,
    bit: Colour,
) -> (f32, bool) {
    let k = channel(bit);
    if field.outbound[cell][side] & bit != 0 {
        return (f32::from(reach.leaving[cell][side][k]), false);
    }
    (inbound_start(reach, cell, side, k), true)
}

/// Where light of channel `k` entering `cell` by `side` starts, at the half's edge: half a cell
/// after the channel it left its neighbour by.
fn inbound_start(reach: &Reach, cell: usize, side: usize, k: usize) -> f32 {
    let from = entering(&reach.leaving, cell, (side + 4) & 7, k).unwrap_or(reach.cells[cell]);
    f32::from(from) + 0.5
}

/// The strands a sweep draws bit `bit` of half (`cell`, `side`) from: `strand_start`'s, and on a
/// half the bit crosses both ways (leaving the cell by `side` and entering it by `side`) the
/// inbound one too, from the half's edge.
pub fn strands(
    field: &BeamField,
    reach: &Reach,
    cell: usize,
    side: usize,
    bit: Colour,
) -> [Option<(f32, bool)>; 2] {
    let first = strand_start(field, reach, cell, side, bit);
    let both = field.outbound[cell][side] & field.inbound[cell][(side + 4) & 7] & bit != 0;
    let second = both.then(|| (inbound_start(reach, cell, side, channel(bit)), true));
    [Some(first), second]
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
    let piece = board.cells[cell].piece;
    // No sparkle on targets (user decision): a bright centre always means a lit target, never
    // beams crossing a spoilt one. (A target only passes light on, so it never bounces either.)
    if piece == Piece::Pinwheel {
        return None;
    }
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
    use spectrograph::Game;

    fn board(pieces: &[(usize, usize, Piece, u8, u8)]) -> (Board, BeamField) {
        board_of(Board::default(), pieces)
    }

    /// As `board`, on an empty board of `game`.
    fn board_in(
        game: spectrograph::Game,
        pieces: &[(usize, usize, Piece, u8, u8)],
    ) -> (Board, BeamField) {
        board_of(Board::new(game), pieces)
    }

    fn board_of(mut b: Board, pieces: &[(usize, usize, Piece, u8, u8)]) -> (Board, BeamField) {
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
    fn a_target_never_glints_so_a_bright_centre_always_means_lit() {
        // Red east and blue south cross on a red target: spoilt (it takes red and blue), so its
        // centre stays dark, with no crossing glint where a lit target's core would be.
        let (b, field) = board(&[
            (0, 5, Piece::Laser, 4, 2),
            (5, 0, Piece::Laser, 1, 4),
            (5, 5, Piece::Pinwheel, 4, 0),
        ]);
        assert_eq!(field.received(board_index(5, 5)), 5, "red and blue");
        let mut line = Beamline::new();
        line.rebuild(&b, &field, None);
        assert!(
            line.glints().iter().all(|g| g.cell != board_index(5, 5)),
            "{:?}",
            line.glints()
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

    /// When light of `bit` entering `n` travelling `s` is at its centre: one step after it left.
    fn entered_at(reach: &Reach, n: usize, s: usize, bit: Colour) -> u16 {
        let (x, y) = (
            (n % SIDE) as i32 - i32::from(DX[s]),
            (n / SIDE) as i32 - i32::from(DY[s]),
        );
        reach.leaving(y as usize * SIDE + x as usize, s, bit) + 1
    }

    /// Spec §15's invariant, both ways: every lit channel's reach is exactly the earliest its
    /// producers (lumen's own rules, per bit) give it, so no step from `(c, s)` makes
    /// `reach(n, s')` more than `reach(c, s) + 1` (a jump + 2), and every channel has a witness
    /// at that bound. A laser's own light is 0.
    fn assert_exact(b: &Board, field: &BeamField, reach: &Reach) {
        let rules = b.rules();
        for n in 0..BOARD_CELLS {
            let cell = b.cells[n];
            for d in 0..8 {
                for bit in [1u8, 2, 4] {
                    let out = field.outbound[n][d];
                    if out & bit == 0 {
                        continue;
                    }
                    let mut best = u16::MAX;
                    match cell.piece {
                        Piece::Laser => best = 0,
                        Piece::Complementor => {
                            let k = field.inbound[n][d];
                            best = [1u8, 2, 4]
                                .into_iter()
                                .filter(|&e| k & e != 0)
                                .map(|e| entered_at(reach, n, d, e))
                                .max()
                                .unwrap_or(u16::MAX);
                        }
                        Piece::LogicGate | Piece::PrimaryLogicGate => {
                            let (a, z) =
                                (usize::from(cell.dir + 1) & 7, usize::from(cell.dir + 7) & 7);
                            assert_eq!(d, usize::from(cell.dir & 7));
                            best = entered_at(reach, n, a, bit).max(entered_at(reach, n, z, bit));
                        }
                        _ => {
                            for s in 0..8 {
                                for k in [1u8, 2, 4] {
                                    if field.inbound[n][s] & k == 0 {
                                        continue;
                                    }
                                    let mut tag = 0;
                                    for e in
                                        transfer(rules, cell, s as Dir, k, 0, &mut tag).as_slice()
                                    {
                                        let lit = e.colour & out;
                                        if usize::from(e.dir) == d && (lit & bit != 0 || lit == 0) {
                                            best = best.min(entered_at(reach, n, s, k));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // A jump from the teleporter behind that lands here.
                    let (mut x, mut y) = ((n % SIDE) as i32, (n / SIDE) as i32);
                    loop {
                        (x, y) = (x - i32::from(DX[d]), y - i32::from(DY[d]));
                        if !on_board(x, y) {
                            break;
                        }
                        let t = y as usize * SIDE + x as usize;
                        let k = field.inbound[t][d];
                        if matches!(
                            b.cells[t].piece,
                            Piece::Teleporter | Piece::MobileTeleporter
                        ) && teleport_landing(b, rules, t, d) == Some(n)
                        {
                            for e in [1u8, 2, 4] {
                                if k & e != 0 && (e == bit || out & e == 0) {
                                    best = best.min(entered_at(reach, t, d, e) + 1);
                                }
                            }
                        }
                        if rules.teleport_targets.contains(&b.cells[t].piece) {
                            break;
                        }
                    }
                    assert_eq!(reach.leaving(n, d, bit), best, "cell {n} dir {d} bit {bit}");
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
        assert_exact(&b, &field, &reach);
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
        assert_exact(&b, &field, &reach);
        assert_eq!(reach.guessed, 0);
    }

    #[test]
    fn reach_relaxes_where_a_teleported_beam_joins_a_longer_path() {
        // Green east from (0, 7) jumps (2, 7) → (5, 7) and runs on through (8, 7), which blue,
        // south from (8, 0), reaches only seven cells on. The jump is a step in the search, so
        // green's light past (8, 7) takes the shorter reach; blue's, going on south, keeps its own.
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
        assert_eq!(reach.leaving(board_index(8, 7), 2, 2), 6, "green on east");
        assert_eq!(at(8, 8), 8, "blue's light, eight cells on");
        assert_exact(&b, &field, &reach);
        assert_eq!(reach.guessed, 0);
    }

    #[test]
    fn light_past_an_empty_crossing_keeps_its_own_beams_reach() {
        // Blue east from (4, 5) crosses (5, 5) one cell on; green south from (5, 0) reaches it
        // five cells on. Below the crossing the light is green's: it leaves (5, 5) at 5 and
        // enters (5, 6) half a cell later, not at blue's early reach.
        let (b, field) = board(&[(4, 5, Piece::Laser, 1, 2), (5, 0, Piece::Laser, 2, 4)]);
        let reach = Reach::compute(&b, &field);
        let (crossing, below) = (board_index(5, 5), board_index(5, 6));
        assert_eq!(strand_start(&field, &reach, crossing, 2, 1), (1.0, false));
        assert_eq!(strand_start(&field, &reach, crossing, 4, 2), (5.0, false));
        assert_eq!(strand_start(&field, &reach, below, 0, 2), (5.5, true));
        assert_eq!(strand_start(&field, &reach, below, 4, 2), (6.0, false));
        assert_eq!(
            strand_start(&field, &reach, board_index(6, 5), 6, 1),
            (1.5, true),
            "blue goes on east at its own reach"
        );
        assert_eq!(reach.cells[crossing], 1, "first touched by blue");
        assert_eq!(reach.cells[below], 6);
        assert_eq!(reach.guessed, 0);
        // Green new to a board blue already crossed: the crossing (its glint) waits for green.
        let (_, old) = board(&[(4, 5, Piece::Laser, 1, 2)]);
        let view = |front| SweepView {
            old: &old,
            reach: &reach,
            front,
        };
        assert!(!view(4.9).arrived(&field, crossing), "green not there yet");
        assert!(view(5.0).arrived(&field, crossing));
        assert!(
            view(0.0).arrived(&field, board_index(6, 5)),
            "nothing new east"
        );
    }

    #[test]
    fn light_a_splitter_merges_keeps_each_colours_reach() {
        // Green east from (6, 5) passes the splitter at (7, 5) one cell on; red south from
        // (7, 0) reflects east there five cells on. The yellow beam east is green's from 1 and
        // red's only from 5, on to the yellow target at (12, 5).
        let (b, field) = board_in(
            Game::C1,
            &[
                (6, 5, Piece::Laser, 2, 2),
                (7, 5, Piece::Splitter, 0, 3),
                (7, 0, Piece::Laser, 4, 4),
                (12, 5, Piece::Pinwheel, 6, 0),
            ],
        );
        let reach = Reach::compute(&b, &field);
        let splitter = board_index(7, 5);
        assert_eq!(strand_start(&field, &reach, splitter, 2, 2), (1.0, false));
        assert_eq!(strand_start(&field, &reach, splitter, 2, 4), (5.0, false));
        let target = board_index(12, 5);
        assert_eq!(strand_start(&field, &reach, target, 6, 4), (9.5, true));
        assert_exact(&b, &field, &reach);
        assert_eq!(reach.guessed, 0);
    }

    #[test]
    fn a_gate_sends_its_light_once_both_inputs_have_it() {
        // C3's gate at (7, 7) pointing north: red travelling north-east enters it two cells on,
        // red travelling north-west six cells on. It sends red north at 6, not 2.
        let (b, field) = board_in(
            Game::C3,
            &[
                (5, 9, Piece::Laser, 4, 1),
                (7, 7, Piece::LogicGate, 0, 0),
                (7, 2, Piece::Pinwheel, 4, 0),
                (13, 13, Piece::Laser, 4, 7),
            ],
        );
        let gate = board_index(7, 7);
        assert_eq!(field.outbound[gate][0], 4, "red north");
        let reach = Reach::compute(&b, &field);
        assert_eq!(strand_start(&field, &reach, gate, 0, 4), (6.0, false));
        assert_eq!(reach.leaving(gate, 0, 4), 6);
        assert_exact(&b, &field, &reach);
        assert_eq!(reach.guessed, 0);
    }

    #[test]
    fn a_complementor_sends_each_complement_when_its_own_input_arrives() {
        // C2's complementor at (7, 7): red east from (6, 7) enters it one cell on, so cyan goes
        // on east from 1; green south from (7, 0) enters seven cells on, so magenta goes on south
        // only from 7.
        let (b, field) = board_in(
            Game::C2,
            &[
                (6, 7, Piece::Laser, 4, 2),
                (7, 7, Piece::Complementor, 0, 0),
                (7, 0, Piece::Laser, 2, 4),
            ],
        );
        let c = board_index(7, 7);
        assert_eq!((field.outbound[c][2], field.outbound[c][4]), (3, 5));
        let reach = Reach::compute(&b, &field);
        assert_eq!(strand_start(&field, &reach, c, 2, 1), (1.0, false));
        assert_eq!(strand_start(&field, &reach, c, 4, 4), (7.0, false));
        assert_eq!(strand_start(&field, &reach, c, 4, 1), (7.0, false));
        assert_exact(&b, &field, &reach);
        // Red and green merged at a splitter enter it on one side: their complement, blue, goes
        // on once both have, the later green's nine cells on.
        let (b, field) = board_in(
            Game::C2,
            &[
                (4, 7, Piece::Laser, 4, 2),
                (5, 7, Piece::Splitter, 0, 3),
                (5, 0, Piece::Laser, 2, 4),
                (7, 7, Piece::Complementor, 0, 0),
            ],
        );
        assert_eq!((field.inbound[c][2], field.outbound[c][2]), (6, 1));
        let reach = Reach::compute(&b, &field);
        assert_eq!(strand_start(&field, &reach, c, 2, 1), (9.0, false));
        assert_exact(&b, &field, &reach);
        assert_eq!(reach.guessed, 0);
    }

    #[test]
    fn a_target_shows_each_new_colour_once_it_lands() {
        // A yellow target at (5, 5): red from (4, 5) lands one cell on, green from (5, 14) nine.
        // Its arcs fill as each lands, and it is lit (shows yellow) only at 9.
        let (_, old) = board(&[(5, 5, Piece::Pinwheel, 6, 0)]);
        let (b, new) = board(&[
            (5, 5, Piece::Pinwheel, 6, 0),
            (4, 5, Piece::Laser, 4, 2),
            (5, 14, Piece::Laser, 2, 0),
        ]);
        let reach = Reach::compute(&b, &new);
        let target = board_index(5, 5);
        let shows = |front| {
            SweepView {
                old: &old,
                reach: &reach,
                front,
            }
            .received(&new, target)
        };
        assert_eq!(shows(0.5), 0);
        assert_eq!(shows(1.0), 4, "red has landed");
        assert_eq!(shows(8.9), 4);
        assert_eq!(shows(9.0), 6, "and green: lit");
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
    fn a_half_lit_both_ways_sweeps_from_both_ends() {
        // White east from (2, 5) into a mirror at (8, 5) that sends it straight back: cell
        // (5, 5)'s west half takes the light coming in from the west early and the light going
        // back out west late. It is drawn from its edge as soon as the first comes in, not dark
        // until the second leaves (the "dashed beam" of C1 level 27).
        let old = BeamField::default();
        let (b, new) = (0..8)
            .map(|dir| board(&[(2, 5, Piece::Laser, 7, 2), (8, 5, Piece::Mirror, 0, dir)]))
            .find(|(_, f)| f.outbound[board_index(5, 5)][6] == 7)
            .expect("a mirror that sends the beam back");
        let reach = Reach::compute(&b, &new);
        let (cell, west) = (board_index(5, 5), 6);
        let [first, second] = strands(&new, &reach, cell, west, 1);
        let ((out_start, out_in), (in_start, in_in)) = (first.unwrap(), second.unwrap());
        assert!(
            !out_in && in_in,
            "outbound from the centre, inbound from the edge"
        );
        assert!(
            in_start + 0.5 < out_start,
            "in at {in_start}, out at {out_start}"
        );
        let view = |front| SweepView {
            old: &old,
            reach: &reach,
            front,
        };
        let mid = view(in_start + 0.25).half(&new, cell, west);
        assert_eq!(
            (mid.steady, mid.len, mid.pieces[0]),
            (0, 1, (0.5, 1.0, 7)),
            "half way in from its edge"
        );
        assert_eq!(mid.head_len, 3, "a head per channel coming in");
        let whole = view(in_start + 0.5).half(&new, cell, west);
        assert_eq!(
            (whole.steady, whole.len),
            (7, 0),
            "whole once the inbound light is"
        );
        // Its east half takes the light going out first: drawn from the centre, as one way.
        let [first, second] = strands(&new, &reach, cell, 2, 1);
        let ((out_start, _), (in_start, _)) = (first.unwrap(), second.unwrap());
        assert!(out_start + 0.5 < in_start);
        let east = view(out_start + 0.25).half(&new, cell, 2);
        assert_eq!((east.len, east.pieces[0]), (1, (0.0, 0.5, 7)), "centre out");
        // Whole since then: no head flashes at the centre when the front lands exactly where
        // the light coming back in would end, nor just before or after.
        for front in [in_start + 0.4, in_start + 0.5, in_start + 0.6] {
            let late = view(front).half(&new, cell, 2);
            assert_eq!(
                (late.steady, late.head_len),
                (7, 0),
                "front {front}: {:?}",
                late.heads
            );
        }
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
