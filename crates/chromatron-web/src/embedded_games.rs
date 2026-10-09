//! All four games' data, decoded from the originals at build time (build.rs).

use spectrograph::{Game, GameData};

mod c1 {
    include!(concat!(env!("OUT_DIR"), "/chromatron1_data.rs"));
}
mod c2 {
    include!(concat!(env!("OUT_DIR"), "/chromatron2_data.rs"));
}
mod c3 {
    include!(concat!(env!("OUT_DIR"), "/chromatron3_data.rs"));
}
mod c4 {
    include!(concat!(env!("OUT_DIR"), "/chromatron4_data.rs"));
}

/// `game`'s embedded data.
pub fn game_data(game: Game) -> &'static GameData {
    match game {
        Game::C1 => &c1::GAME,
        Game::C2 => &c2::GAME,
        Game::C3 => &c3::GAME,
        Game::C4 => &c4::GAME,
    }
}
