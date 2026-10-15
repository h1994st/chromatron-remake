//! Ruling G10: "drag ghosting" is its absence. A dragged piece leaves no ghost: as it moves a
//! few px a frame (while rising, over every region, past the canvas's corners, with a sweep
//! running, then landing), every incremental frame equals a whole repaint at the same moment.
//!
//! `fast` paints every frame (its damage carries the history); `whole` repaints whole at its
//! moment, so it paints only the frames compared. A cue starts only when a lightbox sees a change
//! (a pickup, a turn, a drop), and each such frame is compared, so both see each cue start at the
//! same `now`. Compared: the cold and turn frames, every frame of the rise, each leg's first
//! and last frames, every `SAMPLE`-th frame of a carry, and every frame from a release (a drop
//! off the cell's centre, then a cancel over the card) until the cues end, whose settled frame
//! must equal a cold paint.
//! A stale trail stays in `fast`'s output until something overdraws it, so sampling the carry
//! still finds it (a lift box left out of the damage fails within the first leg or two).
//!
//! Every frame of `fast` also keeps its damage honest: each px the paint changed lies inside a
//! rect of `Painted.damage`, since that is all a presenter copies to the screen (the macOS
//! layer's owed rects, the web's `putImageData`). Neither twin compare nor the paint ledger sees a
//! px recomposed but left out of the report.

mod common;

use halation::{Easel, FRAME_MS, Lightbox, Output, Theme};
use lumen::{BOARD_CELLS, GameState, InputEvent, Piece};
use spectrograph::Game;

/// Frames after the pickup that are all compared: the rise (90 ms) with margin.
const RISE_FRAMES: usize = 16;
/// Elsewhere in a carry, one frame in `SAMPLE` is compared.
const SAMPLE: usize = 3;

struct Twins {
    fast: Lightbox,
    whole: Lightbox,
    a: Vec<u32>,
    b: Vec<u32>,
    /// What a presenter of `fast` shows: its previous frame, updated only inside the damage.
    shown: Vec<u32>,
    now: f64,
    frames: usize,
    /// Frames still to compare one by one after the pickup.
    rising: usize,
}

impl Twins {
    fn new(easel: Easel, theme: Theme) -> Twins {
        let n = (easel.width * easel.height) as usize;
        let mut whole = Lightbox::new(easel, theme, false);
        whole.set_full_frames(true);
        Twins {
            fast: Lightbox::new(easel, theme, false),
            whole,
            a: vec![0; n],
            b: vec![0; n],
            shown: vec![0; n],
            now: 0.0,
            frames: 0,
            rising: 0,
        }
    }

    /// One frame on `fast`; on `whole` too, and compared, when `compare`, while the piece
    /// rises, or on every `SAMPLE`-th frame.
    fn frame(&mut self, state: &GameState, what: &str, compare: bool) {
        self.now += FRAME_MS;
        self.frames += 1;
        let width = self.fast.easel().width as usize;
        let painted = self.fast.paint(state, self.now, Output::Xrgb(&mut self.a));
        contained(
            &self.a,
            &mut self.shown,
            width,
            painted.damage,
            what,
            self.frames,
        );
        let compare = compare || self.rising > 0 || self.frames.is_multiple_of(SAMPLE);
        self.rising = self.rising.saturating_sub(1);
        if compare {
            self.whole.paint(state, self.now, Output::Xrgb(&mut self.b));
            assert!(
                self.a == self.b,
                "{what}, frame {}: a ghost (incremental ≠ whole)",
                self.frames
            );
        }
    }

    /// The pointer carried from `from` to `to` in steps of 1–13 px (cycling), a frame each.
    fn carry(
        &mut self,
        state: &mut GameState,
        from: (i32, i32),
        to: (i32, i32),
        what: &str,
    ) -> (i32, i32) {
        let (mut x, mut y) = from;
        let mut k = 0;
        while (x, y) != to {
            k += 1;
            let step = (k % 13) + 1;
            x += (to.0 - x).clamp(-step, step);
            y += (to.1 - y).clamp(-step, step);
            state.handle_on(&self.fast, InputEvent::MouseDragged { x, y });
            // Each leg's last frame too: a land slides from where its lightbox last saw the
            // piece, so both must see where it is released.
            self.frame(state, what, k == 1 || (x, y) == to);
        }
        to
    }
}

/// Every px where `out` differs from `shown` lies inside some rect of `damage`; `shown` then takes
/// those px, as a presenter copying the damage would.
fn contained(
    out: &[u32],
    shown: &mut [u32],
    width: usize,
    damage: &[halation::DeviceRect],
    what: &str,
    frame: usize,
) {
    for (y, (row, seen)) in out
        .chunks_exact(width)
        .zip(shown.chunks_exact_mut(width))
        .enumerate()
    {
        if row == seen {
            continue;
        }
        for (x, (&px, was)) in row.iter().zip(seen.iter_mut()).enumerate() {
            if px != *was {
                let (x, y) = (x as i32, y as i32);
                assert!(
                    damage
                        .iter()
                        .any(|r| r.left <= x && x < r.right && r.top <= y && y < r.bottom),
                    "{what}, frame {frame}: px ({x}, {y}) changed outside the reported damage"
                );
                *was = px;
            }
        }
    }
}

#[test]
fn a_dragged_piece_leaves_no_ghost() {
    for theme in Theme::ALL {
        for easel in [
            Easel::new(1280, 800, 1.0),
            Easel::new(780, 1480, 2.0).with_touch(true),
        ] {
            let mut state = common::state(Game::C1, 41, true);
            let mut t = Twins::new(easel, theme);
            t.frame(&state, "cold", true);
            let tableau = t.fast.tableau().clone();
            let centre = |i: usize| {
                let r = tableau.cell_rect(i);
                ((r.left + r.right) / 2, (r.top + r.bottom) / 2)
            };
            let movable: Vec<usize> = (0..BOARD_CELLS)
                .filter(|&i| state.board().cells[i].piece.is_movable())
                .collect();
            assert!(
                movable.len() >= 2,
                "C1 slot 41 solved has two movable pieces"
            );
            // A sweep running under the drag: turn another piece first.
            let (tx, ty) = centre(movable[1]);
            state.handle_on(
                &t.fast,
                InputEvent::MouseDown {
                    x: tx,
                    y: ty,
                    alt: true,
                },
            );
            state.handle_on(
                &t.fast,
                InputEvent::MouseUp {
                    x: tx,
                    y: ty,
                    alt: true,
                },
            );
            t.frame(&state, "the turn", true);
            // Pick up the first piece and carry it: over the board, the tray, the card, both
            // canvas corners (past them), and back; it rises over the first frames.
            let from = centre(movable[0]);
            state.handle_on(
                &t.fast,
                InputEvent::MouseDown {
                    x: from.0,
                    y: from.1,
                    alt: false,
                },
            );
            t.rising = RISE_FRAMES;
            let card = tableau.device(tableau.card);
            let empty = (0..BOARD_CELLS)
                .find(|&i| state.board().cells[i].piece == Piece::Empty)
                .expect("an empty cell");
            // Released off its centre, so the land slides it home (a land box left out of the
            // damage leaves a trail along the slide).
            let off_centre = {
                let r = tableau.cell_rect(empty);
                let (cx, cy) = centre(empty);
                (
                    cx + (r.right - r.left) * 3 / 8,
                    cy + (r.bottom - r.top) * 3 / 8,
                )
            };
            let mut at = from;
            for (to, what) in [
                (centre(empty), "over an empty cell"),
                (centre(lumen::toolbox_index(0)), "over the tray"),
                (
                    ((card.left + card.right) / 2, (card.top + card.bottom) / 2),
                    "over the card",
                ),
                ((-6, -6), "past the top-left corner"),
                (
                    (easel.width as i32 + 6, easel.height as i32 + 6),
                    "past the bottom-right corner",
                ),
                (centre(empty), "back over the empty cell"),
                (off_centre, "to the empty cell's corner"),
            ] {
                at = t.carry(&mut state, at, to, what);
            }
            // Dropped there: it lands, sliding into its cell, and the sweep the drop starts runs.
            t.release(&mut state, at, theme, "the drop");
            // Picked up again and let go over the card: a cancel, so it flies back across the
            // board, outside every cell and light the drop changes.
            let home = centre(empty);
            state.handle_on(
                &t.fast,
                InputEvent::MouseDown {
                    x: home.0,
                    y: home.1,
                    alt: false,
                },
            );
            t.rising = RISE_FRAMES;
            let over_card = ((card.left + card.right) / 2, (card.top + card.bottom) / 2);
            let at = t.carry(&mut state, home, over_card, "to the card");
            t.release(&mut state, at, theme, "the cancel");
        }
    }
}

impl Twins {
    /// Lets go at `at`: every frame until every cue ends, capped (review M11), then the settled
    /// frame, which must equal a cold paint.
    fn release(&mut self, state: &mut GameState, at: (i32, i32), theme: Theme, what: &str) {
        state.handle_on(
            &self.fast,
            InputEvent::MouseUp {
                x: at.0,
                y: at.1,
                alt: false,
            },
        );
        // The frame that sees the release starts the land: paint it before asking `next_wake`.
        let mut left = 400;
        let mut landing = 0;
        loop {
            self.frame(state, what, true);
            landing += usize::from(self.fast.choreography().moment(self.now).land.is_some());
            if self.fast.next_wake(self.now).is_none() {
                break;
            }
            left -= 1;
            assert!(left > 0, "{what}: the cues settle within 400 frames");
        }
        assert!(landing >= 9, "{what}: the land's frames ({landing})");
        self.frame(state, what, true);
        let easel = *self.fast.easel();
        assert!(
            self.a == common::cold_in(theme, easel, halation::Scene::of(state)),
            "{theme:?}, {what}: the settled frame differs from a cold paint"
        );
    }
}
