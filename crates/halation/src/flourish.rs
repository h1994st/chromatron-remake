//! `flourish`: motion's passing light, drawn over the composed output with no bloom (ruling
//! M1): sweep heads, old strands fading out, newly lit targets' bursts, and a win's packets and
//! target pulses (spec §7.3, §9.1). Added in the dark theme; in the light one (`Over`, ruling
//! L5) painted as normal-blend strokes in the halo colours, so they show on a white board. A
//! frame's `Cast` is worked out once (`Cast::frame`) and then drawn into each damaged rect; it
//! also reports the device boxes it touches, so the lightbox repaints them the next frame.
//! Colours from the palette; fixed arrays, no allocation.

use lumen::{BLUE, BOARD_CELLS, BeamField, Board, Colour, DX, DY, GREEN, Piece, RED, SIDE, WHITE};

use crate::DeviceRect;
use crate::beamline::{
    Reach, centre_trim, for_each_head, for_each_line, half_colour, on_board, strands,
};
use crate::choreography::{
    Burst, MAX_BURSTS, Moment, PACKET_CELLS_PER_S, PULSE_MS, RING_MS, SPARKS_MS, WIN_MS, WinMoment,
    ease,
};
use crate::darkroom::{beam_stroke, half_ends};
use crate::etch::{Blend, Paint, Pt, Shape, Stops, Target, clip_rect, fill, heading, pt};
use crate::gouache;
use crate::patchwork::Patchwork;
use crate::piece_art::light_blend;
use crate::spectrum::{LightMode, Palette};
use crate::tableau::Tableau;

/// Most boxes a frame's flourish reports (merged past it).
pub const MAX_FLOURISH: usize = 16;
/// Most heads a frame shows: one per channel of every half-segment.
pub const MAX_HEADS: usize = BOARD_CELLS * 8 * 3;
/// Most fading strands a frame shows: one per half-segment.
pub const MAX_STRANDS: usize = BOARD_CELLS * 8;
/// Most win packets a frame shows: one per half-segment.
pub const MAX_PACKETS: usize = BOARD_CELLS * 8;
/// Most target pulses a frame shows: one per cell.
pub const MAX_PULSES: usize = BOARD_CELLS;

/// A win packet's radius, in u, and its alpha at its centre (spec §9.1 "Win": white α 0.8,
/// 0.3u).
const PACKET_RADIUS: f32 = 0.3;
const PACKET_ALPHA: f32 = 0.8;
/// How far a target's pulse reaches, in u: its glow over the whole face, out past the ring
/// (0.3u arcs), since a lit core is already near white and "+30% core" there adds only 1–4%
/// (Task 14's key-frame review); and how much light the pulse adds at its height (30%).
const PULSE_RADIUS: f32 = 0.55;
const PULSE_GAIN: f32 = 0.3;
/// Over the win's last this many ms its packets and pulses fade out, so one still running when
/// the win's 1.4 s are up (a target more than 60 cells out, a path past 84) ends at nothing.
const WIN_TAIL_MS: f32 = 200.0;

/// A sweep head's radius, in u (spec §7.3 "Moving front head").
const HEAD_RADIUS: f32 = 0.22;
/// Half a beam line's width, in u: how far a fading strand reaches past its axis (`Additive`;
/// in `Over`, `gouache::STRAND_REACH`).
const STRAND_REACH: f32 = 0.032;

/// A burst's size, in u (the trailer's stills draw 1.15 cells), and its sparks (spec: 14, not
/// the trailer's 28).
const BURST_SIZE: f32 = 1.2;
const SPARKS: usize = 14;
/// Everything a burst draws lies within this many u of its target: the ring's 1.4 sizes plus
/// its width, the fastest spark's 1.65 sizes plus its own.
const BURST_REACH: f32 = 2.1;

/// The trailer's `prng` (mulberry32): a burst's sparks are the same on every frame.
pub struct Sparkler(u32);

impl Sparkler {
    pub fn new(seed: u32) -> Sparkler {
        Sparkler(seed)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f64 / 4_294_967_296.0) as f32
    }
}

/// A spark's stroke this frame: from `a` to `b`, `width` px wide, in channel `tint` (0: white)
/// at `alpha`.
#[derive(Clone, Copy, Debug, Default)]
struct Spark {
    a: Pt,
    b: Pt,
    width: f32,
    tint: Colour,
    alpha: f32,
}

/// A burst this frame: its ring (radius, width and α; none once the wave is over), its live
/// sparks, and the device px it may touch.
#[derive(Clone, Copy, Debug, Default)]
struct Flare {
    centre: Pt,
    mask: Colour,
    ring: Option<(f32, f32, f32)>,
    sparks: [Spark; SPARKS],
    spark_len: usize,
    bounds: DeviceRect,
}

/// A burst's age in seconds at `now`, while it draws.
fn burst_age(b: &Burst, now: f64) -> Option<f32> {
    let age = ((now - b.at) / 1000.0) as f32;
    (0.0..=(SPARKS_MS / 1000.0) as f32)
        .contains(&age)
        .then_some(age)
}

/// The trailer's `drawBurst` at `age` seconds, with the spec's timings: a ring wave over 0.6 s
/// (outExpo), and 14 sparks in the target's channels and white flying out, slowing, shrinking
/// and fading, each living at most 0.9 s; seeded by the target's cell.
fn flare(centre: Pt, u: f32, b: &Burst, age: f32) -> Flare {
    let size = BURST_SIZE * u;
    let ring_s = (RING_MS / 1000.0) as f32;
    let ring = (age < ring_s).then(|| {
        let wave = ease::out_expo(age / ring_s);
        (
            size * (0.3 + 1.1 * wave),
            size * 0.05 * (1.0 - wave) + 0.5,
            0.45 * (1.0 - wave).powi(2),
        )
    });
    // The trailer's order: the target's channels red, green, blue, then white.
    let mut tints = [0; 4];
    let mut n = 0;
    for bit in [RED, GREEN, BLUE] {
        if b.mask & bit != 0 {
            tints[n] = bit;
            n += 1;
        }
    }
    let n = n + 1;
    let mut f = Flare {
        centre,
        mask: b.mask,
        ring,
        bounds: around(centre, centre, BURST_REACH * u),
        ..Flare::default()
    };
    let mut random = Sparkler::new(101 + 7 * b.cell as u32);
    for _ in 0..SPARKS {
        let angle = random.next() * std::f32::consts::TAU;
        let speed = 0.35 + random.next().powf(0.7) * 1.3;
        let life = 0.45 + random.next() * 0.45;
        let tint = tints[((random.next() * n as f32) as usize).min(n - 1)];
        let thick = size * (0.018 + random.next() * 0.03);
        let k = age / life;
        if k >= 1.0 {
            continue;
        }
        let along = |s: f32| size * speed * (1.0 - (-4.5 * s).exp());
        let (head, tail) = (along(age), along((age - 0.035 * speed).max(0.0)));
        // (cos, sin) of the trailer's angle, through `heading` (no libm `sinf`/`cosf`).
        let d = heading(angle + std::f32::consts::FRAC_PI_2);
        f.sparks[f.spark_len] = Spark {
            a: centre + d * tail,
            b: centre + d * head + pt(0.01, 0.0),
            width: thick * (1.0 - 0.5 * k),
            tint,
            alpha: (1.0 - k).powf(1.4),
        };
        f.spark_len += 1;
    }
    f
}

/// Draws flare `f` over `t` in `blend`, only inside `clip`: its ring, then its sparks. In
/// `Over` the ring is the target's halo (a white target's blue-grey: its mix, white, reads as
/// nothing on a white board) and a white spark carries the white halo (ruling L5).
fn draw_flare(t: &mut dyn Target, clip: DeviceRect, palette: &Palette, f: &Flare, blend: Blend) {
    if let Some((radius, width, alpha)) = f.ring {
        let tone = if blend == Blend::Over {
            palette.halo(f.mask)
        } else {
            palette.mix(f.mask)
        };
        let ring = Shape::ring(f.centre, radius, width);
        fill(
            t,
            clip,
            &[ring],
            &Paint::Solid(tone.alpha(alpha)),
            1.0,
            blend,
        );
    }
    for s in &f.sparks[..f.spark_len] {
        let tint = match (s.tint, blend) {
            (0, Blend::Over) => palette.halo(WHITE),
            (0, _) => palette.white,
            (bit, _) => palette.channel(bit),
        };
        let stroke = Shape::line(s.a, s.b, s.width);
        fill(
            t,
            clip,
            &[stroke],
            &Paint::Solid(tint.alpha(s.alpha)),
            1.0,
            blend,
        );
    }
}

/// A head: its centre, its channel (in `Over`, the union of the channels whose heads share its
/// centre), and the device px it touches.
#[derive(Clone, Copy, Debug, Default)]
struct Head {
    at: Pt,
    bit: Colour,
    bounds: DeviceRect,
}

/// A fading strand: a straight run of fading half-segments of one mask, from `a` to `b` (its
/// ends trimmed at an aperture or a wall face), and the device px it touches.
#[derive(Clone, Copy, Debug, Default)]
struct Strand {
    a: Pt,
    b: Pt,
    mask: Colour,
    bounds: DeviceRect,
}

/// A win packet this frame: its centre, the beam's mask it rides, and the device px it touches.
#[derive(Clone, Copy, Debug, Default)]
struct Packet {
    at: Pt,
    mask: Colour,
    bounds: DeviceRect,
}

/// A target pulsing this frame: its centre, colour and how bright its core's extra light is.
#[derive(Clone, Copy, Debug, Default)]
struct Pulse {
    centre: Pt,
    mask: Colour,
    alpha: f32,
    bounds: DeviceRect,
}

/// One frame's passing light, worked out once and drawn into every damaged rect: the fading
/// strands (each a run of halves, stroked whole so no junction is added twice) at `fade`, the
/// heads, the bursts, a win's packets and pulses, and the boxes they touch. Boxed: fixed arrays.
pub struct Cast {
    u: f32,
    fade: f32,
    /// Whether this frame was worked out in `Over` (the palette's light mode): `draw` paints
    /// in the same mode.
    over: bool,
    /// How far a fading strand reaches past its axis this frame, px: half the widest stroke
    /// the palette's mode draws it with.
    strand_reach: f32,
    /// The packets' alpha this frame (`PACKET_ALPHA`, less in the win's tail).
    packet_alpha: f32,
    heads: [Head; MAX_HEADS],
    head_len: usize,
    strands: [Strand; MAX_STRANDS],
    strand_len: usize,
    flares: [Flare; MAX_BURSTS],
    flare_len: usize,
    packets: [Packet; MAX_PACKETS],
    packet_len: usize,
    pulses: [Pulse; MAX_PULSES],
    pulse_len: usize,
    boxes: Patchwork<MAX_FLOURISH>,
}

/// What the flourish draws over: the layout, the palette (its light mode sets the cues'
/// recipes and boxes), the board and field painted, the reach the cues run on (spec §9.2), the
/// running bursts and the frame's time.
pub struct Stage<'a> {
    pub tableau: &'a Tableau,
    pub palette: &'a Palette,
    pub board: &'a Board,
    pub field: &'a BeamField,
    pub reach: &'a Reach,
    pub bursts: &'a [Burst],
    pub now: f64,
}

/// The device px around `a` and `b`, grown by `r` and 2 px.
fn around(a: Pt, b: Pt, r: f32) -> DeviceRect {
    DeviceRect::new(
        (a.x.min(b.x) - r - 2.0).floor() as i32,
        (a.y.min(b.y) - r - 2.0).floor() as i32,
        (a.x.max(b.x) + r + 2.0).ceil() as i32,
        (a.y.max(b.y) + r + 2.0).ceil() as i32,
    )
}

/// Bits of half (`cell`, `side`) in the old field and not the new: an old strand going out.
fn fading(moment: &Moment, field: &BeamField, cell: usize, side: usize) -> Colour {
    half_colour(moment.old, cell, side) & !half_colour(field, cell, side)
}

/// Where half (`cell`, `side`)'s packet is when the packets are at `packet` (reach units), as a
/// fraction of the half from the cell's centre: it runs over the half from its earliest
/// strand's start (`strands`: a bit crossing the half both ways has two), half a cell in half a
/// reach unit; none off the half or inside its trimmed centre.
fn packet_at(
    field: &BeamField,
    reach: &Reach,
    board: &Board,
    cell: usize,
    side: usize,
    packet: f32,
) -> Option<f32> {
    let mask = half_colour(field, cell, side);
    let (start, inward) = [RED, GREEN, BLUE]
        .into_iter()
        .filter(|b| mask & b != 0)
        .flat_map(|b| strands(field, reach, cell, side, b).into_iter().flatten())
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    let g = (packet - start) * 2.0;
    if !(0.0..1.0).contains(&g) {
        return None;
    }
    let f = if inward { 1.0 - g } else { g };
    (f >= centre_trim(board, field, cell, side)).then_some(f)
}

/// How far (0–1) target `cell`'s pulse has gone `since_ms` into the win, when it is lit (not
/// black): it pulses for `PULSE_MS` from when the packets reach it.
fn pulse(
    board: &Board,
    field: &BeamField,
    reach: &Reach,
    cell: usize,
    since_ms: f32,
) -> Option<f32> {
    let c = board.cells[cell];
    if c.piece != Piece::Pinwheel || c.colour == 0 || field.received(cell) != c.colour {
        return None;
    }
    let arrive = f32::from(reach.cells[cell]) / PACKET_CELLS_PER_S as f32 * 1000.0;
    let k = (since_ms - arrive) / PULSE_MS as f32;
    (0.0..1.0).contains(&k).then_some(k)
}

impl Cast {
    pub fn new() -> Box<Cast> {
        Box::new(Cast {
            u: 0.0,
            fade: 0.0,
            over: false,
            strand_reach: 0.0,
            packet_alpha: 0.0,
            heads: [Head::default(); MAX_HEADS],
            head_len: 0,
            strands: [Strand::default(); MAX_STRANDS],
            strand_len: 0,
            flares: [Flare::default(); MAX_BURSTS],
            flare_len: 0,
            packets: [Packet::default(); MAX_PACKETS],
            packet_len: 0,
            pulses: [Pulse::default(); MAX_PULSES],
            pulse_len: 0,
            boxes: Patchwork::new(),
        })
    }

    /// Works out the frame at `moment` over `stage`: its fading strands, its heads, its bursts,
    /// a win's packets and pulses, and their boxes.
    pub fn frame(&mut self, stage: &Stage, moment: &Moment) {
        let u = stage.tableau.cell as f32;
        (self.u, self.fade) = (u, moment.fade);
        let over = stage.palette.light_mode == LightMode::Over;
        self.over = over;
        self.strand_reach = if over {
            gouache::STRAND_REACH
        } else {
            STRAND_REACH
        } * u;
        self.head_len = 0;
        self.strand_len = 0;
        self.flare_len = 0;
        self.packet_len = 0;
        self.pulse_len = 0;
        self.boxes.clear();
        for b in stage.bursts {
            if let Some(age) = burst_age(b, stage.now)
                && self.flare_len < MAX_BURSTS
            {
                let f = flare(stage.tableau.cell_centre(b.cell), u, b, age);
                self.boxes.add(f.bounds);
                self.flares[self.flare_len] = f;
                self.flare_len += 1;
            }
        }
        if moment.fade > 0.0 {
            self.fading_strands(stage, moment);
        }
        if let Some(view) = &moment.sweep {
            let r = HEAD_RADIUS * u;
            for_each_head(view, stage.board, stage.field, |cell, side, at, bit| {
                let (c, e) = half_ends(stage.tableau, cell, side);
                let p = c + (e - c) * at;
                // In `Over` the channels' heads at one point are one head of their union: painted
                // one over another, a white front's red, green and blue heads would read blue,
                // where white light's halo is blue-grey (ruling L5). A half's heads come
                // together, at most three at a point.
                if over {
                    let from = self.head_len.saturating_sub(2);
                    let heads = &mut self.heads[from..self.head_len];
                    if let Some(h) = heads.iter_mut().find(|h| h.at == p) {
                        h.bit |= bit;
                        return;
                    }
                }
                let bounds = around(p, p, r);
                self.boxes.add(bounds);
                if self.head_len < MAX_HEADS {
                    self.heads[self.head_len] = Head { at: p, bit, bounds };
                    self.head_len += 1;
                }
            });
        }
        if let Some(win) = moment.win.filter(|w| w.packets) {
            self.win(stage, &win);
        }
    }

    /// A win's packets, one on each lit half they are running over, and the pulses of the lit
    /// targets they have reached (spec §9.1 "Win").
    fn win(&mut self, stage: &Stage, win: &WinMoment) {
        let u = self.u;
        let (field, reach, board) = (stage.field, stage.reach, stage.board);
        let tail = ((WIN_MS as f32 - win.since_ms) / WIN_TAIL_MS).clamp(0.0, 1.0);
        self.packet_alpha = PACKET_ALPHA * tail;
        for cell in 0..BOARD_CELLS {
            for side in 0..8 {
                let Some(f) = packet_at(field, reach, board, cell, side, win.packet) else {
                    continue;
                };
                let (c, e) = half_ends(stage.tableau, cell, side);
                let at = c + (e - c) * f;
                let bounds = around(at, at, PACKET_RADIUS * u);
                self.boxes.add(bounds);
                if self.packet_len < MAX_PACKETS {
                    let mask = half_colour(field, cell, side);
                    self.packets[self.packet_len] = Packet { at, mask, bounds };
                    self.packet_len += 1;
                }
            }
            if let Some(k) = pulse(board, field, reach, cell, win.since_ms) {
                let centre = stage.tableau.cell_centre(cell);
                let bounds = around(centre, centre, PULSE_RADIUS * u);
                self.boxes.add(bounds);
                self.pulses[self.pulse_len] = Pulse {
                    centre,
                    mask: board.cells[cell].colour,
                    // sin(πk), through `heading` (no libm `sinf`).
                    alpha: PULSE_GAIN * heading(std::f32::consts::PI * k).x * tail,
                    bounds,
                };
                self.pulse_len += 1;
            }
        }
    }

    /// The fading halves, joined along each line into runs of one mask (cut where a centre is
    /// trimmed, as the beams' runs are), each reporting its halves' boxes.
    fn fading_strands(&mut self, stage: &Stage, moment: &Moment) {
        let reach = self.strand_reach;
        for_each_line(|forward, start| {
            let back = forward + 4;
            let (dx, dy) = (i32::from(DX[forward]), i32::from(DY[forward]));
            let (mut x, mut y) = ((start % SIDE) as i32, (start / SIDE) as i32);
            let mut open: Option<(Pt, Pt, Colour)> = None;
            while on_board(x, y) {
                let cell = y as usize * SIDE + x as usize;
                for side in [back, forward] {
                    let mask = fading(moment, stage.field, cell, side);
                    let (c, e) = half_ends(stage.tableau, cell, side);
                    let trim = if mask != 0 {
                        centre_trim(stage.board, moment.old, cell, side)
                    } else {
                        0.0
                    };
                    let cut = c + (e - c) * trim;
                    // The half in the line's direction: edge to centre, then centre to edge.
                    let (from, to) = if side == back { (e, cut) } else { (cut, e) };
                    if mask != 0 {
                        self.boxes.add(around(c, e, reach));
                    }
                    match open.as_mut() {
                        Some(run)
                            if run.2 == mask && mask != 0 && !(side == forward && trim > 0.0) =>
                        {
                            run.1 = to
                        }
                        _ => {
                            self.push_strand(open.take(), reach);
                            if mask != 0 {
                                open = Some((from, to, mask));
                            }
                        }
                    }
                    if side == back && trim > 0.0 {
                        self.push_strand(open.take(), reach);
                    }
                }
                (x, y) = (x + dx, y + dy);
            }
            self.push_strand(open, reach);
        });
    }

    fn push_strand(&mut self, run: Option<(Pt, Pt, Colour)>, reach: f32) {
        if let Some((a, b, mask)) = run
            && self.strand_len < MAX_STRANDS
        {
            let bounds = around(a, b, reach);
            self.strands[self.strand_len] = Strand { a, b, mask, bounds };
            self.strand_len += 1;
        }
    }

    /// The device boxes this frame draws into.
    pub fn boxes(&self) -> &Patchwork<MAX_FLOURISH> {
        &self.boxes
    }

    /// This frame over `t` (the output), only inside `clip`: the fading strands at the fade,
    /// the heads, the bursts, the packets, then the pulses. Added in `Additive`; in `Over`
    /// painted in the halo colours (ruling L5).
    pub fn draw(&self, t: &mut dyn Target, clip: DeviceRect, palette: &Palette) {
        // The mode `frame` merged the heads and sized the strands in (review T9 M1).
        let over = self.over;
        debug_assert_eq!(
            over,
            palette.light_mode == LightMode::Over,
            "a cast drawn in another light mode than it was framed in"
        );
        let blend = light_blend(palette);
        let u = self.u;
        for s in &self.strands[..self.strand_len] {
            if clip_rect(s.bounds, clip).is_some() {
                if over {
                    gouache::strand(t, clip, palette, (s.a, s.b), s.mask, u, self.fade);
                } else {
                    beam_stroke(t, clip, palette, (s.a, s.b), s.mask, (u, 1.0), self.fade);
                }
            }
        }
        let r = HEAD_RADIUS * u;
        for h in &self.heads[..self.head_len] {
            if clip_rect(h.bounds, clip).is_none() {
                continue;
            }
            let stops = if over {
                let halo = palette.halo(h.bit);
                Stops::new(&[(0.0, halo.alpha(0.8)), (1.0, halo.alpha(0.0))])
            } else {
                Stops::new(&[
                    (0.0, palette.core(h.bit).alpha(1.0)),
                    (1.0, palette.mix(h.bit).alpha(0.0)),
                ])
            };
            let head = Paint::Radial {
                centre: h.at,
                radius: r,
                stops,
            };
            fill(t, clip, &[Shape::disc(h.at, r)], &head, 1.0, blend);
        }
        for f in &self.flares[..self.flare_len] {
            if clip_rect(f.bounds, clip).is_some() {
                draw_flare(t, clip, palette, f, blend);
            }
        }
        // White light: on a light board its rim is the halo of the beam it rides (a white
        // beam's blue-grey), so it reads as light passing along that beam.
        let r = PACKET_RADIUS * u;
        for p in &self.packets[..self.packet_len] {
            if clip_rect(p.bounds, clip).is_none() {
                continue;
            }
            let a = self.packet_alpha;
            let stops = if over {
                let halo = palette.halo(p.mask);
                Stops::new(&[
                    (0.0, palette.white.alpha(a)),
                    (0.4, halo.alpha(0.6 * a)),
                    (1.0, halo.alpha(0.0)),
                ])
            } else {
                Stops::new(&[
                    (0.0, palette.white.alpha(a)),
                    (1.0, palette.white.alpha(0.0)),
                ])
            };
            let packet = Paint::Radial {
                centre: p.at,
                radius: r,
                stops,
            };
            fill(t, clip, &[Shape::disc(p.at, r)], &packet, 1.0, blend);
        }
        // A glow in the target's colour over its face, at the pulse's share of its light: its
        // halo in `Over` (a white target's blue-grey).
        let r = PULSE_RADIUS * u;
        for p in &self.pulses[..self.pulse_len] {
            if clip_rect(p.bounds, clip).is_none() || p.alpha <= 0.0 {
                continue;
            }
            let a = p.alpha;
            let tone = if over {
                palette.halo(p.mask)
            } else {
                palette.mix(p.mask)
            };
            let glow = Paint::Radial {
                centre: p.centre,
                radius: r,
                stops: Stops::new(&[
                    (0.0, tone.alpha(a)),
                    (0.6, tone.alpha(0.6 * a)),
                    (1.0, tone.alpha(0.0)),
                ]),
            };
            fill(t, clip, &[Shape::disc(p.centre, r)], &glow, 1.0, blend);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Easel;
    use crate::choreography::{Choreography, Observed};
    use crate::etch::Layer;
    use crate::spectrum::Rgb;
    use lumen::{Cell, Piece, board_index, trace};

    fn lit(pieces: &[(usize, usize, Piece, u8, u8)]) -> (Board, BeamField) {
        let mut b = Board::default();
        for &(x, y, piece, colour, dir) in pieces {
            b.cells[board_index(x, y)] = Cell { piece, colour, dir };
        }
        let mut field = BeamField::default();
        trace(&b, &mut field);
        (b, field)
    }

    /// A white laser east onto a mirror at (9, 5) turned from south (dir 7) to north (dir 1),
    /// observed at 0 ms: the south strand fades while the north one sweeps in.
    fn turned() -> ((Board, BeamField), (Board, BeamField), Box<Choreography>) {
        let laser = (2, 5, Piece::Laser, 7, 2);
        let (before, after) = (
            lit(&[laser, (9, 5, Piece::Mirror, 0, 7)]),
            lit(&[laser, (9, 5, Piece::Mirror, 0, 1)]),
        );
        let mut c = Choreography::new();
        c.observe(
            &Observed {
                board: [&before.0, &after.0],
                field: [&before.1, &after.1],
                drag: [None, None],
                slot: [0, 0],
                won: [false, false],
                sheet: [false, false],
            },
            0.0,
        );
        (before, after, c)
    }

    #[test]
    fn the_sparkler_is_the_trailers_prng() {
        // trailer/draw.mjs: prng(1) → its first three values.
        let mut s = Sparkler::new(1);
        let first = [s.next(), s.next(), s.next()];
        let want = [0.627_073_9, 0.002_735_721_2, 0.527_447_04];
        for (a, b) in first.iter().zip(want) {
            assert!((a - b).abs() < 1e-6, "{first:?}");
        }
    }

    #[test]
    fn a_burst_stays_within_its_reach_and_is_out_by_900_ms() {
        let (centre, u) = (pt(500.0, 500.0), 40.0);
        let reach = BURST_REACH * u;
        for cell in 0..BOARD_CELLS {
            let b = Burst {
                cell,
                mask: 7,
                at: 0.0,
            };
            for ms in 0..=900 {
                let f = flare(centre, u, &b, ms as f32 / 1000.0);
                if let Some((radius, width, _)) = f.ring {
                    assert!(
                        radius + width / 2.0 <= reach,
                        "cell {cell} at {ms} ms: the ring"
                    );
                }
                for s in &f.sparks[..f.spark_len] {
                    let far = (s.b - centre).len().max((s.a - centre).len()) + s.width / 2.0;
                    assert!(far <= reach, "cell {cell} at {ms} ms: a spark at {far}");
                }
                assert!(
                    ms < 900 || f.spark_len == 0,
                    "cell {cell}: sparks at 900 ms"
                );
                assert_eq!(
                    f.ring.is_some(),
                    ms < 600,
                    "cell {cell} at {ms} ms: the ring"
                );
            }
        }
    }

    #[test]
    fn a_fading_strand_is_even_along_its_halves() {
        // The south strand, 100 ms into its fade (light enough not to saturate), at 96 px cells:
        // across the line, rows at the halves' junctions (cell centres and edges) are as bright
        // as rows in the middle of a half. Strokes per half would add their round caps twice at
        // every junction.
        let (_, after, c) = turned();
        let moment = c.moment(100.0);
        assert!(moment.fade > 0.2 && moment.fade < 0.5, "{}", moment.fade);
        let tableau = Tableau::arrange(&Easel::new(2560, 1600, 2.0));
        let stage = Stage {
            tableau: &tableau,
            palette: &Palette::DARK,
            board: &after.0,
            field: &after.1,
            reach: c.reach(),
            bursts: c.bursts(),
            now: 100.0,
        };
        let (w, h) = (2560, 1600);
        let mut px = vec![0u32; w * h];
        let all = DeviceRect::new(0, 0, w as i32, h as i32);
        let mut cast = Cast::new();
        cast.frame(&stage, &moment);
        let mut layer = Layer {
            px: &mut px,
            width: w,
            height: h,
        };
        cast.draw(&mut layer, all, &Palette::DARK);
        let u = tableau.cell as f32;
        // The light across the line at row `y`.
        let across = |x: f32, y: f32| -> u32 {
            let (x, y) = (x as i32, y as i32);
            (x - 8..=x + 8)
                .map(|x| {
                    let p = px[y as usize * w + x as usize];
                    (p >> 16 & 0xff) + (p >> 8 & 0xff) + (p & 0xff)
                })
                .sum()
        };
        let (mut junctions, mut middles) = (Vec::new(), Vec::new());
        for row in 7..13 {
            let c = tableau.cell_centre(board_index(9, row));
            junctions.push(across(c.x, c.y));
            junctions.push(across(c.x, c.y + u / 2.0));
            middles.push(across(c.x, c.y + u / 4.0));
            middles.push(across(c.x, c.y - u / 4.0));
        }
        let (lo, hi) = (
            *middles.iter().min().unwrap(),
            *middles.iter().max().unwrap(),
        );
        assert!(lo > 100, "the strand shows: {middles:?}");
        assert!(
            junctions.iter().all(|&j| j <= hi + 3),
            "beads at the junctions: {junctions:?} against {middles:?}"
        );
    }

    #[test]
    fn heads_fading_strands_bursts_packets_and_pulses_take_every_colour_from_the_palette() {
        // In both themes: the light one paints its cues `Over`, a fading strand with its glow
        // stroke (`gouache::STRAND_REACH`), and every px of it lies in the boxes (ruling S4).
        for palette in [&Palette::DARK, &Palette::LIGHT] {
            every_colour_from_the_palette(palette);
        }
    }

    fn every_colour_from_the_palette(palette: &Palette) {
        // 40 ms in: the south strand fades while the north one sweeps in with its heads; a white
        // target's burst is 10 ms old (its ring and sparks), a magenta one's 700 ms (sparks
        // only), and two more are not yet started or over.
        let (_, after, c) = turned();
        let moment = c.moment(40.0);
        assert!(moment.sweep.is_some() && moment.fade > 0.0, "{moment:?}");
        let tableau = Tableau::arrange(&Easel::new(1280, 800, 1.0));
        let burst = |x, y, mask, at| Burst {
            cell: board_index(x, y),
            mask,
            at,
        };
        let bursts = [
            burst(4, 10, 7, 30.0),
            burst(12, 3, 5, -660.0),
            burst(13, 13, 1, 60.0),
            burst(1, 1, 2, -900.0),
        ];
        let stage = Stage {
            tableau: &tableau,
            palette,
            board: &after.0,
            field: &after.1,
            reach: c.reach(),
            bursts: &bursts,
            now: 40.0,
        };
        let mut cast = Cast::new();
        cast.frame(&stage, &moment);
        assert_eq!(cast.flare_len, 2, "two bursts draw");
        assert!(cast.flares[0].ring.is_some() && cast.flares[1].ring.is_none());
        assert!(cast.flares[1].spark_len > 0, "sparks still fly at 700 ms");
        assert!(
            cast.strand_len > 0 && cast.head_len > 0,
            "strands and heads"
        );
        let lit_px = drawn_with_the_sentinel(&cast, palette);
        assert!(lit_px > 100, "the flourish drew: {lit_px} px");

        // A win 55 ms in (packets 3.3 cells out): a white laser east through a white target 2
        // cells on (reached at 33 ms, so pulsing), and a red one; each beam's packet is in the
        // 4th cell along.
        let won = lit(&[
            (2, 5, Piece::Laser, 7, 2),
            (4, 5, Piece::Pinwheel, 7, 0),
            (2, 9, Piece::Laser, 1, 2),
        ]);
        let reach = Reach::compute(&won.0, &won.1);
        let idle = Choreography::new();
        let moment = Moment {
            win: Some(WinMoment {
                packets: true,
                packet: 3.3,
                since_ms: 55.0,
                reveal: 0.5,
                header_alpha: 1.0,
            }),
            ..idle.moment(0.0)
        };
        let stage = Stage {
            tableau: &tableau,
            palette,
            board: &won.0,
            field: &won.1,
            reach: &reach,
            bursts: &[],
            now: 55.0,
        };
        cast.frame(&stage, &moment);
        assert_eq!(
            (cast.packet_len, cast.pulse_len),
            (2, 1),
            "two packets, a pulse"
        );
        assert!(cast.pulses[0].alpha > 0.0);
        // The pulse glows over the target's face out past its ring (0.3u), not only over the
        // core, which a lit target already shows near white.
        let (pulse, u) = (cast.pulses[0].bounds, tableau.cell as f32);
        assert!(
            (pulse.right - pulse.left) as f32 >= 2.0 * 0.5 * u,
            "{pulse:?} at u = {u}"
        );
        for (p, y) in cast.packets[..2].iter().zip([5, 9]) {
            let want = tableau.cell_centre(board_index(5, y)) + pt(0.3 * tableau.cell as f32, 0.0);
            assert!((p.at - want).len() < 1e-3, "{:?} against {want:?}", p.at);
        }
        let px = sentinel_picture(&cast, palette);
        let lit_px = px.iter().filter(|&&p| p != 0).count();
        assert!(lit_px > 100, "the packet and the pulse drew: {lit_px} px");
        // West of the target (the packet is east of it), drawn px past 0.3u from its centre.
        let c = tableau.cell_centre(board_index(4, 5));
        let past_ring = px.iter().enumerate().filter(|&(i, &p)| {
            let (x, y) = ((i % 1280) as f32 + 0.5, (i / 1280) as f32 + 0.5);
            p != 0 && x < c.x && (pt(x, y) - c).len() > 0.3 * u + 1.0
        });
        assert!(past_ring.count() > 0, "the pulse reaches past 0.3u");
    }

    /// Draws `cast` on a 1280 × 800 canvas in `palette`'s sentinel, whole and rect by rect:
    /// the same picture, only the sentinel's colour, every px inside the boxes reported. Returns
    /// how many px it lit.
    fn drawn_with_the_sentinel(cast: &Cast, palette: &Palette) -> usize {
        sentinel_picture(cast, palette)
            .iter()
            .filter(|&&p| p != 0)
            .count()
    }

    /// `drawn_with_the_sentinel`'s picture, 1280 px across.
    fn sentinel_picture(cast: &Cast, palette: &Palette) -> Vec<u32> {
        let (w, h) = (1280, 800);
        let mut px = vec![0u32; w * h];
        let all = DeviceRect::new(0, 0, w as i32, h as i32);
        let sentinel = palette.recoloured(Rgb(255, 0, 255));
        let mut layer = Layer {
            px: &mut px,
            width: w,
            height: h,
        };
        cast.draw(&mut layer, all, &sentinel);
        // Drawn rect by rect (as the lightbox draws into its damage), the picture is the same.
        let mut tiled = vec![0u32; w * h];
        let mut layer = Layer {
            px: &mut tiled,
            width: w,
            height: h,
        };
        for y in (0..h as i32).step_by(97) {
            for x in (0..w as i32).step_by(131) {
                cast.draw(
                    &mut layer,
                    DeviceRect::new(x, y, x + 131, y + 97),
                    &sentinel,
                );
            }
        }
        assert!(tiled == px, "drawn by rects, the same picture");
        for &p in &px {
            let (r, g, b) = (p >> 16 & 0xff, p >> 8 & 0xff, p & 0xff);
            assert!(g == 0 && r.abs_diff(b) <= 1, "{p:06x}");
        }
        // Every px drawn is inside the boxes reported.
        let boxes_out = cast.boxes();
        for (i, &p) in px.iter().enumerate() {
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            let inside = boxes_out
                .as_slice()
                .iter()
                .any(|r| (r.left..r.right).contains(&x) && (r.top..r.bottom).contains(&y));
            assert!(p == 0 || inside, "({x}, {y}) drawn outside the boxes");
        }
        px
    }

    #[test]
    fn a_wins_packets_and_pulses_fade_out_by_its_end() {
        // A target 70 cells out (as C1 level 10 has) is reached at 1167 ms, its pulse at its
        // height when the win's 1.4 s are up: the last 200 ms fade it and the packets out.
        let (board, field) = lit(&[(2, 5, Piece::Laser, 7, 2), (4, 5, Piece::Pinwheel, 7, 0)]);
        let mut reach = Reach::compute(&board, &field);
        reach.cells[board_index(4, 5)] = 70;
        let tableau = Tableau::arrange(&Easel::new(1280, 800, 1.0));
        let idle = Choreography::new();
        let stage = Stage {
            tableau: &tableau,
            palette: &Palette::DARK,
            board: &board,
            field: &field,
            reach: &reach,
            bursts: &[],
            now: 0.0,
        };
        let mut cast = Cast::new();
        let mut at = |since_ms: f32| {
            let moment = Moment {
                win: Some(WinMoment {
                    packets: true,
                    packet: 0.4,
                    since_ms,
                    reveal: 1.0,
                    header_alpha: 1.0,
                }),
                ..idle.moment(0.0)
            };
            cast.frame(&stage, &moment);
            (
                cast.packet_alpha,
                cast.pulses[..cast.pulse_len].first().map(|p| p.alpha),
            )
        };
        let (packet, pulse) = at(1199.0);
        assert_eq!(packet, PACKET_ALPHA);
        // 32 ms in: 0.3 · sin(0.08π) ≈ 0.075, before the tail.
        assert!(pulse.is_some_and(|a| a > 0.07), "{pulse:?}");
        let (packet, pulse) = at(1399.0);
        assert!(
            packet < 0.01 && pulse.is_some_and(|a| a < 0.002),
            "{packet} {pulse:?}"
        );
    }

    #[test]
    fn packets_run_from_the_lasers_and_targets_pulse_once_reached() {
        // The white laser at (2, 5) east through the white target at (4, 5) (reach 2) to the
        // board's edge (reach 12): the packet leaves the aperture, crosses into the next cell
        // at 0.5, passes the target's centre at 2 and leaves the board at 12.5; the target
        // pulses from 33.3 ms for 400 ms.
        let (board, field) = lit(&[(2, 5, Piece::Laser, 7, 2), (4, 5, Piece::Pinwheel, 7, 0)]);
        let reach = Reach::compute(&board, &field);
        let (laser, next, target) = (board_index(2, 5), board_index(3, 5), board_index(4, 5));
        let (east, west) = (2, 6);
        let at = |cell, side, packet| packet_at(&field, &reach, &board, cell, side, packet);
        assert_eq!(at(laser, east, 0.1), None, "inside the aperture");
        assert_eq!(at(laser, east, 0.4), Some(0.8));
        assert_eq!(at(next, west, 0.75), Some(0.5), "the next cell's near half");
        assert_eq!(at(next, east, 1.25), Some(0.5), "its far half");
        assert!(at(target, west, 1.99).is_some_and(|f| f < 0.05));
        let anywhere = |packet| {
            (0..BOARD_CELLS)
                .flat_map(|c| (0..8).map(move |s| (c, s)))
                .filter(|&(c, s)| at(c, s, packet).is_some())
                .collect::<Vec<_>>()
        };
        assert_eq!(anywhere(2.0), [(target, east)], "on through the target");
        assert_eq!(anywhere(12.25), [(board_index(14, 5), east)]);
        assert_eq!(
            (anywhere(12.5), anywhere(-0.5)),
            (vec![], vec![]),
            "none off the board, none before"
        );
        let k = |ms| pulse(&board, &field, &reach, target, ms);
        assert_eq!(k(30.0), None, "not reached yet");
        assert!(k(40.0).is_some_and(|k| k > 0.0 && k < 0.05));
        assert_eq!(k(434.0), None, "over 400 ms on");
        assert_eq!(
            pulse(&board, &field, &reach, laser, 100.0),
            None,
            "only targets"
        );
    }

    #[test]
    fn a_packet_on_a_half_lit_both_ways_rides_its_first_strand() {
        // White east from (2, 5) into a mirror at (8, 5) that sends it straight back: (5, 5)'s
        // west half takes the light coming in at 2.5 and going back out at 9. Its packet enters
        // from the edge with the first, as the packet beside it leaves (4, 5).
        let (board, field) = (0..8)
            .map(|dir| lit(&[(2, 5, Piece::Laser, 7, 2), (8, 5, Piece::Mirror, 0, dir)]))
            .find(|(_, f)| f.outbound[board_index(5, 5)][6] == 7)
            .expect("a mirror that sends the beam back");
        let reach = Reach::compute(&board, &field);
        let at = |cell, side, packet| packet_at(&field, &reach, &board, cell, side, packet);
        assert_eq!(at(board_index(4, 5), 2, 2.25), Some(0.5), "leaving (4, 5)");
        assert_eq!(
            at(board_index(5, 5), 6, 2.75),
            Some(0.5),
            "into (5, 5) from its edge"
        );
    }

    #[test]
    fn a_light_fading_strand_lies_in_its_boxes_at_every_size() {
        // Ruling S4: in `Over` a fading strand's boxes grow by the glow stroke's half width
        // (`gouache::STRAND_REACH`), so the damage holds every px it paints, on cells large
        // enough that the boxes' 2-px slack would not hide a short reach.
        let (_, after, c) = turned();
        let moment = c.moment(40.0);
        let mut big = 0.0f32;
        for easel in [
            Easel::new(1280, 800, 1.0),
            Easel::new(2560, 1600, 2.0),
            Easel::new(3456, 2234, 2.0),
        ] {
            let tableau = Tableau::arrange(&easel);
            big = big.max(tableau.cell as f32);
            let stage = Stage {
                tableau: &tableau,
                palette: &Palette::LIGHT,
                board: &after.0,
                field: &after.1,
                reach: c.reach(),
                bursts: &[],
                now: 40.0,
            };
            let mut cast = Cast::new();
            cast.frame(&stage, &moment);
            // The strands alone.
            cast.head_len = 0;
            assert!(cast.strand_len > 0, "a fading strand");
            let (w, h) = (easel.width as usize, easel.height as usize);
            let mut px = vec![0u32; w * h];
            let mut layer = Layer {
                px: &mut px,
                width: w,
                height: h,
            };
            let sentinel = Palette::LIGHT.recoloured(Rgb(255, 0, 255));
            cast.draw(
                &mut layer,
                DeviceRect::new(0, 0, w as i32, h as i32),
                &sentinel,
            );
            let boxes = cast.boxes().as_slice();
            let drawn = px.iter().enumerate().filter(|&(_, &p)| p != 0);
            for (i, _) in drawn {
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                assert!(
                    boxes
                        .iter()
                        .any(|r| (r.left..r.right).contains(&x) && (r.top..r.bottom).contains(&y)),
                    "{easel:?}: ({x}, {y}) drawn outside the boxes"
                );
            }
        }
        // At the largest cell, 0.05u less 0.032u is past the boxes' 2 px slack (a mutant with
        // the dark reach fails there).
        assert!(
            big * (gouache::STRAND_REACH - STRAND_REACH) > 2.0,
            "u = {big}"
        );
    }

    /// The channels of a pixel of a 1280-px-wide picture at `p`.
    fn rgb_at(px: &[u32], p: Pt) -> [f32; 3] {
        let v = px[p.y as usize * 1280 + p.x as usize];
        [v >> 16 & 0xff, v >> 8 & 0xff, v & 0xff].map(|c| c as f32)
    }

    /// `cast` drawn over a white 1280 × 800 canvas in the light palette.
    fn on_white(cast: &Cast) -> Vec<u32> {
        let mut px = vec![0x00ff_ffffu32; 1280 * 800];
        let mut layer = Layer {
            px: &mut px,
            width: 1280,
            height: 800,
        };
        cast.draw(
            &mut layer,
            DeviceRect::new(0, 0, 1280, 800),
            &Palette::LIGHT,
        );
        px
    }

    /// Whether `c` is white with `halo` painted over it at some α ≥ `least` (±3 per channel).
    fn white_under(c: [f32; 3], halo: Rgb, least: f32) -> bool {
        let h = [halo.0, halo.1, halo.2].map(f32::from);
        let a = (255.0 - c[0]) / (255.0 - h[0]);
        a >= least && (0..3).all(|i| (c[i] - (255.0 - a * (255.0 - h[i]))).abs() <= 3.0)
    }

    #[test]
    fn a_white_front_has_one_blue_grey_head_in_light() {
        // Ruling L5: in `Over` a white front's red, green and blue heads (one point) are one
        // head in `halo(WHITE)`, blue-grey; painted one over another they would read blue. The
        // dark theme keeps a head per channel, added.
        let (_, after, c) = turned();
        let moment = c.moment(40.0);
        let tableau = Tableau::arrange(&Easel::new(1280, 800, 1.0));
        let cast = |palette| {
            let stage = Stage {
                tableau: &tableau,
                palette,
                board: &after.0,
                field: &after.1,
                reach: c.reach(),
                bursts: &[],
                now: 40.0,
            };
            let mut cast = Cast::new();
            cast.frame(&stage, &moment);
            cast
        };
        let (dark, light) = (cast(&Palette::DARK), cast(&Palette::LIGHT));
        assert!(light.head_len > 0, "a sweep's heads");
        assert_eq!(dark.head_len, 3 * light.head_len, "a dark head per channel");
        let heads = &light.heads[..light.head_len];
        assert!(heads.iter().all(|h| h.bit == WHITE), "{heads:?}");
        let px = on_white(&light);
        let halo = Palette::LIGHT.halo(WHITE);
        for h in heads {
            let got = rgb_at(&px, h.at);
            assert!(white_under(got, halo, 0.6), "{got:?} at {:?}", h.at);
        }
    }

    #[test]
    fn a_white_targets_pulse_and_a_packet_are_blue_grey_in_light() {
        // Ruling L5 (review finding 6): a white target's win pulse at its height (0.3) is
        // `halo(WHITE)` over its face; `mix(WHITE)` is white, which on a white board is nothing.
        // A packet is white at its centre, `halo(WHITE)` at 0.5α at 0.4 of its radius.
        let won = lit(&[(2, 5, Piece::Laser, 7, 2), (4, 5, Piece::Pinwheel, 7, 0)]);
        let reach = Reach::compute(&won.0, &won.1);
        let tableau = Tableau::arrange(&Easel::new(1280, 800, 1.0));
        let idle = Choreography::new();
        let moment = Moment {
            win: Some(WinMoment {
                packets: true,
                packet: 3.3,
                since_ms: 33.3 + 200.0,
                reveal: 1.0,
                header_alpha: 1.0,
            }),
            ..idle.moment(0.0)
        };
        let stage = Stage {
            tableau: &tableau,
            palette: &Palette::LIGHT,
            board: &won.0,
            field: &won.1,
            reach: &reach,
            bursts: &[],
            now: 0.0,
        };
        let mut cast = Cast::new();
        cast.frame(&stage, &moment);
        assert_eq!(cast.pulse_len, 1, "the target pulses");
        assert_eq!(cast.packet_len, 1, "a packet runs");
        let px = on_white(&cast);
        let halo = Palette::LIGHT.halo(WHITE);
        let got = rgb_at(&px, tableau.cell_centre(board_index(4, 5)));
        assert!(white_under(got, halo, 0.25), "the pulse: {got:?}");
        let p = cast.packets[0].at;
        let got = rgb_at(&px, p + pt(0.0, 0.4 * PACKET_RADIUS * tableau.cell as f32));
        assert!(white_under(got, halo, 0.3), "the packet: {got:?}");
        let centre = rgb_at(&px, p);
        assert!(
            centre.iter().all(|&c| c >= 240.0),
            "its white centre: {centre:?}"
        );
    }

    #[test]
    fn a_packet_on_a_coloured_beam_has_that_beams_halo_in_light() {
        // Still review finding 5: on a red beam a packet's rim is `halo(RED)` at 0.6α at 0.4 of
        // its radius (blue-grey there read as a grey smudge on the line, not passing light).
        let won = lit(&[
            (2, 5, Piece::Laser, RED, 2),
            (6, 5, Piece::Pinwheel, RED, 0),
        ]);
        let reach = Reach::compute(&won.0, &won.1);
        let tableau = Tableau::arrange(&Easel::new(1280, 800, 1.0));
        let idle = Choreography::new();
        let moment = Moment {
            win: Some(WinMoment {
                packets: true,
                packet: 3.3,
                since_ms: 33.3 + 200.0,
                reveal: 1.0,
                header_alpha: 1.0,
            }),
            ..idle.moment(0.0)
        };
        let stage = Stage {
            tableau: &tableau,
            palette: &Palette::LIGHT,
            board: &won.0,
            field: &won.1,
            reach: &reach,
            bursts: &[],
            now: 0.0,
        };
        let mut cast = Cast::new();
        cast.frame(&stage, &moment);
        assert!(cast.packet_len >= 1, "a packet runs");
        let px = on_white(&cast);
        let p = cast.packets[0].at;
        // Across the beam (it runs east), at 0.4 of the packet's radius.
        let got = rgb_at(&px, p + pt(0.0, 0.4 * PACKET_RADIUS * tableau.cell as f32));
        let red = Palette::LIGHT.halo(RED);
        let a = (255.0 - got[1]) / (255.0 - f32::from(red.1));
        let want = [red.0, red.1, red.2].map(|c| 255.0 - a * (255.0 - f32::from(c)));
        assert!(
            a >= 0.4 && (0..3).all(|i| (got[i] - want[i]).abs() <= 4.0),
            "the packet's rim: {got:?}, white under halo(RED) at {a}"
        );
    }
}
