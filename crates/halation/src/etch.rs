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

    /// The x range on the row whose pixel centres are at `y` where coverage can be non-zero.
    /// Exact for capsules (so a diagonal beam costs its own area), the outer circle's chord within
    /// the bounds for arcs (so a quarter arc costs about its own quarter), the bounds otherwise.
    pub fn row_span(&self, y: f32) -> Option<(f32, f32)> {
        let (l, t, r, b) = self.bounds();
        if y < t || y > b {
            return None;
        }
        match *self {
            Shape::Capsule { a, b, radius } => capsule_span(a, b, radius + 0.5, y),
            Shape::Arc {
                centre,
                radius,
                half_width,
                ..
            } => {
                let (reach, dy) = (radius + half_width + 0.5, y - centre.y);
                let w = (reach * reach - dy * dy).max(0.0).sqrt();
                let (lo, hi) = ((centre.x - w).max(l), (centre.x + w).min(r));
                (lo <= hi).then_some((lo, hi))
            }
            _ => Some((l, r)),
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

/// How a colour lands: source-over, or added (Canvas `lighter`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Over,
    Add,
}

/// Something `fill` draws into, in its own pixels.
pub trait Target {
    fn size(&self) -> (usize, usize);
    /// Blends `c` (straight alpha, already scaled by coverage) into pixel (`x`, `y`).
    fn blend(&mut self, x: usize, y: usize, c: Rgba, blend: Blend);
}

fn channel(dst: u32, src: f32, a: f32, blend: Blend) -> u32 {
    let (d, s) = (dst as f32, src * 255.0);
    let v = match blend {
        Blend::Over => s * a + d * (1.0 - a),
        Blend::Add => d + s * a,
    };
    v.round().clamp(0.0, 255.0) as u32
}

/// An opaque `0x00RRGGBB` layer: `base`, the macOS canvas.
pub struct Layer<'a> {
    pub px: &'a mut [u32],
    pub width: usize,
    pub height: usize,
}

impl Target for Layer<'_> {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    fn blend(&mut self, x: usize, y: usize, c: Rgba, blend: Blend) {
        let i = y * self.width + x;
        let d = self.px[i];
        self.px[i] = channel(d >> 16 & 0xff, c.r, c.a, blend) << 16
            | channel(d >> 8 & 0xff, c.g, c.a, blend) << 8
            | channel(d & 0xff, c.b, c.a, blend);
    }
}

/// Opaque RGBA bytes: the web's `ImageData`. Alpha stays 255.
pub struct RgbaBytes<'a> {
    pub px: &'a mut [u8],
    pub width: usize,
    pub height: usize,
}

impl Target for RgbaBytes<'_> {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    fn blend(&mut self, x: usize, y: usize, c: Rgba, blend: Blend) {
        let i = (y * self.width + x) * 4;
        for (k, s) in [c.r, c.g, c.b].into_iter().enumerate() {
            self.px[i + k] = channel(u32::from(self.px[i + k]), s, c.a, blend) as u8;
        }
        self.px[i + 3] = 0xff;
    }
}

/// A premultiplied `0xAARRGGBB` square: a cached sprite.
pub struct SpriteCanvas<'a> {
    pub px: &'a mut [u32],
    pub size: usize,
}

impl Target for SpriteCanvas<'_> {
    fn size(&self) -> (usize, usize) {
        (self.size, self.size)
    }
    fn blend(&mut self, x: usize, y: usize, c: Rgba, blend: Blend) {
        let i = y * self.size + x;
        let d = self.px[i];
        let (da, a) = ((d >> 24) as f32, c.a);
        let mix = |dv: u32, s: f32| {
            let s = s * 255.0 * a;
            let v = match blend {
                Blend::Over => s + dv as f32 * (1.0 - a),
                Blend::Add => dv as f32 + s,
            };
            v.round().clamp(0.0, 255.0) as u32
        };
        let alpha = match blend {
            Blend::Over => a * 255.0 + da * (1.0 - a),
            Blend::Add => da + a * 255.0,
        };
        self.px[i] = (alpha.round().clamp(0.0, 255.0) as u32) << 24
            | mix(d >> 16 & 0xff, c.r) << 16
            | mix(d >> 8 & 0xff, c.g) << 8
            | mix(d & 0xff, c.b);
    }
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

/// The most shapes `fill` culls per row.
const NEAR_SHAPES: usize = 64;

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
    let y1 = (bottom.ceil() as i32 + 1).min(clip.bottom);
    // A row's shapes with their spans grown by 1 px: a pixel more than 1 px outside a shape's
    // span is at distance > 1.5 from it, so it changes no coverage and is not measured (the
    // result is the same min). More shapes than this measure every shape at every pixel.
    let culled = shapes.len() <= NEAR_SHAPES;
    let mut near = [(0.0f32, 0.0f32, 0u8); NEAR_SHAPES];
    for y in y0..y1 {
        let cy = y as f32 + 0.5;
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        let mut n = 0;
        for (i, s) in shapes.iter().enumerate() {
            if let Some((a, b)) = s.row_span(cy) {
                lo = lo.min(a);
                hi = hi.max(b);
                if culled {
                    near[n] = (a - 1.0, b + 1.0, i as u8);
                    n += 1;
                }
            }
        }
        if lo > hi {
            continue;
        }
        let near = &near[..n];
        let x0 = (lo.floor() as i32 - 1).max(clip.left);
        let x1 = (hi.ceil() as i32 + 1).min(clip.right);
        for x in x0..x1 {
            let p = pt(x as f32 + 0.5, cy);
            let d = if culled {
                near.iter()
                    .filter(|&&(a, b, _)| a <= p.x && p.x <= b)
                    .fold(f32::INFINITY, |m, &(_, _, i)| {
                        m.min(shapes[usize::from(i)].distance(p))
                    })
            } else {
                shapes
                    .iter()
                    .fold(f32::INFINITY, |m, s| m.min(s.distance(p)))
            };
            let coverage = (0.5 - d).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            let mut c = paint.at(p);
            c.a *= coverage * alpha;
            if c.a > 0.0 {
                target.blend(x as usize, y as usize, c, blend);
            }
        }
    }
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
            let a = p >> 24;
            if a == 0 {
                continue;
            }
            let keep = 255 - a;
            let ch = |shift: u32| {
                ((p >> shift & 0xff) + ((*dst >> shift & 0xff) * keep + 127) / 255).min(255)
            };
            *dst = ch(16) << 16 | ch(8) << 8 | ch(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spectrum::Rgb;

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
}
