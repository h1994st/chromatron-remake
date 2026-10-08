//! Decoded game data. Every type here can be built in a `static`, so a build script can embed the
//! data as Rust source and the app does no decoding at startup.

use std::borrow::Cow;

use crate::Game;

/// Side length of every sprite, in pixels.
pub const SPRITE_SIZE: usize = 24;
/// Pixels in one sprite.
pub const SPRITE_LEN: usize = SPRITE_SIZE * SPRITE_SIZE;
/// One 24 × 24 palette-indexed sprite, row-major, top row first.
pub type Sprite = [u8; SPRITE_LEN];

/// One 5-byte level record, byte for byte as stored (data.md D1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub kind: u8,
    pub dir: u8,
    pub colour: u8,
    pub x: u8,
    pub y: u8,
}

/// One level: its id (index into the level table), its records and its hint ("" for none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Level {
    pub id: u8,
    pub records: Cow<'static, [Record]>,
    pub hint: Cow<'static, str>,
}

/// The fixed UI strings the game draws with `DrawThemeTextBox` (platform.md P3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiText {
    pub window_title: Cow<'static, str>,
    pub you_win: Cow<'static, str>,
    pub won: Cow<'static, str>,
    pub click_next: Cow<'static, str>,
    pub freeware: Cow<'static, str>,
    pub more_levels: Cow<'static, str>,
    pub site_url: Cow<'static, str>,
}

/// Everything a game needs from its original binary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameData {
    pub game: Game,
    /// Levels in id order; `levels[i].id == i + 1`.
    pub levels: Cow<'static, [Level]>,
    /// Level id shown in each strip slot (slot 0 first).
    pub level_order: Cow<'static, [u8]>,
    pub sprites: Cow<'static, [Sprite]>,
    /// 256 colours as 0x00RRGGBB.
    pub palette: [u32; 256],
    /// Beam colour by colour mask, 0x00RRGGBB.
    pub beam_colors: [u32; 8],
    pub ui: UiText,
}

impl GameData {
    pub fn level(&self, id: u8) -> Option<&Level> {
        self.levels.get(usize::from(id).checked_sub(1)?)
    }

    /// The level shown in strip slot `slot` (0-based).
    pub fn slot_level(&self, slot: usize) -> Option<&Level> {
        self.level(*self.level_order.get(slot)?)
    }
}

/// One ASCII character per cell `kind` (data.md D2), shared by every text dump.
pub fn kind_symbol(kind: u8) -> char {
    match kind {
        0 => '.',
        1 => '#',
        2 => '*',
        3 => 'M',
        4 => 'B',
        5 => 'F',
        6 => 'P',
        7 => 'D',
        8 => 'S',
        9 => 'Q',
        10 => 'o',
        11 => '=',
        12 => 'T',
        _ => '?',
    }
}

/// One ASCII character per colour mask: k(black) b g c r m y w.
pub fn colour_symbol(mask: u8) -> char {
    match mask {
        0..=7 => b"kbgcrmyw"[usize::from(mask)] as char,
        _ => '?',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: Cow<'static, str> = Cow::Borrowed("");

    /// Proves the types can be embedded as a `static` (what Plan 3's build.rs generates).
    static TINY: GameData = GameData {
        game: Game::C1,
        levels: Cow::Borrowed(&[Level {
            id: 1,
            records: Cow::Borrowed(&[Record {
                kind: 2,
                dir: 2,
                colour: 4,
                x: 2,
                y: 7,
            }]),
            hint: Cow::Borrowed("hint"),
        }]),
        level_order: Cow::Borrowed(&[1]),
        sprites: Cow::Borrowed(&[[0; SPRITE_LEN]]),
        palette: [0; 256],
        beam_colors: [0; 8],
        ui: UiText {
            window_title: EMPTY,
            you_win: EMPTY,
            won: EMPTY,
            click_next: EMPTY,
            freeware: EMPTY,
            more_levels: EMPTY,
            site_url: EMPTY,
        },
    };

    #[test]
    fn static_data_is_addressable_by_id_and_slot() {
        assert_eq!(TINY.level(1).map(|l| l.hint.as_ref()), Some("hint"));
        assert_eq!(TINY.level(0), None);
        assert_eq!(TINY.level(2), None);
        assert_eq!(TINY.slot_level(0).map(|l| l.id), Some(1));
        assert_eq!(TINY.slot_level(1), None);
    }

    #[test]
    fn symbols_cover_every_kind_and_colour() {
        let kinds: String = (0..=13).map(kind_symbol).collect();
        assert_eq!(kinds, ".#*MBFPDSQo=T?");
        let colours: String = (0..=8).map(colour_symbol).collect();
        assert_eq!(colours, "kbgcrmyw?");
    }
}
