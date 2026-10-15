//! `lettering`: the brand's laser lettering, ported by hand from `trailer/lettering.mjs` (its
//! twin: change both). Every glyph is centre lines on a grid whose cap height is 1, in
//! hundredths; each line is drawn as a beam (a faint halo, red/green/blue strands side by side
//! that add up to white, a white core), or as one flat stroke (the picker's digits). Spec §10.

use std::f32::consts::TAU;

use crate::DeviceRect;
use crate::etch::{Blend, Paint, Pt, Shape, Stops, Target, fill, pt};
use crate::spectrum::{LightMode, Palette, Rgb, Rgba};

/// Every character the lettering has.
pub const CHARSET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789·.+–×:/&!? ";
/// The beam's nominal width in caps; letters are spaced as if their strokes were this wide.
pub const WEIGHT: f32 = 0.1;
/// `strokes` header bits: the point count, then whether the path closes or is a dot.
const C: u8 = 0x40;
const D: u8 = 0x80;
/// The longest path (B: 12 points, 11 segments; a closed 8-point loop has 8).
const MAX_SEGMENTS: usize = 12;
/// "CHROMATRON" at the wordmark's tracking, in caps (`width("CHROMATRON", 0.36)`).
const WORDMARK_WIDTH: f32 = 12.24;

/// One glyph: its advance width and its strokes, as `[header, x0, y0, x1, y1, …]` per path in
/// hundredths of the cap (header = point count | `C` closed | `D` dot).
#[derive(Debug, PartialEq, Eq)]
pub struct Glyph {
    pub ch: char,
    pub width: u8,
    pub strokes: &'static [u8],
}

/// Generated from `trailer/lettering.mjs` `GLYPHS` (Task 4 Step 1's script prints the checksum).
#[rustfmt::skip]
pub const GLYPHS: [Glyph; 47] = [
    Glyph { ch: 'A', width: 80, strokes: &[6, 0, 100, 0, 30, 30, 0, 50, 0, 80, 30, 80, 100, 2, 0, 58, 80, 58] },
    Glyph { ch: 'B', width: 76, strokes: &[12, 0, 47, 48, 47, 68, 27, 68, 20, 48, 0, 0, 0, 0, 100, 56, 100, 76, 80, 76, 67, 56, 47, 0, 47] },
    Glyph { ch: 'C', width: 74, strokes: &[6, 74, 0, 30, 0, 0, 30, 0, 70, 30, 100, 74, 100] },
    Glyph { ch: 'D', width: 80, strokes: &[6 | C, 0, 0, 50, 0, 80, 30, 80, 70, 50, 100, 0, 100] },
    Glyph { ch: 'E', width: 64, strokes: &[4, 64, 0, 0, 0, 0, 100, 64, 100, 2, 0, 50, 54, 50] },
    Glyph { ch: 'F', width: 62, strokes: &[3, 62, 0, 0, 0, 0, 100, 2, 0, 50, 52, 50] },
    Glyph { ch: 'G', width: 78, strokes: &[8, 78, 0, 30, 0, 0, 30, 0, 70, 30, 100, 78, 100, 78, 54, 42, 54] },
    Glyph { ch: 'H', width: 78, strokes: &[2, 0, 0, 0, 100, 2, 78, 0, 78, 100, 2, 0, 50, 78, 50] },
    Glyph { ch: 'I', width: 0, strokes: &[2, 0, 0, 0, 100] },
    Glyph { ch: 'J', width: 64, strokes: &[5, 64, 0, 64, 70, 34, 100, 30, 100, 0, 70] },
    Glyph { ch: 'K', width: 70, strokes: &[2, 0, 0, 0, 100, 3, 70, 0, 20, 50, 70, 100, 2, 0, 50, 20, 50] },
    Glyph { ch: 'L', width: 62, strokes: &[3, 0, 0, 0, 100, 62, 100] },
    Glyph { ch: 'M', width: 100, strokes: &[5, 0, 100, 0, 0, 50, 50, 100, 0, 100, 100] },
    Glyph { ch: 'N', width: 80, strokes: &[4, 0, 100, 0, 0, 80, 100, 80, 0] },
    Glyph { ch: 'O', width: 86, strokes: &[8 | C, 30, 0, 56, 0, 86, 30, 86, 70, 56, 100, 30, 100, 0, 70, 0, 30] },
    Glyph { ch: 'P', width: 74, strokes: &[7, 0, 100, 0, 0, 54, 0, 74, 20, 74, 34, 54, 54, 0, 54] },
    Glyph { ch: 'Q', width: 86, strokes: &[8 | C, 30, 0, 56, 0, 86, 30, 86, 70, 56, 100, 30, 100, 0, 70, 0, 30, 2, 56, 70, 92, 106] },
    Glyph { ch: 'R', width: 74, strokes: &[7, 0, 100, 0, 0, 54, 0, 74, 20, 74, 34, 54, 54, 0, 54, 2, 31, 57, 74, 100] },
    Glyph { ch: 'S', width: 72, strokes: &[10, 72, 0, 30, 0, 0, 30, 0, 36, 14, 50, 58, 50, 72, 64, 72, 70, 42, 100, 0, 100] },
    Glyph { ch: 'T', width: 78, strokes: &[2, 0, 0, 78, 0, 2, 39, 0, 39, 100] },
    Glyph { ch: 'U', width: 78, strokes: &[6, 0, 0, 0, 70, 30, 100, 48, 100, 78, 70, 78, 0] },
    Glyph { ch: 'V', width: 76, strokes: &[5, 0, 0, 0, 62, 38, 100, 76, 62, 76, 0] },
    Glyph { ch: 'W', width: 100, strokes: &[5, 0, 0, 0, 100, 50, 50, 100, 100, 100, 0] },
    Glyph { ch: 'X', width: 76, strokes: &[2, 0, 0, 76, 100, 2, 76, 0, 0, 100] },
    Glyph { ch: 'Y', width: 76, strokes: &[5, 0, 0, 0, 17, 38, 55, 76, 17, 76, 0, 2, 38, 55, 38, 100] },
    Glyph { ch: 'Z', width: 74, strokes: &[4, 0, 0, 74, 0, 0, 100, 74, 100] },
    Glyph { ch: '0', width: 70, strokes: &[8 | C, 24, 0, 46, 0, 70, 24, 70, 76, 46, 100, 24, 100, 0, 76, 0, 24] },
    Glyph { ch: '1', width: 24, strokes: &[3, 0, 24, 24, 0, 24, 100] },
    Glyph { ch: '2', width: 70, strokes: &[7, 0, 30, 30, 0, 40, 0, 70, 30, 70, 36, 0, 100, 70, 100] },
    Glyph { ch: '3', width: 70, strokes: &[6, 0, 0, 50, 0, 70, 20, 70, 26, 50, 46, 20, 46, 5, 50, 46, 70, 66, 70, 70, 40, 100, 0, 100] },
    Glyph { ch: '4', width: 70, strokes: &[3, 0, 0, 0, 64, 70, 64, 2, 50, 30, 50, 100] },
    Glyph { ch: '5', width: 70, strokes: &[8, 70, 0, 0, 0, 0, 46, 40, 46, 70, 76, 70, 70, 40, 100, 0, 100] },
    Glyph { ch: '6', width: 70, strokes: &[10, 64, 0, 30, 0, 0, 30, 0, 70, 30, 100, 40, 100, 70, 70, 70, 66, 50, 46, 0, 46] },
    Glyph { ch: '7', width: 70, strokes: &[4, 0, 0, 70, 0, 70, 30, 0, 100] },
    Glyph { ch: '8', width: 70, strokes: &[8 | C, 20, 0, 50, 0, 70, 20, 70, 26, 50, 46, 20, 46, 0, 26, 0, 20, 8 | C, 20, 46, 50, 46, 70, 66, 70, 70, 40, 100, 30, 100, 0, 70, 0, 66] },
    Glyph { ch: '9', width: 70, strokes: &[10, 6, 100, 40, 100, 70, 70, 70, 30, 40, 0, 30, 0, 0, 30, 0, 34, 20, 54, 70, 54] },
    Glyph { ch: '·', width: 0, strokes: &[1 | D, 0, 50] },
    Glyph { ch: '.', width: 0, strokes: &[1 | D, 0, 100] },
    Glyph { ch: '+', width: 60, strokes: &[2, 0, 50, 60, 50, 2, 30, 20, 30, 80] },
    Glyph { ch: '–', width: 56, strokes: &[2, 0, 52, 56, 52] },
    Glyph { ch: '×', width: 50, strokes: &[2, 0, 25, 50, 75, 2, 50, 25, 0, 75] },
    Glyph { ch: ':', width: 0, strokes: &[1 | D, 0, 38, 1 | D, 0, 100] },
    Glyph { ch: '/', width: 50, strokes: &[2, 0, 100, 50, 0] },
    Glyph { ch: '&', width: 86, strokes: &[11, 86, 100, 22, 36, 22, 16, 38, 0, 54, 0, 70, 16, 70, 26, 12, 84, 28, 100, 56, 100, 86, 70] },
    Glyph { ch: '!', width: 0, strokes: &[2, 0, 0, 0, 68, 1 | D, 0, 100] },
    Glyph { ch: '?', width: 66, strokes: &[7, 0, 22, 22, 0, 44, 0, 66, 22, 66, 32, 33, 65, 33, 72, 1 | D, 33, 100] },
    Glyph { ch: ' ', width: 34, strokes: &[] },
];

/// Pairs whose open corners leave too much air: hundredths of a cap added to the advance.
#[rustfmt::skip]
const KERN: [(char, char, i8); 10] = [
    ('L', 'T', -20), ('L', 'Y', -14), ('L', 'V', -10), ('T', '.', -18), ('P', '.', -14),
    ('F', '.', -14), ('Y', '.', -12), ('V', '.', -10), ('7', '.', -12), ('1', '.', -4),
];

pub fn glyph(ch: char) -> Option<&'static Glyph> {
    let ch = ch.to_ascii_uppercase();
    GLYPHS.iter().find(|g| g.ch == ch)
}

/// Whether every character of `text` (upper-cased) has a glyph; an empty text is not.
pub fn letterable(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| glyph(c).is_some())
}

fn kern(a: char, b: char) -> f32 {
    let (a, b) = (a.to_ascii_uppercase(), b.to_ascii_uppercase());
    KERN.iter()
        .find(|k| k.0 == a && k.1 == b)
        .map_or(0.0, |k| f32::from(k.2) / 100.0)
}

/// Lays `text` out with `track` extra caps between letters (`layoutLine`): calls `place` with
/// each glyph's index, left x (caps) and glyph, skipping characters without one, and returns the
/// line's width in caps.
fn layout(text: &str, track: f32, mut place: impl FnMut(usize, f32, &'static Glyph)) -> f32 {
    let (mut x, mut end) = (0.0, 0.0);
    let mut chars = text.chars().enumerate().peekable();
    while let Some((i, ch)) = chars.next() {
        let Some(g) = glyph(ch) else { continue };
        place(i, x, g);
        let w = f32::from(g.width) / 100.0;
        end = x + w;
        if let Some(&(_, next)) = chars.peek() {
            x += w + WEIGHT + track + kern(ch, next);
        }
    }
    end
}

pub fn width(text: &str, track: f32) -> f32 {
    layout(text, track, |_, _, _| {})
}

/// The paths of `g`: (points as hundredths, closed, dot) for each.
fn paths(g: &'static Glyph) -> impl Iterator<Item = (&'static [u8], bool, bool)> {
    let mut rest = g.strokes;
    std::iter::from_fn(move || {
        let (&head, tail) = rest.split_first()?;
        let n = usize::from(head & 0x3f);
        let (points, after) = tail.split_at(2 * n);
        rest = after;
        Some((points, head & C != 0, head & D != 0))
    })
}

/// FNV-1a over the table in the trailer's order and encoding (Step 1's script).
pub fn table_checksum() -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut mix = |v: u32| {
        for byte in v.to_le_bytes() {
            h = (h ^ u32::from(byte)).wrapping_mul(0x0100_0193);
        }
    };
    for g in &GLYPHS {
        mix(g.ch as u32);
        mix(u32::from(g.width));
        mix(paths(g).count() as u32);
        for (points, closed, dot) in paths(g) {
            mix(u32::from(closed) | u32::from(dot) << 1);
            mix((points.len() / 2) as u32);
            for &v in points {
                mix(u32::from(v));
            }
        }
    }
    h
}

/// A beam stroke's measures, px: cap height, beam width `t`, intensity, whether the three R/G/B
/// strands show (caps of 12 pt and up; below, one white stroke) and the turn glints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beam {
    pub cap: f32,
    pub t: f32,
    pub intensity: f32,
    pub strands: bool,
    pub glints: bool,
}

impl Beam {
    pub fn new(cap_px: f32, ppp: f32, intensity: f32) -> Beam {
        Beam {
            cap: cap_px,
            t: (0.028 * cap_px).max(1.5),
            intensity,
            strands: cap_px / ppp >= 12.0,
            glints: true,
        }
    }
}

/// Where the text sits relative to `origin.x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
}

/// While a line is revealed, how many glyphs' worth of the reveal each glyph takes to draw (the
/// trailer's `REVEAL_SPAN`).
const REVEAL_SPAN: f32 = 2.5;

/// A glyph path's points (hundredths) at `cap` px, in px from the glyph's origin, into `line`;
/// a closed path repeats its first point at the end. Returns how many.
fn path_points(points: &[u8], closed: bool, cap: f32, line: &mut [Pt; MAX_SEGMENTS + 1]) -> usize {
    let n = (points.len() / 2).min(MAX_SEGMENTS + 1);
    for (k, p) in line[..n].iter_mut().enumerate() {
        *p = pt(f32::from(points[2 * k]), f32::from(points[2 * k + 1])) * (cap / 100.0);
    }
    if closed && n > 2 && n - 1 < MAX_SEGMENTS {
        line[n] = line[0];
        n + 1
    } else {
        n
    }
}

/// Fills a polyline (`line`: px from `at`, the cap-line left) as capsules `width` wide, offset
/// by `shift`, in one `fill` call.
#[allow(clippy::too_many_arguments)]
fn stroke(
    t: &mut dyn Target,
    clip: DeviceRect,
    line: &[Pt],
    at: Pt,
    shift: Pt,
    width: f32,
    paint: &Paint,
    blend: Blend,
) {
    let mut shapes = [Shape::disc(pt(0.0, 0.0), 0.0); MAX_SEGMENTS];
    let mut len = 0;
    for (shape, ends) in shapes.iter_mut().zip(line.windows(2)) {
        *shape = Shape::line(at + shift + ends[0], at + shift + ends[1], width);
        len += 1;
    }
    fill(t, clip, &shapes[..len], paint, 1.0, blend);
}

/// The three strands: (channel colour, offset in t along both axes) — red up-left, green
/// centred, blue down-right (the trailer's `strands`).
fn strand_colours(palette: &Palette) -> [(Rgb, f32); 3] {
    [
        (palette.red, -0.5),
        (palette.green, 0.0),
        (palette.blue, 0.5),
    ]
}

/// A dot of beam light at `c`: a halo, the three strands as discs (or one white disc), a white
/// core (the trailer's `drawDot`).
fn beam_dot(t: &mut dyn Target, clip: DeviceRect, palette: &Palette, c: Pt, beam: &Beam) {
    let (w, k) = (beam.t, beam.intensity);
    let add = Blend::Add;
    fill(
        t,
        clip,
        &[Shape::disc(c, 2.6 * w)],
        &Paint::Solid(palette.letter_halo.alpha(0.06 * k)),
        1.0,
        add,
    );
    if beam.strands {
        for (rgb, s) in strand_colours(palette) {
            fill(
                t,
                clip,
                &[Shape::disc(c + pt(s * w, s * w), 1.25 * w)],
                &Paint::Solid(rgb.alpha(0.85 * k)),
                1.0,
                add,
            );
        }
    } else {
        fill(
            t,
            clip,
            &[Shape::disc(c, 1.25 * w)],
            &Paint::Solid(palette.white.alpha(0.85 * k)),
            1.0,
            add,
        );
    }
    fill(
        t,
        clip,
        &[Shape::disc(c, 0.75 * w)],
        &Paint::Solid(palette.white.alpha(0.95 * k)),
        1.0,
        add,
    );
}

/// A polyline (`line`: px from `at`) as a beam (the trailer's `drawBeamPath`): a faint halo,
/// the R/G/B strands side by side (or one white stroke), a white core.
fn beam_strokes(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    at: Pt,
    line: &[Pt],
    beam: &Beam,
) {
    let (w, k) = (beam.t, beam.intensity);
    let (add, o) = (Blend::Add, pt(0.0, 0.0));
    let halo = Paint::Solid(palette.letter_halo.alpha(0.06 * k));
    stroke(t, clip, line, at, o, 5.0 * w, &halo, add);
    if beam.strands {
        for (rgb, s) in strand_colours(palette) {
            let strand = Paint::Solid(rgb.alpha(0.85 * k));
            stroke(t, clip, line, at, pt(s * w, s * w), 1.5 * w, &strand, add);
        }
    } else {
        let white = Paint::Solid(palette.white.alpha(0.85 * k));
        stroke(t, clip, line, at, o, 1.5 * w, &white, add);
    }
    let core = Paint::Solid(palette.white.alpha(0.95 * k));
    stroke(t, clip, line, at, o, 0.6 * w, &core, add);
}

/// The trailer's `drawSparkle`: a four-point star of light at `at`, `r` px across its long ray,
/// at alpha `a`: a glint 0.35r round, then two rays (r × 0.05r across, 0.05r × 0.6r down) as
/// capsules, each lit by a radial from the centre.
pub fn sparkle(t: &mut dyn Target, clip: DeviceRect, palette: &Palette, at: Pt, r: f32, a: f32) {
    let add = Blend::Add;
    let g = 0.35 * r;
    let glint = Paint::Radial {
        centre: at,
        radius: g,
        stops: Stops::new(&[
            (0.0, palette.white.alpha(a)),
            (0.25, palette.letter_glint[0].alpha(0.4 * a)),
            (1.0, palette.letter_glint[1].alpha(0.0)),
        ]),
    };
    fill(t, clip, &[Shape::disc(at, g)], &glint, 1.0, add);
    for (w, h) in [(r, 0.05 * r), (0.05 * r, 0.6 * r)] {
        let ray = Paint::Radial {
            centre: at,
            radius: w.max(h),
            stops: Stops::new(&[
                (0.0, palette.white.alpha(0.9 * a)),
                (1.0, palette.letter_glint[0].alpha(0.0)),
            ]),
        };
        let thin = w.min(h);
        let reach = pt(w - thin, h - thin);
        let shape = Shape::line(at - reach, at + reach, 2.0 * thin);
        fill(t, clip, &[shape], &ray, 1.0, add);
    }
}

/// The first `drawn` (0–1) of polyline `whole` by length, into `line` (the trailer's
/// `drawGlyphPart` walk): how many points it holds, and how many of `whole`'s it reached whole.
/// Summed in one order, so at 1 every corner is reached exactly.
fn partial_path(whole: &[Pt], drawn: f32, line: &mut [Pt; MAX_SEGMENTS + 1]) -> (usize, usize) {
    let n = whole.len();
    let len = |j: usize| (whole[j] - whole[j - 1]).len();
    let goal = (1..n).map(len).sum::<f32>() * drawn;
    let mut walked = 0.0;
    line[0] = whole[0];
    let (mut m, mut reached) = (1, 1);
    for j in 1..n {
        if walked >= goal {
            break;
        }
        let (a, b, l) = (whole[j - 1], whole[j], len(j));
        line[m] = if walked + l <= goal {
            reached += 1;
            b
        } else {
            a + (b - a) * ((goal - walked) / l)
        };
        m += 1;
        walked += l;
    }
    (m, reached)
}

/// Part of a glyph, as a beam writing it has got to (the trailer's `drawGlyphPart`): `drawn`
/// (0–1) of each stroke's length from its start, in beams, with a sparkle (7t) at each tip; its
/// dots fading in from 60% drawn. Over its last 20% the tips' sparkles fade out and the glints
/// of the turns already drawn fade in (the trailer swaps them at once: a pop), so at 1 it is
/// `beam_glyph` exactly.
fn beam_glyph_part(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    g: &'static Glyph,
    at: Pt,
    beam: &Beam,
    drawn: f32,
) {
    let (cap, w, k) = (beam.cap, beam.t, beam.intensity);
    // (drawn − 0.8) / 0.2, exactly 1 when drawn is.
    let settled = (5.0 * drawn - 4.0).clamp(0.0, 1.0);
    let mut whole = [pt(0.0, 0.0); MAX_SEGMENTS + 1];
    let mut line = [pt(0.0, 0.0); MAX_SEGMENTS + 1];
    for (points, closed, dot) in paths(g) {
        if dot {
            // (drawn − 0.6) / 0.4, exactly 1 when drawn is.
            let a = (2.5 * drawn - 1.5).clamp(0.0, 1.0);
            if a > 0.0 {
                let c = at + pt(f32::from(points[0]), f32::from(points[1])) * (cap / 100.0);
                beam_dot(
                    t,
                    clip,
                    palette,
                    c,
                    &Beam {
                        intensity: k * a,
                        ..*beam
                    },
                );
            }
            continue;
        }
        let n = path_points(points, closed, cap, &mut whole);
        // The points drawn, and how many of them were reached whole.
        let (m, reached) = partial_path(&whole[..n], drawn, &mut line);
        beam_strokes(t, clip, palette, at, &line[..m], beam);
        if beam.glints && settled > 0.0 {
            // As `beam_glyph`: the path's own turns (not a closed path's repeated start).
            for &c in line[..reached].iter().take(points.len() / 2 - 1).skip(1) {
                turn_glint(t, clip, palette, at + c, w, k * settled);
            }
        }
        if settled < 1.0 {
            sparkle(
                t,
                clip,
                palette,
                at + line[m - 1],
                7.0 * w,
                k * (1.0 - settled),
            );
        }
    }
}

/// One glyph's beams: halo, strands (or one white stroke), core; its dots; its turn glints.
fn beam_glyph(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    g: &'static Glyph,
    at: Pt,
    beam: &Beam,
) {
    let (cap, w, k) = (beam.cap, beam.t, beam.intensity);
    let mut line = [pt(0.0, 0.0); MAX_SEGMENTS + 1];
    for (points, closed, dot) in paths(g) {
        if dot {
            let c = at + pt(f32::from(points[0]), f32::from(points[1])) * (cap / 100.0);
            beam_dot(t, clip, palette, c, beam);
            continue;
        }
        let n = path_points(points, closed, cap, &mut line);
        beam_strokes(t, clip, palette, at, &line[..n], beam);
        if beam.glints {
            // Its turns: every point but the first and the last.
            for &c in line.iter().take(points.len() / 2 - 1).skip(1) {
                turn_glint(t, clip, palette, at + c, w, k);
            }
        }
    }
}

/// A faint hot spot where a glyph's beam turns, at `c`: 2.6t round, at intensity `k`.
fn turn_glint(t: &mut dyn Target, clip: DeviceRect, palette: &Palette, c: Pt, w: f32, k: f32) {
    let r = 2.6 * w;
    let paint = Paint::Radial {
        centre: c,
        radius: r,
        stops: Stops::new(&[
            (0.0, palette.white.alpha(0.35 * k)),
            (0.25, palette.letter_glint[0].alpha(0.4 * 0.35 * k)),
            (1.0, palette.letter_glint[1].alpha(0.0)),
        ]),
    };
    fill(t, clip, &[Shape::disc(c, r)], &paint, 1.0, Blend::Add);
}

/// A lit target in place of an O (the trailer's `drawTarget`), centred in the glyph's box.
fn target_o(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    centre: Pt,
    cap: f32,
    beam: &Beam,
) {
    let (r, w, k) = (cap / 2.0, beam.t, beam.intensity);
    let add = Blend::Add;
    fill(
        t,
        clip,
        &[Shape::ring(centre, r, 5.0 * w)],
        &Paint::Solid(palette.letter_halo.alpha(0.05 * k)),
        1.0,
        add,
    );
    for (n, rgb) in [palette.red, palette.green, palette.blue]
        .into_iter()
        .enumerate()
    {
        let (start, sweep) = (n as f32 * TAU / 3.0 + 0.16, TAU / 3.0 - 0.32);
        fill(
            t,
            clip,
            &[Shape::arc(centre, r, 1.7 * w, start, sweep)],
            &Paint::Solid(rgb.alpha(k)),
            1.0,
            add,
        );
        fill(
            t,
            clip,
            &[Shape::arc(centre, r, 0.5 * w, start, sweep)],
            &Paint::Solid(palette.white.alpha(0.7 * k)),
            1.0,
            add,
        );
    }
    let core = Paint::Radial {
        centre,
        radius: 0.5 * r,
        stops: Stops::new(&[
            (0.0, palette.white.alpha(k)),
            (0.45, palette.white.alpha(0.9 * k)),
            (1.0, palette.letter_core.alpha(0.0)),
        ]),
    };
    fill(t, clip, &[Shape::disc(centre, 0.5 * r)], &core, 1.0, add);
}

/// `text` in beams, added into `t` (a light tile), its cap line's left end at `origin`; glyph
/// `target_o_at` (an O) becomes a lit target. Returns the line's width in px.
#[allow(clippy::too_many_arguments)]
pub fn beam_line(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    text: &str,
    origin: Pt,
    track: f32,
    beam: &Beam,
    target_o_at: Option<usize>,
) -> f32 {
    beam_line_revealed(
        t,
        clip,
        palette,
        text,
        origin,
        track,
        beam,
        target_o_at,
        1.0,
    )
}

/// How much of the glyph at char index `i` of a `count`-char line `reveal` (0–1) has drawn:
/// `clamp((reveal · (count + 2.5) − i) / 2.5)`, 1 from a reveal of 1.
fn drawn_at(reveal: f32, count: f32, i: usize) -> f32 {
    if reveal >= 1.0 {
        1.0
    } else {
        ((reveal * (count + REVEAL_SPAN) - i as f32) / REVEAL_SPAN).clamp(0.0, 1.0)
    }
}

/// Whether `beam_line_revealed` draws `text` at `reveal` exactly as `beam_line` does: every
/// glyph drawn whole (an empty text or one with no glyph, always).
pub fn reveal_whole(text: &str, reveal: f32) -> bool {
    let count = text.chars().count() as f32;
    text.chars()
        .enumerate()
        .filter(|&(_, c)| glyph(c).is_some())
        .last()
        .is_none_or(|(i, _)| drawn_at(reveal, count, i) >= 1.0)
}

/// `beam_line` written on as a beam would write it (the trailer's `reveal`, 0–1): glyph `i` (its
/// char index) of a `count`-char line is `clamp((reveal · (count + 2.5) − i) / 2.5)` drawn
/// (`beam_glyph_part`), so the glyphs are drawn one after another, overlapping; a target O
/// brightens instead. At 1 it is `beam_line`.
#[allow(clippy::too_many_arguments)]
pub fn beam_line_revealed(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    text: &str,
    origin: Pt,
    track: f32,
    beam: &Beam,
    target_o_at: Option<usize>,
    reveal: f32,
) -> f32 {
    // `layout`'s indices are char indices, so the count is of chars (the trailer's every char
    // has a glyph).
    let count = if reveal < 1.0 {
        text.chars().count() as f32
    } else {
        0.0
    };
    let cap = beam.cap;
    layout(text, track, |i, x, g| {
        let drawn = drawn_at(reveal, count, i);
        if drawn <= 0.0 {
            return;
        }
        let at = origin + pt(x * cap, 0.0);
        if Some(i) == target_o_at {
            let lit = Beam {
                intensity: beam.intensity * drawn,
                ..*beam
            };
            target_o(
                t,
                clip,
                palette,
                at + pt(f32::from(g.width) / 200.0 * cap, cap / 2.0),
                cap,
                &lit,
            );
        } else if drawn >= 1.0 {
            beam_glyph(t, clip, palette, g, at, beam);
        } else {
            beam_glyph_part(t, clip, palette, g, at, beam, drawn);
        }
    }) * cap
}

/// Ink lettering's measures (spec §10 light, ruling L6): cap height, strand width `t` (px), alpha
/// (a fade), and whether the three strands show (caps of 12 pt and up; below, one ink stroke).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ink {
    pub cap: f32,
    pub t: f32,
    pub alpha: f32,
    pub strands: bool,
}

impl Ink {
    pub fn new(cap_px: f32, ppp: f32, t_px: f32, alpha: f32) -> Ink {
        Ink {
            cap: cap_px,
            t: t_px,
            alpha,
            strands: cap_px / ppp >= 12.0,
        }
    }
}

const INK_STRAND_ALPHA: f32 = 0.95;
const INK_CORE_ALPHA: f32 = 0.55;
const INK_CORE_WIDTH: f32 = 0.55;
const INK_DOT: f32 = 1.1;

/// The most segments one glyph's paths hold (`every_glyph_fits_one_ink_union` checks the
/// charset).
const GLYPH_SEGMENTS: usize = 4 * MAX_SEGMENTS;

/// A glyph's segments (`segs`: px from `at`) in ink: each R/G/B strand multiplied as **one
/// union** (or one ink stroke), then the ink core over them, also one union. Where a glyph's own
/// paths cross (X, 4, A's bar) each strand is laid once, as the comp strokes one `Path2D` per
/// line (review finding 10).
fn ink_strokes(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    at: Pt,
    segs: &[(Pt, Pt)],
    ink: &Ink,
) {
    let (w, k) = (ink.t, ink.alpha);
    let mut shapes = [Shape::disc(pt(0.0, 0.0), 0.0); GLYPH_SEGMENTS];
    let n = segs.len().min(GLYPH_SEGMENTS);
    let mut union = |t: &mut dyn Target, shift: Pt, width: f32, paint: &Paint, blend: Blend| {
        for (shape, &(a, b)) in shapes.iter_mut().zip(&segs[..n]) {
            *shape = Shape::line(at + shift + a, at + shift + b, width);
        }
        fill(t, clip, &shapes[..n], paint, 1.0, blend);
    };
    if ink.strands {
        for (rgb, s) in strand_colours(palette) {
            let strand = Paint::Solid(rgb.alpha(INK_STRAND_ALPHA * k));
            union(t, pt(s * w, s * w), 1.5 * w, &strand, Blend::Multiply);
        }
    } else {
        let one = Paint::Solid(palette.text.primary.alpha(k));
        union(t, pt(0.0, 0.0), 1.5 * w, &one, Blend::Over);
    }
    let core = Paint::Solid(palette.ink_core.alpha(INK_CORE_ALPHA * k));
    union(t, pt(0.0, 0.0), INK_CORE_WIDTH * w, &core, Blend::Over);
}

/// A dot in ink: three multiplied discs (1.1t) at the strand offsets, or one ink disc.
fn ink_dot(t: &mut dyn Target, clip: DeviceRect, palette: &Palette, c: Pt, ink: &Ink) {
    let (w, k) = (ink.t, ink.alpha);
    if ink.strands {
        for (rgb, s) in strand_colours(palette) {
            let disc = [Shape::disc(c + pt(s * w, s * w), INK_DOT * w)];
            let paint = Paint::Solid(rgb.alpha(INK_STRAND_ALPHA * k));
            fill(t, clip, &disc, &paint, 1.0, Blend::Multiply);
        }
    } else {
        let disc = [Shape::disc(c, INK_DOT * w)];
        let paint = Paint::Solid(palette.text.primary.alpha(k));
        fill(t, clip, &disc, &paint, 1.0, Blend::Over);
    }
}

/// The target O in ink: its R/G/B arcs (1.7t, the beam O's gaps) and a primary-text dot of r
/// 0.32 × its radius (spec §10 light).
fn ink_target_o(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    centre: Pt,
    cap: f32,
    ink: &Ink,
) {
    let (r, w, k) = (cap / 2.0, ink.t, ink.alpha);
    for (n, rgb) in [palette.red, palette.green, palette.blue]
        .into_iter()
        .enumerate()
    {
        let (start, sweep) = (n as f32 * TAU / 3.0 + 0.16, TAU / 3.0 - 0.32);
        let arc = [Shape::arc(centre, r, 1.7 * w, start, sweep)];
        fill(t, clip, &arc, &Paint::Solid(rgb.alpha(k)), 1.0, Blend::Over);
    }
    let dot = [Shape::disc(centre, 0.32 * r)];
    let paint = Paint::Solid(palette.text.primary.alpha(k));
    fill(t, clip, &dot, &paint, 1.0, Blend::Over);
}

/// One glyph in ink, `drawn` (0–1) of each stroke from its start: every path's drawn segments
/// gathered, then inked as one union per strand; its dots fade in from 60% (discs of their own,
/// at their own alpha).
fn ink_glyph(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    g: &'static Glyph,
    at: Pt,
    ink: &Ink,
    drawn: f32,
) {
    let cap = ink.cap;
    let mut whole = [pt(0.0, 0.0); MAX_SEGMENTS + 1];
    let mut line = [pt(0.0, 0.0); MAX_SEGMENTS + 1];
    let mut segs = [(pt(0.0, 0.0), pt(0.0, 0.0)); GLYPH_SEGMENTS];
    let mut n = 0;
    for (points, closed, dot) in paths(g) {
        if dot {
            // (drawn − 0.6) / 0.4, exactly 1 when drawn is.
            let a = (2.5 * drawn - 1.5).clamp(0.0, 1.0);
            if a > 0.0 {
                let c = at + pt(f32::from(points[0]), f32::from(points[1])) * (cap / 100.0);
                let faded = Ink {
                    alpha: ink.alpha * a,
                    ..*ink
                };
                ink_dot(t, clip, palette, c, &faded);
            }
            continue;
        }
        let k = path_points(points, closed, cap, &mut whole);
        let (m, _) = partial_path(&whole[..k], drawn, &mut line);
        for pair in line[..m].windows(2) {
            if n < GLYPH_SEGMENTS {
                segs[n] = (pair[0], pair[1]);
                n += 1;
            }
        }
    }
    ink_strokes(t, clip, palette, at, &segs[..n], ink);
}

/// `text` in ink (spec §10 light, ruling L6), its cap line's left end at `origin`, glyph
/// `target_o_at` (an O) a target, written on to `reveal` (0–1) as `beam_line_revealed` writes
/// beams; no glints, no sparkle. Returns the line's width in px.
#[allow(clippy::too_many_arguments)]
pub fn ink_line(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    text: &str,
    origin: Pt,
    track: f32,
    ink: &Ink,
    target_o_at: Option<usize>,
    reveal: f32,
) -> f32 {
    let count = if reveal < 1.0 {
        text.chars().count() as f32
    } else {
        0.0
    };
    let cap = ink.cap;
    layout(text, track, |i, x, g| {
        let drawn = drawn_at(reveal, count, i);
        if drawn <= 0.0 {
            return;
        }
        let at = origin + pt(x * cap, 0.0);
        if Some(i) == target_o_at {
            let centre = at + pt(f32::from(g.width) / 200.0 * cap, cap / 2.0);
            let faded = Ink {
                alpha: ink.alpha * drawn,
                ..*ink
            };
            ink_target_o(t, clip, palette, centre, cap, &faded);
        } else {
            ink_glyph(t, clip, palette, g, at, ink, drawn);
        }
    }) * cap
}

/// `text` as flat strokes `stroke_w` px wide in `colour` (the picker's digits, Next ▶'s "NEXT"):
/// one capsule chain per path, a disc of 0.9 × stroke per dot. Returns the width in px.
#[allow(clippy::too_many_arguments)]
pub fn flat_line(
    t: &mut dyn Target,
    clip: DeviceRect,
    text: &str,
    origin: Pt,
    cap: f32,
    track: f32,
    stroke_w: f32,
    colour: Rgba,
    align: Align,
    blend: Blend,
) -> f32 {
    let width = width(text, track) * cap;
    let left = match align {
        Align::Left => origin.x,
        Align::Centre => origin.x - width / 2.0,
    };
    let paint = Paint::Solid(colour);
    let mut line = [pt(0.0, 0.0); MAX_SEGMENTS + 1];
    layout(text, track, |_, x, g| {
        let at = pt(left + x * cap, origin.y);
        for (points, closed, dot) in paths(g) {
            if dot {
                let c = at + pt(f32::from(points[0]), f32::from(points[1])) * (cap / 100.0);
                fill(
                    t,
                    clip,
                    &[Shape::disc(c, 0.9 * stroke_w)],
                    &paint,
                    1.0,
                    blend,
                );
            } else {
                let n = path_points(points, closed, cap, &mut line);
                let o = pt(0.0, 0.0);
                stroke(t, clip, &line[..n], at, o, stroke_w, &paint, blend);
            }
        }
    });
    width
}

/// CHROMATRON's cap height (px) when the wordmark is `width_px` wide.
pub fn wordmark_cap(width_px: f32) -> f32 {
    width_px / WORDMARK_WIDTH
}

/// The brand row (spec §7.5, the revised comps' `brand`): "CHROMATRON" 0.74 × `col_w` wide (track
/// 0.36, stroke 0.05·cap, its first O a lit target), the R/G/B underline merging into one beam,
/// and the game number at 0.5·cap after it, centred vertically in the row. px; returns the cap.
#[allow(clippy::too_many_arguments)]
pub fn wordmark(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    left: f32,
    row_top: f32,
    row_h: f32,
    col_w: f32,
    game: u8,
    ppp: f32,
) -> f32 {
    let width = 0.74 * col_w;
    let cap = wordmark_cap(width);
    let w = (0.05 * cap).max(1.5);
    let sub = 0.5 * cap;
    let line_y = 1.32 * cap;
    let y = row_top + (row_h - (line_y + sub / 2.0)) / 2.0;
    let gx = left + width + 0.35 * cap;
    let (ly, d, run, end) = (y + line_y, 0.09 * cap, 0.5 * cap, gx - 0.25 * cap);
    if palette.light_mode == LightMode::Over {
        // Ruling L6: the wordmark in ink; the underline's strands multiplied; its core in ink.
        let word = Ink {
            cap,
            t: w,
            alpha: 1.0,
            strands: cap / ppp >= 12.0,
        };
        let o = pt(left, y);
        ink_line(t, clip, palette, "CHROMATRON", o, 0.36, &word, Some(3), 1.0);
        underline(
            t,
            clip,
            palette,
            (left, ly, d, run, end, w),
            Blend::Multiply,
        );
        let core = Shape::line(pt(left + run + d, ly), pt(end, ly), 0.5 * w);
        let paint = Paint::Solid(palette.ink_core.alpha(0.6));
        fill(t, clip, &[core], &paint, 1.0, Blend::Over);
        let mut digit = [0u8; 2];
        let number_ink = Ink {
            cap: sub,
            t: 0.9 * w,
            alpha: 1.0,
            strands: sub / ppp >= 12.0,
        };
        let text = number(usize::from(game), &mut digit);
        let o = pt(gx, ly - sub / 2.0);
        ink_line(t, clip, palette, text, o, 0.3, &number_ink, None, 1.0);
        return cap;
    }
    let word = Beam {
        cap,
        t: w,
        intensity: 1.0,
        strands: cap / ppp >= 12.0,
        glints: true,
    };
    beam_line(
        t,
        clip,
        palette,
        "CHROMATRON",
        pt(left, y),
        0.36,
        &word,
        Some(3),
    );
    underline(t, clip, palette, (left, ly, d, run, end, w), Blend::Add);
    let core = Shape::line(pt(left + run + d, ly), pt(end, ly), 0.5 * w);
    fill(
        t,
        clip,
        &[core],
        &Paint::Solid(palette.white.alpha(0.9)),
        1.0,
        Blend::Add,
    );
    let mut digit = [0u8; 2];
    let number_beam = Beam {
        cap: sub,
        t: 0.9 * w,
        intensity: 1.0,
        strands: sub / ppp >= 12.0,
        glints: false,
    };
    beam_line(
        t,
        clip,
        palette,
        number(usize::from(game), &mut digit),
        pt(gx, ly - sub / 2.0),
        0.3,
        &number_beam,
        None,
    );
    cap
}

/// The wordmark's R/G/B underline (`left`, line y, strand spacing, fade run, end, beam width):
/// three strands fading in over `run`, then merging into one line, in `blend`.
fn underline(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    (left, ly, d, run, end, w): (f32, f32, f32, f32, f32, f32),
    blend: Blend,
) {
    for (rgb, dy) in [(palette.red, -d), (palette.green, 0.0), (palette.blue, d)] {
        let fade = Paint::Linear {
            from: pt(left, 0.0),
            to: pt(left + run, 0.0),
            stops: Stops::new(&[(0.0, rgb.alpha(0.0)), (1.0, rgb.alpha(0.95))]),
        };
        let path = [
            Shape::line(pt(left, ly + dy), pt(left + run, ly + dy), 1.2 * w),
            Shape::line(
                pt(left + run, ly + dy),
                pt(left + run + dy.abs(), ly),
                1.2 * w,
            ),
            Shape::line(pt(left + run + dy.abs(), ly), pt(end, ly), 1.2 * w),
        ];
        fill(t, clip, &path, &fade, 1.0, blend);
    }
}

/// "LEVEL n" for strip slot `slot` (n = slot + 1).
pub fn level_label(slot: usize, buf: &mut [u8; 9]) -> &str {
    buf[..6].copy_from_slice(b"LEVEL ");
    let mut digits = [0u8; 2];
    let n = number(slot + 1, &mut digits).len();
    buf[6..6 + n].copy_from_slice(&digits[..n]);
    std::str::from_utf8(&buf[..6 + n]).unwrap_or("")
}

/// `n` (0–99; larger shows its last two digits) in decimal.
pub fn number(n: usize, buf: &mut [u8; 2]) -> &str {
    let n = n % 100;
    let len = if n >= 10 {
        *buf = [b'0' + (n / 10) as u8, b'0' + (n % 10) as u8];
        2
    } else {
        buf[0] = b'0' + n as u8;
        1
    };
    std::str::from_utf8(&buf[..len]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etch::Layer;
    use crate::{DeviceRect, Rgb};

    #[test]
    fn the_table_is_the_trailers() {
        assert_eq!(
            table_checksum(),
            0xe5e5_836c,
            "trailer/lettering.mjs changed: port it again"
        );
        assert_eq!(GLYPHS.len(), CHARSET.chars().count());
        for ch in CHARSET.chars() {
            assert_eq!(glyph(ch).map(|g| g.ch), Some(ch), "{ch:?}");
        }
        assert_eq!(
            glyph('a').map(|g| g.ch),
            Some('A'),
            "ASCII letters are upper-cased"
        );
        assert!(glyph('é').is_none() && glyph('#').is_none());
    }

    #[test]
    fn widths_match_the_trailers_layout() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(
            near(width("CHROMATRON", 0.36), 12.24),
            "{}",
            width("CHROMATRON", 0.36)
        );
        assert!(near(width("YOU WIN!", 0.3), 7.34));
        assert!(near(width("4 classic games · 200 levels!", 0.3), 27.98));
        assert!(
            width("LT", 0.3) < 0.62 + 0.78 + WEIGHT + 0.3,
            "kerning only tightens"
        );
        assert_eq!(width("", 0.3), 0.0);
    }

    #[test]
    fn letterable_texts() {
        assert!(letterable("You win!") && letterable("LEVEL 14") && letterable("4 × 2"));
        assert!(!letterable("4 × 2 = 8") && !letterable("é") && !letterable(""));
    }

    #[test]
    fn labels_are_formatted_without_allocating() {
        let mut buf = [0u8; 9];
        assert_eq!(level_label(13, &mut buf), "LEVEL 14");
        assert_eq!(level_label(0, &mut buf), "LEVEL 1");
        assert_eq!(level_label(49, &mut buf), "LEVEL 50");
        let mut two = [0u8; 2];
        assert_eq!(number(7, &mut two), "7");
        assert_eq!(number(50, &mut two), "50");
    }

    fn canvas(w: usize, h: usize) -> Vec<u32> {
        vec![0; w * h]
    }

    fn lit(px: &[u32], w: usize) -> impl Iterator<Item = (usize, usize, u32)> + '_ {
        px.iter()
            .enumerate()
            .filter(|(_, p)| **p != 0)
            .map(move |(i, &p)| (i % w, i / w, p))
    }

    #[test]
    fn a_beam_line_stays_in_its_box_with_a_spectral_fringe() {
        let (w, h) = (400usize, 120usize);
        let mut px = canvas(w, h);
        let mut layer = Layer {
            px: &mut px,
            width: w,
            height: h,
        };
        let beam = Beam::new(40.0, 2.0, 1.0);
        let drawn = beam_line(
            &mut layer,
            DeviceRect::new(0, 0, 400, 120),
            &Palette::DARK,
            "LEVEL 14",
            pt(20.0, 40.0),
            0.3,
            &beam,
            None,
        );
        assert!((drawn - 7.36 * 40.0).abs() < 0.5, "{drawn}");
        let reach = 3.0 * beam.t + 1.0;
        let (mut redder, mut bluer) = (false, false);
        for (x, y, p) in lit(&px, w) {
            assert!(
                x as f32 >= 20.0 - reach && x as f32 <= 20.0 + drawn + reach,
                "x {x}"
            );
            assert!(
                y as f32 >= 40.0 - reach && y as f32 <= 80.0 + reach,
                "y {y}"
            );
            let (r, b) = (p >> 16 & 0xff, p & 0xff);
            redder |= r > b + 40;
            bluer |= b > r + 40;
        }
        assert!(
            redder && bluer,
            "the R/G/B strands leave a fringe on each side"
        );
    }

    #[test]
    fn small_caps_collapse_into_one_white_stroke() {
        let beam = Beam::new(20.0, 2.0, 1.0);
        assert!(!beam.strands, "10 pt is under 12 pt");
        assert!(Beam::new(26.0, 2.0, 1.0).strands, "13 pt");
        assert_eq!(
            Beam::new(20.0, 2.0, 1.0).t,
            1.5,
            "t = max(1.5 px, 0.028·cap)"
        );
    }

    #[test]
    fn the_target_o_is_lit_at_its_centre() {
        let (w, h) = (120usize, 120usize);
        let mut px = canvas(w, h);
        let mut layer = Layer {
            px: &mut px,
            width: w,
            height: h,
        };
        beam_line(
            &mut layer,
            DeviceRect::new(0, 0, 120, 120),
            &Palette::DARK,
            "O",
            pt(20.0, 20.0),
            0.3,
            &Beam::new(80.0, 2.0, 1.0),
            Some(0),
        );
        let centre = px[60 * w + (20 + 34)];
        assert!((centre >> 8 & 0xff) > 200, "a white-hot core: {centre:06x}");
    }

    #[test]
    fn flat_lines_are_one_colour_and_centre() {
        let (w, h) = (200usize, 60usize);
        let mut px = canvas(w, h);
        let mut layer = Layer {
            px: &mut px,
            width: w,
            height: h,
        };
        let colour = Rgb(200, 100, 50).alpha(1.0);
        let width = flat_line(
            &mut layer,
            DeviceRect::new(0, 0, 200, 60),
            "14",
            pt(100.0, 10.0),
            40.0,
            0.3,
            3.0,
            colour,
            Align::Centre,
            Blend::Over,
        );
        let xs: Vec<usize> = lit(&px, w).map(|(x, _, _)| x).collect();
        let mid = (xs.iter().min().unwrap() + xs.iter().max().unwrap()) as f32 / 2.0;
        assert!((mid - 100.0).abs() < 3.0, "centred: {mid}");
        assert!((width - (0.24 + WEIGHT + 0.3 + 0.7) * 40.0).abs() < 0.5);
        for (_, _, p) in lit(&px, w) {
            let (r, g, b) = (p >> 16 & 0xff, p >> 8 & 0xff, p & 0xff);
            assert!(
                r >= g && g >= b,
                "only the colour, faded by coverage: {p:06x}"
            );
        }
    }

    use crate::bloom::Planes;

    #[test]
    fn a_line_revealed_whole_is_the_line() {
        // Ruling F9(b): at (count + 1.5) / (count + 2.5) the last glyph has just been drawn
        // whole, so every glyph goes through the reveal's loop drawn whole.
        let (w, h) = (400, 80);
        let palette = Palette::DARK;
        let beam = Beam::new(36.0, 2.0, 1.0);
        let clip = DeviceRect::new(0, 0, w, h);
        let text = "YOU WIN!";
        let count = text.chars().count() as f32;
        let mut a = Planes::new(w as usize, h as usize);
        let mut b = Planes::new(w as usize, h as usize);
        beam_line(
            &mut a,
            clip,
            &palette,
            text,
            pt(10.0, 20.0),
            0.3,
            &beam,
            None,
        );
        let reveal = (count + 1.5) / (count + 2.5);
        beam_line_revealed(
            &mut b,
            clip,
            &palette,
            text,
            pt(10.0, 20.0),
            0.3,
            &beam,
            None,
            reveal,
        );
        assert!(a.r == b.r && a.g == b.g && a.b == b.b);
        assert!(a.r.iter().any(|&v| v > 0), "the line drew");
    }

    #[test]
    fn a_reveal_said_whole_draws_the_line_exactly() {
        // M1: wherever `reveal_whole` holds the reveal is the line, bit for bit (so the card's
        // key can go back to its settled one); just before, it is not yet whole.
        let (w, h) = (400, 80);
        let palette = Palette::DARK;
        let beam = Beam::new(36.0, 2.0, 1.0);
        let clip = DeviceRect::new(0, 0, w, h);
        let text = "YOU WIN!";
        let mut line = Planes::new(w as usize, h as usize);
        beam_line(
            &mut line,
            clip,
            &palette,
            text,
            pt(10.0, 20.0),
            0.3,
            &beam,
            None,
        );
        let mut first = None;
        for k in 850..=1000 {
            let reveal = k as f32 / 1000.0;
            if !reveal_whole(text, reveal) {
                assert!(first.is_none(), "whole stays whole: {reveal}");
                continue;
            }
            first.get_or_insert(reveal);
            let mut p = Planes::new(w as usize, h as usize);
            let origin = pt(10.0, 20.0);
            beam_line_revealed(
                &mut p, clip, &palette, text, origin, 0.3, &beam, None, reveal,
            );
            assert!(p.r == line.r && p.g == line.g && p.b == line.b, "{reveal}");
        }
        let first = first.expect("whole before 1");
        assert!((first - 9.5 / 10.5).abs() < 0.002, "{first}");
        assert!(reveal_whole("", 0.0) && reveal_whole("é", 0.0));
    }

    #[test]
    fn a_reveal_counts_the_chars_layout_indexes() {
        // M5: '#' has no glyph but takes an index; the last glyph is still whole before 1.
        assert!(reveal_whole("A#B", 0.95));
        assert!(!reveal_whole("A#B", 0.8));
    }

    #[test]
    fn a_glyph_drawn_whole_is_the_glyph_and_gets_there_without_a_step() {
        // Ruling F9(b): every glyph's part at 1 is the glyph exactly (strokes, dots, glints:
        // its tips' sparkles have faded out and its glints in). Just short of 1 it is nearly
        // the glyph already, so nothing pops when it is done.
        let (w, h) = (120, 120);
        let palette = Palette::DARK;
        let beam = Beam::new(60.0, 2.0, 1.0);
        let clip = DeviceRect::new(0, 0, w, h);
        let at = pt(20.0, 25.0);
        for g in &GLYPHS {
            let mut whole = Planes::new(w as usize, h as usize);
            beam_glyph(&mut whole, clip, &palette, g, at, &beam);
            let mut part = Planes::new(w as usize, h as usize);
            beam_glyph_part(&mut part, clip, &palette, g, at, &beam, 1.0);
            assert!(
                part.r == whole.r && part.g == whole.g && part.b == whole.b,
                "{:?}",
                g.ch
            );
            let mut near = Planes::new(w as usize, h as usize);
            beam_glyph_part(&mut near, clip, &palette, g, at, &beam, 0.99);
            // The light that differs (the last 1% of each stroke, a faint sparkle, glints not
            // quite in) against the glyph's.
            let total: u64 = whole.g.iter().map(|&v| u64::from(v)).sum();
            let step: u64 = near
                .g
                .iter()
                .zip(&whole.g)
                .map(|(a, b)| u64::from(a.abs_diff(*b)))
                .sum();
            assert!(step * 25 <= total, "{:?}: {step} of {total} differs", g.ch);
        }
    }

    #[test]
    fn a_tips_sparkle_hands_over_to_the_glints_over_the_last_fifth() {
        // "L" (one turn): at 60% its tip sparkles at full and its turn has no glint; at 90%
        // the sparkle is at half and the glint half in.
        let (w, h) = (120, 120);
        let palette = Palette::DARK;
        let beam = Beam::new(60.0, 2.0, 1.0);
        let clip = DeviceRect::new(0, 0, w, h);
        let at = pt(20.0, 25.0);
        let l = glyph('L').unwrap();
        let corner = (25 + 60) * w as usize + 20;
        let drawn = |k: f32| {
            let mut p = Planes::new(w as usize, h as usize);
            beam_glyph_part(&mut p, clip, &palette, l, at, &beam, k);
            p
        };
        let mut whole = Planes::new(w as usize, h as usize);
        beam_glyph(&mut whole, clip, &palette, l, at, &beam);
        let (early, late) = (drawn(0.6), drawn(0.9));
        assert!(early.g[corner] < late.g[corner] && late.g[corner] < whole.g[corner]);
    }

    #[test]
    fn a_half_revealed_line_has_drawn_its_first_glyphs_only() {
        let (w, h) = (400, 80);
        let palette = Palette::DARK;
        let beam = Beam::new(36.0, 2.0, 1.0);
        let clip = DeviceRect::new(0, 0, w, h);
        let mut p = Planes::new(w as usize, h as usize);
        let width = beam_line_revealed(
            &mut p,
            clip,
            &palette,
            "YOU WIN!",
            pt(10.0, 20.0),
            0.3,
            &beam,
            None,
            0.4,
        );
        let lit = |x0: f32, x1: f32| {
            (20..56).any(|y| (x0 as usize..x1 as usize).any(|x| p.r[y * w as usize + x] > 400))
        };
        assert!(lit(10.0, 10.0 + 0.25 * width), "the first glyphs show");
        assert!(
            !lit(10.0 + 0.8 * width, 10.0 + width),
            "the last ones don't yet"
        );
    }

    /// The ground a sentinel test draws on in `theme`: black in the dark theme (light adds), a
    /// dim sentinel hue in the light one, so a multiplied stroke of another hue shows too.
    fn sentinel_ground(theme: crate::Theme) -> u32 {
        match theme {
            crate::Theme::Dark => 0,
            crate::Theme::Light => 0x0080_0080,
        }
    }

    /// Every px of `px` is the sentinel's hue (no green, red equal to blue); returns how many
    /// differ from `ground`.
    fn sentinel_only(px: &[u32], ground: u32, what: &str) -> usize {
        let mut drawn = 0;
        for &p in px {
            let (r, g, b) = (
                (p >> 16 & 0xff) as i32,
                (p >> 8 & 0xff) as i32,
                (p & 0xff) as i32,
            );
            assert!(g == 0 && (r - b).abs() <= 1, "{what}: {p:06x}");
            drawn += usize::from(p != ground);
        }
        drawn
    }

    #[test]
    fn a_revealed_line_and_its_sparkle_take_every_colour_from_the_palette() {
        for theme in crate::Theme::ALL {
            let sentinel = theme.palette().recoloured(Rgb(255, 0, 255));
            let ground = sentinel_ground(theme);
            let (w, h) = (500usize, 120usize);
            let mut px = vec![ground; w * h];
            let mut layer = Layer {
                px: &mut px,
                width: w,
                height: h,
            };
            let all = DeviceRect::new(0, 0, 500, 120);
            let beam = Beam::new(30.0, 2.0, 1.0);
            beam_line_revealed(
                &mut layer,
                all,
                &sentinel,
                "YOU WIN!",
                pt(10.0, 10.0),
                0.3,
                &beam,
                None,
                0.5,
            );
            sparkle(&mut layer, all, &sentinel, pt(300.0, 80.0), 20.0, 1.0);
            if theme == crate::Theme::Light {
                // The light header written on in ink (ruling L6), its O a target.
                let ink = Ink::new(30.0, 2.0, 2.0, 1.0);
                let o = pt(10.0, 60.0);
                ink_line(
                    &mut layer,
                    all,
                    &sentinel,
                    "YOU WON",
                    o,
                    0.3,
                    &ink,
                    Some(5),
                    0.5,
                );
            }
            let lit_px = sentinel_only(&px, ground, &format!("{theme:?}"));
            assert!(
                lit_px > 100,
                "{theme:?}: the reveal and the sparkle drew: {lit_px} px"
            );
            // The sparkle's long ray reaches across, its short one down.
            let at = |x: usize, y: usize| px[y * w + x] != ground;
            assert!(
                at(300 + 15, 80) && at(300, 80 + 8) && !at(300 + 8, 80 + 8),
                "{theme:?}"
            );
        }
    }

    fn ink_on_white(text: &str, target: Option<usize>, reveal: f32) -> Vec<u32> {
        let mut px = vec![0x00ff_ffffu32; 240 * 60];
        let mut layer = Layer {
            px: &mut px,
            width: 240,
            height: 60,
        };
        let ink = Ink::new(26.0, 1.0, 2.0, 1.0);
        let clip = DeviceRect::new(0, 0, 240, 60);
        let at = pt(8.0, 17.0);
        ink_line(
            &mut layer,
            clip,
            &Palette::LIGHT,
            text,
            at,
            0.3,
            &ink,
            target,
            reveal,
        );
        px
    }

    /// Spec §10 light: the three strands multiplied overlap to near-black ink, with a spectral
    /// fringe at the edges (one channel well above the others).
    #[test]
    fn ink_lettering_is_near_black_with_a_spectral_fringe() {
        let px = ink_on_white("LEVEL 1", None, 1.0);
        let ch = |p: u32| {
            [
                (p >> 16 & 0xff) as i32,
                (p >> 8 & 0xff) as i32,
                (p & 0xff) as i32,
            ]
        };
        let darkest = px.iter().map(|&p| ch(p).iter().sum::<i32>()).min().unwrap();
        assert!(darkest < 3 * 60, "darkest {darkest}");
        let fringe = px.iter().map(|&p| ch(p)).any(|[r, g, b]| {
            let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
            hi - lo >= 60 && hi < 250
        });
        assert!(fringe, "a coloured fringe");
    }

    /// Spec §10 light: the target O keeps its R/G/B arcs and gets a #141a2e centre dot (r 0.32 ×
    /// its radius).
    #[test]
    fn an_ink_target_o_has_a_dark_dot() {
        let px = ink_on_white("CHROMATRON", Some(3), 1.0);
        let o = super::glyph('O').unwrap();
        // The O's left from `layout` itself ("CHR"'s width leaves out the gap after the R).
        let mut left = 0.0;
        layout("CHROMATRON", 0.3, |i, x, _| {
            if i == 3 {
                left = x;
            }
        });
        let x = 8.0 + (left + f32::from(o.width) / 200.0) * 26.0;
        let p = px[(17 + 13) * 240 + x.round() as usize];
        let [r, g, b] = [
            (p >> 16 & 0xff) as i32,
            (p >> 8 & 0xff) as i32,
            (p & 0xff) as i32,
        ];
        assert!(
            (r - 0x14).abs() <= 24 && (g - 0x1a).abs() <= 24 && (b - 0x2e).abs() <= 24,
            "{p:06x}"
        );
    }

    /// Review finding 10: every glyph's segments fit one ink union.
    #[test]
    fn every_glyph_fits_one_ink_union() {
        for ch in CHARSET.chars() {
            let g = super::glyph(ch).unwrap();
            let segments: usize = paths(g)
                .filter(|&(_, _, dot)| !dot)
                .map(|(points, closed, _)| points.len() / 2 - 1 + usize::from(closed))
                .sum();
            assert!(segments <= GLYPH_SEGMENTS, "{ch:?}: {segments}");
        }
    }

    /// Review finding 10: where an X's two strokes cross, the ink is laid once, so the
    /// crossing is no darker than a stroke's middle.
    #[test]
    fn crossing_strokes_of_a_glyph_ink_once() {
        let px = ink_on_white("X", None, 1.0);
        let lum = |p: u32| (p >> 16 & 0xff) + (p >> 8 & 0xff) + (p & 0xff);
        let darkest_in = |x0: usize, x1: usize, y0: usize, y1: usize| {
            (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .map(|(x, y)| lum(px[y * 240 + x]))
                .min()
                .unwrap()
        };
        let o = super::glyph('X').unwrap();
        let mid = 8.0 + f32::from(o.width) / 200.0 * 26.0;
        let (cx, cy) = (mid.round() as usize, 17 + 13);
        let crossing = darkest_in(cx - 1, cx + 2, cy - 1, cy + 2);
        let arm = darkest_in(8, cx - 6, 17, 17 + 8);
        assert!(crossing + 6 >= arm, "crossing {crossing} vs arm {arm}");
    }

    /// The reveal writes glyphs on: half-way there is less ink than whole, and none at 0.
    #[test]
    fn ink_is_written_on_by_the_reveal() {
        let inked = |px: &[u32]| px.iter().filter(|&&p| p != 0x00ff_ffff).count();
        assert_eq!(inked(&ink_on_white("YOU WIN!", None, 0.0)), 0);
        let (half, whole) = (
            inked(&ink_on_white("YOU WIN!", None, 0.5)),
            inked(&ink_on_white("YOU WIN!", None, 1.0)),
        );
        assert!(0 < half && half < whole, "{half} of {whole}");
    }

    #[test]
    fn lettering_takes_every_colour_from_the_palette() {
        for theme in crate::Theme::ALL {
            let sentinel = theme.palette().recoloured(Rgb(255, 0, 255));
            let ground = sentinel_ground(theme);
            let (w, h) = (500usize, 120usize);
            let mut px = vec![ground; w * h];
            let mut layer = Layer {
                px: &mut px,
                width: w,
                height: h,
            };
            let all = DeviceRect::new(0, 0, 500, 120);
            beam_line(
                &mut layer,
                all,
                &sentinel,
                "YOU WIN!",
                pt(10.0, 10.0),
                0.3,
                &Beam::new(30.0, 2.0, 1.0),
                None,
            );
            // The light theme's wordmark is ink (strands multiplied, the underline too).
            wordmark(&mut layer, all, &sentinel, 10.0, 50.0, 70.0, 420.0, 3, 1.0);
            if theme == crate::Theme::Light {
                // Ink below 12 pt: one stroke, no strands.
                let ink = Ink::new(10.0, 1.0, 1.2, 1.0);
                let o = pt(300.0, 10.0);
                ink_line(
                    &mut layer, all, &sentinel, "LEVEL 9", o, 0.3, &ink, None, 1.0,
                );
            }
            let drawn = sentinel_only(&px, ground, &format!("{theme:?}"));
            assert!(drawn > 100, "{theme:?}: drew {drawn} px");
        }
    }
}
