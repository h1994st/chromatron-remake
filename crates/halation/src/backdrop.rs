//! `backdrop`: the page's near-black ground (spec §7.2): a radial lift towards the board, a
//! vignette and grain. It is baked per resize at ¼ resolution plus a 256² grain tile, so there
//! is no full-size buffer; `fill` upsamples it in fixed point, a row at a time.

use crate::etch::{Layer, Pt, Shape, Stops, clip_rect, pt};
use crate::spectrum::{Palette, Rgb, Rgba};
use crate::vitrine::falloff;
use crate::{DeviceRect, fit_buffer};

/// A soft shadow on the light page (ruling L7): `rect` (device px, left, top, right, bottom)
/// rounded `radius`, `drop` px down, blurred by `sigma` px (`vitrine::falloff`), in `colour`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Casting {
    pub rect: [f32; 4],
    pub radius: f32,
    pub drop: f32,
    pub sigma: f32,
    pub colour: Rgba,
}

/// The light board panel's fill (spec §7.2, ruling L7): inside `rect` rounded `radius`, radial
/// from `centre` over `reach` px through `stops` at 0, 0.6 and 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelFill {
    pub rect: [f32; 4],
    pub radius: f32,
    pub centre: Pt,
    pub reach: f32,
    pub stops: [Rgb; 3],
}

/// What the page bakes besides its palette: the lift's focus (the board's centre), the shadows
/// that lie on it, and the board panel's fill.
#[derive(Clone, Copy, Debug)]
pub struct Ground<'a> {
    pub focus: Pt,
    pub castings: &'a [Casting],
    pub panel: Option<PanelFill>,
}

/// The most castings the page holds: the board panel's, the card's, the tray's.
pub const MAX_CASTINGS: usize = 3;

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
    /// Whether `overlay` is the identity (a grain of 0): `fill` then skips the grain.
    grainless: bool,
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
            grainless: false,
            row: Vec::new(),
            width: 0,
            height: 0,
            quarter_width: 0,
            quarter_height: 0,
        }
    }

    /// Bakes the ¼-resolution ground for a `width × height` canvas: the palette's ground lifted
    /// towards `ground.focus`, then (light) the castings and the board panel's fill.
    pub fn reshape(&mut self, palette: &Palette, width: usize, height: usize, ground: &Ground) {
        let (qw, qh) = (width.div_ceil(4), height.div_ceil(4));
        self.width = width;
        self.height = height;
        self.quarter_width = qw;
        self.quarter_height = qh;
        fit_buffer(&mut self.quarter, qw * qh, 0);
        fit_buffer(&mut self.row, qw + 2, 0);

        let diag = (width as f32).hypot(height as f32);
        let top = palette.ground[0].opaque();
        let flat = palette.ground[0] == palette.ground[1];
        let fall = Stops::new(&[(0.0, top), (1.0, palette.ground[1].opaque())]);
        let lift = Stops::new(&[
            (0.0, palette.lift[0]),
            (0.45 / 0.55, palette.lift[1]),
            (1.0, palette.lift_end),
        ]);
        let vignette = Stops::new(&[
            (0.0, palette.vignette.with_alpha(0.0)),
            (1.0, palette.vignette),
        ]);
        let darkened = palette.vignette.a > 0.0;
        // Each casting's shape (dropped), the box its shadow can reach (2σ round the shape), and
        // the box inside which its falloff is exactly 1 (`inside`).
        assert!(ground.castings.len() <= MAX_CASTINGS);
        let none = [0.0f32; 4];
        let mut cast = [(Shape::disc(pt(0.0, 0.0), 0.0), none, none); MAX_CASTINGS];
        for (k, c) in ground.castings.iter().enumerate() {
            let [l, t, r, b] = c.rect;
            let reach = 2.0 * c.sigma;
            let (t, b) = (t + c.drop, b + c.drop);
            let shape = Shape::round_rect(l, t, r, b, c.radius);
            cast[k] = (
                shape,
                [l - reach, t - reach, r + reach, b + reach],
                inside(&shape, reach),
            );
        }
        let panel = ground.panel.map(|p| {
            let [l, t, r, b] = p.rect;
            let stops = Stops::new(&[
                (0.0, p.stops[0].opaque()),
                (0.6, p.stops[1].opaque()),
                (1.0, p.stops[2].opaque()),
            ]);
            let shape = Shape::round_rect(l, t, r, b, p.radius);
            // Cover is 0 at a distance of 2 px or more: past `near`, with a px to spare for the
            // f32 distance, it is 0 for certain; at −2 px or less it is 1, as in `inside`.
            let near = [l - 3.0, t - 3.0, r + 3.0, b + 3.0];
            (shape, stops, p, near, inside(&shape, 2.0))
        });
        let (cx, cy) = (width as f32 / 2.0, height as f32 / 2.0);
        let focus = ground.focus;
        let within = |x: f32, y: f32, [l, t, r, b]: [f32; 4]| x > l && x < r && y > t && y < b;
        for qy in 0..qh {
            let y = (qy as f32 + 0.5) * 4.0;
            // The ground: the dark ink, or the light page's vertical fall (per row).
            let floor = if flat {
                top
            } else {
                fall.sample(y / height as f32)
            };
            for qx in 0..qw {
                let x = (qx as f32 + 0.5) * 4.0;
                // The board panel's opaque fill first: coverage at ¼ resolution, the edge's ramp
                // one quarter px (4 px) wide. Where it covers the texel whole, nothing under it
                // shows (`over` at α 1 is the fill exactly), so the page is not computed.
                let fill = panel.as_ref().and_then(|(shape, stops, p, near, full)| {
                    if !within(x, y, *near) {
                        return None;
                    }
                    let cover = if within(x, y, *full) {
                        1.0
                    } else {
                        (0.5 - shape.signed(pt(x, y)) / 4.0).clamp(0.0, 1.0)
                    };
                    (cover > 0.0).then(|| {
                        let s = stops.sample((x - p.centre.x).hypot(y - p.centre.y) / p.reach);
                        s.with_alpha(cover)
                    })
                });
                let c = match fill {
                    Some(s) if s.a >= 1.0 => s,
                    _ => {
                        let t = (x - focus.x).hypot(y - focus.y) / (0.55 * diag);
                        let mut c = over(floor, lift.sample(t));
                        if darkened {
                            let t = ((x - cx).hypot(y - cy) - 0.25 * diag) / ((0.62 - 0.25) * diag);
                            c = over(c, vignette.sample(t));
                        }
                        for (casting, &(shape, [l, t, r, b], full)) in
                            ground.castings.iter().zip(&cast)
                        {
                            if x < l || x > r || y < t || y > b {
                                continue;
                            }
                            let a = if within(x, y, full) {
                                1.0
                            } else {
                                falloff(shape.signed(pt(x, y)), casting.sigma)
                            };
                            if a > 0.0 {
                                c = over(c, casting.colour.with_alpha(casting.colour.a * a));
                            }
                        }
                        fill.map_or(c, |s| over(c, s))
                    }
                };
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
        // No grain (the light page): every grain value leaves every code as it is.
        self.grainless = (self.overlay.iter().enumerate()).all(|(i, &v)| usize::from(v) == i % 256);
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
        let grainless = self.grainless;
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
                // Grainless, the overlay gives each code back: it is not looked up.
                let lane = |shift: u32| {
                    let code = usize::from((v >> shift) as u8);
                    if grainless {
                        code as u32
                    } else {
                        let g = usize::from(grain_row[x % GRAIN_SIDE]) * 256;
                        u32::from(overlay[g + code])
                    }
                };
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

/// The box (left, top, right, bottom), strictly inside which round rect `shape`'s signed
/// distance is −`depth` − 1 or less: it is inset by the larger of `depth` and the corner radius,
/// plus 1 px, so a point inside is past the corners on both axes, where the distance is
/// `max(|dx| − hx, |dy| − hy)`. The px to spare covers the f32 distance's rounding. Empty (never
/// inside) for another shape.
fn inside(shape: &Shape, depth: f32) -> [f32; 4] {
    let Shape::RoundRect {
        left,
        top,
        right,
        bottom,
        radii,
        ..
    } = *shape
    else {
        return [0.0; 4];
    };
    let m = radii.into_iter().fold(depth, f32::max) + 1.0;
    [left + m, top + m, right - m, bottom - m]
}

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

    /// A page with nothing on it but its palette's ground, lifted towards `focus`.
    fn bare(focus: Pt) -> Ground<'static> {
        Ground {
            focus,
            castings: &[],
            panel: None,
        }
    }

    /// The quarter plane's texel at quarter column `qx`, row `qy`, as [r, g, b].
    fn texel(backdrop: &Backdrop, qx: usize, qy: usize) -> [u32; 3] {
        let p = backdrop.quarter[qy * backdrop.quarter_width + qx];
        [p >> 16 & 0xff, p >> 8 & 0xff, p & 0xff]
    }

    /// The texel at (`qx`, `qy`) as the bake defines it, with no box skipped: the panel's cover
    /// and every casting's falloff from the distance itself, and the page under a fill that
    /// covers it whole (light palette: no vignette).
    fn plain_bake(
        palette: &Palette,
        w: usize,
        h: usize,
        ground: &Ground,
        qx: usize,
        qy: usize,
    ) -> u32 {
        let (x, y) = ((qx as f32 + 0.5) * 4.0, (qy as f32 + 0.5) * 4.0);
        let diag = (w as f32).hypot(h as f32);
        let fall = Stops::new(&[
            (0.0, palette.ground[0].opaque()),
            (1.0, palette.ground[1].opaque()),
        ]);
        let lift = Stops::new(&[
            (0.0, palette.lift[0]),
            (0.45 / 0.55, palette.lift[1]),
            (1.0, palette.lift_end),
        ]);
        let fill = ground.panel.and_then(|p| {
            let [l, t, r, b] = p.rect;
            let shape = Shape::round_rect(l, t, r, b, p.radius);
            let stops = Stops::new(&[
                (0.0, p.stops[0].opaque()),
                (0.6, p.stops[1].opaque()),
                (1.0, p.stops[2].opaque()),
            ]);
            let cover = (0.5 - shape.distance(pt(x, y)) / 4.0).clamp(0.0, 1.0);
            (cover > 0.0).then(|| {
                let s = stops.sample((x - p.centre.x).hypot(y - p.centre.y) / p.reach);
                s.with_alpha(cover)
            })
        });
        // The page and its castings under the fill too (the bake skips them where the fill
        // covers the texel whole: review T4 M1).
        let t = (x - ground.focus.x).hypot(y - ground.focus.y) / (0.55 * diag);
        let mut c = over(fall.sample(y / h as f32), lift.sample(t));
        for k in ground.castings {
            let [l, t, r, b] = k.rect;
            let reach = 2.0 * k.sigma;
            let (t, b) = (t + k.drop, b + k.drop);
            if x < l - reach || x > r + reach || y < t - reach || y > b + reach {
                continue;
            }
            let shape = Shape::round_rect(l, t, r, b, k.radius);
            let a = falloff(shape.distance(pt(x, y)), k.sigma);
            if a > 0.0 {
                c = over(c, k.colour.with_alpha(k.colour.a * a));
            }
        }
        let c = fill.map_or(c, |s| over(c, s));
        let code = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u32;
        code(c.r) << 16 | code(c.g) << 8 | code(c.b)
    }

    /// The bake skips the distance where the panel's cover is surely 0 or 1 and where a
    /// casting's falloff is surely 1; every texel is still what the plain bake gives, for
    /// castings whose radius is under and over 2σ, at fractional edges.
    #[test]
    fn the_bake_skips_only_what_it_would_compute_the_same() {
        let (w, h) = (701, 523);
        let shade = Rgb(27, 35, 64).alpha(0.14);
        let castings = [
            Casting {
                rect: [100.5, 90.25, 520.0, 400.75],
                radius: 40.0,
                drop: 20.0,
                sigma: 28.0,
                colour: shade,
            },
            Casting {
                rect: [530.0, 30.0, 690.0, 300.0],
                radius: 28.0,
                drop: 8.0,
                sigma: 18.0,
                colour: shade,
            },
            Casting {
                rect: [20.0, 440.0, 680.0, 515.0],
                radius: 4.0,
                drop: 6.0,
                sigma: 30.0,
                colour: shade,
            },
        ];
        let panel = PanelFill {
            rect: [100.5, 90.25, 520.0, 400.75],
            radius: 40.0,
            centre: pt(310.0, 245.0),
            reach: 330.0,
            stops: [Rgb(251, 252, 254), Rgb(241, 243, 248), Rgb(228, 232, 241)],
        };
        let ground = Ground {
            focus: pt(310.0, 245.0),
            castings: &castings,
            panel: Some(panel),
        };
        let mut baked = Backdrop::new();
        baked.reshape(&Palette::LIGHT, w, h, &ground);
        for qy in 0..h.div_ceil(4) {
            for qx in 0..w.div_ceil(4) {
                let want = plain_bake(&Palette::LIGHT, w, h, &ground, qx, qy);
                let got = baked.quarter[qy * baked.quarter_width + qx];
                assert_eq!(got, want, "({qx}, {qy})");
            }
        }
    }

    /// A casting darkens the page round its dropped rect, most at the dropped edge, and leaves
    /// every texel past its 2σ reach box exactly as the bare page bakes it.
    #[test]
    fn a_casting_darkens_the_page_only_within_its_reach() {
        let (w, h) = (400, 300);
        let focus = pt(200.0, 150.0);
        let mut plain = Backdrop::new();
        plain.reshape(&Palette::LIGHT, w, h, &bare(focus));
        let casting = Casting {
            rect: [100.0, 100.0, 300.0, 180.0],
            radius: 8.0,
            drop: 10.0,
            sigma: 14.0,
            colour: Rgb(27, 35, 64).alpha(0.14),
        };
        let mut shaded = Backdrop::new();
        let ground = Ground {
            castings: &[casting],
            ..bare(focus)
        };
        shaded.reshape(&Palette::LIGHT, w, h, &ground);
        let lum = |c: [u32; 3]| c[0] + c[1] + c[2];
        // The dropped bottom edge at x = 200: y = 190, quarter row 47 (centre 190).
        let (edge, clear) = (texel(&shaded, 50, 47), texel(&plain, 50, 47));
        assert!(
            lum(edge) + 20 < lum(clear),
            "at the edge: {edge:?} vs {clear:?}"
        );
        // Past the reach box (190 + 28 = 218): row 55 (centre 222) and below, and the columns
        // left of 100 − 28 = 72 (column 17, centre 70), are untouched.
        for qy in 0..h / 4 {
            for qx in 0..w / 4 {
                if qy >= 55 || qx <= 17 {
                    assert_eq!(
                        texel(&shaded, qx, qy),
                        texel(&plain, qx, qy),
                        "({qx}, {qy})"
                    );
                }
            }
        }
    }

    /// Inside the board panel's rect its radial fill covers the page: at the centre the first
    /// stop, at the reach the last; outside the rect the page shows.
    #[test]
    fn the_panel_fill_covers_the_page_inside_its_rect() {
        let (w, h) = (400, 400);
        let focus = pt(200.0, 200.0);
        let stops = [Rgb(250, 251, 252), Rgb(200, 210, 220), Rgb(100, 110, 120)];
        let ground = Ground {
            panel: Some(PanelFill {
                rect: [80.0, 80.0, 320.0, 320.0],
                radius: 12.0,
                centre: focus,
                reach: 160.0,
                stops,
            }),
            ..bare(focus)
        };
        let mut backdrop = Backdrop::new();
        backdrop.reshape(&Palette::LIGHT, w, h, &ground);
        let mut plain = Backdrop::new();
        plain.reshape(&Palette::LIGHT, w, h, &bare(focus));
        // Quarter (49, 49) centres at (198, 198), 2.8 px from the centre: the first stop.
        let rgb = |c: Rgb| [u32::from(c.0), u32::from(c.1), u32::from(c.2)];
        let near = |a: [u32; 3], b: [u32; 3], tol: u32| (0..3).all(|k| a[k].abs_diff(b[k]) <= tol);
        assert!(near(texel(&backdrop, 49, 49), rgb(stops[0]), 1));
        // Quarter (49, 79) centres at (198, 318): 118 px out, 0.74 of the reach, 2 px inside.
        let k = (118.0f32.hypot(2.0) / 160.0 - 0.6) / 0.4;
        let want = [0, 1, 2].map(|c| {
            let (a, b) = (rgb(stops[1])[c] as f32, rgb(stops[2])[c] as f32);
            (a + (b - a) * k).round() as u32
        });
        assert!(
            near(texel(&backdrop, 49, 79), want, 2),
            "{:?} vs {want:?}",
            texel(&backdrop, 49, 79)
        );
        // Outside the rect (quarter rows 0–18 end at 76 px): the bare page.
        for qy in 0..19 {
            assert_eq!(texel(&backdrop, 49, qy), texel(&plain, 49, qy));
        }
    }

    /// The overlay table is kept across a resize and made again for another grain strength, as
    /// a fresh backdrop makes it.
    #[test]
    fn the_overlay_follows_the_grain_alone() {
        let focus = &bare(pt(390.0, 400.0));
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

    /// The light page has no grain: its overlay is the identity, which `fill` skips.
    #[test]
    fn only_a_page_without_grain_skips_the_overlay() {
        let focus = &bare(pt(390.0, 400.0));
        let mut backdrop = Backdrop::new();
        backdrop.reshape(&Palette::LIGHT, 780, 1480, focus);
        assert!(backdrop.grainless);
        backdrop.reshape(&Palette::DARK, 780, 1480, focus);
        assert!(!backdrop.grainless);
        backdrop.reshape(&Palette::LIGHT, 781, 1480, focus);
        assert!(backdrop.grainless, "a theme switch makes the table again");
    }

    #[test]
    fn fills_match_plain_bilinear_texels() {
        // Odd canvas widths, odd and even starts, spans of 1–3 px and longer, rows of every phase;
        // with grain (dark) and without (light: the overlay is skipped).
        let sizes = [(37, 23), (101, 9), (8, 8), (5, 6)];
        for (palette, (w, h)) in [Palette::DARK, Palette::LIGHT]
            .into_iter()
            .flat_map(|p| sizes.map(|s| (p, s)))
        {
            let mut backdrop = Backdrop::new();
            let focus = pt(w as f32 * 0.3, h as f32 * 0.6);
            backdrop.reshape(&palette, w, h, &bare(focus));
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
        backdrop.reshape(&Palette::DARK, w, h, &bare(pt(200.0, 200.0)));
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
