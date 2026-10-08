use std::fmt;

use crate::Game;

#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    HashMismatch {
        game: Game,
        found: String,
    },
    NotPowerPc {
        game: Game,
    },
    Parse {
        game: Game,
        message: String,
    },
    MissingSection {
        game: Game,
        segment: &'static str,
        section: &'static str,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::HashMismatch { game, found } => write!(
                f,
                "{game}: binary does not match (expected SHA-256 {}, found {found}); \
                 use the original executable chromatron_mac/{}",
                game.sha256(),
                game.executable()
            ),
            DecodeError::NotPowerPc { game } => write!(f, "{game}: not a PowerPC Mach-O binary"),
            DecodeError::Parse { game, message } => {
                write!(f, "{game}: cannot parse Mach-O: {message}")
            }
            DecodeError::MissingSection {
                game,
                segment,
                section,
            } => {
                write!(f, "{game}: section {segment},{section} not found")
            }
        }
    }
}

impl std::error::Error for DecodeError {}
