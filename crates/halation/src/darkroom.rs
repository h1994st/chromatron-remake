//! `darkroom`: the layers and how they become pixels (spec §8). `base` holds everything opaque
//! (backdrop, grid, tiles, walls, bodies, and the chrome: the tray's panel and wells, the inline
//! picker), each region repainted only when its inputs change; the board's light tile holds
//! beams, glints and piece lights as planar u16 light, with a glow at ½ and a bloom at ⅛
//! resolution. Any rect of the output is recomposed from them:
//! `out = base + T(0.55·bloom + 0.9·glow + light)`.
//! Every buffer is allocated in `new` or `reshape`; painting allocates nothing.

use lumen::layout::STRIP_SLOTS;
use lumen::{
    BOARD_CELLS, BeamField, Board, Cell, Colour, DX, DY, Dir, Drag, GameState, Piece, SlotState,
    VISIBLE_TOOLBOX_SLOTS, WHITE,
};
use spectrograph::Game;

use crate::backdrop::{Backdrop, Casting, Ground, MAX_CASTINGS, PanelFill};
use crate::beamline::{Beamline, GlintKind, Partial, Run, SweepView};
use crate::bloom::{
    ONE, Planes, add_upsampled, block_mean, blur, blur_plane, blur_scratch, box_radius, downsample,
    scale, upsampled_at,
};
use crate::choreography::SheetPose;
use crate::etch::{
    Blend, Layer, Paint, Pt, RgbaBytes, Shape, SpriteCanvas, Stops, Target, blit_sprite, clip_rect,
    fill, pt,
};
use crate::gouache::{
    BOUNCE_DOT, CROSSING_DOT, ITEM_SLACK, MARGIN, STROKE_REACH, Stroked, Wash, mix_dot, passes,
};
use crate::lettering;
use crate::masonry::{WallScratch, draw_grid, draw_tile, draw_walls};
use crate::patchwork::{Patchwork, area, hull, is_empty};
use crate::piece_art::{draw_body, draw_body_spun, draw_light_spun, has_tile, sprite_dir};
use crate::playbill::{Change, Playbill};
use crate::spectrum::{LightMode, Palette, Rgba, Shadow};
use crate::tableau::{
    CARD_PAD, HEADER_H, HEADER_TRACK, LEVEL_CAP, NEXT_PILL, SHEET_CAP, Tableau, WIN_CAP,
};
use crate::vitrine::{self, ChipState, Panel};
use crate::{DeviceRect, fit_buffer, make_room};

/// The level sheet's title, lettered at `SHEET_CAP` with this intensity (spec §7.5, dark).
const SHEET_TITLE: &str = "LEVELS";
const SHEET_INTENSITY: f32 = 0.8;
/// Ink lettering's strand widths in the light theme (pt; the comps' `stroke`, ruling L6).
const LEVEL_INK_PT: f32 = 1.0;
const WIN_INK_PT: f32 = 1.3;
const SHEET_INK_PT: f32 = 1.0;
/// The sheet's panel radius, its shadow's blur and its drop (pt, spec §7.1).
const SHEET_RADIUS: f32 = 14.0;
pub(crate) const SHEET_SHADOW_BLUR: f32 = 40.0;
pub(crate) const SHEET_SHADOW_DROP: f32 = 12.0;

const GLOW_SIGMA: f32 = 0.08;
const BLOOM_SIGMA: f32 = 0.5;
/// The spec's 0.7 and 0.35, raised to the trailer stills' weights in the Task 19 still review:
/// lit targets and crossings read fainter than in the comps and the trailer. The σs, and so the
/// tiles and their memory, are unchanged.
const GLOW_WEIGHT: f32 = 0.9;
const BLOOM_WEIGHT: f32 = 0.55;
/// A light tile bigger than this renders its light at 1800 / its size.
const MAX_LIGHT_PX: f32 = 1800.0;
/// The compose lookup covers light up to 4.0; beyond, `T` is 1 to within 1e-6.
const SHOULDER_TOP: usize = 4 * ONE as usize;
/// The largest piece `lay` makes room for: up to this scale over the larger of a cell and a
/// tray piece, drawing up to this reach of its side from its centre.
pub(crate) const UNDERLAY_SCALE: f32 = 1.3;
pub(crate) const UNDERLAY_REACH: f32 = 0.66;
pub const BOARD_SPRITES: usize = 48;
pub const TRAY_SPRITES: usize = 24;

/// What a paint shows: the board, its beams, the drag and the game (for game-specific art), and
/// the chrome's inputs: the current slot, whether it is won, every slot's state and the win text.
#[derive(Clone, Copy, Debug)]
pub struct Scene<'s> {
    pub board: &'s Board,
    pub field: &'s BeamField,
    pub drag: Option<Drag>,
    pub game: Game,
    pub slot: usize,
    pub won: bool,
    pub strip: [SlotState; STRIP_SLOTS],
    pub you_win: &'s str,
}

impl<'s> Scene<'s> {
    pub fn of(state: &'s GameState<'_>) -> Scene<'s> {
        Scene {
            board: state.board(),
            field: state.field(),
            drag: state.drag(),
            game: state.data().game,
            slot: state.slot(),
            won: state.is_won(),
            strip: std::array::from_fn(|s| state.slot_state(s)),
            you_win: state.data().ui.you_win.as_ref(),
        }
    }

    /// A board with no game around it (the pieces sheet): slot 0 current, the rest locked.
    pub fn bare(board: &'s Board, field: &'s BeamField, game: Game) -> Scene<'s> {
        let mut strip = [SlotState::Locked; STRIP_SLOTS];
        strip[0] = SlotState::Current;
        Scene {
            board,
            field,
            drag: None,
            game,
            slot: 0,
            won: false,
            strip,
            you_win: "",
        }
    }

    /// Next ▶ shows: the level is won, it is not the last, and the next one is open.
    pub fn next_open(&self) -> bool {
        self.won && self.slot + 1 < STRIP_SLOTS && self.strip[self.slot + 1] != SlotState::Locked
    }
}

/// What motion changes in `base` and the light this frame (set by the lightbox before painting):
/// the turning piece and its spin, the piece landing (its cell, shown empty in `base` while the
/// lightbox lays its moving body into `base` for the frame's composes (`lay`/`unlay`), and where
/// its light is in the light tile, moving with the land: its offset from the cell's centre in
/// device px and its size in cells), and the win header's reveal and fade. Settled: none, 1, 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub turn: Option<(usize, f32)>,
    pub landing: Option<(usize, Pt, f32)>,
    pub reveal: f32,
    pub header_alpha: f32,
}

impl Pose {
    pub const SETTLED: Pose = Pose {
        turn: None,
        landing: None,
        reveal: 1.0,
        header_alpha: 1.0,
    };
}

/// The shell's buffer, canvas-sized: the macOS canvas (`0x00RRGGBB`) or the web's RGBA bytes.
/// The wasm build has no `Xrgb`, so its code is not shipped.
pub enum Output<'b> {
    #[cfg(not(target_arch = "wasm32"))]
    Xrgb(&'b mut [u32]),
    Rgba(&'b mut [u8]),
}

/// Where the shoulder's tail stops easing: `T(3.5) = 1 − 2.7e-7`, still under 1 in f32 (which the
/// unheld formula reaches near 4), and 255 in the compose lookup like every value above ~2.
const SHOULDER_HOLD: f32 = 3.5;

/// The compose shoulder `T` (§8): identity to 0.8, then eases to 1.
/// Out of line: it only bakes the compose lookup, and inlined it vectorises into kilobytes of wasm.
#[inline(never)]
pub fn shoulder(x: f32) -> f32 {
    if x <= 0.8 {
        x
    } else {
        let x = x.min(SHOULDER_HOLD);
        0.8 + 0.2 * (1.0 - (-(x - 0.8) / 0.2).exp())
    }
}

/// `r` grown by `by` px on every side.
pub(crate) fn grow(r: DeviceRect, by: i32) -> DeviceRect {
    DeviceRect::new(
        r.left.saturating_sub(by),
        r.top.saturating_sub(by),
        r.right.saturating_add(by),
        r.bottom.saturating_add(by),
    )
}

/// The part of `r` inside `canvas`, or the empty default.
pub(crate) fn within(r: DeviceRect, canvas: DeviceRect) -> DeviceRect {
    clip_rect(r, canvas).unwrap_or_default()
}

/// `r` inside `[0, w) × [0, h)`, or the empty default.
pub(crate) fn clip_to(r: DeviceRect, w: i32, h: i32) -> DeviceRect {
    within(r, DeviceRect::new(0, 0, w, h))
}

/// The hull of `a` and `b`, an empty one standing for nothing.
pub(crate) fn union(a: DeviceRect, b: DeviceRect) -> DeviceRect {
    match (is_empty(a), is_empty(b)) {
        (true, _) => b,
        (_, true) => a,
        _ => hull(a, b),
    }
}

/// `a` less `b` as up to four disjoint rects: the bands above and below `b`, then the parts
/// left and right of it.
fn subtract(a: DeviceRect, b: DeviceRect) -> ([DeviceRect; 4], usize) {
    let mut parts = [DeviceRect::default(); 4];
    let Some(i) = clip_rect(a, b) else {
        parts[0] = a;
        return (parts, usize::from(!is_empty(a)));
    };
    let mut n = 0;
    for part in [
        DeviceRect::new(a.left, a.top, a.right, i.top),
        DeviceRect::new(a.left, i.bottom, a.right, a.bottom),
        DeviceRect::new(a.left, i.top, i.left, i.bottom),
        DeviceRect::new(i.right, i.top, a.right, i.bottom),
    ] {
        if !is_empty(part) {
            parts[n] = part;
            n += 1;
        }
    }
    (parts, n)
}

/// Where `paint_board`, `paint_tray` and `paint_picker` draw into `base`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Regions {
    /// The board grown by `ceil(0.5u)` (the grid's overhang), clipped to the board's room and
    /// the canvas.
    board: DeviceRect,
    /// `board` less the tray region: the disjoint rects a board paint draws in.
    board_parts: [DeviceRect; 4],
    board_part_count: usize,
    /// The tray's panel grown by 1 px, clipped to the canvas.
    tray: DeviceRect,
    /// The inline picker grown by the current chip's glow, clipped to the canvas; empty without
    /// one.
    picker: DeviceRect,
    /// The card grown by 1 px, clipped to the canvas.
    card: DeviceRect,
}

impl Regions {
    fn of(tableau: &Tableau) -> Regions {
        let canvas = tableau.canvas();
        let ppp = tableau.px_per_pt();
        // The grid's overhang, but never past half the margin (the board must not paint into the
        // column on very large canvases).
        let board = clip_rect(
            grow(tableau.board, (tableau.cell + 1) / 2),
            tableau.board_room(),
        )
        .map_or_else(DeviceRect::default, |r| within(r, canvas));
        let tray = within(grow(tableau.device(tableau.tray.rect), 1), canvas);
        // The current chip's glow reaches 3 pt past its pitch cell.
        let picker = tableau.picker.map_or_else(DeviceRect::default, |p| {
            within(
                grow(tableau.device(p.rect), (3.0 * ppp).ceil() as i32 + 1),
                canvas,
            )
        });
        let (board_parts, board_part_count) = subtract(board, tray);
        Regions {
            board,
            board_parts,
            board_part_count,
            tray,
            picker,
            card: within(grow(tableau.device(tableau.card), 1), canvas),
        }
    }

    fn board_parts(&self) -> &[DeviceRect] {
        &self.board_parts[..self.board_part_count]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpriteArt {
    Tile {
        raised: bool,
    },
    Body {
        piece: Piece,
        colour: Colour,
        dir: Dir,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SpriteKey {
    art: SpriteArt,
    size: u16,
    game: Game,
}

/// A fixed pool of `N` premultiplied sprites of one size class (spec §8 "Sprite cache"; the
/// palette is not in the key because the pool empties when the palette changes). A full pool
/// starts over.
struct SpritePool<const N: usize> {
    px: Vec<u32>,
    /// The sprites' side, px.
    size: usize,
    stride: usize,
    /// The palette the sprites were drawn in.
    palette: Option<Palette>,
    keys: [Option<SpriteKey>; N],
    len: usize,
}

impl<const N: usize> SpritePool<N> {
    fn new() -> Self {
        SpritePool {
            px: Vec::new(),
            size: 0,
            stride: 0,
            palette: None,
            keys: [None; N],
            len: 0,
        }
    }

    /// `N` slots of `size × size` in `palette`, empty, unless the pool already has that size and
    /// palette: a sprite depends on nothing else (its key holds the rest), so a resize that keeps
    /// the cell keeps them. Allocates only to grow or to give back a much larger pool.
    fn reshape(&mut self, size: usize, palette: &Palette) {
        if (self.size, self.palette) == (size, Some(*palette)) && self.px.len() == N * size * size {
            return;
        }
        (self.size, self.stride, self.palette) = (size, size * size, Some(*palette));
        fit_buffer(&mut self.px, N * self.stride, 0);
        self.clear();
    }

    fn clear(&mut self) {
        self.keys = [None; N];
        self.len = 0;
    }

    fn bytes(&self) -> usize {
        self.px.capacity() * size_of::<u32>()
    }

    /// The key of `art` in this pool's size class.
    fn key(&self, art: SpriteArt, game: Game) -> SpriteKey {
        SpriteKey {
            art,
            size: u16::try_from(self.size).unwrap_or(u16::MAX),
            game,
        }
    }

    /// The slot holding `key`'s sprite, drawn now on a miss (into the next slot; a full pool
    /// starts over first).
    fn slot(&mut self, key: SpriteKey, palette: &Palette) -> usize {
        if let Some(slot) = self.keys[..self.len].iter().position(|k| *k == Some(key)) {
            return slot;
        }
        if self.len == N {
            self.clear();
        }
        let slot = self.len;
        let px = &mut self.px[slot * self.stride..][..self.stride];
        px.fill(0);
        let size = self.size;
        let mut canvas = SpriteCanvas { px, size };
        let full = DeviceRect::new(0, 0, size as i32, size as i32);
        let (u, centre) = (size as f32, pt(size as f32 / 2.0, size as f32 / 2.0));
        match key.art {
            SpriteArt::Tile { raised } => draw_tile(&mut canvas, full, palette, centre, u, raised),
            SpriteArt::Body { piece, colour, dir } => {
                let cell = Cell { piece, colour, dir };
                draw_body(&mut canvas, full, palette, key.game, cell, centre, u);
            }
        }
        self.keys[slot] = Some(key);
        self.len += 1;
        slot
    }

    fn pixels(&self, slot: usize) -> &[u32] {
        &self.px[slot * self.stride..][..self.stride]
    }
}

/// Two pools sized to what they hold (spec §14 memory): board cells (`u²`) and tray pieces
/// (`tray_piece_px²`). A level shows a few dozen distinct sprites, so a refill is rare (the
/// specimen sheet and `a_full_sprite_pool_starts_over…` exercise it).
struct SpriteCache {
    board: SpritePool<BOARD_SPRITES>,
    tray: SpritePool<TRAY_SPRITES>,
}

/// The chrome's sprites (ruling 13): one well and the four chip states, premultiplied squares
/// baked on the first paint that needs them after `reshape` (the well with the tray, the chips
/// with a picker, so a canvas without an inline picker doesn't bake chips). The chip is the
/// inline picker's (pitch − 6) or, without one, the sheet's; each chip sprite has the glow margin
/// on every side.
#[derive(Default)]
struct ChromeSprites {
    px: Vec<u32>,
    /// The well sprite's side (one tray slot), px.
    well: usize,
    /// The chip's side, px.
    chip: usize,
    /// The glow margin around each chip sprite, px.
    margin: usize,
    /// The scale and palette the sizes were set for.
    ppp: f32,
    palette: Option<Palette>,
    well_baked: bool,
    chips_baked: bool,
}

impl ChromeSprites {
    /// Sizes the pool for `tableau` and `palette`. The sprites depend on nothing else, so they
    /// are drawn again only if one of those changed (a resize that keeps the sizes keeps them).
    fn reshape(&mut self, tableau: &Tableau, palette: &Palette) {
        let ppp = tableau.px_per_pt();
        let was = (self.well, self.chip, self.margin, self.ppp, self.palette);
        self.well = ((tableau.tray.slot * ppp).round() as usize).max(1);
        let chip = tableau.picker.map_or(tableau.sheet.chip, |p| p.pitch - 6.0);
        self.chip = ((chip * ppp).round() as usize).max(1);
        self.margin = vitrine::chip_margin(ppp) as usize;
        (self.ppp, self.palette) = (ppp, Some(*palette));
        let side = self.chip_side();
        let len = self.well * self.well + 4 * side * side;
        let now = (self.well, self.chip, self.margin, self.ppp, self.palette);
        if was == now && self.px.len() == len {
            return;
        }
        fit_buffer(&mut self.px, len, 0);
        self.forget();
    }

    /// Every sprite is drawn again when next needed (after a reshape or a theme change).
    fn forget(&mut self) {
        (self.well_baked, self.chips_baked) = (false, false);
    }

    fn chip_side(&self) -> usize {
        self.chip + 2 * self.margin
    }

    /// Draws the well sprite, once per `reshape` (or `clear_sprites`).
    fn bake_well(&mut self, palette: &Palette) {
        if self.well_baked {
            return;
        }
        let well = self.well;
        let px = &mut self.px[..well * well];
        px.fill(0);
        let mut canvas = SpriteCanvas { px, size: well };
        let half = well as f32 / 2.0;
        let full = DeviceRect::new(0, 0, well as i32, well as i32);
        vitrine::well(&mut canvas, full, palette, pt(half, half), well as f32);
        self.well_baked = true;
    }

    /// Draws the four chip sprites, once per `reshape` (or `clear_sprites`).
    fn bake_chips(&mut self, palette: &Palette, ppp: f32) {
        if self.chips_baked {
            return;
        }
        let (well, side) = (self.well, self.chip_side());
        let chips = &mut self.px[well * well..];
        chips.fill(0);
        for (state, px) in ChipState::ALL
            .into_iter()
            .zip(chips.chunks_exact_mut(side * side))
        {
            let mut canvas = SpriteCanvas { px, size: side };
            let c = side as f32 / 2.0;
            let full = DeviceRect::new(0, 0, side as i32, side as i32);
            let size = self.chip as f32;
            vitrine::chip_body(&mut canvas, full, palette, pt(c, c), size, state, ppp);
        }
        self.chips_baked = true;
    }

    fn well(&self) -> &[u32] {
        &self.px[..self.well * self.well]
    }

    fn chip(&self, state: ChipState) -> &[u32] {
        let side = self.chip_side();
        let k = ChipState::ALL.iter().position(|&s| s == state).unwrap_or(0);
        &self.px[self.well * self.well + k * side * side..][..side * side]
    }

    fn bytes(&self) -> usize {
        self.px.capacity() * size_of::<u32>()
    }
}

/// A bilinear tap pair: padded indices of the two source pixels (0 and `len + 1` are zero pads
/// outside the plane) and the second one's weight in 1/256.
pub(crate) type Tap = (u32, u32, u16);

/// The taps of `n` device px: device px `i`'s centre sits at `(i + 0.5)·s − 0.5` in `sharp`, and
/// (`halve`) at half that plus ¼ less in `half`, whose length is `len`.
#[inline(never)]
pub(crate) fn bake_taps(taps: &mut Vec<Tap>, n: usize, s: f32, halve: bool, len: usize) {
    let pad = |k: f32| (k + 1.0).clamp(0.0, (len + 1) as f32) as u32;
    make_room(taps, n);
    taps.extend((0..n).map(|i| {
        let mut p = (i as f32 + 0.5) * s - 0.5;
        if halve {
            p = (p + 0.5) / 2.0 - 0.5;
        }
        let f = p.floor();
        (pad(f), pad(f + 1.0), ((p - f) * 256.0).round() as u16)
    }));
}

/// Row `p` (padded) of `plane`, columns `lo..=hi` (padded, inside the plane), or `None` for a pad.
pub(crate) fn plane_row<T>(
    plane: &[T],
    width: usize,
    height: usize,
    p: u32,
    lo: usize,
    hi: usize,
) -> Option<&[T]> {
    let p = p as usize;
    (1..=height)
        .contains(&p)
        .then(|| &plane[(p - 1) * width + lo - 1..(p - 1) * width + hi])
}

/// Blends `planes`' rows `tap` vertically into `row` (three channels of `width + 2` padded
/// columns, weights summing to 256), over padded columns `lo..=hi` only.
fn blend_rows(planes: &Planes, (ra, rb, w): Tap, lo: u32, hi: u32, row: &mut [u32]) {
    let (width, height) = (planes.width, planes.height);
    let stride = width + 2;
    let (lo, hi) = ((lo as usize).max(1), (hi as usize).min(width));
    if lo > hi {
        return;
    }
    let (wa, wb) = (256 - u32::from(w), u32::from(w));
    for (c, plane) in [&planes.r, &planes.g, &planes.b].into_iter().enumerate() {
        let dst = &mut row[c * stride + lo..=c * stride + hi];
        let line = |p| plane_row(plane, width, height, p, lo, hi);
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

/// One horizontal tap of a vertically blended row, back in plane units (1/65 536 overall).
#[inline(always)]
pub(crate) fn tap(row: &[u32], (a, b, w): Tap) -> u32 {
    (row[a as usize] * (256 - u32::from(w)) + row[b as usize] * u32::from(w) + 32_768) >> 16
}

/// `base` plus the shouldered light `l`, per channel, saturating.
#[inline(always)]
fn expose(base: u32, l: [u32; 3], lut: &[u8; SHOULDER_TOP + 1]) -> u32 {
    let ch = |shift: u32, l: u32| {
        ((base >> shift & 0xff) + u32::from(lut[l.min(SHOULDER_TOP as u32) as usize])).min(255)
    };
    ch(16, l[0]) << 16 | ch(8, l[1]) << 8 | ch(0, l[2])
}

/// Pixels composed together: their light, then their exposure.
const RUN: usize = 64;

/// The most light the shoulder leaves as it is (`T(x) = x` up to 0.8): there the lookup is
/// `round(255·l / ONE)` = `(255·l + ONE/2) / ONE`, exactly, as `255·l` < 2²⁴ is exact in f32.
const IDENTITY_TOP: u32 = 3276;

/// `expose` for a run of pixels; arithmetic in place of the lookup when every light of the run is
/// in the shoulder's identity part (most of the tile), which vectorises.
fn expose_run<P: Texel>(
    base: &[u32],
    [r, g, b]: &[[u32; RUN]; 3],
    out: &mut [P],
    lut: &[u8; SHOULDER_TOP + 1],
) {
    let n = base.len().min(out.len()).min(RUN);
    let (r, g, b) = (&r[..n], &g[..n], &b[..n]);
    let brightest = |c: &[u32]| c.iter().fold(0, |m, &l| m.max(l));
    let lit = r.iter().zip(g).zip(b);
    if brightest(r).max(brightest(g)).max(brightest(b)) <= IDENTITY_TOP {
        for ((o, &p), ((&r, &g), &b)) in out.iter_mut().zip(base).zip(lit) {
            let ch =
                |shift: u32, l: u32| ((p >> shift & 0xff) + ((255 * l + ONE / 2) >> 12)).min(255);
            o.put(ch(16, r) << 16 | ch(8, g) << 8 | ch(0, b));
        }
    } else {
        for ((o, &p), ((&r, &g), &b)) in out.iter_mut().zip(base).zip(lit) {
            o.put(expose(p, [r, g, b], lut));
        }
    }
}

/// An output pixel: `0x00RRGGBB` or RGBA bytes.
pub(crate) trait Texel: Sized {
    fn put(&mut self, rgb: u32);
    /// The pixel as `0x00RRGGBB`.
    fn get(&self) -> u32;
    /// A canvas of these as a `fill` target.
    fn target(px: &mut [Self], width: usize, height: usize) -> impl Target + '_;
}

impl Texel for u32 {
    #[inline(always)]
    fn put(&mut self, rgb: u32) {
        *self = rgb;
    }

    #[inline(always)]
    fn get(&self) -> u32 {
        *self
    }

    fn target(px: &mut [u32], width: usize, height: usize) -> impl Target + '_ {
        Layer { px, width, height }
    }
}

impl Texel for [u8; 4] {
    #[inline(always)]
    fn put(&mut self, rgb: u32) {
        *self = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 0xff];
    }

    #[inline(always)]
    fn get(&self) -> u32 {
        u32::from(self[0]) << 16 | u32::from(self[1]) << 8 | u32::from(self[2])
    }

    fn target(px: &mut [[u8; 4]], width: usize, height: usize) -> impl Target + '_ {
        RgbaBytes {
            px: px.as_flattened_mut(),
            width,
            height,
        }
    }
}

/// Draws through to `inner` at `alpha` × each colour's alpha: the sheet's panel as it fades
/// (ruling M10(f): per element, not as a group). Its rise is in the shapes' own coordinates (they
/// are drawn lower), so nothing is cut at the canvas's edges.
struct Posed<T> {
    inner: T,
    alpha: f32,
}

impl<T: Target> Target for Posed<T> {
    fn size(&self) -> (usize, usize) {
        self.inner.size()
    }

    fn blend(&mut self, x: usize, y: usize, c: Rgba, blend: Blend) {
        self.inner.blend(
            x,
            y,
            Rgba {
                a: c.a * self.alpha,
                ..c
            },
            blend,
        );
    }
}

/// The output's pixels as a target drawn at `alpha`.
fn posed<P: Texel>(px: &mut [P], (w, h): (usize, usize), alpha: f32) -> impl Target + '_ {
    Posed {
        inner: P::target(px, w, h),
        alpha,
    }
}

/// `alpha` (0 → 1) in 255ths, as the integer passes scale by it.
fn alpha_255(alpha: f32) -> u32 {
    (alpha.clamp(0.0, 1.0) * 255.0).round() as u32
}

/// Integer source-over of `c` at `alpha` × its alpha on `rect` of the output (the scrim; the
/// sheet's interior), as `vitrine::tint` does on `base`. `rect` must lie inside the canvas.
fn tint_texels<P: Texel>(px: &mut [P], width: usize, rect: DeviceRect, c: Rgba, alpha: f32) {
    let a = ((c.a * alpha).clamp(0.0, 1.0) * 255.0).round() as u32;
    if a == 0 {
        return;
    }
    let keep = 255 - a;
    let lift = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32 * a + 127;
    let (lr, lg, lb) = (lift(c.r), lift(c.g), lift(c.b));
    let (x0, x1) = (rect.left as usize, rect.right as usize);
    for y in rect.top as usize..rect.bottom as usize {
        for p in &mut px[y * width + x0..y * width + x1] {
            let v = p.get();
            let ch = |shift: u32, lift: u32| ((v >> shift & 0xff) * keep + lift) / 255;
            p.put(ch(16, lr) << 16 | ch(8, lg) << 8 | ch(0, lb));
        }
    }
}

/// A premultiplied `size`² sprite over the output with its top-left at (`left`, `top`), only
/// inside `clip` (inside the canvas), as `blit_sprite` does over `base`; below `alpha` 1 each
/// texel (its channels and alpha) is first scaled by `alpha` in 255ths.
fn blit_texels<P: Texel>(
    px: &mut [P],
    width: usize,
    clip: DeviceRect,
    (sprite, size): (&[u32], usize),
    (left, top): (i32, i32),
    alpha: f32,
) {
    let s = size as i32;
    let Some(r) = clip_rect(DeviceRect::new(left, top, left + s, top + s), clip) else {
        return;
    };
    let k = alpha_255(alpha);
    let fade = |p: u32| {
        let ch = |shift: u32| ((p >> shift & 0xff) * k + 127) / 255;
        ch(24) << 24 | ch(16) << 16 | ch(8) << 8 | ch(0)
    };
    for y in r.top..r.bottom {
        let src =
            &sprite[((y - top) * s + (r.left - left)) as usize..][..(r.right - r.left) as usize];
        let row = y as usize * width;
        for (dst, &p) in px[row + r.left as usize..row + r.right as usize]
            .iter_mut()
            .zip(src)
        {
            let p = if k < 255 { fade(p) } else { p };
            let a = p >> 24;
            if a == 0 {
                continue;
            }
            let (keep, d) = (255 - a, dst.get());
            let ch = |shift: u32| {
                ((p >> shift & 0xff) + ((d >> shift & 0xff) * keep + 127) / 255).min(255)
            };
            dst.put(ch(16) << 16 | ch(8) << 8 | ch(0));
        }
    }
}

/// A light target that remembers the bounds of every pixel drawn, so the next paint clears
/// only those (spec §8).
struct Exposure<'a> {
    planes: &'a mut Planes,
    bounds: [usize; 4],
}

impl Target for Exposure<'_> {
    fn size(&self) -> (usize, usize) {
        (self.planes.width, self.planes.height)
    }

    fn blend(&mut self, x: usize, y: usize, c: Rgba, blend: Blend) {
        self.planes.blend(x, y, c, blend);
        let b = &mut self.bounds;
        (b[0], b[1], b[2], b[3]) = (b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1));
    }
}

/// Where the board's light items sit: the board's origin and cell `u` (device px), and the
/// tile's origin and scale (light px = (device px − origin) × s).
#[derive(Clone, Copy, Debug)]
pub(crate) struct LightMap {
    board: Pt,
    u: f32,
    origin: Pt,
    s: f32,
}

impl LightMap {
    /// The map of a tile at `rect` (device px) rendered at `scale`.
    pub(crate) fn of(tableau: &Tableau, rect: DeviceRect, scale: f32) -> LightMap {
        LightMap {
            board: pt(tableau.board.left as f32, tableau.board.top as f32),
            u: tableau.cell as f32,
            origin: pt(rect.left as f32, rect.top as f32),
            s: scale,
        }
    }

    /// A run end (half-cell units) in device px.
    fn at(&self, (x, y): (i16, i16)) -> Pt {
        self.board + pt(f32::from(x), f32::from(y)) * (self.u / 2.0)
    }

    fn map(&self, p: Pt) -> Pt {
        (p - self.origin) * self.s
    }

    /// The light px around device points `a` and `b`, grown by `reach`·u and 2 px.
    fn around(&self, a: Pt, b: Pt, reach: f32) -> DeviceRect {
        let (a, b) = (self.map(a), self.map(b));
        let r = reach * self.u * self.s + 2.0;
        DeviceRect::new(
            (a.x.min(b.x) - r).floor() as i32,
            (a.y.min(b.y) - r).floor() as i32,
            (a.x.max(b.x) + r).ceil() as i32,
            (a.y.max(b.y) + r).ceil() as i32,
        )
    }

    /// Where `change` draws: a run's or a partial's line (`line`·u either side: 0.032 for the
    /// dark 0.064u line, `STROKE_REACH` for the light bloom stroke), a glint (0.3u), a piece
    /// light (within 1u of its centre, at its size: the prism's fan reaches 0.8u, a corner exit
    /// plus its ticks; a landing piece's light is off its cell's centre and scaled).
    fn change_box(&self, tableau: &Tableau, change: &Change, line: f32) -> DeviceRect {
        match *change {
            Change::Run(run) => self.around(self.at(run.from), self.at(run.to), line),
            Change::Partial(p) => {
                let (a, b) = partial_ends(tableau, &p);
                self.around(a, b, line)
            }
            Change::Glint(g) => {
                let c = tableau.cell_centre(g.cell);
                self.around(c, c, 0.3)
            }
            Change::Light(i, cl) => {
                let c = tableau.cell_centre(i) + cl.offset;
                self.around(c, c, cl.size)
            }
        }
    }
}

/// Half-segment (`cell`, `side`)'s centre and edge point, device px.
pub(crate) fn half_ends(tableau: &Tableau, cell: usize, side: usize) -> (Pt, Pt) {
    let c = tableau.cell_centre(cell);
    let d = pt(f32::from(DX[side]), f32::from(DY[side]));
    (c, c + d * (tableau.cell as f32 / 2.0))
}

/// Where partial `p` is drawn, device px.
fn partial_ends(tableau: &Tableau, p: &Partial) -> (Pt, Pt) {
    let (c, e) = half_ends(tableau, usize::from(p.cell), usize::from(p.side));
    (c + (e - c) * p.from, c + (e - c) * p.to)
}

/// A dark run's or partial's change box reach (u): its 0.064u line's half width.
const DARK_LINE_REACH: f32 = 0.032;

/// Run `run`'s drawn ends in light px: its ends inset at an aperture or a wall face.
pub(crate) fn run_ends(map: &LightMap, run: &Run) -> (Pt, Pt) {
    let (a, b) = (map.at(run.from), map.at(run.to));
    let along = (b - a) * (map.u / (b - a).len());
    let diagonal = run.from.0 != run.to.0 && run.from.1 != run.to.1;
    (
        map.map(a + along * run.from_end.inset(diagonal)),
        map.map(b - along * run.to_end.inset(diagonal)),
    )
}

/// A beam's stroke (spec §7.3), the one recipe every beam drawing shares: its line, 0.064u wide
/// in the colour's mix at α 0.95, then its core, 0.02u wide (at least one device px) in the
/// colour's core at α 0.85, both α scaled by `alpha_k`, from `a` to `b` (light px) for cells of
/// `u` device px drawn at scale `s`, only inside `clip`.
pub(crate) fn beam_stroke(
    t: &mut dyn Target,
    clip: DeviceRect,
    palette: &Palette,
    (a, b): (Pt, Pt),
    mask: Colour,
    (u, s): (f32, f32),
    alpha_k: f32,
) {
    let line = Paint::Solid(palette.mix(mask).alpha(0.95 * alpha_k));
    fill(
        t,
        clip,
        &[Shape::line(a, b, 0.064 * u * s)],
        &line,
        1.0,
        Blend::Add,
    );
    let core = Paint::Solid(palette.core(mask).alpha(0.85 * alpha_k));
    let width = (0.02 * u).max(1.0) * s;
    fill(t, clip, &[Shape::line(a, b, width)], &core, 1.0, Blend::Add);
}

/// Draws `bill`'s light into `light.sharp`, only inside `clip` (light px), in its order: each
/// run's line then core, each partial's, the glints, then each shown piece light. Items whose box
/// misses `clip` are skipped. Returns the bounds drawn.
fn draw_bill(
    light: &mut LightTile,
    map: &LightMap,
    tableau: &Tableau,
    palette: &Palette,
    game: Game,
    bill: &Playbill,
    clip: DeviceRect,
) -> Option<DeviceRect> {
    let (s, u) = (map.s, map.u);
    let mut t = Exposure {
        planes: &mut light.sharp,
        bounds: [usize::MAX, usize::MAX, 0, 0],
    };
    let skip = |c: &Change| clip_rect(map.change_box(tableau, c, DARK_LINE_REACH), clip).is_none();
    for run in bill.line.runs() {
        if skip(&Change::Run(*run)) {
            continue;
        }
        let ends = run_ends(map, run);
        beam_stroke(&mut t, clip, palette, ends, run.mask, (u, s), 1.0);
    }
    for p in bill.line.partials() {
        if skip(&Change::Partial(*p)) {
            continue;
        }
        let (a, b) = partial_ends(tableau, p);
        let ends = (map.map(a), map.map(b));
        beam_stroke(&mut t, clip, palette, ends, p.mask, (u, s), 1.0);
    }
    for glint in bill.line.glints() {
        if skip(&Change::Glint(*glint)) {
            continue;
        }
        let radius = match glint.kind {
            GlintKind::Crossing => 0.3,
            GlintKind::Bounce => 0.18,
        } * u
            * s;
        let centre = map.map(tableau.cell_centre(glint.cell));
        let mix = palette.mix(glint.mask);
        let paint = Paint::Radial {
            centre,
            radius,
            stops: Stops::new(&[
                (0.0, palette.white.alpha(0.7)),
                (0.3, mix.alpha(0.45)),
                (1.0, mix.alpha(0.0)),
            ]),
        };
        fill(
            &mut t,
            clip,
            &[Shape::disc(centre, radius)],
            &paint,
            1.0,
            Blend::Add,
        );
    }
    for (i, cl) in bill.lights.iter().enumerate() {
        if !cl.shown || skip(&Change::Light(i, *cl)) {
            continue;
        }
        let centre = map.map(tableau.cell_centre(i) + cl.offset);
        draw_light_spun(
            &mut t,
            clip,
            palette,
            game,
            cl.cell,
            cl.received,
            cl.emitting,
            centre,
            u * s * cl.size,
            cl.spin,
        );
    }
    let [x0, y0, x1, y1] = t.bounds;
    (x0 < x1).then(|| DeviceRect::new(x0 as i32, y0 as i32, x1 as i32, y1 as i32))
}

/// Paints `bill` into the wash (ruling L3), only inside `clip` (sharp px), which the caller
/// wiped. Each layer, at `f` sharp px per its px (bloom 4, glow 2, sharp 1), draws every item
/// whose box (grown by `ITEM_SLACK`) meets the sharp-px extent of `coarser(clip, f)`: each mask's
/// strokes as one union, masks 1 to 7; then, in the sharp layer, the mix dots and the piece
/// lights. Returns the bounds of every px each layer painted (bloom, glow, sharp), in sharp px
/// (each layer's px scaled up).
fn draw_wash(
    wash: &mut Wash,
    map: &LightMap,
    tableau: &Tableau,
    palette: &Palette,
    game: Game,
    bill: &Playbill,
    clip: DeviceRect,
) -> [DeviceRect; 3] {
    let Wash {
        sharp,
        glow,
        bloom_raw,
        shapes,
        ..
    } = wash;
    let (u, s) = (map.u, map.s);
    let mut drawn = [DeviceRect::default(); 3];
    for (layer, (f, plane)) in [(4, bloom_raw), (2, glow), (1, sharp)]
        .into_iter()
        .enumerate()
    {
        let lclip = clip_to(coarser(clip, f), plane.width as i32, plane.height as i32);
        if is_empty(lclip) {
            continue;
        }
        let extent = DeviceRect::new(
            lclip.left * f,
            lclip.top * f,
            lclip.right * f,
            lclip.bottom * f,
        );
        let reach = grow(extent, ITEM_SLACK);
        let meets =
            |c: &Change| clip_rect(map.change_box(tableau, c, STROKE_REACH), reach).is_some();
        let k = 1.0 / f as f32;
        let mut plane = Stroked::new(plane);
        let plane = &mut plane;
        for mask in 1..=WHITE {
            for (width, colour) in passes(palette, mask, f).into_iter().flatten() {
                shapes.clear();
                for run in bill.line.runs().iter().filter(|r| r.mask == mask) {
                    if meets(&Change::Run(*run)) {
                        let (a, b) = run_ends(map, run);
                        shapes.push(Shape::line(a * k, b * k, width * u * s * k));
                    }
                }
                for p in bill.line.partials().iter().filter(|p| p.mask == mask) {
                    if meets(&Change::Partial(*p)) {
                        let (a, b) = partial_ends(tableau, p);
                        let w = width * u * s * k;
                        shapes.push(Shape::line(map.map(a) * k, map.map(b) * k, w));
                    }
                }
                if !shapes.is_empty() {
                    let paint = Paint::Solid(colour);
                    fill(plane, lclip, shapes, &paint, 1.0, Blend::Over);
                }
            }
        }
        if f == 1 {
            for glint in bill.line.glints() {
                if !meets(&Change::Glint(*glint)) {
                    continue;
                }
                let radius = match glint.kind {
                    GlintKind::Crossing => CROSSING_DOT,
                    GlintKind::Bounce => BOUNCE_DOT,
                } * u
                    * s;
                let centre = map.map(tableau.cell_centre(glint.cell));
                mix_dot(plane, lclip, palette, centre, radius, u * s, glint.mask);
            }
            for (i, cl) in bill.lights.iter().enumerate() {
                if !cl.shown || !meets(&Change::Light(i, *cl)) {
                    continue;
                }
                let centre = map.map(tableau.cell_centre(i) + cl.offset);
                draw_light_spun(
                    plane,
                    lclip,
                    palette,
                    game,
                    cl.cell,
                    cl.received,
                    cl.emitting,
                    centre,
                    u * s * cl.size,
                    cl.spin,
                );
            }
        }
        drawn[layer] = plane.drawn(f);
    }
    drawn
}

#[derive(Default)]
struct LightTile {
    /// Full-resolution extent: the board grown by 2σ_b (= 1u), clipped to the canvas.
    rect: DeviceRect,
    /// Light px per device px: min(1, 1800 / max(width, height)).
    scale: f32,
    sharp: Planes,
    /// ½: glow, then the halo (0.9·glow + 0.55·bloom upsampled).
    half: Planes,
    /// ⅛ (4×4 means of `half`): bloom.
    eighth: Planes,
    /// ⅛ before the blur (4×4 means of `sharp`'s 2×2 means): kept, so a patched develop
    /// recomputes only the touched blocks (ruling M3).
    coarse: Planes,
    scratch: Vec<u16>,
    /// Three planes of up to `band_cap` u16: the glow band `develop_patches` blurs.
    band: Vec<u16>,
    band_cap: usize,
    /// The box radii (glow at ½, bloom at ⅛) and σs of the last `develop`.
    radii: (usize, usize),
    sigmas: (f32, f32),
    /// Per device column / row of `rect`: the two `half` taps and the second tap's weight in
    /// 1/256 (and the same for `sharp` when `scale < 1`), baked in `reshape`.
    halo_cols: Vec<Tap>,
    halo_rows: Vec<Tap>,
    sharp_cols: Vec<Tap>,
    sharp_rows: Vec<Tap>,
    /// One vertically blended `half` row (3 channels), reused for every compose row.
    row: Vec<u32>,
    /// The same for `sharp`, when `scale < 1`.
    sharp_row: Vec<u32>,
    /// Whether `halo_cols` are the plain ½ pattern (always at scale 1): device column `2k`
    /// taps padded `half` columns `k`, `k + 1` at 192/256, and `2k + 1` taps `k + 1`, `k + 2`
    /// at 64/256.
    halo_halves: bool,
    /// What the last paint drew into `sharp` (its px), so the next one clears only that.
    drawn: DeviceRect,
    /// What the tile's light was lettered from (`Darkroom::letter`), while it still holds it.
    lettered: Option<Lettering>,
}

/// The longest text a lettering key holds; longer text is lettered on every paint.
pub const LETTERING_TEXT: usize = 24;

/// The widest card header lettering (pt) its tile holds: every game's "YOU WIN!" is 132 pt at
/// cap 18 pt, "LEVEL nn" ≈ 102 pt at cap 13 pt, track 0.3 (`every_games_win_text_is_letterable`
/// checks them all).
pub const HEADER_LETTERING_W: f32 = 140.0;

/// What a lettering tile's light is drawn from, besides the tile's size: its text, the
/// lettering's origin in the tile and its sizes (px), the scale and the palette. Equal keys on
/// a tile of the same size draw the same light, so `Darkroom::letter` keeps it (pixel-exact).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Lettering {
    text: [u8; LETTERING_TEXT],
    len: usize,
    origin: Pt,
    sizes: [f32; 3],
    ppp: f32,
    palette: Palette,
}

impl Lettering {
    /// The key, or `None` (never kept) when `text` is too long to hold.
    fn new(text: &str, origin: Pt, sizes: [f32; 3], ppp: f32, palette: &Palette) -> Option<Self> {
        let bytes = text.as_bytes();
        let mut held = [0u8; LETTERING_TEXT];
        held.get_mut(..bytes.len())?.copy_from_slice(bytes);
        Some(Lettering {
            text: held,
            len: bytes.len(),
            origin,
            sizes,
            ppp,
            palette: *palette,
        })
    }
}

/// The `halo_cols` tap of device column `c` at scale 1.
pub(crate) fn half_tap(c: usize) -> Tap {
    let a = c.div_ceil(2) as u32;
    (a, a + 1, if c.is_multiple_of(2) { 192 } else { 64 })
}

/// One channel's light for device columns `c0..` of a scale-1 tile: its `sharp` value plus the
/// halo from the blended ½ row `row`, in the `half_tap` pattern (so contiguous, and vectorised).
fn halved_light(row: &[u32], c0: usize, sharp: &[u16], light: &mut [u32]) {
    let halo = |a: u32, b: u32, w: u32| (a * (256 - w) + b * w + 32_768) >> 16;
    let skip = (c0 % 2).min(light.len()).min(sharp.len());
    let ((first, light), (lead, sharp)) = (light.split_at_mut(skip), sharp.split_at(skip));
    if let ([l], [s]) = (first, lead) {
        let (a, b, w) = half_tap(c0);
        *l = u32::from(*s) + halo(row[a as usize], row[b as usize], u32::from(w));
    }
    let k = (c0 + skip) / 2;
    let (pairs, odd) = light.as_chunks_mut::<2>();
    let (sharps, _) = sharp.as_chunks::<2>();
    let n = pairs.len().min(sharps.len());
    let (r0, r1, r2) = (&row[k..][..n], &row[k + 1..][..n], &row[k + 2..][..n]);
    for ((((pair, s), &a), &b), &c) in pairs.iter_mut().zip(sharps).zip(r0).zip(r1).zip(r2) {
        pair[0] = u32::from(s[0]) + halo(a, b, 192);
        pair[1] = u32::from(s[1]) + halo(b, c, 64);
    }
    if let ([l], Some(&s)) = (odd, sharp.get(2 * n)) {
        *l = u32::from(s) + halo(row[k + n], row[k + n + 1], 192);
    }
}

/// Rows of `half` a patched develop blurs at a time (each with its margins).
pub(crate) const BAND: usize = 64;

/// Most rects a light tile's changes and windows are kept in.
pub const LIGHT_PATCHES: usize = 16;

/// A frame's develop windows covering more than 1 / this of `half` develop whole instead.
pub(crate) const WHOLE_PAST: i64 = 8;

/// The px of a plane `factor` times smaller that hold any of `r`'s.
pub(crate) fn coarser(r: DeviceRect, factor: i32) -> DeviceRect {
    let up = |v: i32| (v + factor - 1).div_euclid(factor);
    DeviceRect::new(
        r.left.div_euclid(factor),
        r.top.div_euclid(factor),
        up(r.right),
        up(r.bottom),
    )
}

/// The device px whose compose reads ½ px window `w` of a tile at `tile`, `scale`: ½ px p feeds
/// device px within one ½ px of p·2/s, so 2·left − 2 … 2·right + 2 over s, one more px each
/// side; in the tile.
pub(crate) fn device_of_half(tile: DeviceRect, scale: f32, w: DeviceRect) -> DeviceRect {
    let s = scale;
    let lo = |v: i32| ((2 * v - 2) as f32 / s).floor() as i32 - 1;
    let hi = |v: i32| ((2 * v + 2) as f32 / s).ceil() as i32 + 1;
    let (x, y) = (tile.left, tile.top);
    let r = DeviceRect::new(
        x + lo(w.left),
        y + lo(w.top),
        x + hi(w.right),
        y + hi(w.bottom),
    );
    within(r, tile)
}

/// The device px whose compose reads sharp px `d` of a tile at `tile`, `scale`: sharp px p feeds
/// device px within one sharp px of p over s (exactly p at scale 1), one more px each side; in
/// the tile.
pub(crate) fn device_of_sharp(tile: DeviceRect, scale: f32, d: DeviceRect) -> DeviceRect {
    let s = scale;
    let lo = |v: i32| ((v - 1) as f32 / s).floor() as i32 - 1;
    let hi = |v: i32| ((v + 1) as f32 / s).ceil() as i32 + 1;
    let (x, y) = (tile.left, tile.top);
    let r = DeviceRect::new(
        x + lo(d.left),
        y + lo(d.top),
        x + hi(d.right),
        y + hi(d.bottom),
    );
    within(r, tile)
}

/// `coarse` at (`x`, `y`): the 4×4 mean of `sharp`'s 2×2 means, each rounded as `downsample` rounds.
fn eighth_mean(sharp: &[u16], sw: usize, sh: usize, x: usize, y: usize) -> u16 {
    let (hw, hh) = (sw.div_ceil(2), sh.div_ceil(2));
    let mut sum = 0u32;
    for hy in 4 * y..(4 * y + 4).min(hh) {
        for hx in 4 * x..(4 * x + 4).min(hw) {
            sum += u32::from(block_mean(sharp, sw, sh, hx, hy, 2));
        }
    }
    ((sum + 8) / 16) as u16
}

impl LightTile {
    /// Fits the tile to `rect`; a side over `max_px` renders its light at `max_px` / that side.
    /// A tile of the same size and scale only moves: its light (and lettering key) is kept.
    fn reshape(&mut self, rect: DeviceRect, max_px: f32) {
        self.rect = rect;
        let (w, h) = (
            (rect.right - rect.left).max(0) as usize,
            (rect.bottom - rect.top).max(0) as usize,
        );
        let s = if w == 0 || h == 0 {
            1.0
        } else {
            (max_px / w.max(h) as f32).min(1.0)
        };
        let side = |v: usize| ((v as f32 * s).ceil() as usize).max(1);
        let (sw, sh) = (side(w), side(h));
        let sized = (self.sharp.width, self.sharp.height) == (sw, sh)
            && (self.halo_cols.len(), self.halo_rows.len()) == (w, h);
        if sized && self.scale == s {
            return;
        }
        self.scale = s;
        self.lettered = None;
        self.sharp.reshape(sw, sh);
        let (hw, hh) = (sw.div_ceil(2), sh.div_ceil(2));
        self.half.reshape(hw, hh);
        self.eighth.reshape(hw.div_ceil(4), hh.div_ceil(4));
        self.coarse.reshape(hw.div_ceil(4), hh.div_ceil(4));
        make_room(&mut self.scratch, blur_scratch(sw, sh));
        bake_taps(&mut self.halo_cols, w, s, true, hw);
        self.halo_halves =
            s >= 1.0 && (self.halo_cols.iter().enumerate()).all(|(c, &tap)| tap == half_tap(c));
        bake_taps(&mut self.halo_rows, h, s, true, hh);
        fit_buffer(&mut self.row, 3 * (hw + 2), 0);
        if s < 1.0 {
            bake_taps(&mut self.sharp_cols, w, s, false, sw);
            bake_taps(&mut self.sharp_rows, h, s, false, sh);
            fit_buffer(&mut self.sharp_row, 3 * (sw + 2), 0);
        } else {
            make_room(&mut self.sharp_cols, 0);
            make_room(&mut self.sharp_rows, 0);
            make_room(&mut self.sharp_row, 0);
        }
        self.drawn = DeviceRect::default();
    }

    /// Zeroes what the last paint drew into `sharp`; the tile no longer holds a lettering.
    fn wipe(&mut self) {
        self.lettered = None;
        let d = self.drawn;
        if is_empty(d) {
            return;
        }
        let w = self.sharp.width;
        let (x0, x1) = (d.left as usize, d.right as usize);
        for plane in [&mut self.sharp.r, &mut self.sharp.g, &mut self.sharp.b] {
            for y in d.top as usize..d.bottom as usize {
                plane[y * w + x0..y * w + x1].fill(0);
            }
        }
        self.drawn = DeviceRect::default();
    }

    /// Zeroes `sharp` inside `r` (light px, clipped to the plane) in its three planes.
    fn wipe_rect(&mut self, r: DeviceRect) {
        let w = self.sharp.width;
        let r = clip_to(r, w as i32, self.sharp.height as i32);
        if is_empty(r) {
            return;
        }
        let (x0, x1) = (r.left as usize, r.right as usize);
        for plane in [&mut self.sharp.r, &mut self.sharp.g, &mut self.sharp.b] {
            for y in r.top as usize..r.bottom as usize {
                plane[y * w + x0..y * w + x1].fill(0);
            }
        }
    }

    /// Room for `develop_patches`' glow band at glow box radius `glow_radius` (½ px).
    fn reserve_band(&mut self, glow_radius: usize) {
        let rows = (BAND + 6 * glow_radius).min(self.half.height);
        self.band_cap = rows * self.half.width;
        fit_buffer(&mut self.band, 3 * self.band_cap, 0);
    }

    /// The glow at ½ and the bloom at ⅛ from `sharp` (σ in light px), weighted into `half` (§8).
    fn develop(&mut self, glow_sigma: f32, bloom_sigma: f32) {
        downsample(&self.sharp, &mut self.half, 2);
        downsample(&self.half, &mut self.coarse, 4);
        self.eighth.copy_from(&self.coarse);
        blur(&mut self.half, glow_sigma / 2.0, &mut self.scratch);
        blur(&mut self.eighth, bloom_sigma / 8.0, &mut self.scratch);
        scale(&mut self.half, GLOW_WEIGHT);
        add_upsampled(&mut self.half, &self.eighth, 4, BLOOM_WEIGHT);
        self.sigmas = (glow_sigma, bloom_sigma);
        self.radii = (box_radius(glow_sigma / 2.0), box_radius(bloom_sigma / 8.0));
    }

    /// `develop` after `sharp` changed only inside `dirty` (sharp px), bit for bit (ruling M3):
    /// the touched ⅛ blocks' means again, the whole (small) bloom blurred again, and the halo
    /// recomputed in windows reaching as far as the glow and the upsampled bloom carry a change.
    /// Adds to `device` every device rect whose compose that changes: each dirty rect's own (its
    /// sharp light) and each window's (its halo). Needs a `develop` since the last `reshape`.
    /// A tile with no band reserved (`reserve_band`: the lettering tiles) has no room for the
    /// windows, so it develops whole and reports its whole rect, which is the same light.
    fn develop_patches(&mut self, dirty: &[DeviceRect], device: &mut Patchwork<LIGHT_PATCHES>) {
        if self.band_cap == 0 {
            if dirty.iter().any(|&d| !is_empty(d)) {
                self.develop(self.sigmas.0, self.sigmas.1);
                device.add(self.rect);
            }
            return;
        }
        let (sw, sh) = (self.sharp.width, self.sharp.height);
        for &d in dirty {
            if is_empty(d) {
                continue;
            }
            let e = self.blocks_of(d);
            let cw = self.coarse.width;
            for y in e.top as usize..e.bottom as usize {
                for x in e.left as usize..e.right as usize {
                    self.coarse.r[y * cw + x] = eighth_mean(&self.sharp.r, sw, sh, x, y);
                    self.coarse.g[y * cw + x] = eighth_mean(&self.sharp.g, sw, sh, x, y);
                    self.coarse.b[y * cw + x] = eighth_mean(&self.sharp.b, sw, sh, x, y);
                }
            }
            device.add(self.device_of_sharp(d));
        }
        let windows = self.windows_of(dirty);
        if windows.is_empty() {
            return;
        }
        self.eighth.copy_from(&self.coarse);
        blur(&mut self.eighth, self.sigmas.1 / 8.0, &mut self.scratch);
        for &w in windows.as_slice() {
            self.halo_window(w);
            device.add(self.device_of_half(w));
        }
    }

    /// The ⅛ blocks (`coarse` px) that hold any of `d` (sharp px), in the plane.
    fn blocks_of(&self, d: DeviceRect) -> DeviceRect {
        let (ew, eh) = (self.coarse.width as i32, self.coarse.height as i32);
        clip_to(coarser(d, 8), ew, eh)
    }

    /// The windows of `half` (½ px) that a change inside `dirty` (sharp px) reaches, coalesced:
    /// each rect's glow, 3r past its ½ px, and its upsampled bloom, past its ⅛ blocks blurred.
    fn windows_of(&self, dirty: &[DeviceRect]) -> Patchwork<LIGHT_PATCHES> {
        let (hw, hh) = (self.half.width as i32, self.half.height as i32);
        let (rg, rb) = (3 * self.radii.0 as i32, 3 * self.radii.1 as i32);
        let mut windows = Patchwork::<LIGHT_PATCHES>::new();
        for &d in dirty.iter().filter(|&&d| !is_empty(d)) {
            let glow = grow(coarser(d, 2), rg);
            // Half px y reads ⅛ rows ⌊(2y − 3)/8⌋ and the next, so ⅛ rows `b` reach half rows
            // 4·top − 2 to 4·bottom + 2 (held here with 2 to spare); columns alike.
            let b = grow(self.blocks_of(d), rb);
            let bloom = DeviceRect::new(
                4 * b.left - 4,
                4 * b.top - 4,
                4 * b.right + 4,
                4 * b.bottom + 4,
            );
            windows.add(clip_to(hull(glow, bloom), hw, hh));
        }
        windows.coalesce();
        windows
    }

    /// The cheaper of two equal develops after `sharp` changed only inside `dirty` (sharp px):
    /// by patches, or whole (reporting the whole tile) once the windows cover more than
    /// 1 / `WHOLE_PAST` of `half`, where their per-px work (scalar, ~6× a whole develop's per px)
    /// costs more than developing it all. The light is the same either way.
    fn develop_changed(&mut self, dirty: &[DeviceRect], device: &mut Patchwork<LIGHT_PATCHES>) {
        let windows = self.windows_of(dirty);
        let covered: i64 = windows.as_slice().iter().map(|&w| area(w)).sum();
        let whole = (self.half.width * self.half.height) as i64;
        if covered * WHOLE_PAST > whole {
            self.develop(self.sigmas.0, self.sigmas.1);
            device.add(self.rect);
        } else {
            self.develop_patches(dirty, device);
        }
    }

    /// `half` inside window `w` (½ px), as `develop` computes it: the glow in bands of `BAND`
    /// rows, each blurred from `sharp`'s 2×2 means over the band grown by the glow's reach (so
    /// the band's own edges never reach the window), scaled, plus the upsampled bloom.
    fn halo_window(&mut self, w: DeviceRect) {
        let rg = 3 * self.radii.0 as i32;
        let (sw, sh) = (self.sharp.width, self.sharp.height);
        let (hw, hh) = (self.half.width, self.half.height);
        let cap = self.band_cap;
        let mut y0 = w.top;
        while y0 < w.bottom {
            let y1 = (y0 + BAND as i32).min(w.bottom);
            let src = clip_to(
                DeviceRect::new(w.left - rg, y0 - rg, w.right + rg, y1 + rg),
                hw as i32,
                hh as i32,
            );
            let (bw, bh) = (
                (src.right - src.left) as usize,
                (src.bottom - src.top) as usize,
            );
            // Plain asserts: no panic messages in the shipped code (the page's size cap).
            assert!(bw * bh <= cap);
            let sharps = [&self.sharp.r, &self.sharp.g, &self.sharp.b];
            for (c, sharp) in sharps.into_iter().enumerate() {
                let band = &mut self.band[c * cap..][..bw * bh];
                for (y, row) in band.chunks_exact_mut(bw).enumerate() {
                    let (x0, y) = (src.left as usize, src.top as usize + y);
                    for (x, v) in row.iter_mut().enumerate() {
                        *v = block_mean(sharp, sw, sh, x0 + x, y, 2);
                    }
                }
                blur_plane(band, bw, bh, self.sigmas.0 / 2.0, &mut self.scratch);
            }
            for y in y0..y1 {
                for x in w.left..w.right {
                    let i = (y - src.top) as usize * bw + (x - src.left) as usize;
                    let up = upsampled_at(&self.eighth, 4, BLOOM_WEIGHT, x as usize, y as usize);
                    let o = y as usize * hw + x as usize;
                    let halves = [&mut self.half.r, &mut self.half.g, &mut self.half.b];
                    for (c, half) in halves.into_iter().enumerate() {
                        let glow = (f32::from(self.band[c * cap + i]) * GLOW_WEIGHT)
                            .round()
                            .min(65_535.0);
                        half[o] = (glow as u32 + up[c]).min(65_535) as u16;
                    }
                }
            }
            y0 = y1;
        }
    }

    /// The device px whose compose reads `half` inside `w` (½ px): see `device_of_half`.
    fn device_of_half(&self, w: DeviceRect) -> DeviceRect {
        device_of_half(self.rect, self.scale, w)
    }

    /// The device px whose compose reads `sharp` inside `d` (sharp px): see `device_of_sharp`.
    fn device_of_sharp(&self, d: DeviceRect) -> DeviceRect {
        device_of_sharp(self.rect, self.scale, d)
    }

    /// Composes light row `ly`, light columns `c0..c1`, over `base` into `out`: in runs of
    /// `RUN` pixels, the light first, then the exposure.
    fn compose_span<P: Texel>(
        &mut self,
        ly: usize,
        (c0, c1): (usize, usize),
        base: &[u32],
        out: &mut [P],
        lut: &[u8; SHOULDER_TOP + 1],
    ) {
        let cols = &self.halo_cols[c0..c1];
        let (lo, hi) = (cols[0].0, cols[cols.len() - 1].1);
        blend_rows(&self.half, self.halo_rows[ly], lo, hi, &mut self.row);
        let hs = self.half.width + 2;
        let (hr, rest) = self.row.split_at(hs);
        let (hg, hb) = rest.split_at(hs);
        let mut light = [[0u32; RUN]; 3];
        let halos = [hr, hg, hb];
        let runs = cols
            .chunks(RUN)
            .zip(base.chunks(RUN))
            .zip(out.chunks_mut(RUN));
        if self.scale >= 1.0 {
            let at = ly * self.sharp.width;
            let planes = [&self.sharp.r, &self.sharp.g, &self.sharp.b];
            for (k, ((cols, base), out)) in runs.enumerate() {
                let (c, n) = (c0 + k * RUN, base.len());
                for ((plane, halo), lit) in planes.into_iter().zip(halos).zip(&mut light) {
                    let sharp = &plane[at + c..][..n];
                    if self.halo_halves {
                        halved_light(halo, c, sharp, &mut lit[..n]);
                    } else {
                        for ((l, &t), &s) in lit.iter_mut().zip(cols).zip(sharp) {
                            *l = u32::from(s) + tap(halo, t);
                        }
                    }
                }
                expose_run(base, &light, out, lut);
            }
        } else {
            let scols = &self.sharp_cols[c0..c1];
            let (lo, hi) = (scols[0].0, scols[scols.len() - 1].1);
            blend_rows(
                &self.sharp,
                self.sharp_rows[ly],
                lo,
                hi,
                &mut self.sharp_row,
            );
            let ss = self.sharp.width + 2;
            let (sr, rest) = self.sharp_row.split_at(ss);
            let (sg, sb) = rest.split_at(ss);
            for (((cols, base), out), scols) in runs.zip(scols.chunks(RUN)) {
                for ((sharp, halo), lit) in [sr, sg, sb].into_iter().zip(halos).zip(&mut light) {
                    for ((l, &t), &st) in lit.iter_mut().zip(cols).zip(scols) {
                        *l = tap(sharp, st) + tap(halo, t);
                    }
                }
                expose_run(base, &light, out, lut);
            }
        }
    }

    fn bytes(&self) -> usize {
        let taps = self.halo_cols.capacity()
            + self.halo_rows.capacity()
            + self.sharp_cols.capacity()
            + self.sharp_rows.capacity();
        self.sharp.bytes()
            + self.half.bytes()
            + self.eighth.bytes()
            + self.coarse.bytes()
            + (self.scratch.capacity() + self.band.capacity()) * size_of::<u16>()
            + (self.row.capacity() + self.sharp_row.capacity()) * size_of::<u32>()
            + taps * size_of::<Tap>()
    }
}

/// The static lettering tiles (spec §8, D1, ruling 8): their light is composed into `base`
/// whenever they change, and re-added to any rect of `base` that is repainted under them. They
/// always render at scale 1 (drawn in device px). The sheet's "LEVELS" is the exception: its
/// tile is composed onto the output over the sheet's panel, never into `base`.
#[derive(Default)]
struct Letters {
    brand: LightTile,
    header: LightTile,
    next: LightTile,
    /// "LEVELS" on the sheet's panel (empty with an inline picker: there is no sheet).
    sheet: LightTile,
    /// One `base` (or output) row, copied, so a tile can be composed into that row itself.
    row: Vec<u32>,
}

impl Letters {
    fn bytes(&self) -> usize {
        self.brand.bytes()
            + self.header.bytes()
            + self.next.bytes()
            + self.sheet.bytes()
            + self.row.capacity() * size_of::<u32>()
    }
}

/// The layers of one canvas: `base`, the board's light tile, the lettering tiles and the sprite
/// pools.
pub struct Darkroom {
    width: usize,
    height: usize,
    backdrop: Backdrop,
    base: Vec<u32>,
    light: LightTile,
    /// The light theme's light (`Over`); empty in the dark theme, as `light` is in the light
    /// one (ruling L2).
    wash: Wash,
    letters: Letters,
    sprites: SpriteCache,
    chrome: ChromeSprites,
    walls: WallScratch,
    /// The playbill being built for this frame, and the one the light tile shows (swapped once
    /// `light_frame` drew it).
    bill: Box<Playbill>,
    shown: Box<Playbill>,
    /// The device rects whose light the last `light_frame` changed.
    lit: Patchwork<LIGHT_PATCHES>,
    /// The pose this frame paints at.
    pose: Pose,
    /// `round(255·T(i / ONE))` for light `i` up to `SHOULDER_TOP`.
    lut: Vec<u8>,
    regions: Regions,
    /// What a piece laid into `base` for one frame covers (`lay`), and where.
    underlay: Vec<u32>,
    laid: Option<DeviceRect>,
    /// The frame a level cross-fade starts from (RGB), over `fade_rect`: the board region. Its
    /// room is made in `reshape`; it is empty until a level cue keeps a frame.
    fade: Vec<[u8; 3]>,
    fade_rect: DeviceRect,
    /// How this theme's light reaches the picture: the last `reshape`'s palette's.
    mode: LightMode,
    /// Where the light theme's brand is lettered in `base` (ruling L6), set by `paint_brand`.
    brand_ink: Option<BrandInk>,
}

/// The light brand's wordmark arguments and the px its ink can reach.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BrandInk {
    left: f32,
    top: f32,
    height: f32,
    col: f32,
    game: u8,
    ppp: f32,
    reach: DeviceRect,
}

impl Darkroom {
    pub fn new(tableau: &Tableau, palette: &Palette) -> Darkroom {
        let mut room = Darkroom {
            width: 0,
            height: 0,
            backdrop: Backdrop::new(),
            base: Vec::new(),
            light: LightTile::default(),
            wash: Wash::default(),
            letters: Letters::default(),
            sprites: SpriteCache {
                board: SpritePool::new(),
                tray: SpritePool::new(),
            },
            chrome: ChromeSprites::default(),
            walls: WallScratch::new(),
            bill: Playbill::new(),
            shown: Playbill::new(),
            lit: Patchwork::new(),
            pose: Pose::SETTLED,
            lut: (0..=SHOULDER_TOP)
                .map(|i| (255.0 * shoulder(i as f32 / ONE as f32)).round() as u8)
                .collect(),
            regions: Regions::default(),
            underlay: Vec::new(),
            laid: None,
            fade: Vec::new(),
            fade_rect: DeviceRect::default(),
            mode: LightMode::Additive,
            brand_ink: None,
        };
        room.reshape(tableau, palette);
        room
    }

    /// Fits every buffer to `tableau` (after a resize or theme change), reusing allocations
    /// that are big enough but not over twice the size (`make_room`), and empties the sprite
    /// pools whose sizes or palette changed. `paint_all` must follow.
    pub fn reshape(&mut self, tableau: &Tableau, palette: &Palette) {
        let canvas = tableau.canvas();
        let (w, h) = (canvas.right.max(0) as usize, canvas.bottom.max(0) as usize);
        (self.width, self.height) = (w, h);
        fit_buffer(&mut self.base, w * h, 0);
        self.laid = None;
        self.brand_ink = None;
        // A box around a piece up to `UNDERLAY_SCALE` × the larger of a cell and a tray piece,
        // `UNDERLAY_REACH` of its side each way, plus 2 px and the centre's own.
        let side = UNDERLAY_SCALE * (tableau.cell as f32).max(tableau.tray_piece_px() as f32);
        let side = 2 * ((UNDERLAY_REACH * side).ceil() as usize + 2) + 1;
        fit_buffer(&mut self.underlay, side * side, 0);
        let board = tableau.board;
        let focus = pt(
            (board.left + board.right) as f32 / 2.0,
            (board.top + board.bottom) as f32 / 2.0,
        );
        // Ruling L7: the light page's shadows and the board panel's fill are baked here, so
        // every region's repaint (which starts with the backdrop) keeps them.
        let ppp = tableau.px_per_pt();
        let unit = tableau.cell as f32;
        let edges = |r: DeviceRect| [r.left as f32, r.top as f32, r.right as f32, r.bottom as f32];
        let casting = |rect: DeviceRect, radius: f32, s: Shadow| Casting {
            rect: edges(rect),
            radius,
            drop: s.drop_pt * ppp,
            sigma: s.blur_pt / 2.0 * ppp,
            colour: s.colour,
        };
        // At most three castings: the board panel's, the card's and the tray's (review T4 M4).
        const { assert!(MAX_CASTINGS >= 3) };
        let mut castings = [Casting::default(); MAX_CASTINGS];
        let mut n = 0;
        let slab = tableau.board_panel();
        if let Some(bp) = palette.board_panel {
            castings[n] = casting(slab, 0.35 * unit, bp.shadow);
            n += 1;
        }
        if let Some(s) = palette.panel_shadow {
            for r in [
                tableau.device(tableau.card),
                tableau.device(tableau.tray.rect),
            ] {
                castings[n] = casting(r, 14.0 * ppp, s);
                n += 1;
            }
        }
        let panel = palette.board_panel.map(|bp| PanelFill {
            rect: edges(slab),
            radius: 0.35 * unit,
            centre: focus,
            reach: 0.75 * (board.right - board.left) as f32,
            stops: bp.fill,
        });
        let ground = Ground {
            focus,
            castings: &castings[..n],
            panel,
        };
        self.backdrop.reshape(palette, w, h, &ground);
        let u = tableau.cell;
        self.regions = Regions::of(tableau);
        self.mode = palette.light_mode;
        let over = self.mode == LightMode::Over;
        self.fade_rect = self.regions.board;
        let f = self.fade_rect;
        // Room only: `keep_for_fade` fills it before a fade reads it, so a resize skips the fill.
        make_room(
            &mut self.fade,
            ((f.right - f.left) * (f.bottom - f.top)) as usize,
        );
        self.walls.reshape(self.regions.board, u as f32);
        let margin = (2.0 * BLOOM_SIGMA * u as f32).ceil() as i32;
        // Ruling L2: one theme's light planes at a time, the outgoing theme's given back before
        // the incoming theme's are sized. In `Over` the dark lettering tiles go empty first, then
        // the dark board tile, then the wash is sized; in `Additive` the wash goes empty first,
        // then the dark board tile and the lettering tiles are sized.
        if over {
            self.reshape_letters(tableau, canvas);
        }
        let wash_rect = within(grow(board, (MARGIN * u as f32).ceil() as i32), canvas);
        if over {
            self.light.reshape(DeviceRect::default(), MAX_LIGHT_PX);
            self.wash.reshape(wash_rect, MAX_LIGHT_PX, u as f32);
        } else {
            self.wash
                .reshape(DeviceRect::default(), MAX_LIGHT_PX, u as f32);
            self.light
                .reshape(within(grow(board, margin), canvas), MAX_LIGHT_PX);
        }
        // Only the board's tile develops by patches; the lettering tiles reserve no band.
        let s = self.light.scale;
        self.light
            .reserve_band(box_radius(GLOW_SIGMA * u as f32 * s / 2.0));
        // The tile may have lost its light: the next frame compares with nothing (and `paint_all`
        // draws it whole).
        self.shown.clear();
        if !over {
            self.reshape_letters(tableau, canvas);
        }
        self.sprites.board.reshape(u.max(1) as usize, palette);
        self.sprites
            .tray
            .reshape(tableau.tray_piece_px().max(1) as usize, palette);
        self.chrome.reshape(tableau, palette);
    }

    /// The lettering tiles' rects, each its lettering grown by one cap (2σ_b, ruling 8).
    fn reshape_letters(&mut self, tableau: &Tableau, canvas: DeviceRect) {
        let ppp = tableau.px_per_pt();
        let card = self.regions.card;
        let cap = |pt: f32| (pt * ppp).ceil() as i32;
        let brand = tableau.brand.map_or_else(DeviceRect::default, |b| {
            let col = tableau.device(b);
            let width = 0.74 * (col.right - col.left) as f32;
            let reach = lettering::wordmark_cap(width).ceil() as i32;
            within(grow(col, reach), canvas)
        });
        // Tiles that change incrementally stay inside the card, so a card repaint owns all of
        // their light (a 13–18 pt header's bloom reaches ≤ 1 cap; the card's padding is 12 pt).
        // Their sizes depend on the scale alone (anchored to the card's rounded corners), so a
        // canvas a px wider keeps them and their lettering: the header from the top left, Next ▶
        // from the bottom right.
        let c = tableau.device(tableau.card);
        let header = DeviceRect::new(
            c.left - 1,
            c.top - 1,
            c.left + cap(CARD_PAD + 2.0 + HEADER_LETTERING_W + 18.0),
            c.top + cap(CARD_PAD + HEADER_H + 18.0),
        );
        let next = DeviceRect::new(
            c.right - cap(CARD_PAD + NEXT_PILL.0 + 11.0),
            c.bottom - cap(CARD_PAD + NEXT_PILL.1 + 11.0),
            c.right + 1,
            c.bottom + 1,
        );
        let inside_card = |r| clip_rect(r, card).unwrap_or_default();
        let (header, next) = (inside_card(header), inside_card(next));
        // "LEVELS" grown by one cap, inside the sheet's panel and anchored to its rounded corner
        // (the panel is centred, so a canvas a px wider moves it but keeps the tile's size).
        let sheet = if tableau.picker.is_none() {
            let (x, y) = tableau.sheet_lettering_at(SHEET_CAP);
            let reach = cap(SHEET_CAP);
            let wide = (lettering::width(SHEET_TITLE, HEADER_TRACK) * SHEET_CAP * ppp).ceil();
            let tile = DeviceRect::new(
                x - reach,
                y - reach,
                x + wide as i32 + reach,
                y + cap(SHEET_CAP) + reach,
            );
            let panel = tableau.device(tableau.sheet.panel);
            within(tile, within(panel, canvas))
        } else {
            DeviceRect::default()
        };
        // The light theme letters in ink, in `base` (ruling L6): no lettering tiles, so their
        // planes are given back (L2).
        let (brand, header, next, sheet) = if self.mode == LightMode::Over {
            Default::default()
        } else {
            (brand, header, next, sheet)
        };
        let letters = &mut self.letters;
        letters.brand.reshape(brand, f32::INFINITY);
        letters.header.reshape(header, f32::INFINITY);
        letters.next.reshape(next, f32::INFINITY);
        letters.sheet.reshape(sheet, f32::INFINITY);
        fit_buffer(&mut letters.row, self.width, 0);
    }

    /// Wipes `tile`, lets `draw` add light into it (device px, offset to the tile), and develops
    /// it with σ_g = 0.08·cap, σ_b = 0.5·cap (spec §8: for lettering, u is the cap height).
    /// `draw` receives the target, the plane's clip and the offset to add to device coordinates.
    /// When `key` (everything `draw` depends on) equals the one the tile was last lettered from,
    /// the tile already holds this light and is kept as it is.
    fn letter(
        tile: &mut LightTile,
        key: Option<Lettering>,
        cap: f32,
        draw: impl FnOnce(&mut dyn Target, DeviceRect, Pt),
    ) {
        if key.is_some() && tile.lettered == key {
            return;
        }
        tile.wipe();
        if is_empty(tile.rect) {
            return;
        }
        let origin = pt(-tile.rect.left as f32, -tile.rect.top as f32);
        let plane = DeviceRect::new(0, 0, tile.sharp.width as i32, tile.sharp.height as i32);
        let mut t = Exposure {
            planes: &mut tile.sharp,
            bounds: [usize::MAX, usize::MAX, 0, 0],
        };
        draw(&mut t, plane, origin);
        let [x0, y0, x1, y1] = t.bounds;
        if x0 < x1 {
            tile.drawn = DeviceRect::new(x0 as i32, y0 as i32, x1 as i32, y1 as i32);
        }
        tile.develop(GLOW_SIGMA * cap, BLOOM_SIGMA * cap);
        tile.lettered = key;
    }

    /// Re-adds every static lettering tile's light to `rect` of `base` (ruling 8). A tile that
    /// drew nothing is skipped, so its stale glow is never composed. In the light theme there
    /// are no tiles: the brand's ink is drawn into `rect` instead (ruling L6), so `rect` must be
    /// one whose `base` was just refilled (its multiply is never applied twice).
    fn relight(&mut self, palette: &Palette, rect: DeviceRect) {
        if self.mode == LightMode::Over {
            if let Some(b) = self.brand_ink
                && let Some(clip) = clip_rect(rect, b.reach)
            {
                let mut layer = Layer {
                    px: &mut self.base,
                    width: self.width,
                    height: self.height,
                };
                let (left, top, height, col) = (b.left, b.top, b.height, b.col);
                lettering::wordmark(
                    &mut layer, clip, palette, left, top, height, col, b.game, b.ppp,
                );
            }
            return;
        }
        let Ok(lut) = <&[u8; SHOULDER_TOP + 1]>::try_from(self.lut.as_slice()) else {
            return;
        };
        let w = self.width;
        let Letters {
            brand,
            header,
            next,
            row,
            ..
        } = &mut self.letters;
        for tile in [brand, header, next] {
            if is_empty(tile.drawn) {
                continue;
            }
            let Some(r) = clip_rect(rect, tile.rect) else {
                continue;
            };
            let (x0, x1) = (r.left as usize, r.right as usize);
            let c0 = x0 - tile.rect.left as usize;
            let row = &mut row[..x1 - x0];
            for y in r.top..r.bottom {
                let span = &mut self.base[y as usize * w..][x0..x1];
                row.copy_from_slice(span);
                let ly = (y - tile.rect.top) as usize;
                tile.compose_span(ly, (c0, c0 + (x1 - x0)), row, span, lut);
            }
        }
    }

    /// The brand row's light (spec §7.5): "CHROMATRON" and the game's number. Cold only: it
    /// changes only with the game, and a game change paints cold.
    fn paint_brand(&mut self, tableau: &Tableau, palette: &Palette, scene: &Scene) {
        let ppp = tableau.px_per_pt();
        let Some(b) = tableau.brand else {
            self.letters.brand.wipe();
            self.brand_ink = None;
            return;
        };
        let d = tableau.device(b);
        let col = (d.right - d.left) as f32;
        let cap = lettering::wordmark_cap(0.74 * col);
        let game = scene.game.number();
        if self.mode == LightMode::Over {
            // Ruling L6: the ink itself is drawn by `relight` (the canvas's, at the end of
            // `paint_all_at`, once; then each refilled region's).
            let canvas = tableau.canvas();
            self.brand_ink = Some(BrandInk {
                left: d.left as f32,
                top: d.top as f32,
                height: (d.bottom - d.top) as f32,
                col,
                game,
                ppp,
                reach: within(grow(d, (0.5 * cap).ceil() as i32), canvas),
            });
            return;
        }
        let tile = &mut self.letters.brand;
        let (left, top, height) = (d.left as f32, d.top as f32, (d.bottom - d.top) as f32);
        // The wordmark's origin in the tile: the same on a canvas that only moved the column.
        let origin = pt(left - tile.rect.left as f32, top - tile.rect.top as f32);
        let mut digits = [0u8; 2];
        let number = lettering::number(usize::from(game), &mut digits);
        let key = Lettering::new(number, origin, [height, col, cap], ppp, palette);
        Self::letter(tile, key, cap, |t, clip, o| {
            let (x, y) = (left + o.x, top + o.y);
            lettering::wordmark(t, clip, palette, x, y, height, col, game, ppp);
        });
    }

    /// Forgets every cached sprite (after a theme change).
    pub fn clear_sprites(&mut self) {
        self.sprites.board.clear();
        self.sprites.tray.clear();
        self.chrome.forget();
    }

    /// Every layer, settled: `paint_all_at` with no sweep.
    pub fn paint_all(&mut self, tableau: &Tableau, palette: &Palette, scene: &Scene) {
        self.paint_all_at(tableau, palette, scene, None);
    }

    /// Every layer: the backdrop wherever the board does not lay its own (the chrome paints
    /// over it), the board and its light (mid-sweep at `view`), the tray, the picker and the card
    /// (cold: no backdrop refill, no relight), the brand, and then every lettering tile's light
    /// added once (ruling 8).
    pub fn paint_all_at(
        &mut self,
        tableau: &Tableau,
        palette: &Palette,
        scene: &Scene,
        view: Option<&SweepView>,
    ) {
        let canvas = tableau.canvas();
        let mut layer = Layer {
            px: &mut self.base,
            width: self.width,
            height: self.height,
        };
        let (outside, count) = subtract(canvas, self.regions.board);
        // The board's paint skips the tray, so where the two regions meet the backdrop is laid
        // here too.
        let shared = clip_rect(self.regions.board, self.regions.tray).unwrap_or_default();
        for &part in outside[..count].iter().chain([&shared]) {
            if !is_empty(part) {
                self.backdrop.fill(&mut layer, part);
            }
        }
        self.paint_board_base(tableau, palette, scene, self.regions.board);
        self.light_frame(tableau, palette, scene, view, true);
        self.paint_tray_region(tableau, palette, scene, true);
        self.paint_picker_region(tableau, palette, scene, true);
        self.paint_card_region(tableau, palette, scene, true);
        self.paint_brand(tableau, palette, scene);
        self.relight(palette, canvas);
    }

    /// The board region of `base` (floor, grid, tiles, walls, bodies): `base` only, the light is
    /// `light_frame`'s. `base` is drawn only in the board region's parts off the tray, so the
    /// tray's pieces survive a board-only paint.
    pub fn paint_board(&mut self, tableau: &Tableau, palette: &Palette, scene: &Scene) {
        self.paint_board_base(tableau, palette, scene, self.regions.board);
        for k in 0..self.regions.board_part_count {
            self.relight(palette, self.regions.board_parts[k]);
        }
    }

    /// `paint_board` after the pieces of the `stale` board cells changed (or were lifted or set
    /// down), walls excepted: `base` is redrawn in those cells' rects only (`base` only: the light
    /// is `light_frame`'s). That equals a whole paint: a cell's tile and body sprites fill its
    /// rect exactly, the floor and grid don't depend on pieces, walls depend on walls alone, and
    /// every step draws a pixel the same whatever its clip.
    pub fn paint_cells(
        &mut self,
        tableau: &Tableau,
        palette: &Palette,
        scene: &Scene,
        stale: &[bool; BOARD_CELLS],
    ) {
        for (i, _) in stale.iter().enumerate().filter(|&(_, &s)| s) {
            self.paint_board_base(tableau, palette, scene, tableau.cell_rect(i));
        }
        for (i, _) in stale.iter().enumerate().filter(|&(_, &s)| s) {
            for k in 0..self.regions.board_part_count {
                if let Some(r) = clip_rect(tableau.cell_rect(i), self.regions.board_parts[k]) {
                    self.relight(palette, r);
                }
            }
        }
    }

    /// The board's `base` inside `clip`, in the board region's parts off the tray.
    fn paint_board_base(
        &mut self,
        tableau: &Tableau,
        palette: &Palette,
        scene: &Scene,
        clip: DeviceRect,
    ) {
        let hidden = scene.drag.map(|d| d.source);
        // A landing piece's cell shows empty, as the held piece's source does (without its
        // outline): the moving piece is laid over `base` for each frame's composes (`lay`).
        let (turn, landing) = (self.pose.turn, self.pose.landing.map(|l| l.0));
        let mut layer = Layer {
            px: &mut self.base,
            width: self.width,
            height: self.height,
        };
        let pool = &mut self.sprites.board;
        let size = pool.size;
        for &part in self.regions.board_parts() {
            let Some(part) = clip_rect(part, clip) else {
                continue;
            };
            self.backdrop.fill(&mut layer, part);
            // The light board panel's crisp 1-px rim (its fill is in the backdrop, ruling L7).
            if let Some(bp) = palette.board_panel {
                let rim = Panel {
                    rect: tableau.board_panel(),
                    radius: 0.35 * tableau.cell as f32,
                    fill: Rgba::CLEAR,
                    rim: [bp.rim, bp.rim],
                    rim_width: 1.0,
                };
                vitrine::panel_edges(&mut layer, part, &rim);
            }
            draw_grid(
                &mut layer,
                part,
                palette,
                tableau.board,
                tableau.cell as f32,
            );
            for i in 0..BOARD_CELLS {
                let r = tableau.cell_rect(i);
                let cell = if hidden == Some(i) || landing == Some(i) {
                    Cell::default()
                } else {
                    scene.board.cells[i]
                };
                if !has_tile(cell.piece) || clip_rect(r, part).is_none() {
                    continue;
                }
                let raised = cell.piece.is_movable();
                let slot = pool.slot(pool.key(SpriteArt::Tile { raised }, scene.game), palette);
                blit_sprite(&mut layer, part, pool.pixels(slot), size, r.left, r.top);
            }
            // The lifted piece's source: its floor and a dashed outline (inside its cell, so
            // `paint_cells` redraws it whole).
            if let Some(i) = hidden.filter(|&i| i < BOARD_CELLS) {
                let (centre, u) = (tableau.cell_centre(i), tableau.cell as f32);
                vitrine::dashed_outline(&mut layer, part, palette, centre, u);
            }
            draw_walls(
                &mut layer,
                part,
                palette,
                scene.board,
                pt(tableau.board.left as f32, tableau.board.top as f32),
                tableau.cell as f32,
                &mut self.walls,
            );
            for i in 0..BOARD_CELLS {
                let r = tableau.cell_rect(i);
                let cell = scene.board.cells[i];
                if matches!(cell.piece, Piece::Empty | Piece::Wall)
                    || hidden == Some(i)
                    || landing == Some(i)
                    || clip_rect(r, part).is_none()
                {
                    continue;
                }
                // Mid-turn, the piece is drawn as vectors at its spin (spec §8). At any angle a
                // body stays within its cell's square (the complementor's ticks reach 0.485u
                // from the centre, and the conduit clips its rails to the square itself), so
                // clipping it to its cell's rect loses nothing.
                if let Some((_, spin)) = turn.filter(|t| t.0 == i) {
                    let (c, u) = (tableau.cell_centre(i), tableau.cell as f32);
                    let clip = clip_rect(part, r).unwrap_or(r);
                    draw_body_spun(&mut layer, clip, palette, scene.game, cell, c, u, spin);
                    continue;
                }
                let slot = pool.slot(pool.key(body_art(cell), scene.game), palette);
                blit_sprite(&mut layer, part, pool.pixels(slot), size, r.left, r.top);
            }
        }
    }

    /// The pose this frame paints at (the lightbox sets it before any paint).
    pub fn set_pose(&mut self, pose: Pose) {
        self.pose = pose;
    }

    /// This frame's playbill (after `light_frame`).
    pub fn shown(&self) -> &Playbill {
        &self.shown
    }

    /// The board's light for this frame (ruling M2): the playbill of `scene` mid-sweep at `view`
    /// (settled with none) and at the pose, drawn whole when `whole` (a cold or whole-board
    /// paint), else wiped and redrawn only inside the items that differ from the last frame's and
    /// developed in windows. Returns the device rects whose light changed (the redrawn light and
    /// its glow and bloom).
    pub fn light_frame(
        &mut self,
        tableau: &Tableau,
        palette: &Palette,
        scene: &Scene,
        view: Option<&SweepView>,
        whole: bool,
    ) -> &[DeviceRect] {
        self.lit.clear();
        if self.mode == LightMode::Over {
            return self.wash_frame(tableau, palette, scene, view, whole);
        }
        if is_empty(self.light.rect) {
            return self.lit.as_slice();
        }
        self.bill.build(scene, view, &self.pose);
        let map = LightMap::of(tableau, self.light.rect, self.light.scale);
        let plane = DeviceRect::new(
            0,
            0,
            self.light.sharp.width as i32,
            self.light.sharp.height as i32,
        );
        if whole {
            self.light.wipe();
            let bill = &self.bill;
            if let Some(b) = draw_bill(
                &mut self.light,
                &map,
                tableau,
                palette,
                scene.game,
                bill,
                plane,
            ) {
                self.light.drawn = b;
            }
            let (u, s) = (tableau.cell as f32, self.light.scale);
            self.light.develop(GLOW_SIGMA * u * s, BLOOM_SIGMA * u * s);
            self.lit.add(self.light.rect);
        } else {
            let mut dirty = Patchwork::<LIGHT_PATCHES>::new();
            self.bill.changes(&self.shown, |c| {
                dirty.add(clip_to(
                    map.change_box(tableau, &c, DARK_LINE_REACH),
                    plane.right,
                    plane.bottom,
                ));
            });
            for &d in dirty.as_slice() {
                self.light.wipe_rect(d);
                let bill = &self.bill;
                if let Some(b) =
                    draw_bill(&mut self.light, &map, tableau, palette, scene.game, bill, d)
                {
                    self.light.drawn = union(self.light.drawn, b);
                }
            }
            self.light.develop_changed(dirty.as_slice(), &mut self.lit);
        }
        std::mem::swap(&mut self.bill, &mut self.shown);
        self.lit.as_slice()
    }

    /// The light theme's `light_frame` (rulings L3, L4): the playbill of `scene` mid-sweep at
    /// `view`, at the pose, painted into the wash: whole when `whole`, else wiped and painted
    /// again only inside the items that differ from the last frame's, and developed in windows.
    /// Returns the device rects whose light changed.
    fn wash_frame(
        &mut self,
        tableau: &Tableau,
        palette: &Palette,
        scene: &Scene,
        view: Option<&SweepView>,
        whole: bool,
    ) -> &[DeviceRect] {
        if is_empty(self.wash.rect) {
            return self.lit.as_slice();
        }
        self.bill.build(scene, view, &self.pose);
        let map = LightMap::of(tableau, self.wash.rect, self.wash.scale);
        let plane = DeviceRect::new(
            0,
            0,
            self.wash.sharp.width as i32,
            self.wash.sharp.height as i32,
        );
        let painted =
            |layers: [DeviceRect; 3]| layers.into_iter().fold(DeviceRect::default(), union);
        if whole {
            self.wash.wipe(self.wash.drawn);
            let layers = draw_wash(
                &mut self.wash,
                &map,
                tableau,
                palette,
                scene.game,
                &self.bill,
                plane,
            );
            self.wash.drawn = painted(layers);
            self.wash.develop();
            self.lit.add(self.wash.rect);
        } else {
            let mut dirty = Patchwork::<LIGHT_PATCHES>::new();
            self.bill.changes(&self.shown, |c| {
                dirty.add(clip_to(
                    map.change_box(tableau, &c, STROKE_REACH),
                    plane.right,
                    plane.bottom,
                ));
            });
            for &d in dirty.as_slice() {
                self.wash.wipe(d);
                let layers = draw_wash(
                    &mut self.wash,
                    &map,
                    tableau,
                    palette,
                    scene.game,
                    &self.bill,
                    d,
                );
                self.wash.drawn = union(self.wash.drawn, painted(layers));
            }
            self.wash.develop_changed(dirty.as_slice(), &mut self.lit);
        }
        std::mem::swap(&mut self.bill, &mut self.shown);
        self.lit.as_slice()
    }

    /// The tray region of `base` after its cells changed: see `paint_tray_region`.
    pub fn paint_tray(&mut self, tableau: &Tableau, palette: &Palette, scene: &Scene) {
        self.paint_tray_region(tableau, palette, scene, false);
        self.relight(palette, self.regions.tray);
    }

    /// The tray region of `base`: its glass panel, a well under each of the 24 visible slots,
    /// and each slot's piece on its tile. `cold`: `paint_all` already laid the backdrop and
    /// relights afterwards.
    fn paint_tray_region(
        &mut self,
        tableau: &Tableau,
        palette: &Palette,
        scene: &Scene,
        cold: bool,
    ) {
        let region = self.regions.tray;
        if is_empty(region) {
            return;
        }
        let ppp = tableau.px_per_pt();
        self.chrome.bake_well(palette);
        let (hidden, landing) = (scene.drag.map(|d| d.source), self.pose.landing.map(|l| l.0));
        let mut layer = Layer {
            px: &mut self.base,
            width: self.width,
            height: self.height,
        };
        if !cold {
            self.backdrop.fill(&mut layer, region);
        }
        let panel = Panel::glass(
            palette,
            tableau.device(tableau.tray.rect),
            14.0 * ppp,
            ppp.max(1.0),
        );
        vitrine::panel(&mut layer, region, &panel);
        let well = self.chrome.well as i32;
        for k in 0..VISIBLE_TOOLBOX_SLOTS {
            let c = tableau.cell_centre(BOARD_CELLS + k);
            let (left, top) = (c.x.round() as i32 - well / 2, c.y.round() as i32 - well / 2);
            blit_sprite(
                &mut layer,
                region,
                self.chrome.well(),
                well as usize,
                left,
                top,
            );
        }
        // The lifted piece's empty slot: its well and a dashed outline.
        if let Some(i) =
            hidden.filter(|i| (BOARD_CELLS..BOARD_CELLS + VISIBLE_TOOLBOX_SLOTS).contains(i))
        {
            let (centre, u) = (tableau.cell_centre(i), tableau.tray_piece_px() as f32);
            vitrine::dashed_outline(&mut layer, region, palette, centre, u);
        }
        let pool = &mut self.sprites.tray;
        let size = pool.size;
        for k in 0..VISIBLE_TOOLBOX_SLOTS {
            let i = BOARD_CELLS + k;
            let cell = scene.board.cells[i];
            // The held piece's slot, and a landing piece's (its well alone, no outline).
            if cell.piece == Piece::Empty || hidden == Some(i) || landing == Some(i) {
                continue;
            }
            let r = tableau.cell_rect(i);
            let s = size as i32;
            let (left, top) = (
                r.left + (r.right - r.left - s) / 2,
                r.top + (r.bottom - r.top - s) / 2,
            );
            for art in [SpriteArt::Tile { raised: true }, body_art(cell)] {
                let slot = pool.slot(pool.key(art, scene.game), palette);
                blit_sprite(&mut layer, region, pool.pixels(slot), size, left, top);
            }
        }
    }

    /// What a whole board paint changes: the board region of `base` (`paint_board`) ∪ the light
    /// tile (`light_frame`, whole).
    pub fn board_damage(&self) -> DeviceRect {
        union(self.regions.board, self.light_rect())
    }

    pub fn tray_damage(&self) -> DeviceRect {
        self.regions.tray
    }

    /// The picker region of `base` after the slots' states changed: see `paint_picker_region`.
    pub fn paint_picker(&mut self, tableau: &Tableau, palette: &Palette, scene: &Scene) {
        self.paint_picker_region(tableau, palette, scene, false);
        self.relight(palette, self.regions.picker);
    }

    /// The inline picker's region of `base`: the 50 chip sprites by state, and their digits.
    /// `cold`: `paint_all` already laid the backdrop and relights afterwards.
    fn paint_picker_region(
        &mut self,
        tableau: &Tableau,
        palette: &Palette,
        scene: &Scene,
        cold: bool,
    ) {
        let region = self.regions.picker;
        if is_empty(region) || tableau.picker.is_none() {
            return;
        }
        self.chrome.bake_chips(palette, tableau.px_per_pt());
        let mut layer = Layer {
            px: &mut self.base,
            width: self.width,
            height: self.height,
        };
        if !cold {
            self.backdrop.fill(&mut layer, region);
        }
        let side = self.chrome.chip_side() as i32;
        for (slot, &state) in scene.strip.iter().enumerate() {
            let r = tableau.chip_rect(slot);
            let (left, top) = (
                (r.left + r.right) / 2 - side / 2,
                (r.top + r.bottom) / 2 - side / 2,
            );
            let state = ChipState::from(state);
            let sprite = self.chrome.chip(state);
            blit_sprite(&mut layer, region, sprite, side as usize, left, top);
            // The sprite's own centre, so the digits sit on the chip whatever the side's parity.
            let half = side as f32 / 2.0;
            let centre = pt(left as f32 + half, top as f32 + half);
            let size = self.chrome.chip as f32;
            vitrine::chip_digits(&mut layer, region, palette, centre, size, slot + 1, state);
        }
    }

    /// The card region of `base` after its key (slot, won, Next ▶) changed: see
    /// `paint_card_region`.
    pub fn paint_card(&mut self, tableau: &Tableau, palette: &Palette, scene: &Scene) {
        self.paint_card_region(tableau, palette, scene, false);
        self.relight(palette, self.regions.card);
    }

    /// The card (spec §7.5): its glass panel, the Levels pill when the picker is a sheet, and
    /// Next ▶'s pill once won with the next level open; the header ("LEVEL n", or the lettered
    /// win text) and Next ▶'s "NEXT" and triangle are light, in their own tiles. Every lettering
    /// position is whole px from the card's rounded corners, so its key holds across a canvas a
    /// px wider. `cold`: `paint_all` already laid the backdrop and relights afterwards.
    fn paint_card_region(
        &mut self,
        tableau: &Tableau,
        palette: &Palette,
        scene: &Scene,
        cold: bool,
    ) {
        let region = self.regions.card;
        if is_empty(region) {
            return;
        }
        let ppp = tableau.px_per_pt();
        let px = |p: f32| p * ppp;
        let whole = |p: f32| px(p).round() as i32;
        // Next ▶'s label origin, for the card's right and bottom edges where it is drawn (its
        // dark tile's coordinates, or the canvas's in light).
        let next_at = |right: i32, bottom: i32| {
            pt(
                (right - whole(CARD_PAD + NEXT_PILL.0 - 16.0)) as f32,
                (bottom - whole(CARD_PAD + (NEXT_PILL.1 + 11.0) / 2.0)) as f32,
            )
        };
        let parts = tableau.card_parts;
        let c = tableau.device(tableau.card);
        // The header: "LEVEL n", or a letterable win text (ruling 7: else a typeset caption).
        let (cap_pt, intensity) = if scene.won {
            (WIN_CAP, 1.0)
        } else {
            (LEVEL_CAP, 0.6)
        };
        let mut buf = [0u8; 9];
        let text = match (scene.won, lettering::letterable(scene.you_win)) {
            (false, _) => lettering::level_label(scene.slot, &mut buf),
            (true, true) => scene.you_win,
            (true, false) => "",
        };
        // A win writes the header on (`pose.reveal`), or fades it in under reduced motion
        // (`pose.header_alpha`); settled, the key is phase 2's, so the lettered tile is kept.
        let (reveal, fade) = (self.pose.reveal, self.pose.header_alpha);
        let next_open = scene.next_open();
        // The light theme has no lettering tiles (Task 2): its header is ink, below.
        if self.mode != LightMode::Over {
            let tile = &mut self.letters.header;
            let (x, y) = tableau.header_lettering_at(cap_pt);
            let at = pt((x - tile.rect.left) as f32, (y - tile.rect.top) as f32);
            let sizes = [px(cap_pt), intensity * fade, 1.0 - reveal];
            let key = Lettering::new(text, at, sizes, ppp, palette);
            Self::letter(tile, key, px(cap_pt), |t, clip, _| {
                if !text.is_empty() {
                    let beam = lettering::Beam::new(px(cap_pt), ppp, intensity * fade);
                    let track = HEADER_TRACK;
                    lettering::beam_line_revealed(
                        t, clip, palette, text, at, track, &beam, None, reveal,
                    );
                }
            });
            // Next ▶: "NEXT" and the triangle as light (white α 0.97), so they glow.
            let tile = &mut self.letters.next;
            let at = next_at(c.right - tile.rect.left, c.bottom - tile.rect.top);
            let label = if next_open { "NEXT" } else { "" };
            let key = Lettering::new(label, at, [px(11.0), 0.0, 0.0], ppp, palette);
            Self::letter(tile, key, px(11.0), |t, clip, _| {
                if next_open {
                    next_label(t, clip, palette.next_ink, at, ppp, Blend::Add);
                }
            });
        }
        let mut layer = Layer {
            px: &mut self.base,
            width: self.width,
            height: self.height,
        };
        if !cold {
            self.backdrop.fill(&mut layer, region);
        }
        let rim = ppp.max(1.0);
        vitrine::panel(&mut layer, region, &Panel::glass(palette, c, px(14.0), rim));
        if let Some(pill) = parts.levels {
            let r = tableau.device(pill);
            let rect = [r.left as f32, r.top as f32, r.right as f32, r.bottom as f32];
            let fill = Paint::Solid(palette.pill_fill);
            vitrine::pill(&mut layer, region, rect, &fill, palette.pill_rim, rim);
            let icon = pt(
                tableau.easel_px_x(pill.x + 11.0),
                tableau.easel_px_y(pill.y + 9.0),
            );
            vitrine::levels_icon(&mut layer, region, palette, icon, ppp);
        }
        if next_open {
            let rect = tableau.next_pill();
            // Light: the pill's shadow first (spec §7.5: blur 10 pt, 3 pt down).
            if let Some(s) = palette.next_shadow {
                let drop = s.drop_pt * ppp;
                let r = (rect[3] - rect[1]) / 2.0;
                let cast = Shape::round_rect(rect[0], rect[1] + drop, rect[2], rect[3] + drop, r);
                vitrine::soft_shape(
                    &mut layer,
                    region,
                    &cast,
                    s.blur_pt / 2.0 * ppp,
                    s.colour,
                    None,
                );
            }
            let glass = Paint::Linear {
                from: pt(rect[0], rect[1]),
                to: pt(rect[2], rect[3]),
                stops: Stops::new(&[(0.0, palette.next_fill[0]), (1.0, palette.next_fill[1])]),
            };
            vitrine::pill(&mut layer, region, rect, &glass, palette.next_rim, rim);
            // Light: "NEXT" and ▶ as flat white strokes on the pill, in `base` (ruling L6).
            if self.mode == LightMode::Over {
                let at = next_at(c.right, c.bottom);
                next_label(&mut layer, region, palette.next_ink, at, ppp, Blend::Over);
            }
        }
        // Light: the header in ink, into `base`, written on at the win's `reveal` (ruling L6).
        if self.mode == LightMode::Over && !text.is_empty() {
            let (x, y) = tableau.header_lettering_at(cap_pt);
            let t_pt = if scene.won { WIN_INK_PT } else { LEVEL_INK_PT };
            let ink = lettering::Ink::new(px(cap_pt), ppp, px(t_pt), fade);
            let at = pt(x as f32, y as f32);
            lettering::ink_line(
                &mut layer,
                region,
                palette,
                text,
                at,
                HEADER_TRACK,
                &ink,
                None,
                reveal,
            );
        }
    }

    /// What `paint_card` changes: the card region of `base` (its lettering tiles lie inside it).
    pub fn card_damage(&self) -> DeviceRect {
        self.regions.card
    }

    /// What `paint_picker` changes: the picker region of `base` (empty without an inline picker).
    pub fn picker_damage(&self) -> DeviceRect {
        self.regions.picker
    }

    /// The level sheet over `clip` of the output, once `clip` is composed (ruling 9), at `pose`:
    /// the scrim (when the pose has it), then at the pose's alpha and `rise_pt` lower, the panel's
    /// soft shadow (ruling 12), the panel, "LEVELS" from its own tile, the close chip and the 50
    /// chips by state. Each pixel is drawn the same whatever the clip, so any damage rect is
    /// redrawn exactly. `out` must be canvas-sized. At `SheetPose::SHOWN` it is the open sheet.
    pub fn draw_sheet(
        &mut self,
        tableau: &Tableau,
        palette: &Palette,
        strip: &[SlotState; STRIP_SLOTS],
        out: &mut Output,
        clip: DeviceRect,
        pose: SheetPose,
    ) {
        let n = self.width * self.height;
        match out {
            #[cfg(not(target_arch = "wasm32"))]
            Output::Xrgb(px) => {
                assert!(px.len() == n);
                self.sheet_into(tableau, palette, strip, px, clip, pose);
            }
            Output::Rgba(bytes) => {
                assert!(bytes.len() == 4 * n);
                let (px, _) = bytes.as_chunks_mut::<4>();
                self.sheet_into(tableau, palette, strip, px, clip, pose);
            }
        }
    }

    /// `draw_sheet` into the output's pixels: integer passes for the scrim, the panel's interior
    /// and the chip sprites; shapes through a target only for the small parts. The panel's parts
    /// are laid out `dy` px lower (its rise), each drawn at the pose's alpha.
    fn sheet_into<P: Texel>(
        &mut self,
        tableau: &Tableau,
        palette: &Palette,
        strip: &[SlotState; STRIP_SLOTS],
        px: &mut [P],
        clip: DeviceRect,
        pose: SheetPose,
    ) {
        let (w, h) = (self.width, self.height);
        let Some(clip) = clip_rect(clip, DeviceRect::new(0, 0, w as i32, h as i32)) else {
            return;
        };
        if pose.scrim {
            tint_texels(px, w, clip, palette.scrim, 1.0);
        }
        // Fully faded, the panel draws nothing (every pass below would leave each pixel as is).
        if alpha_255(pose.alpha) == 0 {
            return;
        }
        let alpha = pose.alpha;
        let ppp = tableau.px_per_pt();
        let dy = (pose.rise_pt * ppp).round() as i32;
        let lower = |r: DeviceRect| DeviceRect::new(r.left, r.top + dy, r.right, r.bottom + dy);
        let sheet = &tableau.sheet;
        let p = lower(tableau.device(sheet.panel));
        let radius = SHEET_RADIUS * ppp;
        let panel = Panel::sheet(palette, p, radius, ppp.max(1.0));
        // The shadow (blur 40 pt, so σ 20 pt; 12 pt down), except under the panel's bands, which
        // its fill covers anyway: around the panel and in its rounded corners.
        let drop = SHEET_SHADOW_DROP * ppp;
        let (l, t, r, b) = (p.left as f32, p.top as f32, p.right as f32, p.bottom as f32);
        let shadow = Shape::round_rect(l, t + drop, r, b + drop, radius);
        let sigma = SHEET_SHADOW_BLUR / 2.0 * ppp;
        let (around, count) = subtract(clip, p);
        {
            let mut target = posed(px, (w, h), alpha);
            for &part in around[..count].iter().chain(&panel.corners()) {
                if let Some(part) = clip_rect(part, clip) {
                    let c = palette.sheet_shadow;
                    vitrine::soft_shape(&mut target, part, &shadow, sigma, c, None);
                }
            }
        }
        for band in panel.bands() {
            if let Some(band) = clip_rect(band, clip) {
                tint_texels(px, w, band, panel.fill, alpha);
            }
        }
        vitrine::panel_edges(&mut posed(px, (w, h), alpha), clip, &panel);
        if self.mode == LightMode::Over {
            // Light: "LEVELS" in ink on the output, at the pose's alpha and rise (ruling L6).
            let (x, y) = tableau.sheet_lettering_at(SHEET_CAP);
            let ink = lettering::Ink::new(SHEET_CAP * ppp, ppp, SHEET_INK_PT * ppp, 1.0);
            let at = pt(x as f32, (y + dy) as f32);
            let mut target = posed(px, (w, h), alpha);
            let track = HEADER_TRACK;
            lettering::ink_line(
                &mut target,
                clip,
                palette,
                SHEET_TITLE,
                at,
                track,
                &ink,
                None,
                1.0,
            );
        } else {
            self.letter_sheet(tableau, palette);
            self.relight_sheet(px, clip, alpha, dy);
        }
        let close = pt(
            tableau.easel_px_x(sheet.close.x + sheet.close.w / 2.0),
            tableau.easel_px_y(sheet.close.y + sheet.close.h / 2.0) + dy as f32,
        );
        vitrine::close_mark(&mut posed(px, (w, h), alpha), clip, palette, close, ppp);
        // The chips as the inline picker draws them: sprites by state, then their digits.
        self.chrome.bake_chips(palette, ppp);
        let side = self.chrome.chip_side();
        let (s, size) = (side as i32, self.chrome.chip as f32);
        for (slot, &state) in strip.iter().enumerate() {
            let r = lower(tableau.sheet_chip_rect(slot));
            let (left, top) = (
                (r.left + r.right) / 2 - s / 2,
                (r.top + r.bottom) / 2 - s / 2,
            );
            if clip_rect(DeviceRect::new(left, top, left + s, top + s), clip).is_none() {
                continue;
            }
            let state = ChipState::from(state);
            let sprite = (self.chrome.chip(state), side);
            blit_texels(px, w, clip, sprite, (left, top), alpha);
            let half = side as f32 / 2.0;
            let centre = pt(left as f32 + half, top as f32 + half);
            let mut target = posed(px, (w, h), alpha);
            vitrine::chip_digits(&mut target, clip, palette, centre, size, slot + 1, state);
        }
    }

    /// "LEVELS" into the sheet's tile, unless it already holds it (its key: whole px from the
    /// panel's rounded corner, so a canvas a px wider keeps it).
    fn letter_sheet(&mut self, tableau: &Tableau, palette: &Palette) {
        let ppp = tableau.px_per_pt();
        let cap = SHEET_CAP * ppp;
        let tile = &mut self.letters.sheet;
        let (x, y) = tableau.sheet_lettering_at(SHEET_CAP);
        let at = pt((x - tile.rect.left) as f32, (y - tile.rect.top) as f32);
        let key = Lettering::new(SHEET_TITLE, at, [cap, SHEET_INTENSITY, 0.0], ppp, palette);
        Self::letter(tile, key, cap, |t, clip, _| {
            let beam = lettering::Beam::new(cap, ppp, SHEET_INTENSITY);
            lettering::beam_line(t, clip, palette, SHEET_TITLE, at, HEADER_TRACK, &beam, None);
        });
    }

    /// Adds the sheet tile's light to `clip` of the output, over what is drawn there, with the
    /// tile `dy` px lower; below `alpha` 1 each span is then mixed back with what was there.
    fn relight_sheet<P: Texel>(&mut self, px: &mut [P], clip: DeviceRect, alpha: f32, dy: i32) {
        let Ok(lut) = <&[u8; SHOULDER_TOP + 1]>::try_from(self.lut.as_slice()) else {
            return;
        };
        let w = self.width;
        let Letters { sheet, row, .. } = &mut self.letters;
        if is_empty(sheet.drawn) {
            return;
        }
        let at = sheet.rect;
        let at = DeviceRect::new(at.left, at.top + dy, at.right, at.bottom + dy);
        let Some(r) = clip_rect(clip, at) else {
            return;
        };
        let (x0, x1) = (r.left as usize, r.right as usize);
        let c0 = x0 - at.left as usize;
        let row = &mut row[..x1 - x0];
        let k = alpha_255(alpha);
        for y in r.top..r.bottom {
            let span = &mut px[y as usize * w..][x0..x1];
            for (b, p) in row.iter_mut().zip(span.iter()) {
                *b = p.get();
            }
            let ly = (y - at.top) as usize;
            sheet.compose_span(ly, (c0, c0 + (x1 - x0)), row, span, lut);
            if k < 255 {
                for (p, &was) in span.iter_mut().zip(row.iter()) {
                    let lit = p.get();
                    let ch = |shift: u32| {
                        ((was >> shift & 0xff) * (255 - k) + (lit >> shift & 0xff) * k + 127) / 255
                    };
                    p.put(ch(16) << 16 | ch(8) << 8 | ch(0));
                }
            }
        }
    }

    /// Recomposes `rect` (clipped to the canvas) of `out` from `base` and the light tile.
    /// `out` must be canvas-sized.
    /// Integer and row by row; `&mut` for the row scratch.
    pub fn compose(&mut self, rect: DeviceRect, out: &mut Output) {
        let n = self.width * self.height;
        match out {
            #[cfg(not(target_arch = "wasm32"))]
            Output::Xrgb(px) => {
                assert!(px.len() == n);
                self.compose_into(rect, px);
            }
            Output::Rgba(bytes) => {
                assert!(bytes.len() == 4 * n);
                let (px, _) = bytes.as_chunks_mut::<4>();
                self.compose_into(rect, px);
            }
        }
    }

    fn compose_into<P: Texel>(&mut self, rect: DeviceRect, out: &mut [P]) {
        let canvas = DeviceRect::new(0, 0, self.width as i32, self.height as i32);
        let Some(r) = clip_rect(rect, canvas) else {
            return;
        };
        let Ok(lut) = <&[u8; SHOULDER_TOP + 1]>::try_from(self.lut.as_slice()) else {
            return;
        };
        let over = self.mode == LightMode::Over;
        let light = &mut self.light;
        let wash = &mut self.wash;
        let lit = if over { wash.rect } else { light.rect };
        let w = self.width;
        let (x0, x1) = (r.left as usize, r.right as usize);
        for y in r.top..r.bottom {
            let base = &self.base[y as usize * w..][..w];
            let row = &mut out[y as usize * w..][..w];
            let (lx0, lx1) = if (lit.top..lit.bottom).contains(&y) {
                (
                    r.left.max(lit.left) as usize,
                    r.right.min(lit.right) as usize,
                )
            } else {
                (x1, x1)
            };
            if lx0 >= lx1 {
                copy(&base[x0..x1], &mut row[x0..x1]);
                continue;
            }
            copy(&base[x0..lx0], &mut row[x0..lx0]);
            copy(&base[lx1..x1], &mut row[lx1..x1]);
            let left = lit.left as usize;
            let (ly, span) = ((y - lit.top) as usize, (lx0 - left, lx1 - left));
            if over {
                wash.compose_span(ly, span, &base[lx0..lx1], &mut row[lx0..lx1]);
            } else {
                light.compose_span(ly, span, &base[lx0..lx1], &mut row[lx0..lx1], lut);
            }
        }
    }

    /// The board region the level cross-fade covers (spec §9.1: "the board region").
    pub fn fade_rect(&self) -> DeviceRect {
        self.fade_rect
    }

    /// Keeps the output's board region (the last frame, still shown) for the cross-fade (M5).
    pub fn keep_for_fade(&mut self, out: &Output) {
        let (rect, width) = (self.fade_rect, self.width);
        match out {
            #[cfg(not(target_arch = "wasm32"))]
            Output::Xrgb(px) => keep_texels(&mut self.fade, &px[..], width, rect),
            Output::Rgba(bytes) => {
                let (px, _) = bytes.as_chunks::<4>();
                keep_texels(&mut self.fade, px, width, rect);
            }
        }
    }

    /// Lerps `rect` ∩ the board region of `out` (just composed) from the kept old frame, at `k` of
    /// the new picture: `(old · (256 − w) + new · w + 128) >> 8`, `w = round(256 k)`; k = 0 is the
    /// old frame and k = 1 the new picture, exactly.
    pub fn fade_into(&self, rect: DeviceRect, out: &mut Output, k: f32) {
        let Some(r) = clip_rect(rect, self.fade_rect) else {
            return;
        };
        // Nothing kept for this region (no level cue began since the last reshape): no fade.
        let f = self.fade_rect;
        if self.fade.len() != ((f.right - f.left) * (f.bottom - f.top)) as usize {
            return;
        }
        let w = (k.clamp(0.0, 1.0) * 256.0).round() as u32;
        let (fade, kept, width) = (&self.fade[..], self.fade_rect, self.width);
        match out {
            #[cfg(not(target_arch = "wasm32"))]
            Output::Xrgb(px) => fade_texels(fade, kept, px, width, r, w),
            Output::Rgba(bytes) => {
                let (px, _) = bytes.as_chunks_mut::<4>();
                fade_texels(fade, kept, px, width, r, w);
            }
        }
    }

    /// The active light tile's device rect, the dark tile's or the wash's (empty when the board
    /// is off the canvas).
    pub fn light_rect(&self) -> DeviceRect {
        if self.mode == LightMode::Over {
            self.wash.rect
        } else {
            self.light.rect
        }
    }

    /// Light px per device px in the active light tile.
    pub fn light_scale(&self) -> f32 {
        if self.mode == LightMode::Over {
            self.wash.scale
        } else {
            self.light.scale
        }
    }

    /// The light tile's glow and bloom developed again from the light it holds, after a paint:
    /// whole (`None`), or by patches as if it changed only inside `dirty` (device px). Returns
    /// the device rects whose compose that changes. Public only for the bench's develop row and
    /// the no-allocation test, which time and count a develop on its own (`light_frame` also
    /// draws, and develops only what a change touched); the shells never call it.
    #[doc(hidden)]
    pub fn develop_light(&mut self, dirty: Option<&[DeviceRect]>) -> Patchwork<LIGHT_PATCHES> {
        let mut device = Patchwork::new();
        let over = self.mode == LightMode::Over;
        if over && is_empty(self.wash.rect) {
            return device;
        }
        let Some(dirty) = dirty else {
            if over {
                self.wash.develop();
                device.add(self.wash.rect);
            } else {
                let (glow, bloom) = self.light.sigmas;
                self.light.develop(glow, bloom);
                device.add(self.light.rect);
            }
            return device;
        };
        let (o, s) = if over {
            (self.wash.rect, self.wash.scale)
        } else {
            (self.light.rect, self.light.scale)
        };
        let at = |v: i32, o: i32| (v - o) as f32 * s;
        let mut light = Patchwork::<LIGHT_PATCHES>::new();
        for &r in dirty {
            light.add(DeviceRect::new(
                at(r.left, o.left).floor() as i32,
                at(r.top, o.top).floor() as i32,
                at(r.right, o.left).ceil() as i32,
                at(r.bottom, o.top).ceil() as i32,
            ));
        }
        if over {
            self.wash.develop_patches(light.as_slice(), &mut device);
        } else {
            self.light.develop_patches(light.as_slice(), &mut device);
        }
        device
    }

    /// Heap held by every buffer, in bytes.
    pub fn buffer_bytes(&self) -> usize {
        self.base.capacity() * size_of::<u32>()
            + self.backdrop.bytes()
            + self.light.bytes()
            + self.wash.bytes()
            + self.letters.bytes()
            + self.sprites.board.bytes()
            + self.sprites.tray.bytes()
            + self.chrome.bytes()
            + self.walls.bytes()
            + 2 * (size_of::<Playbill>() + size_of::<Beamline>())
            + self.lut.capacity()
            + self.underlay.capacity() * size_of::<u32>()
            + self.fade.capacity() * size_of::<[u8; 3]>()
    }

    /// Draws `f` into `base` inside `rect` (clipped to the canvas) until `unlay`, keeping what
    /// it covers: a landing piece, composed under the light as a piece at rest is (spec §8), so
    /// its own light and the beams add over it all the way in. False, with nothing laid, when
    /// `rect` is past the underlay's room.
    pub fn lay(&mut self, rect: DeviceRect, f: impl FnOnce(&mut dyn Target, DeviceRect)) -> bool {
        self.unlay();
        let canvas = DeviceRect::new(0, 0, self.width as i32, self.height as i32);
        let Some(r) = clip_rect(rect, canvas) else {
            return false;
        };
        let (x0, x1, w) = (r.left as usize, r.right as usize, self.width);
        let span = x1 - x0;
        if span * (r.bottom - r.top) as usize > self.underlay.len() {
            return false;
        }
        for (k, y) in (r.top as usize..r.bottom as usize).enumerate() {
            self.underlay[k * span..][..span].copy_from_slice(&self.base[y * w..][x0..x1]);
        }
        self.laid = Some(r);
        let mut layer = Layer {
            px: &mut self.base,
            width: self.width,
            height: self.height,
        };
        f(&mut layer, r);
        true
    }

    /// Puts back what the laid piece covered in `base`.
    pub fn unlay(&mut self) {
        let Some(r) = self.laid.take() else {
            return;
        };
        let (x0, x1, w) = (r.left as usize, r.right as usize, self.width);
        let span = x1 - x0;
        for (k, y) in (r.top as usize..r.bottom as usize).enumerate() {
            self.base[y * w..][x0..x1].copy_from_slice(&self.underlay[k * span..][..span]);
        }
    }
}

/// Next ▶'s label (spec §7.5): "NEXT" as flat glyphs (cap 11 pt, stroke 1.3 pt), then a ▶
/// 9 × 11 pt, 10 pt after it, in `ink`; `at` is the cap line's left end.
fn next_label(t: &mut dyn Target, clip: DeviceRect, ink: Rgba, at: Pt, ppp: f32, blend: Blend) {
    let px = |p: f32| p * ppp;
    let (cap, track, stroke) = (px(11.0), 0.3, px(1.3));
    let left = lettering::Align::Left;
    let w = lettering::flat_line(t, clip, "NEXT", at, cap, track, stroke, ink, left, blend);
    let (tx, ty) = (at.x + w + px(10.0), at.y + px(5.5));
    let tri = Shape::polygon(
        &[
            pt(tx, ty - px(5.5)),
            pt(tx + px(9.0), ty),
            pt(tx, ty + px(5.5)),
        ],
        0.0,
    );
    fill(t, clip, &[tri], &Paint::Solid(ink), 1.0, blend);
}

/// A cell's body sprite: its kind, colour byte and direction folded by symmetry.
fn body_art(cell: Cell) -> SpriteArt {
    SpriteArt::Body {
        piece: cell.piece,
        colour: cell.colour,
        dir: sprite_dir(cell.piece, cell.dir),
    }
}

fn copy<P: Texel>(base: &[u32], out: &mut [P]) {
    for (o, &b) in out.iter_mut().zip(base) {
        o.put(b);
    }
}

/// Copies `rect` of `out` (`width` px across; `rect` inside it) into `fade` as RGB, row by row.
/// Within the room `reshape` made for it, so it never allocates.
fn keep_texels<P: Texel>(fade: &mut Vec<[u8; 3]>, out: &[P], width: usize, rect: DeviceRect) {
    let (x0, span) = (rect.left as usize, (rect.right - rect.left) as usize);
    fade.clear();
    for y in rect.top as usize..rect.bottom as usize {
        fade.extend(out[y * width + x0..][..span].iter().map(|p| {
            let v = p.get();
            [(v >> 16) as u8, (v >> 8) as u8, v as u8]
        }));
    }
}

/// Lerps `r` of `out` (inside `kept`, the rect `fade` holds) from `fade` to what `out` holds, at
/// `w`/256 of `out`.
fn fade_texels<P: Texel>(
    fade: &[[u8; 3]],
    kept: DeviceRect,
    out: &mut [P],
    width: usize,
    r: DeviceRect,
    w: u32,
) {
    let span = (kept.right - kept.left) as usize;
    let (x0, n) = (r.left as usize, (r.right - r.left) as usize);
    let mix = |old: u8, new: u32| (u32::from(old) * (256 - w) + new * w + 128) >> 8;
    for y in r.top..r.bottom {
        let at = (y - kept.top) as usize * span + (r.left - kept.left) as usize;
        let row = &mut out[y as usize * width + x0..][..n];
        for (p, &[or, og, ob]) in row.iter_mut().zip(&fade[at..][..n]) {
            let v = p.get();
            p.put(mix(or, v >> 16 & 0xff) << 16 | mix(og, v >> 8 & 0xff) << 8 | mix(ob, v & 0xff));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playbill::CellLight;
    use crate::spectrum::Theme;
    use crate::{Easel, vitrine};
    use lumen::{BOARD_CELLS, Cell, Piece, board_index, trace};

    fn lit_board() -> (Board, BeamField) {
        let mut board = Board::default();
        board.cells[board_index(0, 7)] = Cell {
            piece: Piece::Laser,
            colour: 4,
            dir: 2,
        };
        let mut field = BeamField::default();
        trace(&board, &mut field);
        (board, field)
    }

    fn painted(easel: Easel, board: &Board, field: &BeamField) -> (Darkroom, Tableau, Vec<u32>) {
        let tableau = Tableau::arrange(&easel);
        let mut room = Darkroom::new(&tableau, &Palette::DARK);
        let scene = Scene::bare(board, field, Game::C1);
        room.paint_all(&tableau, &Palette::DARK, &scene);
        let mut out = vec![0u32; (easel.width * easel.height) as usize];
        room.compose(tableau.canvas(), &mut Output::Xrgb(&mut out));
        (room, tableau, out)
    }

    fn red(p: u32) -> u32 {
        p >> 16 & 0xff
    }

    #[test]
    fn a_turn_ending_draws_its_piece_as_its_sprite() {
        // Mid-turn a body is drawn as vectors straight into `base`; as its spin runs out it must
        // look like the sprite that replaces it, or the turn pops at its end. On the desktop and
        // on a phone (a smaller cell, its centre at another px phase), in every direction.
        let easels = [
            Easel::new(2560, 1600, 2.0),
            Easel::new(780, 1480, 2.0).with_touch(true),
        ];
        for easel in easels {
            let tableau = Tableau::arrange(&easel);
            let cell = board_index(7, 7);
            let r = tableau.cell_rect(cell);
            let mut room = Darkroom::new(&tableau, &Palette::DARK);
            for piece in Piece::ALL
                .into_iter()
                .filter(|&p| crate::piece_art::turn_period(p) != 0)
            {
                for dir in 0..8 {
                    let worst = turn_end_difference(&mut room, &tableau, r, piece, dir);
                    assert!(
                        worst <= 2,
                        "{easel:?} {piece:?} dir {dir}: vectors and sprite differ by {worst}"
                    );
                }
            }
        }
    }

    /// The most a channel of `piece` at `dir` in cell (7, 7) differs, inside its cell rect `r`,
    /// between its sprite and its vectors at a vanishing spin.
    fn turn_end_difference(
        room: &mut Darkroom,
        tableau: &Tableau,
        r: DeviceRect,
        piece: Piece,
        dir: u8,
    ) -> u32 {
        let cell = board_index(7, 7);
        let mut board = Board::default();
        board.cells[cell] = Cell {
            piece,
            colour: 5,
            dir,
        };
        let field = BeamField::default();
        let scene = Scene::bare(&board, &field, Game::C4);
        room.set_pose(Pose::SETTLED);
        room.paint_all(tableau, &Palette::DARK, &scene);
        let sprite = room.base.clone();
        room.set_pose(Pose {
            turn: Some((cell, 1e-6)),
            ..Pose::SETTLED
        });
        room.paint_all(tableau, &Palette::DARK, &scene);
        room.set_pose(Pose::SETTLED);
        let mut worst = 0;
        for y in r.top..r.bottom {
            for x in r.left..r.right {
                let i = y as usize * room.width + x as usize;
                let (p, q) = (sprite[i], room.base[i]);
                for k in 0..3 {
                    worst = worst.max((p >> (8 * k) & 0xff).abs_diff(q >> (8 * k) & 0xff));
                }
            }
        }
        worst
    }

    #[test]
    fn the_chrome_sprites_are_kept_only_while_their_inputs_hold() {
        let (board, field) = lit_board();
        let scene = Scene::bare(&board, &field, Game::C1);
        let tableau = Tableau::arrange(&Easel::new(2560, 1600, 2.0));
        let mut room = Darkroom::new(&tableau, &Palette::DARK);
        room.paint_all(&tableau, &Palette::DARK, &scene);
        let baked = |room: &Darkroom| (room.chrome.well_baked, room.chrome.chips_baked);
        assert_eq!(baked(&room), (true, true));
        // One px wider: the column moves, the well and chip sizes do not.
        let wider = Tableau::arrange(&Easel::new(2561, 1600, 2.0));
        room.reshape(&wider, &Palette::DARK);
        assert_eq!(baked(&room), (true, true), "the same sizes keep them");
        let mut other = Palette::DARK;
        other.white = other.red;
        room.reshape(&wider, &other);
        assert_eq!(
            baked(&room),
            (false, false),
            "a new palette draws them again"
        );
        room.paint_all(&wider, &Palette::DARK, &scene);
        let small = Tableau::arrange(&Easel::new(1280, 800, 1.0));
        room.reshape(&small, &Palette::DARK);
        assert_eq!(baked(&room), (false, false), "new sizes draw them again");
    }

    #[test]
    fn the_sprite_pools_are_kept_only_while_their_size_and_palette_hold() {
        let (board, field) = lit_board();
        let scene = Scene::bare(&board, &field, Game::C1);
        let tableau = Tableau::arrange(&Easel::new(2560, 1600, 2.0));
        let mut room = Darkroom::new(&tableau, &Palette::DARK);
        room.paint_all(&tableau, &Palette::DARK, &scene);
        let held = |room: &Darkroom| room.sprites.board.len;
        let drawn = held(&room);
        assert!(drawn > 0, "the laser's sprites");
        // One px wider: the board moves, its cell does not.
        let wider = Tableau::arrange(&Easel::new(2561, 1600, 2.0));
        assert_eq!(wider.cell, tableau.cell);
        room.reshape(&wider, &Palette::DARK);
        assert_eq!(held(&room), drawn, "the same cell keeps them");
        let mut other = Palette::DARK;
        other.white = other.red;
        room.reshape(&wider, &other);
        assert_eq!(held(&room), 0, "a new palette draws them again");
        room.paint_all(&wider, &other, &scene);
        let small = Tableau::arrange(&Easel::new(1280, 800, 1.0));
        room.reshape(&small, &other);
        assert_eq!(held(&room), 0, "a new cell draws them again");
    }

    #[test]
    fn the_brand_is_lettered_again_only_when_its_key_changes() {
        let (board, field) = lit_board();
        let c2 = Scene::bare(&board, &field, Game::C2);
        let tableau = Tableau::arrange(&Easel::new(2560, 1600, 2.0));
        let mut room = Darkroom::new(&tableau, &Palette::DARK);
        room.paint_all(&tableau, &Palette::DARK, &c2);
        let kept = room.letters.brand.lettered;
        assert!(kept.is_some(), "the brand is keyed");
        // One px wider: the brand moves, its tile keeps its size, so its light is kept.
        let wider = Tableau::arrange(&Easel::new(2561, 1600, 2.0));
        room.reshape(&wider, &Palette::DARK);
        assert_eq!(
            room.letters.brand.lettered, kept,
            "a move keeps the lettering"
        );
        room.paint_all(&wider, &Palette::DARK, &c2);
        assert_eq!(room.letters.brand.lettered, kept);
        let c3 = Scene::bare(&board, &field, Game::C3);
        room.paint_all(&wider, &Palette::DARK, &c3);
        assert!(room.letters.brand.lettered.is_some_and(|k| Some(k) != kept));
        let small = Tableau::arrange(&Easel::new(1280, 800, 1.0));
        room.reshape(&small, &Palette::DARK);
        assert_eq!(
            room.letters.brand.lettered, None,
            "a new size letters again"
        );
        let long = "x".repeat(LETTERING_TEXT + 1);
        assert!(Lettering::new(&long, pt(0.0, 0.0), [1.0; 3], 1.0, &Palette::DARK).is_none());
    }

    /// The card's tiles keep their size and lettering on a canvas one px wider (the cold rows'
    /// alternation), won with Next ▶ or not, and letter again when the card's key changes.
    #[test]
    fn the_card_is_lettered_again_only_when_its_key_changes() {
        let (board, field) = lit_board();
        let mut won = Scene::bare(&board, &field, Game::C1);
        (won.won, won.strip[1], won.you_win) = (true, SlotState::Unlocked, "You win!");
        assert!(won.next_open());
        let playing = Scene::bare(&board, &field, Game::C1);
        for (w, h, s, touch) in [
            (2450, 1671, 1.75, false),
            (2560, 1600, 2.0, false),
            (780, 1480, 2.0, true),
        ] {
            let at = |w| Tableau::arrange(&Easel::new(w, h, s).with_touch(touch));
            let tableau = at(w);
            let mut room = Darkroom::new(&tableau, &Palette::DARK);
            for scene in [&won, &playing] {
                room.paint_all(&tableau, &Palette::DARK, scene);
                let keys = |room: &Darkroom| {
                    let l = &room.letters;
                    (l.header.lettered, l.next.lettered)
                };
                let sizes = |room: &Darkroom| {
                    let size = |t: &LightTile| (t.sharp.width, t.sharp.height);
                    (size(&room.letters.header), size(&room.letters.next))
                };
                let (kept, size) = (keys(&room), sizes(&room));
                assert!(kept.0.is_some() && kept.1.is_some(), "{w}: keyed");
                let wider = at(w + 1);
                room.reshape(&wider, &Palette::DARK);
                room.paint_all(&wider, &Palette::DARK, scene);
                assert_eq!(sizes(&room), size, "{w} + 1: the tiles keep their size");
                assert_eq!(keys(&room), kept, "{w} + 1: and their lettering");
                room.reshape(&tableau, &Palette::DARK);
            }
            let other = Scene { slot: 1, ..playing };
            let before = room.letters.header.lettered;
            room.paint_all(&tableau, &Palette::DARK, &other);
            assert_ne!(
                room.letters.header.lettered, before,
                "a new level letters again"
            );
        }
    }

    /// Mid-fade poses: opening (its scrim, the panel below its place) and closing (no scrim).
    const MID_POSES: [SheetPose; 2] = [
        SheetPose {
            alpha: 0.4,
            rise_pt: 7.3,
            scrim: true,
        },
        SheetPose {
            alpha: 0.65,
            rise_pt: 4.2,
            scrim: false,
        },
    ];

    /// The sheet draws the same by parts (damage rects) as whole, and on both outputs, shown and
    /// mid-fade; its "LEVELS" tile lies inside the panel and keeps its size and lettering a px
    /// wider. With an inline picker there is no sheet, so no tile.
    #[test]
    fn the_sheet_draws_alike_by_parts_and_keeps_its_lettering() {
        let (board, field) = lit_board();
        let mut scene = Scene::bare(&board, &field, Game::C1);
        (scene.strip[1], scene.strip[2]) = (SlotState::Completed, SlotState::Unlocked);
        let palette = &Palette::DARK;
        for (w, h, s, touch) in [
            (780, 1480, 2.0, true),
            (1600, 1200, 2.0, false),
            (1400, 740, 2.0, true),
        ] {
            let at = |w| Tableau::arrange(&Easel::new(w, h, s).with_touch(touch));
            let tableau = at(w);
            assert!(tableau.picker.is_none(), "{w}: a sheet picker");
            let mut room = Darkroom::new(&tableau, palette);
            room.paint_all(&tableau, palette, &scene);
            let canvas = tableau.canvas();
            let n = (w * h) as usize;
            for pose in [SheetPose::SHOWN, MID_POSES[0], MID_POSES[1]] {
                let mut whole = vec![0u32; n];
                let mut out = Output::Xrgb(&mut whole);
                room.compose(canvas, &mut out);
                room.draw_sheet(&tableau, palette, &scene.strip, &mut out, canvas, pose);
                let tile = room.letters.sheet.rect;
                let panel = tableau.device(tableau.sheet.panel);
                assert!(room.letters.sheet.lettered.is_some(), "{w}: keyed");
                assert_eq!(clip_rect(tile, panel), Some(tile), "{w}: inside the panel");
                // By parts: an uneven 3 × 4 grid of clips.
                let mut parts = vec![0u32; n];
                let mut out = Output::Xrgb(&mut parts);
                room.compose(canvas, &mut out);
                let (xs, ys) = (
                    [0, 97, w as i32 / 2 + 13, w as i32],
                    [0, 211, 480, 901, h as i32],
                );
                for x in xs.windows(2) {
                    for y in ys.windows(2) {
                        let clip = DeviceRect::new(x[0], y[0], x[1], y[1]);
                        room.draw_sheet(&tableau, palette, &scene.strip, &mut out, clip, pose);
                    }
                }
                assert!(parts == whole, "{w} {pose:?}: by parts as whole");
                let mut bytes = vec![0u8; 4 * n];
                let mut out = Output::Rgba(&mut bytes);
                room.compose(canvas, &mut out);
                room.draw_sheet(&tableau, palette, &scene.strip, &mut out, canvas, pose);
                let rgb = |p: u32| [(p >> 16) as u8, (p >> 8) as u8, p as u8, 0xff];
                assert!(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(&whole)
                        .all(|(b, &p)| *b == rgb(p)),
                    "{w} {pose:?}: RGBA as XRGB"
                );
            }
            let (kept, size) = (
                room.letters.sheet.lettered,
                (
                    room.letters.sheet.sharp.width,
                    room.letters.sheet.sharp.height,
                ),
            );
            let wider = at(w + 1);
            room.reshape(&wider, palette);
            room.paint_all(&wider, palette, &scene);
            let mut out = vec![0u32; ((w + 1) * h) as usize];
            let canvas = wider.canvas();
            room.draw_sheet(
                &wider,
                palette,
                &scene.strip,
                &mut Output::Xrgb(&mut out),
                canvas,
                SheetPose::SHOWN,
            );
            let sheet = &room.letters.sheet;
            assert_eq!(
                (sheet.sharp.width, sheet.sharp.height),
                size,
                "{w} + 1: same size"
            );
            assert_eq!(sheet.lettered, kept, "{w} + 1: and lettering");
        }
        let inline = Tableau::arrange(&Easel::new(2560, 1600, 2.0));
        assert!(inline.picker.is_some());
        let room = Darkroom::new(&inline, palette);
        assert!(is_empty(room.letters.sheet.rect), "no sheet, no tile");
    }

    /// The sheet drawn onto black: shown, and at `pose`.
    fn sheets_on_black(palette: &Palette, pose: SheetPose) -> (Vec<u32>, Vec<u32>, Tableau) {
        let (board, field) = lit_board();
        let mut scene = Scene::bare(&board, &field, Game::C1);
        (scene.strip[1], scene.strip[2]) = (SlotState::Completed, SlotState::Unlocked);
        let tableau = Tableau::arrange(&Easel::new(780, 1480, 2.0).with_touch(true));
        let mut room = Darkroom::new(&tableau, palette);
        let canvas = tableau.canvas();
        let mut draw = |pose| {
            let mut px = vec![0u32; 780 * 1480];
            let mut out = Output::Xrgb(&mut px);
            room.draw_sheet(&tableau, palette, &scene.strip, &mut out, canvas, pose);
            px
        };
        (draw(SheetPose::SHOWN), draw(pose), tableau)
    }

    /// Risen `rise_pt` at full alpha, every part of the sheet is the shown one moved that far
    /// down (whole px), the rows it left showing the scrim alone; nothing is cut off.
    #[test]
    fn the_rising_sheet_is_the_shown_one_lower() {
        let rise = SheetPose {
            alpha: 1.0,
            rise_pt: 6.0,
            scrim: true,
        };
        let (shown, risen, tableau) = sheets_on_black(&Palette::DARK, rise);
        let dy = (6.0 * tableau.px_per_pt()).round() as usize;
        assert_eq!(dy, 12);
        let w = 780;
        assert!(
            risen[w * dy..] == shown[..shown.len() - w * dy],
            "moved down whole"
        );
        let scrim = shown[w * 2 + w / 2];
        assert!(
            risen[..w * dy].iter().all(|&p| p == scrim),
            "only the scrim above"
        );
        let faded = SheetPose {
            alpha: 0.0,
            rise_pt: 12.0,
            scrim: false,
        };
        let (_, gone, _) = sheets_on_black(&Palette::DARK, faded);
        assert!(gone.iter().all(|&p| p == 0), "faded out, nothing drawn");
    }

    /// Ruling F14: mid-fade, every pixel the sheet draws (the scrim, shadow, panel, "LEVELS", the
    /// close chip, the chips and their digits) takes its colour from the palette alone; and the
    /// pose is drawn: inside the panel (above its bottom edge, below the chips) it is dimmer than
    /// the shown sheet.
    #[test]
    fn the_sheet_mid_fade_takes_every_colour_from_the_palette() {
        for theme in Theme::ALL {
            let sentinel = theme.palette().recoloured(crate::Rgb(255, 0, 255));
            for pose in MID_POSES {
                let (shown, px, tableau) = sheets_on_black(&sentinel, pose);
                let p = tableau.device(tableau.sheet.panel);
                let inside = ((p.bottom - 4) * 780 + (p.left + p.right) / 2) as usize;
                let red = |p: u32| p >> 16 & 0xff;
                assert!(
                    red(px[inside]) < red(shown[inside]),
                    "{theme:?} {pose:?}: the panel fades ({:06x} against {:06x} shown)",
                    px[inside],
                    shown[inside]
                );
                assert!(
                    px.iter().filter(|&&p| p != 0).count() > 100_000,
                    "{theme:?} {pose:?}: drawn"
                );
                for &p in &px {
                    let (r, g, b) = (p >> 16 & 0xff, p >> 8 & 0xff, p & 0xff);
                    assert!(g == 0 && r.abs_diff(b) <= 1, "{theme:?} {pose:?}: {p:06x}");
                }
            }
        }
    }

    /// Global Constraints: a whole light scene (a lit board with crossing beams and lit pieces,
    /// a won card with Next ▶ and the ink header, the brand, the open sheet) takes every colour
    /// from the palette: painted with a sentinel, every pixel is the sentinel's hue.
    #[test]
    fn a_light_scene_takes_every_colour_from_the_palette() {
        let sentinel = Palette::LIGHT.recoloured(crate::Rgb(255, 0, 255));
        let mut board = Board::default();
        for (x, y, piece, colour, dir) in [
            (2, 7, Piece::Laser, 4, 2),
            (7, 12, Piece::Laser, 2, 0),
            (11, 7, Piece::Pinwheel, 4, 0),
            (7, 3, Piece::Mirror, 0, 1),
        ] {
            board.cells[board_index(x, y)] = Cell { piece, colour, dir };
        }
        let mut field = BeamField::default();
        trace(&board, &mut field);
        for (easel, sheet) in [
            (Easel::new(1280, 800, 1.0), false),
            (Easel::new(780, 1480, 2.0).with_touch(true), true),
        ] {
            let tableau = Tableau::arrange(&easel);
            let mut room = Darkroom::new(&tableau, &sentinel);
            let mut scene = Scene::bare(&board, &field, Game::C1);
            (scene.won, scene.you_win) = (true, "YOU WIN!");
            scene.strip[1] = SlotState::Unlocked; // Next ▶ shows
            room.paint_all(&tableau, &sentinel, &scene);
            let (w, h) = (easel.width as usize, easel.height as usize);
            let mut px = vec![0u32; w * h];
            let canvas = tableau.canvas();
            room.compose(canvas, &mut Output::Xrgb(&mut px));
            if sheet {
                let mut out = Output::Xrgb(&mut px);
                room.draw_sheet(
                    &tableau,
                    &sentinel,
                    &scene.strip,
                    &mut out,
                    canvas,
                    SheetPose::SHOWN,
                );
            }
            let mut lit = 0;
            for &p in &px {
                let (r, g, b) = (p >> 16 & 0xff, p >> 8 & 0xff, p & 0xff);
                assert!(g == 0 && r.abs_diff(b) <= 1, "{easel:?}: {p:06x}");
                lit += usize::from(p != 0);
            }
            assert!(lit > w * h / 2, "{easel:?}: painted ({lit} px)");
        }
    }

    /// Ruling L2 (Task 2 review): in `Over` the four dark lettering tiles are emptied and their
    /// planes given back; back in `Additive` they are sized again.
    #[test]
    fn the_light_theme_gives_back_the_lettering_tiles() {
        for easel in [
            Easel::new(1280, 800, 1.0),
            Easel::new(780, 1480, 2.0).with_touch(true),
        ] {
            let tableau = Tableau::arrange(&easel);
            let mut room = Darkroom::new(&tableau, &Palette::DARK);
            let tiles = |room: &Darkroom| {
                let l = &room.letters;
                [&l.brand, &l.header, &l.next, &l.sheet].map(|t| (t.rect, t.bytes()))
            };
            let dark = tiles(&room);
            let (lit, dark_bytes) = (
                dark.iter().filter(|(r, _)| !is_empty(*r)).count(),
                dark.iter().map(|t| t.1).sum::<usize>(),
            );
            // 1280 × 800 has the brand, header and Next ▶ (an inline picker, no sheet); the
            // phone has the header, Next ▶ and the sheet's "LEVELS".
            assert_eq!(lit, 3, "{easel:?}: the dark tiles");
            room.reshape(&tableau, &Palette::LIGHT);
            let light = tiles(&room);
            for (k, (rect, bytes)) in light.into_iter().enumerate() {
                assert!(is_empty(rect), "{easel:?}: tile {k} emptied");
                assert!(bytes <= 256, "{easel:?}: tile {k} holds {bytes} B");
            }
            assert!(dark_bytes > 100_000, "{easel:?}: dark tiles {dark_bytes} B");
            room.reshape(&tableau, &Palette::DARK);
            assert_eq!(tiles(&room), dark, "{easel:?}: sized again in dark");
        }
    }

    #[test]
    fn the_shoulder_is_identity_then_eases_to_one() {
        assert_eq!(shoulder(0.5), 0.5);
        assert_eq!(shoulder(0.8), 0.8);
        assert!((shoulder(1.0) - (0.8 + 0.2 * (1.0 - (-1.0f32).exp()))).abs() < 1e-6);
        assert!(shoulder(100.0) < 1.0 && shoulder(100.0) > 0.999);
    }

    #[test]
    fn the_shoulders_identity_part_needs_no_lookup() {
        let room = Darkroom::new(&Tableau::arrange(&Easel::new(64, 64, 1.0)), &Palette::DARK);
        for l in 0..=IDENTITY_TOP {
            assert_eq!(
                u32::from(room.lut[l as usize]),
                (255 * l + ONE / 2) >> 12,
                "{l}"
            );
        }
    }

    #[test]
    fn halved_light_matches_plain_half_taps() {
        // The per-pixel definition: device column c's two `half_tap` taps of the blended row,
        // plus its sharp value. Spans of 1–3 px and longer, from even and odd columns.
        let mut seed = 0x9e37_79b9_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        // A blended row holds 256-weighted u16 sums: up to 65 535 · 256.
        let row: Vec<u32> = (0..80).map(|_| next() % (65_535 * 256 + 1)).collect();
        let sharp: Vec<u16> = (0..140).map(|_| next() as u16).collect();
        for c0 in 0..9 {
            for n in [1, 2, 3, 4, 5, 7, 64, 129] {
                let sharp = &sharp[c0..c0 + n];
                let mut light = vec![u32::MAX; n];
                halved_light(&row, c0, sharp, &mut light);
                for (i, &l) in light.iter().enumerate() {
                    let (a, b, w) = half_tap(c0 + i);
                    let w = u32::from(w);
                    let halo = (row[a as usize] * (256 - w) + row[b as usize] * w + 32_768) >> 16;
                    assert_eq!(l, u32::from(sharp[i]) + halo, "c0 {c0}, {n} px, column {i}");
                }
            }
        }
    }

    #[test]
    fn a_beam_glows_red_over_the_board() {
        let (board, field) = lit_board();
        let (_, tableau, out) = painted(Easel::new(1280, 800, 1.0), &board, &field);
        let (_, _, dark) = painted(
            Easel::new(1280, 800, 1.0),
            &Board::default(),
            &BeamField::default(),
        );
        let w = 1280;
        let row = tableau.cell_centre(board_index(7, 7));
        let on_beam = out[row.y as usize * w + row.x as usize];
        let i = (row.y as usize - tableau.cell as usize / 4) * w + row.x as usize;
        assert!(
            red(on_beam) > 200,
            "the beam's line and core: {on_beam:06x}"
        );
        assert!(
            red(out[i]) > red(dark[i]) + 3,
            "glow reaches a quarter cell off the line: {:06x} vs unlit {:06x}",
            out[i],
            dark[i]
        );
        // σ_b = 0.5u at weight 0.35 lifts the next row by only ~1 code value, but it does lift it.
        let next = (row.y as usize - tableau.cell as usize) * w + row.x as usize;
        assert!(
            red(out[next]) > red(dark[next]),
            "bloom reaches the next row"
        );
        assert!(red(on_beam) > (on_beam >> 8 & 0xff), "and it is red");
    }

    #[test]
    fn composing_a_rect_matches_the_full_compose() {
        let (board, field) = lit_board();
        let (mut room, tableau, full) = painted(Easel::new(1280, 800, 1.0), &board, &field);
        let mut part = vec![0u32; full.len()];
        let rect = DeviceRect::new(100, 300, 700, 420);
        room.compose(rect, &mut Output::Xrgb(&mut part));
        for y in 300..420 {
            for x in 100..700 {
                assert_eq!(part[y * 1280 + x], full[y * 1280 + x]);
            }
        }
        assert_eq!(part[0], 0, "outside the rect is untouched");
        let mut bytes = vec![0u8; full.len() * 4];
        room.compose(tableau.canvas(), &mut Output::Rgba(&mut bytes));
        let i = 400 * 1280 + 400;
        assert_eq!(
            [
                bytes[i * 4],
                bytes[i * 4 + 1],
                bytes[i * 4 + 2],
                bytes[i * 4 + 3]
            ],
            [
                (full[i] >> 16) as u8,
                (full[i] >> 8) as u8,
                full[i] as u8,
                255
            ]
        );
    }

    #[test]
    fn the_level_fade_lerps_the_board_region_from_the_kept_frame_on_both_outputs() {
        let (board, field) = lit_board();
        let (mut room, _, full) = painted(Easel::new(1280, 800, 1.0), &board, &field);
        let f = room.fade_rect();
        assert_eq!(f, room.regions.board);
        assert!(
            f.left < 640 && f.right > 640,
            "the half canvas splits the region"
        );
        let mut old: Vec<u32> = (0..full.len() as u32)
            .map(|i| i.wrapping_mul(2_654_435_761) & 0xff_ffff)
            .collect();
        let rgba = |px: &[u32]| -> Vec<u8> {
            px.iter()
                .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8, 255])
                .collect()
        };
        let mut old_bytes = rgba(&old);
        let mix = |o: u32, n: u32, w: u32| {
            let ch = |s: u32| ((o >> s & 0xff) * (256 - w) + (n >> s & 0xff) * w + 128) >> 8;
            ch(16) << 16 | ch(8) << 8 | ch(0)
        };
        // Half the canvas: only its part of the board region fades.
        let rect = DeviceRect::new(0, 0, 640, 800);
        for (k, w) in [(0.0, 0), (0.5, 128), (1.0, 256)] {
            room.keep_for_fade(&Output::Xrgb(&mut old));
            let mut out = full.clone();
            room.fade_into(rect, &mut Output::Xrgb(&mut out), k);
            for (i, (&p, (&n, &o))) in out.iter().zip(full.iter().zip(&old)).enumerate() {
                let (x, y) = ((i % 1280) as i32, (i / 1280) as i32);
                let inside =
                    (f.left..f.right.min(640)).contains(&x) && (f.top..f.bottom).contains(&y);
                assert_eq!(
                    p,
                    if inside { mix(o, n, w) } else { n },
                    "k {k} at ({x}, {y})"
                );
            }
            room.keep_for_fade(&Output::Rgba(&mut old_bytes));
            let mut bytes = rgba(&full);
            room.fade_into(rect, &mut Output::Rgba(&mut bytes), k);
            assert!(bytes == rgba(&out), "k {k}: the web's output fades alike");
        }
    }

    /// Xorshift32: the same "random" light on every run.
    struct Dice(u32);

    impl Dice {
        /// Below `n` (0 for `n` = 0).
        fn below(&mut self, n: u32) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0 % n.max(1)
        }

        /// A rect of up to 60 × 60 from (`x0`, `y0`) (random when `None`), inside `sw` × `sh`.
        fn rect(&mut self, sw: u32, sh: u32, at: Option<(u32, u32)>) -> DeviceRect {
            let (x0, y0) = at.unwrap_or_else(|| (self.below(sw), self.below(sh)));
            let (x1, y1) = (
                (x0 + 1 + self.below(60)).min(sw),
                (y0 + 1 + self.below(60)).min(sh),
            );
            DeviceRect::new(x0 as i32, y0 as i32, x1 as i32, y1 as i32)
        }
    }

    /// The same light at (`x`, `y`) of both tiles' `sharp`.
    fn speck(tiles: [&mut LightTile; 2], x: usize, y: usize, v: u16) {
        for tile in tiles {
            let i = y * tile.sharp.width + x;
            (tile.sharp.r[i], tile.sharp.g[i], tile.sharp.b[i]) = (v, v / 2, v / 3);
        }
    }

    /// Every channel of `a` and `b` alike.
    fn same(a: &Planes, b: &Planes) -> bool {
        (a.width, a.height) == (b.width, b.height) && a.r == b.r && a.g == b.g && a.b == b.b
    }

    /// Per ½ px column (or row), the first and last device column (row) of the tile whose
    /// compose taps read it (`taps` being `halo_cols` or `halo_rows`); empty when none does.
    fn readers(taps: &[Tap], len: usize) -> Vec<(i32, i32)> {
        let mut first_last = vec![(i32::MAX, i32::MIN); len];
        for (c, &(a, b, _)) in taps.iter().enumerate() {
            for p in [a, b].map(|p| p as usize) {
                if let Some(e) = p.checked_sub(1).and_then(|p| first_last.get_mut(p)) {
                    *e = (e.0.min(c as i32), e.1.max(c as i32));
                }
            }
        }
        first_last
    }

    /// Changes the light of two tiles that hold the same light inside `dirty` (sharp px, a third
    /// of them lit at random), then develops `patched` by patches and `whole` whole: they must
    /// agree bit for bit, and every device px whose compose reads a halo px that changed must lie
    /// in the damage `develop_patches` reports.
    fn patch_and_compare(
        [patched, whole]: [&mut LightTile; 2],
        dirty: &[DeviceRect],
        dice: &mut Dice,
        what: &str,
    ) {
        let (sw, sh) = (patched.sharp.width as i32, patched.sharp.height as i32);
        for &d in dirty {
            for y in d.top.max(0)..d.bottom.min(sh) {
                for x in d.left.max(0)..d.right.min(sw) {
                    let v = if dice.below(3) == 0 {
                        dice.below(40_000) as u16
                    } else {
                        0
                    };
                    speck([&mut *patched, &mut *whole], x as usize, y as usize, v);
                }
            }
        }
        let before = patched.half.clone();
        let mut device = Patchwork::new();
        patched.develop_patches(dirty, &mut device);
        whole.develop(whole.sigmas.0, whole.sigmas.1);
        assert!(same(&patched.coarse, &whole.coarse), "{what}: the ⅛ means");
        assert!(same(&patched.eighth, &whole.eighth), "{what}: the bloom");
        assert!(same(&patched.half, &whole.half), "{what}: the halo");
        let half = &patched.half;
        let cols = readers(&patched.halo_cols, half.width);
        let rows = readers(&patched.halo_rows, half.height);
        let (left, top) = (patched.rect.left, patched.rect.top);
        for (y, &(r0, r1)) in rows.iter().enumerate() {
            for (x, &(c0, c1)) in cols.iter().enumerate() {
                let i = y * half.width + x;
                let unchanged = before.r[i] == half.r[i]
                    && before.g[i] == half.g[i]
                    && before.b[i] == half.b[i];
                if unchanged || c0 > c1 || r0 > r1 {
                    continue;
                }
                let read = DeviceRect::new(left + c0, top + r0, left + c1 + 1, top + r1 + 1);
                assert!(
                    device.as_slice().iter().any(|&r| hull(r, read) == r),
                    "{what}: halo ({x}, {y}), read by {read:?}, changed outside the damage {:?}",
                    device.as_slice()
                );
            }
        }
        // The sharp light itself: every device px whose compose reads a sharp px in `dirty`.
        let (cols, rows) = if patched.scale >= 1.0 {
            let one = |n: i32| (0..n).map(|c| (c, c)).collect::<Vec<_>>();
            (one(sw), one(sh))
        } else {
            let (w, h) = (sw as usize, sh as usize);
            (
                readers(&patched.sharp_cols, w),
                readers(&patched.sharp_rows, h),
            )
        };
        for &d in dirty {
            for y in d.top.max(0)..d.bottom.min(sh) {
                for x in d.left.max(0)..d.right.min(sw) {
                    let ((c0, c1), (r0, r1)) = (cols[x as usize], rows[y as usize]);
                    if c0 > c1 || r0 > r1 {
                        continue;
                    }
                    let read = DeviceRect::new(left + c0, top + r0, left + c1 + 1, top + r1 + 1);
                    assert!(
                        device.as_slice().iter().any(|&r| hull(r, read) == r),
                        "{what}: sharp ({x}, {y}), read by {read:?}, outside the damage {:?}",
                        device.as_slice()
                    );
                }
            }
        }
    }

    /// Two tiles over `rect` holding the same 400 random specks, developed whole for cells of
    /// `u` device px, `patched` with room for its glow band.
    fn twin_tiles(rect: DeviceRect, max_px: f32, u: f32, dice: &mut Dice) -> [LightTile; 2] {
        let (mut patched, mut whole) = (LightTile::default(), LightTile::default());
        patched.reshape(rect, max_px);
        whole.reshape(rect, max_px);
        let u = u * patched.scale;
        let (glow, bloom) = (GLOW_SIGMA * u, BLOOM_SIGMA * u);
        patched.reserve_band(box_radius(glow / 2.0));
        let (sw, sh) = (patched.sharp.width as u32, patched.sharp.height as u32);
        for _ in 0..400 {
            let (x, y, v) = (dice.below(sw), dice.below(sh), dice.below(30_000));
            speck([&mut patched, &mut whole], x as usize, y as usize, v as u16);
        }
        patched.develop(glow, bloom);
        whole.develop(glow, bloom);
        [patched, whole]
    }

    #[test]
    fn patched_develops_equal_whole_ones() {
        let mut dice = Dice(0x9e37_79b9);
        // Odd sizes, and a tile whose light is drawn at 1800 / 2100 of its size.
        for (w, h, max_px) in [
            (301, 257, f32::INFINITY),
            (97, 61, f32::INFINITY),
            (2100, 48, 1800.0),
        ] {
            let rect = DeviceRect::new(0, 0, w, h);
            let [mut patched, mut whole] = twin_tiles(rect, max_px, 40.0, &mut dice);
            let (sw, sh) = (patched.sharp.width as u32, patched.sharp.height as u32);
            for round in 0..10 {
                // Three rects a round: the corners, overlapping rects, the edges and one
                // hanging off the plane; then 24 at once (past every patchwork's room).
                let count = if round == 9 { 24 } else { 3 };
                let mut dirty = [DeviceRect::default(); 24];
                for k in 0..count {
                    let first = dirty[0];
                    dirty[k] = match (round, k) {
                        (0, 0) => dice.rect(sw, sh, Some((0, 0))),
                        (1, 0) => dice.rect(sw, sh, Some((sw - 5, sh - 5))),
                        (2, 1) => {
                            let inside = ((first.left + first.right) / 2, first.top);
                            let r = dice.rect(sw, sh, Some((inside.0 as u32, inside.1 as u32)));
                            DeviceRect::new(r.left, r.top, r.right.max(first.right + 7), r.bottom)
                        }
                        (3, 0) => dice.rect(sw, sh, Some((sw - 3, 0))),
                        (3, 1) => dice.rect(sw, sh, Some((0, sh - 3))),
                        (3, 2) => {
                            let (x, y) = (sw as i32 - 3, sh as i32 - 2);
                            DeviceRect::new(x, y, x + 12, y + 9)
                        }
                        _ => dice.rect(sw, sh, None),
                    };
                }
                let what = format!("{w}×{h} round {round}");
                patch_and_compare(
                    [&mut patched, &mut whole],
                    &dirty[..count],
                    &mut dice,
                    &what,
                );
            }
        }
    }

    #[test]
    fn a_halo_windows_device_rect_holds_every_px_that_reads_it() {
        // Scale 1, odd offsets, and tiles drawn at 1800 / 2100 and 1800 / 3001 of their size.
        for (rect, max_px) in [
            (DeviceRect::new(13, 7, 314, 264), f32::INFINITY),
            (DeviceRect::new(0, 0, 2100, 48), 1800.0),
            (DeviceRect::new(5, 3, 3006, 30), 1800.0),
        ] {
            let mut tile = LightTile::default();
            tile.reshape(rect, max_px);
            let cols = readers(&tile.halo_cols, tile.half.width);
            let rows = readers(&tile.halo_rows, tile.half.height);
            for (y, &(r0, r1)) in rows.iter().enumerate() {
                for (x, &(c0, c1)) in cols.iter().enumerate() {
                    let (x, y) = (x as i32, y as i32);
                    let d = tile.device_of_half(DeviceRect::new(x, y, x + 1, y + 1));
                    let read = DeviceRect::new(
                        rect.left + c0,
                        rect.top + r0,
                        rect.left + c1 + 1,
                        rect.top + r1 + 1,
                    );
                    assert!(
                        c0 > c1 || r0 > r1 || hull(d, read) == d,
                        "{rect:?}: ({x}, {y}) is read by {read:?}, outside {d:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_radius_in_use_develops_by_patches_exactly() {
        // A board tile holds the board (15 cells), so a cell is at most 1800 / 15 light px:
        // every (glow, bloom) box radius pair a board tile can develop with.
        let mut pairs: Vec<((usize, usize), f32)> = Vec::new();
        for k in 1..=12_000 {
            let u = k as f32 / 100.0;
            let radii = (
                box_radius(GLOW_SIGMA * u / 2.0),
                box_radius(BLOOM_SIGMA * u / 8.0),
            );
            if !pairs.iter().any(|&(seen, _)| seen == radii) {
                pairs.push((radii, u));
            }
        }
        assert!(pairs.len() >= 10, "{pairs:?}");
        let mut dice = Dice(0x2545_f491);
        for (radii, u) in pairs {
            let rect = DeviceRect::new(0, 0, 641, 383);
            let [mut patched, mut whole] = twin_tiles(rect, f32::INFINITY, u, &mut dice);
            assert_eq!(patched.radii, radii);
            for round in 0..4 {
                let dirty = [dice.rect(641, 383, None), dice.rect(641, 383, None)];
                let what = format!("radii {radii:?} round {round}");
                patch_and_compare([&mut patched, &mut whole], &dirty, &mut dice, &what);
            }
        }
    }

    #[test]
    fn board_tiles_develop_by_patches_exactly_on_desktops_and_phones() {
        let (board, field) = lit_board();
        let mut dice = Dice(0x1b87_3593);
        for easel in [
            Easel::new(1280, 800, 1.0),
            Easel::new(2560, 1600, 2.0),
            Easel::new(3456, 2234, 2.0),
            Easel::new(780, 1480, 2.0).with_touch(true),
            Easel::new(1688, 780, 2.0).with_touch(true),
        ] {
            let tableau = Tableau::arrange(&easel);
            let scene = Scene::bare(&board, &field, Game::C1);
            let (mut a, mut b) = (
                Darkroom::new(&tableau, &Palette::DARK),
                Darkroom::new(&tableau, &Palette::DARK),
            );
            a.paint_all(&tableau, &Palette::DARK, &scene);
            b.paint_all(&tableau, &Palette::DARK, &scene);
            let (sw, sh) = (a.light.sharp.width as u32, a.light.sharp.height as u32);
            for round in 0..3 {
                let dirty = [
                    dice.rect(sw, sh, None),
                    dice.rect(sw, sh, None),
                    dice.rect(sw, sh, Some((sw / 2, sh / 2))),
                ];
                let what = format!("{easel:?} round {round}");
                patch_and_compare([&mut a.light, &mut b.light], &dirty, &mut dice, &what);
            }
        }
    }

    #[test]
    fn one_px_patches_at_the_corners_and_on_eighth_edges_develop_exactly() {
        let mut dice = Dice(0x5bd1_e995);
        for (w, h, max_px) in [
            (301, 257, f32::INFINITY),
            (97, 61, f32::INFINITY),
            (2100, 48, 1800.0),
        ] {
            let rect = DeviceRect::new(0, 0, w, h);
            let [mut patched, mut whole] = twin_tiles(rect, max_px, 40.0, &mut dice);
            let (sw, sh) = (patched.sharp.width as i32, patched.sharp.height as i32);
            let (k, j) = (sw / 16, sh / 16);
            let (mx, my) = (sw / 2, sh / 2);
            for (x, y) in [
                (0, 0),
                (sw - 1, sh - 1),
                (7, my),
                (8, my),
                (8 * k - 1, my),
                (8 * k, my),
                (mx, 8 * j - 1),
                (mx, 8 * j),
                (8 * k - 1, 8 * j - 1),
                (8 * k, 8 * j),
            ] {
                // Lit in both first, so the patch below surely changes the px.
                speck([&mut patched, &mut whole], x as usize, y as usize, 50_000);
                for tile in [&mut patched, &mut whole] {
                    tile.develop(tile.sigmas.0, tile.sigmas.1);
                }
                let what = format!("{w}×{h}: 1 × 1 at ({x}, {y})");
                let one = [DeviceRect::new(x, y, x + 1, y + 1)];
                patch_and_compare([&mut patched, &mut whole], &one, &mut dice, &what);
            }
        }
    }

    #[test]
    fn a_change_develops_by_patches_until_its_windows_cost_more_than_the_whole() {
        let mut dice = Dice(0x27d4_eb2f);
        let rect = DeviceRect::new(0, 0, 801, 801);
        let [mut patched, mut whole] = twin_tiles(rect, f32::INFINITY, 53.0, &mut dice);
        for (dirty, by_patches) in [
            (DeviceRect::new(400, 400, 401, 401), true),
            (DeviceRect::new(100, 100, 700, 700), false),
        ] {
            // Light inside `dirty` only: its centre and its top-right corner px.
            let (x, y) = (dirty.right as usize - 1, dirty.top as usize);
            speck(
                [&mut patched, &mut whole],
                400,
                400,
                dice.below(40_000) as u16,
            );
            speck([&mut patched, &mut whole], x, y, dice.below(40_000) as u16);
            let mut device = Patchwork::new();
            patched.develop_changed(&[dirty], &mut device);
            whole.develop(whole.sigmas.0, whole.sigmas.1);
            assert!(same(&patched.half, &whole.half), "{dirty:?}: halo");
            assert!(same(&patched.eighth, &whole.eighth), "{dirty:?}: bloom");
            assert_eq!(
                device.as_slice() != [rect],
                by_patches,
                "{dirty:?}: {device:?}"
            );
        }
    }

    #[test]
    fn a_tile_without_a_band_develops_whole_for_any_patch() {
        let mut dice = Dice(0x68e3_1da4);
        let rect = DeviceRect::new(0, 0, 120, 40);
        let [mut patched, mut whole] = twin_tiles(rect, f32::INFINITY, 30.0, &mut dice);
        (patched.band_cap, patched.band) = (0, Vec::new());
        speck([&mut patched, &mut whole], 60, 20, 40_000);
        let mut device = Patchwork::new();
        patched.develop_patches(&[DeviceRect::new(60, 20, 61, 21)], &mut device);
        whole.develop(whole.sigmas.0, whole.sigmas.1);
        assert!(same(&patched.half, &whole.half) && same(&patched.eighth, &whole.eighth));
        assert_eq!(device.as_slice(), &[rect], "its whole rect");
    }

    /// The board with `pieces` (x, y, piece, colour, dir), traced.
    fn lit(pieces: &[(usize, usize, Piece, u8, u8)]) -> (Board, BeamField) {
        let mut board = Board::default();
        for &(x, y, piece, colour, dir) in pieces {
            board.cells[board_index(x, y)] = Cell { piece, colour, dir };
        }
        let mut field = BeamField::default();
        trace(&board, &mut field);
        (board, field)
    }

    /// A darkroom painted cold for `a`, then its light framed incrementally for `b`, against one
    /// painted cold for `b`: their light tiles (sharp, its ⅛ means, halo and bloom) match in
    /// every channel. Returns whether it developed by patches (not whole).
    fn light_after(easel: Easel, a: (&Board, &BeamField), b: (&Board, &BeamField)) -> bool {
        let tableau = Tableau::arrange(&easel);
        let palette = Theme::Dark.palette();
        let (mut incremental, mut fresh) = (
            Darkroom::new(&tableau, palette),
            Darkroom::new(&tableau, palette),
        );
        incremental.paint_all(&tableau, palette, &Scene::bare(a.0, a.1, Game::C1));
        incremental.light_frame(
            &tableau,
            palette,
            &Scene::bare(b.0, b.1, Game::C1),
            None,
            false,
        );
        fresh.paint_all(&tableau, palette, &Scene::bare(b.0, b.1, Game::C1));
        let (x, y) = (&incremental.light, &fresh.light);
        assert!(same(&x.sharp, &y.sharp), "{easel:?}: sharp");
        assert!(same(&x.coarse, &y.coarse), "{easel:?}: ⅛ means");
        assert!(same(&x.half, &y.half), "{easel:?}: halo");
        assert!(same(&x.eighth, &y.eighth), "{easel:?}: bloom");
        incremental.lit.as_slice() != [incremental.light.rect]
    }

    #[test]
    fn patched_board_light_equals_a_whole_one() {
        let laser = (2, 5, Piece::Laser, 7, 2);
        let boards = [
            vec![laser, (9, 5, Piece::Mirror, 0, 1)],
            vec![laser, (9, 5, Piece::Mirror, 0, 3)],
            vec![
                laser,
                (9, 5, Piece::Splitter, 0, 1),
                (9, 12, Piece::Pinwheel, 7, 0),
            ],
            vec![laser],
            // A piece off the beams: its light alone changes (a small, windowed develop).
            vec![laser, (12, 12, Piece::Mirror, 0, 1)],
            vec![
                (0, 0, Piece::Laser, 4, 3),
                (14, 14, Piece::Mirror, 0, 2),
                laser,
            ],
        ];
        let parts: Vec<_> = boards.iter().map(|p| lit(p)).collect();
        for easel in [
            Easel::new(800, 600, 1.0),
            Easel::new(2560, 1600, 2.0),
            Easel::new(3456, 2234, 2.0),
        ] {
            let mut by_patches = 0;
            for (i, a) in parts.iter().enumerate() {
                for b in &parts[i + 1..] {
                    by_patches += usize::from(light_after(easel, (&a.0, &a.1), (&b.0, &b.1)));
                    by_patches += usize::from(light_after(easel, (&b.0, &b.1), (&a.0, &a.1)));
                }
            }
            assert!(by_patches >= 2, "{easel:?}: some edits develop by patches");
        }
    }

    /// Every light item of `scene` settled, each piece light also as a landing piece's, and
    /// pieces of half-segments mid-sweep.
    fn every_item(scene: &Scene) -> Vec<Change> {
        let mut bill = Playbill::new();
        bill.build(scene, None, &Pose::SETTLED);
        let empty = Playbill::new();
        let mut items = Vec::new();
        bill.changes(&empty, |c| items.push(c));
        // Each piece light as a landing piece's too: off its cell and at the touch lift.
        let landing: Vec<Change> = items
            .iter()
            .filter_map(|c| match *c {
                Change::Light(i, cl) => Some(Change::Light(
                    i,
                    CellLight {
                        offset: pt(7.5, -5.25),
                        size: 1.3,
                        ..cl
                    },
                )),
                _ => None,
            })
            .collect();
        items.extend(landing);
        // Pieces of half-segments mid-sweep: diagonal ones and a whole straight one, white and
        // in a colour (the light theme strokes white differently).
        for (side, from, to, mask) in [
            (3, 0.2, 0.7, 7),
            (2, 0.0, 1.0, 7),
            (1, 0.1, 0.9, 1),
            (5, 0.0, 0.6, 6),
        ] {
            items.push(Change::Partial(Partial {
                order: 0,
                cell: board_index(7, 7) as u16,
                side,
                from,
                to,
                mask,
            }));
        }
        items
    }

    /// A playbill of item `c` alone.
    fn alone(c: Change) -> Box<Playbill> {
        let mut one = Playbill::new();
        match c {
            Change::Run(r) => one.line.push_run(r),
            Change::Partial(p) => one.line.push_partial(p),
            Change::Glint(g) => one.line.push_glint(g),
            Change::Light(i, cl) => one.lights[i] = cl,
        }
        one
    }

    #[test]
    fn every_light_item_stays_in_its_box() {
        let easel = Easel::new(1280, 800, 1.0);
        let tableau = Tableau::arrange(&easel);
        let palette = Theme::Dark.palette();
        let mut room = Darkroom::new(&tableau, palette);
        for page in crate::specimen::pages() {
            let scene = Scene::bare(&page.board, &page.field, page.game);
            let map = LightMap::of(&tableau, room.light.rect, room.light.scale);
            let plane = DeviceRect::new(
                0,
                0,
                room.light.sharp.width as i32,
                room.light.sharp.height as i32,
            );
            for c in every_item(&scene) {
                // The item alone, drawn over the whole plane, stays inside its box.
                let one = alone(c);
                room.light.wipe_rect(plane);
                let drawn = draw_bill(
                    &mut room.light,
                    &map,
                    &tableau,
                    palette,
                    scene.game,
                    &one,
                    plane,
                );
                if let Some(d) = drawn {
                    let b = map.change_box(&tableau, &c, DARK_LINE_REACH);
                    assert!(
                        d.left >= b.left
                            && d.top >= b.top
                            && d.right <= b.right
                            && d.bottom <= b.bottom,
                        "{c:?}: drawn {d:?} past its box {b:?}"
                    );
                }
            }
        }
    }

    /// The light twin (ruling L4): each wash item drawn alone paints, in each layer, only the
    /// layer px that hold some of its change box (`STROKE_REACH`). So a frame that wipes and
    /// paints again `coarser(box, f)` in each layer reaches every px the item paints, and an
    /// item that paints a px inside a layer's clip always meets that clip's sharp-px extent.
    #[test]
    fn every_wash_item_stays_in_its_box_in_every_layer() {
        for easel in [
            Easel::new(1280, 800, 1.0),
            Easel::new(1281, 800, 1.0),
            Easel::new(780, 1688, 2.0).with_touch(true),
            Easel::new(3456, 2234, 2.0),
        ] {
            let tableau = Tableau::arrange(&easel);
            let palette = Theme::Light.palette();
            let mut room = Darkroom::new(&tableau, palette);
            let map = LightMap::of(&tableau, room.wash.rect, room.wash.scale);
            let plane = DeviceRect::new(
                0,
                0,
                room.wash.sharp.width as i32,
                room.wash.sharp.height as i32,
            );
            room.wash.wipe(plane);
            let mut last = DeviceRect::default();
            let mut checked = 0;
            for page in crate::specimen::pages() {
                let scene = Scene::bare(&page.board, &page.field, page.game);
                for c in every_item(&scene) {
                    room.wash.wipe(last);
                    let one = alone(c);
                    let layers = draw_wash(
                        &mut room.wash,
                        &map,
                        &tableau,
                        palette,
                        scene.game,
                        &one,
                        plane,
                    );
                    last = layers.into_iter().fold(DeviceRect::default(), union);
                    let b = map.change_box(&tableau, &c, STROKE_REACH);
                    for (d, f) in layers.into_iter().zip([4, 2, 1]) {
                        if is_empty(d) {
                            continue;
                        }
                        let (d, held) = (coarser(d, f), coarser(b, f));
                        assert!(
                            d.left >= held.left
                                && d.top >= held.top
                                && d.right <= held.right
                                && d.bottom <= held.bottom,
                            "{easel:?} {c:?}: layer ×{f} painted {d:?} past its box's {held:?}"
                        );
                        checked += 1;
                    }
                }
            }
            assert!(checked > 100, "{easel:?}: {checked} layers checked");
        }
    }

    /// The light twin of `light_after` (ruling L4): cold for `a`, then framed incrementally for
    /// `b`, against cold for `b`. Every wash layer matches byte for byte. Returns whether it
    /// developed by windows.
    fn wash_after(easel: Easel, a: (&Board, &BeamField), b: (&Board, &BeamField)) -> bool {
        let tableau = Tableau::arrange(&easel);
        let palette = Theme::Light.palette();
        let (mut incremental, mut fresh) = (
            Darkroom::new(&tableau, palette),
            Darkroom::new(&tableau, palette),
        );
        incremental.paint_all(&tableau, palette, &Scene::bare(a.0, a.1, Game::C1));
        incremental.light_frame(
            &tableau,
            palette,
            &Scene::bare(b.0, b.1, Game::C1),
            None,
            false,
        );
        fresh.paint_all(&tableau, palette, &Scene::bare(b.0, b.1, Game::C1));
        let (x, y) = (&incremental.wash, &fresh.wash);
        for (name, p, q) in [
            ("sharp", &x.sharp, &y.sharp),
            ("glow", &x.glow, &y.glow),
            ("bloom raw", &x.bloom_raw, &y.bloom_raw),
            ("bloom", &x.bloom, &y.bloom),
            ("halo", &x.half, &y.half),
        ] {
            assert!(p.planes() == q.planes(), "{easel:?}: {name}");
        }
        incremental.lit.as_slice() != [incremental.wash.rect]
    }

    #[test]
    fn patched_washes_equal_whole_ones() {
        let laser = (2, 5, Piece::Laser, 7, 2);
        let boards = [
            vec![laser, (9, 5, Piece::Mirror, 0, 1)],
            vec![laser, (9, 5, Piece::Mirror, 0, 3)],
            vec![
                laser,
                (9, 5, Piece::Splitter, 0, 1),
                (9, 12, Piece::Pinwheel, 7, 0),
            ],
            vec![laser],
            vec![laser, (12, 12, Piece::Mirror, 0, 1)],
            // Corners and edges: a front in the board's corner, light past the board.
            vec![
                (0, 0, Piece::Laser, 4, 3),
                (14, 14, Piece::Mirror, 0, 2),
                laser,
            ],
            vec![(0, 14, Piece::Laser, 2, 1), (14, 0, Piece::Pinwheel, 2, 0)],
        ];
        let parts: Vec<_> = boards.iter().map(|p| lit(p)).collect();
        for easel in [
            Easel::new(800, 600, 1.0),
            Easel::new(780, 1688, 2.0).with_touch(true),
            Easel::new(2560, 1600, 2.0),
            Easel::new(3456, 2234, 2.0), // a wash over 1800 px: scale < 1
        ] {
            let mut by_windows = 0;
            for (i, a) in parts.iter().enumerate() {
                for b in &parts[i + 1..] {
                    by_windows += usize::from(wash_after(easel, (&a.0, &a.1), (&b.0, &b.1)));
                    by_windows += usize::from(wash_after(easel, (&b.0, &b.1), (&a.0, &a.1)));
                }
            }
            assert!(by_windows >= 2, "{easel:?}: some edits develop by windows");
        }
    }

    #[test]
    fn a_patched_frame_recomposes_every_px_its_light_changed() {
        // Board edits as the lightbox paints them: the edited cell's `base`, then the light
        // framed, and the cell's rect and the frame's light rects recomposed over the old picture
        // give the cold picture. A laser on the west edge turned from west (its beam half a cell
        // long) to east: its new sharp light runs two cells to a wall, or the whole row (whole:
        // past `WHOLE_PAST`), far past the turned cell and the old light's halo. And a mirror set
        // down off the beams: its light alone, developed by patches.
        let blue = (6, 2, Piece::Laser, 1, 4);
        let wall = (3, 7, Piece::Wall, 0, 0);
        let laser = |dir| (0, 7, Piece::Laser, 4, dir);
        let mirror = (12, 12, Piece::Mirror, 0, 1);
        let edits = [
            (
                vec![blue, wall, laser(6)],
                vec![blue, wall, laser(2)],
                (0, 7),
                (2, 7),
                None,
            ),
            (
                vec![blue, laser(6)],
                vec![blue, laser(2)],
                (0, 7),
                (14, 7),
                Some(false),
            ),
            (
                vec![blue],
                vec![blue, mirror],
                (12, 12),
                (12, 12),
                Some(true),
            ),
        ];
        for (a, b, edited, far, by_patches) in edits {
            let (a, b) = (lit(&a), lit(&b));
            let edited = board_index(edited.0, edited.1);
            for easel in [
                Easel::new(1280, 800, 1.0),
                Easel::new(3456, 2234, 2.0),
                Easel::new(780, 1480, 2.0).with_touch(true),
            ] {
                let (mut room, tableau, mut out) = painted(easel, &a.0, &a.1);
                let scene = Scene::bare(&b.0, &b.1, Game::C1);
                let mut stale = [false; BOARD_CELLS];
                stale[edited] = true;
                room.paint_cells(&tableau, &Palette::DARK, &scene, &stale);
                let lit = room
                    .light_frame(&tableau, &Palette::DARK, &scene, None, false)
                    .to_vec();
                if let Some(by_patches) = by_patches {
                    let whole = lit == [room.light_rect()];
                    assert_eq!(!whole, by_patches, "{easel:?}: {lit:?}");
                }
                let far = tableau.cell_centre(board_index(far.0, far.1));
                let (fx, fy) = (far.x as i32, far.y as i32);
                assert!(
                    lit.iter().any(|r| (r.left..r.right).contains(&fx)
                        && (r.top..r.bottom).contains(&fy)),
                    "{easel:?}: the new light's far end is damaged: {lit:?}"
                );
                for r in lit.into_iter().chain([tableau.cell_rect(edited)]) {
                    room.compose(r, &mut Output::Xrgb(&mut out));
                }
                let (_, _, fresh) = painted(easel, &b.0, &b.1);
                assert!(
                    out == fresh,
                    "{easel:?}: the patched frame equals a cold paint"
                );
            }
        }
    }

    #[test]
    fn a_scaled_light_tile_still_lights_beams() {
        let (board, field) = lit_board();
        let (room, tableau, out) = painted(Easel::new(3456, 2234, 2.0), &board, &field);
        assert!(
            room.light_scale() < 1.0,
            "a {}-px tile renders its light scaled",
            room.light_rect().right - room.light_rect().left
        );
        let c = tableau.cell_centre(board_index(7, 7));
        assert!(red(out[c.y as usize * 3456 + c.x as usize]) > 200);
    }

    fn area(r: DeviceRect) -> i64 {
        if is_empty(r) {
            0
        } else {
            i64::from(r.right - r.left) * i64::from(r.bottom - r.top)
        }
    }

    #[test]
    fn the_board_paints_off_the_chrome_and_everywhere_else_in_its_region() {
        for scale in [1.0, 2.0] {
            for w in (600..=2200).step_by(10) {
                for h in (800..=3000).step_by(10) {
                    let easel =
                        Easel::new((w as f32 * scale) as u32, (h as f32 * scale) as u32, scale);
                    let regions = Regions::of(&Tableau::arrange(&easel));
                    // Clipped to the board's room, the board region meets neither the tray nor
                    // the picker, so a board paint can't overwrite their chrome.
                    for chrome in [regions.tray, regions.picker] {
                        assert!(
                            clip_rect(regions.board, chrome).is_none(),
                            "{w}×{h} @{scale}: the board region {:?} reaches {chrome:?}",
                            regions.board
                        );
                    }
                    let shared = clip_rect(regions.board, regions.tray).map_or(0, area);
                    let parts = regions.board_parts();
                    for (k, &a) in parts.iter().enumerate() {
                        assert!(
                            clip_rect(a, regions.tray).is_none(),
                            "{w}×{h} @{scale}: {a:?} reaches the tray {:?}",
                            regions.tray
                        );
                        for &b in &parts[k + 1..] {
                            assert!(clip_rect(a, b).is_none(), "{w}×{h} @{scale}: parts overlap");
                        }
                        assert_eq!(clip_rect(a, regions.board), Some(a));
                    }
                    assert_eq!(
                        parts.iter().map(|&p| area(p)).sum::<i64>() + shared,
                        area(regions.board),
                        "{w}×{h} @{scale}: the parts cover the board region off the tray"
                    );
                }
            }
        }
    }

    #[test]
    fn a_board_repaint_leaves_the_tray_alone() {
        // 2480 × 2170 pt @2: the grid's overhang would reach into the tray's panel; the board's
        // room clips it.
        let easel = Easel::new(4960, 4340, 2.0);
        let tableau = Tableau::arrange(&easel);
        let regions = Regions::of(&tableau);
        let overhang = grow(tableau.board, (tableau.cell + 1) / 2);
        assert!(
            clip_rect(overhang, regions.tray).is_some(),
            "an overlapping size"
        );
        let (mut board, _) = lit_board();
        for k in 0..VISIBLE_TOOLBOX_SLOTS {
            board.cells[BOARD_CELLS + k] = Cell {
                piece: Piece::Mirror,
                colour: 0,
                dir: 1,
            };
        }
        let mut field = BeamField::default();
        trace(&board, &mut field);
        let scene = Scene::bare(&board, &field, Game::C1);
        let mut room = Darkroom::new(&tableau, &Palette::DARK);
        room.paint_all(&tableau, &Palette::DARK, &scene);
        let tray = room.tray_damage();
        let rows = |room: &Darkroom| {
            (tray.top..tray.bottom)
                .flat_map(|y| {
                    let at = y as usize * room.width;
                    room.base[at + tray.left as usize..at + tray.right as usize].to_vec()
                })
                .collect::<Vec<u32>>()
        };
        let before = rows(&room);
        room.paint_board(&tableau, &Palette::DARK, &scene);
        assert!(
            rows(&room) == before,
            "a board-only paint keeps the tray's pixels"
        );
    }

    #[test]
    fn a_scaled_tile_composes_rects_like_the_whole() {
        let (board, field) = lit_board();
        let (mut room, _, full) = painted(Easel::new(3456, 2234, 2.0), &board, &field);
        let mut part = vec![0u32; full.len()];
        room.compose(
            DeviceRect::new(700, 900, 1900, 1400),
            &mut Output::Xrgb(&mut part),
        );
        for y in 900..1400 {
            for x in 700..1900 {
                assert_eq!(part[y * 3456 + x], full[y * 3456 + x], "({x}, {y})");
            }
        }
    }

    #[test]
    fn a_repaint_clears_the_last_light() {
        let (board, field) = lit_board();
        let (mut room, tableau, _) = painted(Easel::new(1280, 800, 1.0), &board, &field);
        let (empty, unlit) = (Board::default(), BeamField::default());
        let scene = Scene::bare(&empty, &unlit, Game::C1);
        room.paint_board(&tableau, &Palette::DARK, &scene);
        // Framed incrementally: every item the beam had is wiped where it was.
        room.light_frame(&tableau, &Palette::DARK, &scene, None, false);
        let mut after = vec![0u32; 1280 * 800];
        room.compose(tableau.canvas(), &mut Output::Xrgb(&mut after));
        let (_, _, fresh) = painted(Easel::new(1280, 800, 1.0), &empty, &unlit);
        assert!(after == fresh, "no trace of the old beam");
    }

    #[test]
    fn a_degenerate_canvas_has_empty_regions_and_paints() {
        let (board, field) = lit_board();
        let (room, _, _) = painted(Easel::new(5, 3000, 3.0), &board, &field);
        let _ = room.tray_damage();
        let (room, _, _) = painted(Easel::new(1, 1, 1.0), &board, &field);
        assert!(room.buffer_bytes() > 0);
    }

    #[test]
    fn a_full_sprite_pool_starts_over_without_losing_pieces() {
        // 72 distinct bodies (32 lasers, 8 each of five oriented kinds): more than the 48-slot pool.
        let mut board = Board::default();
        let mut cells = board.cells[..BOARD_CELLS].iter_mut();
        for colour in [1, 2, 4, 7] {
            for dir in 0..8 {
                *cells.next().unwrap() = Cell {
                    piece: Piece::Laser,
                    colour,
                    dir,
                };
            }
        }
        for piece in [
            Piece::Mirror,
            Piece::Bender,
            Piece::Prism,
            Piece::Doppler,
            Piece::Tangler,
        ] {
            for dir in 0..8 {
                *cells.next().unwrap() = Cell {
                    piece,
                    colour: 0,
                    dir,
                };
            }
        }
        let mut field = BeamField::default();
        trace(&board, &mut field);
        let (_, _, first) = painted(Easel::new(1280, 800, 1.0), &board, &field);
        let (mut room, tableau, _) = painted(Easel::new(1280, 800, 1.0), &board, &field);
        room.paint_board(
            &tableau,
            &Palette::DARK,
            &Scene::bare(&board, &field, Game::C1),
        );
        let mut again = vec![0u32; first.len()];
        room.compose(tableau.canvas(), &mut Output::Xrgb(&mut again));
        assert!(
            again == first,
            "refilling the pool mid-paint changes nothing visible"
        );
    }

    #[test]
    fn the_tray_has_a_panel_and_a_well_under_every_slot() {
        let easel = Easel::new(1280, 800, 1.0);
        let (board, field) = lit_board(); // an empty tray
        let (_, tableau, out) = painted(easel, &board, &field);
        let mut bare = vec![0u32; 1280 * 800];
        let mut backdrop_only = Darkroom::new(&tableau, &Palette::DARK);
        backdrop_only.backdrop.fill(
            &mut Layer {
                px: &mut bare,
                width: 1280,
                height: 800,
            },
            tableau.canvas(),
        );
        let at = |px: &[u32], x: i32, y: i32| px[y as usize * 1280 + x as usize];
        // Inside the column-wide panel, right of the 6-slot grid, far from the board's light.
        let panel = tableau.device(tableau.tray.rect);
        let (x, y) = (panel.right - 10, panel.top + 30);
        let mut tinted = [at(&bare, x, y)];
        vitrine::tint(
            &mut Layer {
                px: &mut tinted,
                width: 1,
                height: 1,
            },
            DeviceRect::new(0, 0, 1, 1),
            Palette::DARK.panel_fill,
        );
        assert_eq!(
            at(&out, x, y),
            tinted[0],
            "the panel's interior is the backdrop, tinted"
        );
        let blue = |p: u32| p & 0xff;
        for slot in [0, 11, 23] {
            let r = tableau.cell_rect(lumen::toolbox_index(slot));
            let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
            assert!(
                blue(at(&out, cx, cy)) < blue(at(&out, x, y)),
                "slot {slot}: its well is darker than the panel"
            );
        }
    }

    #[test]
    fn a_picker_repaint_stays_in_its_region() {
        let easel = Easel::new(1280, 800, 1.0);
        let tableau = Tableau::arrange(&easel);
        let (board, field) = lit_board();
        let mut room = Darkroom::new(&tableau, &Palette::DARK);
        let mut scene = Scene::bare(&board, &field, Game::C1);
        room.paint_all(&tableau, &Palette::DARK, &scene);
        let before = room.base.clone();
        scene.strip[1] = SlotState::Unlocked;
        scene.strip[7] = SlotState::Completed;
        room.paint_picker(&tableau, &Palette::DARK, &scene);
        let region = room.picker_damage();
        let mut changed = false;
        for (i, (a, b)) in before.iter().zip(&room.base).enumerate() {
            let (x, y) = ((i % 1280) as i32, (i / 1280) as i32);
            let inside = (region.left..region.right).contains(&x)
                && (region.top..region.bottom).contains(&y);
            assert!(inside || a == b, "({x}, {y}) changed outside the picker");
            changed |= a != b;
        }
        assert!(changed, "the two chips changed");
    }

    #[test]
    fn repainting_the_board_alone_matches_a_full_paint() {
        let (board, field) = lit_board();
        let (mut room, tableau, full) = painted(Easel::new(1280, 800, 1.0), &board, &field);
        let scene = Scene::bare(&board, &field, Game::C1);
        room.paint_board(&tableau, &Palette::DARK, &scene);
        room.light_frame(&tableau, &Palette::DARK, &scene, None, true);
        let mut again = vec![0u32; full.len()];
        room.compose(tableau.canvas(), &mut Output::Xrgb(&mut again));
        assert!(again == full, "board repaint is deterministic");
        let damage = room.board_damage();
        assert!(
            damage.left <= tableau.board.left - tableau.cell
                && damage.right >= tableau.board.right + tableau.cell,
            "the damage holds the bloom margin"
        );
    }
}
