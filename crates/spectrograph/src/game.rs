use std::fmt;
use std::path::{Path, PathBuf};

/// One of the four original games. Each maps to exactly one pinned binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Game {
    C1,
    C2,
    C3,
    C4,
}

impl Game {
    pub const ALL: [Game; 4] = [Game::C1, Game::C2, Game::C3, Game::C4];

    pub fn from_number(n: u8) -> Option<Game> {
        match n {
            1 => Some(Game::C1),
            2 => Some(Game::C2),
            3 => Some(Game::C3),
            4 => Some(Game::C4),
            _ => None,
        }
    }

    pub fn number(self) -> u8 {
        match self {
            Game::C1 => 1,
            Game::C2 => 2,
            Game::C3 => 3,
            Game::C4 => 4,
        }
    }

    pub fn version(self) -> &'static str {
        match self {
            Game::C1 => "1.15",
            Game::C2 => "1.4",
            Game::C3 => "1.1",
            Game::C4 => "1.0",
        }
    }

    /// Path of the executable, relative to the originals directory.
    pub fn executable(self) -> &'static str {
        match self {
            Game::C1 => "Chromatron.app/Contents/MacOS/chromatron1",
            Game::C2 => "Chromatron 2.app/Contents/MacOS/chromatron2",
            Game::C3 => "Chromatron 3.app/Contents/MacOS/chromatron3",
            Game::C4 => "Chromatron 4.app/Contents/MacOS/chromatron4",
        }
    }

    /// SHA-256 of the exact binary this crate was written against.
    pub fn sha256(self) -> &'static str {
        match self {
            Game::C1 => "60dac42ee9dd1e4610514b5b4a0c4bf1005ffecb9151d00b8261cecf313ae72e",
            Game::C2 => "ca85620cecc41fee5b5a88a697b9f7ce879e456d2f64191aca7f9b355f90e77e",
            Game::C3 => "a4e22d83e4d25d7d4379c449a23b7201b74eb0d5bd1a2ad2811c1aa9cfb9f7bb",
            Game::C4 => "ee66f35da2e9d4e60f3af4c2b3423c0906cf178b3b7e4cb9e8d660a89adbc8c3",
        }
    }

    pub fn binary_path(self, originals: &Path) -> PathBuf {
        originals.join(self.executable())
    }
}

impl fmt::Display for Game {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Chromatron {} v{}", self.number(), self.version())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn numbers_round_trip() {
        for game in Game::ALL {
            assert_eq!(Game::from_number(game.number()), Some(game));
        }
    }

    #[test]
    fn rejects_unknown_numbers() {
        assert_eq!(Game::from_number(0), None);
        assert_eq!(Game::from_number(5), None);
    }

    #[test]
    fn display_names_game_and_version() {
        assert_eq!(Game::C1.to_string(), "Chromatron 1 v1.15");
        assert_eq!(Game::C2.to_string(), "Chromatron 2 v1.4");
    }

    #[test]
    fn binary_path_keeps_spaces() {
        let path = Game::C2.binary_path(Path::new("/x"));
        assert_eq!(
            path,
            Path::new("/x/Chromatron 2.app/Contents/MacOS/chromatron2")
        );
    }
}
