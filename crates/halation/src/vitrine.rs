//! `vitrine`: the glass chrome around the board (spec §7.1, §7.5): panels, tray wells, pills,
//! controls' marks, analytic soft shadows and glows, and the drag's drop target. Every colour
//! comes from `&Palette`. Panel interiors take an integer row path (`tint`); the rest is
//! `etch::fill` on small shapes, each edge in its own call so a row's span stays narrow.

use std::f32::consts::{FRAC_PI_2, PI};

use lumen::SlotState;

use crate::DeviceRect;
use crate::etch::{Blend, Layer, Paint, Pt, Shape, Stops, Target, clip_rect, fill, pt};
use crate::lettering::{Align, flat_line, number};
use crate::spectrum::{Palette, Rgba};

/// Source-over of the solid `c` on every pixel of `rect` (clipped to the layer), in integers:
/// `(px·(255 − a) + c·a + 127) / 255` per channel with `a = round(255·α)`.
pub fn tint(layer: &mut Layer, rect: DeviceRect, c: Rgba) {
    let full = DeviceRect::new(0, 0, layer.width as i32, layer.height as i32);
    let Some(r) = clip_rect(rect, full) else {
        return;
    };
    let a = (c.a.clamp(0.0, 1.0) * 255.0).round() as u32;
    if a == 0 {
        return;
    }
    let keep = 255 - a;
    let lift = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32 * a + 127;
    let (lr, lg, lb) = (lift(c.r), lift(c.g), lift(c.b));
    let (x0, x1) = (r.left as usize, r.right as usize);
    for y in r.top as usize..r.bottom as usize {
        for px in &mut layer.px[y * layer.width + x0..y * layer.width + x1] {
            let p = *px;
            let ch = |shift: u32, lift: u32| ((p >> shift & 0xff) * keep + lift) / 255;
            *px = ch(16, lr) << 16 | ch(8, lg) << 8 | ch(0, lb);
        }
    }
}

/// A rounded glass panel on whole device px (spec §7.1): a fill, then a rim fading from `rim[0]`
/// at the top to `rim[1]` at the bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    pub rect: DeviceRect,
    pub radius: f32,
    pub fill: Rgba,
    pub rim: [Rgba; 2],
    pub rim_width: f32,
}

impl Panel {
    /// The card and tray panel: `panel_fill`, rim `rim(0.22 → 0.05)`.
    pub fn glass(palette: &Palette, rect: DeviceRect, radius: f32, rim_width: f32) -> Panel {
        let [top, bottom] = palette.panel_rim_alpha;
        Panel {
            rect,
            radius,
            fill: palette.panel_fill,
            rim: [palette.rim.at(top), palette.rim.at(bottom)],
            rim_width,
        }
    }

    /// The level sheet: `sheet_fill` with the same rim.
    pub fn sheet(palette: &Palette, rect: DeviceRect, radius: f32, rim_width: f32) -> Panel {
        Panel {
            fill: palette.sheet_fill,
            ..Panel::glass(palette, rect, radius, rim_width)
        }
    }

    fn inset(&self) -> i32 {
        let r = self.rect;
        (self.radius.ceil() as i32)
            .min((r.right - r.left) / 2)
            .min((r.bottom - r.top) / 2)
            .max(0)
    }

    /// The interior as three whole-pixel bands that miss the rounded corners: the top band
    /// between the corners, the full-width middle, the bottom band between the corners.
    pub fn bands(&self) -> [DeviceRect; 3] {
        let (r, k) = (self.rect, self.inset());
        [
            DeviceRect::new(r.left + k, r.top, r.right - k, r.top + k),
            DeviceRect::new(r.left, r.top + k, r.right, r.bottom - k),
            DeviceRect::new(r.left + k, r.bottom - k, r.right - k, r.bottom),
        ]
    }

    /// The four corner squares the bands leave out (where the rounding is).
    pub fn corners(&self) -> [DeviceRect; 4] {
        let (r, k) = (self.rect, self.inset());
        [
            DeviceRect::new(r.left, r.top, r.left + k, r.top + k),
            DeviceRect::new(r.right - k, r.top, r.right, r.top + k),
            DeviceRect::new(r.right - k, r.bottom - k, r.right, r.bottom),
            DeviceRect::new(r.left, r.bottom - k, r.left + k, r.bottom),
        ]
    }
}

/// A panel into `base`: its bands by `tint`, then `panel_edges`.
pub fn panel(layer: &mut Layer, clip: DeviceRect, p: &Panel) {
    for band in p.bands() {
        if let Some(b) = clip_rect(band, clip) {
            tint(layer, b, p.fill);
        }
    }
    panel_edges(layer, clip, p);
}

/// A panel's corners (the round rect's fill, clipped to each corner square) and its rim: four
/// straight edges and four quarter arcs, `rim_width` wide, centred half a width inside the edge.
/// The radius and the rim are clamped to half the panel's shorter side (as `Panel::inset` clamps
/// the corners), so a small panel stays inside its rect; an empty one draws nothing.
pub fn panel_edges(t: &mut dyn Target, clip: DeviceRect, p: &Panel) {
    let r = p.rect;
    let (wide, high) = (r.right - r.left, r.bottom - r.top);
    if wide <= 0 || high <= 0 {
        return;
    }
    let most = wide.min(high) as f32 / 2.0;
    let (radius, rim) = (p.radius.clamp(0.0, most), p.rim_width.clamp(0.0, most));
    let (l, tp, rt, b) = (r.left as f32, r.top as f32, r.right as f32, r.bottom as f32);
    let shape = Shape::round_rect(l, tp, rt, b, radius);
    for corner in p.corners() {
        if let Some(c) = clip_rect(corner, clip) {
            fill(t, c, &[shape], &Paint::Solid(p.fill), 1.0, Blend::Over);
        }
    }
    let half = rim / 2.0;
    let rad = (radius - half).max(0.0);
    let (il, it, ir, ib) = (l + half, tp + half, rt - half, b - half);
    let paint = Paint::Linear {
        from: pt(0.0, tp),
        to: pt(0.0, b),
        stops: Stops::new(&[(0.0, p.rim[0]), (1.0, p.rim[1])]),
    };
    let w = rim;
    let edges = [
        Shape::line(pt(il + rad, it), pt(ir - rad, it), w),
        Shape::line(pt(il + rad, ib), pt(ir - rad, ib), w),
        Shape::line(pt(il, it + rad), pt(il, ib - rad), w),
        Shape::line(pt(ir, it + rad), pt(ir, ib - rad), w),
        Shape::arc(pt(il + rad, it + rad), rad, w, 3.0 * FRAC_PI_2, FRAC_PI_2),
        Shape::arc(pt(ir - rad, it + rad), rad, w, 0.0, FRAC_PI_2),
        Shape::arc(pt(ir - rad, ib - rad), rad, w, FRAC_PI_2, FRAC_PI_2),
        Shape::arc(pt(il + rad, ib - rad), rad, w, PI, FRAC_PI_2),
    ];
    for edge in edges {
        fill(t, clip, &[edge], &paint, 1.0, Blend::Over);
    }
}

/// A tray well (spec §7.5) for a `slot`-px slot: 0.92 × slot, radius 0.18 × well, `well_fill`,
/// then a top shade 0.1 × well tall fading from `well_shade` to nothing.
pub fn well(t: &mut dyn Target, clip: DeviceRect, palette: &Palette, centre: Pt, slot: f32) {
    let s = 0.92 * slot;
    let (l, tp) = (centre.x - s / 2.0, centre.y - s / 2.0);
    let shape = Shape::round_rect(l, tp, l + s, tp + s, 0.18 * s);
    fill(
        t,
        clip,
        &[shape],
        &Paint::Solid(palette.well_fill),
        1.0,
        Blend::Over,
    );
    let band = DeviceRect::new(
        l.floor() as i32,
        tp.floor() as i32,
        (l + s).ceil() as i32,
        (tp + 0.1 * s).ceil() as i32,
    );
    if let Some(band) = clip_rect(band, clip) {
        let shade = Paint::Linear {
            from: pt(0.0, tp),
            to: pt(0.0, tp + 0.1 * s),
            stops: Stops::new(&[
                (0.0, palette.well_shade),
                (1.0, palette.well_shade.with_alpha(0.0)),
            ]),
        };
        fill(t, band, &[shape], &shade, 1.0, Blend::Over);
    }
}

/// A pill (a round rect with radius half its height) at `[left, top, right, bottom]` device px:
/// `paint`, then a `rim_width` rim of `rim`.
pub fn pill(
    t: &mut dyn Target,
    clip: DeviceRect,
    [l, tp, r, b]: [f32; 4],
    paint: &Paint,
    rim: Rgba,
    rim_width: f32,
) {
    let radius = (b - tp) / 2.0;
    fill(
        t,
        clip,
        &[Shape::round_rect(l, tp, r, b, radius)],
        paint,
        1.0,
        Blend::Over,
    );
    let h = rim_width / 2.0;
    let outline = Shape::round_rect(l + h, tp + h, r - h, b - h, radius - h).outline(rim_width);
    fill(t, clip, &[outline], &Paint::Solid(rim), 1.0, Blend::Over);
}

/// The Levels pill's icon (comps): four 4.2-pt squares, radius 0.8 pt, 6 pt apart, stroked 1.3 pt
/// in the primary text colour, the first at `origin`.
pub fn levels_icon(t: &mut dyn Target, clip: DeviceRect, palette: &Palette, origin: Pt, ppp: f32) {
    let ink = Paint::Solid(palette.text.primary.opaque());
    let side = 4.2 * ppp;
    for (dx, dy) in [(0.0, 0.0), (6.0, 0.0), (0.0, 6.0), (6.0, 6.0)] {
        let (x, y) = (origin.x + dx * ppp, origin.y + dy * ppp);
        let square = Shape::round_rect(x, y, x + side, y + side, 0.8 * ppp).outline(1.3 * ppp);
        fill(t, clip, &[square], &ink, 1.0, Blend::Over);
    }
}

/// The sheet's close chip (spec §7.5): a 14-pt-radius disc in the pill tokens, and a × 9 pt
/// across, stroked 1.6 pt in the primary text colour.
pub fn close_mark(t: &mut dyn Target, clip: DeviceRect, palette: &Palette, centre: Pt, ppp: f32) {
    let r = 14.0 * ppp;
    fill(
        t,
        clip,
        &[Shape::disc(centre, r)],
        &Paint::Solid(palette.pill_fill),
        1.0,
        Blend::Over,
    );
    fill(
        t,
        clip,
        &[Shape::ring(centre, r - 0.5 * ppp, ppp)],
        &Paint::Solid(palette.pill_rim),
        1.0,
        Blend::Over,
    );
    let (h, w) = (4.5 * ppp, 1.6 * ppp);
    let cross = [
        Shape::line(
            pt(centre.x - h, centre.y - h),
            pt(centre.x + h, centre.y + h),
            w,
        ),
        Shape::line(
            pt(centre.x + h, centre.y - h),
            pt(centre.x - h, centre.y + h),
            w,
        ),
    ];
    fill(
        t,
        clip,
        &cross,
        &Paint::Solid(palette.text.primary.opaque()),
        1.0,
        Blend::Over,
    );
}

/// `1` well inside, `0` at 2σ outside, smoothstep between (from −2σ): a blurred edge's profile.
fn falloff(d: f32, sigma: f32) -> f32 {
    let x = ((d + 2.0 * sigma) / (4.0 * sigma.max(1e-3))).clamp(0.0, 1.0);
    1.0 - x * x * (3.0 - 2.0 * x)
}

/// `shape` blurred by `sigma` px (a shadow's or glow's `blur b` is σ = b / 2), as `c` with alpha
/// times the falloff of the signed distance (ruling 12). `skip` is left untouched (the part a
/// panel will cover anyway). Allocation-free; walks the bounds grown by 2σ.
pub fn soft_shape(
    t: &mut dyn Target,
    clip: DeviceRect,
    shape: &Shape,
    sigma: f32,
    c: Rgba,
    skip: Option<DeviceRect>,
) {
    let (l, tp, r, b) = shape.bounds();
    let reach = 2.0 * sigma;
    let area = DeviceRect::new(
        (l - reach).floor() as i32,
        (tp - reach).floor() as i32,
        (r + reach).ceil() as i32,
        (b + reach).ceil() as i32,
    );
    let (w, h) = t.size();
    let Some(area) =
        clip_rect(area, clip).and_then(|a| clip_rect(a, DeviceRect::new(0, 0, w as i32, h as i32)))
    else {
        return;
    };
    for y in area.top..area.bottom {
        for x in area.left..area.right {
            if skip
                .is_some_and(|s| (s.left..s.right).contains(&x) && (s.top..s.bottom).contains(&y))
            {
                continue;
            }
            let k = falloff(shape.distance(pt(x as f32 + 0.5, y as f32 + 0.5)), sigma);
            if k > 0.0 {
                t.blend(x as usize, y as usize, c.with_alpha(c.a * k), Blend::Over);
            }
        }
    }
}

/// An ellipse of radii `rx × ry` blurred by `sigma`, as `soft_shape`. The signed distance is the
/// first-order estimate `k0·(k0 − 1) / k1` (`k0 = |p/r|`, `k1 = |p/r²|`): exact on both axes,
/// approximate elsewhere, and at least 2σ outside the grown bounding box, so nothing is cut off.
pub fn soft_ellipse(
    t: &mut dyn Target,
    clip: DeviceRect,
    centre: Pt,
    rx: f32,
    ry: f32,
    sigma: f32,
    c: Rgba,
) {
    let reach = 2.0 * sigma;
    let area = DeviceRect::new(
        (centre.x - rx - reach).floor() as i32,
        (centre.y - ry - reach).floor() as i32,
        (centre.x + rx + reach).ceil() as i32,
        (centre.y + ry + reach).ceil() as i32,
    );
    let (w, h) = t.size();
    let Some(area) =
        clip_rect(area, clip).and_then(|a| clip_rect(a, DeviceRect::new(0, 0, w as i32, h as i32)))
    else {
        return;
    };
    let (rx, ry) = (rx.max(1e-3), ry.max(1e-3));
    for y in area.top..area.bottom {
        for x in area.left..area.right {
            let (px, py) = (x as f32 + 0.5 - centre.x, y as f32 + 0.5 - centre.y);
            let k0 = ((px / rx).powi(2) + (py / ry).powi(2)).sqrt();
            let k1 = ((px / (rx * rx)).powi(2) + (py / (ry * ry)).powi(2)).sqrt();
            // At the exact centre the gradient vanishes: the nearest edge is the short radius.
            let d = if k1 < 1e-9 {
                -rx.min(ry)
            } else {
                k0 * (k0 - 1.0) / k1
            };
            let k = falloff(d, sigma);
            if k > 0.0 {
                t.blend(x as usize, y as usize, c.with_alpha(c.a * k), Blend::Over);
            }
        }
    }
}

/// The drop target (spec §7.5) for a cell of `u` px: a soft cell fill (a 0.94u square, radius
/// 0.18u, `fg` α 0.10) and a ring of radius 0.62u, `fg` α 0.75, `max(1.5 px, 0.05u)` wide.
pub fn drop_target(t: &mut dyn Target, clip: DeviceRect, palette: &Palette, centre: Pt, u: f32) {
    let h = 0.47 * u;
    let cell = Shape::round_rect(
        centre.x - h,
        centre.y - h,
        centre.x + h,
        centre.y + h,
        0.18 * u,
    );
    fill(
        t,
        clip,
        &[cell],
        &Paint::Solid(palette.fg.at(0.10)),
        1.0,
        Blend::Over,
    );
    let ring = Shape::ring(centre, 0.62 * u, (0.05 * u).max(1.5));
    fill(
        t,
        clip,
        &[ring],
        &Paint::Solid(palette.fg.at(0.75)),
        1.0,
        Blend::Over,
    );
}

/// The dragged piece's empty source (spec §7.5, comps): a rounded square 0.86u wide, radius
/// 0.16u, in dashes of 0.06u every 0.11u along its straight edges and solid quarter arcs at its
/// corners, `fg` α 0.2, `max(1 px, 0.02u)` wide.
pub fn dashed_outline(t: &mut dyn Target, clip: DeviceRect, palette: &Palette, centre: Pt, u: f32) {
    let (half, rad, w) = (0.43 * u, 0.16 * u, (0.02 * u).max(1.0));
    let ink = Paint::Solid(palette.fg.at(0.2));
    let (dash, period) = (0.06 * u, 0.11 * u);
    let run = |t: &mut dyn Target, a: Pt, b: Pt| {
        let len = (b - a).len();
        let along = (b - a) * (1.0 / len.max(1e-3));
        let mut s = 0.0;
        while s < len {
            let e = (s + dash).min(len);
            fill(
                t,
                clip,
                &[Shape::line(a + along * s, a + along * e, w)],
                &ink,
                1.0,
                Blend::Over,
            );
            s += period;
        }
    };
    let (l, tp, r, b) = (
        centre.x - half,
        centre.y - half,
        centre.x + half,
        centre.y + half,
    );
    run(t, pt(l + rad, tp), pt(r - rad, tp));
    run(t, pt(l + rad, b), pt(r - rad, b));
    run(t, pt(l, tp + rad), pt(l, b - rad));
    run(t, pt(r, tp + rad), pt(r, b - rad));
    for (c, start) in [
        (pt(l + rad, tp + rad), 3.0 * FRAC_PI_2),
        (pt(r - rad, tp + rad), 0.0),
        (pt(r - rad, b - rad), FRAC_PI_2),
        (pt(l + rad, b - rad), PI),
    ] {
        fill(
            t,
            clip,
            &[Shape::arc(c, rad, w, start, FRAC_PI_2)],
            &ink,
            1.0,
            Blend::Over,
        );
    }
}

/// How a level chip shows its level (spec §7.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChipState {
    Locked,
    Open,
    Solved,
    Current,
}

impl From<SlotState> for ChipState {
    fn from(state: SlotState) -> ChipState {
        match state {
            SlotState::Current => ChipState::Current,
            SlotState::Completed => ChipState::Solved,
            SlotState::Unlocked => ChipState::Open,
            SlotState::Locked => ChipState::Locked,
        }
    }
}

impl ChipState {
    pub const ALL: [ChipState; 4] = [
        ChipState::Locked,
        ChipState::Open,
        ChipState::Solved,
        ChipState::Current,
    ];

    /// The glass highlight `h`: 0 locked, 0.4 open, 1 solved or current.
    fn highlight(self) -> f32 {
        match self {
            ChipState::Locked => 0.0,
            ChipState::Open => 0.4,
            ChipState::Solved | ChipState::Current => 1.0,
        }
    }

    fn digit(self, palette: &Palette) -> Rgba {
        match self {
            ChipState::Locked => palette.chip_digit[0],
            ChipState::Open => palette.chip_digit[1],
            ChipState::Solved | ChipState::Current => palette.chip_digit[2],
        }
    }
}

/// How far a chip's sprite reaches past the chip, px: the current chip's glow (blur 6 pt, so
/// σ = 3 pt, faded out by 2σ).
pub fn chip_margin(ppp: f32) -> f32 {
    (6.0 * ppp).ceil()
}

/// A chip's body at `centre`, `size` px square, radius 0.22 × size: the current chip's glow (cast
/// by its fill, so at the fill's alpha too) and white fill; the glass fill (TL → BR, `h` by state); the rim (current: white α 0.9 at 1.5 pt;
/// else `rim` α 0.08 locked / 0.3, vertical 1 → 0.35 → 0.55, `max(1 px, 0.028·size)`); the solved
/// mark (R/G/B arcs, 16° gaps, and a centre dot) at the top right; the padlock at the bottom right.
pub fn chip_body(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    centre: Pt,
    size: f32,
    state: ChipState,
    ppp: f32,
) {
    let (x, y) = (centre.x - size / 2.0, centre.y - size / 2.0);
    let shape = Shape::round_rect(x, y, x + size, y + size, 0.22 * size);
    if state == ChipState::Current {
        // A Canvas shadow takes the alpha of the fill that casts it (comps: white α 0.25 under a
        // white α 0.12 fill, so a 0.03 halo; the light theme's opaque fill casts it whole).
        let glow = palette.current_glow;
        let glow = glow.with_alpha(glow.a * palette.current_fill.a);
        soft_shape(t, clip, &shape, 3.0 * ppp, glow, None);
        fill(
            t,
            clip,
            &[shape],
            &Paint::Solid(palette.current_fill),
            1.0,
            Blend::Over,
        );
    }
    let h = state.highlight();
    let glass = Paint::Linear {
        from: pt(x, y),
        to: pt(x + size, y + size),
        stops: Stops::new(&[
            (0.0, palette.glass.at(0.085 + 0.05 * h)),
            (1.0, palette.glass_deep.alpha(0.025 + 0.02 * h)),
        ]),
    };
    fill(t, clip, &[shape], &glass, 1.0, Blend::Over);
    let rim_of = |w: f32| {
        Shape::round_rect(
            x + w / 2.0,
            y + w / 2.0,
            x + size - w / 2.0,
            y + size - w / 2.0,
            0.22 * size - w / 2.0,
        )
        .outline(w)
    };
    if state == ChipState::Current {
        let rim = rim_of(1.5 * ppp);
        fill(
            t,
            clip,
            &[rim],
            &Paint::Solid(palette.current_rim),
            1.0,
            Blend::Over,
        );
    } else {
        let a = if state == ChipState::Locked {
            0.08
        } else {
            0.3
        };
        let rim = rim_of((0.028 * size).max(1.0));
        let paint = Paint::Linear {
            from: pt(0.0, y),
            to: pt(0.0, y + size),
            stops: Stops::new(&[
                (0.0, palette.rim.at(a)),
                (0.5, palette.rim.at(0.35 * a)),
                (1.0, palette.rim.at(0.55 * a)),
            ]),
        };
        fill(t, clip, &[rim], &paint, 1.0, Blend::Over);
    }
    match state {
        ChipState::Solved => {
            let r = 0.09 * size;
            let c = pt(x + size - r - 0.07 * size, y + r + 0.07 * size);
            let w = (0.32 * r).max(1.0);
            for (k, rgb) in [palette.red, palette.green, palette.blue]
                .into_iter()
                .enumerate()
            {
                let start = (k as f32 * 120.0 + 16.0).to_radians();
                fill(
                    t,
                    clip,
                    &[Shape::arc(c, r, w, start, 88f32.to_radians())],
                    &Paint::Solid(rgb.opaque()),
                    1.0,
                    Blend::Over,
                );
            }
            fill(
                t,
                clip,
                &[Shape::disc(c, 0.36 * r)],
                &Paint::Solid(palette.solved_dot.opaque()),
                1.0,
                Blend::Over,
            );
        }
        ChipState::Locked => {
            let (bw, bh) = (0.13 * size, 0.10 * size);
            let (bx, by) = (x + size - bw - 0.08 * size, y + size - bh - 0.08 * size);
            let ink = Paint::Solid(palette.padlock);
            fill(
                t,
                clip,
                &[Shape::round_rect(bx, by, bx + bw, by + bh, 0.25 * bh)],
                &ink,
                1.0,
                Blend::Over,
            );
            let shackle = Shape::arc(
                pt(bx + bw / 2.0, by),
                0.3 * bw,
                (0.022 * size).max(1.0),
                1.5 * PI,
                PI,
            );
            fill(t, clip, &[shackle], &ink, 1.0, Blend::Over);
        }
        ChipState::Open | ChipState::Current => {}
    }
}

/// Chip `n`'s digits (n = slot + 1): the lettering's digit glyphs as flat strokes at cap
/// 0.32 × size, stroke `max(1.5 px, 0.055·cap)`, centred, coloured by state.
pub fn chip_digits(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    centre: Pt,
    size: f32,
    n: usize,
    state: ChipState,
) {
    let cap = 0.32 * size;
    let mut buf = [0u8; 2];
    let text = number(n, &mut buf);
    flat_line(
        t,
        clip,
        text,
        pt(centre.x, centre.y - cap / 2.0),
        cap,
        0.3,
        (0.055 * cap).max(1.5),
        state.digit(palette),
        Align::Centre,
        Blend::Over,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rgb;

    fn black(w: usize, h: usize) -> Vec<u32> {
        vec![0; w * h]
    }

    fn channels(p: u32) -> [i32; 3] {
        [
            (p >> 16 & 0xff) as i32,
            (p >> 8 & 0xff) as i32,
            (p & 0xff) as i32,
        ]
    }

    #[test]
    fn tint_is_integer_source_over() {
        let mut px = vec![0x00_40_80_c0u32; 4];
        let mut layer = Layer {
            px: &mut px,
            width: 2,
            height: 2,
        };
        let c = Rgb(200, 100, 0).alpha(0.25);
        tint(&mut layer, DeviceRect::new(0, 0, 1, 2), c);
        let a = 64.0 / 255.0; // 0.25 rounds to 64/255
        let want = |d: f32, s: f32| (s * a + d * (1.0 - a)).round() as i32;
        let [r, g, b] = channels(px[0]);
        for (got, want) in [
            (r, want(64.0, 200.0)),
            (g, want(128.0, 100.0)),
            (b, want(192.0, 0.0)),
        ] {
            assert!((got - want).abs() <= 1, "{got} vs {want}");
        }
        assert_eq!(px[1], 0x00_40_80_c0, "outside the rect: untouched");
    }

    #[test]
    fn a_panel_is_rounded_and_rimmed() {
        let (w, h) = (200, 120);
        let mut px = black(w, h);
        let mut layer = Layer {
            px: &mut px,
            width: w,
            height: h,
        };
        let p = Panel::glass(&Palette::DARK, DeviceRect::new(10, 10, 190, 110), 28.0, 2.0);
        panel(&mut layer, DeviceRect::new(0, 0, w as i32, h as i32), &p);
        let at = |x: usize, y: usize| channels(px[y * w + x]);
        assert_eq!(at(11, 11), [0, 0, 0], "the corner is round");
        let mut fill = black(1, 1);
        tint(
            &mut Layer {
                px: &mut fill,
                width: 1,
                height: 1,
            },
            DeviceRect::new(0, 0, 1, 1),
            Palette::DARK.panel_fill,
        );
        assert_eq!(at(100, 60), channels(fill[0]), "the interior is the tint");
        assert!(
            at(100, 10)[2] > at(100, 109)[2],
            "the rim fades from top to bottom"
        );
        let [top, middle, bottom] = p.bands();
        assert_eq!(middle, DeviceRect::new(10, 38, 190, 82));
        assert_eq!(
            (top.left, top.right, bottom.left, bottom.right),
            (38, 162, 38, 162)
        );
    }

    #[test]
    fn a_degenerate_panel_stays_inside_its_rect() {
        let (w, h) = (100, 100);
        let all = DeviceRect::new(0, 0, w as i32, h as i32);
        // Smaller than 2·radius each way, one way only, and empty.
        for rect in [
            DeviceRect::new(40, 40, 50, 50),
            DeviceRect::new(20, 40, 80, 52),
            DeviceRect::new(40, 40, 40, 60),
            DeviceRect::new(60, 30, 50, 20),
        ] {
            let mut px = black(w, h);
            let mut layer = Layer {
                px: &mut px,
                width: w,
                height: h,
            };
            panel(
                &mut layer,
                all,
                &Panel::glass(&Palette::DARK, rect, 14.0, 2.0),
            );
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    let inside = (rect.left..rect.right).contains(&x)
                        && (rect.top..rect.bottom).contains(&y);
                    assert!(
                        inside || px[y as usize * w + x as usize] == 0,
                        "{rect:?}: ({x}, {y}) drawn outside the panel"
                    );
                }
            }
        }
    }

    #[test]
    fn a_well_is_shaded_at_its_top() {
        let (w, h) = (100, 100);
        let mut px = vec![0x00_30_30_30u32; w * h];
        let mut layer = Layer {
            px: &mut px,
            width: w,
            height: h,
        };
        well(
            &mut layer,
            DeviceRect::new(0, 0, 100, 100),
            &Palette::DARK,
            pt(50.0, 50.0),
            100.0,
        );
        let at = |x: usize, y: usize| channels(px[y * w + x])[0];
        assert!(at(50, 6) < at(50, 50), "darker under the top edge");
        assert_eq!(at(1, 1), 0x30, "outside the well (0.92 × slot)");
    }

    #[test]
    fn soft_shapes_fall_off_over_their_sigma() {
        let (w, h) = (200, 60);
        let mut px = black(w, h);
        let mut layer = Layer {
            px: &mut px,
            width: w,
            height: h,
        };
        let shape = Shape::rect(50.0, 0.0, 150.0, 60.0);
        soft_shape(
            &mut layer,
            DeviceRect::new(0, 0, 200, 60),
            &shape,
            10.0,
            Rgb(255, 255, 255).alpha(1.0),
            None,
        );
        let at = |x: usize| channels(px[30 * w + x])[0];
        assert_eq!(at(100), 255, "solid inside");
        assert_eq!(at(10), 0, "nothing beyond 2σ");
        assert!(
            at(45) > at(55 - 20) && at(55) > at(50) && at(50) > at(45),
            "a monotonic edge"
        );
        let mut skipped = black(w, h);
        let mut layer = Layer {
            px: &mut skipped,
            width: w,
            height: h,
        };
        soft_shape(
            &mut layer,
            DeviceRect::new(0, 0, 200, 60),
            &shape,
            10.0,
            Rgb(255, 255, 255).alpha(1.0),
            Some(DeviceRect::new(60, 0, 140, 60)),
        );
        assert_eq!(
            channels(skipped[30 * w + 100])[0],
            0,
            "the skipped rect is left alone"
        );
    }

    #[test]
    fn a_soft_ellipse_falls_off_by_its_true_distance() {
        let (w, h) = (200, 60);
        let mut px = black(w, h);
        let mut layer = Layer {
            px: &mut px,
            width: w,
            height: h,
        };
        let (rx, ry, sigma) = (40.0, 10.0, 5.0);
        let white = Rgb(255, 255, 255).alpha(1.0);
        let all = DeviceRect::new(0, 0, 200, 60);
        soft_ellipse(&mut layer, all, pt(100.5, 30.5), rx, ry, sigma, white);
        let at = |x: usize, y: usize| channels(px[y * w + x])[0];
        assert_eq!(at(100, 30), 255, "solid at the centre");
        assert_eq!(at(150, 30), 0, "nothing at rx + 2σ: no hard cut-off");
        assert_eq!(at(151, 30), 0);
        let (long, short) = (at(145, 30), at(100, 45));
        assert!(long > 0 && long < 255, "{long}");
        assert!(
            (long - short).abs() <= 1,
            "rx + σ on the long axis vs ry + σ: {long} vs {short}"
        );
    }

    #[test]
    fn the_drop_ring_sits_at_0_62u() {
        let (w, u) = (200usize, 100.0f32);
        let mut px = black(w, w);
        let mut layer = Layer {
            px: &mut px,
            width: w,
            height: w,
        };
        drop_target(
            &mut layer,
            DeviceRect::new(0, 0, 200, 200),
            &Palette::DARK,
            pt(100.0, 100.0),
            u,
        );
        let at = |x: usize| channels(px[100 * w + x])[0];
        assert!(at(162) > 150, "the ring at 0.62u: {}", at(162));
        assert!(
            at(140) < 40 && at(140) > 0,
            "the soft cell fill inside 0.47u"
        );
        assert_eq!(at(190), 0);
    }

    #[test]
    fn chrome_takes_every_colour_from_the_palette() {
        let sentinel = Palette::DARK.recoloured(Rgb(255, 0, 255));
        let (w, h) = (300, 200);
        let mut px = black(w, h);
        let all = DeviceRect::new(0, 0, 300, 200);
        let mut layer = Layer {
            px: &mut px,
            width: w,
            height: h,
        };
        panel(
            &mut layer,
            all,
            &Panel::glass(&sentinel, DeviceRect::new(5, 5, 120, 90), 14.0, 1.0),
        );
        panel(
            &mut layer,
            all,
            &Panel::sheet(&sentinel, DeviceRect::new(130, 5, 290, 90), 14.0, 1.0),
        );
        well(&mut layer, all, &sentinel, pt(60.0, 140.0), 50.0);
        pill(
            &mut layer,
            all,
            [100.0, 120.0, 180.0, 148.0],
            &Paint::Solid(sentinel.pill_fill),
            sentinel.pill_rim,
            1.0,
        );
        levels_icon(&mut layer, all, &sentinel, pt(110.0, 126.0), 1.0);
        close_mark(&mut layer, all, &sentinel, pt(220.0, 140.0), 1.0);
        soft_ellipse(
            &mut layer,
            all,
            pt(260.0, 170.0),
            20.0,
            10.0,
            3.0,
            sentinel.drag_shadow.with_alpha(1.0),
        );
        drop_target(&mut layer, all, &sentinel, pt(150.0, 170.0), 30.0);
        dashed_outline(&mut layer, all, &sentinel, pt(60.0, 140.0), 50.0);
        for &p in &px {
            let [r, g, b] = channels(p);
            assert!(g == 0 && (r - b).abs() <= 1, "{p:06x}");
        }
    }

    fn chip_canvas(state: ChipState, palette: &Palette) -> Vec<u32> {
        let mut px = black(120, 120);
        let mut layer = Layer {
            px: &mut px,
            width: 120,
            height: 120,
        };
        chip_body(
            &mut layer,
            DeviceRect::new(0, 0, 120, 120),
            palette,
            pt(60.0, 60.0),
            80.0,
            state,
            2.0,
        );
        px
    }

    #[test]
    fn only_the_current_chip_glows_past_its_edge() {
        let at = |px: &[u32], x: usize, y: usize| channels(px[y * 120 + x])[0];
        let current = chip_canvas(ChipState::Current, &Palette::DARK);
        let open = chip_canvas(ChipState::Open, &Palette::DARK);
        assert!(at(&current, 60, 18) > 0, "glow 2 px outside the top edge");
        assert_eq!(at(&open, 60, 18), 0);
        assert!(
            at(&current, 60, 60) > at(&open, 60, 60),
            "the current chip's white fill"
        );
        assert!(
            chip_margin(2.0) >= 12.0,
            "the sprite margin covers 2σ = 6 pt"
        );
    }

    #[test]
    fn locked_chips_carry_a_padlock_and_solved_ones_a_mark() {
        let locked = chip_canvas(ChipState::Locked, &Palette::DARK);
        let solved = chip_canvas(ChipState::Solved, &Palette::DARK);
        let open = chip_canvas(ChipState::Open, &Palette::DARK);
        let region = |px: &[u32], x0: usize, x1: usize, y0: usize, y1: usize| {
            (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .map(|(x, y)| channels(px[y * 120 + x]))
                .collect::<Vec<_>>()
        };
        // The padlock: bottom right, inset 0.08 × chip (chip 80 at centre 60 → x 20..100).
        let lock = region(&locked, 80, 96, 80, 96);
        assert!(
            lock.iter()
                .zip(region(&open, 80, 96, 80, 96))
                .any(|(l, o)| l[0] > o[0] + 30),
            "a padlock"
        );
        // The solved mark: top right, R/G/B arcs.
        let mark = region(&solved, 80, 100, 20, 40);
        assert!(mark.iter().any(|c| c[0] > 150 && c[1] < 120), "a red arc");
        assert!(mark.iter().any(|c| c[2] > 150 && c[0] < 120), "a blue arc");
    }

    #[test]
    fn digits_are_centred_and_coloured_by_state() {
        let mut px = black(120, 120);
        let mut layer = Layer {
            px: &mut px,
            width: 120,
            height: 120,
        };
        chip_digits(
            &mut layer,
            DeviceRect::new(0, 0, 120, 120),
            &Palette::DARK,
            pt(60.0, 60.0),
            80.0,
            14,
            ChipState::Open,
        );
        let lit: Vec<(usize, usize)> = (0..120 * 120)
            .filter(|&i| px[i] != 0)
            .map(|i| (i % 120, i / 120))
            .collect();
        let (xs, ys): (Vec<_>, Vec<_>) = lit.iter().copied().unzip();
        let mid = |v: &[usize]| (v.iter().min().unwrap() + v.iter().max().unwrap()) as f32 / 2.0;
        assert!(
            (mid(&xs) - 60.0).abs() <= 2.0 && (mid(&ys) - 60.0).abs() <= 2.0,
            "centred"
        );
        let cap = 0.32 * 80.0;
        assert!(
            (ys.iter().max().unwrap() - ys.iter().min().unwrap()) as f32 <= cap + 4.0,
            "cap 0.32 × chip"
        );
    }

    #[test]
    fn chips_take_every_colour_from_the_palette() {
        let sentinel = Palette::DARK.recoloured(crate::Rgb(255, 0, 255));
        for state in ChipState::ALL {
            let mut px = chip_canvas(state, &sentinel);
            let mut layer = Layer {
                px: &mut px,
                width: 120,
                height: 120,
            };
            chip_digits(
                &mut layer,
                DeviceRect::new(0, 0, 120, 120),
                &sentinel,
                pt(60.0, 60.0),
                80.0,
                50,
                state,
            );
            for &p in &px {
                let [r, g, b] = channels(p);
                assert!(g == 0 && (r - b).abs() <= 1, "{state:?}: {p:06x}");
            }
        }
    }

    #[test]
    fn chip_states_follow_the_strip() {
        use lumen::SlotState;
        assert_eq!(ChipState::from(SlotState::Current), ChipState::Current);
        assert_eq!(ChipState::from(SlotState::Completed), ChipState::Solved);
        assert_eq!(ChipState::from(SlotState::Unlocked), ChipState::Open);
        assert_eq!(ChipState::from(SlotState::Locked), ChipState::Locked);
    }
}
