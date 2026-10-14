//! `lightbox`: the façade both shells use (spec §5.2). `paint` diffs the game against the last
//! painted state (D5), hands the diff to its `choreography` as cues (spec §9; `next_wake` says
//! when they want the next frame), repaints the stale layers, composes only the damaged rects
//! into the shell's buffer (on a level change, its board region cross-fading from the frame
//! shown before), draws the lifted piece and the level sheet (open, or fading in or out) on
//! top (ruling 9: the sheet is drawn onto the output, never into `base`) and reports the card's
//! captions (spec §10) for the shell to typeset. `route` sends pointer events to the canvas's
//! controls or the game, and `command` opens and closes the level sheet. It implements `Floorplan`, so the shells hand it
//! to `GameState::handle_on`. Allocation-free after `new` and `resize`.

use std::ops::Range;

use lumen::layout::STRIP_SLOTS;
use lumen::{
    BOARD_CELLS, BeamField, Board, Cell, Drag, Floorplan, GameState, Hit, InputEvent, Piece,
    SlotState, TextBoxes, VISIBLE_TOOLBOX_SLOTS,
};
use spectrograph::Game;

use crate::captions::{Captions, caption_card};
use crate::choreography::{Choreography, Observed, SHEET_RISE_PT, SheetPose};
use crate::darkroom::{
    Darkroom, Output, Pose, SHEET_SHADOW_BLUR, SHEET_SHADOW_DROP, Scene, UNDERLAY_REACH,
    UNDERLAY_SCALE, grow,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::etch::Layer;
use crate::etch::{Pt, RgbaBytes, Target, clip_rect, pt};
use crate::flourish::{self, Cast, MAX_FLOURISH};
use crate::masonry::draw_tile;
use crate::piece_art::{draw_body, turn_period};
use crate::spectrum::{Palette, Rgba, Theme};
use crate::tableau::{Control, Tableau};
use crate::{DeviceRect, Easel, Patchwork, lettering, vitrine};

/// Most damage rects a paint reports; past it, the two closest merge into their hull.
pub const MAX_DAMAGE: usize = 8;

/// The lifted piece's scale over its resting size: with a mouse, and under a finger.
const LIFT: f32 = 1.08;
const LIFT_TOUCH: f32 = 1.3;

/// The lifted piece's shadow (spec §7.5), in units of the lifted side `ku`: an ellipse 0.9 × 0.5,
/// 0.16 down, blurred by 0.1 — a Canvas `filter: blur(r)` (as the comp draws it), so σ = r
/// (spec §8).
const SHADOW_RADII: (f32, f32) = (0.45, 0.25);
const SHADOW_DROP: f32 = 0.16;
const SHADOW_SIGMA: f32 = 0.1;
/// How far from the piece's centre the shadow reaches (`2σ` past the ellipse), in `ku`.
const SHADOW_REACH: f32 = 0.66;
const _: () = assert!(
    SHADOW_REACH >= 0.6
        && SHADOW_RADII.0 + 2.0 * SHADOW_SIGMA <= SHADOW_REACH
        && SHADOW_DROP + SHADOW_RADII.1 + 2.0 * SHADOW_SIGMA <= SHADOW_REACH,
    "the lift box holds the tile, the body and the shadow"
);
const _: () = assert!(
    LIFT <= UNDERLAY_SCALE && LIFT_TOUCH <= UNDERLAY_SCALE && SHADOW_REACH <= UNDERLAY_REACH,
    "the darkroom's underlay holds every land box"
);

/// More changed board cells than this (a new level) repaint the whole board and its light.
const MAX_STALE_CELLS: usize = 32;

/// The visible tray slots' cell indices.
const TRAY: Range<usize> = BOARD_CELLS..BOARD_CELLS + VISIBLE_TOOLBOX_SLOTS;

/// What a paint changed, in device px, and the card's captions (borrowing lumen's text). The
/// shell copies the damage before calling anything else, and re-sets its typeset text only when
/// `captions_changed` (always on a cold paint).
pub struct Painted<'s, 'a> {
    pub damage: &'s [DeviceRect],
    pub captions: Captions<'a>,
    pub captions_changed: bool,
}

/// Where a pointer event goes (spec §5.2): to the game (possibly rewritten), to a control, or
/// nowhere (a press on a control and its moves).
#[derive(Debug)]
pub enum Route {
    Game(InputEvent),
    Control(Control),
    Consumed,
}

struct Damage {
    patches: Patchwork<MAX_DAMAGE>,
    canvas: DeviceRect,
}

impl Damage {
    fn clear(&mut self) {
        self.patches.clear();
    }

    /// Adds `rect` clipped to the canvas; past `MAX_DAMAGE` rects the closest merge (M2(g)). The
    /// whole canvas swallows every other rect.
    fn add(&mut self, rect: DeviceRect) {
        if let Some(rect) = clip_rect(rect, self.canvas) {
            self.patches.add(rect);
        }
    }

    fn rects(&self) -> &[DeviceRect] {
        self.patches.as_slice()
    }
}

/// The last painted state, copied with `clone_from` (fixed arrays, no allocation).
struct Remembered {
    board: Board,
    field: BeamField,
    drag: Option<Drag>,
    game: Game,
    slot: usize,
    won: bool,
    strip: [SlotState; STRIP_SLOTS],
    /// The card's key: the slot, whether it is won and whether Next ▶ shows.
    card: (usize, bool, bool),
    /// The win header's reveal and fade (settled `(1, 1)`).
    header: (f32, f32),
    /// The level sheet was open.
    sheet: bool,
    /// The sheet's panel was fading in or out: the next paint redraws its box.
    sheet_moving: bool,
    /// The drop target under the held piece.
    drop: Option<usize>,
    /// The cell whose piece was mid-turn: the next paint redraws it.
    turning: Option<usize>,
    /// The cell a piece was landing in (shown empty): the next paint redraws it once it differs.
    landing: Option<usize>,
    /// The board region was cross-fading: the next paint recomposes it.
    fading: bool,
}

pub struct Lightbox {
    easel: Easel,
    theme: Theme,
    tableau: Tableau,
    darkroom: Darkroom,
    last: Box<Remembered>,
    cold: bool,
    damage: Damage,
    /// The cues the paints' diffs started (spec §9).
    choreography: Box<Choreography>,
    /// The last paint's captions' fingerprint.
    captions_seen: u64,
    /// The level sheet is open (only ever with a sheet picker).
    sheet: bool,
    /// A chip chosen in the sheet: it closes at the next paint (ruling 11).
    close_pending: bool,
    /// The control a press went down on; it fires if the release is on it too.
    pressed: Option<Control>,
    /// A press on a control was dropped by `resize`: its release is swallowed, since lumen acts
    /// on a release without a press (it turns a piece or opens a level).
    swallow_up: bool,
    /// Next ▶ showed in the last paint, so it answers presses.
    next_shown: bool,
    /// The boxes the last paint's flourish, rising held piece and landing piece drew into: the
    /// next paint recomposes them.
    flourished: Patchwork<MAX_FLOURISH>,
    /// This paint's flourish, worked out once and drawn into each damaged rect.
    cast: Box<Cast>,
    /// Diagnostics (ruling M6): every frame repainted whole.
    full_frames: bool,
}

impl Lightbox {
    pub fn new(easel: Easel, theme: Theme, reduced_motion: bool) -> Lightbox {
        // Every small long-lived box (`last`, the choreography) before the darkroom's big buffers.
        // A small box made after them sits above them in the heap, so when a resize frees them
        // they can't merge back into the heap's top, and the next layout's buffers grow wasm
        // memory instead: a phone turned back and forth goes past its budget (spec §14). The
        // flourish's `cast` (about 280 KB) is one of the big ones: made first, it does the same.
        let last = Box::new(Remembered {
            board: Board::default(),
            field: BeamField::default(),
            drag: None,
            game: Game::C1,
            slot: 0,
            won: false,
            strip: [SlotState::Locked; STRIP_SLOTS],
            card: (0, false, false),
            header: (1.0, 1.0),
            sheet: false,
            sheet_moving: false,
            drop: None,
            turning: None,
            landing: None,
            fading: false,
        });
        let mut choreography = Choreography::new();
        choreography.set_reduced(reduced_motion);
        let tableau = Tableau::arrange(&easel);
        let darkroom = Darkroom::new(&tableau, theme.palette());
        let cast = Cast::new();
        let canvas = tableau.canvas();
        Lightbox {
            easel,
            theme,
            tableau,
            darkroom,
            last,
            cold: true,
            damage: Damage {
                patches: Patchwork::new(),
                canvas,
            },
            choreography,
            captions_seen: 0,
            sheet: false,
            close_pending: false,
            pressed: None,
            swallow_up: false,
            next_shown: false,
            flourished: Patchwork::new(),
            cast,
            full_frames: false,
        }
    }

    /// Diagnostics (ruling M6): every frame repainted whole at its moment, cues still running, so
    /// tests can hold incremental frames to it.
    #[doc(hidden)]
    pub fn set_full_frames(&mut self, on: bool) {
        self.full_frames = on;
    }

    /// Re-arranges for `easel`; the next paint is cold. An inline picker shuts the sheet, and a
    /// held press on a control is dropped (the control may have moved) with its release.
    pub fn resize(&mut self, easel: Easel) {
        self.easel = easel;
        self.tableau = Tableau::arrange(&easel);
        self.darkroom.reshape(&self.tableau, self.theme.palette());
        self.damage.canvas = self.tableau.canvas();
        self.cold = true;
        if self.tableau.picker.is_some() {
            self.sheet = false;
            self.close_pending = false;
        }
        if self.pressed.take().is_some() {
            self.swallow_up = true;
        }
    }

    /// Routes a pointer event (spec §5.2, §6 "Hit testing"): the canvas's controls come first.
    /// A press on a control and its moves are consumed; its release fires the control only over
    /// the same control (a press dropped by `resize` swallows its release). A release over a chip
    /// is passed on as primary (lumen ignores alternate strip releases, and a clockwise tap on a
    /// chip still opens its level); a chip chosen in the sheet closes it at the next paint,
    /// unless it was locked as last painted (ruling 11).
    pub fn route(&mut self, event: InputEvent) -> Route {
        let at = |s: &Self, x, y| s.control_at(x, y);
        match event {
            InputEvent::MouseDown { x, y, alt } => {
                self.swallow_up = false;
                self.pressed = at(self, x, y);
                if self.pressed.is_some() {
                    Route::Consumed
                } else {
                    Route::Game(InputEvent::MouseDown { x, y, alt })
                }
            }
            InputEvent::MouseDragged { .. } if self.pressed.is_some() || self.swallow_up => {
                Route::Consumed
            }
            InputEvent::MouseUp { .. } if self.swallow_up => {
                self.swallow_up = false;
                Route::Consumed
            }
            InputEvent::MouseUp { x, y, alt } => {
                if let Some(control) = self.pressed.take() {
                    return if at(self, x, y) == Some(control) {
                        Route::Control(control)
                    } else {
                        Route::Consumed
                    };
                }
                if let Hit::Strip(slot) = self.hit(x, y) {
                    if self.sheet
                        && self
                            .last
                            .strip
                            .get(slot)
                            .is_some_and(|s| *s != SlotState::Locked)
                    {
                        self.close_pending = true;
                    }
                    return Route::Game(InputEvent::MouseUp { x, y, alt: false });
                }
                Route::Game(InputEvent::MouseUp { x, y, alt })
            }
            other => Route::Game(other),
        }
    }

    /// Carries out a control the shell got from `route` (or its toolbar). True when the picture
    /// changes and the shell should paint again: the sheet opened or closed. `Next` is the
    /// shell's (it sends the game `=`), so here it changes nothing.
    pub fn command(&mut self, control: Control) -> bool {
        match control {
            Control::Levels if self.tableau.picker.is_none() => {
                self.sheet = !self.sheet;
                self.close_pending = false;
                true
            }
            Control::CloseSheet if self.sheet => {
                self.sheet = false;
                self.close_pending = false;
                true
            }
            _ => false,
        }
    }

    /// A cancelled pointer (the shell's pointercancel): the control it went down on never fires.
    /// No release follows a cancel, so nothing is swallowed; a release that did arrive would go
    /// to the game.
    pub fn forget_press(&mut self) {
        self.pressed = None;
    }

    /// The level picker is drawn on the canvas itself, so there is no sheet (nor Levels pill).
    pub fn has_inline_picker(&self) -> bool {
        self.tableau.picker.is_some()
    }

    pub fn sheet_open(&self) -> bool {
        self.sheet
    }

    /// A chip was chosen in the open sheet: it closes at the next paint (ruling 11).
    pub fn sheet_closing(&self) -> bool {
        self.close_pending
    }

    /// The control under (`x`, `y`) as `route` would see a press there now.
    pub fn control_at(&self, x: i32, y: i32) -> Option<Control> {
        self.tableau.control_at(x, y, self.sheet, self.next_shown)
    }

    /// Switches the theme; the next paint is cold and redraws every sprite.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.darkroom.reshape(&self.tableau, theme.palette());
        self.cold = true;
    }

    /// Reduced motion (spec §9.3): a running cue settles at once.
    pub fn set_reduced_motion(&mut self, on: bool) {
        self.choreography.set_reduced(on);
    }

    /// Makes the next paint cold and settled.
    pub fn reset(&mut self) {
        self.cold = true;
    }

    /// Paints `state` and reports its card's captions.
    pub fn paint<'s, 'a>(
        &'s mut self,
        state: &GameState<'a>,
        now_ms: f64,
        out: Output<'_>,
    ) -> Painted<'s, 'a> {
        let texts = lumen::text_boxes(state);
        self.paint_with(Scene::of(state), &texts, now_ms, out)
    }

    /// Paints a scene with no game text around it (the pieces sheet, the stills): no captions.
    pub fn paint_scene<'s>(
        &'s mut self,
        scene: Scene<'_>,
        now_ms: f64,
        out: Output<'_>,
    ) -> Painted<'s, 'static> {
        self.paint_with(scene, &TextBoxes::default(), now_ms, out)
    }

    fn paint_with<'s, 'a>(
        &'s mut self,
        scene: Scene<'_>,
        texts: &TextBoxes<'a>,
        now_ms: f64,
        mut out: Output<'_>,
    ) -> Painted<'s, 'a> {
        if self.close_pending {
            self.sheet = false;
            self.close_pending = false;
        }
        // D5: the diff becomes cues. A cold paint (and a non-finite `now`) is settled (M7).
        if self.cold || self.last.game != scene.game || !now_ms.is_finite() {
            self.choreography.clear();
        } else {
            let last = &*self.last;
            let seen = Observed {
                board: [&last.board, scene.board],
                field: [&last.field, scene.field],
                drag: [last.drag, scene.drag],
                slot: [last.slot, scene.slot],
                won: [last.won, scene.won],
                sheet: [last.sheet, self.sheet],
            };
            self.choreography.observe(&seen, now_ms);
        }
        // A level cue's first paint: keep the frame still shown, to fade from (M5); only then.
        if self.choreography.keep_for_fade() {
            self.darkroom.keep_for_fade(&out);
        }
        self.damage.clear();
        let moment = self.choreography.moment(now_ms);
        // A kind that never turns is never posed turning (whatever now stands in the cell).
        let turn = moment
            .turn
            .filter(|&(i, _)| turn_period(scene.board.cells[i].piece) != 0);
        let turning = turn.map(|t| t.0);
        // A released piece sliding from the release point into its cell (ruling M10(c)), from
        // the size and shadow it had risen to down to its cell's, its shadow fading. Its cell
        // shows empty in `base`, and its body is laid into `base` where it is for this frame's
        // composes (`lay`, put back by `unlay`), under the light as at rest; its light is in the
        // light tile where the body is, moving with it, so it has its glow all the way in.
        let settling = moment.land.filter(|l| l.cell < TRAY.end).map(|l| {
            let q = l.progress;
            let end = rest_centre(&self.tableau, l.cell);
            let from = pt(l.from.0 as f32, l.from.1 as f32);
            let start = held_side(&self.tableau, &self.easel, l.lift);
            let size = cell_size(&self.tableau, l.cell);
            let ku = start + (size - start) * q;
            let centre = from + (end - from) * q;
            let lifted = Lifted {
                game: scene.game,
                piece: l.piece,
                centre,
                ku,
                shadow: l.lift * (1.0 - q),
                drop: None,
            };
            (lifted, (l.cell, centre - end, ku / size))
        });
        let landing = settling.as_ref().map(|s| s.1.0);
        // A win writes its header on, or (reduced motion) fades it in. Once every glyph is
        // whole the reveal draws the settled header, so it is posed settled: the card's key
        // and lettering go back to theirs and the card stops repainting.
        // (A win text the lettering cannot write is typeset: nothing is lettered to reveal.)
        let lettered = if lettering::letterable(scene.you_win) {
            scene.you_win
        } else {
            ""
        };
        let header = moment.win.map_or((1.0, 1.0), |w| {
            let whole = lettering::reveal_whole(lettered, w.reveal);
            (if whole { 1.0 } else { w.reveal }, w.header_alpha)
        });
        self.darkroom.set_pose(Pose {
            turn,
            landing: settling.as_ref().map(|s| s.1),
            reveal: header.0,
            header_alpha: header.1,
        });
        let settling = settling.map(|s| s.0);
        let view = moment.sweep;
        // Before the cold/incremental split: a cold paint mid-drag draws the drop target too.
        let drop = self.drop_cell(&scene);
        let palette = self.theme.palette();
        let tableau = &self.tableau;
        let last = &mut *self.last;
        let card = (scene.slot, scene.won, scene.next_open());
        let cold = self.cold || last.game != scene.game;
        if cold || self.full_frames {
            self.darkroom
                .paint_all_at(tableau, palette, &scene, view.as_ref());
            self.damage.add(tableau.canvas());
        } else {
            let source = scene.drag.map(|d| d.source);
            let was = last.drag.map(|d| d.source);
            let lifted = |range: Range<usize>| {
                source != was
                    && (source.is_some_and(|s| range.contains(&s))
                        || was.is_some_and(|s| range.contains(&s)))
            };
            // A landing that starts or ends: its cell shows empty, or its piece again.
            let landed = |range: Range<usize>| {
                landing != last.landing
                    && (landing.is_some_and(|l| range.contains(&l))
                        || last.landing.is_some_and(|l| range.contains(&l)))
            };
            let board = scene.board.cells[..BOARD_CELLS] != last.board.cells[..BOARD_CELLS]
                || *scene.field != last.field
                || lifted(0..BOARD_CELLS)
                || turning.is_some()
                || last.turning.is_some()
                || landed(0..BOARD_CELLS);
            let mut whole_light = false;
            if board {
                // The cells whose pieces changed, were lifted or set down, are turning (or have
                // just stopped), or start or stop showing a landing piece's empty cell.
                let mut stale = [false; BOARD_CELLS];
                let (now, then) = (&scene.board.cells, &last.board.cells);
                for (i, s) in stale.iter_mut().enumerate() {
                    *s = now[i] != then[i]
                        || (source != was && (source == Some(i) || was == Some(i)))
                        || turning == Some(i)
                        || last.turning == Some(i)
                        || (landing != last.landing
                            && (landing == Some(i) || last.landing == Some(i)));
                }
                let count = stale.iter().filter(|&&s| s).count();
                let walls = (0..BOARD_CELLS).any(|i| {
                    stale[i] && (now[i].piece == Piece::Wall || then[i].piece == Piece::Wall)
                });
                if walls || count > MAX_STALE_CELLS {
                    self.darkroom.paint_board(tableau, palette, &scene);
                    self.damage.add(self.darkroom.board_damage());
                    whole_light = true;
                } else {
                    self.darkroom.paint_cells(tableau, palette, &scene, &stale);
                    for (i, _) in stale.iter().enumerate().filter(|&(_, &s)| s) {
                        self.damage.add(tableau.cell_rect(i));
                    }
                }
            }
            // The light only where its items changed (ruling M2), or whole with the board.
            for &r in
                self.darkroom
                    .light_frame(tableau, palette, &scene, view.as_ref(), whole_light)
            {
                self.damage.add(r);
            }
            if scene.board.cells[TRAY] != last.board.cells[TRAY] || lifted(TRAY) || landed(TRAY) {
                self.darkroom.paint_tray(tableau, palette, &scene);
                self.damage.add(self.darkroom.tray_damage());
            }
            if scene.strip != last.strip {
                self.darkroom.paint_picker(tableau, palette, &scene);
                self.damage.add(self.darkroom.picker_damage());
            }
            if card != last.card || header != last.header {
                self.darkroom.paint_card(tableau, palette, &scene);
                self.damage.add(self.darkroom.card_damage());
            }
            if scene.drag != last.drag {
                for drag in [last.drag, scene.drag].into_iter().flatten() {
                    self.damage.add(lift_box(tableau, &self.easel, drag));
                }
            }
            if drop != last.drop {
                for cell in [last.drop, drop].into_iter().flatten() {
                    self.damage.add(drop_box(tableau, cell));
                }
            }
            // The sheet's scrim covers the canvas; its chips follow the slots' states (redrawn
            // over the panel grown by its shadow's 40-pt blur).
            if self.sheet != last.sheet {
                self.damage.add(tableau.canvas());
            } else if self.sheet && scene.strip != last.strip {
                let reach = (40.0 * tableau.px_per_pt()).ceil() as i32;
                self.damage
                    .add(grow(tableau.device(tableau.sheet.panel), reach));
            }
        }
        // The board region cross-fades on every frame of a level cue, and once more after it,
        // to the new picture exactly.
        let fading = moment.level.is_some();
        if fading || last.fading {
            self.damage.add(self.darkroom.fade_rect());
        }
        // The sheet's panel fades and rises on every frame of its cue, and is redrawn once more
        // after it, shown or gone (its scrim comes and goes with the whole canvas, above).
        let sheet_pose = moment.sheet.unwrap_or(SheetPose::SHOWN);
        let sheet_drawn = self.sheet || moment.sheet.is_some();
        let sheet_moving = moment.sheet.is_some();
        if sheet_moving || last.sheet_moving {
            let rise = (SHEET_RISE_PT * tableau.px_per_pt()).ceil() as i32;
            self.damage.add(sheet_box(tableau, rise));
        }
        // Passing light over the output (M1): repaint where it was and where it is.
        let stage = flourish::Stage {
            tableau,
            board: scene.board,
            field: scene.field,
            reach: self.choreography.reach(),
            bursts: self.choreography.bursts(),
            now: now_ms,
        };
        self.cast.frame(&stage, &moment);
        let mut boxes = *self.cast.boxes();
        // The held piece (spec §7.5) at its lift: 1 → 1.08 (1.3 on touch) as it rises.
        let held = scene.drag.map(|drag| Lifted {
            game: scene.game,
            piece: drag.piece,
            centre: pt(drag.x as f32, drag.y as f32),
            ku: held_side(tableau, &self.easel, moment.lift),
            shadow: moment.lift,
            drop: drop.map(|cell| (tableau.cell_centre(cell), cell_size(tableau, cell))),
        });
        // While it rises its box changes with no move (F5); the box at full lift holds every
        // scale, and the next paint recomposes it as it was.
        if let Some(drag) = scene.drag.filter(|_| moment.lift < 1.0) {
            boxes.add(lift_box(tableau, &self.easel, drag));
        }
        if let Some(l) = &settling {
            boxes.add(land_box(l));
        }
        for &r in self.flourished.as_slice().iter().chain(boxes.as_slice()) {
            self.damage.add(r);
        }
        self.flourished = boxes;
        // The landing piece lies in `base` for this frame's composes, under the light as at rest
        // (its own light at its place, the beams over it), so nothing steps when it settles; the
        // held piece is drawn over the output.
        let laid = settling.as_ref().is_some_and(|l| {
            self.darkroom
                .lay(land_box(l), |t, clip| lift_onto(t, palette, l, clip))
        });
        let over = [settling.as_ref().filter(|_| !laid), held.as_ref()];
        for &rect in self.damage.rects() {
            self.darkroom.compose(rect, &mut out);
            if let Some(k) = moment.level {
                self.darkroom.fade_into(rect, &mut out, k);
            }
            on_output(&mut out, &self.easel, |t| self.cast.draw(t, rect, palette));
            for lifted in over.into_iter().flatten() {
                draw_lifted(&mut out, &self.easel, palette, lifted, rect);
            }
            if sheet_drawn {
                self.darkroom.draw_sheet(
                    tableau,
                    palette,
                    &scene.strip,
                    &mut out,
                    rect,
                    sheet_pose,
                );
            }
        }
        self.darkroom.unlay();
        last.board.clone_from(scene.board);
        last.field.clone_from(scene.field);
        last.drag = scene.drag;
        last.game = scene.game;
        last.slot = scene.slot;
        last.won = scene.won;
        last.strip = scene.strip;
        last.card = card;
        last.header = header;
        last.sheet = self.sheet;
        last.sheet_moving = sheet_moving;
        last.drop = drop;
        last.turning = turning;
        last.landing = landing;
        last.fading = fading;
        self.cold = false;
        self.next_shown = scene.next_open();
        // Spec §7.5: the sheet covers the card, so its captions hide while it is open.
        let captions = if texts.as_slice().is_empty() || self.sheet {
            Captions::default()
        } else {
            caption_card(tableau, texts, scene.slot, scene.won, scene.next_open())
        };
        let print = captions.fingerprint();
        let captions_changed = cold || print != self.captions_seen;
        self.captions_seen = print;
        Painted {
            damage: self.damage.rects(),
            captions,
            captions_changed,
        }
    }

    /// The empty cell under a held piece (not its own source): where it would land. With the
    /// sheet open the board and tray hit nothing, so there is none.
    fn drop_cell(&self, scene: &Scene) -> Option<usize> {
        let drag = scene.drag?;
        match self.hit(drag.x, drag.y) {
            Hit::Cell(i) if i != drag.source && scene.board.cells[i].piece == Piece::Empty => {
                Some(i)
            }
            _ => None,
        }
    }

    /// When the next frame is due: `Some(now + FRAME_MS)` while a cue runs, else `None`.
    pub fn next_wake(&self, now_ms: f64) -> Option<f64> {
        self.choreography.next_wake(now_ms)
    }

    /// The running cues (for tests and the darkroom).
    pub fn choreography(&self) -> &Choreography {
        &self.choreography
    }

    /// Diagnostics: the darkroom, for tests that read what the light tile shows.
    #[doc(hidden)]
    pub fn darkroom(&self) -> &Darkroom {
        &self.darkroom
    }

    pub fn easel(&self) -> &Easel {
        &self.easel
    }

    pub fn tableau(&self) -> &Tableau {
        &self.tableau
    }

    /// Heap held by every buffer, in bytes.
    pub fn buffer_bytes(&self) -> usize {
        self.darkroom.buffer_bytes()
            + size_of::<Remembered>()
            + size_of::<Choreography>()
            + size_of::<Cast>()
    }
}

impl Floorplan for Lightbox {
    /// The tableau's hits; with the sheet open, its chips only (board and tray hit `Nothing`).
    fn hit(&self, x: i32, y: i32) -> Hit {
        if self.sheet {
            self.tableau.hit_sheet(x, y)
        } else {
            self.tableau.hit(x, y)
        }
    }

    fn drag_slop(&self) -> i32 {
        self.tableau.drag_slop()
    }
}

/// The lifted piece's side, in device px.
fn lift_side(tableau: &Tableau, easel: &Easel) -> f32 {
    let k = if easel.touch { LIFT_TOUCH } else { LIFT };
    tableau.cell as f32 * k
}

/// The held piece's side at `lift` (0 → 1, spec §7.5): its cell's growing to `lift_side`,
/// exactly that once risen.
fn held_side(tableau: &Tableau, easel: &Easel, lift: f32) -> f32 {
    if lift >= 1.0 {
        lift_side(tableau, easel)
    } else {
        let k = if easel.touch { LIFT_TOUCH } else { LIFT };
        tableau.cell as f32 * (1.0 + (k - 1.0) * lift)
    }
}

/// What the lifted piece can touch, plus 2 px: its tile and body (0.6ku) and its shadow, which
/// reaches 2σ = 0.2ku past its ellipse (0.65ku across, 0.61ku down).
fn lift_box(tableau: &Tableau, easel: &Easel, drag: Drag) -> DeviceRect {
    let half = (SHADOW_REACH * lift_side(tableau, easel)).ceil() as i32 + 2;
    DeviceRect::new(drag.x - half, drag.y - half, drag.x + half, drag.y + half)
}

/// The sheet's panel with its shadow's reach (40 pt blur ⇒ 2σ = 40 pt, plus the 12 pt drop),
/// `rise_px` lower: what one of its frames can touch.
fn sheet_box(tableau: &Tableau, rise_px: i32) -> DeviceRect {
    let ppp = tableau.px_per_pt();
    let reach = ((SHEET_SHADOW_BLUR + SHEET_SHADOW_DROP) * ppp).ceil() as i32;
    let p = grow(tableau.device(tableau.sheet.panel), reach);
    DeviceRect::new(p.left, p.top, p.right, p.bottom + rise_px)
}

/// Where a cell's piece rests, device px: its cell's centre, or for a tray slot the centre of the
/// sprite `paint_tray_region` blits at whole px (half a px off the slot's centre when the slot
/// and the piece differ by an odd count).
fn rest_centre(tableau: &Tableau, cell: usize) -> Pt {
    if cell < BOARD_CELLS {
        return tableau.cell_centre(cell);
    }
    let (r, s) = (tableau.cell_rect(cell), tableau.tray_piece_px());
    let half = s as f32 / 2.0;
    pt(
        (r.left + (r.right - r.left - s) / 2) as f32 + half,
        (r.top + (r.bottom - r.top - s) / 2) as f32 + half,
    )
}

/// A cell's art size in device px: the board's cell, or a tray piece.
fn cell_size(tableau: &Tableau, cell: usize) -> f32 {
    if cell < BOARD_CELLS {
        tableau.cell as f32
    } else {
        tableau.tray_piece_px() as f32
    }
}

/// The drop target's box: the ring's radius 0.62u plus its width, around the cell's centre.
fn drop_box(tableau: &Tableau, cell: usize) -> DeviceRect {
    let u = cell_size(tableau, cell);
    let c = tableau.cell_centre(cell);
    let r = (0.62 * u + (0.05 * u).max(1.5)).ceil() as i32 + 1;
    let (x, y) = (c.x as i32, c.y as i32);
    DeviceRect::new(x - r, y - r, x + r, y + r)
}

/// A held or landing piece: the piece at `centre`, `ku` px on a side, its shadow at `shadow` of
/// its alpha, and the drop target (a held piece only). A held piece is drawn over the composed
/// picture; a landing one is laid into `base` (`Darkroom::lay`), under the light. Neither draws a
/// light: a landing piece's is the light tile's, where its body is (`Pose::landing`).
struct Lifted {
    game: Game,
    piece: Cell,
    centre: Pt,
    ku: f32,
    shadow: f32,
    drop: Option<(Pt, f32)>,
}

/// What a landing piece can touch, plus 2 px: as `lift_box`, at its own side and centre.
fn land_box(lifted: &Lifted) -> DeviceRect {
    let half = (SHADOW_REACH * lifted.ku).ceil() as i32 + 2;
    let (x, y) = (
        lifted.centre.x.floor() as i32,
        lifted.centre.y.floor() as i32,
    );
    DeviceRect::new(x - half, y - half, x + half + 1, y + half + 1)
}

/// Runs `f` on the output as a `Target`.
fn on_output(out: &mut Output, easel: &Easel, f: impl FnOnce(&mut dyn Target)) {
    let (width, height) = (easel.width as usize, easel.height as usize);
    match out {
        #[cfg(not(target_arch = "wasm32"))]
        Output::Xrgb(px) => f(&mut Layer { px, width, height }),
        Output::Rgba(px) => f(&mut RgbaBytes { px, width, height }),
    }
}

/// Draws the drop target, the lifted piece's shadow and the piece (its tile, then its body)
/// into `out`, only inside `clip`: a held piece, or a landing one `lay` had no room for.
fn draw_lifted(
    out: &mut Output,
    easel: &Easel,
    palette: &Palette,
    lifted: &Lifted,
    clip: DeviceRect,
) {
    on_output(out, easel, |t| lift_onto(t, palette, lifted, clip));
}

/// Spec §7.5: the drop target, then the shadow scaled by the lift `k` (an ellipse 0.9ku ×
/// 0.5ku, σ 0.1ku, 0.16ku down) at `shadow` of its alpha, then the piece at k × its size (its
/// tile, then its body). No light: a landing piece's is in the light tile.
fn lift_onto(
    target: &mut dyn Target,
    palette: &Palette,
    &Lifted {
        game,
        piece,
        centre,
        ku,
        shadow,
        drop,
    }: &Lifted,
    clip: DeviceRect,
) {
    if let Some((centre, u)) = drop {
        vitrine::drop_target(target, clip, palette, centre, u);
    }
    if shadow > 0.0 {
        let colour = Rgba {
            a: palette.drag_shadow.a * shadow,
            ..palette.drag_shadow
        };
        vitrine::soft_ellipse(
            target,
            clip,
            pt(centre.x, centre.y + SHADOW_DROP * ku),
            SHADOW_RADII.0 * ku,
            SHADOW_RADII.1 * ku,
            SHADOW_SIGMA * ku,
            colour,
        );
    }
    draw_tile(target, clip, palette, centre, ku, true);
    draw_body(target, clip, palette, game, piece, centre, ku);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_past_its_room_merges_the_closest_rects() {
        let canvas = DeviceRect::new(0, 0, 100, 100);
        let mut damage = Damage {
            patches: Patchwork::new(),
            canvas,
        };
        for k in 0..MAX_DAMAGE as i32 {
            damage.add(DeviceRect::new(10 * k, 0, 10 * k + 2, 2));
        }
        damage.add(DeviceRect::new(1, 50, 3, 52));
        assert_eq!(damage.rects().len(), MAX_DAMAGE, "a ninth rect merges");
        assert!(
            damage.rects().iter().all(|r| *r != canvas),
            "never the whole canvas"
        );
        damage.add(canvas);
        assert_eq!(damage.rects(), &[canvas], "the canvas swallows the rest");
        damage.clear();
        damage.add(DeviceRect::new(-10, -10, -1, -1));
        assert!(damage.rects().is_empty(), "off-canvas rects are dropped");
    }
}
