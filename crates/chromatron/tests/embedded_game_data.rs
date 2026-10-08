//! The data build.rs embedded equals a fresh decode of the original.

use spectrograph::{Game, decode};

#[test]
fn the_embedded_chromatron_1_data_matches_the_original() {
    let binary = Game::C1
        .read_original()
        .expect("the original Chromatron 1 is present");
    let decoded = decode(Game::C1, &binary).expect("the original decodes");
    assert!(
        chromatron::c1::GAME == decoded,
        "embedded data differs from a fresh decode"
    );
}
