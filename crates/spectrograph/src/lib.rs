//! Decodes game data out of the original Chromatron PowerPC Mach-O binaries.
//!
//! The data types ([`GameData`] and friends) are always available and can be built in a `static`.
//! Parsing the originals needs the `decode` feature. The default feature is `cli` (which implies
//! `decode`); library consumers (lumen, the app) depend with `default-features = false`.

mod game;
mod game_data;

#[cfg(feature = "decode")]
mod c1;
#[cfg(feature = "decode")]
mod error;
#[cfg(feature = "decode")]
mod image;
#[cfg(feature = "cli")]
mod rgb_png;
#[cfg(feature = "decode")]
mod strings;

pub use game::{Game, originals_dir};
pub use game_data::{
    GameData, Level, Record, SPRITE_LEN, SPRITE_SIZE, Sprite, UiText, colour_symbol, kind_symbol,
};

#[cfg(feature = "decode")]
pub use c1::decode_rle;
#[cfg(feature = "decode")]
pub use error::DecodeError;
#[cfg(feature = "decode")]
pub use image::{Image, Section};
#[cfg(feature = "cli")]
pub use rgb_png::write_rgb_png;
#[cfg(feature = "decode")]
pub use strings::{CString, cstring_at, cstrings, decode_mac_roman};

/// Verifies `binary` against `game`'s pinned hash and decodes all of its game data.
#[cfg(feature = "decode")]
pub fn decode(game: Game, binary: &[u8]) -> Result<GameData, DecodeError> {
    let image = Image::load(game, binary)?;
    match game {
        Game::C1 => c1::decode(&image),
        _ => Err(DecodeError::Unsupported { game }),
    }
}
