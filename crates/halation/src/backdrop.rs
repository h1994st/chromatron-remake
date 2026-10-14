//! `backdrop`: the page's near-black ground (spec §7.2): a radial lift towards the board, a
//! vignette and grain. It is baked per resize at ¼ resolution plus a 256² grain tile, so there
//! is no full-size buffer; `fill` upsamples it in fixed point, a row at a time.

use crate::etch::{Layer, Pt, Stops, clip_rect};
use crate::spectrum::{Palette, Rgba};
use crate::{DeviceRect, fit_buffer};

/// Mulberry32, the trailer's PRNG: the same seed gives the same sequence in [0, 1).
pub struct Mulberry32(pub u32);

impl Mulberry32 {
    /// Never inlined: the grain tile is made once, and inlined its counter-based steps vectorise
    /// into about 20 KB of wasm.
    #[inline(never)]
    pub fn next_unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ t >> 15).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ t >> 7).wrapping_mul(t | 61));
        f64::from(t ^ t >> 14) / 4_294_967_296.0
    }
}

const GRAIN_SIDE: usize = 256;
const GRAIN_SEED: u32 = 7;

/// A 256 × 256 tile of seeded monochrome noise centred on mid grey, as the trailer makes it.
pub fn grain_tile(seed: u32) -> Vec<u8> {
    let mut random = Mulberry32(seed);
    (0..GRAIN_SIDE * GRAIN_SIDE)
        .map(|_| {
            let v = 128.0 + (random.next_unit() + random.next_unit() - 1.0) * 127.0;
            v.round_ties_even().clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// The baked backdrop.
pub struct Backdrop {
    quarter: Vec<u32>,
    grain: Vec<u8>,
    overlay: Vec<u8>,
    /// The grain strength `overlay` was made for: it depends on nothing else, so a resize keeps
    /// it.
    overlay_grain: Option<f32>,
    /// One vertically blended quarter row, channels in `lanes`.
    row: Vec<u64>,
    width: usize,
    height: usize,
    quarter_width: usize,
    quarter_height: usize,
}

/// Bilinear weights in eighths for a device pixel's phase within its quarter cell: the
/// neighbour on the near side (`i − 1` for phases 0 and 1, `i` for 2 and 3) and the far one.
const WEIGHTS: [(u16, u16); 4] = [(3, 5), (1, 7), (7, 1), (5, 3)];

impl Default for Backdrop {
    fn default() -> Self {
        Self::new()
    }
}

impl Backdrop {
    pub fn new() -> Backdrop {
        Backdrop {
            quarter: Vec::new(),
            grain: grain_tile(GRAIN_SEED),
            overlay: vec![0; 256 * 256],
            overlay_grain: None,
            row: Vec::new(),
            width: 0,
            height: 0,
            quarter_width: 0,
            quarter_height: 0,
        }
    }

    /// Bakes the ¼-resolution ground for a `width × height` canvas lifted towards `focus`.
    pub fn reshape(&mut self, palette: &Palette, width: usize, height: usize, focus: Pt) {
        let (qw, qh) = (width.div_ceil(4), height.div_ceil(4));
        self.width = width;
        self.height = height;
        self.quarter_width = qw;
        self.quarter_height = qh;
        fit_buffer(&mut self.quarter, qw * qh, 0);
        fit_buffer(&mut self.row, qw + 2, 0);

        let ink = palette.ink.opaque();
        let diag = (width as f32).hypot(height as f32);
        let lift = Stops::new(&[
            (0.0, palette.lift[0]),
            (0.45 / 0.55, palette.lift[1]),
            (1.0, ink.with_alpha(0.0)),
        ]);
        let vignette = Stops::new(&[
            (0.0, palette.vignette.with_alpha(0.0)),
            (1.0, palette.vignette),
        ]);
        let (cx, cy) = (width as f32 / 2.0, height as f32 / 2.0);
        for qy in 0..qh {
            let y = (qy as f32 + 0.5) * 4.0;
            for qx in 0..qw {
                let x = (qx as f32 + 0.5) * 4.0;
                let t = (x - focus.x).hypot(y - focus.y) / (0.55 * diag);
                let mut c = over(ink, lift.sample(t));
                let t = ((x - cx).hypot(y - cy) - 0.25 * diag) / ((0.62 - 0.25) * diag);
                c = over(c, vignette.sample(t));
                let code = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u32;
                self.quarter[qy * qw + qx] = code(c.r) << 16 | code(c.g) << 8 | code(c.b);
            }
        }

        let grain = palette.grain;
        if self.overlay_grain == Some(grain) {
            return;
        }
        self.overlay_grain = Some(grain);
        for g in 0..256 {
            let gv = g as f32 / 255.0;
            for b in 0..256 {
                let bv = b as f32 / 255.0;
                let o = if bv <= 0.5 {
                    2.0 * bv * gv
                } else {
                    1.0 - 2.0 * (1.0 - bv) * (1.0 - gv)
                };
                self.overlay[g * 256 + b] =
                    (255.0 * (bv + grain * (o - bv))).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    /// Paints the backdrop into `rect` of `layer` (clipped to both). Integer and row by row:
    /// no allocation.
    pub fn fill(&mut self, layer: &mut Layer, rect: DeviceRect) {
        let bounds = DeviceRect::new(
            0,
            0,
            self.width.min(layer.width) as i32,
            self.height.min(layer.height) as i32,
        );
        let Some(r) = clip_rect(rect, bounds) else {
            return;
        };
        let (qw, qh) = (self.quarter_width, self.quarter_height);
        let Ok(overlay) = <&[u8; 256 * 256]>::try_from(self.overlay.as_slice()) else {
            return;
        };
        let (x0, x1) = (r.left as usize, r.right as usize);
        // The quarter columns with pixels in the span, `first..=last`; the row holds them with one
        // more each side, clamped to the edge as the bilinear taps are.
        let (first, last) = (x0 / 4, (x1 - 1) / 4);
        let columns = last - first + 3;
        let column = |k: usize| (first + k).saturating_sub(1).min(qw - 1);
        for y in r.top as usize..r.bottom as usize {
            let (iy, ky) = (y / 4, y % 4);
            let (a, b) = if ky < 2 {
                (iy.saturating_sub(1), iy)
            } else {
                (iy, (iy + 1).min(qh - 1))
            };
            let (wa, wb) = WEIGHTS[ky];
            let (wa, wb) = (u64::from(wa), u64::from(wb));
            let top = &self.quarter[a * qw..][..qw];
            let bottom = &self.quarter[b * qw..][..qw];
            for (k, dst) in self.row[..columns].iter_mut().enumerate() {
                *dst = lanes(top[column(k)]) * wa + lanes(bottom[column(k)]) * wb;
            }
            let cells = &self.row[..columns];
            let grain_row = &self.grain[(y % GRAIN_SIDE) * GRAIN_SIDE..][..GRAIN_SIDE];
            let out = &mut layer.px[y * layer.width..][..layer.width];
            // Pixel x from its quarter column's row entry and the near neighbour's. Each lane is
            // at most 255·8·8 + 32 < 2¹⁶, so the three channels never carry into one another,
            // and (… + 32) >> 6 ≤ 255 is the lane's low byte after the shift.
            let texel = |x: usize, phase: usize, taps: &[u64]| {
                let (near, far) = if phase < 2 {
                    (taps[0], taps[1])
                } else {
                    (taps[1], taps[2])
                };
                let (w0, w1) = WEIGHTS[phase];
                let v = (near * u64::from(w0) + far * u64::from(w1) + ROUNDING) >> 6;
                let g = usize::from(grain_row[x % GRAIN_SIDE]) * 256;
                let lane = |shift: u32| u32::from(overlay[g + usize::from((v >> shift) as u8)]);
                lane(32) << 16 | lane(16) << 8 | lane(0)
            };
            // A ragged start and end pixel by pixel; whole quarter columns between.
            let body = x0.next_multiple_of(4).min(x1)..x1 / 4 * 4;
            let ends = (x0..body.start).chain(body.end.max(body.start)..x1);
            for x in ends {
                out[x] = texel(x, x % 4, &cells[x / 4 - first..][..3]);
            }
            if body.start < body.end {
                let (quads, _) = out[body.clone()].as_chunks_mut::<4>();
                let taps = cells[body.start / 4 - first..].windows(3);
                for ((quad, taps), x) in quads.iter_mut().zip(taps).zip(body.step_by(4)) {
                    for (phase, px) in quad.iter_mut().enumerate() {
                        *px = texel(x + phase, phase, taps);
                    }
                }
            }
        }
    }

    /// Heap held by the baked buffers, in bytes.
    pub fn bytes(&self) -> usize {
        self.quarter.capacity() * size_of::<u32>()
            + self.grain.capacity()
            + self.overlay.capacity()
            + self.row.capacity() * size_of::<u64>()
    }
}

/// `0x00RRGGBB` spread to 16-bit lanes of a u64 (red at bit 32), so one multiply-add weighs all
/// three channels.
#[inline(always)]
fn lanes(rgb: u32) -> u64 {
    let rgb = u64::from(rgb);
    (rgb & 0xff_0000) << 16 | (rgb & 0xff00) << 8 | rgb & 0xff
}

/// The bilinear rounding term, 32, in every lane.
const ROUNDING: u64 = 32 << 32 | 32 << 16 | 32;

/// `top` source-over an opaque `base`, in straight alpha.
fn over(base: Rgba, top: Rgba) -> Rgba {
    let mix = |b: f32, t: f32| t * top.a + b * (1.0 - top.a);
    Rgba {
        r: mix(base.r, top.r),
        g: mix(base.g, top.g),
        b: mix(base.b, top.b),
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etch::pt;

    /// The overlay table is kept across a resize and made again for another grain strength, as
    /// a fresh backdrop makes it.
    #[test]
    fn the_overlay_follows_the_grain_alone() {
        let focus = pt(390.0, 400.0);
        let mut kept = Backdrop::new();
        kept.reshape(&Palette::DARK, 780, 1480, focus);
        kept.reshape(&Palette::DARK, 781, 1480, focus);
        let mut fresh = Backdrop::new();
        fresh.reshape(&Palette::DARK, 781, 1480, focus);
        assert!(kept.overlay == fresh.overlay, "a resize keeps the table");
        let mut grainier = Palette::DARK;
        grainier.grain *= 2.0;
        kept.reshape(&grainier, 781, 1480, focus);
        let mut fresh = Backdrop::new();
        fresh.reshape(&grainier, 781, 1480, focus);
        assert!(kept.overlay == fresh.overlay, "a new grain makes it again");
        let mut plain = Backdrop::new();
        plain.reshape(&Palette::DARK, 781, 1480, focus);
        assert!(kept.overlay != plain.overlay);
    }

    #[test]
    fn the_grain_tile_is_the_trailers() {
        let tile = grain_tile(7);
        assert_eq!(&tile[..8], &[10, 214, 119, 91, 164, 54, 164, 73]);
        let fnv = tile.iter().fold(2_166_136_261u32, |h, &v| {
            (h ^ u32::from(v)).wrapping_mul(16_777_619)
        });
        assert_eq!(fnv, 0xd569_7a71, "the whole 256 × 256 tile");
    }

    /// Pixel (x, y) by the plain definition: per channel, the quarter plane's two rows and two
    /// columns (clamped to its edges) weighted in eighths, then the grain overlay.
    fn plain_texel(backdrop: &Backdrop, x: usize, y: usize) -> u32 {
        let (qw, qh) = (backdrop.quarter_width, backdrop.quarter_height);
        let taps = |i: usize, phase: usize, last: usize| {
            let (near, far) = if phase < 2 {
                (i.saturating_sub(1), i)
            } else {
                (i, (i + 1).min(last))
            };
            let (wn, wf) = WEIGHTS[phase];
            [(near, u32::from(wn)), (far, u32::from(wf))]
        };
        let grain = usize::from(backdrop.grain[(y % GRAIN_SIDE) * GRAIN_SIDE + x % GRAIN_SIDE]);
        let mut rgb = 0;
        for shift in [16, 8, 0] {
            let mut sum = 0;
            for (row, wy) in taps(y / 4, y % 4, qh - 1) {
                for (column, wx) in taps(x / 4, x % 4, qw - 1) {
                    sum += (backdrop.quarter[row * qw + column] >> shift & 0xff) * wy * wx;
                }
            }
            let v = (sum + 32) >> 6;
            rgb |= u32::from(backdrop.overlay[grain * 256 + v as usize]) << shift;
        }
        rgb
    }

    #[test]
    fn fills_match_plain_bilinear_texels() {
        // Odd canvas widths, odd and even starts, spans of 1–3 px and longer, rows of every phase.
        for (w, h) in [(37, 23), (101, 9), (8, 8), (5, 6)] {
            let mut backdrop = Backdrop::new();
            backdrop.reshape(&Palette::DARK, w, h, pt(w as f32 * 0.3, h as f32 * 0.6));
            let mut px = vec![0u32; w * h];
            for left in 0..w.min(9) {
                for span in [1, 2, 3, 4, 5, 11, w] {
                    let right = (left + span).min(w);
                    px.fill(0);
                    let mut layer = Layer {
                        px: &mut px,
                        width: w,
                        height: h,
                    };
                    let rect = DeviceRect::new(left as i32, 0, right as i32, h as i32);
                    backdrop.fill(&mut layer, rect);
                    for y in 0..h {
                        for x in 0..w {
                            let want = if (left..right).contains(&x) {
                                plain_texel(&backdrop, x, y)
                            } else {
                                0
                            };
                            assert_eq!(px[y * w + x], want, "{w}×{h}, {left}..{right}: ({x}, {y})");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_backdrop_lifts_round_the_board_and_darkens_the_corners() {
        let (w, h) = (640, 400);
        let mut backdrop = Backdrop::new();
        backdrop.reshape(&Palette::DARK, w, h, pt(200.0, 200.0));
        let mut px = vec![0u32; w * h];
        let mut layer = Layer {
            px: &mut px,
            width: w,
            height: h,
        };
        backdrop.fill(&mut layer, DeviceRect::new(0, 0, w as i32, h as i32));
        let lum = |p: u32| (p >> 16 & 0xff) + (p >> 8 & 0xff) + (p & 0xff);
        let focus = (190..210)
            .flat_map(|y| (190..210).map(move |x| y * w + x))
            .map(|i| lum(px[i]))
            .sum::<u32>();
        let corner = (0..20)
            .flat_map(|y| (620..640).map(move |x| y * w + x))
            .map(|i| lum(px[i]))
            .sum::<u32>();
        assert!(
            focus > corner,
            "lifted at the board, vignetted at the corner: {focus} vs {corner}"
        );
        let p = px[200 * w + 200];
        assert!(
            (p & 0xff) > (p >> 16 & 0xff) + 10 && (p & 0xff) <= 60,
            "a dark navy lift: {p:06x}"
        );
        let mut again = vec![0u32; w * h];
        backdrop.fill(
            &mut Layer {
                px: &mut again,
                width: w,
                height: h,
            },
            DeviceRect::new(100, 50, 300, 90),
        );
        for y in 50..90 {
            for x in 100..300 {
                assert_eq!(
                    again[y * w + x],
                    px[y * w + x],
                    "a partial fill matches the full one"
                );
            }
        }
    }
}
