//! The page's handle on the games, driven by `web/main.js`: one `GameState` per game, started on
//! first use, the app's input rules (`lumen::gesture`), and the pixels and text the page draws.
//! Every input returns `Flag` bits. Buffers are allocated once in `new`: no input or render
//! allocates, except the strings that cross to JavaScript (copy, save, changed text).

use lumen::layout::{BOARD_RECT, FULL_FRAME, STRIP_SLOTS, TOOLBOX_RECT};
use lumen::{
    ButtonLatch, Damage, Effects, FRAME_HEIGHT, FRAME_WIDTH, GameState, Held, InputEvent,
    MAX_DAMAGE_RECTS, Rect, Redraw, RuleSet, SaveData, ScrollDelta, SlotState, TextBoxes,
    WheelGate, decode_save, resume_slot,
};
use spectrograph::Game;
use wasm_bindgen::prelude::*;

use crate::browser_keys::{browser_key_byte, pointer_button};
use crate::embedded_games::game_data;
use crate::pixels::{RGBA_LEN, rgba_rect};
use crate::save_text::{from_save_text, to_save_text};
use crate::stage::frame_point;

/// Bits of the `u32` every input returns.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    /// Something changed on screen: schedule one `render`.
    Redraw = 1,
    /// Store `save_text()` for the current game now (a first win, or the cheat).
    Save = 2,
    /// The level changed: store `slot()` as the game's last level.
    Slot = 4,
    /// A key reached the game: the page calls `preventDefault`.
    Consumed = 8,
    /// `select` could not use the stored save and started fresh: keep the old text aside.
    CorruptSave = 16,
}

impl Flag {
    /// Whether `flags` has this bit.
    pub fn is_in(self, flags: u32) -> bool {
        flags & self as u32 != 0
    }
}

/// How the strip shows a level (`lumen::SlotState`), for the page's Levels picker.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LevelState {
    /// The level being played.
    Current = 0,
    /// Solved before.
    Completed = 1,
    /// Open, not solved yet.
    Unlocked = 2,
    /// Not open yet.
    Locked = 3,
}

impl From<SlotState> for LevelState {
    fn from(state: SlotState) -> LevelState {
        match state {
            SlotState::Current => LevelState::Current,
            SlotState::Completed => LevelState::Completed,
            SlotState::Unlocked => LevelState::Unlocked,
            SlotState::Locked => LevelState::Locked,
        }
    }
}

#[wasm_bindgen]
pub struct Session {
    /// One game per number 1–4, at index number − 1, created by the first `select`.
    games: [Option<GameState<'static>>; 4],
    current: Option<usize>,
    /// The 640 × 480 frame `lumen::render` draws into.
    frame: Vec<u32>,
    /// The frame as RGBA for `ImageData`; only damaged rects are rewritten.
    rgba: Vec<u8>,
    damage: Damage,
    /// The rects the last `render` rewrote in `rgba`, `[left, top, right, bottom]`.
    dirty: [[i32; 4]; MAX_DAMAGE_RECTS],
    dirty_len: usize,
    /// The text boxes the page shows.
    text: TextBoxes<'static>,
    /// The page's text is out of date (another game was selected): report it at the next render.
    text_stale: bool,
    text_changed: bool,
    latch: ButtonLatch,
    buttons_held: u32,
    wheel: WheelGate,
}

impl Default for Session {
    fn default() -> Session {
        Session::new()
    }
}

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Session {
        Session {
            games: [None, None, None, None],
            current: None,
            frame: vec![0; FRAME_WIDTH * FRAME_HEIGHT],
            rgba: vec![0; RGBA_LEN],
            damage: Damage::default(),
            dirty: [[0; 4]; MAX_DAMAGE_RECTS],
            dirty_len: 0,
            text: TextBoxes::default(),
            text_stale: true,
            text_changed: false,
            latch: ButtonLatch::default(),
            buttons_held: 0,
            wheel: WheelGate::default(),
        }
    }

    /// Makes game `game` (1–4) the current one, ending any drag in the previous one. The first
    /// time, the game starts from `save` (the base64 save text, `None` for a fresh game) and
    /// reopens `last_slot` if it is open (`resume_slot`); a save that does not decode starts a
    /// fresh game and sets `Flag::CorruptSave`. Later selects resume the running game and ignore
    /// both arguments. An unknown game number does nothing and returns 0.
    pub fn select(&mut self, game: u8, save: Option<String>, last_slot: Option<u32>) -> u32 {
        let Some(game) = Game::from_number(game) else {
            return 0;
        };
        let index = usize::from(game.number() - 1);
        self.pointer_cancel();
        let mut flags = Flag::Redraw as u32;
        if self.games[index].is_none() {
            let rules = RuleSet::of(game);
            let save = match save.as_deref().map(|text| decode_text(rules, text)) {
                None => SaveData::fresh(rules),
                Some(Some(save)) => save,
                Some(None) => {
                    flags |= Flag::CorruptSave as u32;
                    SaveData::fresh(rules)
                }
            };
            let mut state = GameState::new(game_data(game), save);
            let stored = last_slot.map(i64::from);
            if let Some(slot) = resume_slot(stored, STRIP_SLOTS, |s| state.is_unlocked(s)) {
                state.goto_slot(slot);
            }
            self.games[index] = Some(state);
        }
        self.current = Some(index);
        self.damage.add(Redraw::Full);
        self.text_stale = true;
        flags
    }

    /// A button went down at (`x`, `y`) frame pixels (fractional). `button` is
    /// `PointerEvent.button`; Shift, Option (`alt`) or Control make a primary press clockwise.
    pub fn pointer_down(
        &mut self,
        x: f64,
        y: f64,
        button: i16,
        shift: bool,
        alt: bool,
        ctrl: bool,
    ) -> u32 {
        if self.current.is_none() {
            return 0;
        }
        let (x, y) = frame_point(x, y);
        self.buttons_held = self.buttons_held.saturating_add(1);
        let held = Held {
            shift,
            option: alt,
            control: ctrl,
        };
        let alt = self.latch.press(pointer_button(button), held);
        self.send(InputEvent::MouseDown { x, y, alt })
    }

    /// The pointer moved; only a held button makes it a drag.
    pub fn pointer_move(&mut self, x: f64, y: f64) -> u32 {
        if self.buttons_held == 0 {
            return 0;
        }
        let (x, y) = frame_point(x, y);
        self.send(InputEvent::MouseDragged { x, y })
    }

    /// A button went up. A release without a press (after `pointer_cancel`) is dropped, or it
    /// would rotate a piece.
    pub fn pointer_up(&mut self, x: f64, y: f64, button: i16) -> u32 {
        let Some(held) = self.buttons_held.checked_sub(1) else {
            return 0;
        };
        self.buttons_held = held;
        let (x, y) = frame_point(x, y);
        let alt = self.latch.release(pointer_button(button));
        self.send(InputEvent::MouseUp { x, y, alt })
    }

    /// The pointer was lost (`pointercancel`, the window lost focus, the tab was hidden): end
    /// any drag as the app does on focus loss (the piece stays put) and forget the buttons.
    pub fn pointer_cancel(&mut self) -> u32 {
        if self.buttons_held == 0 {
            return 0;
        }
        self.buttons_held = 0;
        self.send(InputEvent::MouseUp {
            x: -1,
            y: -1,
            alt: false,
        })
    }

    /// Whether the frame point (`x`, `y`) is over a level number on the strip. The page sends a
    /// strip tap's release as button 0 even while taps turn clockwise, because the game ignores
    /// alternate releases on the strip.
    pub fn on_strip(&self, x: f64, y: f64) -> bool {
        let (x, y) = frame_point(x, y);
        matches!(lumen::hit_test(x, y), lumen::Hit::Strip(_))
    }

    /// A `keydown`: `KeyboardEvent.key`, Control (without AltGraph), Meta and `repeat`.
    /// `Flag::Consumed` only for a key the game reads (`GameState::reads_key`); the page then
    /// calls `preventDefault`, and every other key stays the browser's.
    pub fn key(&mut self, key: &str, ctrl: bool, meta: bool, repeat: bool) -> u32 {
        let Some(cheat) = self.state().map(|state| state.rules().cheat_key) else {
            return 0;
        };
        let Some(byte) = browser_key_byte(key, ctrl, meta, repeat, cheat) else {
            return 0;
        };
        let read = self.state().is_some_and(|state| state.reads_key(byte));
        let flags = self.send(InputEvent::Key(byte));
        if read {
            flags | Flag::Consumed as u32
        } else {
            flags
        }
    }

    /// A `wheel` event: `deltaX`/`deltaY`, whether `deltaMode` is pixels, and the event time in
    /// milliseconds. Browsers report the scroll direction (positive y: scrolling down, so the
    /// content moves up) where winit and `WheelGate` take content movement, so the signs flip:
    /// on both platforms scrolling up goes to the previous level.
    pub fn wheel(&mut self, dx: f64, dy: f64, pixels: bool, now_ms: f64) -> u32 {
        if self.current.is_none() {
            return 0;
        }
        let (x, y) = (-dx, -dy);
        let delta = if pixels {
            ScrollDelta::Pixels { x, y }
        } else {
            ScrollDelta::Lines { x, y }
        };
        match self.wheel.feed(delta, now_ms) {
            Some(delta) => self.send(InputEvent::Wheel { delta }),
            None => 0,
        }
    }

    /// Copy Level: the current board as the original's clipboard text.
    pub fn copy(&mut self) -> Option<String> {
        self.state_mut()?.handle(InputEvent::Copy).clipboard
    }

    /// Paste Level. Surrounding whitespace (text copied from a chat) is dropped first; a solution
    /// has none. A text the game rejects returns 0 and changes nothing.
    pub fn paste(&mut self, text: &str) -> u32 {
        self.send(InputEvent::Paste(text.trim().to_owned()))
    }

    /// A toolbar command, sent as the menu key the app sends: `reset` (`r`), `next` (`=`),
    /// `previous` (`-`). Anything else does nothing.
    pub fn command(&mut self, name: &str) -> u32 {
        let key = match name {
            "reset" => b'r',
            "next" => b'=',
            "previous" => b'-',
            _ => return 0,
        };
        self.send(InputEvent::Key(key))
    }

    /// The page is going away or switching games: game `game` saves its level's board as the app
    /// does on quit, and its save text is returned (`None` if it never started).
    pub fn quit(&mut self, game: u8) -> Option<String> {
        let index = usize::from(Game::from_number(game)?.number() - 1);
        let state = self.games[index].as_mut()?;
        state.handle(InputEvent::Quit);
        Some(to_save_text(&state.save_bytes()))
    }

    /// Drops game `game`'s running state (another tab saved newer progress for it), so the next
    /// `select` starts it again from the save it is given. If it was the current game, no game is
    /// current until then. Returns whether the game was running.
    pub fn forget(&mut self, game: u8) -> bool {
        let Some(index) = Game::from_number(game).map(|g| usize::from(g.number() - 1)) else {
            return false;
        };
        if self.current == Some(index) {
            self.pointer_cancel();
            self.current = None;
        }
        self.games[index].take().is_some()
    }

    /// The current game's save, as base64 text of the original `.dat` bytes.
    pub fn save_text(&self) -> Option<String> {
        self.state().map(|state| to_save_text(&state.save_bytes()))
    }

    /// The current strip slot, 0-based (0 before any `select`).
    pub fn slot(&self) -> u32 {
        self.state().map_or(0, |state| state.slot() as u32)
    }

    /// The current game, 1–4 (0 before any `select`).
    pub fn game(&self) -> u8 {
        self.state().map_or(0, |state| state.data().game.number())
    }

    /// Whether a piece is being dragged. The page sends a touch release as button 0 then, so the
    /// piece drops even while taps turn clockwise.
    pub fn dragging(&self) -> bool {
        self.state().is_some_and(|state| state.drag().is_some())
    }

    /// Whether the current level is won.
    pub fn won(&self) -> bool {
        self.state().is_some_and(GameState::is_won)
    }

    /// The original's window title, such as "Chromatron 2 (v1.4)".
    pub fn title(&self) -> String {
        self.state().map_or_else(String::new, |state| {
            state.data().ui.window_title.to_string()
        })
    }

    /// Strip slot `slot`'s state (0-based), as the strip colours its number. `Locked` before any
    /// `select` and past the strip.
    pub fn slot_state(&self, slot: u32) -> LevelState {
        let slot = slot as usize;
        match self.state() {
            Some(state) if slot < STRIP_SLOTS => state.slot_state(slot).into(),
            _ => LevelState::Locked,
        }
    }

    /// Opens strip slot `slot` (0-based) as a tap on its number does: only an unlocked slot opens,
    /// and the current one starts again from its saved board. Ends any drag first. Returns 0 for a
    /// locked slot, a slot past the strip, or before any `select`.
    pub fn open_slot(&mut self, slot: u32) -> u32 {
        let slot = slot as usize;
        let open = slot < STRIP_SLOTS && self.state().is_some_and(|state| state.is_unlocked(slot));
        if !open {
            return 0;
        }
        self.pointer_cancel() | self.act(|state| state.goto_slot(slot))
    }

    /// The board in frame pixels, `[left, top, right, bottom]` (`lumen::layout::BOARD_RECT`): the
    /// page's portrait board view.
    pub fn board_rect(&self) -> Vec<i32> {
        rect_vec(BOARD_RECT)
    }

    /// The visible toolbox in frame pixels, `[left, top, right, bottom]`
    /// (`lumen::layout::TOOLBOX_RECT`): the page's portrait toolbox view.
    pub fn toolbox_rect(&self) -> Vec<i32> {
        rect_vec(TOOLBOX_RECT)
    }

    /// Where the strip draws slot `slot`'s number (0-based), `[left, top, right, bottom]` in frame
    /// pixels (`lumen::strip_number_rect`); empty past the strip.
    pub fn strip_number_rect(&self, slot: u32) -> Vec<i32> {
        let slot = slot as usize;
        if slot < STRIP_SLOTS {
            rect_vec(lumen::strip_number_rect(slot))
        } else {
            Vec::new()
        }
    }

    /// Draws the frame if anything changed and converts only the damaged rects to RGBA. Returns
    /// how many rects `dirty_ptr` now holds (0: nothing to draw). Also decides `text_changed`.
    pub fn render(&mut self) -> usize {
        self.dirty_len = 0;
        self.text_changed = false;
        let Some(state) = self.current.and_then(|i| self.games[i].as_ref()) else {
            return 0;
        };
        let damage = self.damage.take();
        if damage.is_none() {
            return 0;
        }
        let boxes = lumen::render(state, &mut self.frame);
        let full = [FULL_FRAME];
        let rects: &[Rect] = if damage == Damage::Full {
            &full
        } else {
            damage.rects()
        };
        for &rect in rects {
            if let Some(r) = rgba_rect(&self.frame, &mut self.rgba, rect) {
                self.dirty[self.dirty_len] = [r.left, r.top, r.right, r.bottom];
                self.dirty_len += 1;
            }
        }
        if self.text_stale || boxes.as_slice() != self.text.as_slice() {
            self.text = boxes;
            self.text_stale = false;
            self.text_changed = true;
        }
        self.dirty_len
    }

    /// The RGBA frame (640 × 480 × 4 bytes) in wasm memory.
    pub fn rgba_ptr(&self) -> *const u8 {
        self.rgba.as_ptr()
    }

    /// The last render's dirty rects: `render()` × `[left, top, right, bottom]` as `i32`s.
    pub fn dirty_ptr(&self) -> *const i32 {
        self.dirty.as_ptr().cast()
    }

    /// Whether the last `render` changed the text boxes; the page then redraws its text layer.
    pub fn text_changed(&self) -> bool {
        self.text_changed
    }

    /// How many text boxes the page shows.
    pub fn text_count(&self) -> usize {
        self.text.as_slice().len()
    }

    /// Text box `i`'s string ("" out of range).
    pub fn text(&self, i: usize) -> String {
        self.text
            .as_slice()
            .get(i)
            .map_or_else(String::new, |b| b.text.to_owned())
    }

    /// Text box `i`'s rect in frame pixels, `[left, top, right, bottom]` (empty out of range).
    pub fn text_rect(&self, i: usize) -> Vec<i32> {
        self.text
            .as_slice()
            .get(i)
            .map_or_else(Vec::new, |b| rect_vec(b.rect))
    }
}

/// Rust-only views for native tests.
impl Session {
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub fn dirty_rects(&self) -> &[[i32; 4]] {
        &self.dirty[..self.dirty_len]
    }

    fn state(&self) -> Option<&GameState<'static>> {
        self.games[self.current?].as_ref()
    }

    fn state_mut(&mut self) -> Option<&mut GameState<'static>> {
        self.games[self.current?].as_mut()
    }

    /// Hands one event to the current game and turns its effects into flags.
    fn send(&mut self, event: InputEvent) -> u32 {
        self.act(|state| state.handle(event))
    }

    /// Runs `action` on the current game and turns its effects into flags.
    fn act(&mut self, action: impl FnOnce(&mut GameState<'static>) -> Effects) -> u32 {
        let Some(index) = self.current else {
            return 0;
        };
        let Some(state) = self.games[index].as_mut() else {
            return 0;
        };
        let slot = state.slot();
        let effects = action(state);
        let mut flags = 0;
        if state.slot() != slot {
            flags |= Flag::Slot as u32;
        }
        if effects.write_save {
            flags |= Flag::Save as u32;
        }
        if effects.redraw != Redraw::None {
            self.damage.add(effects.redraw);
            flags |= Flag::Redraw as u32;
        }
        flags
    }
}

/// A rect as the page reads it: `[left, top, right, bottom]`.
fn rect_vec(rect: Rect) -> Vec<i32> {
    vec![rect.left, rect.top, rect.right, rect.bottom]
}

/// A stored save text as `SaveData`; `None` when it is not base64 or not a valid save.
fn decode_text(rules: &RuleSet, text: &str) -> Option<SaveData> {
    decode_save(rules, &from_save_text(text)?).ok()
}
