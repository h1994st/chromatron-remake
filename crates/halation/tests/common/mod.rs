//! Shared helpers for halation's tests that need the original games.

#![allow(dead_code)]

use std::sync::LazyLock;

use halation::{Easel, Lightbox, Output, Scene, Theme};
use lumen::{GameState, InputEvent, SaveData, stored_solution};
use spectrograph::{Game, GameData};

static GAMES: LazyLock<[GameData; 4]> = LazyLock::new(|| {
    Game::ALL.map(|game| spectrograph::decode(game, &game.read_original().unwrap()).unwrap())
});

pub fn data(game: Game) -> &'static GameData {
    &GAMES[usize::from(game.number() - 1)]
}

/// A paint at this `now` is settled (ruling M7): no cue starts, every running one ends.
pub const SETTLED: f64 = f64::INFINITY;

/// A test's clock: `tick` steps one frame, `later` jumps 10 s (past every cue).
pub struct Clock {
    pub now: f64,
}

impl Clock {
    pub fn tick(&mut self) -> f64 {
        self.now += halation::FRAME_MS;
        self.now
    }

    pub fn later(&mut self) -> f64 {
        self.now += 10_000.0;
        self.now
    }
}

/// `game` at strip slot `slot`, solved with its stored solution when `solved`.
pub fn state(game: Game, slot: usize, solved: bool) -> GameState<'static> {
    let data = data(game);
    let mut state = GameState::new(data, SaveData::default());
    state.goto_slot(slot);
    if solved {
        let text = stored_solution(data, slot).expect("a stored solution");
        state.handle(InputEvent::Paste(text));
        assert!(
            state.is_won(),
            "{game} slot {slot}: the stored solution wins"
        );
    }
    state
}

/// A fresh lightbox's first (cold) paint of `scene`.
pub fn cold(easel: Easel, scene: Scene) -> Vec<u32> {
    cold_with(easel, scene, false)
}

/// A fresh lightbox's first paint of `scene`, with the sheet open when `sheet` (and possible).
pub fn cold_with(easel: Easel, scene: Scene, sheet: bool) -> Vec<u32> {
    let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
    if sheet {
        lightbox.command(halation::Control::Levels);
    }
    let mut out = vec![0u32; (easel.width * easel.height) as usize];
    lightbox.paint_scene(scene, 0.0, Output::Xrgb(&mut out));
    out
}

/// `xrgb` (`0x00RRGGBB`) as PNG bytes.
pub fn png_bytes(width: u32, height: u32, xrgb: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let rgb: Vec<u8> = xrgb
        .iter()
        .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
        .collect();
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&rgb).unwrap();
    writer.finish().unwrap();
    bytes
}
