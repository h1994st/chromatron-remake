//! The shell's pointer bookkeeping and the modern input path, window-free (ruling P11).
//! `PressGate` keeps the buttons held and the last cursor position in physical px. The cursor
//! is read in the look's units only where it is used, so a look switch, a resize or a scale
//! change under a still pointer never reads stale units. The functions below take a pointer
//! event or a sheet command to the canvas and the game, as the web `Session` does (`route`,
//! `pointer_cancel`, `toggle`).

use halation::{Control, Route};
use lumen::{Effects, GameState, InputEvent};

use crate::look_switch::Look;
use crate::modern_stage::{ModernStage, device_point};
use crate::viewport::Viewport;

/// The buttons held, and where the pointer last was (physical px, winit's `CursorMoved`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PressGate {
    held: u32,
    cursor: (f64, f64),
}

impl Default for PressGate {
    fn default() -> Self {
        PressGate {
            held: 0,
            cursor: (-1.0, -1.0),
        }
    }
}

impl PressGate {
    pub fn moved(&mut self, x: f64, y: f64) {
        self.cursor = (x, y);
    }

    pub fn cursor(&self) -> (f64, f64) {
        self.cursor
    }

    pub fn held(&self) -> bool {
        self.held > 0
    }

    pub fn press(&mut self) {
        self.held = self.held.saturating_add(1);
    }

    /// A button went up. False for a release with no press counted (the window-activating
    /// click's mouse-up, or one after `cancel`): the shell drops it, since lumen would turn a
    /// piece or open a level on it.
    pub fn release(&mut self) -> bool {
        match self.held.checked_sub(1) {
            Some(held) => {
                self.held = held;
                true
            }
            None => false,
        }
    }

    /// The pointer was lost: the mouse-up that ends a held drag (lumen ends one on any
    /// mouse-up; the piece stays put), once.
    pub fn cancel(&mut self) -> Option<InputEvent> {
        (std::mem::take(&mut self.held) > 0).then_some(InputEvent::MouseUp {
            x: -1,
            y: -1,
            alt: false,
        })
    }
}

/// The cursor (physical px) in the look's units: frame px in classic, canvas px in modern.
pub fn place(look: Look, viewport: &Viewport, physical: (f64, f64)) -> (i32, i32) {
    match look {
        Look::Classic => viewport.to_frame(physical.0, physical.1),
        Look::Modern => device_point(physical.0, physical.1),
    }
}

/// What one input did: the game's effects, and whether the sheet opened or closed.
#[derive(Debug, Default)]
pub struct Handled {
    pub effects: Effects,
    pub sheet_changed: bool,
}

/// A pointer event's path (spec §5.3). With a stage (the modern look), the canvas's controls
/// come first: Next ▶ is the game's `=`, and Levels and the close chip toggle the sheet. Then
/// the game, through the lightbox's floorplan. Without a stage: the game.
pub fn route(
    stage: Option<&mut ModernStage>,
    state: &mut GameState<'static>,
    event: InputEvent,
) -> Handled {
    let Some(stage) = stage else {
        return Handled {
            effects: state.handle(event),
            sheet_changed: false,
        };
    };
    match stage.route(event) {
        Route::Game(event) => Handled {
            effects: state.handle_on(stage.lightbox(), event),
            sheet_changed: false,
        },
        Route::Control(Control::Next) => Handled {
            effects: state.handle_on(stage.lightbox(), InputEvent::Key(b'=')),
            sheet_changed: false,
        },
        Route::Control(control) => Handled {
            effects: Effects::default(),
            sheet_changed: stage.command(control),
        },
        Route::Consumed => Handled::default(),
    }
}

/// An event straight to the game (a key, the wheel, a cancel's mouse-up), through the
/// lightbox's floorplan when there is a stage.
pub fn send(
    stage: Option<&ModernStage>,
    state: &mut GameState<'static>,
    event: InputEvent,
) -> Effects {
    match stage {
        Some(stage) => state.handle_on(stage.lightbox(), event),
        None => state.handle(event),
    }
}

/// The pointer was lost (focus, a resize, a look switch, Escape, Levels…). A press on a canvas
/// control is forgotten, a held button's drag ends, and the release that follows is dropped
/// (`PressGate::release`).
pub fn cancel(
    gate: &mut PressGate,
    mut stage: Option<&mut ModernStage>,
    state: &mut GameState<'static>,
) -> Effects {
    if let Some(stage) = stage.as_deref_mut() {
        stage.forget_press();
    }
    match gate.cancel() {
        Some(up) => send(stage.as_deref(), state, up),
        None => Effects::default(),
    }
}

/// Escape over the open sheet, or File ▸ Levels… (ruling P10): the held press ends, then the
/// sheet closes or toggles.
pub fn sheet_command(
    gate: &mut PressGate,
    stage: &mut ModernStage,
    state: &mut GameState<'static>,
    control: Control,
) -> Handled {
    let effects = cancel(gate, Some(&mut *stage), state);
    Handled {
        effects,
        sheet_changed: stage.command(control),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modern_stage::{ESCAPE, escape_closes_sheet};
    use halation::{Easel, Theme};
    use lumen::{BOARD_CELLS, Piece, RuleSet, SaveData, VISIBLE_TOOLBOX_SLOTS};

    const SETTLED: f64 = f64::INFINITY;

    #[test]
    fn a_release_without_a_press_is_dropped() {
        let mut gate = PressGate::default();
        assert!(!gate.release(), "the window-activating click's mouse-up");
        gate.press();
        assert!(gate.held());
        assert!(gate.release());
        assert!(!gate.held());
    }

    #[test]
    fn cancel_ends_a_held_press_once() {
        let mut gate = PressGate::default();
        assert_eq!(gate.cancel(), None);
        gate.press();
        assert_eq!(
            gate.cancel(),
            Some(InputEvent::MouseUp {
                x: -1,
                y: -1,
                alt: false
            })
        );
        assert_eq!(gate.cancel(), None);
        assert!(!gate.release(), "its release is dropped");
    }

    /// Ruling P11: a look switch or a scale change under a still pointer, then a click, reads the
    /// same physical point in the new units.
    #[test]
    fn the_cursor_is_read_in_the_looks_units_when_used() {
        let mut gate = PressGate::default();
        gate.moved(300.5, 200.0);
        let retina = Viewport::fit(1280, 960); // classic at 2×
        assert_eq!(place(Look::Classic, &retina, gate.cursor()), (150, 100));
        assert_eq!(
            place(Look::Modern, &retina, gate.cursor()),
            (300, 200),
            "switched to modern"
        );
        assert_eq!(
            place(Look::Classic, &Viewport::fit(640, 480), gate.cursor()),
            (300, 200),
            "Scale 1×"
        );
        assert_eq!(
            place(Look::Classic, &Viewport::fit(1920, 1440), gate.cursor()),
            (100, 66),
            "Scale 3×"
        );
        assert_eq!(
            place(Look::Modern, &retina, PressGate::default().cursor()),
            (-1, -1),
            "never moved"
        );
    }

    /// C1 with its first level solved (so level 2 is open and Next ▶ shows), in an 800 × 600 pt
    /// window whose picker is a sheet (behind the card's Levels pill), the sheet closed.
    fn won_first_level() -> (PressGate, ModernStage, GameState<'static>, Vec<u32>) {
        let data = crate::CHROMATRON_1.data;
        let mut state = GameState::new(data, SaveData::fresh(RuleSet::of(data.game)));
        let solution = lumen::stored_solution(data, 0).expect("a stored solution");
        state.handle(InputEvent::Paste(solution));
        assert!(state.is_won() && state.is_unlocked(1), "level 2 is open");
        let mut stage = ModernStage::new(Easel::new(1600, 1200, 2.0), Theme::Dark, false);
        let mut px = vec![0u32; 1600 * 1200];
        stage.paint(&state, SETTLED, &mut px);
        (PressGate::default(), stage, state, px)
    }

    /// The same, with the sheet open.
    fn sheet_open() -> (PressGate, ModernStage, GameState<'static>, Vec<u32>) {
        let (mut gate, mut stage, mut state, mut px) = won_first_level();
        assert!(sheet_command(&mut gate, &mut stage, &mut state, Control::Levels).sheet_changed);
        stage.paint(&state, SETTLED, &mut px);
        assert!(stage.sheet_open());
        (gate, stage, state, px)
    }

    fn chip_centre(stage: &ModernStage, slot: usize) -> (f64, f64) {
        let r = stage.lightbox().tableau().sheet_chip_rect(slot);
        assert!(r.right > r.left, "chip {slot} shows");
        (
            f64::from(r.left + r.right) / 2.0,
            f64::from(r.top + r.bottom) / 2.0,
        )
    }

    /// The centre of the first 4 px cell, in canvas px, where `control` is hit.
    fn control_point(stage: &ModernStage, control: Control) -> (f64, f64) {
        let canvas = stage.lightbox().tableau().canvas();
        (canvas.top..canvas.bottom)
            .step_by(4)
            .flat_map(|y| (canvas.left..canvas.right).step_by(4).map(move |x| (x, y)))
            .find(|&(x, y)| stage.lightbox().control_at(x + 2, y + 2) == Some(control))
            .map(|(x, y)| (f64::from(x + 2), f64::from(y + 2)))
            .unwrap_or_else(|| panic!("{control:?} shows"))
    }

    /// The shell's `MouseInput` arms (game_window.rs, Task 12): the gate counts the press, the
    /// cursor is placed in the look's units, and the event is routed.
    fn press(
        gate: &mut PressGate,
        stage: &mut ModernStage,
        state: &mut GameState<'static>,
    ) -> Handled {
        gate.press();
        let (x, y) = place(Look::Modern, &Viewport::fit(1, 1), gate.cursor());
        route(
            Some(stage),
            state,
            InputEvent::MouseDown { x, y, alt: false },
        )
    }

    /// The release arm: dropped (`None`) when the gate counted no press.
    fn release(
        gate: &mut PressGate,
        stage: &mut ModernStage,
        state: &mut GameState<'static>,
    ) -> Option<Handled> {
        if !gate.release() {
            return None;
        }
        let (x, y) = place(Look::Modern, &Viewport::fit(1, 1), gate.cursor());
        Some(route(
            Some(stage),
            state,
            InputEvent::MouseUp { x, y, alt: false },
        ))
    }

    /// `route`'s control arms: Next ▶ is the game's `=` (the next level), and the card's
    /// Levels pill and the sheet's close chip toggle the sheet without the game seeing them.
    #[test]
    fn next_and_the_sheet_controls_take_their_arms() {
        let (mut gate, mut stage, mut state, mut px) = won_first_level();
        let (x, y) = control_point(&stage, Control::Next);
        gate.moved(x, y);
        assert!(!press(&mut gate, &mut stage, &mut state).sheet_changed);
        assert_eq!(state.slot(), 0, "a press alone does nothing");
        let next = release(&mut gate, &mut stage, &mut state).expect("released");
        assert!(!next.sheet_changed);
        assert!(
            next.effects.redraw != lumen::Redraw::None,
            "the game moved on"
        );
        assert_eq!(state.slot(), 1, "Next ▶ opened level 2");
        stage.paint(&state, SETTLED, &mut px);

        let (x, y) = control_point(&stage, Control::Levels);
        gate.moved(x, y);
        press(&mut gate, &mut stage, &mut state);
        assert!(!stage.sheet_open(), "a press alone does nothing");
        let open = release(&mut gate, &mut stage, &mut state).expect("released");
        assert!(
            open.sheet_changed && stage.sheet_open(),
            "Levels opened the sheet"
        );
        assert!(
            open.effects.redraw == lumen::Redraw::None,
            "the game saw nothing"
        );
        stage.paint(&state, SETTLED, &mut px);

        let (x, y) = control_point(&stage, Control::CloseSheet);
        gate.moved(x, y);
        press(&mut gate, &mut stage, &mut state);
        let close = release(&mut gate, &mut stage, &mut state).expect("released");
        assert!(
            close.sheet_changed && !stage.sheet_open(),
            "the sheet closed"
        );
        assert_eq!(state.slot(), 1, "on the same level");
    }

    /// `cancel` forgets a press held on a canvas control: a release that still arrives over it
    /// (whatever the gate did with it) never fires the control.
    #[test]
    fn a_cancel_forgets_a_press_on_a_canvas_control() {
        let (mut gate, mut stage, mut state, _) = won_first_level();
        let (x, y) = control_point(&stage, Control::Levels);
        gate.moved(x, y);
        press(&mut gate, &mut stage, &mut state);
        cancel(&mut gate, Some(&mut stage), &mut state);
        assert!(
            release(&mut gate, &mut stage, &mut state).is_none(),
            "the gate drops the release"
        );
        let (x, y) = place(Look::Modern, &Viewport::fit(1, 1), gate.cursor());
        let late = route(
            Some(&mut stage),
            &mut state,
            InputEvent::MouseUp { x, y, alt: false },
        );
        assert!(
            !late.sheet_changed && !stage.sheet_open(),
            "Levels never fired"
        );
    }

    /// The setup is live: a click on the open chip opens its level.
    #[test]
    fn a_click_on_an_unlocked_chip_opens_it() {
        let (mut gate, mut stage, mut state, mut px) = sheet_open();
        let (x, y) = chip_centre(&stage, 1);
        gate.moved(x, y);
        press(&mut gate, &mut stage, &mut state);
        assert!(release(&mut gate, &mut stage, &mut state).is_some());
        stage.paint(&state, SETTLED, &mut px);
        assert_eq!(state.slot(), 1);
    }

    /// Review Focus 3: a press on an open chip, Escape, then the real release: swallowed.
    #[test]
    fn escape_swallows_a_press_on_an_unlocked_chip() {
        let (mut gate, mut stage, mut state, mut px) = sheet_open();
        let before = state.board().clone();
        let (x, y) = chip_centre(&stage, 1);
        gate.moved(x, y);
        press(&mut gate, &mut stage, &mut state);
        assert!(escape_closes_sheet(Some(ESCAPE), stage.sheet_open()));
        assert!(
            sheet_command(&mut gate, &mut stage, &mut state, Control::CloseSheet).sheet_changed
        );
        stage.paint(&state, SETTLED, &mut px);
        assert!(!stage.sheet_open());
        assert!(
            release(&mut gate, &mut stage, &mut state).is_none(),
            "the release is swallowed"
        );
        assert_eq!(state.slot(), 0, "no level opened");
        assert!(*state.board() == before, "the board is untouched");
    }

    /// A look switch or a resize cancels a held drag (the shell calls `cancel`): the piece goes
    /// back and the release that follows is dropped.
    #[test]
    fn a_look_switch_or_resize_swallows_a_held_press() {
        let data = crate::CHROMATRON_1.data;
        let mut state = GameState::new(data, SaveData::fresh(RuleSet::of(data.game)));
        let mut stage = ModernStage::new(Easel::new(1280, 800, 1.0), Theme::Dark, false);
        let mut px = vec![0u32; 1280 * 800];
        stage.paint(&state, SETTLED, &mut px);
        let t = stage.lightbox().tableau().clone();
        let slot = (BOARD_CELLS..BOARD_CELLS + VISIBLE_TOOLBOX_SLOTS)
            .find(|&i| state.board().cells[i].piece != Piece::Empty)
            .expect("a tray piece");
        let before = state.board().clone();
        let mut gate = PressGate::default();
        let from = t.cell_centre(slot);
        gate.moved(f64::from(from.x), f64::from(from.y));
        press(&mut gate, &mut stage, &mut state);
        let to = t.cell_centre(112);
        gate.moved(f64::from(to.x), f64::from(to.y));
        let (x, y) = place(Look::Modern, &Viewport::fit(1, 1), gate.cursor());
        route(
            Some(&mut stage),
            &mut state,
            InputEvent::MouseDragged { x, y },
        );
        assert!(state.drag().is_some(), "dragging");
        cancel(&mut gate, Some(&mut stage), &mut state);
        assert!(state.drag().is_none());
        assert!(*state.board() == before, "the piece went back");
        assert!(
            release(&mut gate, &mut stage, &mut state).is_none(),
            "the release is dropped"
        );
    }
}
