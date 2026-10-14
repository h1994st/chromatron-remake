//! `choreography`: the modern look's motion (spec §9). `paint` hands it what changed since the
//! last paint (D5) and it starts cues; `moment` samples them at `now` and is pure. Times are
//! ms on the shell's clock; a non-finite `now` is settled (ruling M7). Fixed arrays.

use std::f32::consts::FRAC_PI_4;

use lumen::{BOARD_CELLS, BeamField, Board, Cell, Colour, Drag, Piece};

use crate::beamline::{Reach, SweepView, completion, half_colour, strand_start};
use crate::piece_art::turn_period;

/// One frame at 60 Hz: `next_wake` asks for the next this long after `now`.
pub const FRAME_MS: f64 = 1000.0 / 60.0;
/// Every cue has ended this long after the paint that started it (ruling M10(k)).
pub const SETTLE_MS: f64 = 2_500.0;
pub const FRONT_CELLS_PER_S: f64 = 40.0;
pub const SWEEP_MIN_MS: f64 = 250.0;
pub const SWEEP_MAX_MS: f64 = 600.0;
pub const FADE_MS: f64 = 160.0;
pub const RING_MS: f64 = 600.0;
pub const SPARKS_MS: f64 = 900.0;
pub const WIN_MS: f64 = 1_400.0;
pub const PACKET_CELLS_PER_S: f64 = 60.0;
pub const PULSE_MS: f64 = 400.0;
pub const REVEAL_MS: f64 = 700.0;
pub const TURN_MS: f64 = 150.0;
pub const LIFT_MS: f64 = 90.0;
pub const LAND_MS: f64 = 160.0;
pub const LEVEL_MS: f64 = 180.0;
pub const SHEET_MS: f64 = 200.0;
/// Reduced motion's cap on the fades it keeps (level, sheet, win header).
pub const REDUCED_FADE_MS: f64 = 150.0;
pub const SHEET_RISE_PT: f32 = 12.0;
pub const MAX_BURSTS: usize = 32;

/// The trailer's curves.
pub mod ease {
    pub fn out_cubic(t: f32) -> f32 {
        1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
    }

    pub fn out_expo(t: f32) -> f32 {
        if t >= 1.0 {
            1.0
        } else {
            1.0 - 2f32.powf(-10.0 * t.clamp(0.0, 1.0))
        }
    }

    pub fn in_out_cubic(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t < 0.5 {
            4.0 * t * t * t
        } else {
            1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
        }
    }
}

/// What a paint saw change: index 0 is the last painted state, 1 this one's.
pub struct Observed<'a> {
    pub board: [&'a Board; 2],
    pub field: [&'a BeamField; 2],
    pub drag: [Option<Drag>; 2],
    pub slot: [usize; 2],
    pub won: [bool; 2],
    pub sheet: [bool; 2],
}

/// A target's burst: it starts when the sweep completes the target (its last needed colour lands).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Burst {
    pub cell: usize,
    pub mask: Colour,
    pub at: f64,
}

/// A released piece settling into `cell` from where it was let go (`from`, device px) and from
/// how far it had risen (`lift`, 0 → 1: 1 unless released mid-lift).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landing {
    pub cell: usize,
    pub piece: Cell,
    pub from: (i32, i32),
    pub lift: f32,
    pub progress: f32,
}

/// The level sheet mid-fade: its opacity, how far below its place it is, and whether its scrim
/// shows (only while it opens).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetPose {
    pub alpha: f32,
    pub rise_pt: f32,
    pub scrim: bool,
}

impl SheetPose {
    pub const SHOWN: SheetPose = SheetPose {
        alpha: 1.0,
        rise_pt: 0.0,
        scrim: true,
    };
}

/// The win's cues at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WinMoment {
    pub packets: bool,
    /// The packets' place, in reach units (negative before they start).
    pub packet: f32,
    pub since_ms: f32,
    pub reveal: f32,
    pub header_alpha: f32,
}

/// The cues sampled at one `now` (pure).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment<'c> {
    pub sweep: Option<SweepView<'c>>,
    /// The field old strands fade from, at `fade` (1 → 0 over 160 ms; 0 when none fade).
    pub old: &'c BeamField,
    pub fade: f32,
    /// The turning cell and its drawing's angle off its settled one, in radians.
    pub turn: Option<(usize, f32)>,
    /// The held piece's lift, 0 → 1 (1 when settled).
    pub lift: f32,
    pub land: Option<Landing>,
    /// The level cross-fade's weight of the new picture, eased; none when not fading.
    pub level: Option<f32>,
    pub sheet: Option<SheetPose>,
    pub win: Option<WinMoment>,
}

#[derive(Clone, Copy, Debug)]
struct Beams {
    /// The sweep starts (front at `r_min`).
    at: f64,
    /// The paint that started the cue (old strands fade from it).
    began: f64,
    r_min: f32,
    /// Reach units per ms; 0 when nothing is new.
    speed: f64,
    sweep_end: f64,
    fade_end: f64,
}

impl Beams {
    fn front(&self, now: f64) -> f32 {
        if self.speed <= 0.0 {
            return f32::INFINITY;
        }
        (f64::from(self.r_min) + (now - self.at) * self.speed) as f32
    }

    /// When the front reaches `reach`, never before the sweep starts (a target lit by losing
    /// light can lie before `r_min`).
    fn arrival(&self, reach: f32) -> f64 {
        if self.speed <= 0.0 {
            self.at
        } else {
            (self.at + f64::from(reach - self.r_min) / self.speed).max(self.at)
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Turn {
    cell: usize,
    from: f32,
    at: f64,
}

impl Turn {
    /// The drawing's angle off its settled one at `now`, in radians.
    fn offset(&self, now: f64) -> f32 {
        let k = ((now - self.at) / TURN_MS).clamp(0.0, 1.0) as f32;
        self.from * (1.0 - ease::out_cubic(k))
    }
}

#[derive(Clone, Copy, Debug)]
struct Land {
    cell: usize,
    piece: Cell,
    from: (i32, i32),
    lift: f32,
    at: f64,
}

#[derive(Clone, Copy, Debug)]
struct Sheet {
    at: f64,
    opening: bool,
}

/// The running cues, kept between paints. Boxed: the old field and the reach are fixed arrays.
pub struct Choreography {
    reduced: bool,
    beams: Option<Beams>,
    old: BeamField,
    reach: Reach,
    bursts: [Burst; MAX_BURSTS],
    burst_len: usize,
    win: Option<f64>,
    turn: Option<Turn>,
    lift: Option<f64>,
    land: Option<Land>,
    level: Option<f64>,
    /// The running level cue's old frame was kept (its first paint): later paints, even at the
    /// same `now`, keep nothing over it.
    fade_kept: bool,
    sheet: Option<Sheet>,
}

impl Choreography {
    pub fn new() -> Box<Choreography> {
        Box::new(Choreography {
            reduced: false,
            beams: None,
            old: BeamField::default(),
            reach: Reach::default(),
            bursts: [Burst::default(); MAX_BURSTS],
            burst_len: 0,
            win: None,
            turn: None,
            lift: None,
            land: None,
            level: None,
            fade_kept: false,
            sheet: None,
        })
    }

    /// Reduced motion (spec §9.3, ruling M10(i)); a running cue settles at once.
    pub fn set_reduced(&mut self, on: bool) {
        self.reduced = on;
        self.clear();
    }

    /// No cue: the next frame is settled.
    pub fn clear(&mut self) {
        self.beams = None;
        self.burst_len = 0;
        (
            self.win, self.turn, self.lift, self.land, self.level, self.sheet,
        ) = (None, None, None, None, None, None);
    }

    pub fn reach(&self) -> &Reach {
        &self.reach
    }

    /// When the running level cue began (its first paint), if one runs (for tests).
    #[doc(hidden)]
    pub fn level_since(&self) -> Option<f64> {
        self.level
    }

    /// Whether this paint must keep the frame shown, to fade from: true once per level cue, at
    /// its first paint (M5), never again for it (a later paint at the same `now` has drawn
    /// over that frame).
    pub fn keep_for_fade(&mut self) -> bool {
        let due = self.level.is_some() && !self.fade_kept;
        self.fade_kept = true;
        due
    }

    pub fn bursts(&self) -> &[Burst] {
        &self.bursts[..self.burst_len]
    }

    fn fade_ms(&self, normal: f64) -> f64 {
        if self.reduced {
            REDUCED_FADE_MS
        } else {
            normal
        }
    }

    /// Starts the cues for what changed (D5, spec §9.1). A non-finite `now` settles instead.
    pub fn observe(&mut self, o: &Observed, now: f64) {
        if !now.is_finite() {
            self.clear();
            return;
        }
        self.retire(now);
        let level = o.slot[0] != o.slot[1];
        if level {
            // Another level: no turn, landing, win or burst carries over; the board fades (not
            // under the sheet, nor as a chip chosen in it closes it: ruling F4, nor while its
            // panel still fades out, so the frame kept never holds it), then its beams sweep
            // from the lasers.
            // Without a fade (the sheet's own cue covers the change) the sweep starts at once.
            (self.turn, self.land, self.win, self.beams) = (None, None, None, None);
            self.burst_len = 0;
            let fade = !o.sheet[0] && !o.sheet[1] && self.sheet.is_none();
            // A change with no fade ends one still running: it would blend from a frame kept
            // two levels back.
            self.level = fade.then_some(now);
            self.fade_kept = false;
            if !self.reduced {
                self.old.inbound = [[0; 8]; BOARD_CELLS];
                self.old.outbound = [[0; 8]; BOARD_CELLS];
                let at = if fade { now + LEVEL_MS } else { now };
                self.begin_beams(o.board[1], o.field[1], now, at, false);
            }
        } else if o.field[0] != o.field[1] {
            if self.reduced {
                self.beams = None;
                self.burst_len = 0;
            } else {
                self.old.clone_from(o.field[0]);
                self.begin_beams(o.board[1], o.field[1], now, now, true);
            }
        }
        // A running land's cell changed some other way (a paste, a restart, a turn), or its
        // piece was picked up again: what it settles into is gone, so the land ends.
        if self.land.is_some_and(|l| {
            o.board[1].cells[l.cell] != l.piece || o.drag[1].is_some_and(|d| d.source == l.cell)
        }) {
            self.land = None;
        }
        if !self.reduced {
            if !level {
                self.observe_turn(o, now);
            }
            match o.drag {
                [None, Some(_)] => self.lift = Some(now),
                // Dropped or cancelled: nothing is held, so nothing lifts.
                [Some(d), None] => {
                    // How far it had risen: a release mid-lift lands from there.
                    let lift = self.lift.map_or(1.0, |at| {
                        ease::out_cubic(((now - at) / LIFT_MS).clamp(0.0, 1.0) as f32)
                    });
                    self.lift = None;
                    if !level {
                        self.land = Some(landing(o, d, lift, now));
                    }
                }
                _ => {}
            }
        }
        if o.won == [true, false] {
            // Unsolved again (a restart, a piece lifted off): no packets on its beams.
            self.win = None;
        }
        if o.won == [false, true] && !level {
            // A win's packets wait for a sweep still to run or running (ruling M10(d)): the
            // winning drop's own, an earlier edit's, or a new level's after its fade.
            let sweep = self
                .beams
                .filter(|b| b.speed > 0.0 && b.sweep_end > now)
                .map(|b| b.sweep_end);
            self.win = Some(sweep.unwrap_or(now));
            // A live beams cue holds the shown field's reach; without one (the cheat key, the
            // beams long settled), the packets need it worked out.
            if self.beams.is_none() {
                self.reach = Reach::compute(o.board[1], o.field[1]);
            }
        }
        if o.sheet[0] != o.sheet[1] {
            // A sheet turned back mid-fade goes on from where it is: the new cue starts at the
            // progress whose eased value shows it as it is now.
            let opening = o.sheet[1];
            let shown = self.sheet.map_or(if opening { 0.0 } else { 1.0 }, |s| {
                self.sheet_shown(s, now)
            });
            let eased = if opening { shown } else { 1.0 - shown };
            let progress = 1.0 - (1.0 - eased.clamp(0.0, 1.0)).cbrt();
            self.sheet = Some(Sheet {
                at: now - f64::from(progress) * self.fade_ms(SHEET_MS),
                opening,
            });
        }
    }

    /// How much of the sheet cue `s` shows at `now` (0 hidden, 1 shown), eased.
    fn sheet_shown(&self, s: Sheet, now: f64) -> f32 {
        let ms = self.fade_ms(SHEET_MS);
        let e = ease::out_cubic(((now - s.at) / ms).clamp(0.0, 1.0) as f32);
        if s.opening { e } else { 1.0 - e }
    }

    /// A sweep of `new` from `self.old`: `r_min`/`r_max` over the fresh half-segment channels,
    /// the sweep's length from them (40 cells/s, 250–600 ms), and the bursts of non-black targets
    /// it lights (when `bursts`).
    fn begin_beams(&mut self, board: &Board, new: &BeamField, began: f64, at: f64, bursts: bool) {
        self.reach = Reach::compute(board, new);
        let (mut r_min, mut r_max) = (f32::INFINITY, f32::NEG_INFINITY);
        for c in 0..BOARD_CELLS {
            for s in 0..8 {
                let fresh = half_colour(new, c, s) & !half_colour(&self.old, c, s);
                for bit in [1u8, 2, 4].into_iter().filter(|&b| fresh & b != 0) {
                    let (start, _) = strand_start(new, &self.reach, c, s, bit);
                    r_min = r_min.min(start);
                    r_max = r_max.max(start + 0.5);
                }
            }
        }
        let span = if r_max > r_min {
            f64::from(r_max - r_min)
        } else {
            0.0
        };
        let ms = if span > 0.0 {
            (span / FRONT_CELLS_PER_S * 1000.0).clamp(SWEEP_MIN_MS, SWEEP_MAX_MS)
        } else {
            0.0
        };
        let beams = Beams {
            at,
            began,
            r_min: if span > 0.0 { r_min } else { 0.0 },
            speed: if ms > 0.0 { span / ms } else { 0.0 },
            sweep_end: at + ms,
            fade_end: if bursts { began + FADE_MS } else { began },
        };
        self.beams = Some(beams);
        // A held burst, running or pending, whose target `new` no longer lights (it went dark
        // again, or its piece left) has lost its cause: it ends. Compacted in place.
        let mut kept = 0;
        for k in 0..self.burst_len {
            let b = self.bursts[k];
            if lit(board, new, b.cell) {
                self.bursts[kept] = b;
                kept += 1;
            }
        }
        self.burst_len = kept;
        if !bursts {
            return;
        }
        // Past the cap the newest edit's bursts read better: the oldest running ones make room,
        // and of this edit's, the lowest cells are kept.
        let fresh = (0..BOARD_CELLS)
            .filter(|&c| newly_lit(board, &self.old, new, c))
            .count()
            .min(MAX_BURSTS);
        let dropped = (self.burst_len + fresh).saturating_sub(MAX_BURSTS);
        if dropped > 0 {
            self.bursts.copy_within(dropped..self.burst_len, 0);
            self.burst_len -= dropped;
        }
        for c in 0..BOARD_CELLS {
            if self.burst_len == MAX_BURSTS {
                break;
            }
            if !newly_lit(board, &self.old, new, c) {
                continue;
            }
            // When it is complete: the last bit it needs lands, as its light shows
            // (`SweepView::received`); lit only by losing light, at the sweep's start.
            let reach = completion(&self.old, new, &self.reach, c, board.cells[c].colour)
                .unwrap_or(f32::NEG_INFINITY);
            self.bursts[self.burst_len] = Burst {
                cell: c,
                mask: board.cells[c].colour,
                at: beams.arrival(reach),
            };
            self.burst_len += 1;
        }
    }

    /// One board cell changed, only its `dir`, by a turn its drawing shows (ruling M10(a, b)):
    /// the shortest way round to the settled drawing. One turn runs at a time: a second one
    /// replaces it, and the first piece snaps to its settled angle; a second turn of the same
    /// cell goes on from the angle shown.
    fn observe_turn(&mut self, o: &Observed, now: f64) {
        let (a, b) = (&o.board[0].cells, &o.board[1].cells);
        let running = self.turn.map(|t| (t.cell, t.offset(now)));
        // A running turn's cell changed some other way (a paste, a restart, a piece lifted off
        // or set down): its piece is no longer the one turning, so the turn ends.
        if self.turn.is_some_and(|t| a[t.cell] != b[t.cell]) {
            self.turn = None;
        }
        let mut changed = (0..BOARD_CELLS).filter(|&i| a[i] != b[i]);
        let (Some(i), None) = (changed.next(), changed.next()) else {
            return;
        };
        let (from, to) = (a[i], b[i]);
        let period = i32::from(turn_period(to.piece));
        if from.piece != to.piece || from.colour != to.colour || period == 0 {
            return;
        }
        let mut steps = (i32::from(to.dir & 7) - i32::from(from.dir & 7)).rem_euclid(period);
        if steps > period / 2 {
            steps -= period;
        }
        if steps != 0 {
            // The angle the running turn of this cell still showed, off its old settled one.
            let carried = running.filter(|r| r.0 == i).map_or(0.0, |r| r.1);
            self.turn = Some(Turn {
                cell: i,
                from: carried - (steps as f32) * FRAC_PI_4,
                at: now,
            });
        }
    }

    /// Forgets the cues that have ended by `now`.
    fn retire(&mut self, now: f64) {
        if self
            .beams
            .is_some_and(|b| now >= b.sweep_end.max(b.fade_end))
        {
            self.beams = None;
        }
        let mut k = 0;
        while k < self.burst_len {
            if now >= self.bursts[k].at + SPARKS_MS {
                self.burst_len -= 1;
                self.bursts.copy_within(k + 1..=self.burst_len, k);
            } else {
                k += 1;
            }
        }
        let over = |at: Option<f64>, ms: f64| at.filter(|&at| now < at + ms);
        self.win = over(self.win, self.fade_ms(WIN_MS));
        self.lift = over(self.lift, LIFT_MS);
        self.level = over(self.level, self.fade_ms(LEVEL_MS));
        self.turn = self.turn.filter(|t| now < t.at + TURN_MS);
        self.land = self.land.filter(|l| now < l.at + LAND_MS);
        let sheet_ms = self.fade_ms(SHEET_MS);
        self.sheet = self.sheet.filter(|s| now < s.at + sheet_ms);
    }

    /// When the last running cue ends (−∞ with none).
    pub fn end(&self) -> f64 {
        let ends = [
            self.beams.map(|b| b.sweep_end.max(b.fade_end)),
            self.win.map(|w| w + self.fade_ms(WIN_MS)),
            self.turn.map(|t| t.at + TURN_MS),
            self.lift.map(|l| l + LIFT_MS),
            self.land.map(|l| l.at + LAND_MS),
            self.level.map(|l| l + self.fade_ms(LEVEL_MS)),
            self.sheet.map(|s| s.at + self.fade_ms(SHEET_MS)),
        ];
        let bursts = self.bursts().iter().map(|b| b.at + SPARKS_MS);
        ends.into_iter()
            .flatten()
            .chain(bursts)
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// `Some(now + FRAME_MS)` while a cue runs at `now`, else `None` (spec §9.1 "Idle").
    pub fn next_wake(&self, now: f64) -> Option<f64> {
        (now.is_finite() && now < self.end()).then_some(now + FRAME_MS)
    }

    /// The cues at `now`. Pure: the same `now` gives the same moment; an earlier one than a cue's
    /// start shows its start; a non-finite one is settled.
    pub fn moment(&self, now: f64) -> Moment<'_> {
        let live = now.is_finite();
        let k = |at: f64, ms: f64| -> f32 {
            if ms <= 0.0 {
                1.0
            } else {
                ((now - at) / ms).clamp(0.0, 1.0) as f32
            }
        };
        let running = |at: f64, ms: f64| live && now < at + ms;
        let beams = self.beams.filter(|_| live);
        // A sweep of nothing new takes 0 ms: none, even sampled before it began.
        let sweep = beams
            .filter(|b| b.speed > 0.0 && now < b.sweep_end)
            .map(|b| SweepView {
                old: &self.old,
                reach: &self.reach,
                front: b.front(now),
            });
        let fade = beams
            .filter(|b| now < b.fade_end)
            .map_or(0.0, |b| 1.0 - k(b.began, FADE_MS));
        let turn = self
            .turn
            .filter(|t| running(t.at, TURN_MS))
            .map(|t| (t.cell, t.offset(now)));
        let lift = self
            .lift
            .filter(|&at| running(at, LIFT_MS))
            .map_or(1.0, |at| ease::out_cubic(k(at, LIFT_MS)));
        let land = self
            .land
            .filter(|l| running(l.at, LAND_MS))
            .map(|l| Landing {
                cell: l.cell,
                piece: l.piece,
                from: l.from,
                lift: l.lift,
                progress: ease::out_cubic(k(l.at, LAND_MS)),
            });
        let level_ms = self.fade_ms(LEVEL_MS);
        let level = self
            .level
            .filter(|&at| running(at, level_ms))
            .map(|at| ease::in_out_cubic(k(at, level_ms)));
        let sheet_ms = self.fade_ms(SHEET_MS);
        let sheet = self.sheet.filter(|s| running(s.at, sheet_ms)).map(|s| {
            let shown = self.sheet_shown(s, now);
            let rise_pt = if self.reduced {
                0.0
            } else {
                SHEET_RISE_PT * (1.0 - shown)
            };
            SheetPose {
                alpha: shown,
                rise_pt,
                scrim: s.opening,
            }
        });
        let win_ms = self.fade_ms(WIN_MS);
        let win = self.win.filter(|&at| running(at, win_ms)).map(|at| {
            let since = (now - at) as f32;
            if self.reduced {
                WinMoment {
                    packets: false,
                    packet: f32::NEG_INFINITY,
                    since_ms: since,
                    reveal: 1.0,
                    header_alpha: k(at, REDUCED_FADE_MS),
                }
            } else {
                let packet = (f64::from(since) * PACKET_CELLS_PER_S / 1000.0) as f32;
                WinMoment {
                    packets: true,
                    packet,
                    since_ms: since,
                    reveal: ease::out_cubic(k(at, REVEAL_MS)),
                    header_alpha: 1.0,
                }
            }
        });
        Moment {
            sweep,
            old: &self.old,
            fade,
            turn,
            lift,
            land,
            level,
            sheet,
            win,
        }
    }
}

/// A non-black target (cell `c`) that `new` satisfies and `old` didn't: it bursts.
fn newly_lit(board: &Board, old: &BeamField, new: &BeamField, c: usize) -> bool {
    lit(board, new, c) && !lit(board, old, c)
}

/// Cell `c` holds a non-black target that `field` satisfies.
fn lit(board: &Board, field: &BeamField, c: usize) -> bool {
    let cell = board.cells[c];
    cell.piece == Piece::Pinwheel && cell.colour != 0 && field.received(c) == cell.colour
}

/// Where a released piece lands: the cell that took it, else (a cancel) its source; from its
/// release point and the `lift` it had reached.
fn landing(o: &Observed, d: Drag, lift: f32, now: f64) -> Land {
    let (a, b) = (&o.board[0].cells, &o.board[1].cells);
    let cell = (0..a.len())
        .find(|&i| a[i] != b[i] && b[i] == d.piece)
        .unwrap_or(d.source);
    Land {
        cell,
        piece: d.piece,
        from: (d.x, d.y),
        lift,
        at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen::{board_index, trace};

    fn lit(pieces: &[(usize, usize, Piece, u8, u8)]) -> (Board, BeamField) {
        let mut b = Board::default();
        for &(x, y, piece, colour, dir) in pieces {
            b.cells[board_index(x, y)] = Cell { piece, colour, dir };
        }
        let mut field = BeamField::default();
        trace(&b, &mut field);
        (b, field)
    }

    /// `before` → `after`, nothing else changing, at `now`.
    fn observe(
        c: &mut Choreography,
        before: &(Board, BeamField),
        after: &(Board, BeamField),
        now: f64,
    ) {
        c.observe(&one_change(before, after), now);
    }

    // A white laser east along row 5; a mirror at (9, 5) whose turn sends the beam elsewhere:
    // south at dir 7, north at dir 1 (two steps on), absorbed at dir 3.
    fn mirror(dir: u8) -> (Board, BeamField) {
        lit(&[(2, 5, Piece::Laser, 7, 2), (9, 5, Piece::Mirror, 0, dir)])
    }

    #[test]
    fn the_mirror_turned_from_7_to_1_sends_its_light_elsewhere() {
        let ((_, south), (_, north)) = (mirror(7), mirror(1));
        let (below, above) = (board_index(9, 6), board_index(9, 4));
        assert!(south.received(below) != 0 && south.received(above) == 0);
        assert!(north.received(above) != 0 && north.received(below) == 0);
    }

    #[test]
    fn a_target_bursts_when_the_light_that_completes_it_arrives() {
        // A yellow target at (5, 5) already takes green from the south-going laser at (5, 0),
        // five cells on; the red laser set at (14, 5) completes it nine cells on.
        let target = (5, 5, Piece::Pinwheel, 6, 0);
        let green = (5, 0, Piece::Laser, 2, 4);
        let before = lit(&[target, green]);
        let after = lit(&[target, green, (14, 5, Piece::Laser, 4, 6)]);
        let mut c = Choreography::new();
        observe(&mut c, &before, &after, 1000.0);
        let burst = c.bursts()[0];
        assert_eq!(burst.cell, board_index(5, 5));
        assert_eq!(c.reach().cells[burst.cell], 5, "green got there first");
        let front = c.moment(burst.at).sweep.expect("a sweep").front;
        assert!((front - 9.0).abs() < 1e-3, "red's arrival: {front}");
    }

    #[test]
    fn a_target_two_new_colours_light_bursts_when_the_later_one_arrives() {
        // A yellow target at (5, 5): red from (4, 5) gets there one cell on, green from (5, 14)
        // nine cells on. It is complete, and bursts, only at 9.
        let target = (5, 5, Piece::Pinwheel, 6, 0);
        let before = lit(&[target]);
        let after = lit(&[
            target,
            (4, 5, Piece::Laser, 4, 2),
            (5, 14, Piece::Laser, 2, 0),
        ]);
        let mut c = Choreography::new();
        observe(&mut c, &before, &after, 1000.0);
        let burst = c.bursts()[0];
        assert_eq!(burst.cell, board_index(5, 5));
        let front = c.moment(burst.at).sweep.expect("a sweep").front;
        assert!((front - 9.0).abs() < 1e-3, "green's arrival: {front}");
    }

    #[test]
    fn a_target_lit_by_losing_light_bursts_as_the_sweep_starts() {
        // A red target at (9, 5) takes red from (2, 5) and blue from (9, 0): spoilt. Turning the
        // blue laser away leaves it red alone, lit, with nothing new to wait for.
        let target = (9, 5, Piece::Pinwheel, 4, 0);
        let red = (2, 5, Piece::Laser, 4, 2);
        let before = lit(&[target, red, (9, 0, Piece::Laser, 1, 4)]);
        let after = lit(&[target, red, (9, 0, Piece::Laser, 1, 2)]);
        let mut c = Choreography::new();
        observe(&mut c, &before, &after, 1000.0);
        assert_eq!(c.bursts()[0].cell, board_index(9, 5));
        assert_eq!(c.bursts()[0].at, 1000.0);
    }

    #[test]
    fn a_turned_mirror_sweeps_from_the_mirror() {
        let mut c = Choreography::new();
        observe(&mut c, &mirror(7), &mirror(1), 1000.0);
        let m = c.moment(1000.0);
        let sweep = m.sweep.expect("a sweep");
        let mirror_reach = f32::from(c.reach().cells[board_index(9, 5)]);
        assert!(
            sweep.front <= mirror_reach + 0.5 && sweep.front >= mirror_reach - 0.5,
            "it starts at the mirror: {}",
            sweep.front
        );
        assert_eq!(m.fade, 1.0, "old strands at full");
        assert_eq!(
            m.turn.map(|t| t.0),
            Some(board_index(9, 5)),
            "and the mirror turns"
        );
        let end = c.end();
        assert!(
            (1000.0 + SWEEP_MIN_MS..=1000.0 + SWEEP_MAX_MS.max(FADE_MS) + TURN_MS).contains(&end),
            "{end}"
        );
    }

    #[test]
    fn next_wake_runs_until_the_last_cue_ends() {
        let mut c = Choreography::new();
        observe(&mut c, &mirror(7), &mirror(1), 0.0);
        assert_eq!(c.next_wake(0.0), Some(FRAME_MS));
        assert_eq!(c.next_wake(c.end()), None);
        assert_eq!(c.next_wake(SETTLE_MS), None);
    }

    #[test]
    fn a_turn_ends_on_its_target_angle() {
        let mut c = Choreography::new();
        observe(&mut c, &mirror(1), &mirror(2), 0.0);
        let spin = |t: f64| c.moment(t).turn.map_or(0.0, |t| t.1);
        assert!(
            (spin(0.0) + std::f32::consts::FRAC_PI_4).abs() < 1e-6,
            "starts one step back"
        );
        assert!(spin(75.0) < 0.0 && spin(75.0) > -std::f32::consts::FRAC_PI_4);
        assert_eq!(c.moment(TURN_MS).turn, None, "on its angle at 150 ms");
    }

    #[test]
    fn a_second_turn_of_the_turning_cell_goes_on_from_the_angle_shown() {
        // Clicked again two frames into its turn: the mirror's first frame of the new turn is
        // no further from the last frame shown than a turn's first (largest) step.
        let shown = |c: &Choreography, now: f64, dir: u8| {
            f32::from(dir) * FRAC_PI_4 + c.moment(now).turn.map_or(0.0, |t| t.1)
        };
        let mut c = Choreography::new();
        observe(&mut c, &mirror(1), &mirror(2), 0.0);
        let last = shown(&c, FRAME_MS, 2);
        observe(&mut c, &mirror(2), &mirror(3), 2.0 * FRAME_MS);
        let next = shown(&c, 2.0 * FRAME_MS, 3);
        let step = FRAC_PI_4 * ease::out_cubic((FRAME_MS / TURN_MS) as f32);
        assert!(
            next > last && next - last <= step,
            "from {last} to {next}: a frame steps at most {step}"
        );
        assert!(
            (shown(&c, 2.0 * FRAME_MS + TURN_MS, 3) - 3.0 * FRAC_PI_4).abs() < 1e-6,
            "it ends on its angle"
        );
    }

    #[test]
    fn a_turn_ends_when_its_cell_changes_otherwise() {
        // 30 ms into a turn, two cells change at once (a paste): the turning mirror's cell gets
        // a target, so its turn ends; when only other cells change, it runs on.
        let laser = (2, 5, Piece::Laser, 7, 2);
        let (a, b) = (
            lit(&[laser, (9, 5, Piece::Mirror, 0, 7)]),
            lit(&[laser, (9, 5, Piece::Mirror, 0, 1)]),
        );
        let replaced = lit(&[
            laser,
            (9, 5, Piece::Pinwheel, 0, 0),
            (4, 9, Piece::Mirror, 0, 0),
        ]);
        let elsewhere = lit(&[
            laser,
            (9, 5, Piece::Mirror, 0, 1),
            (4, 9, Piece::Mirror, 0, 0),
        ]);
        for (after, runs) in [(&replaced, false), (&elsewhere, true)] {
            let mut c = Choreography::new();
            observe(&mut c, &a, &b, 0.0);
            assert!(c.moment(16.0).turn.is_some(), "a turn runs");
            observe(&mut c, &b, after, 30.0);
            assert_eq!(c.moment(40.0).turn.is_some(), runs, "runs on: {runs}");
        }
    }

    #[test]
    fn a_reflector_turned_half_round_turns_not_at_all() {
        let (a, b) = (
            lit(&[(9, 5, Piece::Splitter, 0, 1)]),
            lit(&[(9, 5, Piece::Splitter, 0, 5)]),
        );
        let mut c = Choreography::new();
        observe(&mut c, &a, &b, 0.0);
        assert_eq!(
            c.moment(0.0).turn,
            None,
            "dir 1 and 5 draw alike (period 4)"
        );
    }

    #[test]
    fn black_targets_never_burst_and_lit_ones_burst_when_the_front_arrives() {
        // A blue beam east onto a black target; a mirror set down at (6, 5) turns it south onto
        // a blue one. The black target, dark at last, is satisfied too, but never bursts.
        let (black, blue) = (board_index(12, 5), board_index(6, 9));
        let targets = [
            (2, 5, Piece::Laser, 1, 2),
            (12, 5, Piece::Pinwheel, 0, 0),
            (6, 9, Piece::Pinwheel, 1, 0),
        ];
        let before = lit(&targets);
        let after = lit(&[
            targets[0],
            targets[1],
            targets[2],
            (6, 5, Piece::Mirror, 0, 7),
        ]);
        assert!(before.1.received(black) != 0 && before.1.received(blue) == 0);
        assert!(after.1.received(black) == 0 && after.1.received(blue) == 1);
        let mut c = Choreography::new();
        observe(&mut c, &before, &after, 0.0);
        let cells: Vec<usize> = c.bursts().iter().map(|b| b.cell).collect();
        assert_eq!(cells, [blue], "the blue target lit; the black one never");
        assert!(c.bursts()[0].at > 0.0, "when the front gets there");
    }

    #[test]
    fn nothing_bursts_before_its_sweep_starts() {
        let beams = Beams {
            at: 100.0,
            began: 100.0,
            r_min: 5.0,
            speed: 0.02,
            sweep_end: 350.0,
            fade_end: 260.0,
        };
        assert_eq!(beams.arrival(2.0), 100.0, "a target behind the front");
        assert_eq!(beams.arrival(7.0), 200.0);
    }

    /// 40 targets at cells 0, 5, … 195, lit when `lit` holds for their index.
    fn targets(lit: impl Fn(usize) -> bool) -> (Board, BeamField) {
        let mut board = Board::default();
        let mut field = BeamField::default();
        for c in 0..40 {
            board.cells[c * 5] = Cell {
                piece: Piece::Pinwheel,
                colour: 1,
                dir: 0,
            };
            if lit(c) {
                field.inbound[c * 5][2] = 1;
            }
        }
        (board, field)
    }

    #[test]
    fn a_new_edits_bursts_push_out_the_oldest() {
        let mut c = Choreography::new();
        observe(&mut c, &targets(|_| false), &targets(|k| k < 20), 0.0);
        assert_eq!(c.bursts().len(), 20);
        observe(&mut c, &targets(|k| k < 20), &targets(|_| true), 100.0);
        assert_eq!(c.bursts().len(), MAX_BURSTS);
        assert!(
            c.bursts().iter().map(|b| b.cell).eq((8..40).map(|k| k * 5)),
            "the first edit's 8 oldest dropped, all 20 new kept"
        );
    }

    #[test]
    fn bursts_past_the_cap_keep_the_lowest_cells() {
        let mut c = Choreography::new();
        observe(&mut c, &targets(|_| false), &targets(|_| true), 0.0);
        assert_eq!(c.bursts().len(), MAX_BURSTS);
        assert!(
            c.bursts()
                .iter()
                .map(|b| b.cell)
                .eq((0..MAX_BURSTS).map(|k| k * 5))
        );
    }

    #[test]
    fn reduced_motion_leaves_no_cue_longer_than_150_ms() {
        let mut c = Choreography::new();
        c.set_reduced(true);
        observe(&mut c, &mirror(7), &mirror(1), 0.0);
        assert_eq!(c.next_wake(0.0), None, "beams and turns take 0 ms");
        c.observe(&level_change(&mirror(7), &mirror(1)), 10.0);
        assert!(c.moment(10.0).level.is_some(), "the level fade stays");
        assert_eq!(c.next_wake(10.0 + REDUCED_FADE_MS), None);
        assert!(c.moment(10.0).sweep.is_none(), "no sweep after it");
    }

    #[test]
    fn reduced_motion_mid_cue_settles_at_once() {
        let mut c = Choreography::new();
        observe(&mut c, &mirror(7), &mirror(1), 0.0);
        c.set_reduced(true);
        let m = c.moment(50.0);
        assert!(m.sweep.is_none() && m.turn.is_none() && m.fade == 0.0 && m.lift == 1.0);
        assert_eq!(c.next_wake(50.0), None);
    }

    #[test]
    fn a_level_change_fades_then_sweeps_from_the_lasers_without_bursts() {
        let mut c = Choreography::new();
        c.observe(
            &level_change(
                &mirror(1),
                &lit(&[(2, 5, Piece::Laser, 1, 2), (12, 5, Piece::Pinwheel, 1, 0)]),
            ),
            0.0,
        );
        assert!(c.bursts().is_empty());
        assert_eq!(c.moment(90.0).level.map(|k| k > 0.0 && k < 1.0), Some(true));
        assert!(
            c.moment(90.0).sweep.is_some_and(|s| s.front < 0.0),
            "no beam before the fade ends"
        );
        assert!(
            c.moment(LEVEL_MS + 1.0)
                .sweep
                .is_some_and(|s| s.front >= 0.0),
            "then from the lasers"
        );
        assert_eq!(c.moment(90.0).fade, 0.0, "another level's beams don't fade");
    }

    /// Ruling F4: a chip chosen in the sheet closes it in the paint that changes the level, so
    /// the board doesn't fade (the sheet's own close cue covers the change).
    #[test]
    fn a_level_chosen_in_the_sheet_does_not_fade() {
        let after = lit(&[(2, 5, Piece::Laser, 1, 2)]);
        for sheet in [[true, false], [true, true], [false, true]] {
            let mut c = Choreography::new();
            c.observe(
                &Observed {
                    sheet,
                    ..level_change(&mirror(1), &after)
                },
                0.0,
            );
            assert_eq!(c.moment(90.0).level, None, "{sheet:?}");
            assert!(
                c.moment(0.0).sweep.is_some_and(|s| s.front >= 0.0),
                "{sheet:?}: with no fade the beams sweep at once"
            );
        }
        let mut c = Choreography::new();
        c.observe(
            &Observed {
                sheet: [true, false],
                ..level_change(&mirror(1), &after)
            },
            0.0,
        );
        assert!(
            c.moment(50.0).sheet.is_some_and(|p| !p.scrim),
            "the sheet closes"
        );
    }

    /// A level changed while the sheet's panel still fades out doesn't fade either (the frame
    /// kept would hold the panel); once the close is over, one does.
    #[test]
    fn a_level_changed_while_the_sheet_closes_does_not_fade() {
        let after = lit(&[(2, 5, Piece::Laser, 1, 2)]);
        let mut c = Choreography::new();
        c.observe(
            &Observed {
                sheet: [true, false],
                ..one_change(&mirror(1), &mirror(1))
            },
            0.0,
        );
        c.observe(&level_change(&mirror(1), &after), 100.0);
        assert!(c.moment(100.0).sheet.is_some(), "still closing");
        assert_eq!((c.moment(100.0).level, c.level_since()), (None, None));
        c.observe(&level_change(&after, &mirror(1)), 100.0 + SHEET_MS);
        assert_eq!(c.level_since(), Some(100.0 + SHEET_MS), "closed: it fades");
        // Mid-fade, a change under the sheet (with no fade of its own) ends that fade.
        c.observe(
            &Observed {
                sheet: [true, true],
                ..level_change(&mirror(1), &after)
            },
            150.0 + SHEET_MS,
        );
        assert_eq!(
            (c.moment(150.0 + SHEET_MS).level, c.level_since()),
            (None, None)
        );
    }

    #[test]
    fn an_unwin_ends_the_win() {
        let mut c = Choreography::new();
        c.observe(
            &Observed {
                won: [false, true],
                ..one_change(&mirror(1), &mirror(1))
            },
            0.0,
        );
        assert!(c.moment(100.0).win.is_some(), "the win runs");
        // A restart (same slot) mid-win: the beams change back and the level is unsolved.
        c.observe(
            &Observed {
                won: [true, false],
                ..one_change(&mirror(1), &mirror(7))
            },
            100.0,
        );
        assert_eq!(c.moment(100.0).win, None);
        assert_eq!(c.moment(500.0).win, None);
        let end = c.end();
        assert!(
            end < 100.0 + WIN_MS,
            "only the restart's own cues run: {end}"
        );
        assert_eq!(c.next_wake(end), None);
    }

    #[test]
    fn a_drag_that_ends_lifts_nothing() {
        let held = |x| Drag {
            source: board_index(9, 5),
            piece: Cell {
                piece: Piece::Mirror,
                colour: 0,
                dir: 1,
            },
            x,
            y: 40,
        };
        for end in [mirror(1), lit(&[(2, 5, Piece::Laser, 7, 2)])] {
            let mut c = Choreography::new();
            c.observe(
                &Observed {
                    drag: [None, Some(held(10))],
                    ..one_change(&mirror(1), &mirror(1))
                },
                0.0,
            );
            assert_eq!(c.moment(30.0).lift, ease::out_cubic(1.0 / 3.0), "it rises");
            // Dropped back (a cancel) or elsewhere, 30 ms in.
            c.observe(
                &Observed {
                    drag: [Some(held(20)), None],
                    ..one_change(&mirror(1), &end)
                },
                30.0,
            );
            let m = c.moment(40.0);
            assert_eq!(m.lift, 1.0, "nothing held, nothing lifts");
            let land = m.land.expect("it lands");
            assert_eq!((land.cell, land.from), (board_index(9, 5), (20, 40)));
            // Released a third of the way into its rise: it lands from that lift.
            assert_eq!(
                land.lift,
                ease::out_cubic(1.0 / 3.0),
                "from where it had risen"
            );
        }
    }

    #[test]
    fn a_sheet_turned_back_mid_fade_goes_on_from_where_it_is() {
        let mut c = Choreography::new();
        let toggle = |c: &mut Choreography, sheet, now| {
            c.observe(
                &Observed {
                    sheet,
                    ..one_change(&mirror(1), &mirror(1))
                },
                now,
            );
        };
        toggle(&mut c, [false, true], 0.0);
        let half = c.moment(60.0).sheet.unwrap().alpha;
        toggle(&mut c, [true, false], 60.0);
        let back = c.moment(60.0).sheet.unwrap();
        assert!((back.alpha - half).abs() < 1e-4, "{} vs {half}", back.alpha);
        assert!(
            c.moment(80.0).sheet.unwrap().alpha < back.alpha,
            "then it closes"
        );
        assert!(c.end() < 60.0 + SHEET_MS, "sooner than a whole close");
        // Turned back again: it opens on from there.
        let at = c.moment(80.0).sheet.unwrap().alpha;
        toggle(&mut c, [false, true], 80.0);
        assert!((c.moment(80.0).sheet.unwrap().alpha - at).abs() < 1e-4);
    }

    #[test]
    fn a_win_during_an_earlier_sweep_waits_for_it() {
        // A turn sweeps at 0; the cheat key wins at 100 ms with the beams as they are: the
        // packets wait for the sweep still running, on its reach.
        let mut c = Choreography::new();
        observe(&mut c, &mirror(7), &mirror(1), 0.0);
        let reach = *c.reach();
        let end = c.end();
        c.observe(
            &Observed {
                won: [false, true],
                ..one_change(&mirror(1), &mirror(1))
            },
            100.0,
        );
        assert_eq!(*c.reach(), reach, "the sweep's reach");
        let w = c.moment(150.0).win.expect("the win is pending");
        assert!(w.packet < 0.0 && w.reveal == 0.0, "nothing yet: {w:?}");
        assert!(
            c.end() >= end + WIN_MS,
            "it runs from the sweep's end: {}",
            c.end()
        );
    }

    #[test]
    fn a_win_during_a_level_fade_waits_for_the_levels_sweep() {
        // Another level fades in (its beams sweep from 180 ms); the cheat key wins at 50 ms:
        // no packet runs over the board while it shows no beams.
        let (before, after) = (mirror(1), lit(&[(2, 5, Piece::Laser, 1, 2)]));
        let mut c = Choreography::new();
        c.observe(&level_change(&before, &after), 0.0);
        c.observe(
            &Observed {
                slot: [4, 4],
                won: [false, true],
                ..one_change(&after, &after)
            },
            50.0,
        );
        for now in [60.0, 150.0, LEVEL_MS + 50.0] {
            let w = c.moment(now).win.expect("the win is pending");
            assert!(w.packet < 0.0, "at {now}: {w:?}");
        }
        assert!(c.end() >= LEVEL_MS + SWEEP_MIN_MS + WIN_MS);
    }

    #[test]
    fn a_win_with_a_sweep_waits_for_it() {
        let mut c = Choreography::new();
        c.observe(
            &Observed {
                won: [false, true],
                ..one_change(&mirror(7), &mirror(1))
            },
            0.0,
        );
        assert!(c.moment(0.0).sweep.is_some(), "a sweep runs");
        let w = c.moment(100.0).win.expect("the win is pending");
        assert!(w.packet < 0.0 && w.reveal == 0.0, "nothing yet: {w:?}");
    }

    #[test]
    fn a_win_that_moves_no_beam_runs_its_packets_on_the_field_shown() {
        // The cheat key wins with the beams as they are: no sweep, so the reach the packets run
        // on is the shown field's (the laser at (2, 5), the mirror 7 cells on).
        let mut c = Choreography::new();
        c.observe(
            &Observed {
                won: [false, true],
                ..one_change(&mirror(1), &mirror(1))
            },
            0.0,
        );
        assert_eq!(c.reach().cells[board_index(9, 5)], 7);
        let w = c.moment(100.0).win.expect("the win runs");
        assert!(w.packets && (w.packet - 6.0).abs() < 1e-4, "{w:?}");
    }

    #[test]
    fn the_sheet_fades_and_rises_and_closes_without_its_scrim() {
        let mut c = Choreography::new();
        c.observe(
            &Observed {
                sheet: [false, true],
                ..one_change(&mirror(1), &mirror(1))
            },
            0.0,
        );
        let p = c.moment(0.0).sheet.unwrap();
        assert_eq!((p.alpha, p.rise_pt, p.scrim), (0.0, SHEET_RISE_PT, true));
        assert_eq!(c.moment(SHEET_MS).sheet, None);
        c.observe(
            &Observed {
                sheet: [true, false],
                ..one_change(&mirror(1), &mirror(1))
            },
            1000.0,
        );
        let p = c.moment(1050.0).sheet.unwrap();
        assert!(!p.scrim && p.alpha < 1.0 && p.rise_pt > 0.0);
    }

    #[test]
    fn a_late_frame_settles_every_cue() {
        let mut c = Choreography::new();
        c.observe(
            &Observed {
                won: [false, true],
                ..one_change(&mirror(7), &mirror(1))
            },
            0.0,
        );
        let m = c.moment(30_000.0);
        assert!(m.sweep.is_none() && m.win.is_none() && m.turn.is_none() && m.fade == 0.0);
        assert_eq!(c.next_wake(30_000.0), None);
    }

    #[test]
    fn the_same_now_twice_is_one_frame_and_an_earlier_now_shows_the_start() {
        let mut c = Choreography::new();
        observe(&mut c, &mirror(7), &mirror(1), 500.0);
        assert_eq!(c.moment(600.0), c.moment(600.0), "sampling is pure");
        observe(&mut c, &mirror(1), &mirror(1), 600.0);
        assert!(
            c.moment(600.0).sweep.is_some(),
            "an unchanged paint keeps the cues"
        );
        let early = c.moment(400.0);
        assert_eq!(
            early.turn.map(|t| t.1),
            Some(-std::f32::consts::FRAC_PI_2),
            "the start"
        );
        assert!(early.sweep.is_some_and(|s| s.front.is_finite()));
    }

    /// A change that only takes light away sweeps for 0 ms: nothing to sweep, no NaN, only the
    /// old strands' fade.
    #[test]
    fn zero_durations_sample_settled() {
        let mut c = Choreography::new();
        observe(&mut c, &mirror(1), &mirror(3), 0.0);
        for now in [-10.0, 0.0, 80.0] {
            let m = c.moment(now);
            assert!(m.sweep.is_none() && m.turn.is_some() && m.fade > 0.0 && m.fade <= 1.0);
            assert!(m.turn.is_some_and(|t| t.1.is_finite()) && m.lift.is_finite());
        }
        assert_eq!(c.end(), FADE_MS, "the fade outlasts the 150 ms turn");
        assert_eq!(c.next_wake(FADE_MS), None);
    }

    #[test]
    fn a_settled_paint_starts_nothing() {
        let mut c = Choreography::new();
        observe(&mut c, &mirror(7), &mirror(1), f64::INFINITY);
        assert_eq!(c.end(), f64::NEG_INFINITY);
        assert_eq!(c.next_wake(0.0), None);
    }

    /// `before` → `after` with no other change.
    fn one_change<'a>(
        before: &'a (Board, BeamField),
        after: &'a (Board, BeamField),
    ) -> Observed<'a> {
        Observed {
            board: [&before.0, &after.0],
            field: [&before.1, &after.1],
            drag: [None, None],
            slot: [3, 3],
            won: [false, false],
            sheet: [false, false],
        }
    }

    fn level_change<'a>(
        before: &'a (Board, BeamField),
        after: &'a (Board, BeamField),
    ) -> Observed<'a> {
        Observed {
            slot: [3, 4],
            ..one_change(before, after)
        }
    }
}
