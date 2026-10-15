//! `bloom`: planar light (one u16 plane per channel, `ONE` = 1.0, 16× headroom) and the passes
//! that make glow and bloom from it (spec §8): 2×2 downsamples, three zero-padded box passes per
//! axis, and a bilinear upsample-add. Loops run over whole rows so they vectorise.

use crate::etch::{Blend, Target, round_away};
use crate::fit_buffer;
use crate::spectrum::Rgba;

/// 1.0 in a light plane.
pub const ONE: u32 = 4096;

#[derive(Clone, Debug, Default)]
pub struct Planes {
    pub width: usize,
    pub height: usize,
    pub r: Vec<u16>,
    pub g: Vec<u16>,
    pub b: Vec<u16>,
}

impl Planes {
    pub fn new(width: usize, height: usize) -> Planes {
        let mut p = Planes::default();
        p.reshape(width, height);
        p
    }

    /// `width × height`, cleared. Allocates only to grow or to give back a much larger plane.
    pub fn reshape(&mut self, width: usize, height: usize) {
        (self.width, self.height) = (width, height);
        for plane in [&mut self.r, &mut self.g, &mut self.b] {
            fit_buffer(plane, width * height, 0);
        }
    }

    pub fn clear(&mut self) {
        for plane in [&mut self.r, &mut self.g, &mut self.b] {
            plane.fill(0);
        }
    }

    pub fn bytes(&self) -> usize {
        (self.r.capacity() + self.g.capacity() + self.b.capacity()) * 2
    }

    pub fn at(&self, x: usize, y: usize) -> [u32; 3] {
        let i = y * self.width + x;
        [self.r[i].into(), self.g[i].into(), self.b[i].into()]
    }

    /// Bilinear, with pixel (i, j)'s centre at (i, j); zero outside.
    pub fn sample(&self, x: f32, y: f32) -> [u32; 3] {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let mut out = [0.0f32; 3];
        for (dx, dy, w) in [
            (0, 0, (1.0 - fx) * (1.0 - fy)),
            (1, 0, fx * (1.0 - fy)),
            (0, 1, (1.0 - fx) * fy),
            (1, 1, fx * fy),
        ] {
            let (px, py) = (x0 + dx, y0 + dy);
            if w <= 0.0 || px < 0 || py < 0 || px >= self.width as i64 || py >= self.height as i64 {
                continue;
            }
            let v = self.at(px as usize, py as usize);
            for (o, &v) in out.iter_mut().zip(v.iter()) {
                *o += w * v as f32;
            }
        }
        out.map(|v| round_away(v) as u32)
    }

    pub fn total(&self) -> [u64; 3] {
        let sum = |p: &[u16]| p.iter().map(|&v| u64::from(v)).sum();
        [sum(&self.r), sum(&self.g), sum(&self.b)]
    }

    /// Copies `other`, of the same size, into these planes (no allocation).
    pub fn copy_from(&mut self, other: &Planes) {
        assert!((self.width, self.height) == (other.width, other.height));
        self.r.copy_from_slice(&other.r);
        self.g.copy_from_slice(&other.g);
        self.b.copy_from_slice(&other.b);
    }
}

impl Target for Planes {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn blend(&mut self, x: usize, y: usize, c: Rgba, blend: Blend) {
        let i = y * self.width + x;
        for (plane, v) in [(&mut self.r, c.r), (&mut self.g, c.g), (&mut self.b, c.b)] {
            plane[i] = light_value(plane[i], v, c.a, blend);
        }
    }

    fn blend_span(&mut self, x0: usize, y: usize, colours: &[Rgba], blend: Blend) {
        let row = y * self.width + x0;
        let n = colours.len();
        let (r, g, b) = (
            &mut self.r[row..row + n],
            &mut self.g[row..row + n],
            &mut self.b[row..row + n],
        );
        for (i, &c) in colours.iter().enumerate() {
            if c.a > 0.0 {
                r[i] = light_value(r[i], c.r, c.a, blend);
                g[i] = light_value(g[i], c.g, c.a, blend);
                b[i] = light_value(b[i], c.b, c.a, blend);
            }
        }
    }
}

/// A light plane's value `d` with channel `v` at alpha `a` blended in: `Planes`' px.
#[inline(always)]
fn light_value(d: u16, v: f32, a: f32, blend: Blend) -> u16 {
    let add = v * a * ONE as f32;
    let d = f32::from(d);
    let out = match blend {
        Blend::Add => d + add,
        Blend::Over | Blend::Multiply => d * (1.0 - a) + add,
    };
    // `as` saturates at both ends (and takes NaN to 0), as `clamp(0.0, 65 535.0)` then `as` did.
    round_away(out) as u16
}

/// `factor × factor` means (2 for ½, 4 for ½ → ⅛ in one pass): `dst` must be
/// `ceil(src / factor)` on each side. Taps past the edge count as 0, so energy (value × area) is kept.
pub fn downsample(src: &Planes, dst: &mut Planes, factor: usize) {
    // Plain asserts: no panic messages in the shipped code (the page's size cap).
    assert!(dst.width == src.width.div_ceil(factor) && dst.height == src.height.div_ceil(factor));
    assert!(factor == 2 || factor == 4);
    // An empty plane has nothing to take (`chunks_exact_mut(0)` would panic).
    if dst.width == 0 || dst.height == 0 {
        return;
    }
    if factor == 2 {
        for (s, d) in [
            (&src.r, &mut dst.r),
            (&src.g, &mut dst.g),
            (&src.b, &mut dst.b),
        ] {
            for (y, out) in d.chunks_exact_mut(dst.width).enumerate() {
                downsampled_row(s, (src.width, src.height), (0, y), out);
            }
        }
    } else {
        downsample_by::<4>(src, dst);
    }
}

/// `downsample` by `F`: per destination row, the sums of whole `F`-column groups in blocks on the
/// stack, then the part group at a ragged right edge.
fn downsample_by<const F: usize>(src: &Planes, dst: &mut Planes) {
    const BLOCK: usize = 64;
    let area = (F * F) as u32;
    let mean = |sum: u32| ((sum + area / 2) / area) as u16;
    let whole = src.width / F;
    for (s, d) in [
        (&src.r, &mut dst.r),
        (&src.g, &mut dst.g),
        (&src.b, &mut dst.b),
    ] {
        for (y, out) in d.chunks_exact_mut(dst.width).enumerate() {
            let rows = &s[F * y * src.width..(F * y + F).min(src.height) * src.width];
            for (k, block) in out[..whole].chunks_mut(BLOCK).enumerate() {
                let mut sums = [0u32; BLOCK];
                for row in rows.chunks_exact(src.width) {
                    let (groups, _) = row[F * BLOCK * k..][..F * block.len()].as_chunks::<F>();
                    for (sum, group) in sums.iter_mut().zip(groups) {
                        *sum += group.iter().map(|&v| u32::from(v)).sum::<u32>();
                    }
                }
                for (o, &sum) in block.iter_mut().zip(&sums) {
                    *o = mean(sum);
                }
            }
            if let Some(o) = out.get_mut(whole) {
                *o = mean(
                    rows.chunks_exact(src.width)
                        .map(|row| row[F * whole..].iter().map(|&v| u32::from(v)).sum::<u32>())
                        .sum(),
                );
            }
        }
    }
}

/// `downsample`'s value at (`x`, `y`) of a plane `factor` times smaller than `plane`: the sum of
/// the block's pixels inside the plane, over the whole block's area, rounded as it rounds.
pub fn block_mean(
    plane: &[u16],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    factor: usize,
) -> u16 {
    let area = (factor * factor) as u32;
    let (x0, y0) = (factor * x, factor * y);
    let x1 = (x0 + factor).min(width);
    let mut sum = 0u32;
    for row in y0..(y0 + factor).min(height) {
        sum += plane[row * width + x0..row * width + x1]
            .iter()
            .map(|&v| u32::from(v))
            .sum::<u32>();
    }
    ((sum + area / 2) / area) as u16
}

/// `downsample`'s ½ px (`x0 + i`, `y`) into `out[i]`, from a `sw × sh` plane: the 2×2 means of
/// sharp rows `2y` and `2y + 1` (the second off the plane at an odd last row) by column pairs,
/// then the part pair at a ragged right edge, each the in-plane sum over 4, as `block_mean`.
pub(crate) fn downsampled_row(
    plane: &[u16],
    (sw, sh): (usize, usize),
    (x0, y): (usize, usize),
    out: &mut [u16],
) {
    let mean = |sum: u32| ((sum + 2) / 4) as u16;
    let rows = &plane[2 * y * sw..(2 * y + 2).min(sh) * sw];
    let whole = out.len().min((sw / 2).saturating_sub(x0));
    let (body, ragged) = out.split_at_mut(whole);
    let (first, second) = rows.split_at(sw);
    let (p0, _) = first[2 * x0..][..2 * whole].as_chunks::<2>();
    if second.is_empty() {
        for (o, a) in body.iter_mut().zip(p0) {
            *o = mean(u32::from(a[0]) + u32::from(a[1]));
        }
    } else {
        let (p1, _) = second[2 * x0..][..2 * whole].as_chunks::<2>();
        for ((o, a), b) in body.iter_mut().zip(p0).zip(p1) {
            *o = mean(u32::from(a[0]) + u32::from(a[1]) + u32::from(b[0]) + u32::from(b[1]));
        }
    }
    // At most one: the last ½ column of a plane of odd width holds one sharp column.
    if let Some(o) = ragged.first_mut() {
        let last = rows.chunks_exact(sw).map(|row| u32::from(row[sw - 1]));
        *o = mean(last.sum());
    }
}

/// The box radius `blur` uses for `sigma`.
pub fn box_radius(sigma: f32) -> usize {
    box_width(sigma).min(MAX_BOX) / 2
}

/// The odd box width whose three passes approximate a Gaussian of `sigma` px: nearest odd to
/// `√(4σ² + 1)`. 1 (no blur) for σ ≤ 0 or NaN.
pub fn box_width(sigma: f32) -> usize {
    if sigma.is_nan() || sigma <= 0.0 {
        return 1;
    }
    let w = (4.0 * sigma * sigma + 1.0).sqrt();
    2 * ((w - 1.0) / 2.0).round() as usize + 1
}

/// Lines blurred together in one pass: columns of the vertical blur, rows of the horizontal one.
pub const LANES: usize = 32;

/// The scratch `blur` needs for a `width × height` plane: reserve it with the planes so a paint
/// never allocates.
pub fn blur_scratch(width: usize, height: usize) -> usize {
    2 * LANES * width.max(height)
}

/// Three box passes per axis, zero outside. Lines in blocks of `LANES`, interleaved so that every
/// lane's running sum advances together: rows transposed, columns as they lie.
pub fn blur(planes: &mut Planes, sigma: f32, scratch: &mut Vec<u16>) {
    let (width, height) = (planes.width, planes.height);
    for plane in [&mut planes.r, &mut planes.g, &mut planes.b] {
        blur_plane(plane, width, height, sigma, scratch);
    }
}

/// A plane's value as the box passes hold it: u16 light (dark), or u8 premultiplied paint
/// (`gouache`), widened into the u16 lanes and narrowed back (a box mean of u8 stays a u8).
pub trait Lane: Copy + Into<u16> {
    fn narrow(v: u16) -> Self;
    /// `dst = src`, widened (a block of columns into the scratch).
    fn load(dst: &mut [u16], src: &[Self]);
    /// `dst = src`, narrowed (the block back into the plane).
    fn store(dst: &mut [Self], src: &[u16]);
}

impl Lane for u16 {
    #[inline(always)]
    fn narrow(v: u16) -> u16 {
        v
    }

    #[inline(always)]
    fn load(dst: &mut [u16], src: &[u16]) {
        dst.copy_from_slice(src);
    }

    #[inline(always)]
    fn store(dst: &mut [u16], src: &[u16]) {
        dst.copy_from_slice(src);
    }
}

impl Lane for u8 {
    #[inline(always)]
    fn narrow(v: u16) -> u8 {
        v as u8
    }

    #[inline(always)]
    fn load(dst: &mut [u16], src: &[u8]) {
        for (d, &s) in dst.iter_mut().zip(src) {
            *d = u16::from(s);
        }
    }

    #[inline(always)]
    fn store(dst: &mut [u8], src: &[u16]) {
        for (d, &s) in dst.iter_mut().zip(src) {
            *d = s as u8;
        }
    }
}

/// `blur` for one `width × height` plane of u16 light (the walls' shadow mask uses it too) or
/// u8 paint.
pub fn blur_plane<P: Lane>(
    plane: &mut [P],
    width: usize,
    height: usize,
    sigma: f32,
    scratch: &mut Vec<u16>,
) {
    let radius = box_radius(sigma);
    if radius == 0 || width == 0 || height == 0 {
        return;
    }
    let n = blur_scratch(width, height) / 2;
    // Every value is written before it is read: only the length matters, and a longer scratch
    // (left by a larger plane) serves as well, so planes of two sizes in turn never re-zero it.
    if scratch.len() < 2 * n {
        scratch.clear();
        scratch.resize(2 * n, 0);
    }
    let (a, b) = scratch[..2 * n].split_at_mut(n);
    // The rows holding any light (first, end): only they, and 3r rows round them, can be lit
    // after the column passes.
    let mut lit_rows = (height, 0);
    // Up to box radius 4 the row pass blurs each row as it lies (`row_boxes`) instead of
    // transposing blocks of rows into lanes: the same sums and means, in less time (u8 paint and
    // u16 light alike).
    let direct = radius <= ROW_PAD;
    if direct {
        let (a, b) = (&mut a[..width + 2 * ROW_PAD], &mut b[..width + 2 * ROW_PAD]);
        // The zero pads round the row, which no pass writes.
        for p in [&mut *a, &mut *b] {
            p.fill(0);
        }
        // Each row as it lies (no transpose); a dark row stays dark.
        for (y, row) in plane.chunks_exact_mut(width).enumerate() {
            if row.iter().all(|&v| v.into() == 0u16) {
                continue;
            }
            lit_rows = (lit_rows.0.min(y), y + 1);
            P::load(&mut a[ROW_PAD..ROW_PAD + width], row);
            match radius {
                1 => row_boxes::<1>(a, b),
                2 => row_boxes::<2>(a, b),
                3 => row_boxes::<3>(a, b),
                _ => row_boxes::<4>(a, b),
            }
            P::store(row, &b[ROW_PAD..ROW_PAD + width]);
        }
    }
    // A block of dark lines stays dark: it is skipped.
    for (k, rows) in plane
        .chunks_mut(LANES * width)
        .enumerate()
        .filter(|_| !direct)
    {
        if rows.iter().all(|&v| v.into() == 0u16) {
            continue;
        }
        let lanes = rows.len() / width;
        lit_rows = (lit_rows.0.min(k * LANES), k * LANES + lanes);
        let len = width * lanes;
        for (lane, row) in rows.chunks_exact(width).enumerate() {
            for (cell, &v) in a[..len].chunks_exact_mut(lanes).zip(row) {
                cell[lane] = v.into();
            }
        }
        three_boxes(&mut a[..len], &mut b[..len], radius, lanes);
        for (lane, row) in rows.chunks_exact_mut(width).enumerate() {
            for (dst, cell) in row.iter_mut().zip(b[..len].chunks_exact(lanes)) {
                *dst = P::narrow(cell[lane]);
            }
        }
    }
    if lit_rows.0 >= lit_rows.1 {
        return;
    }
    // Zero padding at these rows' ends is the zero rows past them, at every pass.
    let (y0, y1) = (
        lit_rows.0.saturating_sub(3 * radius),
        (lit_rows.1 + 3 * radius).min(height),
    );
    for x0 in (0..width).step_by(LANES) {
        let lanes = LANES.min(width - x0);
        let len = (y1 - y0) * lanes;
        let mut lit = 0;
        for (y, block) in (y0..).zip(a[..len].chunks_exact_mut(lanes)) {
            P::load(block, &plane[y * width + x0..][..lanes]);
            lit |= block.iter().fold(0, |m, &v| m | v);
        }
        if lit == 0 {
            continue;
        }
        three_boxes(&mut a[..len], &mut b[..len], radius, lanes);
        for (y, block) in (y0..).zip(b[..len].chunks_exact(lanes)) {
            P::store(&mut plane[y * width + x0..][..lanes], block);
        }
    }
}

/// The zero pad each side of a row `row_boxes` blurs: its widest box's radius.
const ROW_PAD: usize = 4;

/// `three_boxes` of radius `R` (1 to `ROW_PAD`) along one row as it lies, from `a` into `b`
/// (each the row between zero pads of `ROW_PAD`): three box means, each the sum of the `2R + 1`
/// values round a px (the pads standing for the zero padding) through `BoxMean`, which is what
/// `box_block`'s running sums hold there. Passes write only the row, so the pads stay zero.
fn row_boxes<const R: usize>(a: &mut [u16], b: &mut [u16]) {
    let w = a.len() - 2 * ROW_PAD;
    let mean = BoxMean::new((2 * R + 1) as u32);
    let pass = |s: &[u16], d: &mut [u16]| {
        let d = &mut d[ROW_PAD..ROW_PAD + w];
        for (o, near) in d.iter_mut().zip(s.array_windows::<{ 2 * ROW_PAD + 1 }>()) {
            let near = &near[ROW_PAD - R..=ROW_PAD + R];
            *o = mean.of(near.iter().map(|&v| u32::from(v)).sum());
        }
    };
    pass(a, b);
    pass(b, a);
    pass(a, b);
}

/// `a → b → a → b`, each a running box mean of radius `radius`, zero-padded, over `lanes`
/// interleaved lines. A mean of u16 values is a u16.
fn three_boxes(a: &mut [u16], b: &mut [u16], radius: usize, lanes: usize) {
    if lanes == LANES {
        box_block::<LANES>(a, b, radius);
        box_block::<LANES>(b, a, radius);
        box_block::<LANES>(a, b, radius);
    } else {
        box_lanes(a, b, radius, lanes);
        box_lanes(b, a, radius, lanes);
        box_lanes(a, b, radius, lanes);
    }
}

/// The widest box `blur_plane` runs (σ ≈ 127 px, far past any glow, bloom or wall shadow):
/// `BoxMean` is exact up to it.
const MAX_BOX: usize = 255;

/// Rounded division of a box sum by its width `w` (up to `MAX_BOX`) in f32, which vectorises
/// (there is no SIMD integer divide): exact for every sum of `w` values up to 65 535. The
/// dividend n = sum + w/2 < 2²⁴ is exact in f32, and a quotient that is not whole lies at least
/// 1/w ≥ 1/255 below the next integer, more than the 2⁻⁹ rounding of an f32 under 65 536, so
/// the truncated quotient is ⌊n / w⌋.
#[derive(Clone, Copy)]
struct BoxMean {
    half: u32,
    width: f32,
}

impl BoxMean {
    fn new(w: u32) -> BoxMean {
        debug_assert!((1..=MAX_BOX as u32).contains(&w), "box width {w}");
        BoxMean {
            half: w / 2,
            width: w as f32,
        }
    }

    /// The quotient is at most 65 535.5 (a sum of `w` values up to 65 535, plus w/2, over w), so
    /// its truncation fits a u16 and goes through u32: the same value as a saturating `as u16`,
    /// in a conversion wasm SIMD has (`i32x4.trunc_sat_f32x4_u`; `as u16` is done lane by lane).
    #[inline(always)]
    fn of(self, sum: u32) -> u16 {
        ((sum + self.half) as f32 / self.width) as u32 as u16
    }
}

/// `box_lanes` for a full block of `L` lanes, whose running sums stay in registers.
fn box_block<const L: usize>(src: &[u16], dst: &mut [u16], radius: usize) {
    let (src, _) = src.as_chunks::<L>();
    let (dst, _) = dst.as_chunks_mut::<L>();
    let rows = src.len().min(dst.len());
    if rows == 0 {
        return;
    }
    let mean = BoxMean::new((2 * radius + 1) as u32);
    let mut sum = [0u32; L];
    for row in &src[..=radius.min(rows - 1)] {
        for (s, &v) in sum.iter_mut().zip(row) {
            *s += u32::from(v);
        }
    }
    for i in 0..rows {
        for (d, &s) in dst[i].iter_mut().zip(&sum) {
            *d = mean.of(s);
        }
        if let Some(row) = src.get(i + radius + 1) {
            for (s, &v) in sum.iter_mut().zip(row) {
                *s += u32::from(v);
            }
        }
        if i >= radius {
            for (s, &v) in sum.iter_mut().zip(&src[i - radius]) {
                *s -= u32::from(v);
            }
        }
    }
}

fn box_lanes(src: &[u16], dst: &mut [u16], radius: usize, lanes: usize) {
    let rows = src.len() / lanes;
    if rows == 0 {
        return;
    }
    let mean = BoxMean::new((2 * radius + 1) as u32);
    let mut sums = [0u32; LANES];
    let sum = &mut sums[..lanes];
    for r in 0..=radius.min(rows - 1) {
        for (s, &v) in sum.iter_mut().zip(&src[r * lanes..][..lanes]) {
            *s += u32::from(v);
        }
    }
    for i in 0..rows {
        for (d, &s) in dst[i * lanes..][..lanes].iter_mut().zip(sum.iter()) {
            *d = mean.of(s);
        }
        if i + radius + 1 < rows {
            for (s, &v) in sum
                .iter_mut()
                .zip(&src[(i + radius + 1) * lanes..][..lanes])
            {
                *s += u32::from(v);
            }
        }
        if i >= radius {
            for (s, &v) in sum.iter_mut().zip(&src[(i - radius) * lanes..][..lanes]) {
                *s -= u32::from(v);
            }
        }
    }
}

/// Multiplies every value by `weight`.
pub fn scale(planes: &mut Planes, weight: f32) {
    for plane in [&mut planes.r, &mut planes.g, &mut planes.b] {
        scale_values(plane, weight);
    }
}

/// `scale` for one run of values.
pub(crate) fn scale_values(values: &mut [u16], weight: f32) {
    for v in values {
        *v = round_away(f32::from(*v) * weight).min(65_535.0) as u16;
    }
}

/// The two source taps (index, weight in `1/(2·factor)`) of destination pixel `p` in an exact
/// bilinear upsample by an integer `factor`; a tap past the edge gets index 0 and weight 0.
pub(crate) fn taps(p: usize, factor: usize, len: usize) -> [(usize, u32); 2] {
    let span = 2 * factor as i64;
    let n = 2 * p as i64 + 1 - factor as i64;
    let (i0, f) = (n.div_euclid(span), n.rem_euclid(span) as u32);
    let at = |i: i64, w: u32| {
        if (0..len as i64).contains(&i) {
            (i as usize, w)
        } else {
            (0, 0)
        }
    };
    [at(i0, span as u32 - f), at(i0 + 1, f)]
}

/// `dst += weight × src` upsampled bilinearly by the integer `factor`, 2 or 4 (src pixel centre
/// i ↔ dst `(i + 0.5)·factor − 0.5`), zero outside, saturating; `weight` ≤ 1. Integer
/// arithmetic throughout.
pub fn add_upsampled(dst: &mut Planes, src: &Planes, factor: usize, weight: f32) {
    assert!(factor == 2 || factor == 4);
    let w = (weight * 256.0).round() as u32;
    assert!(w <= 256);
    if src.width == 0 || src.height == 0 || dst.width == 0 || dst.height == 0 {
        return;
    }
    if factor == 2 {
        add_upsampled_by::<2>(dst, src, w);
    } else {
        add_upsampled_by::<4>(dst, src, w);
    }
}

/// `add_upsampled` by `F` with the weight `w` in 1/256. Source column j and j + 1 feed the `F`
/// destination pixels from `F·j + F/2`, the p-th of them weighted `2F − 2p − 1` and `2p + 1`; so a
/// row blends each source column once per run of `F` pixels. Sums stay below 2³²:
/// 65 535 · (2F)² · 256.
fn add_upsampled_by<const F: usize>(dst: &mut Planes, src: &Planes, w: u32) {
    let span = 2 * F as u32;
    let denom = span * span * 256;
    let (sw, dw) = (src.width, dst.width);
    // A row's first F/2 px and its last ones past the runs take taps of their own, the same in
    // every row, so they are worked out once: at most F/2 + F ≤ 6 px (a tail px past the first
    // F would lie past F·sw + F/2, where both taps are off the source and add 0).
    let lead = (F / 2).min(dw);
    let runs = (dw - lead).min(F * (sw - 1)) / F;
    let tail = lead + F * runs;
    let n = lead + (dw - tail).min(F);
    const { assert!(F <= 4) };
    let mut edges = [(0, [(0, 0); 2]); 6];
    for (k, e) in edges[..n].iter_mut().enumerate() {
        let x = if k < lead { k } else { tail + k - lead };
        *e = (x, taps(x, F, sw));
    }
    for y in 0..dst.height {
        let [(ra, wa), (rb, wb)] = taps(y, F, src.height);
        for (d, s) in [
            (&mut dst.r, &src.r),
            (&mut dst.g, &src.g),
            (&mut dst.b, &src.b),
        ] {
            let (a, b) = (&s[ra * sw..][..sw], &s[rb * sw..][..sw]);
            let column = |j: usize| u32::from(a[j]) * wa + u32::from(b[j]) * wb;
            let add = |d: &mut u16, acc: u32| {
                *d = (u32::from(*d) + (acc * w + denom / 2) / denom).min(65_535) as u16;
            };
            let row = &mut d[y * dw..][..dw];
            let (body, _) = row[lead..tail].as_chunks_mut::<F>();
            for (j, run) in body.iter_mut().enumerate() {
                let (c0, c1) = (column(j), column(j + 1));
                for (p, d) in run.iter_mut().enumerate() {
                    let p = p as u32;
                    add(d, c0 * (span - 2 * p - 1) + c1 * (2 * p + 1));
                }
            }
            // Edge px x, from source columns i0 and i0 + 1 (either may be past an edge).
            for &(x, [(i0, w0), (i1, w1)]) in &edges[..n] {
                add(&mut row[x], column(i0) * w0 + column(i1) * w1);
            }
        }
    }
}

/// What `add_upsampled(dst, src, factor, weight)` adds at destination pixel (`x`, `y`), per
/// channel: the same taps, weights and rounding, one pixel at a time (the tests' reference).
#[cfg(test)]
pub fn upsampled_at(src: &Planes, factor: usize, weight: f32, x: usize, y: usize) -> [u32; 3] {
    if src.width == 0 || src.height == 0 {
        return [0; 3];
    }
    let w = round_away(weight * 256.0) as u32;
    let span = 2 * factor as u32;
    let denom = span * span * 256;
    let sw = src.width;
    let [(ra, wa), (rb, wb)] = taps(y, factor, src.height);
    let [(i0, w0), (i1, w1)] = taps(x, factor, sw);
    [&src.r, &src.g, &src.b].map(|p| {
        let column = |j: usize| u32::from(p[ra * sw + j]) * wa + u32::from(p[rb * sw + j]) * wb;
        ((column(i0) * w0 + column(i1) * w1) * w + denom / 2) / denom
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceRect;
    use crate::etch::{Paint, Shape, fill, pt};
    use crate::spectrum::Rgb;

    /// Ruling G4: a span blends into the light planes exactly as its px one by one, zero alphas
    /// untouched, in every blend.
    #[test]
    fn spans_blend_like_single_px_in_the_planes() {
        for blend in [Blend::Over, Blend::Add, Blend::Multiply] {
            let colours = crate::etch::span_colours(150, 13);
            let mut start = Planes::new(200, 3);
            for (i, v) in start.r.iter_mut().enumerate() {
                *v = (i as u32).wrapping_mul(0x9e37_79b9) as u16;
            }
            start.g.fill(7000);
            start.b.fill(65_000);
            let (mut one, mut span) = (start.clone(), start.clone());
            for (i, &c) in colours.iter().enumerate() {
                if c.a > 0.0 {
                    one.blend(30 + i, 1, c, blend);
                }
            }
            span.blend_span(30, 1, &colours, blend);
            let planes = |p: &Planes| [p.r.clone(), p.g.clone(), p.b.clone()];
            assert!(planes(&one) == planes(&span), "{blend:?}");
            assert!(planes(&one) != planes(&start), "{blend:?} drew");
        }
    }

    fn impulse(size: usize, value: u16) -> Planes {
        let mut p = Planes::new(size, size);
        let c = size / 2 * size + size / 2;
        p.r[c] = value;
        p.g[c] = value / 2;
        p
    }

    #[test]
    fn box_widths_follow_sigma() {
        assert_eq!([0.0, 1.0, 3.8, 6.0, 20.0].map(box_width), [1, 3, 7, 13, 41]);
        assert_eq!(box_width(f32::NAN), 1);
    }

    #[test]
    fn blurring_conserves_energy_and_spreads_by_sigma() {
        let mut p = impulse(121, 60_000);
        let before = p.total();
        let mut scratch = Vec::new();
        blur(&mut p, 6.0, &mut scratch);
        let after = p.total();
        for k in 0..2 {
            let (a, b) = (before[k] as f64, after[k] as f64);
            assert!((a - b).abs() <= a * 0.01, "channel {k}: {a} → {b}");
        }
        let (mut sum, mut var) = (0.0, 0.0);
        for x in 0..121 {
            let v = f64::from(p.r[60 * 121 + x]);
            sum += v;
            var += v * (x as f64 - 60.0).powi(2);
        }
        let sigma = (var / sum).sqrt();
        assert!((sigma - 6.0).abs() < 0.15 * 6.0, "measured σ {sigma}");
    }

    #[test]
    fn downsampling_halves_each_side_and_keeps_energy() {
        let mut src = Planes::new(9, 7);
        src.r.fill(400);
        let mut dst = Planes::new(5, 4);
        downsample(&src, &mut dst, 2);
        assert_eq!(dst.at(0, 0)[0], 400);
        assert_eq!(dst.at(4, 3)[0], 100, "a corner tap with one source pixel");
        let energy = |p: &Planes, k: u64| p.total()[0] * k;
        assert!((energy(&dst, 4) as i64 - energy(&src, 1) as i64).abs() < 1000);
        let mut eighth = Planes::new(3, 2);
        downsample(&src, &mut eighth, 4);
        assert_eq!(eighth.at(0, 0)[0], 400);
        assert_eq!(
            eighth.at(2, 1)[0],
            75,
            "one column × three rows of a 4 × 4 block"
        );
    }

    #[test]
    fn downsampling_matches_plain_block_means() {
        for (w, h) in [(300, 9), (517, 13), (64, 4), (129, 6)] {
            let mut src = Planes::new(w, h);
            for (i, v) in src.g.iter_mut().enumerate() {
                *v = ((i * 7919) % 65_536) as u16;
            }
            for factor in [2, 4] {
                let mut dst = Planes::new(w.div_ceil(factor), h.div_ceil(factor));
                downsample(&src, &mut dst, factor);
                for y in 0..dst.height {
                    for x in 0..dst.width {
                        let want = block_mean(&src.g, w, h, x, y, factor);
                        assert_eq!(
                            dst.g[y * dst.width + x],
                            want,
                            "{w}×{h} /{factor} ({x}, {y})"
                        );
                    }
                    // A row's segments, by rows (the dark glow windows' band), from every start
                    // to the plane's right edge and one px short of it.
                    for x0 in (factor == 2).then_some(0..dst.width).into_iter().flatten() {
                        for x1 in [dst.width - 1, dst.width].into_iter().filter(|&x1| x1 > x0) {
                            let mut row = vec![0; x1 - x0];
                            downsampled_row(&src.g, (w, h), (x0, y), &mut row);
                            assert!(row == dst.g[y * dst.width..][x0..x1], "{w}×{h} y {y} {x0}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn upsampling_matches_plain_bilinear_taps() {
        // The per-pixel definition: both taps of both axes, u64, zero past the edges.
        let plain = |dst: &mut Planes, src: &Planes, factor: usize, w: u64| {
            let span = 2 * factor as i64;
            let taps = |p: usize, len: usize| {
                let n = 2 * p as i64 + 1 - factor as i64;
                let (i0, f) = (n.div_euclid(span), n.rem_euclid(span));
                [(i0, span - f), (i0 + 1, f)]
                    .map(|(i, w)| ((0..len as i64).contains(&i).then_some(i as usize), w as u64))
            };
            let denom = (span * span * 256) as u64;
            for y in 0..dst.height {
                for x in 0..dst.width {
                    let mut acc = 0u64;
                    for (ry, wy) in taps(y, src.height) {
                        for (rx, wx) in taps(x, src.width) {
                            if let (Some(ry), Some(rx)) = (ry, rx) {
                                acc += u64::from(src.b[ry * src.width + rx]) * wy * wx;
                            }
                        }
                    }
                    let d = &mut dst.b[y * dst.width + x];
                    *d = (u64::from(*d) + (acc * w + denom / 2) / denom).min(65_535) as u16;
                }
            }
        };
        // The last two: a destination wider than its source covers (tail px past its taps).
        let sizes: [(usize, usize); 7] = [
            (33, 17),
            (16, 16),
            (61, 9),
            (5, 3),
            (130, 7),
            (40, 9),
            (23, 30),
        ];
        for (k, (w, h)) in sizes.into_iter().enumerate() {
            for factor in [2, 4] {
                let fit = (w.div_ceil(factor), h.div_ceil(factor));
                let (sw, sh) = if k < 5 { fit } else { (3, 4) };
                let mut src = Planes::new(sw, sh);
                for (i, v) in src.b.iter_mut().enumerate() {
                    *v = ((i * 7919) % 65_536) as u16;
                }
                let mut dst = Planes::new(w, h);
                for (i, v) in dst.b.iter_mut().enumerate() {
                    *v = ((i * 104_729) % 40_000) as u16;
                }
                let mut want = dst.clone();
                add_upsampled(&mut dst, &src, factor, 0.55);
                plain(&mut want, &src, factor, 141);
                assert!(dst.b == want.b, "{w}×{h} ×{factor}");
            }
        }
    }

    #[test]
    fn one_upsampled_pixel_is_what_the_whole_upsample_adds() {
        let mut src = Planes::new(23, 17);
        for (i, ((r, g), b)) in (src.r.iter_mut())
            .zip(src.g.iter_mut())
            .zip(src.b.iter_mut())
            .enumerate()
        {
            (*r, *g, *b) = (
                ((i * 7919) % 65_536) as u16,
                ((i * 104_729) % 65_536) as u16,
                ((i * 31) % 9000) as u16,
            );
        }
        let mut dst = Planes::new(92, 68);
        add_upsampled(&mut dst, &src, 4, 0.55);
        for y in 0..dst.height {
            for x in 0..dst.width {
                assert_eq!(
                    upsampled_at(&src, 4, 0.55, x, y),
                    dst.at(x, y),
                    "({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn box_means_divide_exactly() {
        // Both sides are non-decreasing in the sum, so agreeing on each side of every step
        // (n = k·w − 1 and k·w, for n = sum + w/2) means agreeing everywhere.
        for w in (1..=MAX_BOX as u32).step_by(2) {
            let mean = BoxMean::new(w);
            let top = 65_535 * w;
            for k in 0..=65_536 {
                for n in [k * w, (k * w).saturating_sub(1)] {
                    let Some(sum) = n.checked_sub(w / 2).filter(|&s| s <= top) else {
                        continue;
                    };
                    let exact = (sum + w / 2) / w;
                    assert_eq!(u32::from(mean.of(sum)), exact, "w {w}, sum {sum}");
                }
            }
        }
    }

    #[test]
    fn column_blocks_blur_like_single_columns() {
        // 70 columns: two full 32-lane blocks and a 6-lane tail.
        let mut p = Planes::new(70, 40);
        for (i, v) in p.r.iter_mut().enumerate() {
            *v = ((i * 7919) % 5000) as u16;
        }
        let mut q = p.clone();
        let mut scratch = Vec::new();
        blur(&mut p, 3.0, &mut scratch);
        // The same blur, one column at a time, through the lanes code with one lane.
        let w = box_width(3.0);
        let mut b = vec![0u16; 70];
        let width = p.width;
        for row in q.r.chunks_exact_mut(width) {
            three_boxes(row, &mut b, w / 2, 1);
            row.copy_from_slice(&b);
        }
        let (mut a, mut b) = (vec![0u16; 40], vec![0u16; 40]);
        for x in 0..70 {
            for (y, v) in a.iter_mut().enumerate() {
                *v = q.r[y * 70 + x];
            }
            three_boxes(&mut a, &mut b, w / 2, 1);
            for (y, &v) in b.iter().enumerate() {
                q.r[y * 70 + x] = v;
            }
        }
        assert!(p.r == q.r);
    }

    /// The σ whose box radius is `r`.
    fn sigma_of(r: usize) -> f32 {
        let s = (((2 * r + 1) * (2 * r + 1) - 1) as f32).sqrt() / 2.0;
        assert_eq!(box_radius(s), r);
        s
    }

    /// Rows lit only where `y % 17 < 6` (so whole row blocks and runs of rows are dark).
    fn banded(w: usize, h: usize) -> Vec<u8> {
        (0..w * h)
            .map(|i| {
                let y = i / w;
                if y % 17 < 6 || y > h / 2 && y < h / 2 + 2 {
                    ((i * 7919) % 256) as u8
                } else {
                    0
                }
            })
            .collect()
    }

    /// `blur_plane` as plainly as it can be put: each row, then each column, through the lanes
    /// code with one lane.
    fn single_lines(p: &[u16], w: usize, h: usize, r: usize) -> Vec<u16> {
        let mut q = p.to_vec();
        let mut b = vec![0u16; w];
        for row in q.chunks_exact_mut(w) {
            three_boxes(row, &mut b, r, 1);
            row.copy_from_slice(&b);
        }
        let (mut a, mut b) = (vec![0u16; h], vec![0u16; h]);
        for x in 0..w {
            for (y, v) in a.iter_mut().enumerate() {
                *v = q[y * w + x];
            }
            three_boxes(&mut a, &mut b, r, 1);
            for (y, &v) in b.iter().enumerate() {
                q[y * w + x] = v;
            }
        }
        q
    }

    /// u16 light blurs its rows as they lie up to radius 4, in blocks of lanes past that (both
    /// sides of the cut): both as single lines do, up to the largest values (the means' bound).
    #[test]
    fn u16_planes_blur_like_single_lines() {
        for (w, h) in [(70, 40), (5, 3), (130, 77), (33, 200)] {
            for r in 1..=6 {
                let lit: Vec<u16> = banded(w, h)
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| if v == 0 { 0 } else { 65_535 - (i % 997) as u16 })
                    .collect();
                let mut p = lit.clone();
                let mut scratch = Vec::new();
                blur_plane(&mut p, w, h, sigma_of(r), &mut scratch);
                assert!(p == single_lines(&lit, w, h, r), "{w}×{h} r {r}");
            }
        }
    }

    /// u8 planes blur as single lines of their values widened do (review T13b M2: not against
    /// the u16 path, which shares `row_boxes` with them up to radius 4).
    #[test]
    fn u8_planes_blur_like_single_lines() {
        for (w, h) in [(70, 40), (5, 3), (130, 77), (33, 200)] {
            for r in 1..=6 {
                let mut p8 = banded(w, h);
                let wide: Vec<u16> = p8.iter().map(|&v| u16::from(v)).collect();
                let mut scratch = Vec::new();
                blur_plane(&mut p8, w, h, sigma_of(r), &mut scratch);
                let want = single_lines(&wide, w, h, r);
                assert!(
                    p8.iter().zip(&want).all(|(&a, &b)| u16::from(a) == b),
                    "{w}×{h} r {r}"
                );
            }
        }
    }

    /// Ruling G7: a scratch left longer by a larger plane is kept as it is (not zeroed again to
    /// the smaller length), and blurs exactly as a fresh one.
    #[test]
    fn a_longer_scratch_is_kept_and_blurs_alike() {
        let mut scratch = Vec::new();
        let mut large = impulse(400, 60_000);
        blur(&mut large, 6.0, &mut scratch);
        let left = scratch.len();
        assert!(left > blur_scratch(50, 50));
        let mut small = Planes::new(50, 50);
        for (i, v) in small.g.iter_mut().enumerate() {
            *v = ((i * 7919) % 5000) as u16;
        }
        let mut fresh = small.clone();
        for sigma in [1.0, 6.0] {
            blur(&mut small, sigma, &mut scratch);
            blur(&mut fresh, sigma, &mut Vec::new());
        }
        assert_eq!(scratch.len(), left);
        assert!(small.g == fresh.g);
    }

    /// Rows of a sparse plane far from its light are left out of the column pass, exactly.
    #[test]
    fn a_sparse_plane_blurs_like_single_lines() {
        // Light from a block's first row, and to a block's last (the rows go in blocks of 32).
        let cases = [
            (70, 150, 60..66),
            (40, 150, 64..70),
            (40, 150, 26..32),
            (40, 90, 0..3),
            (40, 90, 85..90),
        ];
        for (w, h, lit) in cases {
            let mut p = Planes::new(w, h);
            for y in lit.clone() {
                for x in 0..w {
                    p.r[y * w + x] = ((x * 7919 + y) % 5000) as u16;
                }
            }
            let mut q = p.r.clone();
            let mut scratch = Vec::new();
            blur(&mut p, 3.0, &mut scratch);
            let r = box_width(3.0) / 2;
            let mut b = vec![0u16; w];
            for row in q.chunks_exact_mut(w) {
                three_boxes(row, &mut b, r, 1);
                row.copy_from_slice(&b);
            }
            let (mut a, mut b) = (vec![0u16; h], vec![0u16; h]);
            for x in 0..w {
                for (y, v) in a.iter_mut().enumerate() {
                    *v = q[y * w + x];
                }
                three_boxes(&mut a, &mut b, r, 1);
                for (y, &v) in b.iter().enumerate() {
                    q[y * w + x] = v;
                }
            }
            assert!(p.r == q, "{w}×{h} lit {lit:?}");
        }
    }

    #[test]
    fn upsampling_adds_a_bilinear_copy() {
        let mut small = Planes::new(4, 4);
        small.r.fill(1000);
        let mut big = Planes::new(16, 16);
        big.r.fill(10);
        add_upsampled(&mut big, &small, 4, 0.5);
        assert_eq!(big.at(8, 8)[0], 510);
        scale(&mut big, 0.5);
        assert_eq!(big.at(8, 8)[0], 255);
        assert_eq!(small.sample(1.5, 1.5)[0], 1000);
        assert_eq!(small.sample(-1.0, 0.0)[0], 0, "outside is dark");
    }

    #[test]
    fn light_adds_and_saturates() {
        let mut p = Planes::new(8, 8);
        let white = Paint::Solid(Rgb(255, 255, 255).opaque());
        fill(
            &mut p,
            DeviceRect::new(0, 0, 8, 8),
            &[Shape::rect(0.0, 0.0, 8.0, 8.0)],
            &white,
            1.0,
            crate::etch::Blend::Add,
        );
        assert_eq!(p.at(3, 3), [ONE; 3]);
        for _ in 0..20 {
            fill(
                &mut p,
                DeviceRect::new(0, 0, 8, 8),
                &[Shape::disc(pt(4.0, 4.0), 3.0)],
                &white,
                1.0,
                crate::etch::Blend::Add,
            );
        }
        assert_eq!(p.at(4, 4)[0], 65_535, "saturates, never wraps");
        p.reshape(4, 2);
        assert_eq!((p.width, p.height, p.r.len()), (4, 2, 8));
        assert_eq!(p.total(), [0; 3], "reshaping clears");
    }

    /// Ruling G13: a plane with no width or height is left alone (it panicked in
    /// `chunks_exact_mut(0)`).
    #[test]
    fn zero_sized_planes_are_left_alone() {
        let mut half = Planes::new(0, 4);
        downsample(&Planes::new(0, 7), &mut half, 2);
        let mut flat = Planes::new(2, 0);
        downsample(&Planes::new(4, 0), &mut flat, 2);
        let mut dst = Planes::new(5, 5);
        add_upsampled(&mut dst, &Planes::new(0, 3), 2, 0.5);
        add_upsampled(&mut Planes::new(0, 0), &Planes::new(2, 2), 2, 0.5);
        let mut scratch = Vec::new();
        blur_plane::<u16>(&mut [], 0, 9, 3.0, &mut scratch);
        blur_plane::<u8>(&mut [], 9, 0, 3.0, &mut scratch);
        assert_eq!(dst.total(), [0; 3]);
    }
}
