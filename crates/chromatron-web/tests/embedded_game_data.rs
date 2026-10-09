//! The data build.rs embedded equals a fresh decode of each original.

use chromatron_web::embedded_games::game_data;
use spectrograph::{Game, decode};

#[test]
fn every_embedded_game_matches_its_original() {
    for game in Game::ALL {
        let binary = game.read_original().expect("the original is present");
        let decoded = decode(game, &binary).expect("the original decodes");
        assert!(
            *game_data(game) == decoded,
            "{game}: embedded data differs from a fresh decode"
        );
    }
}
