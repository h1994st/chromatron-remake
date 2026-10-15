//! `etch`: the analytic rasteriser (spec §4). A shape is a signed distance `d` (px, negative
//! inside); a pixel's coverage is `clamp(0.5 − d, 0, 1)` at its centre. Fills walk per-row spans,
//! so cost follows covered area. A fill of several shapes covers their union (min distance), so
//! joined parts have no seams. Colours are sRGB code values, blended as Canvas 2D does.

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::ops::{Add, Mul, Sub};

use crate::DeviceRect;
use crate::spectrum::Rgba;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

pub const fn pt(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

impl Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        pt(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        pt(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f32> for Pt {
    type Output = Pt;
    fn mul(self, k: f32) -> Pt {
        pt(self.x * k, self.y * k)
    }
}

impl Pt {
    pub fn dot(self, o: Pt) -> f32 {
        self.x * o.x + self.y * o.y
    }
    pub fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
}

/// The unit vector of angle `a` (radians clockwise from north, y down).
pub fn heading(a: f32) -> Pt {
    pt(sine(a), -sine(a + FRAC_PI_2))
}

/// `sin(a)` to about 4e-6 for any angle, from a Taylor polynomial: the page ships no libm
/// `sinf` and `cosf` (with their large-argument reduction) for two arc end caps.
fn sine(a: f32) -> f32 {
    let mut x = a - TAU * (a / TAU + 0.5).floor();
    if x > FRAC_PI_2 {
        x = PI - x;
    } else if x < -FRAC_PI_2 {
        x = -PI - x;
    }
    let s = x * x;
    x * (1.0 + s * (-1.0 / 6.0 + s * (1.0 / 120.0 + s * (-1.0 / 5040.0 + s * (1.0 / 362_880.0)))))
}

/// The angle of the vector (`x`, `y`) in (−π, π], to about 1e-5, for the same reason as `sine`.
fn angle_of(y: f32, x: f32) -> f32 {
    let (ay, ax) = (y.abs(), x.abs());
    let (lo, hi) = if ay < ax { (ay, ax) } else { (ax, ay) };
    if hi == 0.0 {
        return 0.0;
    }
    let t = lo / hi;
    let s = t * t;
    let mut a =
        t * (0.999_866 + s * (-0.330_299_5 + s * (0.180_141 + s * (-0.085_133 + s * 0.020_835_1))));
    if ay > ax {
        a = FRAC_PI_2 - a;
    }
    if x < 0.0 {
        a = PI - a;
    }
    if y < 0.0 { -a } else { a }
}

pub const MAX_POLYGON: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// Points within `radius` of segment `a`–`b`: a stroke with round caps, or a disc.
    Capsule { a: Pt, b: Pt, radius: f32 },
    /// A stroke `2·half_width` wide along the circle of `radius`, from angle `start` for `sweep`
    /// radians clockwise, with round caps. A sweep ≥ 2π is a whole ring.
    Arc {
        centre: Pt,
        radius: f32,
        half_width: f32,
        start: f32,
        sweep: f32,
    },
    /// An axis-aligned rect with corner radii top-left, top-right, bottom-right, bottom-left;
    /// `stroke > 0` makes it an outline of that half-width centred on the edge.
    RoundRect {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        radii: [f32; 4],
        stroke: f32,
    },
    /// A convex polygon, clockwise on screen, with corners rounded by `round` (stored already
    /// inset by `round`); `stroke > 0` makes it an outline.
    Polygon {
        points: [Pt; MAX_POLYGON],
        len: usize,
        round: f32,
        stroke: f32,
    },
}

impl Shape {
    pub fn disc(centre: Pt, radius: f32) -> Shape {
        Shape::Capsule {
            a: centre,
            b: centre,
            radius,
        }
    }
    pub fn line(a: Pt, b: Pt, width: f32) -> Shape {
        Shape::Capsule {
            a,
            b,
            radius: width / 2.0,
        }
    }
    pub fn ring(centre: Pt, radius: f32, width: f32) -> Shape {
        Shape::arc(centre, radius, width, 0.0, TAU)
    }
    pub fn arc(centre: Pt, radius: f32, width: f32, start: f32, sweep: f32) -> Shape {
        Shape::Arc {
            centre,
            radius,
            half_width: width / 2.0,
            start,
            sweep,
        }
    }
    pub fn rect(left: f32, top: f32, right: f32, bottom: f32) -> Shape {
        Shape::round_rect(left, top, right, bottom, 0.0)
    }
    pub fn round_rect(left: f32, top: f32, right: f32, bottom: f32, radius: f32) -> Shape {
        let r = radius
            .min((right - left) / 2.0)
            .min((bottom - top) / 2.0)
            .max(0.0);
        Shape::RoundRect {
            left,
            top,
            right,
            bottom,
            radii: [r; 4],
            stroke: 0.0,
        }
    }

    /// A convex polygon of 3–8 points, clockwise on screen, its corners rounded by `round`.
    pub fn polygon(points: &[Pt], round: f32) -> Shape {
        assert!((3..=MAX_POLYGON).contains(&points.len()));
        let n = points.len();
        let normal = |i: usize| {
            let e = points[(i + 1) % n] - points[i];
            pt(e.y, -e.x) * (1.0 / e.len())
        };
        let mut inset = [Pt::default(); MAX_POLYGON];
        for (i, slot) in inset.iter_mut().enumerate().take(n) {
            let (n1, n2) = (normal((i + n - 1) % n), normal(i));
            let miter = (n1 + n2) * (round / (1.0 + n1.dot(n2)));
            *slot = points[i] - miter;
        }
        Shape::Polygon {
            points: inset,
            len: n,
            round,
            stroke: 0.0,
        }
    }

    /// This rect or polygon as an outline `width` wide on its edge; other shapes are unchanged.
    pub fn outline(self, width: f32) -> Shape {
        match self {
            Shape::RoundRect {
                left,
                top,
                right,
                bottom,
                radii,
                ..
            } => Shape::RoundRect {
                left,
                top,
                right,
                bottom,
                radii,
                stroke: width / 2.0,
            },
            Shape::Polygon {
                points, len, round, ..
            } => Shape::Polygon {
                points,
                len,
                round,
                stroke: width / 2.0,
            },
            other => other,
        }
    }

    /// Signed distance from `p` to the shape's edge, px (negative inside).
    pub fn distance(&self, p: Pt) -> f32 {
        self.signed(p)
    }

    /// `distance`, inlined into the three loops that measure every px and measured faster with
    /// it (ruling G9): `fill`'s culled fold, the walls' mask and the light page's bake. Through
    /// `distance`, the bake's cold phone frames paint about 3% slower, and `fill`'s and the
    /// walls' cold frames 2–4%. Every other caller, `vitrine::soft_shape` among them (on a
    /// rounded rect most px skip it), shares one copy through `distance`: each inlined copy costs
    /// page bytes (about 0.9 KB gzip for the bake's, 1 KB for `fill`'s and the walls').
    #[inline(always)]
    pub(crate) fn signed(&self, p: Pt) -> f32 {
        match *self {
            Shape::Capsule { a, b, radius } => segment_distance(p, a, b) - radius,
            Shape::Arc {
                centre,
                radius,
                half_width,
                start,
                sweep,
            } => {
                let d = p - centre;
                if sweep >= TAU - 1e-4 {
                    return (d.len() - radius).abs() - half_width;
                }
                let theta = angle_of(d.x, -d.y);
                if (theta - start).rem_euclid(TAU) <= sweep {
                    (d.len() - radius).abs() - half_width
                } else {
                    let e0 = centre + heading(start) * radius;
                    let e1 = centre + heading(start + sweep) * radius;
                    (p - e0).len().min((p - e1).len()) - half_width
                }
            }
            Shape::RoundRect {
                left,
                top,
                right,
                bottom,
                radii,
                stroke,
            } => {
                let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
                let (hx, hy) = ((right - left) / 2.0, (bottom - top) / 2.0);
                let (dx, dy) = (p.x - cx, p.y - cy);
                let r = match (dx < 0.0, dy < 0.0) {
                    (true, true) => radii[0],
                    (false, true) => radii[1],
                    (false, false) => radii[2],
                    (true, false) => radii[3],
                };
                let (qx, qy) = (dx.abs() - hx + r, dy.abs() - hy + r);
                let d = pt(qx.max(0.0), qy.max(0.0)).len() + qx.max(qy).min(0.0) - r;
                if stroke > 0.0 { d.abs() - stroke } else { d }
            }
            Shape::Polygon {
                points,
                len,
                round,
                stroke,
            } => {
                let d = convex_distance(&points[..len], p) - round;
                if stroke > 0.0 { d.abs() - stroke } else { d }
            }
        }
    }

    /// Where coverage can be non-zero: left, top, right, bottom (px).
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        let grow = |l: f32, t: f32, r: f32, b: f32, by: f32| (l - by, t - by, r + by, b + by);
        match *self {
            Shape::Capsule { a, b, radius } => grow(
                a.x.min(b.x),
                a.y.min(b.y),
                a.x.max(b.x),
                a.y.max(b.y),
                radius + 0.5,
            ),
            Shape::Arc {
                centre,
                radius,
                half_width,
                start,
                sweep,
            } => {
                let reach = half_width + 0.5;
                if sweep >= TAU - 1e-4 {
                    return grow(centre.x, centre.y, centre.x, centre.y, radius + reach);
                }
                // A part arc: every covered pixel is within `reach` of its centre line, whose
                // box holds its two ends and each compass point it passes (as `distance` tests
                // the sweep).
                let (mut l, mut t, mut r, mut b) = (
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                );
                let compass = (0..4u8).map(|k| f32::from(k) * FRAC_PI_2);
                let passed = compass.filter(|&a| (a - start).rem_euclid(TAU) <= sweep);
                for a in [start, start + sweep].into_iter().chain(passed) {
                    let p = centre + heading(a) * radius;
                    (l, t, r, b) = (l.min(p.x), t.min(p.y), r.max(p.x), b.max(p.y));
                }
                grow(l, t, r, b, reach)
            }
            Shape::RoundRect {
                left,
                top,
                right,
                bottom,
                stroke,
                ..
            } => grow(left, top, right, bottom, stroke + 0.5),
            Shape::Polygon {
                points,
                len,
                round,
                stroke,
            } => {
                let (mut l, mut t, mut r, mut b) = (
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                );
                for p in &points[..len] {
                    l = l.min(p.x);
                    t = t.min(p.y);
                    r = r.max(p.x);
                    b = b.max(p.y);
                }
                grow(l, t, r, b, round + stroke + 0.5)
            }
        }
    }

    /// The x ranges on the row whose pixel centres are at `y` where coverage can be non-zero:
    /// one, or two flanks where the row crosses a hollow shape's hole (ruling G5). Exact for
    /// capsules (so a diagonal beam costs its own area), the outer circle's chord within the
    /// bounds for arcs (so a quarter arc costs about its own quarter), the bounds otherwise; a
    /// full ring and a rounded rect's outline leave out their holes, every px of which is at
    /// least `stroke + 1.5` px from the stroke (so `fill`'s 1-px growth still measures nothing
    /// there that could be covered). The two flanks are more than 2 px apart, so grown by 1 px
    /// they never overlap.
    pub fn row_spans(&self, y: f32) -> [Option<(f32, f32)>; 2] {
        let (l, t, r, b) = self.bounds();
        if y < t || y > b {
            return [None, None];
        }
        // The row's x range (lo, hi) less the hole `cx ± w_in`, when the hole is wider than 2 px.
        let flanks = |(lo, hi): (f32, f32), cx: f32, w_in: f32| {
            if w_in > 1.0 && lo < cx - w_in && cx + w_in < hi {
                [Some((lo, cx - w_in)), Some((cx + w_in, hi))]
            } else {
                [Some((lo, hi)), None]
            }
        };
        match *self {
            Shape::Capsule { a, b, radius } => [capsule_span(a, b, radius + 0.5, y), None],
            Shape::Arc {
                centre,
                radius,
                half_width,
                sweep,
                ..
            } => {
                let (reach, dy) = (radius + half_width + 0.5, y - centre.y);
                let w = (reach * reach - dy * dy).max(0.0).sqrt();
                let (lo, hi) = ((centre.x - w).max(l), (centre.x + w).min(r));
                // (False for NaN too: nothing drawn, as before.)
                let ordered = lo <= hi;
                if !ordered {
                    return [None, None];
                }
                // A full ring's hole: every point nearer its centre than `radius − half_width −
                // 1.5` is at least 1.5 px inside the stroke's inner edge.
                let inner = radius - half_width - 1.5;
                if sweep >= TAU - 1e-4 && dy.abs() < inner {
                    flanks((lo, hi), centre.x, (inner * inner - dy * dy).sqrt())
                } else {
                    [Some((lo, hi)), None]
                }
            }
            Shape::RoundRect {
                left,
                top,
                right,
                bottom,
                radii,
                stroke,
            } if stroke > 0.0 => {
                // The hole is the rounded rect inset by `s`: half sizes `h − s`, its corners of
                // radius `r − s` (square when `r ≤ s`). Taking the largest radius at every corner
                // gives a smaller hole (rounded rects of one size nest by radius).
                let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
                let (hx, hy) = ((right - left) / 2.0, (bottom - top) / 2.0);
                let (s, dy) = (stroke + 1.5, (y - cy).abs());
                let r_max = radii.into_iter().fold(0.0f32, f32::max);
                let round = (r_max - s).max(0.0);
                // The hole's straight band, then its corner rows (when `r_max > s`).
                let band = hy - s - round;
                // (Radii past the half sizes, which `round_rect` never makes, keep one span.)
                let w_in = if r_max > hx.min(hy) {
                    0.0
                } else if dy <= band {
                    hx - s
                } else if dy < hy - s {
                    let e = dy - band;
                    hx - s - round + (round * round - e * e).max(0.0).sqrt()
                } else {
                    0.0
                };
                flanks((l, r), cx, w_in)
            }
            _ => [Some((l, r)), None],
        }
    }

    /// `row_spans` as one range: their hull.
    #[cfg(test)]
    pub fn row_span(&self, y: f32) -> Option<(f32, f32)> {
        match self.row_spans(y) {
            [Some((a, _)), Some((_, b))] => Some((a, b)),
            [one, _] => one,
        }
    }
}

/// The x range on row `y` of the points within `reach` of segment `a`–`b`: the hull of the two
/// end discs and the band between them.
fn capsule_span(a: Pt, b: Pt, reach: f32, y: f32) -> Option<(f32, f32)> {
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut take = |x: f32| {
        lo = lo.min(x);
        hi = hi.max(x);
    };
    for c in [a, b] {
        let dy = y - c.y;
        if dy.abs() <= reach {
            let w = (reach * reach - dy * dy).sqrt();
            take(c.x - w);
            take(c.x + w);
        }
    }
    let axis = b - a;
    let length = axis.len();
    if length > 0.0 {
        let n = pt(-axis.y, axis.x) * (reach / length);
        let quad = [a + n, b + n, b - n, a - n];
        for (i, &p) in quad.iter().enumerate() {
            let q = quad[(i + 1) % 4];
            if (p.y - y) * (q.y - y) <= 0.0 {
                if (q.y - p.y).abs() < 1e-6 {
                    take(p.x);
                    take(q.x);
                } else {
                    take(p.x + (y - p.y) * (q.x - p.x) / (q.y - p.y));
                }
            }
        }
    }
    (lo <= hi).then_some((lo, hi))
}

fn segment_distance(p: Pt, a: Pt, b: Pt) -> f32 {
    let (ab, ap) = (b - a, p - a);
    let len2 = ab.dot(ab);
    let t = if len2 > 0.0 {
        (ap.dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (ap - ab * t).len()
}

/// Signed distance to a convex polygon given clockwise on screen.
fn convex_distance(points: &[Pt], p: Pt) -> f32 {
    let n = points.len();
    let (mut outside, mut edge) = (f32::NEG_INFINITY, f32::INFINITY);
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        let e = b - a;
        let side = (p - a).dot(pt(e.y, -e.x)) / e.len();
        outside = outside.max(side);
        edge = edge.min(segment_distance(p, a, b));
    }
    if outside <= 0.0 { outside } else { edge }
}

/// Up to four gradient stops, interpolated in straight alpha, clamped beyond the ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stops {
    len: usize,
    at: [f32; 4],
    colour: [Rgba; 4],
}

impl Stops {
    pub fn new(stops: &[(f32, Rgba)]) -> Stops {
        assert!((1..=4).contains(&stops.len()));
        let mut s = Stops {
            len: stops.len(),
            at: [0.0; 4],
            colour: [Rgba::CLEAR; 4],
        };
        for (i, &(at, colour)) in stops.iter().enumerate() {
            s.at[i] = at;
            s.colour[i] = colour;
        }
        s
    }

    pub fn sample(&self, t: f32) -> Rgba {
        let last = self.len - 1;
        if t <= self.at[0] {
            return self.colour[0];
        }
        if t >= self.at[last] {
            return self.colour[last];
        }
        let mut i = 0;
        while t > self.at[i + 1] {
            i += 1;
        }
        let span = self.at[i + 1] - self.at[i];
        let k = if span > 0.0 {
            (t - self.at[i]) / span
        } else {
            1.0
        };
        let (a, b) = (self.colour[i], self.colour[i + 1]);
        let mix = |x: f32, y: f32| x + (y - x) * k;
        Rgba {
            r: mix(a.r, b.r),
            g: mix(a.g, b.g),
            b: mix(a.b, b.b),
            a: mix(a.a, b.a),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    Solid(Rgba),
    /// Projected on `from → to`: 0 at `from`, 1 at `to`.
    Linear {
        from: Pt,
        to: Pt,
        stops: Stops,
    },
    /// 0 at `centre`, 1 at `radius`.
    Radial {
        centre: Pt,
        radius: f32,
        stops: Stops,
    },
}

impl Paint {
    pub fn at(&self, p: Pt) -> Rgba {
        match self {
            Paint::Solid(c) => *c,
            Paint::Linear { from, to, stops } => {
                let d = *to - *from;
                let len2 = d.dot(d);
                stops.sample(if len2 > 0.0 {
                    (p - *from).dot(d) / len2
                } else {
                    0.0
                })
            }
            Paint::Radial {
                centre,
                radius,
                stops,
            } => stops.sample(if *radius > 0.0 {
                (p - *centre).len() / radius
            } else {
                1.0
            }),
        }
    }
}

/// How a colour lands: source-over, added (Canvas `lighter`), or multiplied (Canvas `multiply`
/// over an opaque backdrop: the light theme's ink lettering, spec §10). A premultiplied sprite
/// or a light plane takes `Multiply` as `Over`: nothing draws it there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Over,
    Add,
    Multiply,
}

/// Something `fill` draws into, in its own pixels.
pub trait Target {
    fn size(&self) -> (usize, usize);
    /// Blends `c` (straight alpha, already scaled by coverage) into pixel (`x`, `y`).
    fn blend(&mut self, x: usize, y: usize, c: Rgba, blend: Blend);
    /// Blends `colours[i]` into px (`x0 + i`, `y`) as `blend` does, skipping any with zero alpha
    /// (ruling G4: one call per span, not per px; every target overrides it with its own loop).
    fn blend_span(&mut self, x0: usize, y: usize, colours: &[Rgba], blend: Blend) {
        for (i, &c) in colours.iter().enumerate() {
            if c.a > 0.0 {
                self.blend(x0 + i, y, c, blend);
            }
        }
    }
}

/// `v.round()` (half away from zero) through `trunc`, which wasm has as an instruction (its
/// `round` is a libm call, ruling G3). Exact for every f32: below 2²³ the fraction `v − t` is
/// exact; past it `v` is whole and the fraction 0; NaN and ±0 come back as `round` gives them
/// (a step of ±0 keeps `t`'s sign). Branchless: px values make the step unpredictable, and
/// branches on it made a web drag frame 35% slower than libm's `roundf`. Compiled everywhere,
/// so its test runs natively; only wasm calls it.
#[inline(always)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn round_by_trunc(v: f32) -> f32 {
    let t = v.trunc();
    let step = f32::from(u8::from((v - t).abs() >= 0.5));
    t + step.copysign(v)
}

/// `v.round()`, as one instruction on every target this ships to (ruling G3).
#[inline(always)]
pub fn round_away(v: f32) -> f32 {
    #[cfg(target_arch = "wasm32")]
    return round_by_trunc(v);
    #[cfg(not(target_arch = "wasm32"))]
    return v.round();
}

/// `round_away(v).clamp(0.0, 255.0) as u32`, a code value, in fewer steps: `as` already takes
/// everything below 0 (and NaN) to 0, so only the top needs a bound, and natively the round and
/// the conversion are one instruction.
#[inline(always)]
pub(crate) fn code_value(v: f32) -> u32 {
    (round_away(v) as u32).min(255)
}

#[inline(always)]
fn channel(dst: u32, src: f32, a: f32, blend: Blend) -> u32 {
    let (d, s) = (dst as f32, src * 255.0);
    let v = match blend {
        Blend::Over => s * a + d * (1.0 - a),
        Blend::Add => d + s * a,
        Blend::Multiply => d * (1.0 - a + a * src),
    };
    code_value(v)
}

/// An opaque `0x00RRGGBB` layer: `base`, the macOS canvas.
pub struct Layer<'a> {
    pub px: &'a mut [u32],
    pub width: usize,
    pub height: usize,
}

/// `d` with `c` blended in: `Layer`'s px, one at a time or by spans.
#[inline(always)]
fn layer_px(d: u32, c: Rgba, blend: Blend) -> u32 {
    channel(d >> 16 & 0xff, c.r, c.a, blend) << 16
        | channel(d >> 8 & 0xff, c.g, c.a, blend) << 8
        | channel(d & 0xff, c.b, c.a, blend)
}

impl Target for Layer<'_> {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    fn blend(&mut self, x: usize, y: usize, c: Rgba, blend: Blend) {
        let i = y * self.width + x;
        self.px[i] = layer_px(self.px[i], c, blend);
    }
    fn blend_span(&mut self, x0: usize, y: usize, colours: &[Rgba], blend: Blend) {
        let row = y * self.width + x0;
        for (d, &c) in self.px[row..row + colours.len()].iter_mut().zip(colours) {
            if c.a > 0.0 {
                *d = layer_px(*d, c, blend);
            }
        }
    }
}

/// Opaque RGBA bytes: the web's `ImageData`. Alpha stays 255.
pub struct RgbaBytes<'a> {
    pub px: &'a mut [u8],
    pub width: usize,
    pub height: usize,
}

/// `d` (RGBA bytes) with `c` blended in, alpha 255: `RgbaBytes`' px.
#[inline(always)]
fn rgba_px(d: &mut [u8; 4], c: Rgba, blend: Blend) {
    for (k, s) in [c.r, c.g, c.b].into_iter().enumerate() {
        d[k] = channel(u32::from(d[k]), s, c.a, blend) as u8;
    }
    d[3] = 0xff;
}

impl Target for RgbaBytes<'_> {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    fn blend(&mut self, x: usize, y: usize, c: Rgba, blend: Blend) {
        let i = (y * self.width + x) * 4;
        let d: &mut [u8; 4] = (&mut self.px[i..i + 4]).try_into().unwrap();
        rgba_px(d, c, blend);
    }
    fn blend_span(&mut self, x0: usize, y: usize, colours: &[Rgba], blend: Blend) {
        let row = (y * self.width + x0) * 4;
        let (px, _) = self.px[row..row + 4 * colours.len()].as_chunks_mut::<4>();
        for (d, &c) in px.iter_mut().zip(colours) {
            if c.a > 0.0 {
                rgba_px(d, c, blend);
            }
        }
    }
}

/// A premultiplied `0xAARRGGBB` square: a cached sprite.
pub struct SpriteCanvas<'a> {
    pub px: &'a mut [u32],
    pub size: usize,
}

/// `d` (premultiplied) with `c` blended in: `SpriteCanvas`' px.
#[inline(always)]
fn sprite_px(d: u32, c: Rgba, blend: Blend) -> u32 {
    let (da, a) = ((d >> 24) as f32, c.a);
    let mix = |dv: u32, s: f32| {
        let s = s * 255.0 * a;
        let v = match blend {
            Blend::Over | Blend::Multiply => s + dv as f32 * (1.0 - a),
            Blend::Add => dv as f32 + s,
        };
        code_value(v)
    };
    let alpha = match blend {
        Blend::Over | Blend::Multiply => a * 255.0 + da * (1.0 - a),
        Blend::Add => da + a * 255.0,
    };
    code_value(alpha) << 24
        | mix(d >> 16 & 0xff, c.r) << 16
        | mix(d >> 8 & 0xff, c.g) << 8
        | mix(d & 0xff, c.b)
}

impl Target for SpriteCanvas<'_> {
    fn size(&self) -> (usize, usize) {
        (self.size, self.size)
    }
    fn blend(&mut self, x: usize, y: usize, c: Rgba, blend: Blend) {
        let i = y * self.size + x;
        self.px[i] = sprite_px(self.px[i], c, blend);
    }
    fn blend_span(&mut self, x0: usize, y: usize, colours: &[Rgba], blend: Blend) {
        let row = y * self.size + x0;
        for (d, &c) in self.px[row..row + colours.len()].iter_mut().zip(colours) {
            if c.a > 0.0 {
                *d = sprite_px(*d, c, blend);
            }
        }
    }
}

/// Colours for a span test: every fifth transparent, the rest across the range, from `seed`.
#[cfg(test)]
pub(crate) fn span_colours(n: usize, mut seed: u32) -> Vec<Rgba> {
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 1001) as f32 / 1000.0
    };
    (0..n)
        .map(|i| Rgba {
            r: next(),
            g: next(),
            b: next(),
            a: if i % 5 == 0 { 0.0 } else { next() },
        })
        .collect()
}

/// The part of `a` inside `b`, or `None` when nothing is left.
pub fn clip_rect(a: DeviceRect, b: DeviceRect) -> Option<DeviceRect> {
    let r = DeviceRect::new(
        a.left.max(b.left),
        a.top.max(b.top),
        a.right.min(b.right),
        a.bottom.min(b.bottom),
    );
    (r.left < r.right && r.top < r.bottom).then_some(r)
}

/// The most shapes a row culls: a row of `fill` with more near shapes than this measures
/// every shape at every pixel it walks.
const NEAR_SHAPES: usize = 64;

/// The most px `fill` and the soft shapes gather before blending them by one `blend_span`.
pub(crate) const SPAN: usize = 64;

// The pixels `fill` has measured on this thread (each test runs on its own thread).
#[cfg(test)]
thread_local! {
    static VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Fills the union of `shapes` with `paint` at `alpha`, only inside `clip` (and the target).
pub fn fill(
    target: &mut dyn Target,
    clip: DeviceRect,
    shapes: &[Shape],
    paint: &Paint,
    alpha: f32,
    blend: Blend,
) {
    let (w, h) = target.size();
    let Some(clip) = clip_rect(clip, DeviceRect::new(0, 0, w as i32, h as i32)) else {
        return;
    };
    if shapes.is_empty() || alpha <= 0.0 {
        return;
    }
    let (mut top, mut bottom) = (f32::INFINITY, f32::NEG_INFINITY);
    for s in shapes {
        let (_, t, _, b) = s.bounds();
        top = top.min(t);
        bottom = bottom.max(b);
    }
    let y0 = (top.floor() as i32).max(clip.top);
    // Saturating: a shape at ±∞ or ±1e30 px draws nothing and never overflows (ruling G13).
    let y1 = (bottom.ceil() as i32).saturating_add(1).min(clip.bottom);
    // A row's shapes with their spans grown by 1 px: a pixel more than 1 px outside a shape's
    // span is at distance > 1.5 from it, so it changes no coverage and is not measured (the
    // result is the same min).
    let mut near = [(0.0f32, 0.0f32, 0u16); NEAR_SHAPES];
    let mut spans = [(0i32, 0i32); NEAR_SHAPES];
    let mut run = [Rgba::CLEAR; SPAN];
    for y in y0..y1 {
        let cy = y as f32 + 0.5;
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut n, mut crowded) = (0, false);
        // Up to two entries a shape (a hollow one's flanks, ruling G5), each counted.
        for (i, s) in shapes.iter().enumerate() {
            for (a, b) in s.row_spans(cy).into_iter().flatten() {
                lo = lo.min(a);
                hi = hi.max(b);
                match u16::try_from(i) {
                    Ok(k) if n < NEAR_SHAPES => {
                        near[n] = (a - 1.0, b + 1.0, k);
                        n += 1;
                    }
                    _ => crowded = true,
                }
            }
        }
        if lo > hi {
            continue;
        }
        // A row with more near shapes than the array holds measures every shape (the same
        // minimum: a shape more than 1 px outside its span changes no coverage).
        let culled = !crowded;
        let near = &near[..n];
        let walks = if culled {
            // Walk only the near spans' pixels, sorted and merged (insertion sort of ≤ 64, no
            // allocation): a pixel outside every grown span measures no shape, so it would get
            // no coverage anyway. A union then costs its shapes' area, not the row between them.
            // A pixel whose centre is in [a, b] lies in floor(a)..ceil(b): for one shape, the
            // same pixels the whole-row walk visits.
            for (k, &(a, b, _)) in near.iter().enumerate() {
                let s = (
                    (a.floor() as i32).max(clip.left),
                    (b.ceil() as i32).min(clip.right),
                );
                let mut j = k;
                while j > 0 && spans[j - 1].0 > s.0 {
                    spans[j] = spans[j - 1];
                    j -= 1;
                }
                spans[j] = s;
            }
            n
        } else {
            spans[0] = (
                (lo.floor() as i32).saturating_sub(1).max(clip.left),
                (hi.ceil() as i32).saturating_add(1).min(clip.right),
            );
            1
        };
        let mut k = 0;
        while k < walks {
            let (x0, mut x1) = spans[k];
            k += 1;
            while k < walks && spans[k].0 <= x1 {
                x1 = x1.max(spans[k].1);
                k += 1;
            }
            // The walk's colours, a chunk at a time, blended by one call per chunk (ruling G4);
            // an uncovered px is clear, which `blend_span` skips.
            let (mut start, mut len) = (x0, 0);
            for x in x0..x1 {
                #[cfg(test)]
                VISITS.with(|v| v.set(v.get() + 1));
                let p = pt(x as f32 + 0.5, cy);
                let d = if culled {
                    near.iter()
                        .filter(|&&(a, b, _)| a <= p.x && p.x <= b)
                        .fold(f32::INFINITY, |m, &(_, _, i)| {
                            m.min(shapes[usize::from(i)].signed(p))
                        })
                } else {
                    shapes
                        .iter()
                        .fold(f32::INFINITY, |m, s| m.min(s.distance(p)))
                };
                let coverage = (0.5 - d).clamp(0.0, 1.0);
                run[len] = if coverage > 0.0 {
                    let mut c = paint.at(p);
                    c.a *= coverage * alpha;
                    c
                } else {
                    Rgba::CLEAR
                };
                len += 1;
                if len == SPAN {
                    target.blend_span(start as usize, y as usize, &run, blend);
                    (start, len) = (x + 1, 0);
                }
            }
            if len > 0 {
                target.blend_span(start as usize, y as usize, &run[..len], blend);
            }
        }
    }
}

/// Layer px `d` with premultiplied sprite px `p` over it: `p` itself where opaque (its keep is 0),
/// `d` where clear, else `p + d·(255 − a)/255` rounded, per channel.
#[inline(always)]
fn over_sprite(d: u32, p: u32) -> u32 {
    let a = p >> 24;
    if a == 255 {
        return p & 0x00ff_ffff;
    }
    if a == 0 {
        return d;
    }
    let keep = 255 - a;
    let ch = |shift: u32| ((p >> shift & 0xff) + div255((d >> shift & 0xff) * keep + 127)).min(255);
    ch(16) << 16 | ch(8) << 8 | ch(0)
}

/// `v / 255` for `v` ≤ 255·255 + 127 (every product here), in shifts and adds so it vectorises.
#[inline(always)]
pub(crate) fn div255(v: u32) -> u32 {
    (v + 1 + (v >> 8)) >> 8
}

/// Draws premultiplied `sprite` (`size × size`) with its top-left at (`left`, `top`), over the
/// layer, only inside `clip`.
pub fn blit_sprite(
    layer: &mut Layer,
    clip: DeviceRect,
    sprite: &[u32],
    size: usize,
    left: i32,
    top: i32,
) {
    let s = size as i32;
    let full = DeviceRect::new(0, 0, layer.width as i32, layer.height as i32);
    let Some(r) = clip_rect(DeviceRect::new(left, top, left + s, top + s), full)
        .and_then(|r| clip_rect(r, clip))
    else {
        return;
    };
    for y in r.top..r.bottom {
        let src =
            &sprite[((y - top) * s + (r.left - left)) as usize..][..(r.right - r.left) as usize];
        let row = y as usize * layer.width;
        for (dst, &p) in layer.px[row + r.left as usize..row + r.right as usize]
            .iter_mut()
            .zip(src)
        {
            *dst = over_sprite(*dst, p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spectrum::Rgb;

    /// Ruling G4: a span blends exactly as its px one by one, zero alphas untouched, in every
    /// target `etch` owns and every blend.
    #[test]
    fn spans_blend_like_single_px_in_every_target() {
        for blend in [Blend::Over, Blend::Add, Blend::Multiply] {
            let colours = span_colours(150, 9);
            let start: Vec<u32> = (0..200u32)
                .map(|i| i.wrapping_mul(0x9e37_79b9) & 0x00ff_ffff)
                .collect();
            let (mut one, mut span) = (start.clone(), start.clone());
            let (w, h) = (200, 1);
            for (i, &c) in colours.iter().enumerate() {
                if c.a > 0.0 {
                    Layer {
                        px: &mut one,
                        width: w,
                        height: h,
                    }
                    .blend(30 + i, 0, c, blend);
                }
            }
            Layer {
                px: &mut span,
                width: w,
                height: h,
            }
            .blend_span(30, 0, &colours, blend);
            assert!(one == span, "Layer {blend:?}");
            assert!(one != start, "Layer {blend:?} drew");

            // RgbaBytes: alpha stays 255 where drawn, and only there.
            let bytes: Vec<u8> = start
                .iter()
                .flat_map(|p| (p | 0x7f00_0000).to_le_bytes())
                .collect();
            let (mut one, mut span) = (bytes.clone(), bytes.clone());
            for (i, &c) in colours.iter().enumerate() {
                if c.a > 0.0 {
                    RgbaBytes {
                        px: &mut one,
                        width: w,
                        height: h,
                    }
                    .blend(30 + i, 0, c, blend);
                }
            }
            RgbaBytes {
                px: &mut span,
                width: w,
                height: h,
            }
            .blend_span(30, 0, &colours, blend);
            assert!(one == span, "RgbaBytes {blend:?}");
            assert!(one != bytes, "RgbaBytes {blend:?} drew");

            // SpriteCanvas (premultiplied, 15 × 15): a span along row 7 from column 2.
            let sprite: Vec<u32> = (0..225u32)
                .map(|i| {
                    let a = i.wrapping_mul(0x9e37_79b9) >> 24;
                    let v = a * (i % 7) / 7;
                    a << 24 | v << 16 | (a / 2) << 8 | (a / 3)
                })
                .collect();
            let colours = &colours[..13];
            let (mut one, mut span) = (sprite.clone(), sprite.clone());
            for (i, &c) in colours.iter().enumerate() {
                if c.a > 0.0 {
                    SpriteCanvas {
                        px: &mut one,
                        size: 15,
                    }
                    .blend(2 + i, 7, c, blend);
                }
            }
            SpriteCanvas {
                px: &mut span,
                size: 15,
            }
            .blend_span(2, 7, colours, blend);
            assert!(one == span, "SpriteCanvas {blend:?}");
            assert!(one != sprite, "SpriteCanvas {blend:?} drew");
        }
    }

    /// Ruling G5: outlines and rings, thin and thick, small and large, at fractional places, fill
    /// exactly as every px's nearest-shape measure gives.
    #[test]
    fn hollow_shapes_fill_like_their_nearest_one() {
        let mut shapes = Vec::new();
        for (k, size) in [3.0f32, 7.5, 18.25, 40.0, 61.7].into_iter().enumerate() {
            let o = 2.3 + k as f32 * 0.37;
            for stroke in [0.6f32, 1.0, 2.5, 6.0] {
                let rr = Shape::round_rect(o, o + 1.0, o + size, o + 1.0 + size * 0.8, size * 0.16);
                shapes.push(rr.outline(stroke));
                shapes.push(Shape::ring(
                    pt(o + size / 2.0, o + size / 2.0),
                    size * 0.4,
                    stroke,
                ));
            }
        }
        // Deep corners (the hole's own round corner rows), uneven corners, and shapes crossing
        // the canvas's edges.
        for stroke in [0.6f32, 2.5] {
            let deep = Shape::round_rect(6.3, 4.9, 66.2, 51.4, 22.7).outline(stroke);
            shapes.push(deep);
            if let Shape::RoundRect { radii, .. } = &mut shapes.last_mut().unwrap() {
                *radii = [22.7, 3.0, 0.0, 14.1];
            }
            shapes.push(deep);
            // Radii past the half sizes (as `RoundRect` may be built, not by `round_rect`).
            shapes.push(deep);
            if let Shape::RoundRect { radii, .. } = &mut shapes.last_mut().unwrap() {
                *radii = [40.0, 0.0, 33.0, 0.0];
            }
            shapes.push(Shape::round_rect(-20.4, 30.2, 40.1, 90.0, 9.0).outline(stroke));
            shapes.push(Shape::ring(pt(4.0, 66.5), 25.3, stroke));
        }
        for s in &shapes {
            let (px, want) = filled_and_nearest(std::slice::from_ref(s), 72);
            assert!(px == want, "{s:?}");
        }
        // And all together (overlapping holes and rims).
        let (px, want) = filled_and_nearest(&shapes[..12], 72);
        assert!(px == want);
    }

    /// Ruling G5: a large outline and a large ring walk their rims, not their holes.
    #[test]
    fn a_hollow_shape_walks_only_its_rim() {
        let mut px = vec![0u32; 300 * 300];
        let mut layer = Layer {
            px: &mut px,
            width: 300,
            height: 300,
        };
        let all = DeviceRect::new(0, 0, 300, 300);
        let paint = Paint::Solid(Rgb(255, 255, 255).opaque());
        VISITS.with(|v| v.set(0));
        let outline = Shape::round_rect(20.0, 20.0, 280.0, 280.0, 30.0).outline(2.0);
        fill(&mut layer, all, &[outline], &paint, 1.0, Blend::Over);
        let ring = Shape::ring(pt(150.0, 150.0), 120.0, 3.0);
        fill(&mut layer, all, &[ring], &paint, 1.0, Blend::Over);
        let visits = VISITS.with(|v| v.get());
        // Measured (ruling P1): 117 708 px before holes (rect 70 224, ring 47 484); with them
        // 12 012 (rect 6 812, its corner rows' holes rounded; ring 5 200). Corner rows walked
        // whole would give about 25 000.
        assert!(visits < 14_000, "{visits} px measured");
    }

    /// Ruling G4 at a chunk's edges: walks of 62–67 px (one chunk, exactly one, one and a px),
    /// and a clip cutting each in two, fill as every px's nearest-shape measure gives.
    #[test]
    fn walks_across_a_chunk_edge_fill_like_the_nearest_shape() {
        for width in 60..=65 {
            // A rect `width` px wide walks `width + 2` px or so (its span grown by 1 px).
            let left = 3.25 + width as f32 * 0.01;
            let shapes = [Shape::round_rect(left, 2.5, left + width as f32, 9.5, 2.0)];
            let (px, want) = filled_and_nearest(&shapes, 72);
            assert!(px == want, "{width} px");
            assert!(px.iter().any(|&p| p != 0), "{width} px drew");
            // The same in two clips, cut at every column across the first chunk's end.
            for cut in [30, 63, 64, 65, 66] {
                let mut halves = vec![0u32; 72 * 72];
                let mut layer = Layer {
                    px: &mut halves,
                    width: 72,
                    height: 72,
                };
                let paint = Paint::Solid(Rgb(255, 255, 255).alpha(0.6));
                for clip in [
                    DeviceRect::new(0, 0, cut, 72),
                    DeviceRect::new(cut, 0, 72, 72),
                ] {
                    fill(&mut layer, clip, &shapes, &paint, 1.0, Blend::Over);
                }
                assert!(halves == want, "{width} px cut at {cut}");
            }
        }
    }

    #[test]
    fn dividing_by_255_in_shifts_is_exact_for_every_product() {
        for v in 0..=255 * 255 + 127 {
            assert_eq!(div255(v), v / 255, "{v}");
        }
    }

    /// A blit's px are the plain formula's, `p + (d·(255 − a) + 127) / 255` per channel
    /// (capped), for 2 000 random pairs: premultiplied or not, clear, opaque and between.
    #[test]
    fn blits_match_the_plain_formula() {
        let plain = |d: u32, p: u32| {
            let a = p >> 24;
            if a == 0 {
                return d;
            }
            let keep = 255 - a;
            let ch = |shift: u32| {
                ((p >> shift & 0xff) + ((d >> shift & 0xff) * keep + 127) / 255).min(255)
            };
            ch(16) << 16 | ch(8) << 8 | ch(0)
        };
        let mut seed = 0x1234_5678u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let (mut sprite, mut layer) = (Vec::new(), Vec::new());
        for k in 0..2000 {
            let (p, d) = (next(), next());
            let p = match k % 4 {
                0 => p & 0x00ff_ffff,
                1 => p | 0xff00_0000,
                _ => p,
            };
            sprite.push(p);
            layer.push(d);
        }
        let want: Vec<u32> = layer
            .iter()
            .zip(&sprite)
            .map(|(&d, &p)| plain(d, p))
            .collect();
        let mut px = layer.clone();
        let mut target = Layer {
            px: &mut px,
            width: 2000,
            height: 1,
        };
        // The 2 000 px as one row of 2 000 one-px sprites: each blit a px.
        for (x, p) in sprite.iter().enumerate() {
            let clip = DeviceRect::new(0, 0, 2000, 1);
            blit_sprite(&mut target, clip, std::slice::from_ref(p), 1, x as i32, 0);
        }
        assert!(px == want);
        // And as one 40 × 40 sprite (its rows, clipped to the layer), over the same px.
        let mut px = layer[..1600].to_vec();
        let mut target = Layer {
            px: &mut px,
            width: 40,
            height: 40,
        };
        blit_sprite(
            &mut target,
            DeviceRect::new(0, 0, 40, 40),
            &sprite[..1600],
            40,
            0,
            0,
        );
        assert!(px[..] == want[..1600]);
    }

    /// Ruling G3: the trunc form rounds exactly as `f32::round`, sign and NaN included.
    #[test]
    fn rounding_by_trunc_is_round() {
        let same = |v: f32| {
            let (a, b) = (round_by_trunc(v), v.round());
            a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
        };
        // Every 97th bit pattern: every exponent, both signs, NaN and the infinities.
        let mut bits = 0u32;
        loop {
            assert!(same(f32::from_bits(bits)), "{bits:#010x}");
            match bits.checked_add(97) {
                Some(b) => bits = b,
                None => break,
            }
        }
        // Every half and its neighbours up to 2²⁴, where rounding ties live.
        for k in -(1i32 << 24)..=(1 << 24) {
            let h = k as f32 / 2.0;
            for v in [
                h,
                f32::from_bits(h.to_bits().wrapping_add(1)),
                f32::from_bits(h.to_bits().wrapping_sub(1)),
            ] {
                assert!(same(v), "{v}");
            }
        }
    }

    /// `code_value` is the rounded, clamped cast it replaces, both natively and through the
    /// wasm `round_by_trunc`; so are the plain saturating casts to `u8` and `u16`.
    #[test]
    fn code_values_are_clamped_rounds() {
        let check = |v: f32| {
            let clamped = |r: f32, top: f32| r.clamp(0.0, top);
            for r in [round_away(v), round_by_trunc(v)] {
                let (code, plain) = ((r as u32).min(255), clamped(r, 255.0) as u32);
                assert_eq!(code, plain, "{v:?} ({:#010x})", v.to_bits());
                assert_eq!(r as u8, clamped(r, 255.0) as u8, "{v:?}");
                assert_eq!(r as u16, clamped(r, 65_535.0) as u16, "{v:?}");
            }
            assert_eq!(code_value(v), round_away(v).clamp(0.0, 255.0) as u32);
        };
        // Every 97th bit pattern: every exponent, both signs, NaN and the infinities.
        let mut bits = 0u32;
        loop {
            check(f32::from_bits(bits));
            match bits.checked_add(97) {
                Some(b) => bits = b,
                None => break,
            }
        }
        // Every half from −2 to 65 538 and its neighbours, where the bounds and every rounding
        // step lie.
        for k in -4..=131_076 {
            let h = k as f32 / 2.0;
            for v in [h, h.next_up(), h.next_down()] {
                check(v);
            }
        }
    }

    #[test]
    fn the_small_trigonometry_matches_libm() {
        for k in -400..=400 {
            let a = k as f32 * 0.037;
            assert!((sine(a) - a.sin()).abs() < 1e-5, "sin {a}");
            let (x, y) = (a.cos(), a.sin());
            assert!(
                (angle_of(y, x) - y.atan2(x)).abs() < 1e-4 || (a.rem_euclid(TAU) - PI).abs() < 1e-3
            );
        }
    }

    /// Sums coverage: every blend of an opaque solid paint adds its alpha.
    struct Area {
        total: f64,
        w: usize,
        h: usize,
    }
    impl Target for Area {
        fn size(&self) -> (usize, usize) {
            (self.w, self.h)
        }
        fn blend(&mut self, _: usize, _: usize, c: Rgba, _: Blend) {
            self.total += f64::from(c.a);
        }
    }

    fn area(shapes: &[Shape]) -> f64 {
        let mut t = Area {
            total: 0.0,
            w: 200,
            h: 200,
        };
        fill(
            &mut t,
            DeviceRect::new(0, 0, 200, 200),
            shapes,
            &Paint::Solid(Rgb(255, 255, 255).opaque()),
            1.0,
            Blend::Over,
        );
        t.total
    }

    fn near(actual: f64, expected: f64, tolerance: f64, what: &str) {
        assert!(
            (actual - expected).abs() <= expected * tolerance,
            "{what}: {actual} vs {expected}"
        );
    }

    #[test]
    fn coverage_sums_to_the_exact_area() {
        near(
            area(&[Shape::disc(pt(50.3, 40.7), 20.0)]),
            f64::from(PI) * 400.0,
            0.005,
            "disc",
        );
        let (a, b) = (pt(10.0, 10.0), pt(80.0, 60.0));
        let len = f64::from((b - a).len());
        near(
            area(&[Shape::line(a, b, 6.0)]),
            len * 6.0 + f64::from(PI) * 9.0,
            0.005,
            "capsule",
        );
        let rr = Shape::round_rect(10.2, 12.7, 70.6, 50.1, 8.0);
        near(
            area(&[rr]),
            60.4 * 37.4 - (4.0 - f64::from(PI)) * 64.0,
            0.005,
            "round rect",
        );
        let tri = Shape::polygon(&[pt(20.0, 10.0), pt(80.0, 70.0), pt(10.0, 60.0)], 0.0);
        near(
            area(&[tri]),
            0.5 * ((60.0_f64 * 50.0) - (-10.0 * 60.0)).abs(),
            0.005,
            "triangle",
        );
        near(
            area(&[Shape::ring(pt(100.0, 100.0), 30.0, 4.0)]),
            f64::from(TAU) * 30.0 * 4.0,
            0.005,
            "ring",
        );
        let half = Shape::arc(pt(100.0, 100.0), 30.0, 4.0, 0.0, PI);
        near(
            area(&[half]),
            f64::from(PI) * 30.0 * 4.0 + f64::from(PI) * 4.0,
            0.01,
            "half arc with round caps",
        );
    }

    #[test]
    fn a_union_covers_its_seam_once() {
        let mut px = vec![0u32; 100 * 60];
        let mut layer = Layer {
            px: &mut px,
            width: 100,
            height: 60,
        };
        let pair = [
            Shape::rect(10.0, 10.0, 50.0, 50.0),
            Shape::rect(40.0, 10.0, 90.0, 50.0),
        ];
        fill(
            &mut layer,
            DeviceRect::new(0, 0, 100, 60),
            &pair,
            &Paint::Solid(Rgb(255, 255, 255).opaque()),
            0.5,
            Blend::Over,
        );
        assert_eq!(px[30 * 100 + 45], 0x808080, "the overlap is blended once");
        assert_eq!(px[30 * 100 + 20], 0x808080);
    }

    #[test]
    fn spans_hold_every_covered_pixel_and_little_else() {
        let line = Shape::line(pt(5.5, 7.25), pt(93.1, 61.8), 6.6);
        let mut spans = 0.0;
        for y in 0..100 {
            let cy = y as f32 + 0.5;
            let span = line.row_span(cy);
            for x in 0..100 {
                if line.distance(pt(x as f32 + 0.5, cy)) < 0.5 {
                    let (lo, hi) = span.expect("a covered row has a span");
                    assert!(lo <= x as f32 + 0.5 && x as f32 + 0.5 <= hi, "({x}, {y})");
                }
            }
            if let Some((lo, hi)) = span {
                spans += hi - lo;
            }
        }
        assert!(
            spans < 0.25 * 88.0 * 55.0,
            "a diagonal stroke costs about its own area, not its box: {spans}"
        );
    }

    /// `fill` measures only the shapes near each pixel, and paints exactly what measuring every
    /// shape at every pixel paints (the reference below is `fill` before culling).
    #[test]
    fn culling_far_shapes_changes_no_pixel() {
        fn reference(t: &mut dyn Target, shapes: &[Shape], paint: &Paint, blend: Blend) {
            let (w, h) = t.size();
            for y in 0..h {
                for x in 0..w {
                    let p = pt(x as f32 + 0.5, y as f32 + 0.5);
                    let d = shapes
                        .iter()
                        .fold(f32::INFINITY, |m, s| m.min(s.distance(p)));
                    let coverage = (0.5 - d).clamp(0.0, 1.0);
                    let mut c = paint.at(p);
                    c.a *= coverage * 0.8;
                    if coverage > 0.0 && c.a > 0.0 {
                        t.blend(x, y, c, blend);
                    }
                }
            }
        }
        let zig: Vec<Shape> = (0..40)
            .map(|k| {
                let (x, y) = (3.0 + 2.3 * k as f32, 20.0 + 9.0 * (k % 3) as f32);
                Shape::line(pt(x, y), pt(x + 2.9, 50.0 - y * 0.4), 1.7)
            })
            .collect();
        let mixed = [
            Shape::arc(pt(40.0, 40.0), 17.0, 2.5, 0.4, 2.0),
            Shape::round_rect(10.2, 60.7, 52.9, 80.1, 6.0).outline(1.5),
            Shape::disc(pt(70.3, 30.6), 9.1),
            Shape::polygon(&[pt(60.0, 60.0), pt(90.0, 70.0), pt(66.0, 92.0)], 1.0),
            Shape::line(pt(5.0, 95.0), pt(95.0, 5.0), 3.3),
        ];
        let paint = Paint::Solid(Rgb(250, 120, 30).alpha(0.9));
        for (shapes, blend) in [(&zig[..], Blend::Add), (&mixed[..], Blend::Over)] {
            let (mut a, mut b) = (
                vec![0x0020_3040u32; 100 * 100],
                vec![0x0020_3040u32; 100 * 100],
            );
            let mut culled = Layer {
                px: &mut a,
                width: 100,
                height: 100,
            };
            fill(
                &mut culled,
                DeviceRect::new(0, 0, 100, 100),
                shapes,
                &paint,
                0.8,
                blend,
            );
            let mut every = Layer {
                px: &mut b,
                width: 100,
                height: 100,
            };
            reference(&mut every, shapes, &paint, blend);
            assert!(a == b, "{} shapes", shapes.len());
            assert!(a.iter().any(|&p| p != 0x0020_3040), "something is drawn");
        }
    }

    /// A part arc's bounds and spans hold every pixel it covers (so `fill` draws it the same as
    /// over the whole circle's box) and a quarter arc costs about a quarter of the ring's spans.
    #[test]
    fn arc_spans_hold_every_covered_pixel() {
        let centre = pt(60.25, 59.5);
        let spans = |arc: &Shape| {
            let mut sum = 0.0;
            for y in 0..120 {
                let cy = y as f32 + 0.5;
                let span = arc.row_span(cy);
                for x in 0..120 {
                    let cx = x as f32 + 0.5;
                    if arc.distance(pt(cx, cy)) < 0.5 {
                        let (lo, hi) = span.expect("a covered row has a span");
                        assert!(lo <= cx && cx <= hi, "{arc:?}: ({x}, {y})");
                    }
                }
                sum += span.map_or(0.0, |(lo, hi)| hi - lo);
            }
            sum
        };
        let starts = [0.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2, 0.3, 2.9, 5.0, -0.7];
        for start in starts {
            for sweep in [FRAC_PI_2, 1.0, 3.0, 5.5] {
                for (radius, width) in [(27.0, 2.0), (7.3, 4.5), (40.0, 1.0)] {
                    spans(&Shape::arc(centre, radius, width, start, sweep));
                }
            }
        }
        let quarter = spans(&Shape::arc(centre, 27.0, 2.0, 0.0, FRAC_PI_2));
        let ring = spans(&Shape::ring(centre, 27.0, 2.0));
        assert!(
            quarter < 0.3 * ring,
            "a quarter arc: {quarter}, the ring: {ring}"
        );
    }

    #[test]
    fn gradients_interpolate_between_stops_and_clamp() {
        let red = Rgb(255, 0, 0).opaque();
        let green = Rgb(0, 255, 0).opaque();
        let blue = Rgb(0, 0, 255).opaque();
        let paint = Paint::Linear {
            from: pt(0.0, 0.0),
            to: pt(100.0, 0.0),
            stops: Stops::new(&[(0.0, red), (0.5, green), (1.0, blue)]),
        };
        assert_eq!(paint.at(pt(50.0, 7.0)), green);
        let quarter = paint.at(pt(25.0, 0.0));
        assert!(
            (quarter.r - 0.5).abs() < 1e-6 && (quarter.g - 0.5).abs() < 1e-6 && quarter.b == 0.0
        );
        assert_eq!(paint.at(pt(-40.0, 0.0)), red);
        assert_eq!(paint.at(pt(400.0, 0.0)), blue);
        let radial = Paint::Radial {
            centre: pt(10.0, 10.0),
            radius: 10.0,
            stops: Stops::new(&[(0.0, red), (1.0, blue)]),
        };
        assert_eq!(radial.at(pt(10.0, 30.0)), blue);
    }

    #[test]
    fn layers_add_with_saturation_and_blend_over_in_code_values() {
        let mut px = vec![0x00f0f0f0u32, 0x00ffffff];
        let mut layer = Layer {
            px: &mut px,
            width: 2,
            height: 1,
        };
        layer.blend(0, 0, Rgb(255, 255, 255).opaque(), Blend::Add);
        layer.blend(1, 0, Rgb(0, 0, 0).alpha(0.5), Blend::Over);
        assert_eq!(px, [0x00ffffff, 0x00808080]);
    }

    #[test]
    fn multiply_darkens_by_the_source_colour() {
        let mut px = vec![0x00c8_6432u32]; // (200, 100, 50)
        let mut layer = Layer {
            px: &mut px,
            width: 1,
            height: 1,
        };
        let s = Rgb(128, 255, 0);
        layer.blend(0, 0, s.alpha(1.0), Blend::Multiply);
        assert_eq!(px[0], 0x0064_6400, "200·128/255, 100·1, 50·0");
        let mut px = vec![0x00c8_6432u32];
        let mut layer = Layer {
            px: &mut px,
            width: 1,
            height: 1,
        };
        layer.blend(0, 0, s.alpha(0.5), Blend::Multiply);
        // d·(1 − 0.5 + 0.5·s): 200·0.751 = 150.2, 100, 50·0.5 = 25.
        assert_eq!(px[0], 0x0096_6419);
        let mut bytes = [200u8, 100, 50, 0];
        let mut rgba = RgbaBytes {
            px: &mut bytes,
            width: 1,
            height: 1,
        };
        rgba.blend(0, 0, s.alpha(0.5), Blend::Multiply);
        assert_eq!(
            bytes,
            [150, 100, 25, 255],
            "the web's bytes multiply the same"
        );
    }

    /// Review T3 M3: a sprite and a light plane take `Multiply` as `Over` (nothing multiplies
    /// there), so the same blend in either gives the same px.
    #[test]
    fn sprites_and_planes_take_multiply_as_over() {
        let s = Rgb(128, 255, 0).alpha(0.5);
        let sprite = |blend| {
            let mut px = [0x80c8_6432u32];
            let mut canvas = SpriteCanvas {
                px: &mut px,
                size: 1,
            };
            canvas.blend(0, 0, s, blend);
            px[0]
        };
        assert_eq!(sprite(Blend::Multiply), sprite(Blend::Over));
        assert_ne!(sprite(Blend::Over), 0x80c8_6432, "the blend drew");
        let plane = |blend| {
            let mut planes = crate::bloom::Planes::new(1, 1);
            planes.r[0] = 40_000;
            planes.g[0] = 9_000;
            planes.blend(0, 0, s, blend);
            [planes.r[0], planes.g[0], planes.b[0]]
        };
        assert_eq!(plane(Blend::Multiply), plane(Blend::Over));
        assert_ne!(plane(Blend::Over), [40_000, 9_000, 0], "the blend drew");
    }

    /// `shapes` filled over a `size × size` layer, and the same picture from every px's nearest
    /// shape measured directly.
    fn filled_and_nearest(shapes: &[Shape], size: usize) -> (Vec<u32>, Vec<u32>) {
        let colour = Rgb(255, 255, 255).alpha(0.6);
        let mut px = vec![0u32; size * size];
        let mut layer = Layer {
            px: &mut px,
            width: size,
            height: size,
        };
        let all = DeviceRect::new(0, 0, size as i32, size as i32);
        fill(
            &mut layer,
            all,
            shapes,
            &Paint::Solid(colour),
            1.0,
            Blend::Over,
        );
        let mut want = vec![0u32; size * size];
        let mut reference = Layer {
            px: &mut want,
            width: size,
            height: size,
        };
        for y in 0..size {
            for x in 0..size {
                let p = pt(x as f32 + 0.5, y as f32 + 0.5);
                let d = shapes
                    .iter()
                    .fold(f32::INFINITY, |m, s| m.min(s.distance(p)));
                let c = (0.5 - d).clamp(0.0, 1.0);
                if c > 0.0 {
                    reference.blend(x, y, colour.with_alpha(colour.a * c), Blend::Over);
                }
            }
        }
        (px, want)
    }

    /// `count` random lines over a 100 × 100 px square, from `seed`.
    fn random_lines(count: usize, mut seed: u32) -> Vec<Shape> {
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed % 1000) as f32 / 10.0
        };
        (0..count)
            .map(|_| {
                let (a, b) = (pt(next(), next()), pt(next(), next()));
                Shape::line(a, b, 0.5 + next() / 50.0)
            })
            .collect()
    }

    /// Ruling L14(b): with more than `NEAR_SHAPES` shapes, `fill` gives every pixel exactly the
    /// coverage of the nearest shape, as measuring them all would.
    #[test]
    fn many_shapes_fill_like_their_nearest_one() {
        let (px, want) = filled_and_nearest(&random_lines(300, 7), 100);
        assert!(px == want);
    }

    /// Review T3 M1: a few dozen shapes take the culled path on most rows (with several
    /// overlapping, merged spans) and the crowded one on none or few: the same picture.
    #[test]
    fn a_few_dozen_shapes_fill_like_their_nearest_one() {
        for (count, seed) in [(12, 3), (30, 11), (40, 29)] {
            let shapes = random_lines(count, seed);
            let (px, want) = filled_and_nearest(&shapes, 100);
            assert!(px == want, "{count} shapes");
            assert!(px.iter().any(|&p| p != 0), "{count} shapes drew");
        }
    }

    /// Review T3 M2: a shape past index 65 535 (no u16 index) on a row makes the row measure
    /// every shape, so it is drawn like the rest.
    #[test]
    fn a_shape_past_a_u16_index_is_still_drawn() {
        // 65 536 shapes far below the square (never on its rows), then two across it.
        let far = Shape::line(pt(0.0, 1000.0), pt(1.0, 1000.0), 0.5);
        let mut shapes = vec![far; 1 << 16];
        shapes.push(Shape::line(pt(0.0, 2.0), pt(8.0, 6.0), 1.0));
        shapes.push(Shape::line(pt(8.0, 0.0), pt(0.0, 8.0), 0.8));
        let (px, want) = filled_and_nearest(&shapes, 8);
        assert!(px == want);
        assert!(
            px.iter().filter(|&&p| p != 0).count() > 8,
            "both lines drew"
        );
    }

    /// Review finding 1: a row with two short strokes 1000 px apart walks about their widths,
    /// not the 1000 px between them.
    #[test]
    fn a_row_walks_only_its_shapes() {
        let mut px = vec![0u32; 1100 * 4];
        let mut layer = Layer {
            px: &mut px,
            width: 1100,
            height: 4,
        };
        let strokes = [
            Shape::line(pt(10.0, 2.0), pt(14.0, 2.0), 2.0),
            Shape::line(pt(1010.0, 2.0), pt(1014.0, 2.0), 2.0),
        ];
        VISITS.with(|v| v.set(0));
        fill(
            &mut layer,
            DeviceRect::new(0, 0, 1100, 4),
            &strokes,
            &Paint::Solid(Rgb(255, 255, 255).opaque()),
            1.0,
            Blend::Over,
        );
        let visits = VISITS.with(|v| v.get());
        assert!(visits < 4 * 40, "{visits} px visited");
        assert_eq!(px[2 * 1100 + 12], 0x00ff_ffff, "the first stroke is drawn");
        assert_eq!(
            px[2 * 1100 + 1012],
            0x00ff_ffff,
            "the second stroke is drawn"
        );
    }

    #[test]
    fn sprites_are_premultiplied_and_blit_over() {
        let mut sprite = vec![0u32; 16 * 16];
        let mut canvas = SpriteCanvas {
            px: &mut sprite,
            size: 16,
        };
        fill(
            &mut canvas,
            DeviceRect::new(0, 0, 16, 16),
            &[Shape::disc(pt(8.0, 8.0), 6.0)],
            &Paint::Solid(Rgb(255, 0, 0).opaque()),
            0.5,
            Blend::Over,
        );
        assert_eq!(sprite[8 * 16 + 8], 0x80800000, "α 0.5 red, premultiplied");
        let mut px = vec![0x00ffffffu32; 32 * 32];
        let mut layer = Layer {
            px: &mut px,
            width: 32,
            height: 32,
        };
        blit_sprite(
            &mut layer,
            DeviceRect::new(0, 0, 32, 32),
            &sprite,
            16,
            10,
            4,
        );
        assert_eq!(layer.px[12 * 32 + 18], 0x00ff7f7f);
        assert_eq!(
            layer.px[0], 0x00ffffff,
            "transparent sprite pixels leave the layer alone"
        );
        blit_sprite(
            &mut layer,
            DeviceRect::new(0, 0, 32, 32),
            &sprite,
            16,
            -12,
            30,
        );
    }

    #[test]
    fn outlines_and_clips() {
        let ring = Shape::round_rect(10.0, 10.0, 50.0, 50.0, 6.0).outline(2.0);
        assert!(ring.distance(pt(30.0, 30.0)) > 5.0, "an outline is hollow");
        assert!(ring.distance(pt(30.0, 10.0)) < 0.0);
        assert_eq!(
            clip_rect(DeviceRect::new(-5, -5, 3, 2), DeviceRect::new(0, 0, 10, 10)),
            Some(DeviceRect::new(0, 0, 3, 2))
        );
        assert_eq!(
            clip_rect(DeviceRect::new(20, 0, 30, 5), DeviceRect::new(0, 0, 10, 10)),
            None
        );
        let mut px = vec![0u32; 4];
        let mut layer = Layer {
            px: &mut px,
            width: 2,
            height: 2,
        };
        fill(
            &mut layer,
            DeviceRect::new(1, 0, 2, 2),
            &[Shape::rect(-10.0, -10.0, 10.0, 10.0)],
            &Paint::Solid(Rgb(255, 255, 255).opaque()),
            1.0,
            Blend::Over,
        );
        assert_eq!(px, [0, 0xffffff, 0, 0xffffff], "only the clip is touched");
    }

    /// Ruling G13: shapes at absurd coordinates never panic (debug builds check overflow) and a
    /// shape wholly off the canvas draws nothing.
    #[test]
    fn extreme_coordinates_draw_nothing_and_never_panic() {
        const EXTREMES: [f32; 7] = [
            1e30,
            -1e30,
            3e9,
            -3e9,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
        ];
        let mut px = vec![0u32; 64 * 64];
        let all = DeviceRect::new(0, 0, 64, 64);
        let paint = Paint::Solid(Rgb(255, 255, 255).opaque());
        for v in EXTREMES {
            let mut layer = Layer {
                px: &mut px,
                width: 64,
                height: 64,
            };
            for shape in [
                Shape::disc(pt(v, 10.0), 5.0),
                Shape::disc(pt(10.0, v), 5.0),
                Shape::rect(v, v, v, v),
                Shape::ring(pt(v, -v), 4.0, 1.0),
            ] {
                fill(&mut layer, all, &[shape], &paint, 1.0, Blend::Over);
            }
            // Lines from far away may cross the canvas: they only must not panic.
            let line = Shape::line(pt(v, v), pt(10.0, 10.0), 2.0);
            fill(&mut layer, all, &[line], &paint, 1.0, Blend::Over);
        }
        let mut clean = vec![0u32; 64 * 64];
        let mut layer = Layer {
            px: &mut clean,
            width: 64,
            height: 64,
        };
        for v in EXTREMES {
            let shapes = [Shape::disc(pt(v, 10.0), 5.0), Shape::rect(v, v, v, v)];
            fill(&mut layer, all, &shapes, &paint, 1.0, Blend::Over);
            // More near shapes on a row than `fill` culls: the whole-row span from `lo`/`hi`.
            let crowd = [Shape::disc(pt(v, 10.0), 5.0); NEAR_SHAPES + 1];
            fill(&mut layer, all, &crowd, &paint, 1.0, Blend::Over);
        }
        assert!(clean.iter().all(|&p| p == 0), "an off-canvas shape drew");
    }
}
