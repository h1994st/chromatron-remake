//! Game state and the pure event handler (logic.md L6–L8, platform.md P4–P5).

use spectrograph::GameData;

use crate::board::{Board, Cell};
use crate::layout::{ClassicFloorplan, Floorplan, LAST_SLOT, Rect, STRIP_SLOTS};
use crate::progress::PROGRESS_SLOTS;
use crate::rule_set::RuleSet;
use crate::save::{SaveData, encode_save};
use crate::solution::{apply_solution, encode_solution};
use crate::toolbox::move_pieces_to_toolbox;
use crate::trace::{BeamField, trace};
use crate::win::is_won;

/// Input. Pointer coordinates are the floorplan's pixels: the 640 × 480 frame for `handle`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// A button went down. `alt`: button 2, or the primary button with Shift, Option or Control
    /// held. The shell latches `alt` at mouse-down and repeats it on the matching mouse-up.
    MouseDown {
        x: i32,
        y: i32,
        alt: bool,
    },
    /// The pointer moved with a button held.
    MouseDragged {
        x: i32,
        y: i32,
    },
    MouseUp {
        x: i32,
        y: i32,
        alt: bool,
    },
    /// A typed character without Cmd. Menu Next, Previous and Reset Level send `=`, `-`, `r`.
    Key(u8),
    /// A scroll-wheel event; the shell sends it only while "Mouse Wheel Changes Level" is on.
    Wheel {
        delta: i32,
    },
    /// Edit ▸ Copy Level.
    Copy,
    /// Edit ▸ Paste Level, with the clipboard's text.
    Paste(String),
    /// The app is quitting.
    Quit,
}

/// What the shell must do after an event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    pub redraw: Redraw,
    /// Write `GameState::save_bytes()` to the save file now.
    pub write_save: bool,
    /// Put this text on the clipboard.
    pub clipboard: Option<String>,
}

impl Effects {
    pub(crate) fn full_redraw() -> Effects {
        Effects {
            redraw: Redraw::Full,
            ..Effects::default()
        }
    }
}

/// Which part of the window changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Redraw {
    #[default]
    None,
    Full,
    /// While dragging: the sprite's old 60 × 60 box and its new 25 × 25 box (platform.md P1).
    Rects([Rect; 2]),
}

/// How the strip draws a slot's number (data.md D6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    Current,
    Completed,
    Unlocked,
    Locked,
}

/// A piece being dragged: where it came from, what it is, and the pointer position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drag {
    pub source: usize,
    pub piece: Cell,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DragState {
    #[default]
    Idle,
    Pressed,
    Dragging,
}

/// `drag_state`, `cursor_row/col`, `press_x/y` and the drag sprite position.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Pointer {
    pub(crate) state: DragState,
    pub(crate) cursor: usize,
    pub(crate) press: (i32, i32),
    pub(crate) at: (i32, i32),
    pub(crate) piece: Cell,
}

pub struct GameState<'a> {
    data: &'a GameData,
    rules: &'static RuleSet,
    pub(crate) board: Board,
    field: BeamField,
    slot: usize,
    won: bool,
    force_win: bool,
    completed: [bool; PROGRESS_SLOTS],
    unlocked: [bool; PROGRESS_SLOTS],
    /// `saved_states`, by level id.
    saved: Vec<Option<Board>>,
    pub(crate) pointer: Pointer,
}

/// Pointer coordinates beyond this are clamped, so hit-testing arithmetic cannot overflow.
const POINTER_LIMIT: i32 = 0x10000;

fn clamp_pointer(event: InputEvent) -> InputEvent {
    let c = |v: i32| v.clamp(-POINTER_LIMIT, POINTER_LIMIT);
    match event {
        InputEvent::MouseDown { x, y, alt } => InputEvent::MouseDown {
            x: c(x),
            y: c(y),
            alt,
        },
        InputEvent::MouseDragged { x, y } => InputEvent::MouseDragged { x: c(x), y: c(y) },
        InputEvent::MouseUp { x, y, alt } => InputEvent::MouseUp {
            x: c(x),
            y: c(y),
            alt,
        },
        other => other,
    }
}

impl<'a> GameState<'a> {
    /// `game_init`: progress from `save`, then slot 0, restoring its saved board if there is one.
    /// The game's rules come from `data.game`. `save.boards` is brought to the game's
    /// `RuleSet::saved_ids` entries if it has another length. If `save.replay_wins`, the completed
    /// slots are first recomputed from the saved boards (`replay_wins`).
    ///
    /// Trusts `save`: build it only with `decode_save` for the same game, `SaveData::fresh` or
    /// `Default`. The initial level's `Effects` are not returned, so callers must paint the whole
    /// window first.
    pub fn new(data: &'a GameData, save: SaveData) -> GameState<'a> {
        let rules = RuleSet::of(data.game);
        let mut saved = save.boards;
        saved.resize(rules.saved_ids, None);
        let mut state = GameState {
            data,
            rules,
            board: Board::new(data.game),
            field: BeamField::default(),
            slot: 0,
            won: false,
            force_win: false,
            completed: save.completed,
            unlocked: [false; PROGRESS_SLOTS],
            saved,
            pointer: Pointer::default(),
        };
        if save.replay_wins {
            state.replay_wins();
        }
        rules.unlock.apply(&state.completed, &mut state.unlocked);
        state.start_level(true);
        state
    }

    /// `handle_on` the original window (`ClassicFloorplan`).
    pub fn handle(&mut self, event: InputEvent) -> Effects {
        self.handle_on(&ClassicFloorplan, event)
    }

    /// Handles one event, hit-testing pointer events with `plan` and taking its drag slop.
    /// Monomorphised, so classic costs nothing. Allocation-free.
    pub fn handle_on(&mut self, plan: &impl Floorplan, event: InputEvent) -> Effects {
        let event = clamp_pointer(event);
        match event {
            InputEvent::MouseDown { x, y, alt } => self.mouse_down(plan, x, y, alt),
            InputEvent::MouseDragged { x, y } => self.mouse_dragged(plan, x, y),
            InputEvent::MouseUp { x, y, alt } => self.mouse_up(plan, x, y, alt),
            InputEvent::Key(ch) => self.key(ch),
            InputEvent::Wheel { delta } => self.key(if delta > 0 { b'-' } else { b'=' }),
            InputEvent::Copy => Effects {
                clipboard: encode_solution(&self.board, self.slot),
                ..Effects::default()
            },
            InputEvent::Paste(text) => {
                if !apply_solution(&mut self.board, self.slot, &text) {
                    return Effects::default();
                }
                self.pointer = Pointer::default();
                self.check_win()
            }
            InputEvent::Quit => {
                self.save_level_state();
                Effects {
                    write_save: true,
                    ..Effects::default()
                }
            }
        }
    }

    /// `goto_slot`: saves this level's board and starts `slot` from its saved board. The caller
    /// checks that the slot is unlocked. Precondition: `slot < STRIP_SLOTS` (asserts).
    pub fn goto_slot(&mut self, slot: usize) -> Effects {
        assert!(slot < STRIP_SLOTS, "slot {slot} is not on the strip");
        self.save_level_state();
        self.slot = slot;
        self.start_level(true)
    }

    /// The save file's bytes for the current progress and saved boards.
    pub fn save_bytes(&self) -> Vec<u8> {
        encode_save(self.rules, &self.completed, &self.saved)
    }

    pub fn data(&self) -> &'a GameData {
        self.data
    }

    /// The rules of the game being played.
    pub fn rules(&self) -> &'static RuleSet {
        self.rules
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn field(&self) -> &BeamField {
        &self.field
    }

    /// The current strip slot, 0-based.
    pub fn slot(&self) -> usize {
        self.slot
    }

    pub fn level_id(&self) -> u8 {
        self.data.level_order[self.slot]
    }

    /// Whether `handle(InputEvent::Key(ch))` acts on `ch`: the keys `handle_key` reads (space,
    /// `=`, `+`, `-`, `r`) and the game's cheat key. A shell lets every other key keep its usual
    /// meaning.
    pub fn reads_key(&self, ch: u8) -> bool {
        matches!(ch, b' ' | b'=' | b'+' | b'-' | b'r') || ch == self.rules.cheat_key
    }

    /// The result of the last win check.
    pub fn is_won(&self) -> bool {
        self.won
    }

    pub fn is_completed(&self, slot: usize) -> bool {
        self.completed[slot]
    }

    pub fn is_unlocked(&self, slot: usize) -> bool {
        self.unlocked[slot]
    }

    pub fn slot_state(&self, slot: usize) -> SlotState {
        if slot == self.slot {
            SlotState::Current
        } else if self.completed[slot] {
            SlotState::Completed
        } else if self.unlocked[slot] {
            SlotState::Unlocked
        } else {
            SlotState::Locked
        }
    }

    /// The current level's hint, "" for none.
    pub fn hint(&self) -> &'a str {
        let data: &'a GameData = self.data;
        &data
            .slot_level(self.slot)
            .expect("level order names decoded levels")
            .hint
    }

    /// The current level's name, "" in a game without names.
    pub fn level_name(&self) -> &'a str {
        let data: &'a GameData = self.data;
        &data
            .slot_level(self.slot)
            .expect("level order names decoded levels")
            .name
    }

    pub fn drag(&self) -> Option<Drag> {
        let p = &self.pointer;
        (p.state == DragState::Dragging).then_some(Drag {
            source: p.cursor,
            piece: p.piece,
            x: p.at.0,
            y: p.at.1,
        })
    }

    /// `load_save_file`'s migration of a save without the format marker (Chromatron 3,
    /// platform.md P7): every strip slot whose level has a saved board counts as completed exactly
    /// when that board is a solved position. The original's reads past the 50 slots are left out.
    /// The file itself is rewritten at the next save, not at once as in the original.
    fn replay_wins(&mut self) {
        let data = self.data;
        for (slot, &id) in data.level_order.iter().enumerate().take(STRIP_SLOTS) {
            if let Some(board) = &self.saved[usize::from(id)] {
                trace(board, &mut self.field);
                self.completed[slot] = is_won(board, &self.field);
            }
        }
    }

    fn key(&mut self, ch: u8) -> Effects {
        match ch {
            b' ' | b'=' | b'+' => self.next_level(),
            b'-' => self.prev_level(),
            b'r' => self.start_level(false),
            cheat if cheat == self.rules.cheat_key => {
                self.force_win = true;
                self.check_win()
            }
            _ => Effects::default(),
        }
    }

    fn next_level(&mut self) -> Effects {
        if !self.unlocked[self.slot + 1] {
            return Effects::default();
        }
        self.save_level_state();
        if self.slot < LAST_SLOT {
            self.slot += 1;
        }
        self.start_level(true)
    }

    fn prev_level(&mut self) -> Effects {
        self.save_level_state();
        self.slot = self.slot.saturating_sub(1);
        self.start_level(true)
    }

    /// `start_level(restore)`: the level's saved board if `restore` and there is one, otherwise a
    /// fresh board with every piece in the toolbox. Any drag in progress is dropped.
    fn start_level(&mut self, restore: bool) -> Effects {
        self.pointer = Pointer::default();
        let level = self
            .data
            .slot_level(self.slot)
            .expect("level order names decoded levels");
        match self.saved[usize::from(level.id)]
            .as_ref()
            .filter(|_| restore)
        {
            Some(saved) => self.board.clone_from(saved),
            None => {
                self.board = Board::from_level(level);
                move_pieces_to_toolbox(&mut self.board);
            }
        }
        self.check_win()
    }

    fn save_level_state(&mut self) {
        let id = usize::from(self.level_id());
        match &mut self.saved[id] {
            Some(saved) => saved.clone_from(&self.board),
            empty => *empty = Some(self.board.clone()),
        }
    }

    /// `check_win`: retrace, test every pinwheel, and on the first win of this slot mark it
    /// completed, save its board, ask for a save-file write and recompute the unlocks.
    pub(crate) fn check_win(&mut self) -> Effects {
        trace(&self.board, &mut self.field);
        let forced = std::mem::take(&mut self.force_win);
        self.won = forced || is_won(&self.board, &self.field);
        let mut effects = Effects::full_redraw();
        if self.won && !self.completed[self.slot] {
            self.completed[self.slot] = true;
            self.save_level_state();
            self.rules.unlock.apply(&self.completed, &mut self.unlocked);
            effects.write_save = true;
        }
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{board_index, toolbox_index};
    use crate::fixture::{tiny_game, tiny_game_of};
    use crate::piece::Piece;
    use crate::save::decode_save;
    use spectrograph::Game;

    fn fresh(data: &GameData) -> GameState<'_> {
        GameState::new(data, SaveData::default())
    }

    #[test]
    fn only_the_original_keys_and_the_cheat_are_read() {
        let c1 = tiny_game();
        let state = fresh(&c1);
        for ch in *b" =+-rL" {
            assert!(state.reads_key(ch), "{:?}", ch as char);
        }
        for ch in [b'x', b'R', b'/', 0x0f, 0x1b] {
            assert!(!state.reads_key(ch), "{ch:#x}");
        }
        let c4 = tiny_game_of(Game::C4);
        let state = fresh(&c4);
        assert!(state.reads_key(0x0f), "Control-O");
        assert!(!state.reads_key(b'L'));
    }

    #[test]
    fn a_new_game_starts_on_slot_0_with_pieces_in_the_toolbox() {
        let data = tiny_game();
        let state = fresh(&data);
        assert_eq!((state.slot(), state.level_id()), (0, 1));
        assert_eq!(
            state.board().cells[toolbox_index(0)],
            Cell {
                piece: Piece::Mirror,
                colour: 4,
                dir: 0
            }
        );
        assert_eq!(state.board().at(8, 7).piece, Piece::Empty);
        assert!(!state.is_won());
        assert_eq!(state.hint(), "one");
        assert_eq!(
            (0..4).map(|s| state.slot_state(s)).collect::<Vec<_>>(),
            [
                SlotState::Current,
                SlotState::Locked,
                SlotState::Locked,
                SlotState::Locked
            ]
        );
    }

    #[test]
    fn a_locked_next_level_does_nothing() {
        let data = tiny_game();
        let mut state = fresh(&data);
        assert_eq!(state.handle(InputEvent::Key(b' ')), Effects::default());
        assert_eq!(state.slot(), 0);
    }

    #[test]
    fn the_l_cheat_completes_the_level_and_opens_the_next() {
        let data = tiny_game();
        let mut state = fresh(&data);
        let effects = state.handle(InputEvent::Key(b'L'));
        assert_eq!(
            effects,
            Effects {
                redraw: Redraw::Full,
                write_save: true,
                clipboard: None
            }
        );
        assert!(state.is_won() && state.is_completed(0) && state.is_unlocked(1));
        assert_eq!(state.slot_state(1), SlotState::Unlocked);
        state.handle(InputEvent::Key(b'='));
        assert_eq!(
            (state.slot(), state.level_id(), state.hint()),
            (1, 2, "two")
        );
        assert!(!state.is_won(), "the cheat lasts one check");
        assert_eq!(state.slot_state(0), SlotState::Completed);
    }

    #[test]
    fn pasting_the_solution_wins_and_asks_for_a_save() {
        let data = tiny_game();
        let mut state = fresh(&data);
        assert_eq!(
            state.handle(InputEvent::Copy).clipboard.as_deref(),
            Some("1-01-er")
        );
        let effects = state.handle(InputEvent::Paste("1-01-hI".into()));
        assert!(effects.write_save);
        assert!(state.is_won());
        assert_eq!(
            state.handle(InputEvent::Copy).clipboard.as_deref(),
            Some("1-01-hI")
        );
        assert!(
            !state.handle(InputEvent::Paste("1-01-hI".into())).write_save,
            "only the first win saves"
        );
        assert_eq!(
            state.handle(InputEvent::Paste("nonsense".into())),
            Effects::default()
        );
    }

    #[test]
    fn leaving_a_level_keeps_its_board_and_r_resets_it() {
        let data = tiny_game();
        let mut state = fresh(&data);
        state.handle(InputEvent::Paste("1-01-hI".into()));
        state.handle(InputEvent::Key(b'+'));
        assert_eq!(state.slot(), 1);
        state.handle(InputEvent::Key(b'-'));
        assert_eq!(state.slot(), 0);
        assert_eq!(state.board().at(8, 7).piece, Piece::Mirror, "restored");
        assert!(state.is_won());
        state.handle(InputEvent::Key(b'r'));
        assert_eq!(
            state.board().at(8, 7).piece,
            Piece::Empty,
            "back in the toolbox"
        );
        assert!(!state.is_won());
        assert!(state.is_completed(0), "completion is never taken back");
    }

    #[test]
    fn previous_on_slot_0_restarts_from_the_saved_board() {
        let data = tiny_game();
        let mut state = fresh(&data);
        state.handle(InputEvent::Paste("1-01-hI".into()));
        assert_eq!(state.handle(InputEvent::Key(b'-')).redraw, Redraw::Full);
        assert_eq!(state.slot(), 0);
        assert!(state.is_won());
    }

    #[test]
    fn the_wheel_changes_level() {
        let data = tiny_game();
        let mut state = fresh(&data);
        state.handle(InputEvent::Key(b'L'));
        state.handle(InputEvent::Wheel { delta: -3 });
        assert_eq!(state.slot(), 1);
        state.handle(InputEvent::Wheel { delta: 1 });
        assert_eq!(state.slot(), 0);
        state.handle(InputEvent::Wheel { delta: 0 });
        assert_eq!(state.slot(), 1, "zero counts as next");
    }

    #[test]
    fn other_keys_do_nothing() {
        let data = tiny_game();
        let mut state = fresh(&data);
        for ch in [0x1b, b'l', b'R', b'x', b'_'] {
            assert_eq!(
                state.handle(InputEvent::Key(ch)),
                Effects::default(),
                "{ch}"
            );
        }
    }

    #[test]
    fn next_on_the_last_slot_stays_there() {
        let data = tiny_game();
        let save = SaveData {
            completed: [true; PROGRESS_SLOTS],
            ..SaveData::default()
        };
        let mut state = GameState::new(&data, save);
        state.goto_slot(LAST_SLOT);
        state.handle(InputEvent::Key(b' '));
        assert_eq!(state.slot(), LAST_SLOT);
    }

    #[test]
    fn extreme_pointer_coordinates_are_clamped_and_change_nothing() {
        let data = tiny_game();
        let mut state = fresh(&data);
        let before = state.board().clone();
        for (x, y) in [
            (i32::MIN, i32::MIN),
            (i32::MAX, i32::MAX),
            (i32::MIN, i32::MAX),
            (i32::MAX, i32::MIN),
        ] {
            for alt in [false, true] {
                state.handle(InputEvent::MouseDown { x, y, alt });
                state.handle(InputEvent::MouseDragged { x, y });
                state.handle(InputEvent::MouseUp { x, y, alt });
            }
        }
        assert_eq!(state.board(), &before);
    }

    #[test]
    fn changing_level_or_pasting_cancels_a_drag() {
        let data = tiny_game();
        let mut state = fresh(&data);
        state.handle(InputEvent::Key(b'L'));
        let start_drag = |state: &mut GameState| {
            state.handle(InputEvent::MouseDown {
                x: 460,
                y: 20,
                alt: false,
            });
            state.handle(InputEvent::MouseDragged { x: 100, y: 100 });
            assert!(state.drag().is_some());
        };
        for event in [
            InputEvent::Key(b'r'),
            InputEvent::Key(b' '),
            InputEvent::Key(b'-'),
        ] {
            start_drag(&mut state);
            state.handle(event.clone());
            assert_eq!(state.drag(), None, "{event:?}");
            state.handle(InputEvent::MouseUp {
                x: 60,
                y: 30,
                alt: false,
            });
            assert_eq!(
                state.board().at(0, 0).piece,
                Piece::Empty,
                "{event:?}: nothing is dropped"
            );
        }
        start_drag(&mut state);
        state.handle(InputEvent::Paste("1-01-hI".into()));
        assert_eq!(state.drag(), None);
        state.handle(InputEvent::MouseUp {
            x: 60,
            y: 30,
            alt: false,
        });
        assert_eq!(state.board().at(0, 0).piece, Piece::Empty);
        assert_eq!(state.board().at(8, 7).piece, Piece::Mirror);
    }

    #[test]
    fn quitting_saves_the_board_and_progress() {
        let data = tiny_game();
        let mut state = fresh(&data);
        state.handle(InputEvent::Paste("1-01-hI".into()));
        state.handle(InputEvent::Key(b' '));
        state.handle(InputEvent::Paste("1-02-lmzz".into())); // rejected: wrong length
        assert_eq!(
            state.handle(InputEvent::Quit),
            Effects {
                write_save: true,
                ..Effects::default()
            }
        );
        let save = decode_save(&RuleSet::C1, &state.save_bytes()).unwrap();
        assert!(save.completed[0] && !save.completed[1]);
        assert_eq!(
            save.boards[1].as_ref().unwrap().at(8, 7).piece,
            Piece::Mirror
        );
        assert!(
            save.boards[2].is_some(),
            "the level being played is saved on quit"
        );
    }

    #[test]
    fn a_save_with_too_few_boards_is_padded() {
        let data = tiny_game();
        let save = SaveData {
            boards: vec![],
            ..SaveData::default()
        };
        let mut state = GameState::new(&data, save);
        state.handle(InputEvent::Quit);
        let saved = decode_save(&RuleSet::C1, &state.save_bytes()).unwrap();
        assert_eq!(saved.boards.len(), RuleSet::C1.saved_ids);
        assert!(saved.boards[1].is_some());
    }

    #[test]
    fn a_save_restores_progress_and_the_first_board() {
        let data = tiny_game();
        let mut board = Board::from_level(data.level(1).unwrap());
        board.cells[board_index(0, 0)] = Cell {
            piece: Piece::Wall,
            colour: 0,
            dir: 0,
        };
        let mut save = SaveData::default();
        save.completed[0] = true;
        save.boards[1] = Some(board);
        let state = GameState::new(&data, save);
        assert!(state.is_won());
        assert_eq!(state.board().at(0, 0).piece, Piece::Wall);
        assert!(state.is_unlocked(1));
    }

    #[test]
    fn chromatron_2_cheats_with_z_and_opens_five_levels() {
        let data = tiny_game_of(Game::C2);
        let mut state = fresh(&data);
        assert_eq!(state.rules(), &RuleSet::C2);
        assert_eq!(state.board().game, Game::C2);
        assert_eq!(
            (0..6).map(|s| state.is_unlocked(s)).collect::<Vec<_>>(),
            [true, true, true, true, true, false]
        );
        assert_eq!(state.handle(InputEvent::Key(b'L')), Effects::default());
        assert!(state.handle(InputEvent::Key(b'z')).write_save);
        assert!(state.is_won() && state.is_completed(0));
        assert!(
            !state.is_unlocked(5),
            "after the first win four uncompleted slots stay open"
        );
    }

    #[test]
    fn chromatron_3_cheats_with_a_caret_and_keeps_25_levels_open() {
        let data = tiny_game_of(Game::C3);
        let mut state = fresh(&data);
        assert_eq!(state.rules(), &RuleSet::C3);
        assert!((0..25).all(|s| state.is_unlocked(s)) && !state.is_unlocked(25));
        for other in [b'z', b'L', 0x0f] {
            assert_eq!(state.handle(InputEvent::Key(other)), Effects::default());
        }
        assert!(state.handle(InputEvent::Key(b'^')).write_save);
        assert!(state.is_won() && state.is_completed(0));
        assert!(state.is_unlocked(25), "each first win opens one more");
    }

    #[test]
    fn chromatron_4_cheats_with_control_o() {
        let data = tiny_game_of(Game::C4);
        let mut state = fresh(&data);
        for other in *b"z^oO" {
            assert_eq!(state.handle(InputEvent::Key(other)), Effects::default());
        }
        assert!(state.handle(InputEvent::Key(0x0f)).write_save);
        assert!(state.is_won() && state.is_completed(0) && state.is_unlocked(25));
    }

    #[test]
    fn chromatron_3_and_4_copy_paste_and_save_in_their_own_formats() {
        for (game, digit) in [(Game::C3, '3'), (Game::C4, '4')] {
            let data = tiny_game_of(game);
            let mut state = fresh(&data);
            assert_eq!(
                state.handle(InputEvent::Copy).clipboard,
                Some(format!("{digit}-01-er"))
            );
            assert_eq!(
                state.handle(InputEvent::Paste("2-01-hI".into())),
                Effects::default()
            );
            assert!(
                state
                    .handle(InputEvent::Paste(format!("{digit}-01-hI")))
                    .write_save
            );
            let rules = RuleSet::of(game);
            let save = decode_save(rules, &state.save_bytes()).unwrap();
            assert!(save.completed[0] && !save.replay_wins);
            assert_eq!(save.boards.len(), 128);
            assert_eq!(save.boards[1].as_ref().unwrap().game, game);
        }
    }

    #[test]
    fn a_chromatron_3_save_without_its_marker_is_completed_by_its_solved_boards() {
        // platform.md P7: the bitmap is discarded and every slot whose saved board wins counts.
        let data = tiny_game_of(Game::C3);
        let mut boards = vec![None; 128];
        boards[1] = Some(Board::from_level(data.level(1).unwrap()));
        let mut unsolved = Board::from_level(data.level(2).unwrap());
        move_pieces_to_toolbox(&mut unsolved);
        boards[2] = Some(unsolved);
        let mut bitmap_says = [false; PROGRESS_SLOTS];
        bitmap_says[1] = true;
        bitmap_says[7] = true;
        let mut bytes = encode_save(&RuleSet::C3, &bitmap_says, &boards);
        bytes.remove(32);
        let save = decode_save(&RuleSet::C3, &bytes).unwrap();
        assert!(save.replay_wins);
        let state = GameState::new(&data, save);
        let completed: Vec<usize> = (0..STRIP_SLOTS)
            .filter(|&s| state.is_completed(s))
            .collect();
        assert_eq!(
            completed,
            (0..STRIP_SLOTS).step_by(2).collect::<Vec<_>>(),
            "id 1 (every even slot) is solved, id 2 is not, the bitmap's slots 1 and 7 are gone"
        );
        assert!(state.is_won());
        assert_eq!(
            state.save_bytes()[32],
            0xff,
            "the next write has the marker"
        );
    }

    #[test]
    fn chromatron_2_copies_pastes_and_saves_in_its_own_formats() {
        let data = tiny_game_of(Game::C2);
        let mut state = fresh(&data);
        assert_eq!(
            state.handle(InputEvent::Copy).clipboard.as_deref(),
            Some("2-01-er")
        );
        assert_eq!(
            state.handle(InputEvent::Paste("1-01-hI".into())),
            Effects::default()
        );
        assert!(state.handle(InputEvent::Paste("2-01-hI".into())).write_save);
        let bytes = state.save_bytes();
        assert_eq!(
            bytes[..3],
            [0x00, 0x00, 0x10],
            "platform.md P7: slot 0 only"
        );
        let save = decode_save(&RuleSet::C2, &bytes).unwrap();
        assert!(save.completed[0]);
        assert_eq!(save.boards.len(), 52);
        let board = save.boards[1].as_ref().unwrap();
        assert_eq!(
            (board.game, board.at(8, 7).piece),
            (Game::C2, Piece::Mirror)
        );
    }
}
