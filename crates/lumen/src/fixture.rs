//! A tiny hand-made game for unit tests, so they need no original data.

use std::borrow::Cow;

use spectrograph::{Game, GameData, Level, Record, SPRITE_LEN, UiText};

use crate::layout::STRIP_SLOTS;

fn record(kind: u8, dir: u8, colour: u8, x: u8, y: u8) -> Record {
    Record {
        kind,
        dir,
        colour,
        x,
        y,
    }
}

/// Two levels shown alternately in all 50 slots:
/// - id 1: a red laser at (2,7) facing E and a red pinwheel at (8,2); solved by the mirror at
///   (8,7) dir 1 (data.md D1's worked example).
/// - id 2: a blue laser at (0,5) facing E and a blue pinwheel at (5,9); solved by the mirror at
///   (5,5) dir 7. Its hint is "two".
///
/// The levels are named "first" and "second".
///
/// Sprite `i` is filled with pixel value `i`, and palette entry `i` is `0x10000 + i`, so a test
/// can tell which sprite was drawn where.
pub(crate) fn tiny_game() -> GameData {
    tiny_game_of(Game::C1)
}

/// `tiny_game` as a level set of `game`, so it plays by that game's rules. Every UI string is
/// set, whichever game draws it.
pub(crate) fn tiny_game_of(game: Game) -> GameData {
    let text = |s: &'static str| Cow::Borrowed(s);
    let mut sprites = vec![[0u8; SPRITE_LEN]; 128];
    for (i, sprite) in sprites.iter_mut().enumerate() {
        sprite.fill(i as u8);
    }
    let mut palette = [0u32; 256];
    for (i, colour) in palette.iter_mut().enumerate() {
        *colour = 0x10000 + i as u32;
    }
    GameData {
        game,
        levels: Cow::Owned(vec![
            Level {
                game,
                id: 1,
                records: Cow::Owned(vec![
                    record(10, 6, 4, 8, 2),
                    record(2, 2, 4, 2, 7),
                    record(3, 1, 4, 8, 7),
                ]),
                name: text("first"),
                hint: text("one"),
            },
            Level {
                game,
                id: 2,
                records: Cow::Owned(vec![
                    record(2, 2, 1, 0, 5),
                    record(10, 0, 1, 5, 9),
                    record(3, 7, 4, 5, 5),
                ]),
                name: text("second"),
                hint: text("two"),
            },
        ]),
        level_order: Cow::Owned(
            (0..STRIP_SLOTS)
                .map(|s| if s % 2 == 0 { 1 } else { 2 })
                .collect(),
        ),
        sprites: Cow::Owned(sprites),
        palette,
        beam_colors: [0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7],
        ui: UiText {
            window_title: text("title"),
            you_win: text("You win!"),
            won: text("(won)"),
            click_next: text("next please"),
            freeware: text("freeware"),
            more_levels: text("more levels @"),
            thank_you: text("thanks"),
            site_url: text("example.com"),
        },
    }
}
