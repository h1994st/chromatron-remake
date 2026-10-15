//! `gouache`: the light theme's light (spec §7.3 "Light", §8 `Over`; rulings L2–L5). The dark
//! theme adds light into u16 planes that glow and bloom; the light theme paints it. There are
//! three premultiplied RGBA8 layers over `base`, each pass covering every beam before the next:
//! a wide faint bloom (¼ resolution, blurred), a narrow glow (½, blurred) and the sharp layer
//! (full: each mask's lines, the computed mix dots at crossings and bounces, the piece lights).
//! The bloom is upsampled under the glow at ½ (`half`, "the halo"), so a compose reads two
//! layers: `out = sharp + (halo + base·(1 − halo.a))·(1 − sharp.a)`. `glow` and `bloom_raw` keep
//! the strokes unblurred, so a frame re-blurs only windows, bit for bit what a whole develop
//! gives (ruling L4). Every buffer is made in `reshape`; nothing here allocates afterwards.

use lumen::{Colour, WHITE};

use crate::DeviceRect;
use crate::beamline::{MAX_PARTIALS, MAX_RUNS};
use crate::bloom::{blur_plane, blur_scratch, box_radius, taps};
use crate::darkroom::{
    BAND, LIGHT_PATCHES, Tap, Texel, WHOLE_PAST, bake_taps, clip_to, coarser, device_of_half,
    device_of_sharp, grow, half_tap, plane_row, tap,
};
use crate::etch::{Blend, Paint, Pt, Shape, Stops, Target, code_value, div255, fill};
use crate::patchwork::{Patchwork, area, hull, is_empty};
use crate::spectrum::{Palette, Rgba};
use crate::{fit_buffer, make_room};

/// A line's change box reach (u): the bloom stroke's half width (L4).
pub(crate) const STROKE_REACH: f32 = 0.135;
/// The wash's reach past the board (u): the bloom's stroke and blur, the lit halo, the prism's
/// fan (L3; the spec's 2σ_b is the dark tile's, C4).
pub(crate) const MARGIN: f32 = 0.5;
/// Sharp px an item's box grows by when a layer chooses what to redraw: the ¼ layer's
/// anti-aliasing ramp is one ¼ px (4 sharp px).
pub(crate) const ITEM_SLACK: i32 = 4;
pub(crate) const CROSSING_DOT: f32 = 0.28;
pub(crate) const BOUNCE_DOT: f32 = 0.16;
const MIX_DOT: f32 = 0.06;
const BLOOM_ALPHA: f32 = 0.14;
const BLOOM_WIDTH: f32 = 0.27;
const BLOOM_SIGMA: f32 = 0.12;
const GLOW_ALPHA: f32 = 0.6;
const GLOW_ALPHA_WHITE: f32 = 0.75;
const GLOW_WIDTH: f32 = 0.12;
const GLOW_SIGMA: f32 = 0.04;
const LINE_WIDTH: f32 = 0.036;
const WHITE_HALO_ALPHA: f32 = 0.9;
const WHITE_HALO_WIDTH: f32 = 0.075;
const WHITE_CORE_WIDTH: f32 = 0.045;

/// Premultiplied RGBA8 planes (one `Vec<u8>` per channel, so loops vectorise).
#[derive(Default)]
pub struct Pigment {
    pub width: usize,
    pub height: usize,
    pub r: Vec<u8>,
    pub g: Vec<u8>,
    pub b: Vec<u8>,
    pub a: Vec<u8>,
}

impl Pigment {
    /// `width × height`, cleared; allocates only as `fit_buffer` does.
    pub fn reshape(&mut self, width: usize, height: usize) {
        (self.width, self.height) = (width, height);
        for p in self.planes_mut() {
            fit_buffer(p, width * height, 0);
        }
    }

    pub fn planes(&self) -> [&[u8]; 4] {
        [&self.r, &self.g, &self.b, &self.a]
    }

    pub fn planes_mut(&mut self) -> [&mut Vec<u8>; 4] {
        [&mut self.r, &mut self.g, &mut self.b, &mut self.a]
    }

    /// Clears `r` (clipped to the planes).
    pub fn wipe(&mut self, r: DeviceRect) {
        let w = self.width;
        let r = clip_to(r, w as i32, self.height as i32);
        if is_empty(r) {
            return;
        }
        let (x0, x1) = (r.left as usize, r.right as usize);
        for p in self.planes_mut() {
            for y in r.top as usize..r.bottom as usize {
                p[y * w + x0..y * w + x1].fill(0);
            }
        }
    }

    /// Copies `o`, of the same size (no allocation).
    pub fn copy_from(&mut self, o: &Pigment) {
        assert!((self.width, self.height) == (o.width, o.height));
        for (d, s) in self.planes_mut().into_iter().zip(o.planes()) {
            d.copy_from_slice(s);
        }
    }

    /// The premultiplied `[r, g, b, a]` at (`x`, `y`) (tests read the planes through it).
    #[cfg(test)]
    pub fn at(&self, x: usize, y: usize) -> [u8; 4] {
        let i = y * self.width + x;
        [self.r[i], self.g[i], self.b[i], self.a[i]]
    }

    pub fn bytes(&self) -> usize {
        [&self.r, &self.g, &self.b, &self.a]
            .iter()
            .map(|p| p.capacity())
            .sum()
    }
}

impl Target for Pigment {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Premultiplied source-over, whatever the blend: the wash only paints (L14(a)).
    fn blend(&mut self, x: usize, y: usize, c: Rgba, _: Blend) {
        let i = y * self.width + x;
        [self.r[i], self.g[i], self.b[i], self.a[i]] =
            pigment_px([self.r[i], self.g[i], self.b[i], self.a[i]], c);
    }

    fn blend_span(&mut self, x0: usize, y: usize, colours: &[Rgba], _: Blend) {
        let row = y * self.width + x0;
        let n = colours.len();
        let (r, g, b, a) = (
            &mut self.r[row..row + n],
            &mut self.g[row..row + n],
            &mut self.b[row..row + n],
            &mut self.a[row..row + n],
        );
        for (i, &c) in colours.iter().enumerate() {
            if c.a > 0.0 {
                [r[i], g[i], b[i], a[i]] = pigment_px([r[i], g[i], b[i], a[i]], c);
            }
        }
    }
}

/// Premultiplied `d` (r, g, b, a) with `c` painted over: `Pigment`'s px.
#[inline(always)]
fn pigment_px(d: [u8; 4], c: Rgba) -> [u8; 4] {
    let a = c.a.clamp(0.0, 1.0);
    let keep = 1.0 - a;
    let mix = |d: u8, s: f32| code_value(s * a * 255.0 + f32::from(d) * keep) as u8;
    [
        mix(d[0], c.r),
        mix(d[1], c.g),
        mix(d[2], c.b),
        mix(d[3], 1.0),
    ]
}

/// A layer being painted that remembers the bounds of every px it paints, so the next whole
/// paint wipes only those.
pub(crate) struct Stroked<'a> {
    pigment: &'a mut Pigment,
    bounds: [usize; 4],
}

impl<'a> Stroked<'a> {
    pub(crate) fn new(pigment: &'a mut Pigment) -> Stroked<'a> {
        Stroked {
            pigment,
            bounds: [usize::MAX, usize::MAX, 0, 0],
        }
    }

    /// What it painted, in sharp px for a layer of `factor` sharp px per px (empty for nothing).
    pub(crate) fn drawn(&self, factor: i32) -> DeviceRect {
        let [x0, y0, x1, y1] = self.bounds;
        if x0 >= x1 {
            return DeviceRect::default();
        }
        let up = |v: usize| v as i32 * factor;
        DeviceRect::new(up(x0), up(y0), up(x1), up(y1))
    }
}

impl Target for Stroked<'_> {
    fn size(&self) -> (usize, usize) {
        self.pigment.size()
    }

    fn blend(&mut self, x: usize, y: usize, c: Rgba, blend: Blend) {
        self.pigment.blend(x, y, c, blend);
        let b = &mut self.bounds;
        (b[0], b[1], b[2], b[3]) = (b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1));
    }

    /// Widens its bounds once, over the px the span paints (none for an all-clear one).
    fn blend_span(&mut self, x0: usize, y: usize, colours: &[Rgba], blend: Blend) {
        let painted = |c: &Rgba| c.a > 0.0;
        let Some(first) = colours.iter().position(painted) else {
            return;
        };
        let last = colours.iter().rposition(painted).unwrap_or(first);
        let (x, x1) = (x0 + first, x0 + last + 1);
        self.pigment.blend_span(x, y, &colours[first..=last], blend);
        let b = &mut self.bounds;
        (b[0], b[1], b[2], b[3]) = (b[0].min(x), b[1].min(y), b[2].max(x1), b[3].max(y + 1));
    }
}

/// Mask `m`'s strokes in the layer `factor` (4 bloom, 2 glow, 1 sharp): (width in u, colour);
/// white's sharp line is a halo stroke, then a white core (spec §7.3 Light).
pub(crate) fn passes(palette: &Palette, m: Colour, factor: i32) -> [Option<(f32, Rgba)>; 2] {
    let halo = palette.halo(m);
    let white = m & WHITE == WHITE;
    match factor {
        4 => [Some((BLOOM_WIDTH, halo.alpha(BLOOM_ALPHA))), None],
        2 => {
            let a = if white { GLOW_ALPHA_WHITE } else { GLOW_ALPHA };
            [Some((GLOW_WIDTH, halo.alpha(a))), None]
        }
        _ if white => [
            Some((WHITE_HALO_WIDTH, halo.alpha(WHITE_HALO_ALPHA))),
            Some((WHITE_CORE_WIDTH, palette.white.opaque())),
        ],
        _ => [Some((LINE_WIDTH, palette.mix(m).opaque())), None],
    }
}

/// A crossing's or bounce's computed mix (spec §7.3 Light): a radial from the mix (white for
/// white) at α 1 to the halo at α 0.55 at 0.28 of `radius` to nothing, then a solid mix dot of
/// r 0.06u (`u` px), none for white.
pub(crate) fn mix_dot(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    centre: Pt,
    radius: f32,
    u: f32,
    mask: Colour,
) {
    let (mix, halo) = (palette.mix(mask), palette.halo(mask));
    let white = mask & WHITE == WHITE;
    let hot = if white { palette.white } else { mix };
    let paint = Paint::Radial {
        centre,
        radius,
        stops: Stops::new(&[
            (0.0, hot.opaque()),
            (0.28, halo.alpha(0.55)),
            (1.0, halo.alpha(0.0)),
        ]),
    };
    fill(
        t,
        clip,
        &[Shape::disc(centre, radius)],
        &paint,
        1.0,
        Blend::Over,
    );
    if !white {
        let dot = Paint::Solid(mix.opaque());
        fill(
            t,
            clip,
            &[Shape::disc(centre, MIX_DOT * u)],
            &dot,
            1.0,
            Blend::Over,
        );
    }
}

/// A fading strand's reach past its axis (u): half its widest stroke, the glow's (ruling L5;
/// ruling S4: the box follows what `strand` draws).
pub(crate) const STRAND_REACH: f32 = GLOW_WIDTH / 2.0;

/// An old strand fading out in `Over` (ruling L5), over the output inside `clip`: mask `mask`'s
/// glow stroke unblurred at half its alpha, then its sharp line(s), all at `k` (the fade), `u`
/// px a cell.
pub(crate) fn strand(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    (a, b): (Pt, Pt),
    mask: Colour,
    u: f32,
    k: f32,
) {
    let glow = passes(palette, mask, 2).into_iter().flatten();
    let line = passes(palette, mask, 1).into_iter().flatten();
    for (n, (w, c)) in glow.chain(line).enumerate() {
        let k = if n == 0 { 0.5 * k } else { k };
        let paint = Paint::Solid(c.with_alpha(c.a * k));
        fill(
            t,
            clip,
            &[Shape::line(a, b, w * u)],
            &paint,
            1.0,
            Blend::Over,
        );
    }
}

/// Spec §8 `Over`, per channel in 255ths: `sharp + (halo + base·(1 − halo.a))·(1 − sharp.a)`;
/// `halo` and `sharp` premultiplied `[r, g, b, a]`.
#[inline(always)]
pub fn paint_over(base: u32, halo: [u32; 4], sharp: [u32; 4]) -> u32 {
    let (kh, ks) = (255 - halo[3].min(255), 255 - sharp[3].min(255));
    let ch = |shift: u32, k: usize| {
        let under = (halo[k] + div255((base >> shift & 0xff) * kh + 127)).min(255);
        (sharp[k] + div255(under * ks + 127)).min(255)
    };
    ch(16, 0) << 16 | ch(8, 1) << 8 | ch(0, 2)
}

/// Pixels composed together: their halo and sharp paint gathered, then laid over `base`.
const RUN: usize = 64;

/// Channel `k` of a run: `[r, g, b, a]` of `RUN` px.
type Lanes = [[u32; RUN]; 4];

/// `paint_over` for a run of `base.len()` (≤ `RUN`) px.
#[inline(always)]
fn over_run<P: Texel>(base: &[u32], halo: &Lanes, sharp: &Lanes, out: &mut [P]) {
    let n = base.len().min(out.len()).min(RUN);
    for i in 0..n {
        let h = [halo[0][i], halo[1][i], halo[2][i], halo[3][i]];
        let s = [sharp[0][i], sharp[1][i], sharp[2][i], sharp[3][i]];
        out[i].put(paint_over(base[i], h, s));
    }
}

/// The bloom upsampled ×2 at ½ columns `x0..` from `p`, one channel of a blended ¼ row padded by
/// a zero column each side: `taps(x, 2, ·)` are ¼ columns m − 1, m at 1 : 3 for x = 2m and m,
/// m + 1 at 3 : 1 for x = 2m + 1 (padded m, m + 1 and m + 1, m + 2), so contiguous, and
/// vectorised; a tap past an edge reads a pad.
fn doubled(p: &[u32], x0: usize, out: &mut [u32]) {
    let skip = (x0 % 2).min(out.len());
    let (first, rest) = out.split_at_mut(skip);
    if let [l] = first {
        let m = x0 / 2;
        *l = (3 * p[m + 1] + p[m + 2] + 8) / 16;
    }
    let k = (x0 + skip) / 2;
    let (pairs, odd) = rest.as_chunks_mut::<2>();
    let n = pairs.len();
    let (r0, r1, r2) = (&p[k..][..n], &p[k + 1..][..n], &p[k + 2..][..n]);
    for (((pair, &a), &b), &c) in pairs.iter_mut().zip(r0).zip(r1).zip(r2) {
        pair[0] = (a + 3 * b + 8) / 16;
        pair[1] = (3 * b + c + 8) / 16;
    }
    if let [l] = odd {
        *l = (p[k + n] + 3 * p[k + n + 1] + 8) / 16;
    }
}

/// The halo taps of the blended padded ½ row `row` for device columns `c0..` of a scale-1
/// wash, in the `half_tap` pattern (so contiguous, and vectorised): as `tap` gives them.
fn halved(row: &[u32], c0: usize, out: &mut [u32]) {
    let halo = |a: u32, b: u32, w: u32| (a * (256 - w) + b * w + 32_768) >> 16;
    let skip = (c0 % 2).min(out.len());
    let (first, rest) = out.split_at_mut(skip);
    if let [l] = first {
        let (a, b, w) = half_tap(c0);
        *l = halo(row[a as usize], row[b as usize], u32::from(w));
    }
    let k = (c0 + skip) / 2;
    let (pairs, odd) = rest.as_chunks_mut::<2>();
    let n = pairs.len();
    let (r0, r1, r2) = (&row[k..][..n], &row[k + 1..][..n], &row[k + 2..][..n]);
    for (((pair, &a), &b), &c) in pairs.iter_mut().zip(r0).zip(r1).zip(r2) {
        pair[0] = halo(a, b, 192);
        pair[1] = halo(b, c, 64);
    }
    if let [l] = odd {
        *l = halo(row[k + n], row[k + n + 1], 192);
    }
}

/// The light theme's light tile: the board grown by `MARGIN`·u, its layers, and the scratch its
/// drawing, windows and compose use.
#[derive(Default)]
pub struct Wash {
    /// Full-resolution extent, clipped to the canvas (empty in the dark theme).
    pub rect: DeviceRect,
    /// Light px per device px: min(1, max_px / its larger side).
    pub scale: f32,
    pub sharp: Pigment,
    /// ½: the glow's strokes, unblurred.
    pub glow: Pigment,
    /// ½: the glow blurred, with the bloom upsampled under it (the halo the compose reads).
    pub half: Pigment,
    /// ¼: the bloom's strokes, unblurred, and blurred.
    pub bloom_raw: Pigment,
    pub bloom: Pigment,
    /// One mask's strokes for one `fill` (L3: a union), room for every run and partial.
    pub shapes: Vec<Shape>,
    /// What the last paints drew into `sharp` (sharp px): a whole repaint clears that.
    pub drawn: DeviceRect,
    /// Four planes of up to `band_cap` u8: the windows' blur band (Task 7).
    pub(crate) band: Vec<u8>,
    band_cap: usize,
    scratch: Vec<u16>,
    /// σ and box radius of the glow (½ px) and the bloom (¼ px).
    sigmas: (f32, f32),
    radii: (usize, usize),
    /// One vertically blended `bloom` row (4 channels of `bloom.width + 2`, padded), for the
    /// bloom upsampled under the glow.
    bloom_row: Vec<u32>,
    /// Per device column / row of `rect`: the two `half` taps and the second tap's weight in
    /// 1/256 (and the same for `sharp` when `scale < 1`).
    halo_cols: Vec<Tap>,
    halo_rows: Vec<Tap>,
    sharp_cols: Vec<Tap>,
    sharp_rows: Vec<Tap>,
    /// One vertically blended `half` row (4 channels of `half.width + 2`, padded), reused for
    /// every compose row.
    row: Vec<u32>,
    /// The same for `sharp`, when `scale < 1`.
    sharp_row: Vec<u32>,
}

impl Wash {
    /// Fits the wash to `rect` (device px) for cells of `u` px; a side over `max_px` renders at
    /// `max_px` / that side. An empty rect gives every plane back (L2).
    pub fn reshape(&mut self, rect: DeviceRect, max_px: f32, u: f32) {
        self.rect = rect;
        let (w, h) = (
            (rect.right - rect.left).max(0) as usize,
            (rect.bottom - rect.top).max(0) as usize,
        );
        let empty = w == 0 || h == 0;
        let s = if empty {
            1.0
        } else {
            (max_px / w.max(h) as f32).min(1.0)
        };
        let side = |v: usize| ((v as f32 * s).ceil() as usize).max(1);
        let (sw, sh) = (side(w), side(h));
        let sigmas = (GLOW_SIGMA * u * s / 2.0, BLOOM_SIGMA * u * s / 4.0);
        let sized = (self.sharp.width, self.sharp.height) == (sw, sh)
            && self.sharp.r.len() == sw * sh
            && (self.halo_cols.len(), self.halo_rows.len()) == (w, h);
        if sized && self.scale == s && self.sigmas == sigmas {
            // Only moved: every plane and tap is kept, and the next paint wipes what is drawn.
            return;
        }
        self.scale = s;
        let (hw, hh) = (sw.div_ceil(2), sh.div_ceil(2));
        let (qw, qh) = (sw.div_ceil(4), sh.div_ceil(4));
        self.sharp.reshape(sw, sh);
        self.glow.reshape(hw, hh);
        self.half.reshape(hw, hh);
        self.bloom_raw.reshape(qw, qh);
        self.bloom.reshape(qw, qh);
        self.sigmas = sigmas;
        self.radii = (box_radius(self.sigmas.0), box_radius(self.sigmas.1));
        let (rg, rb) = self.radii;
        self.band_cap = if empty {
            0
        } else {
            ((BAND + 6 * rg).min(hh) * hw).max((BAND + 6 * rb).min(qh) * qw)
        };
        fit_buffer(&mut self.band, 4 * self.band_cap, 0);
        make_room(
            &mut self.scratch,
            if empty { 0 } else { blur_scratch(hw, hh) },
        );
        make_room(
            &mut self.shapes,
            if empty { 0 } else { MAX_RUNS + MAX_PARTIALS },
        );
        fit_buffer(&mut self.bloom_row, 4 * (qw + 2), 0);
        bake_taps(&mut self.halo_cols, w, s, true, hw);
        // At scale 1 the halo taps are the plain ½ pattern, which the compose reads in pairs.
        debug_assert!(
            s < 1.0 || (self.halo_cols.iter().enumerate()).all(|(c, &tap)| tap == half_tap(c))
        );
        bake_taps(&mut self.halo_rows, h, s, true, hh);
        fit_buffer(&mut self.row, 4 * (hw + 2), 0);
        if s < 1.0 {
            bake_taps(&mut self.sharp_cols, w, s, false, sw);
            bake_taps(&mut self.sharp_rows, h, s, false, sh);
            fit_buffer(&mut self.sharp_row, 4 * (sw + 2), 0);
        } else {
            make_room(&mut self.sharp_cols, 0);
            make_room(&mut self.sharp_rows, 0);
            make_room(&mut self.sharp_row, 0);
        }
        self.drawn = DeviceRect::default();
    }

    /// Clears what a paint inside `d` (sharp px) drew, in each layer at its resolution.
    pub fn wipe(&mut self, d: DeviceRect) {
        self.sharp.wipe(d);
        self.glow.wipe(coarser(d, 2));
        self.bloom_raw.wipe(coarser(d, 4));
    }

    /// The bloom blurred whole, the glow blurred whole, and the bloom put under the glow.
    pub fn develop(&mut self) {
        let Wash {
            bloom,
            bloom_raw,
            half,
            glow,
            scratch,
            sigmas,
            ..
        } = self;
        bloom.copy_from(bloom_raw);
        let (bw, bh) = (bloom.width, bloom.height);
        for p in bloom.planes_mut() {
            blur_plane(p.as_mut_slice(), bw, bh, sigmas.1, scratch);
        }
        half.copy_from(glow);
        let (hw, hh) = (half.width, half.height);
        for p in half.planes_mut() {
            blur_plane(p.as_mut_slice(), hw, hh, sigmas.0, scratch);
        }
        for y in 0..hh {
            self.under(y, (0, hw));
        }
    }

    /// The windows a change inside `dirty` (sharp px) reaches: the bloom's (¼ px: its px grown
    /// by the blur's 3r) and the halo's (½ px: the glow's change grown by 3r, with the bloom
    /// windows ×2 ± 2, which the upsample reads), coalesced.
    fn windows_of(
        &self,
        dirty: &[DeviceRect],
    ) -> (Patchwork<LIGHT_PATCHES>, Patchwork<LIGHT_PATCHES>) {
        let (rg, rb) = (3 * self.radii.0 as i32, 3 * self.radii.1 as i32);
        let (qw, qh) = (self.bloom.width as i32, self.bloom.height as i32);
        let (hw, hh) = (self.half.width as i32, self.half.height as i32);
        let (mut blooms, mut halos) = (Patchwork::new(), Patchwork::new());
        for &d in dirty.iter().filter(|&&d| !is_empty(d)) {
            let wb = clip_to(grow(coarser(d, 4), rb), qw, qh);
            // ½ px x reads ¼ px ⌊(x − 1)/2⌋ and the next, so ¼ px j reaches ½ px 2j − 1 ..= 2j + 2.
            let up = DeviceRect::new(
                2 * wb.left - 2,
                2 * wb.top - 2,
                2 * wb.right + 2,
                2 * wb.bottom + 2,
            );
            let glow = grow(coarser(d, 2), rg);
            let w = if is_empty(wb) { glow } else { hull(glow, up) };
            blooms.add(wb);
            halos.add(clip_to(w, hw, hh));
        }
        blooms.coalesce();
        halos.coalesce();
        (blooms, halos)
    }

    /// `develop` after the layers changed only inside `dirty` (sharp px), bit for bit (ruling
    /// L4): the bloom blurred again in its windows from `bloom_raw`, then the halo blurred again
    /// in its windows from `glow` and laid over the bloom again. Adds the device rects whose
    /// compose that changes: each dirty rect's own (its sharp paint) and each halo window's.
    pub fn develop_patches(&mut self, dirty: &[DeviceRect], device: &mut Patchwork<LIGHT_PATCHES>) {
        if self.band_cap == 0 {
            return;
        }
        let windows = self.windows_of(dirty);
        self.develop_windows(dirty, windows, device);
    }

    /// `develop_patches` in `windows_of(dirty)`, worked out by the caller.
    fn develop_windows(
        &mut self,
        dirty: &[DeviceRect],
        (blooms, halos): (Patchwork<LIGHT_PATCHES>, Patchwork<LIGHT_PATCHES>),
        device: &mut Patchwork<LIGHT_PATCHES>,
    ) {
        let (rg, rb) = self.radii;
        for &w in blooms.as_slice() {
            let Wash {
                bloom_raw,
                bloom,
                band,
                band_cap,
                scratch,
                sigmas,
                ..
            } = self;
            blur_window(bloom_raw, bloom, w, sigmas.1, rb, band, *band_cap, scratch);
        }
        for &w in halos.as_slice() {
            let Wash {
                glow,
                half,
                band,
                band_cap,
                scratch,
                sigmas,
                ..
            } = self;
            blur_window(glow, half, w, sigmas.0, rg, band, *band_cap, scratch);
            for y in w.top as usize..w.bottom as usize {
                self.under(y, (w.left as usize, w.right as usize));
            }
        }
        self.report(dirty, &halos, device);
    }

    /// Adds to `device` the device rects whose compose a change inside `dirty` (sharp px)
    /// changes, developed in the halo windows `halos` (½ px): each dirty rect's own and each
    /// window's.
    fn report(
        &self,
        dirty: &[DeviceRect],
        halos: &Patchwork<LIGHT_PATCHES>,
        device: &mut Patchwork<LIGHT_PATCHES>,
    ) {
        for &d in dirty.iter().filter(|&&d| !is_empty(d)) {
            device.add(device_of_sharp(self.rect, self.scale, d));
        }
        for &w in halos.as_slice() {
            device.add(device_of_half(self.rect, self.scale, w));
        }
    }

    /// The cheaper of two equal develops (as the dark tile's `develop_changed`): by windows, or
    /// whole once the halo windows cover more than 1/`WHOLE_PAST` of `half`. The light is the
    /// same either way, so either reports only the rects it changed in (ruling G7).
    pub fn develop_changed(&mut self, dirty: &[DeviceRect], device: &mut Patchwork<LIGHT_PATCHES>) {
        let windows = self.windows_of(dirty);
        let covered: i64 = windows.1.as_slice().iter().map(|&w| area(w)).sum();
        let whole = (self.half.width * self.half.height) as i64;
        if covered * WHOLE_PAST > whole {
            self.develop();
            self.report(dirty, &windows.1, device);
        } else if self.band_cap != 0 {
            self.develop_windows(dirty, windows, device);
        }
    }

    /// `half` row `y`, columns `x0..x1` (½ px): the blurred glow there over the bloom
    /// upsampled there (`taps(·, 2, ·)` on both axes), `g + b·(255 − g.a)/255`. The bloom's two
    /// rows are blended into `bloom_row` once (padded, over the columns these read), then each
    /// run of px takes its two columns of that in the ×2 pattern (`doubled`).
    fn under(&mut self, y: usize, (x0, x1): (usize, usize)) {
        let Wash {
            bloom,
            half,
            bloom_row,
            ..
        } = self;
        let (qw, hw) = (bloom.width, half.width);
        let x1 = x1.min(hw);
        if x0 >= x1 {
            return;
        }
        let stride = qw + 2;
        // ½ px x reads ¼ px ⌊(x − 1)/2⌋ and the next: padded columns x0/2 ..= (x1 − 1)/2 + 2,
        // of which the real ones (1..=qw) are blended and the two pads stay 0.
        let (lo, hi) = ((x0 / 2).max(1), ((x1 - 1) / 2 + 2).min(qw));
        let [(ra, wa), (rb, wb)] = taps(y, 2, bloom.height);
        let mut lit = 0;
        for (k, p) in bloom.planes().into_iter().enumerate() {
            let row = &mut bloom_row[k * stride..][..stride];
            (row[0], row[qw + 1]) = (0, 0);
            if lo > hi {
                continue;
            }
            let (a, b) = (&p[ra * qw..][lo - 1..hi], &p[rb * qw..][lo - 1..hi]);
            for ((d, &u), &v) in row[lo..=hi].iter_mut().zip(a).zip(b) {
                *d = u32::from(u) * wa + u32::from(v) * wb;
                lit |= *d;
            }
        }
        // No bloom over these columns: `half` keeps its glow (premultiplied, so a bloom with
        // no alpha has no colour either).
        if lit == 0 {
            return;
        }
        let o = y * hw;
        let mut up = [[0u32; RUN]; 4];
        let mut x = x0;
        while x < x1 {
            let n = RUN.min(x1 - x);
            for (k, up) in up.iter_mut().enumerate() {
                doubled(&bloom_row[k * stride..][..stride], x, &mut up[..n]);
            }
            let [hr, hg, hb, ha] = half.planes_mut();
            let (hr, hg, hb, ha) = (
                &mut hr[o + x..][..n],
                &mut hg[o + x..][..n],
                &mut hb[o + x..][..n],
                &mut ha[o + x..][..n],
            );
            for i in 0..n {
                let keep = 255 - u32::from(ha[i]);
                let lay = |h: u8, b: u32| (u32::from(h) + div255(b * keep + 127)).min(255) as u8;
                hr[i] = lay(hr[i], up[0][i]);
                hg[i] = lay(hg[i], up[1][i]);
                hb[i] = lay(hb[i], up[2][i]);
                ha[i] = lay(ha[i], up[3][i]);
            }
            x += n;
        }
    }

    /// Composes wash row `ly`, columns `c0..c1` (wash-relative), over `base` into `out` (§8).
    pub fn compose_span<P: Texel>(
        &mut self,
        ly: usize,
        (c0, c1): (usize, usize),
        base: &[u32],
        out: &mut [P],
    ) {
        let cols = &self.halo_cols[c0..c1];
        let (lo, hi) = (cols[0].0, cols[cols.len() - 1].1);
        blend_pigment_rows(&self.half, self.halo_rows[ly], lo, hi, &mut self.row);
        let hs = self.half.width + 2;
        let halos: [&[u32]; 4] = std::array::from_fn(|k| &self.row[k * hs..][..hs]);
        let (mut halo, mut sharp) = ([[0u32; RUN]; 4], [[0u32; RUN]; 4]);
        let runs = cols
            .chunks(RUN)
            .zip(base.chunks(RUN))
            .zip(out.chunks_mut(RUN));
        if self.scale >= 1.0 {
            let at = ly * self.sharp.width;
            let planes = self.sharp.planes();
            for (j, ((cols, base), out)) in runs.enumerate() {
                let (c, n) = (c0 + j * RUN, base.len());
                // No paint over the run (no halo alpha at its taps, no sharp alpha): `base`,
                // exactly (`paint_over(b, 0, 0) == b`). Every layer is premultiplied with each
                // channel ≤ its alpha (`Pigment::blend` paints channels ≤ 1, the box means and
                // `under` keep the order), so no alpha means no colour either.
                let (lo, hi) = (cols[0].0 as usize, cols[n - 1].1 as usize);
                let bare = |p: &[u8]| p.iter().all(|&a| a == 0);
                if halos[3][lo..=hi].iter().all(|&a| a == 0) && bare(&planes[3][at + c..][..n]) {
                    for (o, &b) in out.iter_mut().zip(base) {
                        o.put(b);
                    }
                    continue;
                }
                for k in 0..4 {
                    halved(halos[k], c, &mut halo[k][..n]);
                    for (s, &v) in sharp[k][..n].iter_mut().zip(&planes[k][at + c..][..n]) {
                        *s = u32::from(v);
                    }
                }
                over_run(base, &halo, &sharp, out);
            }
        } else {
            let scols = &self.sharp_cols[c0..c1];
            let (lo, hi) = (scols[0].0, scols[scols.len() - 1].1);
            blend_pigment_rows(
                &self.sharp,
                self.sharp_rows[ly],
                lo,
                hi,
                &mut self.sharp_row,
            );
            let ss = self.sharp.width + 2;
            let sharps: [&[u32]; 4] = std::array::from_fn(|k| &self.sharp_row[k * ss..][..ss]);
            for (((cols, base), out), scols) in runs.zip(scols.chunks(RUN)) {
                let n = base.len();
                for k in 0..4 {
                    for (h, &t) in halo[k][..n].iter_mut().zip(cols) {
                        *h = tap(halos[k], t);
                    }
                    for (s, &t) in sharp[k][..n].iter_mut().zip(scols) {
                        *s = tap(sharps[k], t);
                    }
                }
                over_run(base, &halo, &sharp, out);
            }
        }
    }

    pub fn bytes(&self) -> usize {
        let taps = self.halo_cols.capacity()
            + self.halo_rows.capacity()
            + self.sharp_cols.capacity()
            + self.sharp_rows.capacity();
        [
            &self.sharp,
            &self.glow,
            &self.half,
            &self.bloom_raw,
            &self.bloom,
        ]
        .iter()
        .map(|p| p.bytes())
        .sum::<usize>()
            + self.band.capacity()
            + self.scratch.capacity() * size_of::<u16>()
            + self.shapes.capacity() * size_of::<Shape>()
            + taps * size_of::<Tap>()
            + (self.row.capacity() + self.sharp_row.capacity() + self.bloom_row.capacity())
                * size_of::<u32>()
    }
}

/// `dst` inside window `w` becomes `src` blurred by σ (box radius `r`), in bands of `BAND` rows:
/// each band copied from `src` over the band grown by 3r (so its own edges never reach the
/// window), blurred, and its window rows written back. The same zero padding at the planes'
/// edges as a whole blur, so the same values.
#[allow(clippy::too_many_arguments)]
fn blur_window(
    src: &Pigment,
    dst: &mut Pigment,
    w: DeviceRect,
    sigma: f32,
    r: usize,
    band: &mut [u8],
    cap: usize,
    scratch: &mut Vec<u16>,
) {
    let reach = 3 * r as i32;
    let (sw, sh) = (src.width, src.height);
    let ww = (w.right - w.left) as usize;
    let mut y0 = w.top;
    while y0 < w.bottom {
        let y1 = (y0 + BAND as i32).min(w.bottom);
        let s = clip_to(
            DeviceRect::new(w.left - reach, y0 - reach, w.right + reach, y1 + reach),
            sw as i32,
            sh as i32,
        );
        let (bw, bh) = ((s.right - s.left) as usize, (s.bottom - s.top) as usize);
        // Plain asserts: no panic messages in the shipped code (the page's size cap).
        assert!(bw * bh <= cap);
        for (k, (from, to)) in src.planes().into_iter().zip(dst.planes_mut()).enumerate() {
            let b = &mut band[k * cap..][..bw * bh];
            for (row, line) in b.chunks_exact_mut(bw).enumerate() {
                line.copy_from_slice(&from[(s.top as usize + row) * sw + s.left as usize..][..bw]);
            }
            blur_plane(b, bw, bh, sigma, scratch);
            for y in y0..y1 {
                let at = (y - s.top) as usize * bw + (w.left - s.left) as usize;
                to[y as usize * sw + w.left as usize..][..ww].copy_from_slice(&b[at..][..ww]);
            }
        }
        y0 = y1;
    }
}

/// `blend_rows` for four u8 planes: `p`'s rows `tap` blended vertically into `row` (four
/// channels of `width + 2` padded columns), over padded columns `lo..=hi` only.
fn blend_pigment_rows(p: &Pigment, (ra, rb, w): Tap, lo: u32, hi: u32, row: &mut [u32]) {
    let (width, height) = (p.width, p.height);
    let stride = width + 2;
    let (lo, hi) = ((lo as usize).max(1), (hi as usize).min(width));
    if lo > hi {
        return;
    }
    let (wa, wb) = (256 - u32::from(w), u32::from(w));
    for (c, plane) in p.planes().into_iter().enumerate() {
        let dst = &mut row[c * stride + lo..=c * stride + hi];
        let line = |q| plane_row(plane, width, height, q, lo, hi);
        match (line(ra), line(rb)) {
            (Some(a), Some(b)) => {
                for ((d, &x), &y) in dst.iter_mut().zip(a).zip(b) {
                    *d = u32::from(x) * wa + u32::from(y) * wb;
                }
            }
            (Some(a), None) => {
                for (d, &x) in dst.iter_mut().zip(a) {
                    *d = u32::from(x) * wa;
                }
            }
            (None, Some(b)) => {
                for (d, &y) in dst.iter_mut().zip(b) {
                    *d = u32::from(y) * wb;
                }
            }
            (None, None) => dst.fill(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etch::{Layer, Paint, Shape, fill, pt};
    use crate::spectrum::Rgb;
    use lumen::RED;

    /// Ruling G4: a span paints the wash's planes exactly as its px one by one, zero alphas
    /// untouched, in every blend; through `Stroked` too, whose `drawn` box is the same.
    #[test]
    fn spans_blend_like_single_px_in_the_wash() {
        let fresh = || {
            let mut p = Pigment::default();
            p.reshape(200, 3);
            for (k, plane) in p.planes_mut().into_iter().enumerate() {
                for (i, v) in plane.iter_mut().enumerate() {
                    *v = ((i * 37 + k * 71) % 256) as u8;
                }
            }
            p
        };
        for blend in [Blend::Over, Blend::Add, Blend::Multiply] {
            let colours = crate::etch::span_colours(150, 17);
            let start = fresh();
            let (mut one, mut span) = (fresh(), fresh());
            for (i, &c) in colours.iter().enumerate() {
                if c.a > 0.0 {
                    one.blend(30 + i, 1, c, blend);
                }
            }
            span.blend_span(30, 1, &colours, blend);
            assert!(one.planes() == span.planes(), "Pigment {blend:?}");
            assert!(one.planes() != start.planes(), "Pigment {blend:?} drew");

            // Stroked: the same px and the same box. Clear ends, so the box is the painted px'.
            let mut colours = colours;
            colours[..3].fill(Rgba::CLEAR);
            colours[147..].fill(Rgba::CLEAR);
            let (mut one, mut span) = (fresh(), fresh());
            let mut a = Stroked::new(&mut one);
            for (i, &c) in colours.iter().enumerate() {
                if c.a > 0.0 {
                    a.blend(30 + i, 1, c, blend);
                }
            }
            let mut b = Stroked::new(&mut span);
            b.blend_span(30, 1, &colours, blend);
            b.blend_span(10, 2, &[Rgba::CLEAR; 5], blend);
            assert_eq!(a.drawn(2), b.drawn(2), "Stroked {blend:?}");
            assert_eq!(a.drawn(1), DeviceRect::new(33, 1, 177, 2));
            assert!(one.planes() == span.planes(), "Stroked {blend:?}");
        }
    }

    #[test]
    fn paint_over_is_sharp_over_halo_over_base() {
        // base grey 100; halo red 50 at α 128 (premultiplied); no sharp.
        let base = 0x0064_6464;
        assert_eq!(paint_over(base, [50, 0, 0, 128], [0; 4]), 0x0064_3232);
        // An opaque sharp hides both.
        assert_eq!(
            paint_over(base, [50, 0, 0, 128], [255, 0, 0, 255]),
            0x00ff_0000
        );
        // Nothing over the base: the base.
        assert_eq!(paint_over(base, [0; 4], [0; 4]), base);
    }

    #[test]
    fn pigment_paints_premultiplied_over() {
        let mut p = Pigment::default();
        p.reshape(1, 1);
        p.blend(0, 0, Rgb(255, 255, 255).alpha(0.5), Blend::Over);
        assert_eq!(p.at(0, 0), [128, 128, 128, 128]);
        p.blend(0, 0, Rgb(255, 0, 0).alpha(0.5), Blend::Add); // every blend paints over (L14(a))
        // 255·0.5 + 128·0.5 = 191.5 exactly, which rounds to 192.
        assert_eq!(p.at(0, 0), [192, 64, 64, 192]);
    }

    /// Ruling L3: a mask's strokes in one pass are one union, so where two meet (a bend, a run
    /// joining a partial) the paint is as strong as in one stroke, not stronger.
    #[test]
    fn a_masks_strokes_join_without_a_bead() {
        let mut p = Pigment::default();
        p.reshape(40, 20);
        let strokes = [
            Shape::line(pt(2.0, 10.0), pt(20.0, 10.0), 4.0),
            Shape::line(pt(20.0, 10.0), pt(38.0, 10.0), 4.0),
        ];
        let all = DeviceRect::new(0, 0, 40, 20);
        fill(
            &mut p,
            all,
            &strokes,
            &Paint::Solid(Rgb(255, 0, 0).alpha(0.6)),
            1.0,
            Blend::Over,
        );
        assert_eq!(p.at(20, 10)[3], p.at(10, 10)[3], "the joint");
        assert_eq!(p.at(10, 10)[3], 153);
    }

    #[test]
    fn white_takes_a_halo_stroke_then_a_white_core() {
        let pal = &Palette::LIGHT;
        let [a, b] = passes(pal, WHITE, 1);
        assert_eq!(a, Some((0.075, Rgb(127, 147, 207).alpha(0.9))));
        assert_eq!(b, Some((0.045, Rgb(255, 255, 255).opaque())));
        assert_eq!(
            passes(pal, RED, 1),
            [Some((0.036, pal.mix(RED).opaque())), None]
        );
        assert_eq!(
            passes(pal, RED, 2),
            [Some((0.12, pal.mix(RED).alpha(0.6))), None]
        );
        assert_eq!(
            passes(pal, WHITE, 2),
            [Some((0.12, pal.halo(WHITE).alpha(0.75))), None]
        );
        assert_eq!(
            passes(pal, RED, 4),
            [Some((0.27, pal.mix(RED).alpha(0.14))), None]
        );
    }

    #[test]
    fn a_strands_reach_is_its_widest_strokes_half_width() {
        // Ruling S4: the fading strand's box grows by half the widest stroke `strand` draws.
        let pal = &Palette::LIGHT;
        let mut widest: f32 = 0.0;
        for mask in 1..=WHITE {
            for f in [2, 1] {
                for (w, _) in passes(pal, mask, f).into_iter().flatten() {
                    assert!(w / 2.0 <= STRAND_REACH, "mask {mask} layer {f}: {w}");
                    widest = widest.max(w / 2.0);
                }
            }
        }
        assert_eq!(widest, STRAND_REACH);
    }

    #[test]
    fn a_fading_strand_is_its_glow_at_half_alpha_under_its_line() {
        // Ruling L5: a red strand across a white layer, u = 100 px, along the row boundary at
        // y = 50: its line (3.6 px wide) on the axis, the glow's 10 px at α 0.3 beside it,
        // nothing past; at k = 0.5, each at half that.
        let (w, h) = (200, 100);
        let ends = (pt(20.0, 50.0), pt(180.0, 50.0));
        let pal = &Palette::LIGHT;
        let paint = |k: f32| {
            let mut px = vec![0x00ff_ffffu32; w * h];
            let mut layer = Layer {
                px: &mut px,
                width: w,
                height: h,
            };
            let all = DeviceRect::new(0, 0, w as i32, h as i32);
            strand(&mut layer, all, pal, ends, RED, 100.0, k);
            px
        };
        let rgb = |p: u32| [p >> 16 & 0xff, p >> 8 & 0xff, p & 0xff];
        let over = |a: f32, under: [f32; 3]| {
            let red = pal.mix(RED);
            let c = [red.0, red.1, red.2].map(f32::from);
            [0, 1, 2].map(|i| (c[i] * a + under[i] * (1.0 - a)).round() as u32)
        };
        let full = paint(1.0);
        // The line is the mix, opaque (`passes` at factor 1), so a retune moves it too.
        let line = passes(pal, RED, 1)[0].expect("a line").1;
        let line = [line.r, line.g, line.b].map(|v| (v * 255.0).round() as u32);
        assert_eq!(rgb(full[49 * w + 100]), line, "the line");
        assert_eq!(rgb(full[46 * w + 100]), over(0.3, [255.0; 3]), "the glow");
        assert_eq!(rgb(full[42 * w + 100]), [255; 3], "nothing past the glow");
        let half = paint(0.5);
        let glow = over(0.15, [255.0; 3]).map(|c| c as f32);
        assert_eq!(rgb(half[49 * w + 100]), over(0.5, glow), "the line at k");
    }

    /// Fills `p` with premultiplied paint (each channel ≤ its alpha) on the columns
    /// `x % 23 < 15` and rows `y % 19 < 12`, nothing elsewhere (so whole rows are bare); when
    /// `sparse`, only in its 6 × 6 px at (5, 5), so most of each row is bare too.
    fn speckle(p: &mut Pigment, seed: u32, sparse: bool) {
        let mut s = seed;
        let mut next = || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            s >> 24
        };
        for y in 0..p.height {
            for x in 0..p.width {
                let painted = if sparse {
                    (5..11).contains(&x) && (5..11).contains(&y)
                } else {
                    x % 23 < 15 && y % 19 < 12
                };
                if !painted {
                    continue;
                }
                let i = y * p.width + x;
                let a = next();
                p.a[i] = a as u8;
                for c in [&mut p.r, &mut p.g, &mut p.b] {
                    c[i] = (next() % (a + 1)) as u8;
                }
            }
        }
    }

    /// A wash of `w × h` device px at `max_px` (scale below 1 when `max_px` < the larger side),
    /// its strokes speckled and developed.
    fn developed(w: i32, h: i32, max_px: f32, sparse: bool) -> Wash {
        let mut wash = Wash::default();
        wash.reshape(DeviceRect::new(3, 5, 3 + w, 5 + h), max_px, 40.0);
        speckle(&mut wash.glow, 7, sparse);
        speckle(&mut wash.bloom_raw, 11, sparse);
        speckle(&mut wash.sharp, 13, sparse);
        wash.develop();
        wash
    }

    /// `half` as the brief defines it, per px: the glow blurred, then the blurred bloom's
    /// `taps(·, 2, ·)` on both axes laid under it; and the blurred glow alone.
    fn plain_half(wash: &Wash) -> (Pigment, Pigment) {
        let blurred = |p: &Pigment, sigma: f32| {
            let mut q = Pigment::default();
            q.reshape(p.width, p.height);
            q.copy_from(p);
            let (w, h) = (q.width, q.height);
            let mut scratch = Vec::new();
            for plane in q.planes_mut() {
                blur_plane(plane.as_mut_slice(), w, h, sigma, &mut scratch);
            }
            q
        };
        let (glow, bloom) = (
            blurred(&wash.glow, wash.sigmas.0),
            blurred(&wash.bloom_raw, wash.sigmas.1),
        );
        let mut half = blurred(&wash.glow, wash.sigmas.0);
        let (gw, qw, qh) = (glow.width, bloom.width, bloom.height);
        for y in 0..glow.height {
            let [(ra, wa), (rb, wb)] = taps(y, 2, qh);
            for x in 0..gw {
                let [(i0, w0), (i1, w1)] = taps(x, 2, qw);
                let o = y * gw + x;
                let keep = 255 - u32::from(glow.a[o]);
                let planes = half.planes_mut();
                for ((h, g), b) in planes.into_iter().zip(glow.planes()).zip(bloom.planes()) {
                    let col = |j| u32::from(b[ra * qw + j]) * wa + u32::from(b[rb * qw + j]) * wb;
                    let up = (col(i0) * w0 + col(i1) * w1 + 8) / 16;
                    h[o] = (u32::from(g[o]) + (up * keep + 127) / 255).min(255) as u8;
                }
            }
        }
        (half, glow)
    }

    /// The padded bilinear tap (`taps`: rows `rows`, columns `cols`) of `p`, as plainly as it
    /// is defined: zero outside, rows blended in 1/256, then columns, rounded once.
    fn plain_tap(p: &[u8], width: usize, height: usize, rows: Tap, cols: Tap) -> u32 {
        let v = |r: u32, c: u32| {
            let (r, c) = (r as usize, c as usize);
            if (1..=height).contains(&r) && (1..=width).contains(&c) {
                u32::from(p[(r - 1) * width + c - 1])
            } else {
                0
            }
        };
        let (ra, rb, wr) = (rows.0, rows.1, u32::from(rows.2));
        let row = |c: u32| v(ra, c) * (256 - wr) + v(rb, c) * wr;
        let (ca, cb, wc) = (cols.0, cols.1, u32::from(cols.2));
        (row(ca) * (256 - wc) + row(cb) * wc + 32_768) >> 16
    }

    /// `develop` lays the bloom under the glow as the per-px definition does: the glow blurred,
    /// then at each ½ px the blurred bloom's `taps(·, 2, ·)` on both axes, over it.
    #[test]
    fn the_bloom_lies_under_the_glow_as_plain_taps_give_it() {
        for (w, h) in [(146, 82), (151, 77), (9, 6), (130, 130)] {
            let wash = developed(w, h, f32::INFINITY, false);
            let (want, _) = plain_half(&wash);
            assert!(wash.half.planes() == want.planes(), "{w}×{h}");
        }
    }

    /// `under` over any span of a row (odd starts, single px, a span past the row's end) lays
    /// the same bloom as the whole row does (Task 7's windows call it by spans).
    #[test]
    fn the_bloom_lies_under_the_glow_by_any_span() {
        for (w, h) in [(146, 82), (151, 77), (9, 6)] {
            let mut wash = developed(w, h, f32::INFINITY, false);
            let (want, glow) = plain_half(&wash);
            wash.half.copy_from(&glow);
            let hw = wash.half.width;
            let cuts = [0, 1, 6, 7, 70, 71, hw + 5];
            for y in 0..wash.half.height {
                for pair in cuts.windows(2) {
                    wash.under(y, (pair[0].min(hw), pair[1]));
                }
            }
            assert!(wash.half.planes() == want.planes(), "{w}×{h}");
        }
    }

    /// `compose_span`, by any span, is `paint_over` of the plain halo and sharp taps (the sharp
    /// px itself at scale 1), at scale 1 and below it, and on a sparse wash whose runs are
    /// mostly bare (`base` as it is).
    #[test]
    fn composed_spans_are_plain_taps_painted_over_the_base() {
        for (w, h, max_px, sparse) in [
            (146, 82, f32::INFINITY, false),
            (151, 77, f32::INFINITY, false),
            (151, 77, 100.0, false),
            (200, 40, f32::INFINITY, true),
        ] {
            let mut wash = developed(w, h, max_px, sparse);
            let (w, h) = (w as usize, h as usize);
            let base: Vec<u32> = (0..w as u32)
                .map(|i| i.wrapping_mul(0x9e37_79b9) >> 8)
                .collect();
            let mut out = vec![0u32; w];
            for ly in 0..h {
                for (c0, c1) in [(0, w), (1, w - 1), (3, 70), (64, 129), (w - 1, w)] {
                    out.fill(0);
                    wash.compose_span(ly, (c0, c1), &base[c0..c1], &mut out[c0..c1]);
                    for c in c0..c1 {
                        let (half, sharp) = (&wash.half, &wash.sharp);
                        let (hr, hc) = (wash.halo_rows[ly], wash.halo_cols[c]);
                        let hp = |p: &[u8]| plain_tap(p, half.width, half.height, hr, hc);
                        let s = if wash.scale >= 1.0 {
                            sharp.planes().map(|p| u32::from(p[ly * sharp.width + c]))
                        } else {
                            let (sr, sc) = (wash.sharp_rows[ly], wash.sharp_cols[c]);
                            sharp
                                .planes()
                                .map(|p| plain_tap(p, sharp.width, sharp.height, sr, sc))
                        };
                        let want = paint_over(base[c], half.planes().map(hp), s);
                        assert_eq!(out[c], want, "{w}×{h} @{max_px}: ({c}, {ly}) of {c0}..{c1}");
                    }
                }
            }
        }
    }

    /// Paints new premultiplied paint (or none, for one px in five) over `r` of `p`, clipped.
    fn repaint(p: &mut Pigment, r: DeviceRect, seed: &mut u32) {
        let r = clip_to(r, p.width as i32, p.height as i32);
        let mut next = || {
            *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *seed >> 24
        };
        for y in r.top.max(0) as usize..r.bottom.max(0) as usize {
            for x in r.left.max(0) as usize..r.right.max(0) as usize {
                let i = y * p.width + x;
                let a = if next() % 5 == 0 { 0 } else { next() };
                p.a[i] = a as u8;
                for c in [&mut p.r, &mut p.g, &mut p.b] {
                    c[i] = (next() % (a + 1)) as u8;
                }
            }
        }
    }

    /// Ruling L4, at the wash: after its layers change only inside some rects (sharp px; any
    /// size, at the corners and edges, past the planes, overlapping; more than `LIGHT_PATCHES`
    /// of them, so the windows merge), `develop_patches` gives the bloom and the halo of a whole
    /// `develop`, byte for byte, at box radii 1 to 7 and at scale 1 and below it.
    #[test]
    fn patched_develops_equal_whole_ones() {
        let mut seed = 0x5eed_u32;
        let mut below = |n: i32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as i32 % n
        };
        let mut paint = 17u32;
        let (mut radii, mut merged) = ((usize::MAX, 0), false);
        for (w, h, max_px, u) in [
            (146, 82, f32::INFINITY, 100.0),
            (151, 77, f32::INFINITY, 240.0),
            (151, 77, 100.0, 240.0),
            (9, 6, f32::INFINITY, 100.0),
            (300, 41, f32::INFINITY, 60.0),
            (90, 260, f32::INFINITY, 160.0),
        ] {
            for round in 0..10 {
                let rect = DeviceRect::new(3, 5, 3 + w, 5 + h);
                let mut wash = Wash::default();
                wash.reshape(rect, max_px, u);
                assert!(wash.radii.0 >= 1 && wash.radii.1 >= 1, "{w}×{h} at u {u}");
                radii.0 = radii.0.min(wash.radii.0.min(wash.radii.1));
                radii.1 = radii.1.max(wash.radii.0.max(wash.radii.1));
                speckle(&mut wash.glow, 7 + round, false);
                speckle(&mut wash.bloom_raw, 11 + round, false);
                wash.develop();
                let (sw, sh) = (wash.sharp.width as i32, wash.sharp.height as i32);
                let mut dirty: Vec<DeviceRect> = Vec::new();
                // Round 8: 17 to 24 small scattered rects (past LIGHT_PATCHES); round 9: two,
                // the second inside the first's reach.
                let count = match round {
                    8 => LIGHT_PATCHES as i32 + 1 + below(8),
                    9 => 2,
                    _ => 1 + below(3),
                };
                for k in 0..count {
                    let (x, y) = (below(sw + 6) - 3, below(sh + 6) - 3);
                    let (dw, dh) = (1 + below(sw / 3 + 1), 1 + below(sh / 3 + 1));
                    dirty.push(match round {
                        0 => DeviceRect::new(0, 0, 1, 1),
                        1 => DeviceRect::new(sw - 1, sh - 1, sw + 4, sh + 4),
                        8 => DeviceRect::new(x, y, x + 1 + below(3), y + 1 + below(3)),
                        9 if k == 1 => {
                            let f = dirty[0];
                            DeviceRect::new(f.left + 1, f.top + 1, f.right + 2, f.bottom + 2)
                        }
                        _ => DeviceRect::new(x, y, x + dw, y + dh),
                    });
                }
                if round == 8 {
                    let (blooms, halos) = wash.windows_of(&dirty);
                    let most = blooms.as_slice().len().max(halos.as_slice().len());
                    merged |= most < dirty.len();
                }
                for &d in &dirty {
                    repaint(&mut wash.glow, coarser(d, 2), &mut paint);
                    repaint(&mut wash.bloom_raw, coarser(d, 4), &mut paint);
                }
                let mut device = Patchwork::new();
                wash.develop_patches(&dirty, &mut device);
                let mut whole = Wash::default();
                whole.reshape(rect, max_px, u);
                whole.glow.copy_from(&wash.glow);
                whole.bloom_raw.copy_from(&wash.bloom_raw);
                whole.develop();
                let what = format!("{w}×{h} @{max_px} u {u}, round {round}: {dirty:?}");
                assert!(wash.bloom.planes() == whole.bloom.planes(), "{what}: bloom");
                assert!(wash.half.planes() == whole.half.planes(), "{what}: halo");
                assert!(!device.is_empty(), "{what}: damage");
            }
        }
        assert_eq!(radii, (1, 7), "the box radii covered");
        assert!(merged, "more rects than windows: the windows merged");
    }

    /// Ruling G7, at the wash: a change past 1/`WHOLE_PAST` of it develops whole, but reports
    /// only the rects its light changed in, not the whole wash.
    #[test]
    fn a_whole_wash_develop_reports_only_what_changed() {
        let mut wash = developed(400, 400, f32::INFINITY, false);
        let mut before = Pigment::default();
        before.reshape(wash.half.width, wash.half.height);
        before.copy_from(&wash.half);
        let dirty = [DeviceRect::new(60, 60, 340, 340)];
        let (_, halos) = wash.windows_of(&dirty);
        let covered: i64 = halos.as_slice().iter().map(|&w| area(w)).sum();
        let whole = (wash.half.width * wash.half.height) as i64;
        assert!(covered * WHOLE_PAST > whole, "the whole path");
        let mut paint = 29u32;
        repaint(&mut wash.glow, coarser(dirty[0], 2), &mut paint);
        repaint(&mut wash.bloom_raw, coarser(dirty[0], 4), &mut paint);
        let mut device = Patchwork::new();
        wash.develop_changed(&dirty, &mut device);
        assert!(!device.as_slice().contains(&wash.rect), "{device:?}");
        let (hw, mut changed) = (wash.half.width, 0);
        for y in 0..wash.half.height {
            for x in 0..hw {
                let px = |p: &Pigment| p.planes().map(|c| c[y * hw + x]);
                if px(&wash.half) != px(&before) {
                    changed += 1;
                    let (x, y) = (x as i32, y as i32);
                    let one = DeviceRect::new(x, y, x + 1, y + 1);
                    let d = device_of_half(wash.rect, wash.scale, one);
                    let inside = device.as_slice().iter().any(|&r| hull(r, d) == r);
                    assert!(inside, "½ px ({x}, {y}) changed outside the reports");
                }
            }
        }
        assert!(changed > 0, "the light changed");
    }

    /// A resize that only moves the wash (the same size, scale and σ) takes `reshape`'s early
    /// return: the planes keep their paint and `drawn` is kept, so the next whole paint's
    /// `wipe(drawn)` clears every layer.
    #[test]
    fn a_moved_wash_keeps_its_paint_until_it_wipes_what_it_drew() {
        let mut wash = developed(146, 82, f32::INFINITY, true);
        // `speckle` painted layer px 5..11 of each: sharp 5..11, glow 10..22, bloom 20..44.
        let drawn = DeviceRect::new(5, 5, 44, 44);
        wash.drawn = drawn;
        let painted = |w: &Wash| {
            [&w.sharp, &w.glow, &w.bloom_raw]
                .map(|p| p.planes().iter().any(|plane| plane.iter().any(|&v| v != 0)))
        };
        assert_eq!(painted(&wash), [true; 3]);
        let moved = DeviceRect::new(40, 9, 186, 91);
        wash.reshape(moved, f32::INFINITY, 40.0);
        assert_eq!(wash.rect, moved);
        assert_eq!(
            painted(&wash),
            [true; 3],
            "moved, not resized: the paint is kept"
        );
        assert_eq!(wash.drawn, drawn);
        wash.wipe(wash.drawn);
        assert_eq!(painted(&wash), [false; 3], "the wipe clears every layer");
    }

    /// The u8 blur keeps a block's energy within 2% (its box means round per pass).
    #[test]
    fn a_blurred_pigment_keeps_its_paint() {
        let mut plane = vec![0u8; 64 * 64];
        for y in 28..37 {
            plane[y * 64 + 28..y * 64 + 37].fill(255);
        }
        let before: u32 = plane.iter().map(|&v| u32::from(v)).sum();
        let mut scratch = Vec::new();
        blur_plane(&mut plane, 64, 64, 2.0, &mut scratch);
        let after: u32 = plane.iter().map(|&v| u32::from(v)).sum();
        assert!(after.abs_diff(before) * 50 <= before, "{before} → {after}");
    }
}
