//! `tableau`: where everything sits (spec §6), in pt from the safe area's top-left, plus the
//! device-px board and tray that hit testing and drawing use: Beside or Stacked, each from its
//! full form through its fit steps until the card holds `card_min`.

use lumen::layout::STRIP_SLOTS;
use lumen::{BOARD_CELLS, Floorplan, Hit, VISIBLE_TOOLBOX_SLOTS, board_index, toolbox_index};

use crate::etch::{Pt, pt};
use crate::{DeviceRect, Easel};

/// The longest hint a card is sized for (C1's longest; `tests/hints.rs` checks every game).
pub const LONGEST_HINT: f32 = 245.0;
/// The most tray columns (Stacked on a wide canvas) and rows (Beside).
pub const MAX_TRAY_COLS: usize = 12;
pub const MAX_TRAY_ROWS: usize = 4;
const BRAND_H: f32 = 44.0;
const CREDITS_H: f32 = 18.0;
const MIN_COLUMN: f32 = 280.0;
const MAX_COLUMN: f32 = 420.0;
/// Beside needs a board of at least this many pt (D3); Stacked shrinks its board to this, no more.
const MIN_BESIDE: f32 = 330.0;
const MIN_STACKED: f32 = 300.0;
/// A short touch landscape (a phone with its browser's bars, spec §6 "Revision: short
/// landscape") keeps Beside with a smaller board and tray: the board down to this many pt...
pub const MIN_BESIDE_TOUCH: f32 = 240.0;
/// ...and the tray's slots down to this many pt, 6×4 or 8×3 (step 7).
pub const MIN_TOUCH_SLOT: f32 = 28.0;

/// The card height its text needs at column width `col_w` (spec §6, "Text demand").
pub fn card_min(col_w: f32, compact: bool) -> f32 {
    let cpl = ((col_w - 24.0) / 5.5).floor().max(1.0);
    24.0 + 28.0 + if compact { 0.0 } else { 22.0 } + ((LONGEST_HINT / cpl).ceil() + 1.0) * 14.3
}

/// Backing px per CSS px for the web canvas (D2): `min(ratio, 2, 1600 / board side)`, floored to
/// a multiple of 0.25, at least 1. `width_pt × height_pt` is the canvas less its safe area.
pub fn render_scale(width_pt: f32, height_pt: f32, ratio: f32, touch: bool) -> f32 {
    let ratio = if ratio.is_finite() && ratio > 0.0 {
        ratio
    } else {
        1.0
    };
    let easel = Easel::new(whole(width_pt), whole(height_pt), 1.0).with_touch(touch);
    let side = Tableau::arrange(&easel).board_slot.w.max(1.0);
    ((ratio.min(2.0).min(1600.0 / side) * 4.0).floor() / 4.0).max(1.0)
}

/// How far the page's whole backing px (`round(css × scale)`, any scale ≥ 1) and the browser's
/// layout snapping may move the width and height the tableau sees, in pt.
const SNAP: f32 = 0.5;

/// The canvas height (pt) the page should give a `width_pt` wide canvas with `room_pt` of room:
/// the room itself when the arrangement there fits its card, else Stacked's minimum height
/// (whole pt), so a too-short window scrolls instead of squeezing the card (spec §6 "Minimum
/// height"). The returned `h ≥ room` holds its card at every width in `width_pt ± ½` and height in
/// `h ± ½`, so it survives the page's rounding to whole backing px: `room` only when Beside holds
/// over that whole box (and stays Beside, `W ≥ 1.2·H`) or the room clears Stacked's minimum for
/// every width in it.
pub fn stage_height(width_pt: f32, room_pt: f32, touch: bool) -> f32 {
    let sane = |v: f32| if v.is_finite() { v.max(1.0) } else { 1.0 };
    let (w, room) = (sane(width_pt), sane(room_pt));
    let (lo, hi) = (w - SNAP, w + SNAP);
    let beside = lo >= 1.2 * (room + SNAP)
        && [lo, hi].into_iter().all(|x| {
            [room - SNAP, room + SNAP]
                .into_iter()
                .all(|y| beside(x, y, touch).is_some())
        });
    let stacked = min_height_within(lo, hi) + SNAP;
    if beside || room >= stacked {
        room
    } else {
        stacked.ceil()
    }
}

/// The most `Tableau::min_height` asks for any width in `[lo, hi]`: its board and tray terms grow
/// with the width and its card term (`card_min`) shrinks, so within each of Stacked's spacing
/// regimes (below and from 600 pt) the bound takes the first at `hi` and the second at `lo`.
fn min_height_within(lo: f32, hi: f32) -> f32 {
    let bound = |a: f32, b: f32| {
        let (m, g, cols, rows) = stacked_metrics(a);
        let band = |x: f32| (x - 2.0 * m).max(0.0);
        let slot = (band(b) / cols as f32).min(44.0);
        2.0 * m + MIN_STACKED.min(band(b)) + 2.0 * g + rows as f32 * slot + card_min(band(a), true)
    };
    let mut h = 0.0f32;
    if lo < 600.0 {
        h = h.max(bound(lo, hi.min(600.0)));
    }
    if hi >= 600.0 {
        h = h.max(bound(lo.max(600.0), hi));
    }
    h
}

/// A pt length as whole px at scale 1 (at least 1; 1 for NaN).
fn whole(v: f32) -> u32 {
    if v.is_finite() {
        v.max(1.0).round() as u32
    } else {
        1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrangement {
    Beside,
    Stacked,
}

/// The fit steps an arrangement applied (spec §6), as a bit set: step `k` is bit `k − 1`. Only
/// steps that changed something are recorded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Steps(pub u8);

impl Steps {
    pub fn has(self, step: u8) -> bool {
        (1..=8).contains(&step) && self.0 & 1 << (step - 1) != 0
    }

    pub fn list(self) -> impl Iterator<Item = u8> {
        (1..=8).filter(move |&k| self.has(k))
    }

    fn add(&mut self, step: u8) {
        self.0 |= 1 << (step - 1);
    }
}

/// An arrangement's choices before placement.
#[derive(Clone, Copy, Debug)]
struct Fit {
    arrangement: Arrangement,
    margin: f32,
    gap: f32,
    side: f32,
    slot: f32,
    cols: usize,
    rows: usize,
    brand: bool,
    inline: bool,
    compact: bool,
    steps: Steps,
}

impl Fit {
    /// Beside's column right of the board on a `w` pt wide canvas.
    fn column(&self, w: f32) -> f32 {
        (w - self.side - 3.0 * self.margin).clamp(MIN_COLUMN, MAX_COLUMN)
    }

    /// Shrinks Beside's board 1 pt at a time, down to `floor`, until the card fits: only while the
    /// column is under its cap, since the board's size changes the card only through the column's
    /// width (`card_min`). Records `step` when it shrank.
    fn shrink_board(&mut self, w: f32, h: f32, floor: f32, step: u8) {
        while !self.fits(w, h) && self.side > floor && self.column(w) < MAX_COLUMN {
            self.side = (self.side - 1.0).max(floor);
            self.steps.add(step);
        }
    }

    /// Shrinks the tray's slots 1 pt at a time, down to `MIN_TOUCH_SLOT`, until the card fits.
    fn shrink_slots(&mut self, w: f32, h: f32) {
        while !self.fits(w, h) && self.slot > MIN_TOUCH_SLOT {
            self.slot = (self.slot - 1.0).max(MIN_TOUCH_SLOT);
        }
    }

    /// The card's width (Beside's column, Stacked's band) and height on a `w × h` pt canvas.
    fn card(&self, w: f32, h: f32) -> (f32, f32) {
        let (m, g) = (self.margin, self.gap);
        match self.arrangement {
            Arrangement::Beside => {
                let col = self.column(w);
                let (mut parts, mut gaps) = (self.rows as f32 * self.slot, 1.0);
                if self.brand {
                    parts += BRAND_H;
                    gaps += 1.0;
                }
                if self.inline {
                    parts += 5.0 * col / 10.0;
                    gaps += 1.0;
                }
                if !self.compact {
                    parts += CREDITS_H;
                    gaps += 1.0;
                }
                (col, h - 2.0 * m - parts - g * gaps)
            }
            Arrangement::Stacked => {
                let band = (w - 2.0 * m).max(0.0);
                let credits = if self.compact { 0.0 } else { g + CREDITS_H };
                let card = h - 2.0 * m - self.side - 2.0 * g - self.rows as f32 * self.slot;
                (band, card - credits)
            }
        }
    }

    fn fits(&self, w: f32, h: f32) -> bool {
        let (width, card) = self.card(w, h);
        card >= card_min(width, self.compact)
    }
}

/// Beside, through its fit steps; `None` when it cannot hold its card (then Stacked). A touch
/// canvas has two more steps for a short landscape: 7, the tray's slots down to
/// `MIN_TOUCH_SLOT`, as 6×4 or as 8×3 when that keeps them bigger; 8, the board below 330 down
/// to `MIN_BESIDE_TOUCH` (and may start there).
fn beside(w: f32, h: f32, touch: bool) -> Option<Fit> {
    if w < 1.2 * h {
        return None;
    }
    let (margin, gap) = if h >= 600.0 {
        (24.0, 16.0)
    } else {
        (12.0, 8.0)
    };
    let side = (h - 2.0 * margin).min(w - 3.0 * margin - MIN_COLUMN);
    let least = if touch { MIN_BESIDE_TOUCH } else { MIN_BESIDE };
    if side < least {
        return None;
    }
    let col = (w - side - 3.0 * margin).clamp(MIN_COLUMN, MAX_COLUMN);
    let mut f = Fit {
        arrangement: Arrangement::Beside,
        margin,
        gap,
        side,
        slot: (col / 6.0).min(60.0),
        cols: 6,
        rows: 4,
        brand: h >= 700.0,
        inline: col / 10.0 >= 32.0,
        compact: false,
        steps: Steps::default(),
    };
    if !f.fits(w, h) && f.slot > 48.0 {
        f.slot = 48.0;
        f.steps.add(1);
    }
    if !f.fits(w, h) && f.brand {
        f.brand = false;
        f.steps.add(2);
    }
    if !f.fits(w, h) && f.inline {
        f.inline = false;
        f.steps.add(3);
    }
    if !f.fits(w, h) {
        f.compact = true;
        f.steps.add(4);
    }
    let floor = if touch { 44.0 } else { 36.0 };
    if !f.fits(w, h) && f.slot > floor {
        f.slot = floor;
        f.steps.add(5);
    }
    f.shrink_board(w, h, MIN_BESIDE, 6);
    if touch && !f.fits(w, h) {
        // Step 7: the 6×4 tray's slots shrink, or the tray becomes 8×3 (at most 44 pt, the
        // column's eighth) when that keeps slots as big, filling the column's width and leaving
        // the card more room; neither fitting, the shorter 8×3 goes on.
        let mut six = f;
        six.shrink_slots(w, h);
        let mut eight = Fit {
            cols: 8,
            rows: 3,
            slot: (f.column(w) / 8.0).min(44.0),
            ..f
        };
        eight.shrink_slots(w, h);
        f = if six.fits(w, h) && !(eight.fits(w, h) && eight.slot >= six.slot) {
            six
        } else {
            eight
        };
        f.steps.add(7);
        f.shrink_board(w, h, MIN_BESIDE_TOUCH, 8);
    }
    f.fits(w, h).then_some(f)
}

/// Stacked's spacing and tray shape for a `w` pt wide canvas: (m, g, cols, rows).
fn stacked_metrics(w: f32) -> (f32, f32, usize, usize) {
    if w < 600.0 {
        (12.0, 8.0, 8, 3)
    } else {
        (16.0, 16.0, 12, 2)
    }
}

/// Stacked, through its fit steps (it may still not fit on a very short canvas).
fn stacked(w: f32, h: f32) -> Fit {
    let (margin, gap, cols, rows) = stacked_metrics(w);
    let band = (w - 2.0 * margin).max(0.0);
    let mut f = Fit {
        arrangement: Arrangement::Stacked,
        margin,
        gap,
        side: band.min(0.6 * h),
        slot: (band / cols as f32).min(56.0),
        cols,
        rows,
        brand: false,
        inline: false,
        compact: false,
        steps: Steps::default(),
    };
    if !f.fits(w, h) {
        f.compact = true;
        f.steps.add(1);
    }
    if !f.fits(w, h) && f.slot > 44.0 {
        f.slot = 44.0;
        f.steps.add(2);
    }
    while !f.fits(w, h) && f.side > MIN_STACKED {
        f.side = (f.side - 1.0).max(MIN_STACKED);
        f.steps.add(3);
    }
    f
}

/// The arrangement for a `w × h` pt room: Beside when it holds its card, else Stacked.
fn fit_at(w: f32, h: f32, touch: bool) -> Fit {
    beside(w, h, touch).unwrap_or_else(|| stacked(w, h))
}

/// A rect in pt, from the safe area's top-left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PtRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tray {
    /// The tray's panel: the column's width in Beside, the grid itself in Stacked.
    pub rect: PtRect,
    /// `cols × rows` slots of `slot` pt.
    pub grid: PtRect,
    pub cols: usize,
    pub rows: usize,
    pub slot: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Picker {
    pub rect: PtRect,
    pub pitch: f32,
}

/// Card sizes (pt, spec §7.5 and ruling 5): the Levels pill, Next ▶, the "(won)" badge's slot.
pub const LEVELS_PILL: (f32, f32) = (80.0, 28.0);
pub const NEXT_PILL: (f32, f32) = (96.0, 34.0);
pub const BADGE_W: f32 = 44.0;
pub const CARD_PAD: f32 = 12.0;
pub const HEADER_H: f32 = 28.0;
pub const NAME_H: f32 = 22.0;
/// The card header's lettering caps (pt): "LEVEL n", and a won card's win text.
pub const LEVEL_CAP: f32 = 13.0;
pub const WIN_CAP: f32 = 18.0;
/// The level sheet's "LEVELS" cap (pt, spec §7.5).
pub const SHEET_CAP: f32 = 14.0;
/// The header lettering's tracking (caps between letters).
pub const HEADER_TRACK: f32 = 0.3;
const SHEET_PAD: f32 = 16.0;
const SHEET_HEADER: f32 = 44.0;
const CLOSE: f32 = 28.0;

/// The canvas's own buttons (spec §6 "Hit testing": tested before the floorplan).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// The card's Levels pill: opens the sheet.
    Levels,
    /// The sheet's close chip, or the scrim around it.
    CloseSheet,
    /// Next ▶ on a won card.
    Next,
}

/// The card's parts in pt (spec §7.5 "Card").
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CardParts {
    /// The header row inside the padding; the lettering starts at its left.
    pub header: PtRect,
    /// The Levels pill at the header's right, when the picker is a sheet.
    pub levels: Option<PtRect>,
    /// The "(won)" badge's slot, right-aligned just left of the pill (or at the header's right).
    pub badge: PtRect,
    /// The level name's row; `None` on a compact card (the name joins the header row).
    pub name: Option<PtRect>,
    /// The hint's box.
    pub body: PtRect,
    /// The hint's box when the level has no name (C1's): the name row joins it, so the hint
    /// starts right under the header (equal to `body` on a compact card).
    pub nameless_body: PtRect,
    /// The click-next box once won: from the header down to 8 pt above Next ▶.
    pub won_body: PtRect,
    /// Next ▶ at the bottom right (shown only when won and the next level is open).
    pub next: PtRect,
}

/// The level sheet (spec §6 "Picker", §7.5 "Sheet"), centred on the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sheet {
    pub panel: PtRect,
    /// Where "LEVELS" is lettered: the header row's left part.
    pub title: PtRect,
    /// The close chip's square (a 28-pt circle).
    pub close: PtRect,
    pub grid: PtRect,
    pub cols: usize,
    pub rows: usize,
    pub pitch: f32,
    pub chip: f32,
}

fn card_parts(card: PtRect, inline: bool, compact: bool) -> CardParts {
    let (x, w) = (card.x + CARD_PAD, (card.w - 2.0 * CARD_PAD).max(0.0));
    let bottom = card.y + card.h - CARD_PAD;
    let header = PtRect {
        x,
        y: card.y + CARD_PAD,
        w,
        h: HEADER_H,
    };
    let levels = (!inline).then_some(PtRect {
        x: x + w - LEVELS_PILL.0,
        y: header.y,
        w: LEVELS_PILL.0,
        h: LEVELS_PILL.1,
    });
    let right = levels.map_or(x + w, |p| p.x - 10.0);
    let below = header.y + HEADER_H;
    let name = (!compact).then_some(PtRect {
        x,
        y: below,
        w,
        h: NAME_H,
    });
    let body_y = if compact { below } else { below + NAME_H };
    let next = PtRect {
        x: x + w - NEXT_PILL.0,
        y: bottom - NEXT_PILL.1,
        w: NEXT_PILL.0,
        h: NEXT_PILL.1,
    };
    CardParts {
        header,
        levels,
        badge: PtRect {
            x: right - BADGE_W,
            y: header.y,
            w: BADGE_W,
            h: HEADER_H,
        },
        name,
        body: PtRect {
            x,
            y: body_y,
            w,
            h: (bottom - body_y).max(0.0),
        },
        nameless_body: PtRect {
            x,
            y: below,
            w,
            h: (bottom - below).max(0.0),
        },
        won_body: PtRect {
            x,
            y: below,
            w,
            h: (next.y - 8.0 - below).max(0.0),
        },
        next,
    }
}

/// The sheet for a `w × h` pt room with margin `m`: 10 × 5 when wide, else 5 × 10; pitch
/// `min(56, (W − 2m − 32)/cols, (H − 2m − 76)/rows)`, chip = pitch − 8, at least 44 on touch.
fn sheet(w: f32, h: f32, m: f32, touch: bool) -> Sheet {
    let (cols, rows) = if w >= h { (10, 5) } else { (5, 10) };
    let mut pitch = 56f32
        .min((w - 2.0 * m - 2.0 * SHEET_PAD) / cols as f32)
        .min((h - 2.0 * m - 2.0 * SHEET_PAD - SHEET_HEADER) / rows as f32)
        .max(1.0);
    let mut chip = (pitch - 8.0).max(1.0);
    if touch && chip < 44.0 {
        chip = 44.0;
        pitch = pitch.max(48.0);
    }
    // Ruling 17: a canvas too short for a 44-pt grid gets the largest pitch that fits.
    let room = ((w - 2.0 * m - 2.0 * SHEET_PAD) / cols as f32)
        .min((h - 2.0 * m - 2.0 * SHEET_PAD - SHEET_HEADER) / rows as f32)
        .max(1.0);
    if pitch > room {
        pitch = room;
        chip = (room - 4.0).max(1.0);
    }
    let (gw, gh) = (cols as f32 * pitch, rows as f32 * pitch);
    let (pw, ph) = (gw + 2.0 * SHEET_PAD, gh + 2.0 * SHEET_PAD + SHEET_HEADER);
    let panel = PtRect {
        x: (w - pw) / 2.0,
        y: (h - ph) / 2.0,
        w: pw,
        h: ph,
    };
    let top = panel.y + SHEET_PAD;
    Sheet {
        panel,
        title: PtRect {
            x: panel.x + SHEET_PAD,
            y: top,
            w: (gw - CLOSE - 8.0).max(0.0),
            h: SHEET_HEADER,
        },
        close: PtRect {
            x: panel.x + pw - SHEET_PAD - CLOSE,
            y: top + (SHEET_HEADER - CLOSE) / 2.0,
            w: CLOSE,
            h: CLOSE,
        },
        grid: PtRect {
            x: panel.x + SHEET_PAD,
            y: top + SHEET_HEADER,
            w: gw,
            h: gh,
        },
        cols,
        rows,
        pitch,
        chip,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tableau {
    pub arrangement: Arrangement,
    pub steps: Steps,
    pub margin: f32,
    pub gap: f32,
    /// The board's slot: 15 cells plus the panel margin `pad` on every side.
    pub board_slot: PtRect,
    pub pad: f32,
    /// The 15 × 15 board in device px, whole cells, centred in its slot.
    pub board: DeviceRect,
    /// Device px per board cell (`u`).
    pub cell: i32,
    /// Beside: the column right of the board. Stacked: the band under the board (x = m).
    pub column: PtRect,
    pub brand: Option<PtRect>,
    pub card: PtRect,
    /// The card puts the level name on its header row, and the credits leave the canvas.
    pub compact: bool,
    /// The card holds `card_min` (false only on a canvas below `min_height`).
    pub fits: bool,
    pub tray: Tray,
    /// The inline picker, when Beside's column is wide enough (pitch ≥ 32) and kept it.
    pub picker: Option<Picker>,
    /// The credits line; `None` on a compact card.
    pub credits: Option<PtRect>,
    pub card_parts: CardParts,
    pub sheet: Sheet,
    easel: Easel,
    /// Device-px slot edges: x for `cols + 1` columns, y for `rows + 1` rows.
    tray_x: [i32; MAX_TRAY_COLS + 1],
    tray_y: [i32; MAX_TRAY_ROWS + 1],
}

impl Tableau {
    pub fn arrange(easel: &Easel) -> Tableau {
        let (w, h) = easel.room();
        Tableau::place(easel, w, h, fit_at(w, h, easel.touch))
    }

    /// The canvas height (pt, safe area excluded) at which Stacked just holds its compact card
    /// with a 300-pt board and 44-pt slots: `2m + 300 + g + rows·44 + g + card_min` (spec §6).
    pub fn min_height(width_pt: f32) -> f32 {
        let w = if width_pt.is_finite() {
            width_pt.max(0.0)
        } else {
            0.0
        };
        let (m, g, cols, rows) = stacked_metrics(w);
        let band = (w - 2.0 * m).max(0.0);
        let slot = (band / cols as f32).min(44.0);
        2.0 * m + MIN_STACKED.min(band) + 2.0 * g + rows as f32 * slot + card_min(band, true)
    }

    fn place(easel: &Easel, w: f32, h: f32, fit: Fit) -> Tableau {
        let ppp = easel.px_per_pt;
        let fits = fit.fits(w, h);
        // Room for at least 15 device px, so the board always has a 1 px cell.
        let fit = Fit {
            side: fit.side.max(16.0 / ppp),
            ..fit
        };
        // A degenerate canvas (a 0-pt band) still gets a 1-px slot.
        let (m, g, side, slot) = (fit.margin, fit.gap, fit.side, fit.slot.max(1.0 / ppp));
        let (band, card_h) = fit.card(w, h);
        let card_h = card_h.max(0.0);
        let (cols, rows) = (fit.cols, fit.rows);
        let (grid_w, grid_h) = (cols as f32 * slot, rows as f32 * slot);
        let (board_slot, column, brand, card, tray, picker, credits);
        match fit.arrangement {
            Arrangement::Beside => {
                let extra = (w - side - band - 3.0 * m).max(0.0) / 2.0;
                board_slot = PtRect {
                    x: extra + m,
                    y: m + ((h - 2.0 * m - side) / 2.0).max(0.0),
                    w: side,
                    h: side,
                };
                let x = board_slot.x + side + m;
                column = PtRect {
                    x,
                    y: m,
                    w: band,
                    h: (h - 2.0 * m).max(0.0),
                };
                let mut y = m;
                brand = fit.brand.then(|| {
                    let r = PtRect {
                        x,
                        y,
                        w: band,
                        h: BRAND_H,
                    };
                    y += BRAND_H + g;
                    r
                });
                card = PtRect {
                    x,
                    y,
                    w: band,
                    h: card_h,
                };
                y += card_h + g;
                tray = Tray {
                    rect: PtRect {
                        x,
                        y,
                        w: band,
                        h: grid_h,
                    },
                    grid: PtRect {
                        x: x + (band - grid_w) / 2.0,
                        y,
                        w: grid_w,
                        h: grid_h,
                    },
                    cols,
                    rows,
                    slot,
                };
                y += grid_h + g;
                picker = fit.inline.then(|| {
                    let pitch = band / 10.0;
                    let r = Picker {
                        rect: PtRect {
                            x,
                            y,
                            w: band,
                            h: 5.0 * pitch,
                        },
                        pitch,
                    };
                    y += 5.0 * pitch + g;
                    r
                });
                credits = (!fit.compact).then_some(PtRect {
                    x,
                    y,
                    w: band,
                    h: CREDITS_H,
                });
            }
            Arrangement::Stacked => {
                board_slot = PtRect {
                    x: ((w - side) / 2.0).max(0.0),
                    y: m,
                    w: side,
                    h: side,
                };
                let mut y = m + side + g;
                column = PtRect {
                    x: m,
                    y,
                    w: band,
                    h: (h - m - y).max(0.0),
                };
                brand = None;
                let grid = PtRect {
                    x: ((w - grid_w) / 2.0).max(0.0),
                    y,
                    w: grid_w,
                    h: grid_h,
                };
                tray = Tray {
                    rect: grid,
                    grid,
                    cols,
                    rows,
                    slot,
                };
                y += grid_h + g;
                card = PtRect {
                    x: m,
                    y,
                    w: band,
                    h: card_h,
                };
                y += card_h + g;
                picker = None;
                credits = (!fit.compact).then_some(PtRect {
                    x: m,
                    y,
                    w: band,
                    h: CREDITS_H,
                });
            }
        }
        let pad = (0.02 * side).min(m - 6.0).max(0.0);
        let cell = (((side - 2.0 * pad) * ppp / 15.0).floor() as i32).max(1);
        let left =
            (easel.px_x(board_slot.x) + (side * ppp - 15.0 * cell as f32) / 2.0).round() as i32;
        let top =
            (easel.px_y(board_slot.y) + (side * ppp - 15.0 * cell as f32) / 2.0).round() as i32;
        let mut tray_x = [0; MAX_TRAY_COLS + 1];
        let mut tray_y = [0; MAX_TRAY_ROWS + 1];
        for (k, x) in tray_x.iter_mut().enumerate().take(cols + 1) {
            *x = easel.px_x(tray.grid.x + k as f32 * slot).round() as i32;
        }
        for (k, y) in tray_y.iter_mut().enumerate().take(rows + 1) {
            *y = easel.px_y(tray.grid.y + k as f32 * slot).round() as i32;
        }
        Tableau {
            arrangement: fit.arrangement,
            steps: fit.steps,
            margin: m,
            gap: g,
            board_slot,
            pad,
            board: DeviceRect::new(left, top, left + 15 * cell, top + 15 * cell),
            cell,
            column,
            brand,
            card,
            compact: fit.compact,
            fits,
            tray,
            picker,
            credits,
            card_parts: card_parts(card, picker.is_some(), fit.compact),
            sheet: sheet(w, h, m, easel.touch),
            easel: *easel,
            tray_x,
            tray_y,
        }
    }

    /// Where a board paint may reach: the board's slot grown by half the margin, in device px.
    /// The grid's overhang (0.35u past the board) is clipped here on very large canvases.
    pub fn board_room(&self) -> DeviceRect {
        let half = self.margin / 2.0;
        let s = self.board_slot;
        self.device(PtRect {
            x: s.x - half,
            y: s.y - half,
            w: s.w + 2.0 * half,
            h: s.h + 2.0 * half,
        })
    }

    /// The light board's panel (spec §6 `pad`, §7.2): the board grown by `pad`, in whole
    /// device px.
    pub fn board_panel(&self) -> DeviceRect {
        let pad = (self.pad * self.px_per_pt()).round() as i32;
        let b = self.board;
        DeviceRect::new(b.left - pad, b.top - pad, b.right + pad, b.bottom + pad)
    }

    pub fn px_per_pt(&self) -> f32 {
        self.easel.px_per_pt
    }

    pub fn canvas(&self) -> DeviceRect {
        DeviceRect::new(0, 0, self.easel.width as i32, self.easel.height as i32)
    }

    /// Device x of pt `pt` (not rounded), measured like every rect from the safe area.
    pub fn easel_px_x(&self, pt: f32) -> f32 {
        self.easel.px_x(pt)
    }

    /// Device y of pt `pt` (not rounded), measured like every rect from the safe area.
    pub fn easel_px_y(&self, pt: f32) -> f32 {
        self.easel.px_y(pt)
    }

    /// Where the card header's lettering at cap `cap_pt` starts (its cap line's left, device px):
    /// whole px from the card's rounded top-left corner, so its tile's key holds across a canvas
    /// a px wider.
    pub fn header_lettering_at(&self, cap_pt: f32) -> (i32, i32) {
        let c = self.device(self.card);
        let whole = |p: f32| (p * self.px_per_pt()).round() as i32;
        (
            c.left + whole(CARD_PAD + 2.0),
            c.top + whole(CARD_PAD + (HEADER_H - cap_pt) / 2.0),
        )
    }

    /// Where the sheet's "LEVELS" at cap `cap_pt` starts (its cap line's left, device px): 2 pt
    /// into its title row, centred on the row, whole px from the panel's rounded top-left
    /// corner, so its tile's key holds across a canvas a px wider.
    pub fn sheet_lettering_at(&self, cap_pt: f32) -> (i32, i32) {
        let p = self.device(self.sheet.panel);
        let whole = |pt: f32| (pt * self.px_per_pt()).round() as i32;
        (
            p.left + whole(SHEET_PAD + 2.0),
            p.top + whole(SHEET_PAD + (SHEET_HEADER - cap_pt) / 2.0),
        )
    }

    /// Next ▶'s pill as drawn: `[left, top, right, bottom]` (device px, unrounded) from the
    /// card's rounded bottom-right corner, so it can sit ≤ 1 px off `card_parts.next`.
    pub fn next_pill(&self) -> [f32; 4] {
        let c = self.device(self.card);
        let px = |p: f32| p * self.px_per_pt();
        let (r, b) = (c.right as f32, c.bottom as f32);
        [
            r - px(CARD_PAD + NEXT_PILL.0),
            b - px(CARD_PAD + NEXT_PILL.1),
            r - px(CARD_PAD),
            b - px(CARD_PAD),
        ]
    }

    /// A pt rect in device px (edges rounded).
    pub fn device(&self, r: PtRect) -> DeviceRect {
        let e = &self.easel;
        DeviceRect::new(
            e.px_x(r.x).round() as i32,
            e.px_y(r.y).round() as i32,
            e.px_x(r.x + r.w).round() as i32,
            e.px_y(r.y + r.h).round() as i32,
        )
    }

    /// A board cell's square, or a visible tray slot's pitch cell; empty for any other index.
    pub fn cell_rect(&self, index: usize) -> DeviceRect {
        if index < BOARD_CELLS {
            let (x, y) = ((index % 15) as i32, (index / 15) as i32);
            let (l, t) = (
                self.board.left + x * self.cell,
                self.board.top + y * self.cell,
            );
            return DeviceRect::new(l, t, l + self.cell, t + self.cell);
        }
        let slot = index - BOARD_CELLS;
        if slot >= VISIBLE_TOOLBOX_SLOTS {
            return DeviceRect::default();
        }
        let (c, r) = (slot % self.tray.cols, slot / self.tray.cols);
        DeviceRect::new(
            self.tray_x[c],
            self.tray_y[r],
            self.tray_x[c + 1],
            self.tray_y[r + 1],
        )
    }

    pub fn cell_centre(&self, index: usize) -> Pt {
        let r = self.cell_rect(index);
        pt(
            (r.left + r.right) as f32 / 2.0,
            (r.top + r.bottom) as f32 / 2.0,
        )
    }

    /// A tray piece's size, device px: 0.9 × the slot.
    pub fn tray_piece_px(&self) -> i32 {
        ((0.9 * self.tray.slot * self.easel.px_per_pt).round() as i32).max(1)
    }

    /// The pitch cell of a `cols × rows` grid at `grid` (device px) under (`x`, `y`), as a slot.
    fn grid_slot(&self, grid: PtRect, cols: usize, rows: usize, x: i32, y: i32) -> Option<usize> {
        let r = self.device(grid);
        if !((r.left..r.right).contains(&x) && (r.top..r.bottom).contains(&y)) {
            return None;
        }
        let c = ((x - r.left) as usize * cols / (r.right - r.left).max(1) as usize).min(cols - 1);
        let row = ((y - r.top) as usize * rows / (r.bottom - r.top).max(1) as usize).min(rows - 1);
        Some(row * cols + c).filter(|&s| s < STRIP_SLOTS)
    }

    /// The `size`-pt chip square centred in pitch cell `slot` of a grid.
    fn grid_chip(
        &self,
        grid: PtRect,
        cols: usize,
        pitch: f32,
        size: f32,
        slot: usize,
    ) -> DeviceRect {
        if slot >= STRIP_SLOTS {
            return DeviceRect::default();
        }
        let (c, r) = ((slot % cols) as f32, (slot / cols) as f32);
        let (cx, cy) = (grid.x + (c + 0.5) * pitch, grid.y + (r + 0.5) * pitch);
        self.device(PtRect {
            x: cx - size / 2.0,
            y: cy - size / 2.0,
            w: size,
            h: size,
        })
    }

    /// Inline chip `slot`'s square (chip = pitch − 6); empty without an inline picker.
    pub fn chip_rect(&self, slot: usize) -> DeviceRect {
        match self.picker {
            Some(p) => self.grid_chip(p.rect, 10, p.pitch, p.pitch - 6.0, slot),
            None => DeviceRect::default(),
        }
    }

    pub fn sheet_chip_rect(&self, slot: usize) -> DeviceRect {
        let s = &self.sheet;
        self.grid_chip(s.grid, s.cols, s.pitch, s.chip, slot)
    }

    /// With the sheet open: a sheet chip's pitch cell is `Strip`, everything else `Nothing`.
    pub fn hit_sheet(&self, x: i32, y: i32) -> Hit {
        let s = &self.sheet;
        self.grid_slot(s.grid, s.cols, s.rows, x, y)
            .map_or(Hit::Nothing, Hit::Strip)
    }

    /// The control under (`x`, `y`): with the sheet open, its close chip, or the scrim outside its
    /// panel; otherwise the Levels pill (a sheet picker) and Next ▶ (`next_shown`).
    pub fn control_at(
        &self,
        x: i32,
        y: i32,
        sheet_open: bool,
        next_shown: bool,
    ) -> Option<Control> {
        let within = |r: PtRect| {
            let d = self.device(r);
            (d.left..d.right).contains(&x) && (d.top..d.bottom).contains(&y)
        };
        if sheet_open {
            return (within(self.sheet.close) || !within(self.sheet.panel))
                .then_some(Control::CloseSheet);
        }
        if self.card_parts.levels.is_some_and(within) {
            return Some(Control::Levels);
        }
        if !next_shown {
            return None;
        }
        // The pill as drawn (grown to whole px) can sit ≤ 1 px off `card_parts.next`: both hit.
        let [l, t, r, b] = self.next_pill();
        let drawn = (l.floor() as i32..r.ceil() as i32).contains(&x)
            && (t.floor() as i32..b.ceil() as i32).contains(&y);
        (drawn || within(self.card_parts.next)).then_some(Control::Next)
    }
}

impl Floorplan for Tableau {
    fn hit(&self, x: i32, y: i32) -> Hit {
        let b = self.board;
        if (b.left..b.right).contains(&x) && (b.top..b.bottom).contains(&y) {
            let (col, row) = ((x - b.left) / self.cell, (y - b.top) / self.cell);
            return Hit::Cell(board_index(col as usize, row as usize));
        }
        let (cols, rows) = (self.tray.cols, self.tray.rows);
        let col = (0..cols).find(|&c| (self.tray_x[c]..self.tray_x[c + 1]).contains(&x));
        let row = (0..rows).find(|&r| (self.tray_y[r]..self.tray_y[r + 1]).contains(&y));
        match (col, row) {
            (Some(c), Some(r)) => Hit::Cell(toolbox_index(r * cols + c)),
            _ => match self.picker {
                Some(p) => self
                    .grid_slot(p.rect, 10, 5, x, y)
                    .map_or(Hit::Nothing, Hit::Strip),
                None => Hit::Nothing,
            },
        }
    }

    /// One pt, in whole device px, at least 1.
    fn drag_slop(&self) -> i32 {
        (self.easel.px_per_pt.round() as i32).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Insets;
    use lumen::{BOARD_CELLS, Hit, VISIBLE_TOOLBOX_SLOTS, board_index, toolbox_index};

    fn near(a: f32, b: f32, tol: f32, what: &str) {
        assert!((a - b).abs() <= tol, "{what}: {a} vs {b}");
    }

    struct Worked {
        size: (u32, u32),
        touch: bool,
        arrangement: Arrangement,
        steps: &'static [u8],
        side: f32,
        cell: f32,
        tray: (usize, usize, f32),
        pitch: Option<f32>,
        card: f32,
        compact: bool,
        brand: bool,
    }

    /// Spec §6 "Worked sizes", every row, and its short-landscape revision's.
    const WORKED: [Worked; 11] = [
        Worked {
            size: (1280, 800),
            touch: false,
            arrangement: Arrangement::Beside,
            steps: &[],
            side: 752.0,
            cell: 48.1,
            tray: (6, 4, 60.0),
            pitch: Some(42.0),
            card: 176.0,
            compact: false,
            brand: true,
        },
        Worked {
            size: (1400, 955),
            touch: false,
            arrangement: Arrangement::Beside,
            steps: &[],
            side: 907.0,
            cell: 58.1,
            tray: (6, 4, 60.0),
            pitch: Some(42.0),
            card: 331.0,
            compact: false,
            brand: true,
        },
        Worked {
            size: (800, 600),
            touch: false,
            arrangement: Arrangement::Beside,
            steps: &[],
            side: 448.0,
            cell: 28.7,
            tray: (6, 4, 46.7),
            pitch: None,
            card: 315.0,
            compact: false,
            brand: false,
        },
        Worked {
            size: (700, 370),
            touch: true,
            arrangement: Arrangement::Beside,
            steps: &[1, 4],
            side: 346.0,
            cell: 22.3,
            tray: (6, 4, 48.0),
            pitch: None,
            card: 146.0,
            compact: true,
            brand: false,
        },
        // A short landscape: iPhone 12–14, SE and the smallest canvas with their bars.
        Worked {
            size: (741, 321),
            touch: true,
            arrangement: Arrangement::Beside,
            steps: &[1, 3, 4, 5, 7],
            side: 297.0,
            cell: 19.0,
            tray: (8, 3, 44.0),
            pitch: None,
            card: 157.0,
            compact: true,
            brand: false,
        },
        Worked {
            size: (611, 327),
            touch: true,
            arrangement: Arrangement::Beside,
            steps: &[4, 5, 7],
            side: 295.0,
            cell: 18.9,
            tray: (8, 3, 35.0),
            pitch: None,
            card: 190.0,
            compact: true,
            brand: false,
        },
        Worked {
            size: (568, 300),
            touch: true,
            arrangement: Arrangement::Beside,
            steps: &[4, 5, 7],
            side: 252.0,
            cell: 16.1,
            tray: (8, 3, 35.0),
            pitch: None,
            card: 163.0,
            compact: true,
            brand: false,
        },
        // A short landscape whose 6×4 slots stay bigger than 8×3's (a column's eighth, 35).
        Worked {
            size: (640, 360),
            touch: true,
            arrangement: Arrangement::Beside,
            steps: &[4, 5, 7],
            side: 324.0,
            cell: 20.8,
            tray: (6, 4, 43.0),
            pitch: None,
            card: 156.0,
            compact: true,
            brand: false,
        },
        Worked {
            size: (390, 740),
            touch: true,
            arrangement: Arrangement::Stacked,
            steps: &[],
            side: 366.0,
            cell: 23.6,
            tray: (8, 3, 45.75),
            pitch: None,
            card: 171.0,
            compact: false,
            brand: false,
        },
        Worked {
            size: (390, 616),
            touch: true,
            arrangement: Arrangement::Stacked,
            steps: &[1, 2, 3],
            side: 320.0,
            cell: 20.5,
            tray: (8, 3, 44.0),
            pitch: None,
            card: 124.0,
            compact: true,
            brand: false,
        },
        Worked {
            size: (820, 1180),
            touch: true,
            arrangement: Arrangement::Stacked,
            steps: &[],
            side: 708.0,
            cell: 45.9,
            tray: (12, 2, 56.0),
            pitch: None,
            card: 262.0,
            compact: false,
            brand: false,
        },
    ];

    #[test]
    fn the_worked_sizes() {
        for row in &WORKED {
            let (w, h) = row.size;
            let t = Tableau::arrange(&Easel::new(w, h, 1.0).with_touch(row.touch));
            let what = format!("{w}×{h}");
            assert_eq!(t.arrangement, row.arrangement, "{what}");
            assert_eq!(
                t.steps.list().collect::<Vec<_>>(),
                row.steps,
                "{what} steps"
            );
            near(t.board_slot.w, row.side, 0.5, &what);
            near((t.board_slot.w - 2.0 * t.pad) / 15.0, row.cell, 0.05, &what);
            assert_eq!(
                (t.tray.cols, t.tray.rows),
                (row.tray.0, row.tray.1),
                "{what}"
            );
            near(t.tray.slot, row.tray.2, 0.05, &what);
            assert_eq!(
                t.picker.map(|p| (p.pitch * 10.0).round() / 10.0),
                row.pitch,
                "{what}"
            );
            near(t.card.h, row.card, 0.5, &what);
            assert_eq!(t.compact, row.compact, "{what}");
            assert_eq!(
                t.credits.is_none(),
                row.compact,
                "{what}: credits leave a compact card"
            );
            assert_eq!(t.brand.is_some(), row.brand, "{what}");
            assert!(t.fits, "{what}");
        }
    }

    #[test]
    fn card_min_matches_the_spec_table() {
        for (col, full, compact) in [
            (280.0, 174.0, 152.0),
            (318.0, 160.0, 138.0),
            (366.0, 146.0, 124.0),
            (420.0, 146.0, 124.0),
        ] {
            near(card_min(col, false), full, 0.5, "full");
            near(card_min(col, true), compact, 0.5, "compact");
        }
    }

    #[test]
    fn the_minimum_height_is_stacked_at_its_floor() {
        near(
            Tableau::min_height(390.0),
            596.0,
            0.5,
            "spec §6: 24 + 300 + 8 + 132 + 8 + 124",
        );
        for w in [320.0, 390.0, 430.0, 600.0, 820.0] {
            let h = Tableau::min_height(w);
            let at = Tableau::arrange(&Easel::new(w as u32, h.ceil() as u32, 1.0).with_touch(true));
            assert!(at.fits, "{w}: the minimum height fits");
        }
    }

    #[test]
    fn the_stage_grows_only_when_the_room_is_too_short() {
        assert_eq!(stage_height(390.0, 740.0, true), 740.0);
        near(
            stage_height(390.0, 500.0, true),
            596.0,
            0.5,
            "a short phone scrolls",
        );
        assert_eq!(stage_height(1400.0, 955.0, false), 955.0);
        assert!(stage_height(f32::NAN, f32::NAN, false) >= 1.0);
    }

    /// The page's backing scales: whole and fractional `devicePixelRatio`s (render_scale is a
    /// multiple of ¼ up to 2; 2.625 and 3 cover a raw ratio).
    const PAGE_SCALES: [f32; 7] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.625, 3.0];

    /// The page sizes the canvas `w × stage_height(w, room)` CSS px and its backing
    /// `round(css × s)`: the tableau the Session then arranges must hold its card.
    fn assert_page_fits(w: f32, room: f32, touch: bool) {
        let h = stage_height(w, room, touch);
        assert!(h >= room, "{w} × {room} touch {touch}: {h} below the room");
        for s in PAGE_SCALES {
            let (bw, bh) = ((w * s).round() as u32, (h * s).round() as u32);
            let easel = Easel::new(bw, bh, s).with_touch(touch);
            assert!(
                Tableau::arrange(&easel).fits,
                "{w} × {room} touch {touch}: stage {h} at @{s} ({bw} × {bh} px) does not fit"
            );
        }
    }

    /// `stage_height`'s ±½-pt contract under the page's whole-px backing: a coarse grid of widths
    /// and rooms, the band where Beside turns Stacked (W ≈ 1.2·H), the band around Stacked's
    /// minimum height, and the cases the review found.
    #[test]
    fn the_stage_height_fits_after_the_pages_rounding() {
        for touch in [false, true] {
            let mut w = 300.0f32;
            while w < 1700.0 {
                let mut room = 180.0f32;
                while room < 1300.0 {
                    assert_page_fits(w, room, touch);
                    room += 3.7;
                }
                let floor = Tableau::min_height(w);
                for k in -12..=12 {
                    assert_page_fits(w, floor + 0.19 * k as f32, touch);
                }
                w += 2.3;
            }
            let mut room = 280.0f32;
            while room < 1300.0 {
                for k in -10..=10 {
                    assert_page_fits(1.2 * room + 0.37 * k as f32, room, touch);
                }
                room += 13.7;
            }
            for (w, room) in [
                (329.3, 592.37),
                (341.7, 180.0),
                (646.0, 538.3),
                (646.0, 539.0),
                (673.4, 560.86),
                (390.0, 500.0),
            ] {
                assert_page_fits(w, room, touch);
            }
        }
    }

    fn apart(a: &PtRect, b: &PtRect) -> bool {
        a.x + a.w <= b.x + 0.01
            || b.x + b.w <= a.x + 0.01
            || a.y + a.h <= b.y + 0.01
            || b.y + b.h <= a.y + 0.01
    }

    fn overlaps(a: DeviceRect, b: DeviceRect) -> bool {
        a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    }

    /// Everything §15 asks of a layout, for one easel.
    fn check_layout(t: &Tableau, easel: &Easel) {
        let (w, h) = easel.room();
        let mut regions = vec![
            ("board", t.board_slot),
            ("card", t.card),
            ("tray", t.tray.rect),
        ];
        regions.extend(t.brand.map(|r| ("brand", r)));
        regions.extend(t.picker.map(|p| ("picker", p.rect)));
        regions.extend(t.credits.map(|r| ("credits", r)));
        // Only a canvas below Stacked's minimum height may fail to fit; it then overflows downward
        // (the page grows the canvas to `stage_height` and scrolls).
        assert!(
            t.fits || h < Tableau::min_height(w) + 0.01,
            "{easel:?}: does not fit above min_height"
        );
        for (i, (na, a)) in regions.iter().enumerate() {
            let inside =
                a.x >= -0.01 && a.y >= -0.01 && a.x + a.w <= w + 0.01 && a.y + a.h <= h + 0.01;
            assert!(inside || !t.fits, "{easel:?}: {na} {a:?} outside");
            for (nb, b) in &regions[i + 1..] {
                assert!(apart(a, b), "{easel:?}: {na} {a:?} overlaps {nb} {b:?}");
            }
        }
        assert_eq!(
            t.board.right - t.board.left,
            15 * t.cell,
            "{easel:?}: square, whole cells"
        );
        assert_eq!(t.board.bottom - t.board.top, 15 * t.cell);
        assert!(t.cell >= 1 && t.card.h >= 0.0);
        if t.fits {
            let width = t.card.w;
            assert!(
                t.card.h >= card_min(width, t.compact) - 0.01,
                "{easel:?}: card under card_min"
            );
        }
        if t.arrangement == Arrangement::Beside {
            let least = if easel.touch {
                MIN_BESIDE_TOUCH
            } else {
                MIN_BESIDE
            };
            assert!(
                w >= 1.2 * h - 0.01 && t.board_slot.w >= least - 0.01 && t.fits,
                "{easel:?}"
            );
            // A board under 330 is a touch canvas's: one that starts there, or step 8's.
            let m = t.margin;
            let full = (h - 2.0 * m).min(w - 3.0 * m - MIN_COLUMN);
            assert!(
                t.board_slot.w >= MIN_BESIDE - 0.01
                    || t.steps.has(8)
                    || (easel.touch && (t.board_slot.w - full).abs() < 0.01),
                "{easel:?}: board {} without step 8",
                t.board_slot.w
            );
            assert!(
                !t.steps.has(8) || t.steps.has(7),
                "{easel:?}: the slot first"
            );
            // 6×4, or a touch step 7's 8×3, inside the column.
            let shape = (t.tray.cols, t.tray.rows);
            assert!(
                shape == (6, 4) || (easel.touch && t.steps.has(7) && shape == (8, 3)),
                "{easel:?}: tray {shape:?}"
            );
            assert!(
                t.tray.grid.w <= t.column.w + 0.01,
                "{easel:?}: the tray is wider than the column"
            );
        }
        // Touch slots: ≥ 44 pt in Beside but on a short landscape (step 7, down to 28) and in
        // Stacked from 376 pt wide (ruling 3).
        if easel.touch && (t.arrangement == Arrangement::Beside || w >= 376.0) {
            let least = if t.steps.has(7) && t.arrangement == Arrangement::Beside {
                MIN_TOUCH_SLOT
            } else {
                44.0
            };
            assert!(
                t.tray.slot >= least - 0.01,
                "{easel:?}: touch slot {}",
                t.tray.slot
            );
        }
        assert!(
            easel.touch || !(t.steps.has(7) || t.steps.has(8)),
            "{easel:?}: steps 7 and 8 are touch only"
        );
        // The board's paint never reaches the card, brand, picker or credits (the tray may be
        // reached: the darkroom subtracts it).
        let room = t.board_room();
        for (name, r) in &regions[1..] {
            if *name != "tray" {
                assert!(
                    !overlaps(room, t.device(*r)),
                    "{easel:?}: board room reaches {name}"
                );
            }
        }
        for index in (0..BOARD_CELLS).chain((0..VISIBLE_TOOLBOX_SLOTS).map(toolbox_index)) {
            let r = t.cell_rect(index);
            assert_eq!(
                t.hit((r.left + r.right) / 2, (r.top + r.bottom) / 2),
                Hit::Cell(index),
                "{easel:?} {index}"
            );
            assert_eq!(t.hit(r.left, r.top), Hit::Cell(index));
            assert_eq!(t.hit(r.right - 1, r.bottom - 1), Hit::Cell(index));
        }
    }

    #[test]
    fn a_dense_sweep_of_canvases_lays_out_cleanly() {
        for w in (320..=1600).step_by(7) {
            for h in (300..=1200).step_by(7) {
                for touch in [false, true] {
                    let easel = Easel::new(w, h, 1.0).with_touch(touch);
                    check_layout(&Tableau::arrange(&easel), &easel);
                }
            }
        }
    }

    /// Spec §6 "Revision: short landscape": every touch landscape canvas from 300 pt tall (phones
    /// with their browser's bars, 568–932 pt wide) keeps Beside, fits its card and needs no
    /// taller stage, so the page never scrolls; the layout holds at every pt, and its controls on
    /// a coarser grid.
    #[test]
    fn short_touch_landscapes_keep_beside() {
        for w in 568..=932u32 {
            for h in 300..=430u32 {
                let easel = Easel::new(w, h, 1.0).with_touch(true);
                let t = Tableau::arrange(&easel);
                assert_eq!(t.arrangement, Arrangement::Beside, "{w} × {h}");
                assert!(t.fits, "{w} × {h}");
                // 8×3 keeps the slots at a column's eighth (≥ 35) at the least.
                assert!(t.tray.slot >= 35.0, "{w} × {h}: slot {}", t.tray.slot);
                check_layout(&t, &easel);
                if (w - 568) % 7 == 0 && (h - 300) % 5 == 0 {
                    check_controls(&t, &easel);
                }
                for (dw, dh) in [(0.0, 0.0), (0.5, 0.5), (0.3, 0.7)] {
                    let (cw, ch) = (w as f32 + dw, h as f32 + dh);
                    assert_eq!(stage_height(cw, ch, true), ch, "{cw} × {ch}: no scroll");
                }
            }
        }
        // The review's phones, with Safari's or Chrome's bars, at their own scales and insets
        // (rail 56 pt; notched iPhones keep 47 pt right and 21 pt bottom).
        for (w, h, scale, right, bottom) in [
            (844.0, 342.0, 3.0, 47.0, 21.0),
            (667.0, 327.0, 2.0, 0.0, 0.0),
            (740.0, 304.0, 3.0, 0.0, 0.0),
            (800.0, 304.0, 2.625, 0.0, 0.0),
        ] {
            let canvas = w - 56.0;
            let easel = Easel::new((canvas * scale) as u32, (h * scale) as u32, scale)
                .with_touch(true)
                .with_safe(Insets {
                    top: 0.0,
                    right,
                    bottom,
                    left: 0.0,
                });
            let t = Tableau::arrange(&easel);
            assert_eq!(t.arrangement, Arrangement::Beside, "{w} × {h}");
            check_layout(&t, &easel);
            check_controls(&t, &easel);
        }
    }

    /// A short Beside's 8×3 tray reflows lumen's 24 slots like Stacked's: across 8, then down,
    /// every slot hitting itself, the grid inside the column under the card.
    #[test]
    fn a_short_besides_8_by_3_tray_maps_lumen_slots() {
        for scale in [1.0, 2.0, 3.0] {
            let easel =
                Easel::new((741.0 * scale) as u32, (321.0 * scale) as u32, scale).with_touch(true);
            let t = Tableau::arrange(&easel);
            assert_eq!(t.arrangement, Arrangement::Beside);
            assert_eq!((t.tray.cols, t.tray.rows), (8, 3));
            let (first, last_in_row, next_row, last) = (
                t.cell_rect(toolbox_index(0)),
                t.cell_rect(toolbox_index(7)),
                t.cell_rect(toolbox_index(8)),
                t.cell_rect(toolbox_index(23)),
            );
            assert!(last_in_row.left > first.left && last_in_row.top == first.top);
            assert!(next_row.top > first.top && next_row.left == first.left);
            assert_eq!((last.right, last.bottom), (last_in_row.right, t.tray_y[3]));
            assert!(
                first.left >= t.board.right,
                "the tray is right of the board"
            );
            for slot in 0..VISIBLE_TOOLBOX_SLOTS {
                let r = t.cell_rect(toolbox_index(slot));
                let (x, y) = centre(r);
                assert_eq!(
                    t.hit(x, y),
                    Hit::Cell(toolbox_index(slot)),
                    "@{scale} {slot}"
                );
            }
            assert!(inside(t.tray.grid, t.column) && t.tray.grid.y >= t.card.y + t.card.h);
        }
    }

    #[test]
    fn scales_and_safe_areas_lay_out_cleanly() {
        let mut easels = vec![];
        for (w, h) in [
            (800, 600),
            (1280, 800),
            (1400, 955),
            (2560, 1440),
            (3840, 2160),
            (390, 844),
            (844, 390),
            (820, 1180),
        ] {
            for scale in [1.0, 2.0, 3.0] {
                easels.push(
                    Easel::new((w as f32 * scale) as u32, (h as f32 * scale) as u32, scale)
                        .with_touch(w < 1000),
                );
            }
        }
        easels.push(
            Easel::new(2532, 1170, 3.0)
                .with_touch(true)
                .with_safe(Insets {
                    top: 0.0,
                    right: 47.0,
                    bottom: 21.0,
                    left: 0.0,
                }),
        );
        easels.push(
            Easel::new(1170, 2532, 3.0)
                .with_touch(true)
                .with_safe(Insets {
                    top: 0.0,
                    right: 0.0,
                    bottom: 34.0,
                    left: 0.0,
                }),
        );
        for easel in easels {
            let t = Tableau::arrange(&easel);
            check_layout(&t, &easel);
            assert_eq!(t.drag_slop(), (easel.px_per_pt.round() as i32).max(1));
        }
    }

    /// `lightbox_budgets.mjs` drives its pointers by these numbers (its `BOARD` and `TRAY`): the
    /// Beside full form at the spec's web size must not move.
    #[test]
    fn the_web_budget_geometry_holds() {
        let t = Tableau::arrange(&Easel::new(2450, 1671, 1.75));
        assert_eq!((t.board.left, t.board.top, t.cell), (79, 78, 101));
        let (first, second) = (t.cell_rect(toolbox_index(0)), t.cell_rect(toolbox_index(1)));
        assert_eq!(
            (first.left, first.top, second.left - first.left),
            (1725, 754, 105)
        );
    }

    #[test]
    fn rotating_a_phone_switches_arrangement() {
        let portrait = Tableau::arrange(&Easel::new(390, 740, 1.0).with_touch(true));
        let landscape = Tableau::arrange(&Easel::new(740, 390, 1.0).with_touch(true));
        assert_eq!(
            (portrait.arrangement, landscape.arrangement),
            (Arrangement::Stacked, Arrangement::Beside)
        );
    }

    #[test]
    fn margins_and_outside_hit_nothing() {
        let t = Tableau::arrange(&Easel::new(2560, 1600, 2.0));
        for (x, y) in [
            (0, 0),
            (t.board.left - 1, t.board.top),
            (t.board.right, t.board.top),
            (-5, 300),
            (100_000, 100_000),
            (i32::MIN / 2, i32::MAX / 2),
        ] {
            assert_eq!(t.hit(x, y), Hit::Nothing, "({x}, {y})");
        }
        let card = t.device(t.card);
        assert_eq!(
            t.hit((card.left + card.right) / 2, (card.top + card.bottom) / 2),
            Hit::Nothing
        );
    }

    #[test]
    fn tiny_and_broken_easels_arrange() {
        for easel in [
            Easel::new(0, 0, 1.0),
            Easel::new(1, 1, 3.0),
            Easel::new(300, 200, 1.0),
            Easel::new(200, 900, 2.0),
            Easel::new(5000, 40, 1.0),
            Easel::new(640, 480, f32::NAN),
        ] {
            let t = Tableau::arrange(&easel);
            assert!(t.cell >= 1, "{easel:?}");
            assert!(t.card.h >= 0.0 && t.tray.slot > 0.0);
            let _ = t.hit(0, 0);
            assert_eq!(t.cell_rect(500), DeviceRect::default());
        }
    }

    #[test]
    fn render_scale_caps_the_board_at_1600_px() {
        assert_eq!(render_scale(1400.0, 955.0, 2.0, false), 1.75);
        assert_eq!(render_scale(1400.0, 955.0, 1.0, false), 1.0);
        assert_eq!(render_scale(1400.0, 955.0, 1.25, false), 1.25);
        assert_eq!(render_scale(1280.0, 800.0, 3.0, false), 2.0);
        assert_eq!(
            render_scale(390.0, 740.0, 3.0, true),
            2.0,
            "a 3× phone draws at 2×"
        );
        assert_eq!(render_scale(1280.0, 800.0, 0.5, false), 1.0, "at least 1");
        assert_eq!(render_scale(1280.0, 800.0, f32::NAN, false), 1.0);
    }

    #[test]
    fn tray_slots_map_to_lumen_indices_in_both_arrangements() {
        for (easel, cols) in [
            (Easel::new(1280, 800, 1.0), 6),
            (Easel::new(390, 740, 1.0).with_touch(true), 8),
        ] {
            let t = Tableau::arrange(&easel);
            let r = t.cell_rect(board_index(14, 0));
            assert_eq!((r.right, r.top), (t.board.right, t.board.top));
            let first = t.cell_rect(toolbox_index(0));
            let last_in_row = t.cell_rect(toolbox_index(cols - 1));
            let next_row = t.cell_rect(toolbox_index(cols));
            assert!(
                last_in_row.left > first.left && last_in_row.top == first.top,
                "slots run across"
            );
            assert!(
                next_row.top > first.top && next_row.left == first.left,
                "then down"
            );
        }
        assert_eq!(
            Tableau::arrange(&Easel::new(1280, 800, 1.0)).tray_piece_px(),
            54,
            "0.9 × 60 pt at 1×"
        );
    }

    fn centre(r: DeviceRect) -> (i32, i32) {
        ((r.left + r.right) / 2, (r.top + r.bottom) / 2)
    }

    fn inside(inner: PtRect, outer: PtRect) -> bool {
        inner.x >= outer.x - 0.01
            && inner.y >= outer.y - 0.01
            && inner.x + inner.w <= outer.x + outer.w + 0.01
            && inner.y + inner.h <= outer.y + outer.h + 0.01
    }

    /// Chips, the sheet and the controls of one easel (Review Focus 1 and 5).
    fn check_controls(t: &Tableau, easel: &Easel) {
        for slot in 0..50 {
            let inline = t.chip_rect(slot);
            match t.picker {
                Some(_) => {
                    let (x, y) = centre(inline);
                    assert_eq!(
                        t.hit(x, y),
                        Hit::Strip(slot),
                        "{easel:?}: inline chip {slot}"
                    );
                }
                None => assert_eq!(inline, DeviceRect::default()),
            }
            let (x, y) = centre(t.sheet_chip_rect(slot));
            assert_eq!(
                t.hit_sheet(x, y),
                Hit::Strip(slot),
                "{easel:?}: sheet chip {slot}"
            );
        }
        // Touch chips are ≥ 44 pt whenever a 44-pt sheet fits the room (ruling 17).
        let (w, h) = easel.room();
        let (cols, rows) = (t.sheet.cols as f32, t.sheet.rows as f32);
        let fits44 =
            48.0 * cols + 32.0 <= w - 2.0 * t.margin && 48.0 * rows + 76.0 <= h - 2.0 * t.margin;
        if easel.touch && fits44 {
            assert!(
                t.sheet.chip >= 44.0 - 0.01,
                "{easel:?}: sheet chip {}",
                t.sheet.chip
            );
        }
        let parts = &t.card_parts;
        assert_eq!(
            parts.levels.is_some(),
            t.picker.is_none(),
            "the pill opens the sheet"
        );
        assert_eq!(parts.name.is_none(), t.compact);
        // A canvas below min_height overflows; its card is squeezed to nothing, so its controls
        // may overlap. The page never shows one (it grows the canvas to `stage_height`).
        if !t.fits {
            return;
        }
        {
            for (name, part) in [
                ("header", Some(parts.header)),
                ("levels", parts.levels),
                ("badge", Some(parts.badge)),
                ("name", parts.name),
                ("body", Some(parts.body)),
                ("next", Some(parts.next)),
            ] {
                if let Some(p) = part {
                    assert!(
                        inside(p, t.card),
                        "{easel:?}: {name} {p:?} outside the card {:?}",
                        t.card
                    );
                }
            }
        }
        let point = |r: PtRect| centre(t.device(r));
        if let Some(pill) = parts.levels {
            let (x, y) = point(pill);
            assert_eq!(t.control_at(x, y, false, false), Some(Control::Levels));
            assert_ne!(
                t.control_at(x, y, true, false),
                Some(Control::Levels),
                "hidden by the sheet"
            );
        }
        let (x, y) = point(parts.next);
        assert_eq!(t.control_at(x, y, false, true), Some(Control::Next));
        assert_eq!(
            t.control_at(x, y, false, false),
            None,
            "Next only while shown"
        );
        // Every px the pill is drawn on answers, though it can sit ≤ 1 px off its rect.
        let [l, top, r, b] = t.next_pill();
        let (l, top, r, b) = (
            l.floor() as i32,
            top.floor() as i32,
            r.ceil() as i32 - 1,
            b.ceil() as i32 - 1,
        );
        for (x, y) in [(l, top), (r, top), (l, b), (r, b)] {
            assert_eq!(
                t.control_at(x, y, false, true),
                Some(Control::Next),
                "{easel:?}: the drawn pill's corner ({x}, {y})"
            );
        }
        // Next's hit rect (its rect ∪ the drawn pill) stays inside the card.
        let (next, card) = (t.device(parts.next), t.device(t.card));
        assert!(
            next.left.min(l) >= card.left
                && next.top.min(top) >= card.top
                && next.right.max(r + 1) <= card.right
                && next.bottom.max(b + 1) <= card.bottom,
            "{easel:?}: Next ▶ {next:?} / drawn ({l}, {top})–({r}, {b}) leaves the card {card:?}"
        );
        let (x, y) = point(t.sheet.close);
        assert_eq!(t.control_at(x, y, true, false), Some(Control::CloseSheet));
        let (x, y) = point(t.sheet.title);
        assert_eq!(
            t.control_at(x, y, true, false),
            None,
            "inside the panel is not the scrim"
        );
        let (x, y) = point(t.board_slot);
        assert_eq!(t.control_at(x, y, false, false), None);
        let (panel, canvas) = (t.device(t.sheet.panel), t.canvas());
        assert!(
            panel.left >= canvas.left
                && panel.top >= canvas.top
                && panel.right <= canvas.right
                && panel.bottom <= canvas.bottom,
            "{easel:?}: the sheet {panel:?} leaves the canvas"
        );
        if panel.left > 0 && panel.top > 0 {
            assert_eq!(
                t.control_at(0, 0, true, false),
                Some(Control::CloseSheet),
                "the scrim"
            );
        }
    }

    #[test]
    fn every_chip_and_control_hits_itself_across_the_sweep() {
        for w in (320..=1600).step_by(23) {
            for h in (300..=1200).step_by(23) {
                for touch in [false, true] {
                    let easel = Easel::new(w, h, 1.0).with_touch(touch);
                    check_controls(&Tableau::arrange(&easel), &easel);
                }
            }
        }
        for row in &WORKED {
            for scale in [1.0, 1.25, 1.75, 2.0, 2.625, 3.0] {
                let easel = Easel::new(
                    (row.size.0 as f32 * scale) as u32,
                    (row.size.1 as f32 * scale) as u32,
                    scale,
                )
                .with_touch(row.touch);
                check_controls(&Tableau::arrange(&easel), &easel);
            }
        }
    }

    #[test]
    fn degenerate_canvases_never_panic_on_hits() {
        for (w, h) in [(0, 0), (1, 1), (3, 200), (200, 3), (16, 16), (40, 30)] {
            for touch in [false, true] {
                let t = Tableau::arrange(&Easel::new(w, h, 1.0).with_touch(touch));
                for (x, y) in [
                    (-5, -5),
                    (0, 0),
                    (1, 1),
                    (w as i32 / 2, h as i32 / 2),
                    (w as i32, h as i32),
                    (i32::MAX, i32::MAX),
                ] {
                    let _ = (t.hit(x, y), t.hit_sheet(x, y));
                    for open in [false, true] {
                        let _ = t.control_at(x, y, open, true);
                    }
                }
                for slot in [0, 49, 50, usize::MAX] {
                    let _ = (t.chip_rect(slot), t.sheet_chip_rect(slot));
                }
            }
        }
    }

    #[test]
    fn the_sheet_follows_the_canvas_shape() {
        let wide = Tableau::arrange(&Easel::new(1280, 800, 1.0));
        assert_eq!((wide.sheet.cols, wide.sheet.rows), (10, 5));
        near(wide.sheet.pitch, 56.0, 0.01, "pitch capped at 56");
        near(wide.sheet.chip, 48.0, 0.01, "chip = pitch − 8");
        let tall = Tableau::arrange(&Easel::new(390, 616, 1.0).with_touch(true));
        assert_eq!((tall.sheet.cols, tall.sheet.rows), (5, 10));
        near(tall.sheet.chip, 44.0, 0.01, "at least 44 on touch");
        let p = tall.sheet.panel;
        near(
            p.w,
            5.0 * tall.sheet.pitch + 32.0,
            0.01,
            "grid + 16 padding",
        );
        near(
            p.h,
            10.0 * tall.sheet.pitch + 76.0,
            0.01,
            "grid + 16 padding + 44 header",
        );
        near(p.x + p.w / 2.0, 195.0, 0.01, "centred");
    }

    #[test]
    fn inline_chips_number_across_then_down() {
        let t = Tableau::arrange(&Easel::new(1280, 800, 1.0));
        let (a, b, k) = (t.chip_rect(0), t.chip_rect(9), t.chip_rect(10));
        assert!(b.left > a.left && b.top == a.top && k.top > a.top && k.left == a.left);
        near(
            (a.right - a.left) as f32,
            42.0 - 6.0,
            1.0,
            "chip = pitch − 6",
        );
        assert_eq!(t.chip_rect(50), DeviceRect::default());
        let gap = (a.right + 1, (a.top + a.bottom) / 2);
        assert_eq!(
            t.hit(gap.0, gap.1),
            Hit::Strip(0),
            "a pitch cell, gap included, is its chip's"
        );
    }

    #[test]
    fn the_card_parts_follow_the_spec_rows() {
        let t = Tableau::arrange(&Easel::new(390, 740, 1.0).with_touch(true));
        let (c, p) = (t.card, t.card_parts);
        near(p.header.y, c.y + 12.0, 0.01, "padding");
        near(p.header.h, 28.0, 0.01, "header row");
        let name = p.name.expect("a full card has a name row");
        near(name.y, p.header.y + 28.0, 0.01, "name row under the header");
        near(p.body.y, name.y + 22.0, 0.01, "body under the name");
        near(
            p.body.y + p.body.h,
            c.y + c.h - 12.0,
            0.01,
            "to the bottom padding",
        );
        near(
            p.nameless_body.y,
            p.header.y + 28.0,
            0.01,
            "nameless: the body takes the name row",
        );
        near(
            p.nameless_body.y + p.nameless_body.h,
            c.y + c.h - 12.0,
            0.01,
            "nameless: to the bottom padding",
        );
        let pill = p.levels.expect("a sheet picker");
        near(pill.x + pill.w, c.x + c.w - 12.0, 0.01, "pill at the right");
        near(
            p.badge.x + p.badge.w,
            pill.x - 10.0,
            0.01,
            "badge left of the pill",
        );
        near(
            p.next.x + p.next.w,
            c.x + c.w - 12.0,
            0.01,
            "Next at the bottom right",
        );
        near(
            p.next.y + p.next.h,
            c.y + c.h - 12.0,
            0.01,
            "Next at the bottom",
        );
        near(
            p.won_body.y,
            p.header.y + 28.0,
            0.01,
            "won: the body follows the header",
        );
        assert!(
            p.won_body.y + p.won_body.h <= p.next.y - 8.0 + 0.01,
            "won: the body stays above Next"
        );
        let compact = Tableau::arrange(&Easel::new(700, 370, 1.0).with_touch(true));
        assert!(compact.card_parts.name.is_none());
        near(
            compact.card_parts.body.y,
            compact.card_parts.header.y + 28.0,
            0.01,
            "compact: no name row",
        );
        assert_eq!(
            compact.card_parts.nameless_body, compact.card_parts.body,
            "compact: a name or not, the same body"
        );
    }
}
