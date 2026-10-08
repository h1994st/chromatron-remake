//! Decodes game data out of the original Chromatron PowerPC Mach-O binaries.

mod error;
mod game;
mod image;
mod strings;

pub use error::DecodeError;
pub use game::Game;
pub use image::{Image, Section};
pub use strings::{CString, cstrings, decode_mac_roman};
