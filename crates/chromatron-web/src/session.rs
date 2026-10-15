//! The page's handle on the games, driven by `web/main.js`: one `GameState` per game, started on
//! first use, the app's input rules (`lumen::gesture`), and the pixels and text the page draws.
//! Every input returns `Flag` bits. Buffers are allocated in `new` and `resize` only: no input or
//! `render_at` allocates, except the strings and rects that cross to JavaScript (copy, save,
//! changed text, captions).

use halation::{Captions, Control, Easel, Insets, Lightbox, Output, Route, Scene, Theme};
use lumen::layout::{BOARD_RECT, FULL_FRAME, STRIP_SLOTS, TOOLBOX_RECT};
use lumen::{
    ButtonLatch, Damage, Effects, FRAME_HEIGHT, FRAME_WIDTH, Floorplan, GameState, Held, Hit,
    InputEvent, MAX_DAMAGE_RECTS, Rect, Redraw, RuleSet, SaveData, ScrollDelta, SlotState,
    TextBoxes, WheelGate, decode_save, resume_slot,
};
use spectrograph::Game;
use wasm_bindgen::prelude::*;

use crate::browser_keys::{browser_key_byte, pointer_button};
use crate::embedded_games::game_data;
use crate::pixels::{RGBA_LEN, rgba_rect};
use crate::save_text::{from_save_text, to_save_text};
use crate::stage::{device_point, frame_point};

const _: () = assert!(MAX_DAMAGE_RECTS <= halation::MAX_DAMAGE);

/// Which look the page shows: the original's 640 × 480 frame, or halation's modern canvas.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Classic = 0,
    Modern = 1,
}

/// Bits of the `u32` every input returns.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    /// Something changed on screen: schedule one `render_at`.
    Redraw = 1,
    /// Store `save_text()` for the current game now (a first win, or the cheat).
    Save = 2,
    /// The level changed: store `slot()` as the game's last level.
    Slot = 4,
    /// A key reached the game: the page calls `preventDefault`.
    Consumed = 8,
    /// `select` could not use the stored save and started fresh: keep the old text aside.
    CorruptSave = 16,
    /// The sheet opened: the page sends `open_levels`.
    SheetOpened = 32,
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

/// `caption_style`'s numbers, which the page maps to its CSS classes: `halation::CaptionStyle`'s
/// order, pinned below at compile time.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionStyle {
    Title = 0,
    TitleCompact = 1,
    Body = 2,
    Badge = 3,
    Credit = 4,
    Pill = 5,
    /// Read by screen readers only.
    Hidden = 6,
}

/// The `caption_role` numbers the page reads: lumen's `TextRole` indices, pinned below.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionRole {
    Win = 0,
    Hint = 3,
    ClickNext = 4,
}

const _: () = {
    use halation::CaptionStyle as Style;
    use lumen::TextRole;
    assert!(CaptionStyle::Title as u8 == Style::Title as u8);
    assert!(CaptionStyle::TitleCompact as u8 == Style::TitleCompact as u8);
    assert!(CaptionStyle::Body as u8 == Style::Body as u8);
    assert!(CaptionStyle::Badge as u8 == Style::Badge as u8);
    assert!(CaptionStyle::Credit as u8 == Style::Credit as u8);
    assert!(CaptionStyle::Pill as u8 == Style::Pill as u8);
    assert!(CaptionStyle::Hidden as u8 == Style::Hidden as u8);
    assert!(CaptionRole::Win as u8 == TextRole::Win as u8);
    assert!(CaptionRole::Hint as u8 == TextRole::Hint as u8);
    assert!(CaptionRole::ClickNext as u8 == TextRole::ClickNext as u8);
};

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
    /// The rects the last `render_at` rewrote in `rgba` (or, modern, `canvas`),
    /// `[left, top, right, bottom]`.
    dirty: [[i32; 4]; halation::MAX_DAMAGE],
    dirty_len: usize,
    look: Look,
    /// The modern look's painter; made by the first `resize`.
    lightbox: Option<Lightbox>,
    /// The modern canvas as RGBA, sized by `resize` only.
    canvas: Vec<u8>,
    /// What `resize` gives each new lightbox: the theme and reduced motion.
    theme: Theme,
    reduced_motion: bool,
    /// The card's captions from the last modern `render_at`, which the page typesets.
    captions: Captions<'static>,
    captions_changed: bool,
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
            dirty: [[0; 4]; halation::MAX_DAMAGE],
            dirty_len: 0,
            look: Look::Classic,
            lightbox: None,
            canvas: Vec::new(),
            theme: Theme::Dark,
            reduced_motion: false,
            captions: Captions::default(),
            captions_changed: false,
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
    /// both arguments. An open level sheet stays open (its chips show the new game's levels). An
    /// unknown game number does nothing and returns 0.
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
        if let Some(lightbox) = &mut self.lightbox {
            lightbox.reset();
        }
        self.damage.add(Redraw::Full);
        self.text_stale = true;
        flags
    }

    /// The look being shown.
    pub fn look(&self) -> Look {
        self.look
    }

    /// Switches look: ends any drag, closes the level sheet, repaints the new look whole (frame
    /// and text, or canvas). The classic look has no captions: they are cleared. The same look
    /// returns 0.
    pub fn set_look(&mut self, look: Look) -> u32 {
        if look == self.look {
            return 0;
        }
        let flags = self.pointer_cancel();
        self.look = look;
        if look == Look::Classic {
            self.captions = Captions::default();
            self.captions_changed = false;
        }
        if let Some(lightbox) = &mut self.lightbox {
            lightbox.command(Control::CloseSheet);
            lightbox.reset();
        }
        self.damage.add(Redraw::Full);
        self.text_stale = true;
        flags | Flag::Redraw as u32
    }

    /// The modern canvas is `width_px × height_px` backing px at `px_per_css` backing px per CSS
    /// px, with `safe` insets (top, right, bottom, left) in CSS px, laid out for touch when
    /// `touch` (the page's touch layouts). Modern only: the page calls it only in the modern look,
    /// and it builds a new lightbox and canvas. Ends any drag and forgets the held buttons, so the
    /// release of a press made before it (on a control or not) never reaches the game. A level
    /// sheet that was open stays open when the new layout still has a sheet picker (ruling 18);
    /// the theme and reduced motion carry over. The next `render_at` paints the canvas whole.
    /// Degenerate sizes are clamped (1 × 1 at least).
    pub fn resize(
        &mut self,
        width_px: u32,
        height_px: u32,
        px_per_css: f64,
        safe: Vec<f64>,
        touch: bool,
    ) -> u32 {
        let flags = self.pointer_cancel();
        let inset = |i: usize| safe.get(i).copied().unwrap_or(0.0) as f32;
        let easel = Easel::new(width_px, height_px, px_per_css as f32)
            .with_safe(Insets {
                top: inset(0),
                right: inset(1),
                bottom: inset(2),
                left: inset(3),
            })
            .with_touch(touch);
        // A sheet whose chip was just chosen is closing (at the next paint): it stays closed.
        let sheet = self
            .lightbox
            .as_ref()
            .is_some_and(|l| l.sheet_open() && !l.sheet_closing());
        // The canvas and the lightbox are given back whole before new ones are made, the canvas
        // (the biggest buffer) first: the allocator then reuses the one space they leave. Resized
        // buffer by buffer, each would leave a hole too small for the next, and a live resize
        // would pile them up (wasm memory never shrinks).
        self.lightbox = None;
        self.canvas = Vec::new();
        self.canvas
            .resize(easel.width as usize * easel.height as usize * 4, 0);
        let mut lightbox = Lightbox::new(easel, self.theme, self.reduced_motion);
        if sheet && !lightbox.has_inline_picker() {
            lightbox.command(Control::Levels);
        }
        self.lightbox = Some(lightbox);
        flags | Flag::Redraw as u32
    }

    /// Spec §9.3: the visitor's `prefers-reduced-motion`. Kept for every lightbox (it survives
    /// `resize`); a running animation settles at once. `Flag::Redraw` when the modern canvas must
    /// paint again to show it settled; 0 when nothing changes.
    pub fn set_reduced_motion(&mut self, on: bool) -> u32 {
        if on == self.reduced_motion {
            return 0;
        }
        self.reduced_motion = on;
        let Some(lightbox) = &mut self.lightbox else {
            return 0;
        };
        lightbox.set_reduced_motion(on);
        if self.look == Look::Modern {
            Flag::Redraw as u32
        } else {
            0
        }
    }

    /// Spec §11–12: the visitor's `prefers-color-scheme` (`dark`: true for dark). Kept for every
    /// lightbox (it survives `resize`, and a classic page keeps it for modern). The modern canvas
    /// repaints cold in the new theme, with no cue; a held piece stays held (ruling L10).
    /// `Flag::Redraw` when the modern canvas must paint again; 0 when nothing changes (the same
    /// scheme, or the classic look, whose page stays dark).
    pub fn set_theme(&mut self, dark: bool) -> u32 {
        let theme = if dark { Theme::Dark } else { Theme::Light };
        if theme == self.theme {
            return 0;
        }
        self.theme = theme;
        let Some(lightbox) = &mut self.lightbox else {
            return 0;
        };
        lightbox.set_theme(theme);
        if self.look == Look::Modern {
            Flag::Redraw as u32
        } else {
            0
        }
    }

    /// Whether the modern layout draws its level picker on the canvas (no sheet, no Levels
    /// pill). False before the first `resize`.
    pub fn has_inline_picker(&self) -> bool {
        self.lightbox
            .as_ref()
            .is_some_and(Lightbox::has_inline_picker)
    }

    /// Whether the level sheet is open over the modern canvas.
    pub fn sheet_open(&self) -> bool {
        self.lightbox.as_ref().is_some_and(Lightbox::sheet_open)
    }

    /// Modern: whether nothing answers a press at (`x`, `y`) (canvas backing px): no board cell,
    /// tray slot, chip or control is there (the card, the margins), so the page may pan the page
    /// from it (ruling 19). False in the classic look and before the first `resize`.
    pub fn inert(&self, x: f64, y: f64) -> bool {
        let (Look::Modern, Some(lightbox)) = (self.look, &self.lightbox) else {
            return false;
        };
        let (x, y) = device_point(x, y);
        matches!(lightbox.hit(x, y), Hit::Nothing) && lightbox.control_at(x, y).is_none()
    }

    /// Whether the current level is won and Next ▶ shows: the level is not the last and the next
    /// one is open (the page's touch wording, ruling 21).
    pub fn next_open(&self) -> bool {
        self.state()
            .is_some_and(|state| Scene::of(state).next_open())
    }

    /// Modern: board cell or tray slot `index`'s square (`lumen` cell indices) in canvas backing
    /// px, `[left, top, right, bottom]`; empty for other indices and before the first `resize`.
    pub fn cell_rect(&self, index: usize) -> Vec<i32> {
        self.lightbox.as_ref().map_or_else(Vec::new, |lightbox| {
            area_vec(lightbox.tableau().cell_rect(index))
        })
    }

    /// Modern: level chip `slot`'s square (0-based) in canvas backing px, `[left, top, right,
    /// bottom]`: the sheet's while it is open, else the inline picker's. Empty past the strip,
    /// with neither picker showing, and before the first `resize`.
    pub fn chip_rect(&self, slot: u32) -> Vec<i32> {
        let Some(lightbox) = &self.lightbox else {
            return Vec::new();
        };
        let (tableau, slot) = (lightbox.tableau(), slot as usize);
        area_vec(if lightbox.sheet_open() {
            tableau.sheet_chip_rect(slot)
        } else {
            tableau.chip_rect(slot)
        })
    }

    /// The modern canvas width in backing px (0 before the first `resize`).
    pub fn canvas_width(&self) -> u32 {
        self.lightbox.as_ref().map_or(0, |l| l.easel().width)
    }

    /// The modern canvas height in backing px (0 before the first `resize`).
    pub fn canvas_height(&self) -> u32 {
        self.lightbox.as_ref().map_or(0, |l| l.easel().height)
    }

    /// When the next modern frame is due, in `now_ms` terms; -1 when nothing animates (always in
    /// the classic look).
    pub fn next_wake(&self, now_ms: f64) -> f64 {
        match (&self.lightbox, self.look) {
            (Some(lightbox), Look::Modern) => lightbox.next_wake(now_ms).unwrap_or(-1.0),
            _ => -1.0,
        }
    }

    /// A button went down at (`x`, `y`), fractional: frame pixels in the classic look, canvas
    /// backing px in the modern one (as for every pointer call; there the canvas's controls see
    /// it before the game). `button` is `PointerEvent.button`; Shift, Option (`alt`) or Control
    /// make a primary press clockwise.
    pub fn pointer_down(
        &mut self,
        x: f64,
        y: f64,
        button: i16,
        shift: bool,
        alt: bool,
        ctrl: bool,
    ) -> u32 {
        if self.current.is_none() || self.canvasless() {
            return 0;
        }
        let (x, y) = self.point(x, y);
        self.buttons_held = self.buttons_held.saturating_add(1);
        let held = Held {
            shift,
            option: alt,
            control: ctrl,
        };
        let alt = self.latch.press(pointer_button(button), held);
        self.route(InputEvent::MouseDown { x, y, alt })
    }

    /// The pointer moved; only a held button makes it a drag.
    pub fn pointer_move(&mut self, x: f64, y: f64) -> u32 {
        if self.buttons_held == 0 {
            return 0;
        }
        let (x, y) = self.point(x, y);
        self.route(InputEvent::MouseDragged { x, y })
    }

    /// A button went up. A release without a press (after `pointer_cancel`, `resize` or
    /// `command("levels")`) is dropped, or it would rotate a piece or open a level. In the modern
    /// look a press on a control fires it only when released over the same control
    /// (`Flag::SheetOpened` when that opened the sheet).
    pub fn pointer_up(&mut self, x: f64, y: f64, button: i16) -> u32 {
        let Some(held) = self.buttons_held.checked_sub(1) else {
            return 0;
        };
        self.buttons_held = held;
        let (x, y) = self.point(x, y);
        let alt = self.latch.release(pointer_button(button));
        self.route(InputEvent::MouseUp { x, y, alt })
    }

    /// The pointer was lost (`pointercancel`, the window lost focus, the tab was hidden): end
    /// any drag as the app does on focus loss (the piece stays put), forget a press on a canvas
    /// control and forget the buttons.
    pub fn pointer_cancel(&mut self) -> u32 {
        if let Some(lightbox) = &mut self.lightbox {
            lightbox.forget_press();
        }
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

    /// Whether the point (`x`, `y`) (frame pixels, or canvas backing px in the modern look) is
    /// over a level number on the strip. The page sends a strip tap's release as button 0 even
    /// while taps turn clockwise, because the game ignores alternate releases on the strip.
    pub fn on_strip(&self, x: f64, y: f64) -> bool {
        let (x, y) = self.point(x, y);
        let hit = match (&self.lightbox, self.look) {
            (Some(lightbox), Look::Modern) => lightbox.hit(x, y),
            (None, Look::Modern) => return false,
            _ => lumen::hit_test(x, y),
        };
        matches!(hit, Hit::Strip(_))
    }

    /// A `keydown`: `KeyboardEvent.key`, Control (without AltGraph), Meta and `repeat`.
    /// `Flag::Consumed` only for a key the game reads (`GameState::reads_key`); the page then
    /// calls `preventDefault`, and every other key stays the browser's. In the modern look Escape
    /// closes an open level sheet first (`Consumed`), ending any held press as every other way of
    /// closing it does, so its release reaches nothing; the game never sees that Escape.
    pub fn key(&mut self, key: &str, ctrl: bool, meta: bool, repeat: bool) -> u32 {
        if key == "Escape" && self.look == Look::Modern && self.sheet_open() {
            return self.pointer_cancel()
                | self.toggle(Control::CloseSheet)
                | Flag::Consumed as u32;
        }
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
    /// `previous` (`-`). `levels`, in the modern look, ends any drag (its flags) and then opens or
    /// closes the level sheet (`Redraw`, plus `SheetOpened` when it opened); with an inline picker
    /// there is no sheet, so only the drag's end is reported. In the classic look the page opens
    /// its own dialog, so `levels` returns 0. Anything else does nothing.
    pub fn command(&mut self, name: &str) -> u32 {
        let key = match name {
            "reset" => b'r',
            "next" => b'=',
            "previous" => b'-',
            "levels" if self.look == Look::Modern => {
                return self.pointer_cancel() | self.toggle(Control::Levels);
            }
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
        if let Some(lightbox) = &mut self.lightbox {
            lightbox.reset();
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

    /// The game's "click for the next level" text, shown in the hint box once a level is won
    /// (the page words it differently on a touch screen). "" before any `select`.
    pub fn click_next(&self) -> String {
        self.state()
            .map_or_else(String::new, |state| state.data().ui.click_next.to_string())
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

    /// Draws whatever changed and returns how many rects `dirty_ptr` now holds (0: nothing to
    /// draw). Classic draws the frame, converts only the damaged rects to RGBA and decides
    /// `text_changed`; it ignores `now_ms`. Modern paints the canvas at `now_ms` and reports the
    /// card's captions instead of text boxes (`captions_changed`).
    pub fn render_at(&mut self, now_ms: f64) -> usize {
        self.dirty_len = 0;
        self.text_changed = false;
        self.captions_changed = false;
        if self.look == Look::Modern {
            let (Some(lightbox), Some(state)) = (
                self.lightbox.as_mut(),
                self.current.and_then(|i| self.games[i].as_ref()),
            ) else {
                return 0;
            };
            let painted = lightbox.paint(state, now_ms, Output::Rgba(&mut self.canvas));
            self.captions = painted.captions;
            self.captions_changed = painted.captions_changed;
            for (slot, r) in self.dirty.iter_mut().zip(painted.damage) {
                *slot = [r.left, r.top, r.right, r.bottom];
            }
            self.dirty_len = painted.damage.len().min(self.dirty.len());
            return self.dirty_len;
        }
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

    /// The RGBA pixels in wasm memory: the 640 × 480 frame, or in the modern look the canvas
    /// (`canvas_width × canvas_height × 4` bytes).
    pub fn rgba_ptr(&self) -> *const u8 {
        self.rgba().as_ptr()
    }

    /// The last `render_at`'s dirty rects: its return value × `[left, top, right, bottom]` as
    /// `i32`s, in frame pixels (classic) or canvas backing px (modern).
    pub fn dirty_ptr(&self) -> *const i32 {
        self.dirty.as_ptr().cast()
    }

    /// Whether the last `render_at` changed the text boxes; the page then redraws its text layer.
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

    /// Whether the last `render_at` (modern) changed the captions; the page then sets its caption
    /// spans again. Always after a cold paint; never in the classic look.
    pub fn captions_changed(&self) -> bool {
        self.captions_changed
    }

    /// How many captions the modern card shows (0 while the level sheet covers it).
    pub fn caption_count(&self) -> usize {
        self.captions.as_slice().len()
    }

    /// Caption `i`'s text ("" out of range).
    pub fn caption_text(&self, i: usize) -> String {
        self.captions
            .as_slice()
            .get(i)
            .map_or_else(String::new, |c| c.text.to_owned())
    }

    /// Caption `i`'s rect in canvas backing px, `[left, top, right, bottom]` (empty out of range).
    pub fn caption_rect(&self, i: usize) -> Vec<i32> {
        self.captions
            .as_slice()
            .get(i)
            .map_or_else(Vec::new, |c| rect_vec(c.rect))
    }

    /// Caption `i`'s role: lumen's text role index (0–8), or 9 for the Levels pill's label (0 out
    /// of range).
    pub fn caption_role(&self, i: usize) -> u8 {
        self.captions.as_slice().get(i).map_or(0, |c| c.role.code())
    }

    /// Caption `i`'s style (`halation::CaptionStyle` as a number, which the page maps to a CSS
    /// class; 0 out of range).
    pub fn caption_style(&self, i: usize) -> u8 {
        self.captions.as_slice().get(i).map_or(0, |c| c.style as u8)
    }

    /// Caption `i`'s opacity, 0–1 (0 out of range).
    pub fn caption_alpha(&self, i: usize) -> f32 {
        self.captions.as_slice().get(i).map_or(0.0, |c| c.alpha)
    }
}

/// Rust-only views for native tests.
impl Session {
    pub fn rgba(&self) -> &[u8] {
        match self.look {
            Look::Classic => &self.rgba,
            Look::Modern => &self.canvas,
        }
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

    /// The modern look is on but has no canvas yet: pointers hit nothing.
    fn canvasless(&self) -> bool {
        self.look == Look::Modern && self.lightbox.is_none()
    }

    /// A pointer position as the pixel it falls in: a frame pixel in classic, a canvas backing
    /// pixel in modern.
    fn point(&self, x: f64, y: f64) -> (i32, i32) {
        match self.look {
            Look::Classic => frame_point(x, y),
            Look::Modern => device_point(x, y),
        }
    }

    /// The modern look's pointer path (spec §5.3): the lightbox's controls first, then the game.
    fn route(&mut self, event: InputEvent) -> u32 {
        let (Look::Modern, Some(lightbox)) = (self.look, self.lightbox.as_mut()) else {
            return self.send(event);
        };
        match lightbox.route(event) {
            Route::Game(event) => self.send(event),
            Route::Control(Control::Next) => self.send(InputEvent::Key(b'=')),
            Route::Control(control) => self.toggle(control),
            Route::Consumed => 0,
        }
    }

    /// Opens or closes the level sheet; `Redraw`, plus `SheetOpened` when it opened, or 0 when
    /// nothing changed.
    fn toggle(&mut self, control: Control) -> u32 {
        let Some(lightbox) = self.lightbox.as_mut() else {
            return 0;
        };
        if !lightbox.command(control) {
            return 0;
        }
        let opened = if lightbox.sheet_open() {
            Flag::SheetOpened as u32
        } else {
            0
        };
        Flag::Redraw as u32 | opened
    }

    /// Hands one event to the current game (through the lightbox's floorplan in the modern look)
    /// and turns its effects into flags.
    fn send(&mut self, event: InputEvent) -> u32 {
        let plan = if self.look == Look::Modern {
            self.lightbox.as_ref()
        } else {
            None
        };
        let Some(index) = self.current else {
            return 0;
        };
        let Some(state) = self.games[index].as_mut() else {
            return 0;
        };
        let slot = state.slot();
        let effects = match plan {
            Some(lightbox) => state.handle_on(lightbox, event),
            None => state.handle(event),
        };
        self.flags_for(slot, effects)
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
        self.flags_for(slot, effects)
    }

    /// The flags for `effects` of an action that began at strip slot `slot`.
    fn flags_for(&mut self, slot: usize, effects: Effects) -> u32 {
        let Some(state) = self.current.and_then(|i| self.games[i].as_ref()) else {
            return 0;
        };
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

/// `rect_vec`, or empty for a rect with no area (what `Tableau` returns for a missing part).
fn area_vec(rect: Rect) -> Vec<i32> {
    if rect.right > rect.left && rect.bottom > rect.top {
        rect_vec(rect)
    } else {
        Vec::new()
    }
}

/// A stored save text as `SaveData`; `None` when it is not base64 or not a valid save.
fn decode_text(rules: &RuleSet, text: &str) -> Option<SaveData> {
    decode_save(rules, &from_save_text(text)?).ok()
}
