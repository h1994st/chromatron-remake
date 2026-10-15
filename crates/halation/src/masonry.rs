//! `masonry`: the board's furniture (spec §7.2): the grid, the glass tiles under pieces and
//! the raised wall slabs. Every colour comes from the palette.

use lumen::{BOARD_CELLS, Board, Piece, SIDE, board_index};

use crate::bloom::{blur_plane, blur_scratch};
use crate::etch::{Blend, Layer, Paint, Pt, Shape, Stops, Target, clip_rect, fill, pt};
use crate::spectrum::{Palette, Rgba};
use crate::{DeviceRect, fit_buffer, make_room, vitrine};

/// Grid lines 0.35u past the board, `max(1 px, 0.014u)` wide, and crossing dots of radius
/// `max(1 px, 0.022u)`; both fade radially over 0.75 × the board diagonal.
pub fn draw_grid(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    board: DeviceRect,
    u: f32,
) {
    let (l, tp, r, b) = (
        board.left as f32,
        board.top as f32,
        board.right as f32,
        board.bottom as f32,
    );
    let centre = pt((l + r) / 2.0, (tp + b) / 2.0);
    let reach = 0.75 * (r - l).hypot(b - tp);
    let [a0, a1, a2] = palette.grid_line_alpha;
    let lines = Paint::Radial {
        centre,
        radius: reach,
        stops: Stops::new(&[
            (0.0, palette.grid_line.alpha(a0)),
            (0.65, palette.grid_line.alpha(a1)),
            (1.0, palette.grid_line.alpha(a2)),
        ]),
    };
    let half = (0.014 * u).max(1.0) / 2.0;
    let over = 0.35 * u;
    for k in 0..=15 {
        let x = l + k as f32 * u;
        fill(
            t,
            clip,
            &[Shape::rect(x - half, tp - over, x + half, b + over)],
            &lines,
            1.0,
            Blend::Over,
        );
        let y = tp + k as f32 * u;
        fill(
            t,
            clip,
            &[Shape::rect(l - over, y - half, r + over, y + half)],
            &lines,
            1.0,
            Blend::Over,
        );
    }
    let [d0, d1] = palette.grid_dot_alpha;
    let dots = Paint::Radial {
        centre,
        radius: reach,
        stops: Stops::new(&[
            (0.0, palette.grid_dot.alpha(d0)),
            (1.0, palette.grid_dot.alpha(d1)),
        ]),
    };
    let radius = (0.022 * u).max(1.0);
    for j in 0..=15 {
        for i in 0..=15 {
            fill(
                t,
                clip,
                &[Shape::disc(pt(l + i as f32 * u, tp + j as f32 * u), radius)],
                &dots,
                1.0,
                Blend::Over,
            );
        }
    }
}

/// The light tile's shadow (spec §7.2: blur 0.08u, so σ 0.04u; 0.03u down), in units of `u`.
pub(crate) const TILE_SHADOW_SIGMA: f32 = 0.04;
pub(crate) const TILE_SHADOW_DROP: f32 = 0.03;

/// The glass tile under a piece (`raised`: movable, h = 1): inset 0.07u, radius 0.16u, a
/// diagonal glass fill and a vertical three-stop rim `max(1 px, 0.014u)` wide. A light palette
/// casts a soft shadow under it first (`tile_shadow`).
pub fn draw_tile(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    centre: Pt,
    u: f32,
    raised: bool,
) {
    let h = if raised { 1.0 } else { 0.0 };
    let half = u / 2.0 - 0.07 * u;
    let (l, tp, r, b) = (
        centre.x - half,
        centre.y - half,
        centre.x + half,
        centre.y + half,
    );
    // The light theme's shadow, cast under the tile, inside its sprite, which is its cell: the
    // tail past the cell is cut (≤ 2 code values at the sides, about 5 at the bottom; ruling L8).
    if let Some(shade) = palette.tile_shadow {
        let drop = TILE_SHADOW_DROP * u;
        let cast = Shape::round_rect(l, tp + drop, r, b + drop, 0.16 * u);
        vitrine::soft_shape(t, clip, &cast, TILE_SHADOW_SIGMA * u, shade.at(h), None);
    }
    let shape = Shape::round_rect(l, tp, r, b, 0.16 * u);
    let glass = vitrine::glaze(&palette.tile, (l, tp, r, b), h);
    fill(t, clip, &[shape], &glass, 1.0, Blend::Over);
    let rim = Paint::Linear {
        from: pt(0.0, tp),
        to: pt(0.0, b),
        stops: Stops::new(&[
            (0.0, palette.tile_rim[0].at(h)),
            (0.5, palette.tile_rim[1].at(h)),
            (1.0, palette.tile_rim[2].at(h)),
        ]),
    };
    fill(
        t,
        clip,
        &[shape.outline((0.014 * u).max(1.0))],
        &rim,
        1.0,
        Blend::Over,
    );
}

/// Board rows per wall strip.
const STRIP_ROWS: usize = 2;
/// Strips that cover the board's rows.
const STRIPS: usize = SIDE.div_ceil(STRIP_ROWS);
/// A fully covered ½-resolution shadow pixel: four mask pixels of 255, ×16 for the blur's
/// precision.
const SHADOW_FULL: f32 = (255 * 4 * 16) as f32;

/// The walls' working memory (spec §14): one strip of masks over the board region, two board
/// rows tall plus a halo above and below, and the slab of every wall cell. Allocated in
/// `reshape` only.
pub struct WallScratch {
    region: DeviceRect,
    /// Rows a strip reaches past its core: `ceil(0.5u)`, past the shadow's offset + 3σ and the
    /// bevels' shifts.
    halo: i32,
    /// Rows the strip buffers hold.
    rows: usize,
    /// Slab coverage, 255 = full.
    mask: Vec<u8>,
    /// The mask at ½ resolution, blurred into the drop shadow.
    shadow: Vec<u16>,
    blur: Vec<u16>,
    /// Per-row working space.
    line: LineScratch,
    /// Each board cell's slab, 1-based (0: not a wall).
    labels: [u8; BOARD_CELLS],
    /// Each slab's top and bottom board rows, by label.
    spans: [[u8; 2]; BOARD_CELLS + 1],
}

impl Default for WallScratch {
    fn default() -> Self {
        Self::new()
    }
}

impl WallScratch {
    pub fn new() -> WallScratch {
        WallScratch {
            region: DeviceRect::default(),
            halo: 0,
            rows: 0,
            mask: Vec::new(),
            shadow: Vec::new(),
            blur: Vec::new(),
            line: LineScratch::default(),
            labels: [0; BOARD_CELLS],
            spans: [[0; 2]; BOARD_CELLS + 1],
        }
    }

    /// One strip over `region` (the darkroom's board region) for cells of `u` px.
    pub fn reshape(&mut self, region: DeviceRect, u: f32) {
        self.region = region;
        self.halo = (0.5 * u).ceil().max(0.0) as i32;
        let width = (region.right - region.left).max(0) as usize;
        // Two rows, both halos, and a row each for rounding and the ½ grid's alignment.
        self.rows = if width == 0 {
            0
        } else {
            (STRIP_ROWS as f32 * u).ceil() as usize + 2 * self.halo as usize + 2
        };
        fit_buffer(&mut self.mask, width * self.rows, 0);
        let (hw, hh) = (width.div_ceil(2), self.rows.div_ceil(2));
        fit_buffer(&mut self.shadow, hw * hh, 0);
        make_room(&mut self.blur, blur_scratch(hw, hh));
        fit_buffer(&mut self.line.shadow, hw + 2, 0.0);
        fit_buffer(&mut self.line.near, width, 0);
        fit_buffer(&mut self.line.deep, width, 0);
        fit_buffer(&mut self.line.columns, 2 * width, 0);
    }

    pub fn bytes(&self) -> usize {
        self.mask.capacity()
            + self.shadow.capacity() * size_of::<u16>()
            + self.blur.capacity() * size_of::<u16>()
            + self.line.shadow.capacity() * size_of::<f32>()
            + self.line.near.capacity()
            + self.line.deep.capacity()
            + self.line.columns.capacity() * size_of::<u16>()
    }

    /// Labels the board's 4-connected wall groups and finds each one's rows; false when the
    /// board has no wall.
    fn label(&mut self, board: &Board) -> bool {
        let wall = |i: usize| board.cells[i].piece == Piece::Wall;
        self.labels = [0; BOARD_CELLS];
        let mut stack = [0u8; BOARD_CELLS];
        let mut next = 0u8;
        for seed in 0..BOARD_CELLS {
            if !wall(seed) || self.labels[seed] != 0 {
                continue;
            }
            next += 1;
            self.labels[seed] = next;
            let mut span = [(seed / SIDE) as u8; 2];
            let (mut len, mut k) = (1, 0);
            stack[0] = seed as u8;
            while k < len {
                let i = usize::from(stack[k]);
                k += 1;
                let (x, y) = (i % SIDE, i / SIDE);
                span = [span[0].min(y as u8), span[1].max(y as u8)];
                let neighbours = [
                    (x > 0).then(|| i - 1),
                    (x + 1 < SIDE).then(|| i + 1),
                    (y > 0).then(|| i - SIDE),
                    (y + 1 < SIDE).then(|| i + SIDE),
                ];
                for j in neighbours.into_iter().flatten() {
                    if wall(j) && self.labels[j] == 0 {
                        self.labels[j] = next;
                        stack[len] = j as u8;
                        len += 1;
                    }
                }
            }
            self.spans[usize::from(next)] = span;
        }
        next > 0
    }
}

/// One device row's working space while a strip is painted.
#[derive(Default)]
struct LineScratch {
    /// The shadow's alpha per ½ column, padded by a zero at each end.
    shadow: Vec<f32>,
    /// The least mask over the rows the vertical bands look at.
    near: Vec<u8>,
    /// `near`'s least across the horizontal bands' reach: 255 where no band touches a pixel.
    deep: Vec<u8>,
    /// `cover`'s coverage per column of a slab's edge, per half (two widths).
    columns: Vec<u16>,
}

/// A coverage mask that `fill` draws into: a union keeps each pixel's largest coverage, so
/// overlapping parts join without a seam.
struct SlabMask<'a> {
    px: &'a mut [u8],
    width: usize,
    height: usize,
}

impl Target for SlabMask<'_> {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    fn blend(&mut self, x: usize, y: usize, c: Rgba, _: Blend) {
        let p = &mut self.px[y * self.width + x];
        *p = (*p).max((c.a * 255.0).round().clamp(0.0, 255.0) as u8);
    }
}

/// What `fill` with an opaque ink leaves in a `SlabMask` pixel at signed distance `d`.
#[inline(always)]
fn coverage_byte(d: f32) -> u8 {
    let coverage = (0.5 - d).clamp(0.0, 1.0);
    (coverage * 255.0).round().clamp(0.0, 255.0) as u8
}

/// `cover`'s mark for a column's coverage not yet measured (a coverage is a byte).
const UNMEASURED: u16 = 0x100;

/// Fills `shape` into `mask` as `fill` would. A slab's rect is drawn directly: 255 where it
/// covers fully, away from its rounded corners; elsewhere each pixel's coverage, where along a
/// straight top or bottom edge one distance serves the whole run, and along a straight left or
/// right edge one distance serves the whole column (per half: `columns` holds them, at least
/// twice the mask's width).
fn cover(mask: &mut SlabMask, shape: Shape, columns: &mut [u16]) {
    debug_assert!(
        columns.len() >= 2 * mask.width,
        "cover's columns: at least twice the mask's width"
    );
    let (w, h) = (mask.width as i32, mask.height as i32);
    let full = DeviceRect::new(0, 0, w, h);
    let Shape::RoundRect {
        left,
        top,
        right,
        bottom,
        radii,
        stroke: 0.0..=0.0,
    } = shape
    else {
        let ink = Paint::Solid(Rgba::CLEAR.with_alpha(1.0));
        fill(mask, full, &[shape], &ink, 1.0, Blend::Over);
        return;
    };
    // Pixel centres at least ½ px inside every edge, in rows clear of the corners' radius.
    let r = radii.into_iter().fold(0.0f32, f32::max);
    let inner = clip_rect(
        DeviceRect::new(
            left.ceil() as i32,
            (top + r).ceil() as i32,
            right.floor() as i32,
            (bottom - r).floor() as i32,
        ),
        full,
    )
    .unwrap_or_default();
    for y in inner.top..inner.bottom {
        let at = y as usize * mask.width;
        mask.px[at + inner.left as usize..at + inner.right as usize].fill(255);
    }
    // Every other pixel `fill` would visit (any wider set adds only zeros).
    let (bl, bt, br, bb) = shape.bounds();
    let (x0, x1) = (
        (bl.floor() as i32 - 1).max(0),
        (br.ceil() as i32 + 1).min(w),
    );
    let (y0, y1) = ((bt.floor() as i32).max(0), (bb.ceil() as i32 + 1).min(h));
    // `Shape::distance`'s terms: a pixel whose |dx| term `qx` is ≤ 0 on a row whose `qy` is
    // > 0 is at distance `|(0, qy)| − r` whatever its x.
    let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
    let (hx, hy) = ((right - left) / 2.0, (bottom - top) / 2.0);
    // On a row whose `qy` is ≤ 0, `distance` is `|(max(qx, 0), 0)| + min(qx, 0) − r` where
    // `qx ≥ qy` (its row's terms drop out: `max(qy, 0)` squares to 0 and `max(qx, qy)` is
    // `qx`), which holds for its column in that half (top or bottom: the radii), and
    // `0 + min(qy, 0) − r` where `qx < qy` (`max(qx, 0)` is 0), which holds for the row.
    let span = (x1 - x0).max(0) as usize;
    let columns = &mut columns[..2 * span];
    columns.fill(UNMEASURED);
    for y in y0..y1 {
        let py = y as f32 + 0.5;
        let dy = py - cy;
        let (r_left, r_right) = if dy < 0.0 {
            (radii[0], radii[1])
        } else {
            (radii[3], radii[2])
        };
        let run = |r: f32| {
            let qy = dy.abs() - hy + r;
            // Its `max(qx, qy).min(0)` term is 0, and `len + 0 = len` for `len ≥ 0`.
            (qy > 0.0).then(|| coverage_byte(pt(0.0, qy).len() - r))
        };
        let (run_left, run_right) = (run(r_left), run(r_right));
        // The px the filled rect leaves on this row: either side of it (`inner` lies in x0..x1),
        // or the whole row past its rows. The full px are stepped over, not tested.
        let (before, after) = if (inner.top..inner.bottom).contains(&y) {
            (x1.min(inner.left), x0.max(inner.right))
        } else {
            (x1, x1)
        };
        let half = &mut columns[usize::from(dy >= 0.0) * span..][..span];
        let row = &mut mask.px[y as usize * mask.width..][..mask.width];
        for x in (x0..before).chain(after..x1) {
            let px = x as f32 + 0.5;
            let dx = px - cx;
            let (r, run) = if dx < 0.0 {
                (r_left, run_left)
            } else {
                (r_right, run_right)
            };
            let qx = dx.abs() - hx + r;
            let qy = dy.abs() - hy + r;
            let v = match run {
                Some(v) if qx <= 0.0 => v,
                None if qy <= 0.0 && qx >= qy => {
                    let v = &mut half[(x - x0) as usize];
                    if *v == UNMEASURED {
                        *v = coverage_byte(pt(qx.max(0.0), 0.0).len() + qx.min(0.0) - r).into();
                    }
                    *v as u8
                }
                None if qy <= 0.0 => coverage_byte(0.0 + qy.min(0.0) - r),
                _ => coverage_byte(shape.signed(pt(px, py))),
            };
            let p = &mut row[x as usize];
            *p = (*p).max(v);
        }
    }
}

/// `d` with `c` laid over it at `a`, both in code values.
#[inline(always)]
fn over(d: &mut [f32; 3], c: [f32; 3], a: f32) {
    for (d, c) in d.iter_mut().zip(c) {
        *d += (c - *d) * a;
    }
}

/// `c`'s colour in code values.
fn code(c: Rgba) -> [f32; 3] {
    [c.r * 255.0, c.g * 255.0, c.b * 255.0]
}

fn unpack(p: u32) -> [f32; 3] {
    [
        (p >> 16 & 0xff) as f32,
        (p >> 8 & 0xff) as f32,
        (p & 0xff) as f32,
    ]
}

fn pack(d: [f32; 3]) -> u32 {
    let ch = |v: f32| v.round().clamp(0.0, 255.0) as u32;
    ch(d[0]) << 16 | ch(d[1]) << 8 | ch(d[2])
}

/// A wall paint's colours (code values) and sizes.
struct SlabStyle {
    shadow: ([f32; 3], f32),
    top: [f32; 3],
    bottom: [f32; 3],
    hatch: ([f32; 3], f32),
    /// Bevel and rim bands: the shift and the colour with its alpha.
    bands: [(i32, i32, [f32; 3], f32); 8],
    /// Hatch spacing along x and half its width, px.
    period: f32,
    half_width: f32,
    /// The shadow's offset down, px.
    drop: f32,
}

impl SlabStyle {
    fn of(palette: &Palette, u: f32) -> SlabStyle {
        let tones = &palette.wall;
        let b = (0.045 * u).round().max(2.0) as i32;
        let side = (0.7 * b as f32).round() as i32;
        let r1 = (0.014 * u).round().max(1.0) as i32;
        let (hi, lo, rim) = (tones.highlight, tones.shade, tones.rim);
        let band = |dx: i32, dy: i32, c: Rgba, k: f32| (dx, dy, code(c), c.a * k);
        SlabStyle {
            shadow: (code(tones.shadow), tones.shadow.a),
            top: code(tones.top.opaque()),
            bottom: code(tones.bottom.opaque()),
            hatch: (code(tones.hatch), tones.hatch.a),
            bands: [
                band(0, b, hi, 1.0),
                band(side, 0, hi, 0.45),
                band(0, -b, lo, 1.0),
                band(-side, 0, lo, 0.6),
                band(0, r1, rim, 1.0),
                band(0, -r1, rim, 1.0),
                band(r1, 0, rim, 1.0),
                band(-r1, 0, rim, 1.0),
            ],
            period: 0.14 * u,
            half_width: (0.012 * u).max(1.0) / 2.0,
            drop: 0.08 * u,
        }
    }
}

/// One window's mask and shadow, and the board's slabs, to paint its core rows from.
struct Window<'a> {
    mask: &'a [u8],
    width: usize,
    height: usize,
    shadow: &'a [u16],
    half_width: usize,
    half_height: usize,
    /// The window's top-left, device px.
    left: i32,
    top: i32,
    labels: &'a [u8; BOARD_CELLS],
    spans: &'a [[u8; 2]; BOARD_CELLS + 1],
    origin: Pt,
    u: f32,
}

impl Window<'_> {
    /// The mask at window px (`x`, `y`), 0 outside.
    #[inline(always)]
    fn cover(&self, x: i32, y: i32) -> u8 {
        if (0..self.width as i32).contains(&x) && (0..self.height as i32).contains(&y) {
            self.mask[y as usize * self.width + x as usize]
        } else {
            0
        }
    }

    fn half_row(&self, j: i32) -> Option<&[u16]> {
        (0..self.half_height as i32)
            .contains(&j)
            .then(|| &self.shadow[j as usize * self.half_width..][..self.half_width])
    }

    /// The fill of the slab holding board cell (`col`, `row`) on the row whose centres are at
    /// `py`: its gradient from the slab's top row + 0.1u to its bottom row − 0.1u.
    fn tone(&self, col: i32, row: i32, py: f32, style: &SlabStyle) -> [f32; 3] {
        let on_board = (0..SIDE as i32).contains(&col) && (0..SIDE as i32).contains(&row);
        let label = if on_board {
            self.labels[board_index(col as usize, row as usize)]
        } else {
            0
        };
        let [first, last] = self.spans[usize::from(label)];
        let y0 = self.origin.y + (f32::from(first) + 0.1) * self.u;
        let y1 = self.origin.y + (f32::from(last) + 0.9) * self.u;
        let t = ((py - y0) / (y1 - y0)).clamp(0.0, 1.0);
        let (a, b) = (style.top, style.bottom);
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    }

    /// The mask's row `y`, `None` outside the window.
    fn mask_row(&self, y: i32) -> Option<&[u8]> {
        (0..self.height as i32)
            .contains(&y)
            .then(|| &self.mask[y as usize * self.width..][..self.width])
    }

    /// Row `ly`'s pixels deep in a slab (255): the mask is full at every bevel and rim shift.
    fn deep_row<'d>(
        &self,
        ly: i32,
        style: &SlabStyle,
        near: &mut [u8],
        deep: &'d mut [u8],
    ) -> &'d [u8] {
        let w = self.width;
        let near = &mut near[..w];
        // The least of the rows the vertical bands look at…
        match self.mask_row(ly) {
            Some(r) => near.copy_from_slice(r),
            None => near.fill(0),
        }
        for &(dx, dy, ..) in &style.bands {
            if dx != 0 {
                continue;
            }
            match self.mask_row(ly - dy) {
                Some(r) => {
                    for (n, &v) in near.iter_mut().zip(r) {
                        *n = (*n).min(v);
                    }
                }
                None => near.fill(0),
            }
        }
        // …then the least across the horizontal bands' reach.
        let s = style
            .bands
            .iter()
            .map(|&(dx, ..)| dx.unsigned_abs() as usize)
            .max()
            .unwrap_or(0);
        let deep = &mut deep[..w];
        deep.fill(0);
        if w > 2 * s {
            let inner = &mut deep[s..w - s];
            inner.copy_from_slice(&near[..w - 2 * s]);
            for k in 1..=2 * s {
                for (d, &v) in inner.iter_mut().zip(&near[k..]) {
                    *d = (*d).min(v);
                }
            }
        }
        deep
    }

    /// Device row `y`, columns `x0..x1`, of `line`: the drop shadow, then the slab's fill,
    /// hatch, bevel and rim through the mask.
    fn paint_row(
        &self,
        line: &mut [u32],
        y: i32,
        (x0, x1): (i32, i32),
        style: &SlabStyle,
        rows: &mut LineScratch,
    ) {
        let (origin, u) = (self.origin, self.u);
        let ly = y - self.top;
        let py = y as f32 + 0.5;
        let row = ((py - origin.y) / u).floor() as i32;
        // The shadow's alpha 0.08u up, blended from the ½ rows either side (½ px j is centred on
        // window row 2j + 1), with a zero column padding each end.
        let (shade, shade_a) = style.shadow;
        let sy = (py - style.drop - self.top as f32) / 2.0 - 0.5;
        let (j, fy) = (sy.floor() as i32, sy - sy.floor());
        let (upper, lower) = (self.half_row(j), self.half_row(j + 1));
        let k = shade_a / SHADOW_FULL;
        let shadow_row = &mut rows.shadow[..self.half_width + 2];
        shadow_row.fill(0.0);
        for (r, w) in [(upper, 1.0 - fy), (lower, fy)] {
            if let Some(r) = r {
                for (s, &v) in shadow_row[1..].iter_mut().zip(r) {
                    *s += f32::from(v) * w * k;
                }
            }
        }
        let shadow_row = &*shadow_row;
        let deep = self.deep_row(ly, style, &mut rows.near, &mut rows.deep);
        let Some(mask) = self.mask_row(ly) else {
            return;
        };
        let shadow = |lx: i32| {
            // Window column lx sits ¾ of the way from ½ column i to i + 1 (even lx), or ¼.
            let i = ((lx - 1).div_euclid(2) + 1) as usize;
            let fx = if lx % 2 == 0 { 0.75 } else { 0.25 };
            shadow_row[i] * (1.0 - fx) + shadow_row[i + 1] * fx
        };
        let (hatch, hatch_a) = style.hatch;
        let period = style.period;
        // Hatch lines run bottom-left → top-right: x + y constant, `period` apart in x.
        let mut along = (x0 as f32 + 0.5 - origin.x + py - origin.y).rem_euclid(period);
        let mut x = x0;
        while x < x1 {
            let col = ((x as f32 + 0.5 - origin.x) / u).floor() as i32;
            let end = ((origin.x + (col + 1) as f32 * u - 0.5).ceil() as i32).clamp(x + 1, x1);
            let tone = self.tone(col, row, py, style);
            let plain = pack(tone);
            for (x, p) in (x..end).zip(&mut line[x as usize..end as usize]) {
                let lx = x - self.left;
                let here = along;
                along += 1.0;
                while along >= period {
                    along -= period;
                }
                let m = mask[lx as usize];
                if m == 0 {
                    let s = shadow(lx);
                    if s > 0.0 {
                        let mut d = unpack(*p);
                        over(&mut d, shade, s);
                        *p = pack(d);
                    }
                    continue;
                }
                let off = here.min(period - here) * std::f32::consts::FRAC_1_SQRT_2;
                let line = (style.half_width + 0.5 - off).clamp(0.0, 1.0);
                if deep[lx as usize] == 255 {
                    // Deep in a slab, clear of every band: the fill and the hatch.
                    *p = if line > 0.0 {
                        let mut d = tone;
                        over(&mut d, hatch, hatch_a * line);
                        pack(d)
                    } else {
                        plain
                    };
                    continue;
                }
                let mf = f32::from(m) / 255.0;
                let mut d = if m == 255 {
                    tone
                } else {
                    let mut d = unpack(*p);
                    over(&mut d, shade, shadow(lx));
                    over(&mut d, tone, mf);
                    d
                };
                over(&mut d, hatch, hatch_a * line * mf);
                for &(dx, dy, c, a) in &style.bands {
                    let n = self.cover(lx - dx, ly - dy);
                    if n < 255 {
                        over(&mut d, c, a * mf * (1.0 - f32::from(n) / 255.0));
                    }
                }
                *p = pack(d);
            }
            x = end;
        }
    }
}

/// The raised wall slabs (spec §7.2) of `board`, whose top-left is at `origin`, inside `clip`:
/// per strip of two board rows, in windows around its runs of wall columns, a drop shadow,
/// then each slab's fill, hatch, bevel and rim.
pub fn draw_walls(
    layer: &mut Layer,
    clip: DeviceRect,
    palette: &Palette,
    board: &Board,
    origin: Pt,
    u: f32,
    scratch: &mut WallScratch,
) {
    let canvas = DeviceRect::new(0, 0, layer.width as i32, layer.height as i32);
    let Some(clip) = clip_rect(clip, canvas).and_then(|c| clip_rect(c, scratch.region)) else {
        return;
    };
    if u.is_nan() || u < 1.0 || scratch.rows == 0 || !scratch.label(board) {
        return;
    }
    let walls = Walls {
        board,
        origin,
        u,
        style: SlabStyle::of(palette, u),
    };
    let halo = scratch.halo;
    // Strip k's core rows are [edge(k), edge(k + 1)); the first and last reach a halo past the
    // board, which holds everything a wall draws outside it.
    let edge = |k: usize| match k {
        0 => origin.y.round() as i32 - halo,
        STRIPS => (origin.y + SIDE as f32 * u).round() as i32 + halo,
        k => (origin.y + (k * STRIP_ROWS) as f32 * u).round() as i32,
    };
    for k in 0..STRIPS {
        let core = (edge(k).max(clip.top), edge(k + 1).min(clip.bottom));
        if core.0 >= core.1 {
            continue;
        }
        // Walls that reach the core: its own rows and one more on each side. The strip holds
        // the core and a halo either side, its top on the ½ grid.
        let first = (k * STRIP_ROWS) as i32 - 1;
        let top = edge(0) + (edge(k) - halo - edge(0)).max(0) / 2 * 2;
        let strip = StripRows {
            walls: first.max(0)..=(first + STRIP_ROWS as i32 + 1).min(SIDE as i32 - 1),
            top,
            bottom: (edge(k + 1) + halo)
                .min(edge(STRIPS))
                .min(top + scratch.rows as i32),
            core,
        };
        // The columns with walls in those rows, in runs whose windows (the walls plus the halo)
        // don't meet: nothing a wall draws reaches the next run's window.
        let mut run: Option<(i32, i32)> = None;
        for x in 0..SIDE as i32 {
            if !strip.walls.clone().any(|y| walls.at(x, y)) {
                continue;
            }
            run = match run {
                Some((c0, c1))
                    if scratch.window(&walls, x, x).0 < scratch.window(&walls, c0, c1).1 =>
                {
                    Some((c0, x))
                }
                Some(done) => {
                    draw_window(layer, clip, scratch, &walls, &strip, done);
                    Some((x, x))
                }
                None => Some((x, x)),
            };
        }
        if let Some(done) = run {
            draw_window(layer, clip, scratch, &walls, &strip, done);
        }
    }
}

/// One `draw_walls` call's board, placement and style.
struct Walls<'a> {
    board: &'a Board,
    origin: Pt,
    u: f32,
    style: SlabStyle,
}

impl Walls<'_> {
    fn at(&self, x: i32, y: i32) -> bool {
        (0..SIDE as i32).contains(&x)
            && (0..SIDE as i32).contains(&y)
            && self.board.cells[board_index(x as usize, y as usize)].piece == Piece::Wall
    }
}

/// A strip's rows: the board rows whose walls reach it, its device rows and its core.
struct StripRows {
    walls: std::ops::RangeInclusive<i32>,
    top: i32,
    bottom: i32,
    core: (i32, i32),
}

impl WallScratch {
    /// The device columns of the window around board columns `c0..=c1`: the walls plus the
    /// halo, its left on the ½ grid, inside the region.
    fn window(&self, walls: &Walls, c0: i32, c1: i32) -> (i32, i32) {
        let (region, (o, u)) = (self.region, (walls.origin.x, walls.u));
        let left = ((o + c0 as f32 * u).floor() as i32 - self.halo).max(region.left);
        let right = ((o + (c1 + 1) as f32 * u).ceil() as i32 + self.halo).min(region.right);
        (region.left + (left - region.left) / 2 * 2, right)
    }
}

/// The walls of board columns `c0..=c1` in `strip`'s window: its mask, shadow and core pixels.
fn draw_window(
    layer: &mut Layer,
    clip: DeviceRect,
    scratch: &mut WallScratch,
    walls: &Walls,
    strip: &StripRows,
    (c0, c1): (i32, i32),
) {
    let (origin, u) = (walls.origin, walls.u);
    let (left, right) = scratch.window(walls, c0, c1);
    let (top, bottom) = (strip.top, strip.bottom);
    let (x0, x1) = (clip.left.max(left), clip.right.min(right));
    if x0 >= x1 || top >= bottom {
        return;
    }
    let (mw, mh) = ((right - left) as usize, (bottom - top) as usize);
    let wall = |x: i32, y: i32| walls.at(x, y);

    // 1. The mask: per wall, its 0.8u square (rounded at corners with no neighbour on either
    // side), bridges to its east and south neighbours and the centre of a 2 × 2 block.
    let mask = &mut scratch.mask[..mw * mh];
    mask.fill(0);
    let mut target = SlabMask {
        px: mask,
        width: mw,
        height: mh,
    };
    let (a, radius) = (0.4 * u, 0.12 * u);
    for y in strip.walls.clone() {
        for x in c0..=c1 {
            if !wall(x, y) {
                continue;
            }
            let (cx, cy) = (
                origin.x - left as f32 + (x as f32 + 0.5) * u,
                origin.y - top as f32 + (y as f32 + 0.5) * u,
            );
            let (n, e, s, w) = (
                wall(x, y - 1),
                wall(x + 1, y),
                wall(x, y + 1),
                wall(x - 1, y),
            );
            let round = |open: bool| if open { radius } else { 0.0 };
            let square = Shape::RoundRect {
                left: cx - a,
                top: cy - a,
                right: cx + a,
                bottom: cy + a,
                radii: [
                    round(!n && !w),
                    round(!n && !e),
                    round(!s && !e),
                    round(!s && !w),
                ],
                stroke: 0.0,
            };
            let (near, far) = (a - 1.0, u - a + 1.0);
            let parts = [
                Some(square),
                e.then(|| Shape::rect(cx + near, cy - a, cx + far, cy + a)),
                s.then(|| Shape::rect(cx - a, cy + near, cx + a, cy + far)),
                (e && s && wall(x + 1, y + 1))
                    .then(|| Shape::rect(cx + near, cy + near, cx + far, cy + far)),
            ];
            for shape in parts.into_iter().flatten() {
                cover(&mut target, shape, &mut scratch.line.columns);
            }
        }
    }

    // 2. The drop shadow: the mask at ½ resolution, blurred by σ = 0.09u (0.045u in ½ px).
    let mask = &scratch.mask[..mw * mh];
    let (hw, hh) = (mw.div_ceil(2), mh.div_ceil(2));
    let shadow = &mut scratch.shadow[..hw * hh];
    for (j, line) in shadow.chunks_exact_mut(hw).enumerate() {
        line.fill(0);
        for y in 2 * j..(2 * j + 2).min(mh) {
            let (pairs, odd) = mask[y * mw..][..mw].as_chunks::<2>();
            for (v, &[a, b]) in line.iter_mut().zip(pairs) {
                *v += (u16::from(a) + u16::from(b)) * 16;
            }
            if let (Some(v), [last]) = (line.get_mut(pairs.len()), odd) {
                *v += u16::from(*last) * 16;
            }
        }
    }
    blur_plane(shadow, hw, hh, 0.045 * u, &mut scratch.blur);

    // 3–6. Per pixel of the core: the shadow, then fill, hatch, bevel and rim through the mask.
    let window = Window {
        mask,
        width: mw,
        height: mh,
        shadow: &scratch.shadow[..hw * hh],
        half_width: hw,
        half_height: hh,
        left,
        top,
        labels: &scratch.labels,
        spans: &scratch.spans,
        origin,
        u,
    };
    for y in strip.core.0..strip.core.1 {
        let line = &mut layer.px[y as usize * layer.width..][..layer.width];
        window.paint_row(line, y, (x0, x1), &walls.style, &mut scratch.line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etch::{Layer, pt};
    use lumen::{Board, Cell, Piece, board_index};

    fn black(w: usize, h: usize) -> Vec<u32> {
        vec![0u32; w * h]
    }

    /// Spec §7.2 Light tile: nearly white glass, a shadow 0.03u down (σ 0.04u) inside its
    /// cell, darker below the tile than above it.
    #[test]
    fn a_light_tile_is_white_glass_with_a_shadow_below() {
        let ground = 0x00f0_f2f7u32;
        let mut px = vec![ground; 100 * 100];
        let mut layer = Layer {
            px: &mut px,
            width: 100,
            height: 100,
        };
        let full = DeviceRect::new(0, 0, 100, 100);
        draw_tile(
            &mut layer,
            full,
            &Palette::LIGHT,
            pt(50.0, 50.0),
            100.0,
            true,
        );
        let lum = |p: u32| (p >> 16 & 0xff) + (p >> 8 & 0xff) + (p & 0xff);
        // The glass (white α 0.95 → (223,228,242) α 0.7) covers its own shadow: inside, the
        // tile is as light as the bare ground (α 0.14 of ink would take ≈ 85 off it).
        let glass = px[30 * 100 + 30];
        assert!(lum(glass) + 3 >= lum(ground), "the glass: {glass:06x}");
        // Pinned (review T5 M2): the glaze over its shadow over this ground is #f0f2f7 ± 2.
        let near = |s: u32| (glass >> s & 0xff).abs_diff(0xf0f2f7 >> s & 0xff) <= 2;
        assert!(near(16) && near(8) && near(0), "the glass: {glass:06x}");
        // The tile spans 7..93: 2 px below its bottom edge vs 2 px above its top edge.
        let (below, above) = (px[95 * 100 + 50], px[4 * 100 + 50]);
        assert!(lum(below) + 6 < lum(above), "{below:06x} vs {above:06x}");
        // The shadow's tail reaches 2σ = 8 px past the tile (its edge at 7): at x = 1 it is
        // α ≈ 0.009, within 2 code values of the ground.
        let (side, g) = (px[50 * 100 + 1], ground);
        let close = |s: u32| ((side >> s & 0xff) as i32 - (g >> s & 0xff) as i32).abs() <= 2;
        assert!(
            close(16) && close(8) && close(0),
            "the cell's side edge: {side:06x}"
        );
    }

    fn walled_with(palette: &Palette, cells: &[(usize, usize)]) -> (Vec<u32>, f32) {
        let u = 40.0;
        let mut board = Board::default();
        for &(x, y) in cells {
            board.cells[board_index(x, y)] = Cell {
                piece: Piece::Wall,
                colour: 0,
                dir: 0,
            };
        }
        let (w, h) = (600, 600);
        let mut px = vec![0x0010_1010u32; w * h];
        let region = DeviceRect::new(0, 0, w as i32, h as i32);
        let mut scratch = WallScratch::new();
        scratch.reshape(region, u);
        draw_walls(
            &mut Layer {
                px: &mut px,
                width: w,
                height: h,
            },
            region,
            palette,
            &board,
            pt(0.0, 0.0),
            u,
            &mut scratch,
        );
        (px, u)
    }

    fn walled(cells: &[(usize, usize)]) -> (Vec<u32>, f32) {
        walled_with(&Palette::DARK, cells)
    }

    fn at(px: &[u32], x: f32, y: f32) -> u32 {
        px[y as usize * 600 + x as usize]
    }

    fn channels(p: u32) -> [i32; 3] {
        [
            (p >> 16 & 0xff) as i32,
            (p >> 8 & 0xff) as i32,
            (p & 0xff) as i32,
        ]
    }

    /// The dark palette with a flat fill and no hatch, so bands compare with the interior.
    fn flat() -> Palette {
        let mut flat = Palette::DARK;
        flat.wall.bottom = flat.wall.top;
        flat.wall.hatch = flat.wall.hatch.with_alpha(0.0);
        flat
    }

    #[test]
    fn covering_a_slab_matches_filling_it() {
        let (w, h) = (97, 83);
        let mut columns = vec![0; 2 * w];
        let ink = Paint::Solid(Rgba::CLEAR.with_alpha(1.0));
        let mut k = 0u32;
        let mut next = || {
            k = k.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (k >> 8) as f32 / (1 << 24) as f32
        };
        for _ in 0..4000 {
            let (cx, cy) = (-10.0 + 117.0 * next(), -10.0 + 103.0 * next());
            let (a, b) = (1.0 + 45.0 * next(), 1.0 + 45.0 * next());
            let radius = [0.0, 0.12 * a.min(b), 0.5 * a.min(b)][(3.0 * next()) as usize % 3];
            let radii = [0, 1, 2, 3].map(|_| if next() < 0.5 { radius } else { 0.0 });
            let shape = Shape::RoundRect {
                left: cx - a,
                top: cy - b,
                right: cx + a,
                bottom: cy + b,
                radii,
                stroke: 0.0,
            };
            let (mut fast, mut plain) = (vec![9u8; w * h], vec![9u8; w * h]);
            cover(
                &mut SlabMask {
                    px: &mut fast,
                    width: w,
                    height: h,
                },
                shape,
                &mut columns,
            );
            fill(
                &mut SlabMask {
                    px: &mut plain,
                    width: w,
                    height: h,
                },
                DeviceRect::new(0, 0, w as i32, h as i32),
                &[shape],
                &ink,
                1.0,
                Blend::Over,
            );
            assert!(fast == plain, "{shape:?}");
        }
    }

    #[test]
    fn a_lone_wall_is_a_slab_that_leaves_its_neighbours_alone() {
        let (px, u) = walled(&[(3, 3)]);
        let c = 3.5 * u;
        let [r, g, b] = channels(at(&px, c, c));
        assert!(b > r && (25..=80).contains(&b), "slate fill: {r} {g} {b}");
        assert_eq!(
            at(&px, c + 1.6 * u, c),
            0x0010_1010,
            "the next cell but one is untouched"
        );
    }

    #[test]
    fn bevels_light_the_top_and_shade_the_bottom() {
        let flat = flat();
        let (px, u) = walled_with(&flat, &[(3, 3)]);
        let c = 3.5 * u;
        let inside = channels(at(&px, c, c))[2];
        assert!(
            channels(at(&px, c, c - 0.4 * u + 1.0))[2] > inside + 20,
            "a highlight on the top edge"
        );
        assert!(
            channels(at(&px, c, c + 0.4 * u - 1.5))[2] + 10 < inside,
            "a shade on the bottom edge"
        );
        let (pair, _) = walled_with(&flat, &[(3, 3), (3, 4)]);
        assert!(
            (channels(at(&pair, c, c + 0.4 * u - 1.5))[2] - inside).abs() <= 2,
            "no shade where the slab continues"
        );
    }

    #[test]
    fn strips_join_seamlessly() {
        // A tall slab crosses several two-row strips: no band or shadow line at their borders.
        let (px, u) = walled_with(&flat(), &[(3, 1), (3, 2), (3, 3), (3, 4), (3, 5), (3, 6)]);
        let first = channels(at(&px, 3.5 * u, 1.5 * u));
        for y in (2.0 * u) as usize..(6.5 * u) as usize {
            let p = channels(at(&px, 3.5 * u, y as f32));
            assert!((p[2] - first[2]).abs() <= 2, "row {y}: {p:?} vs {first:?}");
        }
    }

    #[test]
    fn joined_walls_have_no_seam() {
        let (px, u) = walled(&[(3, 3), (4, 3)]);
        let seam = channels(at(&px, 4.0 * u, 3.5 * u));
        let inside = channels(at(&px, 3.5 * u, 3.5 * u));
        // Within a hatch line's worth (α 0.075): a bevel or rim at the seam would be far brighter.
        for k in 0..3 {
            assert!(
                (seam[k] - inside[k]).abs() <= 20,
                "seam {seam:?} vs inside {inside:?}"
            );
        }
    }

    #[test]
    fn a_slab_shades_from_its_own_top_to_its_own_bottom() {
        let (px, u) = walled(&[(3, 2), (3, 3), (3, 4)]);
        let top = channels(at(&px, 3.5 * u, 2.5 * u))[2];
        let bottom = channels(at(&px, 3.5 * u, 4.5 * u))[2];
        assert!(top > bottom + 8, "{top} vs {bottom}");
    }

    #[test]
    fn walls_take_every_colour_from_the_palette() {
        for theme in crate::Theme::ALL {
            let sentinel = theme.palette().recoloured(crate::Rgb(255, 0, 255));
            let mut board = Board::default();
            for (x, y) in [(1, 1), (2, 1), (1, 2), (2, 2), (5, 5)] {
                board.cells[board_index(x, y)] = Cell {
                    piece: Piece::Wall,
                    colour: 0,
                    dir: 0,
                };
            }
            let mut px = vec![0u32; 300 * 300];
            let region = DeviceRect::new(0, 0, 300, 300);
            let mut scratch = WallScratch::new();
            scratch.reshape(region, 40.0);
            let mut layer = Layer {
                px: &mut px,
                width: 300,
                height: 300,
            };
            draw_walls(
                &mut layer,
                region,
                &sentinel,
                &board,
                pt(0.0, 0.0),
                40.0,
                &mut scratch,
            );
            draw_grid(
                &mut layer,
                region,
                &sentinel,
                DeviceRect::new(0, 0, 300, 300),
                20.0,
            );
            draw_tile(&mut layer, region, &sentinel, pt(150.0, 150.0), 40.0, true);
            for &p in &px {
                let [r, g, b] = channels(p);
                assert!(g == 0 && (r - b).abs() <= 1, "{theme:?}: {p:06x}");
            }
        }
    }

    #[test]
    fn a_raised_tiles_glass_is_the_spec_blend() {
        let mut px = black(100, 100);
        let mut layer = Layer {
            px: &mut px,
            width: 100,
            height: 100,
        };
        draw_tile(
            &mut layer,
            DeviceRect::new(0, 0, 100, 100),
            &Palette::DARK,
            pt(50.0, 50.0),
            100.0,
            true,
        );
        // Halfway along the fill: glass α 0.135 and the tile's far stop α 0.045,
        // straight-interpolated to (145, 165, 237.5) at α 0.09, over black.
        let p = px[50 * 100 + 50];
        let (r, g, b) = (
            (p >> 16 & 0xff) as i32,
            (p >> 8 & 0xff) as i32,
            (p & 0xff) as i32,
        );
        assert!(
            (r - 13).abs() <= 1 && (g - 15).abs() <= 1 && (b - 21).abs() <= 1,
            "{r} {g} {b}"
        );
        assert!(
            (px[50 * 100 + 7] & 0xff) as i32 > b + 10,
            "the rim (on the edge at x = 7) is brighter than the glass"
        );
        assert_eq!(
            px[50 * 100 + 2],
            0,
            "inset 0.07u: nothing at the cell's edge"
        );
    }

    #[test]
    fn fixed_tiles_are_fainter_than_movable_ones() {
        let mut raised = black(64, 64);
        let mut flat = black(64, 64);
        let clip = DeviceRect::new(0, 0, 64, 64);
        draw_tile(
            &mut Layer {
                px: &mut raised,
                width: 64,
                height: 64,
            },
            clip,
            &Palette::DARK,
            pt(32.0, 32.0),
            64.0,
            true,
        );
        draw_tile(
            &mut Layer {
                px: &mut flat,
                width: 64,
                height: 64,
            },
            clip,
            &Palette::DARK,
            pt(32.0, 32.0),
            64.0,
            false,
        );
        assert!(raised[32 * 64 + 32] > flat[32 * 64 + 32]);
    }

    #[test]
    fn grid_lines_run_past_the_board_and_fade_outward() {
        let (w, h) = (500, 500);
        let mut px = black(w, h);
        let board = DeviceRect::new(25, 25, 475, 475); // 15 cells of 30 px
        draw_grid(
            &mut Layer {
                px: &mut px,
                width: w,
                height: h,
            },
            DeviceRect::new(0, 0, 500, 500),
            &Palette::DARK,
            board,
            30.0,
        );
        let at = |x: usize, y: usize| px[y * w + x] & 0xff;
        assert!(at(25, 16) > 0, "lines overhang 0.35u above the board");
        assert_eq!(at(25, 12), 0, "and stop there");
        assert!(
            at(235, 250) > at(25, 250),
            "brighter near the centre (x = 235 is a line, 250 a cell middle)"
        );
        assert_eq!(at(40, 40), 0, "a cell's middle is empty");
    }
}
