//! `piece_art`: every piece kind's body and lights (spec §7.4), drawn from a `Palette` with a
//! `Pen` in cell units. Bodies are static and cached as sprites by the darkroom; lights go into
//! the board's light tile. Tasks 17–19 fill in the kinds.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_8, PI, TAU};

use lumen::{BLUE, Cell, Colour, Dir, GREEN, Piece, RED, WHITE};
use spectrograph::Game;

use crate::DeviceRect;
use crate::etch::{Blend, Paint, Pt, Shape, Stops, Target, clip_rect, fill, heading, pt};
use crate::spectrum::{LightMode, Palette, Rgba};

const S: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// cos and sin of k·22.5°, k = 0..16 (fixed, so every platform draws the same pixels).
const TURNS: [(f32, f32); 16] = [
    (1.0, 0.0),
    (0.923_879_5, 0.382_683_43),
    (S, S),
    (0.382_683_43, 0.923_879_5),
    (0.0, 1.0),
    (-0.382_683_43, 0.923_879_5),
    (-S, S),
    (-0.923_879_5, 0.382_683_43),
    (-1.0, 0.0),
    (-0.923_879_5, -0.382_683_43),
    (-S, -S),
    (-0.382_683_43, -0.923_879_5),
    (0.0, -1.0),
    (0.382_683_43, -0.923_879_5),
    (S, -S),
    (0.923_879_5, -0.382_683_43),
];

/// Piece-local coordinates in cell units, y down, turned clockwise so local −y points along the
/// piece's direction: `pen.at(0.0, -0.25)` is 0.25u toward `dir`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pen {
    pub centre: Pt,
    pub u: f32,
    cos: f32,
    sin: f32,
    /// The turn in radians clockwise from north.
    turn: f32,
}

impl Pen {
    /// Turned `sixteenths` × 22.5° clockwise from north.
    pub fn new(centre: Pt, u: f32, sixteenths: i32) -> Pen {
        let k = sixteenths.rem_euclid(16);
        let (cos, sin) = TURNS[k as usize];
        Pen {
            centre,
            u,
            cos,
            sin,
            turn: k as f32 * FRAC_PI_8,
        }
    }

    /// The `etch` angle (radians clockwise from north) of local angle `a`, measured clockwise
    /// from local north (−y).
    pub fn angle(&self, a: f32) -> f32 {
        self.turn + a
    }

    /// Turned toward direction `dir` (raw 8 and 9 draw as `dir & 7`).
    pub fn toward(centre: Pt, u: f32, dir: Dir) -> Pen {
        Pen::new(centre, u, 2 * i32::from(dir & 7))
    }

    /// This pen turned `spin` radians further clockwise: a piece mid-turn (spec §8). At 0 it is
    /// the pen itself, with its fixed trig, so settled pieces draw as before. (Its trig is
    /// `etch::heading`'s, so the page ships no libm `sinf`/`cosf`.)
    pub fn spun(self, spin: f32) -> Pen {
        if spin == 0.0 { self } else { self.turned(spin) }
    }

    /// `spun` mid-turn: out of line, so the trig is not copied into every pen's caller.
    #[inline(never)]
    fn turned(self, spin: f32) -> Pen {
        let turn = self.turn + spin;
        let h = heading(turn);
        Pen {
            cos: -h.y,
            sin: h.x,
            turn,
            ..self
        }
    }

    pub fn at(&self, x: f32, y: f32) -> Pt {
        let (x, y) = (x * self.u, y * self.u);
        pt(
            self.centre.x + x * self.cos - y * self.sin,
            self.centre.y + x * self.sin + y * self.cos,
        )
    }

    pub fn len(&self, f: f32) -> f32 {
        f * self.u
    }

    /// A stroke width of `f`·u, never under one device pixel.
    pub fn width(&self, f: f32) -> f32 {
        (f * self.u).max(1.0)
    }
}

/// The direction a sprite is drawn at: `dir & 7`, folded to `& 3` for kinds that look the same
/// turned 180°, and 0 for kinds without an orientation.
pub fn sprite_dir(piece: Piece, dir: Dir) -> Dir {
    let p = dir & 7;
    match piece {
        Piece::Filter
        | Piece::Splitter
        | Piece::Conduit
        | Piece::MultiFilter
        | Piece::DualReflector
        | Piece::QuadBender => p & 3,
        Piece::Empty
        | Piece::Wall
        | Piece::Pinwheel
        | Piece::Teleporter
        | Piece::MobileTeleporter
        | Piece::Twister
        | Piece::Starburst
        | Piece::OccupiedSquare => 0,
        _ => p,
    }
}

/// How many 45° steps turn a piece's drawing back onto itself (ruling M10(a)): 0 for pieces
/// without an orientation, 4 for those drawn alike turned 180°, else 8.
pub fn turn_period(piece: Piece) -> u8 {
    if sprite_dir(piece, 1) == 0 {
        0
    } else if sprite_dir(piece, 4) == 0 {
        4
    } else {
        8
    }
}

/// Whether a glass tile sits under the piece (§7.2).
pub fn has_tile(piece: Piece) -> bool {
    !matches!(
        piece,
        Piece::Empty | Piece::Wall | Piece::Conduit | Piece::OccupiedSquare
    )
}

/// What a drawing shows about its rule; angles in sixteenths of a turn (22.5°) clockwise from north.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Geometry {
    Plain,
    /// Lasers.
    Aperture {
        toward: Dir,
    },
    /// Logic gates: `inputs` are travel directions.
    Gate {
        toward: Dir,
        inputs: [Dir; 2],
    },
    /// Bar axis and silvered-face outward normals.
    Reflector {
        bar: u8,
        faces: [Option<u8>; 2],
    },
    /// Filters: light crosses at right angles.
    Slab {
        axis: u8,
    },
    /// Splitters.
    Plate {
        axis: u8,
    },
    /// Doppler: forward when travelling this way.
    Chevron {
        forward: Dir,
    },
    /// Fan: red straight, green +45°, blue +90°.
    Prism {
        apex: Dir,
    },
    Tangler {
        input: Dir,
        arms: [Dir; 2],
    },
    /// Conduits.
    Rails {
        axis: u8,
    },
    /// Multi-filter: blue on this axis, then green, red, all.
    Lines {
        blue: u8,
    },
    /// Complementor: inputs with this parity.
    Ticks {
        parity: u8,
    },
}

/// The per-piece geometric facts the bodies and lights draw from (§7.4 "Orientation").
pub fn geometry(piece: Piece, dir: Dir) -> Geometry {
    let p = dir & 7;
    let at = |k: u8| (p + k) & 7;
    let six = |v: i32| v.rem_euclid(16) as u8;
    let (p16, a16) = (2 * i32::from(p), 2 * i32::from(p & 3));
    match piece {
        Piece::Laser => Geometry::Aperture { toward: p },
        Piece::LogicGate | Piece::PrimaryLogicGate => Geometry::Gate {
            toward: p,
            inputs: [at(1), at(7)],
        },
        Piece::Mirror => Geometry::Reflector {
            bar: six(p16),
            faces: [Some(six(p16 - 4)), None],
        },
        Piece::DualReflector => Geometry::Reflector {
            bar: six(a16),
            faces: [Some(six(a16 - 4)), Some(six(a16 + 4))],
        },
        Piece::Bender => Geometry::Reflector {
            bar: six(p16 - 1),
            faces: [Some(six(p16 - 5)), None],
        },
        Piece::QuadBender => Geometry::Reflector {
            bar: six(a16 - 1),
            faces: [Some(six(a16 - 5)), Some(six(a16 + 3))],
        },
        Piece::Filter => Geometry::Slab { axis: p & 3 },
        Piece::Splitter => Geometry::Plate { axis: p & 3 },
        Piece::Doppler => Geometry::Chevron { forward: at(2) },
        Piece::Prism => Geometry::Prism { apex: at(2) },
        Piece::Tangler => Geometry::Tangler {
            input: at(2),
            arms: [p, at(4)],
        },
        Piece::Conduit => Geometry::Rails { axis: p & 3 },
        Piece::MultiFilter => Geometry::Lines { blue: p & 3 },
        Piece::Complementor => Geometry::Ticks { parity: p & 1 },
        _ => Geometry::Plain,
    }
}

/// A piece's body (§7.4): `draw_body_spun` at no spin.
pub fn draw_body(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    game: Game,
    cell: Cell,
    centre: Pt,
    u: f32,
) {
    draw_body_spun(t, clip, palette, game, cell, centre, u, 0.0);
}

/// A piece's body (§7.4), centred at `centre`, `u` px per cell, oriented by its `geometry` and
/// turned `spin` radians further clockwise (mid-turn; 0 settled). Every kind has its arm (walls
/// are masonry's); `every_kind_draws_a_body` catches a kind whose geometry doesn't match its
/// arm, which would draw nothing.
#[allow(clippy::too_many_arguments)]
pub fn draw_body_spun(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    game: Game,
    cell: Cell,
    centre: Pt,
    u: f32,
    spin: f32,
) {
    let mut art = Art {
        t,
        clip,
        palette,
        blend: Blend::Over,
        spin,
    };
    let c = cell.colour;
    let toward = |d: Dir| Pen::toward(centre, u, d).spun(spin);
    let g = geometry(cell.piece, cell.dir);
    let reflector = |art: &mut Art, kind: Reflector| {
        if let Geometry::Reflector { bar, faces } = g {
            art.reflector(
                Pen::new(centre, u, i32::from(bar)).spun(spin),
                kind,
                faces[1].is_some(),
            );
        }
    };
    match cell.piece {
        Piece::Empty | Piece::Wall => {}
        Piece::Laser => {
            if let Geometry::Aperture { toward: d } = g {
                art.laser(toward(d), c);
            }
        }
        Piece::Mirror | Piece::DualReflector => reflector(&mut art, MIRROR),
        Piece::Bender | Piece::QuadBender => reflector(&mut art, BENDER),
        Piece::Filter => {
            if let Geometry::Slab { axis } = g {
                art.filter(toward(axis), c);
            }
        }
        Piece::Prism => {
            if let Geometry::Prism { apex } = g {
                art.prism(toward(apex));
            }
        }
        Piece::Doppler => {
            // The slab lies on the axis that local +x crosses: forward is the pen's dir + 2.
            if let Geometry::Chevron { forward } = g {
                art.doppler(toward((forward + 6) & 7));
            }
        }
        Piece::Splitter => {
            if let Geometry::Plate { axis } = g {
                art.splitter(toward(axis));
            }
        }
        Piece::Tangler => {
            if let Geometry::Tangler { input, arms } = g {
                art.tangler(centre, u, input, arms);
            }
        }
        Piece::Pinwheel => art.target(toward(0), c),
        Piece::Conduit => {
            if let Geometry::Rails { axis } = g {
                art.conduit(toward(axis));
            }
        }
        Piece::Teleporter => art.teleporter(centre, u),
        Piece::MobileTeleporter => {
            art.teleporter(centre, u);
            // Chromatron 3 draws it exactly as the teleporter (its sprite is the teleporter's).
            if game == Game::C4 {
                art.brackets(toward(0), 0.12, 0.4);
            }
        }
        Piece::MultiFilter => {
            if let Geometry::Lines { blue } = g {
                art.multi_filter(centre, u, blue);
            }
        }
        Piece::Twister => art.twister(toward(0)),
        Piece::Complementor => {
            if let Geometry::Ticks { parity } = g {
                art.complementor(centre, u, cell.dir, parity);
            }
        }
        Piece::LogicGate | Piece::PrimaryLogicGate => {
            if let Geometry::Gate { toward: d, inputs } = g {
                art.gate(toward(d), d, inputs, cell.piece == Piece::PrimaryLogicGate);
            }
        }
        Piece::Starburst => art.starburst(centre, u),
        Piece::OccupiedSquare => art.occupied(toward(0)),
    }
}

/// A mirror-like slab (§7.4 Mirror and Bender): `len` × 0.15u, bowed `bow` toward local +x in
/// the middle (concave toward the silvered −x face), its glass's two stops from `glass`.
#[derive(Clone, Copy)]
struct Reflector {
    len: f32,
    bow: f32,
    glass: fn(&Palette) -> [Rgba; 2],
}

const MIRROR: Reflector = Reflector {
    len: 0.86,
    bow: 0.0,
    glass: |p| p.mirror_glass,
};
const BENDER: Reflector = Reflector {
    len: 0.62,
    bow: 0.06,
    glass: |p| p.bender_glass,
};
/// The reflector slab's thickness.
const SLAB: f32 = 0.15;
/// Segments of a bowed slab's and bar's polylines.
const BOW_STEPS: usize = 8;

/// Where the bodies and lights draw: one target, one clip, one palette, one blend (bodies
/// `Over`, lights `Add`), and the piece's spin mid-turn (0 settled).
struct Art<'a, 'b> {
    t: &'a mut dyn Target,
    clip: DeviceRect,
    palette: &'b Palette,
    blend: Blend,
    spin: f32,
}

impl Art<'_, '_> {
    /// A pen turned toward `dir`, at the piece's spin.
    fn toward(&self, centre: Pt, u: f32, dir: Dir) -> Pen {
        Pen::toward(centre, u, dir).spun(self.spin)
    }

    /// The piece's unturned pen (north), at its spin.
    fn flat(&self, centre: Pt, u: f32) -> Pen {
        Pen::new(centre, u, 0).spun(self.spin)
    }

    fn paint(&mut self, shapes: &[Shape], paint: &Paint) {
        fill(self.t, self.clip, shapes, paint, 1.0, self.blend);
    }

    /// A radial dot of `radius`·u at `at`, through `stops`.
    fn glow(&mut self, pen: &Pen, at: Pt, radius: f32, stops: &[(f32, Rgba)]) {
        let radius = pen.len(radius);
        let paint = Paint::Radial {
            centre: at,
            radius,
            stops: Stops::new(stops),
        };
        self.paint(&[Shape::disc(at, radius)], &paint);
    }

    fn solid(&mut self, shapes: &[Shape], colour: Rgba) {
        self.paint(shapes, &Paint::Solid(colour));
    }

    /// Capsule 0.66u × 0.36u running back from an aperture 0.25u toward the pen's dir.
    fn laser(&mut self, pen: Pen, c: Colour) {
        let pal = self.palette;
        let (len, wide, mid) = (0.66, 0.36, -0.03);
        let (front, back) = (mid - len / 2.0 + wide / 2.0, mid + len / 2.0 - wide / 2.0);
        let metal = linear(
            &pen,
            (-wide / 2.0, 0.0),
            (wide / 2.0, 0.0),
            &[
                (0.0, pal.metal[0].opaque()),
                (0.45, pal.metal[1].opaque()),
                (1.0, pal.metal[2].opaque()),
            ],
        );
        self.paint(
            &[Shape::line(
                pen.at(0.0, front),
                pen.at(0.0, back),
                pen.len(wide),
            )],
            &metal,
        );
        let (r, w) = (pen.len(wide / 2.0), pen.width(0.014));
        self.solid(
            &[
                Shape::arc(pen.at(0.0, front), r, w, pen.angle(-FRAC_PI_2), PI),
                Shape::arc(pen.at(0.0, back), r, w, pen.angle(FRAC_PI_2), PI),
                Shape::line(pen.at(-wide / 2.0, front), pen.at(-wide / 2.0, back), w),
                Shape::line(pen.at(wide / 2.0, front), pen.at(wide / 2.0, back), w),
            ],
            pal.rim.at(0.28),
        );
        let (y0, y1) = (mid + 0.38 * len, mid - 0.05 * len);
        let stripe = |dx: f32, w: f32| Shape::line(pen.at(dx, y0), pen.at(dx, y1), w);
        if c == WHITE {
            for (k, bit) in [RED, GREEN, BLUE].into_iter().enumerate() {
                let colour = pal.channel(bit).alpha(0.85);
                let strand = [stripe((k as f32 - 1.0) * 0.014, pen.width(0.026))];
                fill(
                    self.t,
                    self.clip,
                    &strand,
                    &Paint::Solid(colour),
                    1.0,
                    Blend::Add,
                );
            }
            self.solid(&[stripe(0.0, pen.width(0.012))], pal.fg.at(0.7));
        } else {
            self.solid(&[stripe(0.0, pen.width(0.03))], pal.mix(c).alpha(0.85));
        }
        self.solid(
            &[Shape::disc(pen.at(0.0, -0.25), pen.len(0.108))],
            pal.aperture.opaque(),
        );
    }

    /// A glass slab on the pen's axis, its bright bar on the silvered (local −x) side and a
    /// matte ink line on the other; `both` silvers both sides.
    fn reflector(&mut self, pen: Pen, kind: Reflector, both: bool) {
        let pal = self.palette;
        let g = (kind.glass)(pal);
        let (len, half, b) = (kind.len, SLAB / 2.0, kind.bow);
        let glass = linear(&pen, (-half, 0.0), (half, 0.0), &[(0.0, g[0]), (1.0, g[1])]);
        let rim = pal.rim.at(0.35);
        let rim_w = pen.width(0.012);
        // The centre line: x = −b/2 at the ends, +b/2 in the middle (the comps' quadratic).
        let x = |s: f32| -b / 2.0 + 4.0 * b * s * (1.0 - s);
        let y = |s: f32| len * (s - 0.5);
        if b == 0.0 {
            let slab = plate(&pen, (-half, -len / 2.0), (half, len / 2.0), SLAB * 0.35);
            self.paint(&[slab], &glass);
            self.solid(&[slab.outline(rim_w)], rim);
        } else {
            // Quads overlapping by about a pixel, so the union has no seams.
            let lap = 1.0 / pen.len(len);
            let mut quads = [Shape::disc(pen.centre, 0.0); BOW_STEPS];
            for (i, q) in quads.iter_mut().enumerate() {
                let s0 = (i as f32 / BOW_STEPS as f32 - lap).max(0.0);
                let s1 = ((i + 1) as f32 / BOW_STEPS as f32 + lap).min(1.0);
                *q = Shape::polygon(
                    &[
                        pen.at(x(s0) - half, y(s0)),
                        pen.at(x(s0) + half, y(s0)),
                        pen.at(x(s1) + half, y(s1)),
                        pen.at(x(s1) - half, y(s1)),
                    ],
                    0.0,
                );
            }
            self.paint(&quads, &glass);
            // The rim: both curved sides and the two flat ends.
            let mut edges = [Shape::disc(pen.centre, 0.0); 2 * BOW_STEPS + 2];
            let (left, rest) = edges.split_at_mut(BOW_STEPS);
            let (right, ends) = rest.split_at_mut(BOW_STEPS);
            bowed(left, &pen, |s| (x(s) - half, y(s)), rim_w);
            bowed(right, &pen, |s| (x(s) + half, y(s)), rim_w);
            for (end, s) in ends.iter_mut().zip([0.0, 1.0]) {
                *end = Shape::line(pen.at(x(s) - half, y(s)), pen.at(x(s) + half, y(s)), rim_w);
            }
            self.solid(&edges, rim);
        }
        // The bar and the ink line follow the centre line, shortened to 0.88 of the slab.
        let squash = 0.88;
        let bar = linear(
            &pen,
            (0.0, -squash * len / 2.0),
            (0.0, squash * len / 2.0),
            &[
                (0.0, pal.fg.at(0.35)),
                (0.5, pal.fg.at(1.0)),
                (1.0, pal.fg.at(0.35)),
            ],
        );
        let mut line = [Shape::disc(pen.centre, 0.0); BOW_STEPS];
        if both {
            for dx in [-0.038, 0.038] {
                bowed(
                    &mut line,
                    &pen,
                    |s| (x(s) + dx, squash * y(s)),
                    pen.width(0.04),
                );
                self.paint(&line, &bar);
            }
            return;
        }
        bowed(
            &mut line,
            &pen,
            |s| (x(s) - 0.035, squash * y(s)),
            pen.width(0.04),
        );
        self.paint(&line, &bar);
        bowed(
            &mut line,
            &pen,
            |s| (x(s) + 0.045, squash * y(s)),
            pen.width(0.025),
        );
        self.solid(&line, pal.hollow.at(0.6));
    }

    /// A slab 0.62u × 0.2u in the filter's colour on the pen's axis, with a white sheen.
    fn filter(&mut self, pen: Pen, c: Colour) {
        let pal = self.palette;
        let (a, b) = ((-0.1, -0.31), (0.1, 0.31));
        let slab = plate(&pen, a, b, 0.06);
        self.solid(&[slab], pal.mix(c).alpha(0.3));
        self.solid(&[slab.outline(pen.width(0.024))], pal.mix(c).alpha(0.95));
        let sheen = linear(
            &pen,
            a,
            b,
            &[
                (0.0, pal.fg.at(0.32)),
                (0.45, pal.fg.at(0.04)),
                (1.0, pal.fg.at(0.0)),
            ],
        );
        self.paint(&[slab], &sheen);
    }

    /// A glass triangle, its apex toward the pen's dir.
    fn prism(&mut self, pen: Pen) {
        let pal = self.palette;
        let r = 0.37;
        let (cx, cy) = (r * 0.866_025_4, r * 0.5);
        let glass = plate3(&pen, [(0.0, -r), (cx, cy), (-cx, cy)], 0.05);
        let fill_paint = linear(
            &pen,
            (-r, -r),
            (r * 0.6, r),
            &[
                (0.0, pal.fg.at(0.40)),
                (0.5, pal.prism_tints[0]),
                (1.0, pal.prism_tints[1]),
            ],
        );
        self.paint(&[glass], &fill_paint);
        self.solid(&[glass.outline(pen.width(0.016))], pal.rim.at(0.5));
    }

    /// A slab 0.62u × 0.24u on the pen's axis, banded R, G, B from the front, with a white
    /// chevron toward local +x.
    fn doppler(&mut self, pen: Pen) {
        let pal = self.palette;
        let (a, b) = ((-0.12, -0.31), (0.12, 0.31));
        let slab = plate(&pen, a, b, 0.06);
        // Hard band edges, softened over a device pixel.
        let e = 0.5 / pen.len(0.62);
        let band = |bit| pal.channel(bit).alpha(0.35);
        let bands = linear(
            &pen,
            (0.0, a.1),
            (0.0, b.1),
            &[
                (1.0 / 3.0 - e, band(RED)),
                (1.0 / 3.0 + e, band(GREEN)),
                (2.0 / 3.0 - e, band(GREEN)),
                (2.0 / 3.0 + e, band(BLUE)),
            ],
        );
        self.paint(&[slab], &bands);
        self.solid(&[slab.outline(pen.width(0.014))], pal.rim.at(0.35));
        let (cx, cy, w) = (0.12 * 0.35, 0.06, pen.width(0.03));
        self.solid(
            &[
                Shape::line(pen.at(-cx, -cy), pen.at(cx, 0.0), w),
                Shape::line(pen.at(cx, 0.0), pen.at(-cx, cy), w),
            ],
            pal.fg.at(0.9),
        );
    }

    /// A clear plate 0.8u × 0.12u on the pen's axis with a dashed white bar.
    fn splitter(&mut self, pen: Pen) {
        let pal = self.palette;
        let plate_shape = plate(&pen, (-0.06, -0.4), (0.06, 0.4), 0.12 * 0.4);
        self.solid(&[plate_shape], pal.rim.at(0.07));
        self.solid(&[plate_shape.outline(pen.width(0.012))], pal.rim.at(0.3));
        let (span, n) = (0.68, 5);
        let dash = span / (2 * n - 1) as f32;
        let mut dashes = [Shape::disc(pen.centre, 0.0); 5];
        for (k, d) in dashes.iter_mut().enumerate() {
            let y = -span / 2.0 + k as f32 * 2.0 * dash;
            *d = Shape::line(
                pen.at(0.0, y + dash * 0.15),
                pen.at(0.0, y + dash * 0.85),
                pen.width(0.03),
            );
        }
        self.solid(&dashes, pal.fg.at(0.8));
    }

    /// An ink core in a double ring, the outer ring broken where the input enters, and two arms.
    fn tangler(&mut self, centre: Pt, u: f32, input: Dir, arms: [Dir; 2]) {
        let pal = self.palette;
        let pen = self.flat(centre, u);
        self.solid(&[Shape::disc(centre, pen.len(0.14))], pal.hollow.at(0.9));
        self.solid(
            &[Shape::ring(centre, pen.len(0.2), pen.width(0.02))],
            pal.fg.at(0.5),
        );
        // The notch faces the side the input comes from, opposite its travel.
        let notch = f32::from((input + 4) & 7) * FRAC_PI_4;
        let gap = 20f32.to_radians();
        self.solid(
            &[Shape::arc(
                centre,
                pen.len(0.26),
                pen.width(0.016),
                pen.angle(notch + gap),
                TAU - 2.0 * gap,
            )],
            pal.fg.at(0.25),
        );
        let arm = |d: Dir| {
            let p = self.toward(centre, u, d);
            Shape::line(p.at(0.0, -0.2), p.at(0.0, -0.36), p.width(0.035))
        };
        self.solid(&[arm(arms[0]), arm(arms[1])], pal.fg.at(0.6));
    }

    /// An ink disc with an arc per required channel (or, for black, a slash) and an inner ring.
    fn target(&mut self, pen: Pen, c: Colour) {
        let pal = self.palette;
        let centre = pen.centre;
        self.solid(&[Shape::disc(centre, pen.len(0.35))], pal.hollow.at(0.85));
        let n = [RED, GREEN, BLUE].iter().filter(|&&b| c & b != 0).count();
        let gap = if n > 1 { 0.22 } else { 0.0 };
        let mut k = 0;
        for bit in [RED, GREEN, BLUE] {
            if c & bit == 0 {
                continue;
            }
            let share = TAU / n as f32;
            let arc = Shape::arc(
                centre,
                pen.len(0.3),
                pen.width(0.045),
                pen.angle(k as f32 * share + gap / 2.0),
                share - gap,
            );
            self.solid(&[arc], pal.channel(bit).alpha(0.42));
            k += 1;
        }
        if n == 0 {
            let k = 0.2 * S;
            self.solid(
                &[Shape::line(pen.at(-k, k), pen.at(k, -k), pen.width(0.03))],
                pal.fg.at(0.25),
            );
        }
        self.solid(
            &[Shape::ring(centre, pen.len(0.186), pen.width(0.01))],
            pal.fg.at(0.18),
        );
    }

    /// Two rails ±0.2u from the pen's axis with glass between, running out to the cell's edges
    /// (and no further) so neighbours join into tubes.
    fn conduit(&mut self, pen: Pen) {
        let pal = self.palette;
        let reach = 0.75;
        let w = pen.width(0.02);
        self.within(&pen, 0.5, |art| {
            art.solid(
                &[plate(&pen, (-0.2, -reach), (0.2, reach), 0.0)],
                pal.glass.at(0.05),
            );
            art.solid(
                &[
                    Shape::line(pen.at(-0.2, -reach), pen.at(-0.2, reach), w),
                    Shape::line(pen.at(0.2, -reach), pen.at(0.2, reach), w),
                ],
                pal.conduit_rail,
            );
        });
    }

    /// An ink core inside two ice rings, with eight ticks pointing in from the outer ring.
    fn teleporter(&mut self, centre: Pt, u: f32) {
        let pal = self.palette;
        let pen = self.flat(centre, u);
        let ice = pal.teleport_ice;
        self.solid(&[Shape::disc(centre, pen.len(0.12))], pal.hollow.at(0.9));
        self.solid(
            &[Shape::ring(centre, pen.len(0.22), pen.width(0.02))],
            ice.alpha(0.6),
        );
        self.solid(
            &[Shape::ring(centre, pen.len(0.32), pen.width(0.016))],
            ice.alpha(0.3),
        );
        let mut ticks = [Shape::disc(centre, 0.0); 8];
        for (d, tick) in ticks.iter_mut().enumerate() {
            let p = self.toward(centre, u, d as Dir);
            *tick = Shape::line(p.at(0.0, -0.32), p.at(0.0, -0.26), p.width(0.016));
        }
        self.solid(&ticks, ice.alpha(0.45));
    }

    /// Four corner brackets at ±0.4u with `leg`-long legs, white α `alpha`.
    fn brackets(&mut self, pen: Pen, leg: f32, alpha: f32) {
        let (e, w) = (0.4, pen.width(0.02));
        let mut legs = [Shape::disc(pen.centre, 0.0); 8];
        for (k, (sx, sy)) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .into_iter()
            .enumerate()
        {
            let corner = pen.at(sx * e, sy * e);
            legs[2 * k] = Shape::line(pen.at(sx * e, sy * (e - leg)), corner, w);
            legs[2 * k + 1] = Shape::line(corner, pen.at(sx * (e - leg), sy * e), w);
        }
        self.solid(&legs, self.palette.fg.at(alpha));
    }

    /// A clear disc crossed by four lines: blue on axis `blue`, then green, red and white on
    /// the axes 45° on, clockwise.
    fn multi_filter(&mut self, centre: Pt, u: f32, blue: Dir) {
        let pal = self.palette;
        let pen = self.flat(centre, u);
        self.solid(&[Shape::disc(centre, pen.len(0.36))], pal.glass.at(0.07));
        self.solid(
            &[Shape::ring(centre, pen.len(0.36), pen.width(0.014))],
            pal.rim.at(0.35),
        );
        let lines = [
            pal.blue.alpha(0.9),
            pal.green.alpha(0.9),
            pal.red.alpha(0.9),
            pal.fg.at(0.5),
        ];
        for (k, colour) in lines.into_iter().enumerate() {
            let p = self.toward(centre, u, blue + k as Dir);
            self.solid(
                &[Shape::line(p.at(0.0, -0.3), p.at(0.0, 0.3), p.width(0.03))],
                colour,
            );
        }
    }

    /// A glass disc with two counter-clockwise arrows (140° arcs of radius 0.22u).
    fn twister(&mut self, pen: Pen) {
        let pal = self.palette;
        let centre = pen.centre;
        self.solid(&[Shape::disc(centre, pen.len(0.36))], pal.glass.at(0.08));
        self.solid(
            &[Shape::ring(centre, pen.len(0.36), pen.width(0.014))],
            pal.rim.at(0.3),
        );
        let (r, head) = (0.22, 0.08);
        let mut shapes = [Shape::disc(centre, 0.0); 4];
        for (k, half) in [0.0, PI].into_iter().enumerate() {
            // Each arc runs anticlockwise and ends 20° below east (of its half), where its
            // head points along the travel.
            let end = half + FRAC_PI_2 + 20f32.to_radians();
            shapes[2 * k] = Shape::arc(
                centre,
                pen.len(r),
                pen.width(0.03),
                pen.angle(end),
                140f32.to_radians(),
            );
            let (out, along) = (heading(end), heading(end - FRAC_PI_2));
            let at = |o: f32, a: f32| {
                let v = out * (r + o) + along * a;
                pen.at(v.x, v.y)
            };
            shapes[2 * k + 1] = Shape::polygon(
                &[
                    at(0.0, 0.9 * head),
                    at(0.55 * head, 0.0),
                    at(-0.55 * head, 0.0),
                ],
                0.0,
            );
        }
        self.solid(&shapes, pal.fg.at(0.7));
    }

    /// A disc turned toward `dir`, its back (local −x) half dark and its front half light, with
    /// a tick outside the rim on each direction of the input `parity`.
    fn complementor(&mut self, centre: Pt, u: f32, dir: Dir, parity: u8) {
        let pal = self.palette;
        let (pen, r) = (self.toward(centre, u, dir), 0.3);
        // The dark back: the −x half disc and a strip reaching 1.5 px past the diameter, under
        // the light half's edge there (a full dark disc would show through the light arc's AA).
        let mut back = [Shape::disc(centre, 0.0); HALF_DISC_CHORDS + 2];
        back[..=HALF_DISC_CHORDS].copy_from_slice(&half_disc(&self.toward(centre, u, dir + 4), r));
        let (x0, x1) = ((0.5 / u).min(0.5 * r), (1.5 / u).min(0.5 * r));
        let y = (r * r - x1 * x1).max(0.0).sqrt();
        back[HALF_DISC_CHORDS + 1] = plate(&pen, (-x0, -y), (x1, y), 0.0);
        self.solid(&back, pal.complement[0].opaque());
        self.solid(&half_disc(&pen, r), pal.complement[1].opaque());
        self.solid(
            &[Shape::ring(centre, pen.len(r), pen.width(0.014))],
            pal.rim.at(0.35),
        );
        let mut ticks = [Shape::disc(centre, 0.0); 4];
        for (tick, d) in ticks.iter_mut().zip((parity..8).step_by(2)) {
            let p = self.toward(centre, pen.u, d);
            *tick = Shape::line(p.at(0.0, -0.33), p.at(0.0, -0.47), p.width(0.03));
        }
        self.solid(&ticks, pal.fg.at(0.7));
    }

    /// The chevron plate toward the pen's dir: two wings meet at the front, where the aperture
    /// sits on the leading edge, and an input channel runs along each wing from its side.
    /// `primary` adds the R/G/B ring around the aperture.
    fn gate(&mut self, pen: Pen, toward: Dir, inputs: [Dir; 2], primary: bool) {
        let pal = self.palette;
        // The outline dips to (0, 0) at the back, so it is filled as two convex halves that
        // overlap 0.02u across the centre line; each inner back edge stays on the outline's
        // line through (0, 0), so the union is exactly the outline.
        let dip = 0.02 * 0.33 / 0.26;
        let right = [
            (-0.02, -0.24),
            (0.13, -0.24),
            (0.4, 0.2),
            (0.26, 0.33),
            (-0.02, -dip),
        ];
        let half = |sx: f32| {
            let mut v = [pen.centre; 5];
            for (i, &(x, y)) in right.iter().enumerate() {
                // Mirroring reverses the winding; reversing the order keeps it clockwise.
                v[if sx > 0.0 { i } else { 4 - i }] = pen.at(sx * x, y);
            }
            Shape::polygon(&v, pen.len(0.007))
        };
        let metal = linear(
            &pen,
            (-0.3, 0.0),
            (0.3, 0.0),
            &[
                (0.0, pal.gate_metal[0].opaque()),
                (0.45, pal.gate_metal[1].opaque()),
                (1.0, pal.gate_metal[2].opaque()),
            ],
        );
        self.paint(&[half(1.0), half(-1.0)], &metal);
        // The rim: capsules along the seven outer edges (round joins, no seam down the middle).
        let outline = [
            (-0.13, -0.24),
            (0.13, -0.24),
            (0.4, 0.2),
            (0.26, 0.33),
            (0.0, 0.0),
            (-0.26, 0.33),
            (-0.4, 0.2),
        ];
        let mut rim = [Shape::disc(pen.centre, 0.0); 7];
        for (i, edge) in rim.iter_mut().enumerate() {
            let (a, b) = (outline[i], outline[(i + 1) % 7]);
            *edge = Shape::line(pen.at(a.0, a.1), pen.at(b.0, b.1), pen.width(0.014));
        }
        self.solid(&rim, pal.gate_rim);
        // An input travelling `i` enters from side `i + 4`: back left (dir + 5) or back right.
        let mut channels = [Shape::disc(pen.centre, 0.0); 2];
        for (channel, i) in channels.iter_mut().zip(inputs) {
            let sx = if (i + 12 - toward) & 7 == 5 {
                -1.0
            } else {
                1.0
            };
            *channel = Shape::line(
                pen.at(sx * 0.31, 0.24),
                pen.at(sx * 0.06, -0.17),
                pen.width(0.02),
            );
        }
        self.solid(&channels, pal.gate_channel);
        let aperture = pen.at(0.0, -0.24);
        self.solid(
            &[Shape::disc(aperture, pen.len(0.1))],
            pal.aperture.opaque(),
        );
        self.solid(
            &[Shape::ring(aperture, pen.len(0.1), pen.width(0.022))],
            pal.fg.at(0.7),
        );
        if primary {
            let third = 120f32.to_radians();
            let gap = 10f32.to_radians();
            for (k, bit) in [RED, GREEN, BLUE].into_iter().enumerate() {
                let arc = Shape::arc(
                    aperture,
                    pen.len(0.145),
                    pen.width(0.025),
                    pen.angle(k as f32 * third + gap),
                    third - 2.0 * gap,
                );
                self.solid(&[arc], pal.channel(bit).alpha(0.75));
            }
        }
    }

    /// Eight rays (long orthogonal, short diagonal) from a glass centre.
    fn starburst(&mut self, centre: Pt, u: f32) {
        let pal = self.palette;
        let mut rays = [Shape::disc(centre, 0.0); 8];
        for (d, ray) in rays.iter_mut().enumerate() {
            let p = self.toward(centre, u, d as Dir);
            let len = if d & 1 == 0 { 0.42 } else { 0.3 };
            *ray = Shape::line(p.at(0.0, -0.1), p.at(0.0, -len), p.width(0.03));
        }
        self.solid(&rays, pal.fg.at(0.6));
        let pen = self.flat(centre, u);
        let r = pen.len(0.1);
        self.solid(&[Shape::disc(centre, r)], pal.rim.at(0.22));
        self.solid(&[Shape::ring(centre, r, pen.width(0.014))], pal.rim.at(0.6));
    }

    /// Corner brackets over a faint crosshatch inside ±0.36u.
    fn occupied(&mut self, pen: Pen) {
        let pal = self.palette;
        self.brackets(pen, 0.14, 0.25);
        let mut hatch = [Shape::disc(pen.centre, 0.0); 26];
        let w = pen.width(0.01);
        for k in 0..13 {
            let o = (k as f32 - 6.0) * 0.12;
            hatch[2 * k] = Shape::line(pen.at(o - 1.0, 1.0), pen.at(o + 1.0, -1.0), w);
            hatch[2 * k + 1] = Shape::line(pen.at(o - 1.0, -1.0), pen.at(o + 1.0, 1.0), w);
        }
        self.within(&pen, 0.36, |art| art.solid(&hatch, pal.fg.at(0.06)));
    }

    /// Runs `draw` clipped (also) to the square ±`half`·u around the pen's centre, rounded to
    /// device pixels.
    fn within(&mut self, pen: &Pen, half: f32, draw: impl FnOnce(&mut Self)) {
        let (c, h) = (pen.centre, pen.len(half));
        let square = DeviceRect::new(
            (c.x - h).round() as i32,
            (c.y - h).round() as i32,
            (c.x + h).round() as i32,
            (c.y + h).round() as i32,
        );
        let Some(clip) = clip_rect(self.clip, square) else {
            return;
        };
        let outer = self.clip;
        self.clip = clip;
        draw(self);
        self.clip = outer;
    }
}

/// Steps of the half disc's main polygon and of each sliver between its chords and the arc.
const HALF_DISC_CHORDS: usize = 6;
const SLIVER_STEPS: usize = 4;

/// The half disc of radius `r` on the pen's local +x side, as convex pieces: a 7-gon on the
/// diameter and six slivers out to the arc (7.5° steps). Each sliver reaches 1.5 px inside its
/// chord (at most half the radius, so tiny cells don't fold), so the union has no seams.
fn half_disc(pen: &Pen, r: f32) -> [Shape; HALF_DISC_CHORDS + 1] {
    let step = PI / HALF_DISC_CHORDS as f32;
    let on = |a: f32, radius: f32| {
        let h = heading(a);
        pen.at(h.x * radius, h.y * radius)
    };
    let mut main = [pen.centre; HALF_DISC_CHORDS + 1];
    for (k, v) in main.iter_mut().enumerate() {
        *v = on(k as f32 * step, r);
    }
    let mut pieces = [Shape::polygon(&main, 0.0); HALF_DISC_CHORDS + 1];
    let inner = (r - 1.5 / pen.u).max(0.5 * r);
    for (k, sliver) in pieces[1..].iter_mut().enumerate() {
        let a0 = k as f32 * step;
        let mut v = [pen.centre; SLIVER_STEPS + 3];
        v[0] = on(a0, inner);
        for (j, p) in v[1..=SLIVER_STEPS + 1].iter_mut().enumerate() {
            *p = on(a0 + j as f32 * step / SLIVER_STEPS as f32, r);
        }
        v[SLIVER_STEPS + 2] = on(a0 + step, inner);
        *sliver = Shape::polygon(&v, 0.0);
    }
    pieces
}

/// A linear gradient between two pen points.
fn linear(pen: &Pen, from: (f32, f32), to: (f32, f32), stops: &[(f32, Rgba)]) -> Paint {
    Paint::Linear {
        from: pen.at(from.0, from.1),
        to: pen.at(to.0, to.1),
        stops: Stops::new(stops),
    }
}

/// The pen-turned rect from corner `a` to corner `b` (local), its corners rounded by `round`.
fn plate(pen: &Pen, a: (f32, f32), b: (f32, f32), round: f32) -> Shape {
    Shape::polygon(
        &[
            pen.at(a.0, a.1),
            pen.at(b.0, a.1),
            pen.at(b.0, b.1),
            pen.at(a.0, b.1),
        ],
        pen.len(round),
    )
}

/// The pen-turned triangle `v` (clockwise on screen), its corners rounded by `round`.
fn plate3(pen: &Pen, v: [(f32, f32); 3], round: f32) -> Shape {
    Shape::polygon(&v.map(|(x, y)| pen.at(x, y)), pen.len(round))
}

/// `out.len()` capsules `width` wide along the local curve `f(s)`, s = 0..1.
fn bowed(out: &mut [Shape], pen: &Pen, f: impl Fn(f32) -> (f32, f32), width: f32) {
    let n = out.len() as f32;
    for (i, shape) in out.iter_mut().enumerate() {
        let (a, b) = (f(i as f32 / n), f((i + 1) as f32 / n));
        *shape = Shape::line(pen.at(a.0, a.1), pen.at(b.0, b.1), width);
    }
}

/// How a palette's lights land: added into a light tile (dark), painted over (light, L3).
pub fn light_blend(palette: &Palette) -> Blend {
    match palette.light_mode {
        LightMode::Additive => Blend::Add,
        LightMode::Over => Blend::Over,
    }
}

/// A piece's lights (§7.4 "Light" column): `draw_light_spun` at no spin.
#[allow(clippy::too_many_arguments)]
pub fn draw_light(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    game: Game,
    cell: Cell,
    received: Colour,
    emitting: bool,
    centre: Pt,
    u: f32,
) {
    draw_light_spun(
        t, clip, palette, game, cell, received, emitting, centre, u, 0.0,
    );
}

/// A piece's lights (§7.4 "Light" column), added into a light target: `received` is every
/// colour entering the cell, `emitting` whether it sends light out toward its `dir`; turned
/// `spin` radians further clockwise (mid-turn; 0 settled).
#[allow(clippy::too_many_arguments)]
pub fn draw_light_spun(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    game: Game,
    cell: Cell,
    received: Colour,
    emitting: bool,
    centre: Pt,
    u: f32,
    spin: f32,
) {
    // Every game lights its kinds alike (C3's mobile teleporter included).
    let _ = game;
    let mut art = Art {
        t,
        clip,
        palette,
        blend: light_blend(palette),
        spin,
    };
    let pal = palette;
    let c = cell.colour;
    let toward = |d: Dir| Pen::toward(centre, u, d).spun(spin);
    let flat = Pen::new(centre, u, 0).spun(spin);
    let white = |a: f32| pal.white.alpha(a);
    match geometry(cell.piece, cell.dir) {
        Geometry::Aperture { toward: d } => {
            let pen = toward(d);
            let at = pen.at(0.0, -0.25);
            // The halo colour (dark: the mix, as `halo` is the mix there; light white: blue-grey).
            art.solid(&[Shape::disc(at, pen.len(0.075))], pal.halo(c).opaque());
            art.solid(&[Shape::disc(at, pen.len(0.04))], pal.core(c).opaque());
        }
        Geometry::Reflector { bar, faces } => {
            let kind = if matches!(cell.piece, Piece::Bender | Piece::QuadBender) {
                BENDER
            } else {
                MIRROR
            };
            art.reflector_glint(
                Pen::new(centre, u, i32::from(bar)).spun(spin),
                kind,
                faces[1].is_some(),
            );
        }
        Geometry::Prism { apex } => {
            let pen = toward(apex);
            for (k, bit) in [RED, GREEN, BLUE].into_iter().enumerate() {
                let h = heading(k as f32 * FRAC_PI_4) * 0.3;
                let tick = Shape::line(pen.at(0.0, -0.2), pen.at(h.x, h.y - 0.2), pen.width(0.022));
                art.solid(&[tick], pal.channel(bit).alpha(0.5));
            }
        }
        Geometry::Tangler { arms, .. } => {
            let tip = |d: Dir, r: f32| Shape::disc(toward(d).at(0.0, -0.36), flat.len(r));
            if pal.light_mode == LightMode::Over {
                // Light: white's halo round a white core, as the aperture (white on a white
                // tile is nothing).
                let halo = pal.halo(WHITE).opaque();
                art.solid(&[tip(arms[0], 0.03), tip(arms[1], 0.03)], halo);
                art.solid(&[tip(arms[0], 0.016), tip(arms[1], 0.016)], white(1.0));
            } else {
                art.solid(&[tip(arms[0], 0.03), tip(arms[1], 0.03)], white(0.9));
            }
        }
        Geometry::Gate { toward: d, .. } if emitting => {
            let pen = toward(d);
            art.solid(
                &[Shape::disc(pen.at(0.0, -0.24), pen.len(0.04))],
                white(1.0),
            );
        }
        _ => match cell.piece {
            Piece::Pinwheel => art.target_light(flat, c, received),
            Piece::Teleporter | Piece::MobileTeleporter => art.glow(
                &flat,
                centre,
                0.15,
                &[(0.0, white(0.3)), (1.0, pal.teleport_ice.alpha(0.0))],
            ),
            // Light: white's halo under a white core (white on a glass centre is nothing).
            Piece::Starburst if pal.light_mode == LightMode::Over => {
                let halo = pal.halo(WHITE);
                art.glow(
                    &flat,
                    centre,
                    0.12,
                    &[(0.0, halo.alpha(0.5)), (1.0, halo.alpha(0.0))],
                );
                art.solid(&[Shape::disc(centre, flat.len(0.04))], white(1.0));
            }
            Piece::Starburst => art.glow(
                &flat,
                centre,
                0.12,
                &[(0.0, white(0.75)), (1.0, pal.star_glint.alpha(0.0))],
            ),
            _ => {}
        },
    }
}

impl Art<'_, '_> {
    /// The faint glint line along a reflector's bright bar (both bars when `both`).
    fn reflector_glint(&mut self, pen: Pen, kind: Reflector, both: bool) {
        let (len, b) = (kind.len, kind.bow);
        let mut line = [Shape::disc(pen.centre, 0.0); BOW_STEPS];
        let bars: &[f32] = if both { &[-0.038, 0.038] } else { &[-0.035] };
        // The body's centre line (bowed `b`), a little longer than its bar.
        let x = |s: f32| -b / 2.0 + 4.0 * b * s * (1.0 - s);
        for &dx in bars {
            bowed(
                &mut line,
                &pen,
                |s| (x(s) + dx, 0.95 * len * (s - 0.5)),
                pen.width(0.03),
            );
            self.solid(&line, self.palette.mirror_glint);
        }
    }

    /// Per required channel received, its arc (dimmed when spoilt); when lit, a core and a ring
    /// in the target's colour (black: an `fg(0.5)` rim), all from the palette's `lit` tone; the
    /// light theme first lays a halo under the arcs.
    fn target_light(&mut self, pen: Pen, c: Colour, received: Colour) {
        let pal = self.palette;
        let lit = pal.lit;
        let centre = pen.centre;
        let complete = received == c;
        // Light: a halo under the arcs (spec §7.4), in the lit tone (below).
        if let (true, Some((a, r))) = (complete && c != 0, lit.halo) {
            let tone = pal.halo(c);
            self.glow(
                &pen,
                centre,
                r,
                &[(0.0, tone.alpha(a)), (1.0, tone.alpha(0.0))],
            );
        }
        // Light: a spoilt black target is shaded in ink, not left a bare white disc.
        if let (true, Some((shade, r))) = (c == 0 && !complete, lit.spoilt_shade) {
            self.glow(
                &pen,
                centre,
                r,
                &[(0.0, shade), (1.0, Rgba { a: 0.0, ..shade })],
            );
        }
        let n = [RED, GREEN, BLUE].iter().filter(|&&b| c & b != 0).count();
        let gap = if n > 1 { 0.22 } else { 0.0 };
        let alpha = if received & !c & WHITE != 0 { 0.4 } else { 1.0 };
        for (k, bit) in [RED, GREEN, BLUE]
            .into_iter()
            .filter(|&b| c & b != 0)
            .enumerate()
        {
            if received & bit == 0 {
                continue;
            }
            let share = TAU / n as f32;
            let arc = Shape::arc(
                centre,
                pen.len(0.3),
                pen.width(lit.arc_width),
                pen.angle(k as f32 * share + gap / 2.0),
                share - gap,
            );
            self.solid(&[arc], pal.channel(bit).alpha(alpha));
        }
        if !complete {
            return;
        }
        if c == 0 {
            self.solid(
                &[Shape::ring(centre, pen.len(0.35), pen.width(0.02))],
                pal.fg.at(0.5),
            );
            return;
        }
        // The lit light's tone: the mix, but white's halo for white (light: blue-grey, as white
        // on a white disc reads as nothing; dark: white, the mix).
        let tone = pal.halo(c);
        let mid = match lit.yellow_mid {
            Some(y) if c == RED | GREEN => y.opaque(),
            _ => tone.alpha(lit.core_mid),
        };
        self.glow(
            &pen,
            centre,
            lit.core_radius,
            &[
                (0.0, pal.white.opaque()),
                (0.35, mid),
                (1.0, tone.alpha(lit.core_end)),
            ],
        );
        self.solid(
            &[Shape::ring(
                centre,
                pen.len(0.186),
                pen.width(lit.ring_width),
            )],
            tone.alpha(lit.ring_alpha),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::etch::Layer;
    use crate::spectrum::Theme;
    use lumen::{
        BeamField, Board, DX, DY, RuleSet, board_index, doppler_backward, doppler_forward, trace,
        transfer,
    };

    /// Spec §7.4 light lit target: a white core, the ring in the mix at r 0.186u, and a halo in
    /// the mix out past the arcs (r 0.45u is between the arcs and the halo's 0.62u edge).
    #[test]
    fn a_lit_light_target_glows_white_in_its_colour() {
        let mut px = vec![0x00ff_ffffu32; 128 * 128];
        let mut layer = Layer {
            px: &mut px,
            width: 128,
            height: 128,
        };
        let cell = Cell {
            piece: Piece::Pinwheel,
            colour: RED,
            dir: 0,
        };
        draw_light(
            &mut layer,
            DeviceRect::new(0, 0, 128, 128),
            &Palette::LIGHT,
            Game::C1,
            cell,
            RED,
            false,
            pt(64.0, 64.0),
            100.0,
        );
        let rgb = |x: usize, y: usize| {
            let p = px[y * 128 + x];
            [
                (p >> 16 & 0xff) as i32,
                (p >> 8 & 0xff) as i32,
                (p & 0xff) as i32,
            ]
        };
        let core = rgb(64, 64);
        assert!(core.iter().all(|&c| c >= 235), "core {core:?}");
        let ring = rgb(64 + 19, 64); // 0.186u
        assert!(ring[0] > 200 && ring[1] < 140, "ring {ring:?}");
        let halo = rgb(64 + 45, 64); // 0.45u
        assert!(halo[0] - halo[1] >= 25, "halo {halo:?}");
    }

    /// Spec §7.4 light laser aperture: the halo colour (white: blue-grey) at r 0.075u around a
    /// white centre.
    #[test]
    fn a_white_lasers_light_aperture_has_a_blue_grey_halo() {
        let mut px = vec![0u32; 128 * 128];
        let mut layer = Layer {
            px: &mut px,
            width: 128,
            height: 128,
        };
        let cell = Cell {
            piece: Piece::Laser,
            colour: WHITE,
            dir: 0,
        };
        draw_light(
            &mut layer,
            DeviceRect::new(0, 0, 128, 128),
            &Palette::LIGHT,
            Game::C1,
            cell,
            0,
            true,
            pt(64.0, 64.0),
            100.0,
        );
        // The aperture sits 0.25u north; 0.06u east of it is inside the halo, outside the core.
        let p = px[(64 - 25) * 128 + 64 + 6];
        let (r, b) = ((p >> 16 & 0xff) as i32, (p & 0xff) as i32);
        assert!(b > r + 40, "{p:06x}");
        let centre = px[(64 - 25) * 128 + 64];
        assert_eq!(centre, 0x00ff_ffff, "a white core");
    }

    /// `cell`'s light-theme lights, receiving `received` and emitting, on a white 256 × 256 canvas
    /// (u = 200, centred): an [r, g, b] reader of the result.
    fn light_on_white(cell: Cell, received: Colour) -> impl Fn(usize, usize) -> [i32; 3] {
        let mut px = vec![0x00ff_ffffu32; 256 * 256];
        let mut layer = Layer {
            px: &mut px,
            width: 256,
            height: 256,
        };
        draw_light(
            &mut layer,
            DeviceRect::new(0, 0, 256, 256),
            &Palette::LIGHT,
            Game::C1,
            cell,
            received,
            true,
            pt(128.0, 128.0),
            200.0,
        );
        move |x, y| {
            let p = px[y * 256 + x];
            [
                (p >> 16 & 0xff) as i32,
                (p >> 8 & 0xff) as i32,
                (p & 0xff) as i32,
            ]
        }
    }

    /// Still review finding 3: a spoilt black target is shaded in ink (`rgba(27,35,64,.12)` → 0
    /// over 0.45u), not left a bare white disc that reads half lit.
    #[test]
    fn a_spoilt_black_target_is_shaded_in_ink_in_light() {
        let black = Cell {
            piece: Piece::Pinwheel,
            colour: 0,
            dir: 0,
        };
        let rgb = light_on_white(black, WHITE);
        let centre = rgb(128, 128);
        assert!(
            centre[1] <= 235 && centre[2] > centre[0],
            "ink at its centre: {centre:?}"
        );
        let mid = rgb(128 + 40, 128); // 0.2u
        assert!(mid[1] <= 247, "the shade at 0.2u: {mid:?}");
        assert_eq!(rgb(128 + 92, 128), [255; 3], "nothing past 0.45u");
    }

    /// Still review finding 4: the tangler's arm tips are `halo(WHITE)` discs (r 0.03u) round a
    /// white core (r 0.016u), so they show on a white tile.
    #[test]
    fn the_tanglers_arm_tips_are_blue_grey_round_white_in_light() {
        let tangler = Cell {
            piece: Piece::Tangler,
            colour: 0,
            dir: 0,
        };
        let rgb = light_on_white(tangler, WHITE);
        // The north arm's tip is 0.36u up, at (128, 56): r 6 px of halo round r 3.2 px of white.
        // All round the tip, between the core and the halo's edge (review T16 M3: not one px).
        for k in 0..16 {
            let a = k as f32 * std::f32::consts::TAU / 16.0;
            let (x, y) = (128.0 + 4.6 * a.cos(), 56.0 + 4.6 * a.sin());
            let rim = rgb(x.round() as usize, y.round() as usize);
            assert!(
                rim[2] - rim[0] >= 50,
                "the tip's blue-grey at {k}/16: {rim:?}"
            );
        }
        for (x, y) in [(128, 56), (127, 56), (129, 56), (128, 55), (128, 57)] {
            assert!(
                rgb(x, y).iter().all(|&c| c >= 245),
                "its white core at ({x}, {y}): {:?}",
                rgb(x, y)
            );
        }
    }

    /// Still review finding 7: the starburst's glint is a `halo(WHITE)` glow (α 0.5 → 0 over
    /// 0.12u) under a white core (r 0.04u).
    #[test]
    fn the_starbursts_glint_is_blue_grey_round_white_in_light() {
        let star = Cell {
            piece: Piece::Starburst,
            colour: 0,
            dir: 0,
        };
        let rgb = light_on_white(star, 0);
        let glow = rgb(128 + 10, 128);
        assert!(
            glow[0] <= 235 && glow[2] - glow[0] >= 15,
            "the glow: {glow:?}"
        );
        assert!(rgb(128, 128).iter().all(|&c| c >= 250), "the white core");
    }

    /// The travel direction of a beam travelling `d` after reflecting off a bar at `bar` sixteenths.
    fn reflect(bar: u8, d: Dir) -> Dir {
        ((2 * i32::from(bar) - 2 * i32::from(d)).rem_euclid(16) / 2) as Dir
    }

    /// Whether a beam travelling `d` strikes a face with outward normal `n` (less than 90° from head-on).
    fn strikes(n: u8, d: Dir) -> bool {
        let diff = (2 * i32::from(d) - (i32::from(n) + 8)).rem_euclid(16);
        diff <= 3 || diff >= 13
    }

    fn emissions(rules: &RuleSet, cell: Cell, d: Dir, c: Colour) -> Vec<(Dir, Colour)> {
        let mut tag = 1;
        transfer(rules, cell, d, c, 0, &mut tag)
            .as_slice()
            .iter()
            .map(|e| (e.dir, e.colour))
            .collect()
    }

    /// Traces `pieces` (x, y, piece, colour, dir) on an empty board of `game` and returns the field.
    fn traced(game: Game, pieces: &[(usize, usize, Piece, u8, u8)]) -> BeamField {
        let mut board = Board::new(game);
        for &(x, y, piece, colour, dir) in pieces {
            board.cells[board_index(x, y)] = Cell { piece, colour, dir };
        }
        let mut field = BeamField::default();
        trace(&board, &mut field);
        field
    }

    /// A red laser 3 cells back from (7, 7), so its beam enters (7, 7) travelling `d`.
    fn feeder(d: Dir) -> (usize, usize, Piece, u8, u8) {
        let (x, y) = (
            7 - 3 * i32::from(DX[usize::from(d)]),
            7 - 3 * i32::from(DY[usize::from(d)]),
        );
        (x as usize, y as usize, Piece::Laser, 4, d)
    }

    #[test]
    fn every_drawing_shows_its_rule() {
        let centre = board_index(7, 7);
        for piece in Piece::ALL {
            for p in 0..8u8 {
                let g = geometry(piece, p);
                for d in 0..8u8 {
                    let rules = &RuleSet::C4;
                    let what = format!("{piece:?} dir {p}, beam travelling {d}");
                    match g {
                        Geometry::Reflector { bar, faces } => {
                            let out = emissions(
                                rules,
                                Cell {
                                    piece,
                                    colour: 0,
                                    dir: p,
                                },
                                d,
                                7,
                            );
                            let hit = faces.iter().flatten().any(|&n| strikes(n, d));
                            assert_eq!(
                                out.iter().any(|&(o, _)| o == reflect(bar, d)),
                                hit,
                                "{what}: {out:?}"
                            );
                        }
                        Geometry::Slab { axis } => {
                            let out = emissions(
                                rules,
                                Cell {
                                    piece,
                                    colour: 4,
                                    dir: p,
                                },
                                d,
                                7,
                            );
                            assert_eq!(!out.is_empty(), (d + 8 - axis) % 4 == 2, "{what}");
                        }
                        Geometry::Plate { axis } => {
                            for rules in [&RuleSet::C1, &RuleSet::C2] {
                                let out = emissions(
                                    rules,
                                    Cell {
                                        piece,
                                        colour: 0,
                                        dir: p,
                                    },
                                    d,
                                    7,
                                );
                                match (d + 8 - axis) % 4 {
                                    0 => assert!(out.is_empty(), "{what}"),
                                    2 => assert!(out.iter().any(|&(o, _)| o == d), "{what}"),
                                    _ => assert!(
                                        out.iter().any(|&(o, _)| o == d)
                                            && out.iter().any(|&(o, _)| o == reflect(2 * axis, d)),
                                        "{what}: {out:?}"
                                    ),
                                }
                            }
                        }
                        Geometry::Chevron { forward } => {
                            let out = emissions(
                                rules,
                                Cell {
                                    piece,
                                    colour: 0,
                                    dir: p,
                                },
                                d,
                                4,
                            );
                            let want = if d == forward {
                                vec![(d, doppler_forward(4))]
                            } else if d == (forward + 4) & 7 {
                                vec![(d, doppler_backward(4))]
                            } else {
                                vec![]
                            };
                            assert_eq!(out, want, "{what}");
                        }
                        Geometry::Prism { apex } => {
                            let out = emissions(
                                rules,
                                Cell {
                                    piece,
                                    colour: 0,
                                    dir: p,
                                },
                                d,
                                7,
                            );
                            if d == apex {
                                for want in [(apex, 4), ((apex + 1) & 7, 2), ((apex + 2) & 7, 1)] {
                                    assert!(out.contains(&want), "{what}: {out:?}");
                                }
                            }
                        }
                        Geometry::Tangler { input, arms } => {
                            let out = emissions(
                                rules,
                                Cell {
                                    piece,
                                    colour: 0,
                                    dir: p,
                                },
                                d,
                                4,
                            );
                            assert_eq!(!out.is_empty(), d == input, "{what}");
                            assert!(out.iter().all(|(o, _)| arms.contains(o)), "{what}: {out:?}");
                        }
                        Geometry::Rails { axis } => {
                            let out = emissions(
                                rules,
                                Cell {
                                    piece,
                                    colour: 0,
                                    dir: p,
                                },
                                d,
                                7,
                            );
                            assert_eq!(out == vec![(d, 7)], (d + 8 - axis) % 4 == 0, "{what}");
                        }
                        Geometry::Lines { blue } => {
                            let out = emissions(
                                rules,
                                Cell {
                                    piece,
                                    colour: 0,
                                    dir: p,
                                },
                                d,
                                7,
                            );
                            assert_eq!(
                                out,
                                vec![(d, [1, 2, 4, 7][usize::from((d + 8 - blue) % 4)])],
                                "{what}"
                            );
                        }
                        Geometry::Ticks { parity } => {
                            let field = traced(Game::C2, &[(7, 7, piece, 0, p), feeder(d)]);
                            assert_eq!(
                                field.outbound[centre].iter().any(|&m| m != 0),
                                d & 1 == parity,
                                "{what}"
                            );
                        }
                        Geometry::Aperture { toward } if d == 0 => {
                            let field = traced(Game::C1, &[(7, 7, piece, 4, p)]);
                            for s in 0..8 {
                                assert_eq!(
                                    field.outbound[centre][s] != 0,
                                    s == usize::from(toward),
                                    "{what} side {s}"
                                );
                            }
                        }
                        Geometry::Gate { toward, inputs } if d == 0 => {
                            let both = traced(
                                Game::C4,
                                &[(7, 7, piece, 0, p), feeder(inputs[0]), feeder(inputs[1])],
                            );
                            assert_ne!(
                                both.outbound[centre][usize::from(toward)],
                                0,
                                "{what}: both inputs"
                            );
                            let one = traced(Game::C4, &[(7, 7, piece, 0, p), feeder(inputs[0])]);
                            assert!(
                                one.outbound[centre].iter().all(|&m| m == 0),
                                "{what}: one input"
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Draws one body at u = 96 on mid grey and reads the pixel at (x, y) in the frame turned
    /// toward `frame` (usually the piece's own `dir`).
    pub(crate) fn body_at(piece: Piece, colour: u8, dir: u8, frame: u8, x: f32, y: f32) -> u32 {
        let mut px = vec![0x0080_8080u32; 128 * 128];
        let mut layer = Layer {
            px: &mut px,
            width: 128,
            height: 128,
        };
        let centre = pt(64.0, 64.0);
        draw_body(
            &mut layer,
            DeviceRect::new(0, 0, 128, 128),
            &Palette::DARK,
            Game::C4,
            Cell { piece, colour, dir },
            centre,
            96.0,
        );
        let p = Pen::toward(centre, 96.0, frame).at(x, y);
        px[p.y as usize * 128 + p.x as usize]
    }

    pub(crate) fn bright(p: u32) -> u32 {
        (p >> 16 & 0xff) + (p >> 8 & 0xff) + (p & 0xff)
    }

    #[test]
    fn drawings_put_their_parts_where_geometry_says() {
        for dir in 0..8 {
            assert!(
                bright(body_at(Piece::Laser, 4, dir, dir, 0.0, -0.25)) < 30,
                "laser {dir}: the aperture is dark and in front"
            );
            assert!(
                bright(body_at(Piece::Laser, 4, dir, dir, 0.0, 0.25)) < bright(0x0080_8080),
                "the dark capsule runs back"
            );
            assert!(
                bright(body_at(Piece::Mirror, 0, dir, dir, -0.035, 0.0))
                    > bright(body_at(Piece::Mirror, 0, dir, dir, 0.045, 0.0)) + 60,
                "mirror {dir}: the bright bar on the silvered side (local −x is direction dir − 2)"
            );
            let apex = (dir + 2) & 7;
            assert_ne!(
                body_at(Piece::Prism, 0, dir, apex, 0.0, -0.25),
                0x0080_8080,
                "prism {dir}: glass toward the apex, dir + 2"
            );
            assert_eq!(
                body_at(Piece::Prism, 0, dir, apex, 0.0, 0.3),
                0x0080_8080,
                "nothing beyond the base, which faces dir + 6"
            );
        }
    }

    #[test]
    fn later_games_parts_sit_where_geometry_says() {
        for dir in 0..8 {
            assert!(
                bright(body_at(Piece::LogicGate, 0, dir, dir, 0.0, -0.24)) < 30,
                "gate {dir}: the aperture on the leading edge"
            );
            assert_eq!(
                body_at(Piece::LogicGate, 0, dir, dir, 0.0, 0.3),
                0x0080_8080,
                "the notch at the back is open"
            );
            // The red arc of the primary gate's ring runs from −80° to 20° (comps angles); −30° is on it.
            let ring = body_at(
                Piece::PrimaryLogicGate,
                0,
                dir,
                dir,
                0.145 * 0.866,
                -0.24 - 0.145 * 0.5,
            );
            assert!(
                ring >> 16 & 0xff > (ring & 0xff) + 40,
                "primary gate {dir}: the red third of the ring: {ring:06x}"
            );
            let dual = dir & 3;
            assert!(
                bright(body_at(Piece::DualReflector, 0, dir, dual, -0.038, 0.0))
                    > bright(0x0080_8080) + 60,
                "dual {dir}: silvered"
            );
            assert!(
                bright(body_at(Piece::DualReflector, 0, dir, dual, 0.038, 0.0))
                    > bright(0x0080_8080) + 60,
                "on both faces"
            );
        }
        assert_ne!(
            body_at(Piece::MobileTeleporter, 0, 0, 0, 0.4, 0.4),
            0x0080_8080,
            "C4's brackets"
        );
    }

    /// Every kind but Empty and Wall (masonry draws walls) has an arm in `draw_body`, and it
    /// draws something in every direction: a kind–geometry mismatch would draw nothing.
    #[test]
    fn every_kind_draws_a_body() {
        for piece in Piece::ALL {
            if matches!(piece, Piece::Empty | Piece::Wall) {
                continue;
            }
            for dir in 0..8 {
                let mut px = vec![0u32; 64 * 64];
                let mut layer = Layer {
                    px: &mut px,
                    width: 64,
                    height: 64,
                };
                draw_body(
                    &mut layer,
                    DeviceRect::new(0, 0, 64, 64),
                    &Palette::DARK,
                    Game::C4,
                    Cell {
                        piece,
                        colour: 4,
                        dir,
                    },
                    pt(32.0, 32.0),
                    64.0,
                );
                let drawn = px.iter().filter(|&&p| p != 0).count();
                assert!(drawn > 40, "{piece:?} dir {dir}: {drawn} px drawn");
            }
        }
    }

    #[test]
    fn bodies_take_every_colour_from_the_palette() {
        for theme in Theme::ALL {
            let sentinel = theme.palette().recoloured(crate::Rgb(255, 0, 255));
            for piece in Piece::ALL {
                for colour in [0, 1, 2, 4, 5, 7] {
                    for dir in [0, 3, 6] {
                        let mut px = vec![0u32; 64 * 64];
                        let mut layer = Layer {
                            px: &mut px,
                            width: 64,
                            height: 64,
                        };
                        draw_body(
                            &mut layer,
                            DeviceRect::new(0, 0, 64, 64),
                            &sentinel,
                            Game::C4,
                            Cell { piece, colour, dir },
                            pt(32.0, 32.0),
                            64.0,
                        );
                        for &p in &px {
                            let (r, g, b) = (p >> 16 & 0xff, p >> 8 & 0xff, p & 0xff);
                            assert!(
                                g == 0 && r.abs_diff(b) <= 1,
                                "{theme:?} {piece:?} colour {colour} dir {dir}: {p:06x}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// Review Focus 1: at clamped tiny cells the half disc's slivers stay finite and on their
    /// own (+x) side; unguarded, they reach the centre at u = 5 px (NaN normals) and fold past it.
    #[test]
    fn tiny_half_discs_neither_fold_nor_go_nan() {
        for tenth in 10..=120 {
            let u = tenth as f32 / 10.0;
            let pen = Pen::toward(pt(16.0, 16.0), u, 2);
            for shape in half_disc(&pen, 0.3) {
                let Shape::Polygon { points, len, .. } = shape else {
                    unreachable!()
                };
                for p in &points[..len] {
                    // Pen toward east: local +x is screen +y.
                    assert!(
                        p.x.is_finite() && p.y.is_finite() && p.y >= 16.0 - 1e-3,
                        "u {u}: {p:?}"
                    );
                }
            }
        }
    }

    /// Review Focus 1: clamped tiny cells draw the complementor (its half-disc slivers) without
    /// folding: the light half stays on its side and nothing lands outside the piece.
    #[test]
    fn tiny_complementors_stay_in_shape() {
        for tenth in 10..=120 {
            let u = tenth as f32 / 10.0;
            for dir in [0, 2, 3] {
                let mut px = vec![0u32; 32 * 32];
                let mut layer = Layer {
                    px: &mut px,
                    width: 32,
                    height: 32,
                };
                let centre = pt(16.0, 16.0);
                let cell = Cell {
                    piece: Piece::Complementor,
                    colour: 0,
                    dir,
                };
                draw_body(
                    &mut layer,
                    DeviceRect::new(0, 0, 32, 32),
                    &Palette::DARK,
                    Game::C4,
                    cell,
                    centre,
                    u,
                );
                // Inside the disc, the −x side is the dark back and the +x side the light front.
                let front = Pen::toward(centre, u, dir).at(1.0, 0.0) - centre;
                let (mut back_light, mut front_light) = (0, 0);
                for (i, &p) in px.iter().enumerate() {
                    let d = pt((i % 32) as f32 + 0.5, (i / 32) as f32 + 0.5) - centre;
                    assert!(
                        p == 0 || d.len() <= 0.5 * u + 2.5,
                        "u {u} dir {dir}: {p:06x} at {i}"
                    );
                    let side = (d.x * front.x + d.y * front.y) / u;
                    if d.len() < 0.25 * u && side.abs() > 0.08 * u {
                        *(if side < 0.0 {
                            &mut back_light
                        } else {
                            &mut front_light
                        }) += bright(p);
                    }
                }
                assert!(
                    back_light < front_light || (u < 8.0 && back_light == front_light),
                    "u {u} dir {dir}: back {back_light}, front {front_light}"
                );
            }
        }
    }

    fn light_total(cell: Cell, received: Colour, emitting: bool) -> [u64; 3] {
        let mut planes = crate::bloom::Planes::new(96, 96);
        draw_light(
            &mut planes,
            DeviceRect::new(0, 0, 96, 96),
            &Palette::DARK,
            Game::C4,
            cell,
            received,
            emitting,
            pt(48.0, 48.0),
            96.0,
        );
        planes.total()
    }

    #[test]
    fn targets_light_up_when_satisfied_and_dim_when_spoilt() {
        let red = Cell {
            piece: Piece::Pinwheel,
            colour: 4,
            dir: 0,
        };
        let lit = light_total(red, 4, false)[0];
        let spoilt = light_total(red, 6, false)[0];
        let dark = light_total(red, 0, false)[0];
        assert!(lit > 2 * spoilt && spoilt > dark, "{lit} {spoilt} {dark}");
        let black = Cell {
            piece: Piece::Pinwheel,
            colour: 0,
            dir: 0,
        };
        assert!(
            light_total(black, 0, false)[0] > light_total(black, 4, false)[0],
            "black is lit by nothing"
        );
        let gate = Cell {
            piece: Piece::LogicGate,
            colour: 0,
            dir: 2,
        };
        assert!(
            light_total(gate, 0, true)[0] > light_total(gate, 0, false)[0],
            "an emitting gate shows it"
        );
    }

    #[test]
    fn lights_take_every_colour_from_the_palette() {
        for theme in Theme::ALL {
            let sentinel = theme.palette().recoloured(crate::Rgb(255, 0, 255));
            for piece in Piece::ALL {
                for colour in [0, 1, 4, 7] {
                    for received in [0, colour, 7] {
                        let mut planes = crate::bloom::Planes::new(64, 64);
                        draw_light(
                            &mut planes,
                            DeviceRect::new(0, 0, 64, 64),
                            &sentinel,
                            Game::C4,
                            Cell {
                                piece,
                                colour,
                                dir: 3,
                            },
                            received,
                            true,
                            pt(32.0, 32.0),
                            64.0,
                        );
                        for i in 0..64 * 64 {
                            assert!(
                                planes.g[i] == 0 && planes.r[i].abs_diff(planes.b[i]) <= 1,
                                "{theme:?} {piece:?} {colour} {received}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// `cell`'s body at `spin`, drawn 64 × 64 (u = 64).
    fn body_px(cell: Cell, spin: f32) -> Vec<u32> {
        let mut px = vec![0u32; 64 * 64];
        let mut layer = Layer {
            px: &mut px,
            width: 64,
            height: 64,
        };
        let clip = DeviceRect::new(0, 0, 64, 64);
        let palette = Palette::DARK;
        draw_body_spun(
            &mut layer,
            clip,
            &palette,
            Game::C4,
            cell,
            pt(32.0, 32.0),
            64.0,
            spin,
        );
        px
    }

    /// The px of two 64 × 64 bodies whose channels differ by more than 3 / 255.
    fn bodies_differ(a: &[u32], b: &[u32]) -> usize {
        let channel = |p: u32, k: u32| (p >> (8 * k)) & 0xff;
        a.iter()
            .zip(b)
            .filter(|&(&p, &q)| (0..3).any(|k| channel(p, k).abs_diff(channel(q, k)) > 3))
            .count()
    }

    /// `cell`'s lights at `spin`, drawn 64 × 64 (u = 64), receiving its colour (black: white)
    /// and emitting.
    fn light_planes(cell: Cell, spin: f32) -> crate::bloom::Planes {
        let mut planes = crate::bloom::Planes::new(64, 64);
        let received = if cell.colour == 0 { 7 } else { cell.colour };
        draw_light_spun(
            &mut planes,
            DeviceRect::new(0, 0, 64, 64),
            &Palette::DARK,
            Game::C4,
            cell,
            received,
            true,
            pt(32.0, 32.0),
            64.0,
            spin,
        );
        planes
    }

    /// The px of two 64 × 64 lights whose channels differ by more than 3 / 255.
    fn lights_differ(a: &crate::bloom::Planes, b: &crate::bloom::Planes) -> usize {
        let near = crate::bloom::ONE as u16 * 3 / 255;
        (0..64 * 64)
            .filter(|&i| {
                [(&a.r, &b.r), (&a.g, &b.g), (&a.b, &b.b)]
                    .iter()
                    .any(|(p, q)| p[i].abs_diff(q[i]) > near)
            })
            .count()
    }

    /// `cell` turned one step clockwise, and the spin that draws it 45° on from `cell`'s drawing:
    /// none, except where a kind drawn alike turned 180° folds back (3 → 4 draws axis 0, not
    /// 4), which is the next drawing turned half round. (Those drawings are not exactly alike
    /// turned 180° — their sheens run one way — so a turn across the fold shows its sheen flip
    /// at its first frame.)
    fn next_direction(cell: Cell) -> (Cell, f32) {
        let next = Cell {
            dir: (cell.dir + 1) & 7,
            ..cell
        };
        let folds = turn_period(cell.piece) == 4 && cell.dir & 3 == 3;
        (next, if folds { PI } else { 0.0 })
    }

    /// Every kind that turns, in every direction and in two colours.
    fn turning_cells() -> impl Iterator<Item = Cell> {
        Piece::ALL
            .into_iter()
            .filter(|&p| turn_period(p) != 0)
            .flat_map(|piece| {
                [5u8, 7]
                    .into_iter()
                    .flat_map(move |colour| (0..8u8).map(move |dir| Cell { piece, colour, dir }))
            })
    }

    #[test]
    fn a_body_spun_by_45_degrees_is_the_next_direction_drawn() {
        let mut off_cells = Vec::new();
        for cell in turning_cells() {
            let (next, unfold) = next_direction(cell);
            let off = bodies_differ(&body_px(next, unfold), &body_px(cell, FRAC_PI_4));
            if off * 100 > 64 * 64 {
                off_cells.push((cell, off));
            }
        }
        assert!(
            off_cells.is_empty(),
            "px differ (a literal angle not turned by the pen?): {off_cells:?}"
        );
    }

    #[test]
    fn a_light_spun_by_45_degrees_is_the_next_direction_lit() {
        let mut off_cells = Vec::new();
        for cell in turning_cells() {
            let (next, unfold) = next_direction(cell);
            let off = lights_differ(&light_planes(next, unfold), &light_planes(cell, FRAC_PI_4));
            if off * 100 > 64 * 64 {
                off_cells.push((cell, off));
            }
        }
        assert!(
            off_cells.is_empty(),
            "light px differ (a literal angle not turned by the pen?): {off_cells:?}"
        );
    }

    #[test]
    fn a_full_spin_draws_as_no_spin() {
        // Spun, a pen computes its own trig; a whole turn round lands where the fixed trig
        // draws, within the tolerance a 45° spin is held to.
        for cell in turning_cells() {
            let off = bodies_differ(&body_px(cell, 0.0), &body_px(cell, TAU));
            assert!(off * 100 <= 64 * 64, "{cell:?}: {off} px differ");
            let off = lights_differ(&light_planes(cell, 0.0), &light_planes(cell, TAU));
            assert!(off * 100 <= 64 * 64, "{cell:?}: {off} light px differ");
        }
    }

    #[test]
    fn a_pen_turns_clockwise_from_north() {
        let east = Pen::toward(pt(10.0, 10.0), 4.0, 2);
        let p = east.at(0.0, -1.0);
        assert!(
            (p.x - 14.0).abs() < 1e-5 && (p.y - 10.0).abs() < 1e-5,
            "{p:?}"
        );
        let south_west = Pen::toward(pt(0.0, 0.0), 1.0, 13);
        let p = south_west.at(0.0, -1.0);
        assert!(
            (p.x + S).abs() < 1e-6 && (p.y - S).abs() < 1e-6,
            "dir 13 draws as 5: {p:?}"
        );
    }

    #[test]
    fn sprites_fold_symmetric_kinds() {
        assert_eq!(sprite_dir(Piece::Mirror, 6), 6);
        assert_eq!(sprite_dir(Piece::Filter, 6), 2);
        assert_eq!(sprite_dir(Piece::Twister, 5), 0);
        assert_eq!(sprite_dir(Piece::Laser, 9), 1);
        assert!(has_tile(Piece::Mirror) && !has_tile(Piece::Conduit) && !has_tile(Piece::Wall));
    }
}
