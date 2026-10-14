//! The modern look in the macOS shell (spec §13): one `Lightbox` painting into the shell's
//! `Xrgb` canvas, then the card's captions inked by Core Text inside each paint's damage
//! (ruling P3). Changed captions recompose their old rects first (`Lightbox::invalidate`,
//! ruling P4); none show while the level sheet does (P5). Testable without a window, as the
//! web `Session` is.

use std::time::{Duration, Instant};

use halation::captions::MAX_CAPTIONS;
use halation::{
    Captions, Control, DeviceRect, Easel, Lightbox, MAX_DAMAGE, Output, Route, Scene, Theme,
};
use lumen::{GameState, InputEvent};
use spectrograph::Game;

use crate::caption_typeset::{CaptionLines, CaptionTypesetter, Clip, lay_out};

/// What decides the card's captions besides the layout. When it differs from the last
/// paint's, the old captions' rects are recomposed by that same paint (ruling P4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CardKey {
    game: Game,
    slot: usize,
    won: bool,
    next: bool,
    sheet: bool,
}

impl CardKey {
    fn of(state: &GameState<'_>, sheet: bool) -> CardKey {
        CardKey {
            game: state.data().game,
            slot: state.slot(),
            won: state.is_won(),
            next: Scene::of(state).next_open(),
            sheet,
        }
    }
}

/// The byte `key_byte` gives for Escape.
pub const ESCAPE: u8 = 0x1b;

/// Whether a key closes the level sheet instead of reaching the game (ruling P10, as
/// `Session::key`): Escape, while the sheet is open.
pub fn escape_closes_sheet(byte: Option<u8>, sheet_open: bool) -> bool {
    sheet_open && byte == Some(ESCAPE)
}

/// A position in canvas device px (winit's physical cursor) as the pixel it falls in: floors,
/// NaN gives 0, clamped to ±65 536 (as the web's `device_point`).
pub fn device_point(x: f64, y: f64) -> (i32, i32) {
    let axis = |v: f64| {
        if v.is_nan() {
            0
        } else {
            v.floor().clamp(-65_536.0, 65_536.0) as i32
        }
    };
    (axis(x), axis(y))
}

/// Time spent in one stage paint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StageTimes {
    /// `Lightbox::paint` (twice when P4's fallback ran).
    pub paint: Duration,
    /// Laying out, setting and inking the captions.
    pub captions: Duration,
    /// Lightbox paints this frame: 1, or 2 when changed captions needed P4's fallback.
    pub paints: u8,
}

pub struct ModernStage {
    lightbox: Lightbox,
    theme: Theme,
    typesetter: CaptionTypesetter,
    /// The captions shown, as inked lines (≤ 8).
    lines: CaptionLines,
    /// The shown captions' fingerprint (`None` before the first paint and after a resize).
    shown: Option<u64>,
    /// The last paint's card key (`None` before the first paint and after a resize).
    key: Option<CardKey>,
    /// This paint's damage: one or two lightbox paints' rects, then the rects of captions inked
    /// whole (only when they changed, so `Clip::Within` never sees more than `MAX_SPANS`).
    damage: [DeviceRect; 2 * MAX_DAMAGE + MAX_CAPTIONS],
    damage_len: usize,
}

impl ModernStage {
    pub fn new(easel: Easel, theme: Theme, reduced_motion: bool) -> ModernStage {
        ModernStage {
            lightbox: Lightbox::new(easel, theme, reduced_motion),
            theme,
            typesetter: CaptionTypesetter::new(easel.px_per_pt),
            lines: CaptionLines::new(),
            shown: None,
            key: None,
            damage: [DeviceRect::default(); 2 * MAX_DAMAGE + MAX_CAPTIONS],
            damage_len: 0,
        }
    }

    /// A new window size or scale (ruling P6): re-arranged, the next paint cold; captions set
    /// again at the new scale.
    pub fn resize(&mut self, easel: Easel) {
        self.lightbox.resize(easel);
        self.typesetter.set_scale(easel.px_per_pt);
        self.shown = None;
        self.key = None;
    }

    pub fn set_theme(&mut self, theme: Theme) {
        if theme != self.theme {
            self.theme = theme;
            self.lightbox.set_theme(theme);
        }
    }

    pub fn set_reduced_motion(&mut self, on: bool) {
        self.lightbox.set_reduced_motion(on);
    }

    /// The next paint is cold and settled (a look switch, spec §5.3).
    pub fn reset(&mut self) {
        self.lightbox.reset();
    }

    /// The floorplan the game's input goes through (`handle_on`).
    pub fn lightbox(&self) -> &Lightbox {
        &self.lightbox
    }

    pub fn route(&mut self, event: InputEvent) -> Route {
        self.lightbox.route(event)
    }

    /// Opens or closes the sheet (`Levels`, `CloseSheet`); true when the picture changes.
    pub fn command(&mut self, control: Control) -> bool {
        self.lightbox.command(control)
    }

    pub fn forget_press(&mut self) {
        self.lightbox.forget_press();
    }

    pub fn sheet_open(&self) -> bool {
        self.lightbox.sheet_open()
    }

    pub fn has_inline_picker(&self) -> bool {
        self.lightbox.has_inline_picker()
    }

    pub fn next_wake(&self, now_ms: f64) -> Option<f64> {
        self.lightbox.next_wake(now_ms)
    }

    /// The last paint's damage (device px), captions included.
    pub fn damage(&self) -> &[DeviceRect] {
        &self.damage[..self.damage_len]
    }

    /// The shown captions' rects.
    pub fn caption_rects(&self) -> impl Iterator<Item = DeviceRect> + '_ {
        self.lines.iter().map(|l| l.rect)
    }

    fn note(&mut self, rects: &[DeviceRect]) {
        for &r in rects {
            if self.damage_len < self.damage.len() {
                self.damage[self.damage_len] = r;
                self.damage_len += 1;
            }
        }
    }

    /// Adds each shown line's rect, clipped to `canvas`, to the damage: they are inked whole.
    fn note_lines(&mut self, canvas: DeviceRect) {
        for i in 0..self.lines.len() {
            let r = self.lines[i].rect;
            let clipped = DeviceRect::new(
                r.left.max(canvas.left),
                r.top.max(canvas.top),
                r.right.min(canvas.right),
                r.bottom.min(canvas.bottom),
            );
            if clipped.left < clipped.right && clipped.top < clipped.bottom {
                self.note(&[clipped]);
            }
        }
    }

    /// Recomposes every shown caption's rect at the next lightbox paint.
    fn invalidate_lines(&mut self) {
        for line in self.lines.iter() {
            self.lightbox.invalidate(line.rect);
        }
    }

    /// Paints `state` at `now_ms` into `canvas` (the easel's size, `0x00RRGGBB`) and inks the
    /// captions: within the damage when they are unchanged; when they changed, the new ones
    /// whole, once their old rects are recomposed. A new card (game, slot, won, Next ▶, sheet)
    /// invalidates the old rects before the paint, so that paint recomposes them. A caption
    /// change without one falls back to a second paint at the same `now`, which is skipped
    /// after a cold paint (ruling P4). Lays out (and so measures) only when the captions
    /// changed; allocates only then, to set new text.
    pub fn paint(
        &mut self,
        state: &GameState<'static>,
        now_ms: f64,
        canvas: &mut [u32],
    ) -> StageTimes {
        let started = Instant::now();
        self.damage_len = 0;
        let key = CardKey::of(state, self.lightbox.sheet_open());
        let early = self.key != Some(key) && !self.lines.is_empty();
        if early {
            self.invalidate_lines();
        }
        let mut paints = 1;
        // `Painted` borrows the lightbox: its damage is copied out before anything else.
        let mut copied = [DeviceRect::default(); MAX_DAMAGE];
        let painted = self.lightbox.paint(state, now_ms, Output::Xrgb(canvas));
        let captions = painted.captions;
        let n = painted.damage.len().min(MAX_DAMAGE);
        copied[..n].copy_from_slice(&painted.damage[..n]);
        self.note(&copied[..n]);
        // Ruling P5: none while the sheet shows, open or fading.
        let sheet = self.lightbox.sheet_open()
            || self.lightbox.choreography().moment(now_ms).sheet.is_some();
        let shown = if sheet { Captions::default() } else { captions };
        let print = shown.fingerprint();
        let canvas_rect = self.lightbox.tableau().canvas();
        let changed = self.shown != Some(print);
        // A cold paint damages exactly the canvas: nothing old is left to erase.
        if changed && !early && !self.lines.is_empty() && !self.damage().contains(&canvas_rect) {
            self.invalidate_lines();
            let again = self.lightbox.paint(state, now_ms, Output::Xrgb(canvas));
            let n = again.damage.len().min(MAX_DAMAGE);
            copied[..n].copy_from_slice(&again.damage[..n]);
            self.note(&copied[..n]);
            paints = 2;
        }
        // The sheet as painted: a chip chosen in it closed it inside the paint (ruling 11).
        self.key = Some(CardKey {
            sheet: self.lightbox.sheet_open(),
            ..key
        });
        let paint = started.elapsed();
        let started = Instant::now();
        let easel = *self.lightbox.easel();
        let (width, height) = (easel.width as usize, easel.height as usize);
        let palette = self.theme.palette();
        if changed {
            let typesetter = &self.typesetter;
            lay_out(
                shown.as_slice(),
                canvas_rect.right,
                easel.px_per_pt,
                |text, style| typesetter.measure(text, style),
                &mut self.lines,
            );
            self.shown = Some(print);
            self.note_lines(canvas_rect);
        }
        // Every shown line, every paint, so the cache keeps exactly them. With none shown (the
        // sheet) the cache keeps the last ones, so they come back without Core Text.
        if !self.lines.is_empty() {
            let clip = if changed {
                Clip::Whole
            } else {
                Clip::Within(&self.damage[..self.damage_len])
            };
            self.typesetter
                .ink(&self.lines, palette, canvas, width, height, clip);
        }
        StageTimes {
            paint,
            captions: started.elapsed(),
            paints,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen::{BOARD_CELLS, Piece, SaveData, VISIBLE_TOOLBOX_SLOTS};

    const SETTLED: f64 = f64::INFINITY;

    fn state(slot: usize) -> GameState<'static> {
        let data = crate::CHROMATRON_1.data;
        let mut s = GameState::new(data, SaveData::fresh(lumen::RuleSet::of(data.game)));
        s.goto_slot(slot);
        s
    }

    fn easel(w: u32, h: u32, ppp: f32) -> Easel {
        Easel::new(w, h, ppp)
    }

    fn canvas(e: Easel) -> Vec<u32> {
        vec![0; (e.width * e.height) as usize]
    }

    /// A fresh stage's settled paint of `state`: what any sequence must end on.
    fn fresh(e: Easel, state: &GameState<'static>) -> Vec<u32> {
        let mut stage = ModernStage::new(e, Theme::Dark, false);
        let mut px = canvas(e);
        stage.paint(state, SETTLED, &mut px);
        px
    }

    /// Paints frame by frame until nothing animates.
    fn settle(stage: &mut ModernStage, st: &GameState<'static>, px: &mut [u32], mut now: f64) {
        for _ in 0..400 {
            stage.paint(st, now, px);
            match stage.next_wake(now) {
                Some(at) => now = at,
                None => return,
            }
        }
        panic!("still animating after 400 frames");
    }

    #[test]
    fn captions_ink_only_inside_their_rects() {
        let e = easel(1280, 800, 1.0);
        let st = state(0);
        let mut stage = ModernStage::new(e, Theme::Dark, false);
        let mut px = canvas(e);
        stage.paint(&st, SETTLED, &mut px);
        let mut bare = canvas(e);
        Lightbox::new(e, Theme::Dark, false).paint(&st, SETTLED, Output::Xrgb(&mut bare));
        let rects: Vec<DeviceRect> = stage.caption_rects().collect();
        assert!(rects.len() >= 2, "a name and a hint at least: {rects:?}");
        let mut changed = 0;
        for (i, (a, b)) in px.iter().zip(&bare).enumerate() {
            if a != b {
                changed += 1;
                let (x, y) = ((i % 1280) as i32, (i / 1280) as i32);
                assert!(
                    rects
                        .iter()
                        .any(|r| r.left <= x && x < r.right && r.top <= y && y < r.bottom),
                    "({x}, {y})"
                );
            }
        }
        assert!(changed > 100, "text was inked");
    }

    #[test]
    fn a_level_change_leaves_no_old_ink() {
        let e = easel(1280, 800, 1.0);
        for (from, to) in [(3, 4), (4, 5), (0, 1)] {
            let mut st = state(from);
            let mut stage = ModernStage::new(e, Theme::Dark, false);
            let mut px = canvas(e);
            stage.paint(&st, SETTLED, &mut px);
            st.goto_slot(to);
            stage.paint(&st, SETTLED, &mut px);
            assert!(px == fresh(e, &st), "slot {from} → {to}");
        }
    }

    #[test]
    fn a_level_change_with_motion_settles_to_the_fresh_picture() {
        let e = easel(1280, 800, 1.0);
        let mut st = state(3);
        let mut stage = ModernStage::new(e, Theme::Dark, false);
        let mut px = canvas(e);
        stage.paint(&st, 0.0, &mut px);
        st.goto_slot(4);
        settle(&mut stage, &st, &mut px, 1000.0);
        assert!(px == fresh(e, &st));
    }

    #[test]
    fn a_piece_dragged_over_the_card_leaves_no_trace() {
        let e = easel(1280, 800, 1.0);
        let mut st = state(0);
        let mut stage = ModernStage::new(e, Theme::Dark, false);
        let mut px = canvas(e);
        stage.paint(&st, 0.0, &mut px);
        let t = stage.lightbox().tableau().clone();
        let slot = (BOARD_CELLS..BOARD_CELLS + VISIBLE_TOOLBOX_SLOTS)
            .find(|&i| st.board().cells[i].piece != Piece::Empty)
            .expect("a tray piece");
        let from = t.cell_centre(slot);
        let card = t.device(t.card);
        let over = ((card.left + card.right) / 2, (card.top + card.bottom) / 2);
        let mut now = 100.0;
        let send = |stage: &mut ModernStage, st: &mut GameState<'static>, ev: InputEvent| {
            if let Route::Game(ev) = stage.route(ev) {
                st.handle_on(stage.lightbox(), ev);
            }
        };
        send(
            &mut stage,
            &mut st,
            InputEvent::MouseDown {
                x: from.x as i32,
                y: from.y as i32,
                alt: false,
            },
        );
        for k in 1..=10 {
            let (x, y) = (
                from.x as i32 + (over.0 - from.x as i32) * k / 10,
                from.y as i32 + (over.1 - from.y as i32) * k / 10,
            );
            send(&mut stage, &mut st, InputEvent::MouseDragged { x, y });
            now += 16.7;
            stage.paint(&st, now, &mut px);
        }
        assert!(st.drag().is_some(), "held over the card");
        send(
            &mut stage,
            &mut st,
            InputEvent::MouseDragged {
                x: from.x as i32,
                y: from.y as i32,
            },
        );
        send(
            &mut stage,
            &mut st,
            InputEvent::MouseUp {
                x: from.x as i32,
                y: from.y as i32,
                alt: false,
            },
        );
        settle(&mut stage, &st, &mut px, now + 16.7);
        assert_eq!(
            st.board().cells[slot].piece,
            state(0).board().cells[slot].piece,
            "put back"
        );
        assert!(px == fresh(e, &st));
    }

    #[test]
    fn the_sheet_hides_the_captions_and_they_come_back() {
        let e = easel(1600, 1200, 2.0); // 800 × 600 pt: the picker is a sheet
        let st = state(0);
        let mut stage = ModernStage::new(e, Theme::Dark, false);
        let mut px = canvas(e);
        stage.paint(&st, 0.0, &mut px);
        let shown = stage.caption_rects().count();
        assert!(shown > 0 && !stage.has_inline_picker());
        assert!(stage.command(Control::Levels));
        stage.paint(&st, 100.0, &mut px);
        assert_eq!(stage.caption_rects().count(), 0, "under the sheet");
        settle(&mut stage, &st, &mut px, 116.7);
        assert!(stage.command(Control::CloseSheet));
        stage.paint(&st, 1000.0, &mut px);
        assert_eq!(
            stage.caption_rects().count(),
            0,
            "not over the fading panel (ruling P5)"
        );
        settle(&mut stage, &st, &mut px, 1016.7);
        assert_eq!(
            stage.caption_rects().count(),
            shown,
            "back once the fade ends"
        );
        assert!(px == fresh(e, &st));
    }

    #[test]
    fn a_resize_relays_the_captions_at_the_new_scale() {
        let st = state(0);
        let small = easel(1280, 800, 1.0);
        let big = easel(2560, 1600, 2.0);
        let mut stage = ModernStage::new(small, Theme::Dark, false);
        let mut px = canvas(small);
        stage.paint(&st, SETTLED, &mut px);
        stage.resize(big);
        let mut px = canvas(big);
        stage.paint(&st, SETTLED, &mut px);
        assert!(px == fresh(big, &st));
        stage.resize(small);
        let mut px = canvas(small);
        stage.paint(&st, SETTLED, &mut px);
        assert!(px == fresh(small, &st));
    }

    /// Ruling P4: a new card erases the old ink in the same lightbox paint (no second paint),
    /// and the frame is still the fresh picture.
    #[test]
    fn a_level_change_paints_the_lightbox_once() {
        let e = easel(1280, 800, 1.0);
        let mut st = state(3);
        let mut stage = ModernStage::new(e, Theme::Dark, false);
        let mut px = canvas(e);
        assert_eq!(stage.paint(&st, SETTLED, &mut px).paints, 1);
        st.goto_slot(4);
        assert_eq!(stage.paint(&st, SETTLED, &mut px).paints, 1);
        assert!(px == fresh(e, &st));
        assert_eq!(stage.paint(&st, SETTLED, &mut px).paints, 1, "idle");
    }

    /// Ruling P4's fallback: captions that change while the card key stays (no level change
    /// does this today, so the key is forged) take a second paint and still leave no old ink.
    #[test]
    fn a_caption_change_without_a_new_card_paints_twice() {
        let e = easel(1280, 800, 1.0);
        let mut st = state(3);
        let mut stage = ModernStage::new(e, Theme::Dark, false);
        let mut px = canvas(e);
        stage.paint(&st, SETTLED, &mut px);
        st.goto_slot(4);
        stage.key = Some(CardKey::of(&st, false));
        assert_eq!(stage.paint(&st, SETTLED, &mut px).paints, 2);
        assert!(px == fresh(e, &st));
    }

    /// Whether `r` lies inside the union of `damage` (each of its pixels covered).
    fn covered(r: DeviceRect, damage: &[DeviceRect]) -> bool {
        (r.top..r.bottom).all(|y| {
            (r.left..r.right).all(|x| {
                damage
                    .iter()
                    .any(|d| d.left <= x && x < d.right && d.top <= y && y < d.bottom)
            })
        })
    }

    /// `damage()` names every pixel the paint changed: the captions back after the sheet's fade
    /// are inked whole, partly outside the lightbox's own damage (the card reaches past the
    /// panel at 800 × 600 pt), and their rects must be in it for a damage-only present.
    #[test]
    fn newly_inked_captions_are_in_the_damage() {
        let e = easel(1600, 1200, 2.0);
        let st = state(0);
        let mut stage = ModernStage::new(e, Theme::Dark, false);
        let mut px = canvas(e);
        stage.paint(&st, 0.0, &mut px);
        assert!(stage.command(Control::Levels));
        settle(&mut stage, &st, &mut px, 16.7);
        assert!(stage.command(Control::CloseSheet));
        let mut now = 1000.0;
        for _ in 0..400 {
            stage.paint(&st, now, &mut px);
            if stage.caption_rects().count() > 0 {
                break;
            }
            now = stage
                .next_wake(now)
                .expect("the captions come back once the fade ends");
        }
        let rects: Vec<DeviceRect> = stage.caption_rects().collect();
        assert!(!rects.is_empty(), "the captions came back");
        for r in rects {
            assert!(
                covered(r, stage.damage()),
                "{r:?} not in {:?}",
                stage.damage()
            );
        }
    }

    /// A chip chosen in the sheet closes it inside the paint (ruling 11); a settled paint (no
    /// fade) brings the captions back in that paint, and the next frame has nothing to redo.
    #[test]
    fn a_chip_closing_the_sheet_leaves_the_next_frame_idle() {
        let e = easel(1600, 1200, 2.0);
        let mut st = state(0);
        let mut stage = ModernStage::new(e, Theme::Dark, false);
        let mut px = canvas(e);
        stage.paint(&st, 0.0, &mut px);
        assert!(stage.command(Control::Levels));
        settle(&mut stage, &st, &mut px, 16.7);
        let chip = stage.lightbox().tableau().sheet_chip_rect(0);
        let (x, y) = ((chip.left + chip.right) / 2, (chip.top + chip.bottom) / 2);
        for ev in [
            InputEvent::MouseDown { x, y, alt: false },
            InputEvent::MouseUp { x, y, alt: false },
        ] {
            if let Route::Game(ev) = stage.route(ev) {
                st.handle_on(stage.lightbox(), ev);
            }
        }
        stage.paint(&st, SETTLED, &mut px);
        assert!(!stage.sheet_open() && stage.caption_rects().count() > 0);
        stage.paint(&st, SETTLED, &mut px);
        assert_eq!(stage.damage(), &[], "nothing to recompose or re-ink");
    }

    #[test]
    fn chromatron_1s_credits_are_joined() {
        let e = easel(1280, 800, 1.0);
        let mut stage = ModernStage::new(e, Theme::Dark, false);
        stage.paint(&state(0), SETTLED, &mut canvas(e));
        assert!(stage.lines.iter().any(|l| l.text.contains(" · ")));
    }

    #[test]
    fn escape_closes_only_an_open_sheet() {
        assert!(escape_closes_sheet(Some(ESCAPE), true));
        assert!(!escape_closes_sheet(Some(ESCAPE), false));
        assert!(!escape_closes_sheet(Some(b'r'), true));
        assert!(!escape_closes_sheet(None, true));
    }

    #[test]
    fn device_points_floor_and_clamp() {
        assert_eq!(device_point(10.9, -0.1), (10, -1));
        assert_eq!(device_point(f64::NAN, 1e12), (0, 65_536));
        assert_eq!(device_point(f64::NEG_INFINITY, 3.0), (-65_536, 3));
    }
}
