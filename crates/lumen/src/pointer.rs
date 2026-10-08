//! The mouse state machine (logic.md L7, `handle_mouse` @ 0x50d4).

use crate::game_state::{DragState, Effects, GameState, Redraw};
use crate::layout::{Hit, Rect, hit_test};
use crate::piece::{Piece, turn};

impl GameState<'_> {
    /// Primary down on a board or toolbox cell makes it the cursor, and arms a drag if it holds a
    /// movable piece. An alternate-button down is otherwise ignored, as in the original.
    ///
    /// A press while a drag is still running means its mouse-up was lost (released outside the
    /// window, platform.md P4): the drag is cancelled and the piece stays where it was, where the
    /// original would later drop whatever cell this press selects.
    pub(crate) fn mouse_down(&mut self, x: i32, y: i32, alt: bool) -> Effects {
        let cancelled = std::mem::take(&mut self.pointer.state) == DragState::Dragging;
        let effects = if cancelled {
            Effects::full_redraw()
        } else {
            Effects::default()
        };
        if alt {
            return effects;
        }
        if let Hit::Cell(index) = hit_test(x, y) {
            self.pointer.cursor = index;
            if self.board.cells[index].piece.is_movable() {
                self.pointer.state = DragState::Pressed;
                self.pointer.press = (x, y);
            }
        }
        effects
    }

    /// A drag starts once the pointer is over the board or toolbox and more than 1 px away from the
    /// press. Beams are not retraced while dragging.
    pub(crate) fn mouse_dragged(&mut self, x: i32, y: i32) -> Effects {
        match self.pointer.state {
            DragState::Idle => Effects::default(),
            DragState::Pressed => {
                let (px, py) = self.pointer.press;
                let near = (x - px).abs() <= 1 && (y - py).abs() <= 1;
                if near || !matches!(hit_test(x, y), Hit::Cell(_)) {
                    return Effects::default();
                }
                let piece = self.board.cells[self.pointer.cursor];
                if !piece.piece.is_movable() {
                    self.pointer.state = DragState::Idle;
                    return Effects::default();
                }
                self.pointer.state = DragState::Dragging;
                self.pointer.piece = piece;
                self.pointer.at = (px, py);
                self.move_drag(x, y)
            }
            DragState::Dragging => self.move_drag(x, y),
        }
    }

    fn move_drag(&mut self, x: i32, y: i32) -> Effects {
        let (ox, oy) = self.pointer.at;
        self.pointer.at = (x, y);
        let old = Rect::new(ox - 30, oy - 30, ox + 30, oy + 30);
        let new = Rect::new(x - 12, y - 12, x + 13, y + 13);
        Effects {
            redraw: Redraw::Rects([old, new]),
            ..Effects::default()
        }
    }

    /// Ends a drag (moving the piece if the target cell is empty), or rotates a movable piece
    /// (primary: counter-clockwise, alternate: clockwise), or opens an unlocked strip slot.
    ///
    /// Unlike the original, every release ends the gesture: the original can leave `drag_state`
    /// at 1 after a click, which later swallows right-click rotations (logic.md L7 quirk).
    pub(crate) fn mouse_up(&mut self, x: i32, y: i32, alt: bool) -> Effects {
        let was = std::mem::take(&mut self.pointer.state);
        let hit = hit_test(x, y);
        if was == DragState::Dragging {
            if let Hit::Cell(target) = hit
                && !alt
                && self.board.cells[target].piece == Piece::Empty
                && self.board.cells[self.pointer.cursor].piece.is_movable()
            {
                let source = self.pointer.cursor;
                self.board.cells[target] = self.board.cells[source];
                self.board.cells[source].piece = Piece::Empty;
            }
            return self.check_win();
        }
        match hit {
            Hit::Cell(target) if self.board.cells[target].piece.is_movable() => {
                let cell = &mut self.board.cells[target];
                cell.dir = turn(cell.dir, if alt { 1 } else { 7 });
                self.check_win()
            }
            Hit::Strip(slot) if !alt && self.is_unlocked(slot) => self.goto_slot(slot),
            _ => Effects::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::board::{Cell, board_index, toolbox_index};
    use crate::fixture::tiny_game;
    use crate::game_state::{Drag, Effects, GameState, InputEvent, Redraw};
    use crate::layout::{Rect, cell_origin, strip_origin};
    use crate::piece::Piece;
    use crate::save::SaveData;

    /// The centre pixel of cell `index`.
    fn centre(index: usize) -> (i32, i32) {
        let (x, y) = cell_origin(index);
        (x + 12, y + 12)
    }

    fn down(state: &mut GameState, (x, y): (i32, i32)) -> Effects {
        state.handle(InputEvent::MouseDown { x, y, alt: false })
    }

    fn drag(state: &mut GameState, (x, y): (i32, i32)) -> Effects {
        state.handle(InputEvent::MouseDragged { x, y })
    }

    fn up(state: &mut GameState, (x, y): (i32, i32), alt: bool) -> Effects {
        state.handle(InputEvent::MouseUp { x, y, alt })
    }

    /// Press on `from`, start the drag over the empty cell (0, 0), then release over `to`.
    fn drag_and_drop(state: &mut GameState, from: (i32, i32), to: (i32, i32)) -> Effects {
        down(state, from);
        drag(state, centre(board_index(0, 0)));
        drag(state, to);
        up(state, to, false)
    }

    const MIRROR_SLOT: usize = 225; // toolbox slot 0

    #[test]
    fn drag_a_piece_onto_the_board_and_rotate_it_into_place() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        let target = board_index(8, 7);
        down(&mut state, centre(MIRROR_SLOT));
        let effects = drag(&mut state, centre(target));
        assert!(
            matches!(effects.redraw, Redraw::Rects(_)),
            "dragging redraws only the sprite boxes"
        );
        let (x, y) = centre(target);
        assert_eq!(
            state.drag(),
            Some(Drag {
                source: MIRROR_SLOT,
                piece: Cell {
                    piece: Piece::Mirror,
                    colour: 4,
                    dir: 0
                },
                x,
                y
            })
        );
        assert_eq!(up(&mut state, centre(target), false).redraw, Redraw::Full);
        assert_eq!(state.drag(), None);
        assert_eq!(state.board().cells[target].piece, Piece::Mirror);
        assert_eq!(
            state.board().cells[MIRROR_SLOT],
            Cell {
                piece: Piece::Empty,
                colour: 4,
                dir: 0
            }
        );
        assert!(!state.is_won());
        let effects = state.handle(InputEvent::MouseDown { x, y, alt: true });
        assert_eq!(
            effects,
            Effects::default(),
            "alternate presses do nothing by themselves"
        );
        let effects = up(&mut state, centre(target), true);
        assert_eq!(
            state.board().cells[target].dir,
            1,
            "alternate click turns clockwise"
        );
        assert!(effects.write_save && state.is_won());
    }

    #[test]
    fn drag_boxes_track_the_pointer() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        down(&mut state, (460, 20));
        drag(&mut state, (100, 100));
        let effects = drag(&mut state, (110, 105));
        assert_eq!(
            effects.redraw,
            Redraw::Rects([Rect::new(70, 70, 130, 130), Rect::new(98, 93, 123, 118)])
        );
    }

    #[test]
    fn a_click_without_moving_rotates_counter_clockwise() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        let (x, y) = centre(MIRROR_SLOT);
        down(&mut state, (x, y));
        drag(&mut state, (x + 1, y - 1));
        assert_eq!(state.drag(), None, "1 px is not a drag");
        up(&mut state, (x + 1, y - 1), false);
        assert_eq!(state.board().cells[MIRROR_SLOT].dir, 7);
    }

    #[test]
    fn dropping_on_an_occupied_cell_or_outside_leaves_the_piece() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        let laser = board_index(2, 7);
        for target in [
            centre(laser),
            centre(board_index(8, 2)),
            (620, 300),
            strip_origin(3),
            centre(MIRROR_SLOT),
        ] {
            assert!(state.drag().is_none());
            let effects = drag_and_drop(&mut state, centre(MIRROR_SLOT), target);
            assert_eq!(effects.redraw, Redraw::Full);
            assert_eq!(
                state.board().cells[MIRROR_SLOT].piece,
                Piece::Mirror,
                "{target:?}"
            );
            assert_eq!(state.board().cells[laser].piece, Piece::Laser);
        }
    }

    #[test]
    fn a_drag_cannot_start_off_the_board() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        down(&mut state, centre(MIRROR_SLOT));
        assert_eq!(drag(&mut state, (620, 300)), Effects::default());
        assert_eq!(state.drag(), None);
        drag(&mut state, centre(board_index(0, 0)));
        assert!(
            state.drag().is_some(),
            "it starts once the pointer is back over a cell"
        );
    }

    #[test]
    fn fixed_pieces_and_empty_cells_ignore_clicks() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        for index in [
            board_index(2, 7),
            board_index(8, 2),
            board_index(0, 0),
            toolbox_index(5),
        ] {
            down(&mut state, centre(index));
            assert_eq!(up(&mut state, centre(index), false), Effects::default());
            assert_eq!(up(&mut state, centre(index), true), Effects::default());
        }
        assert_eq!(
            state.board().at(2, 7),
            Cell {
                piece: Piece::Laser,
                colour: 4,
                dir: 2
            }
        );
    }

    #[test]
    fn a_click_released_off_the_board_does_not_block_later_rotations() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        down(&mut state, centre(MIRROR_SLOT));
        up(&mut state, (620, 300), false);
        up(&mut state, centre(MIRROR_SLOT), true);
        assert_eq!(state.board().cells[MIRROR_SLOT].dir, 1);
    }

    #[test]
    fn a_press_after_a_lost_mouse_up_cancels_the_drag() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        let laser = board_index(2, 7);
        down(&mut state, centre(MIRROR_SLOT));
        drag(&mut state, centre(board_index(0, 0)));
        assert!(state.drag().is_some());
        // The mouse-up happened outside the window; the next press lands on the laser.
        assert_eq!(down(&mut state, centre(laser)).redraw, Redraw::Full);
        assert_eq!(state.drag(), None);
        up(&mut state, centre(board_index(0, 0)), false);
        assert_eq!(state.board().cells[laser].piece, Piece::Laser);
        assert_eq!(state.board().at(0, 0).piece, Piece::Empty);
        assert_eq!(state.board().cells[MIRROR_SLOT].piece, Piece::Mirror);
    }

    #[test]
    fn strip_clicks_open_unlocked_slots_only() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        assert_eq!(up(&mut state, strip_origin(1), false), Effects::default());
        assert_eq!(state.slot(), 0);
        state.handle(InputEvent::Key(b'L'));
        assert_eq!(up(&mut state, strip_origin(1), false).redraw, Redraw::Full);
        assert_eq!(state.slot(), 1);
        assert_eq!(
            up(&mut state, strip_origin(0), true),
            Effects::default(),
            "not with the alternate button"
        );
    }
}
