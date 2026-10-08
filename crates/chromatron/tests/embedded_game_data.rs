//! The data build.rs embedded equals a fresh decode of each original.

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

#[test]
fn the_embedded_chromatron_2_data_matches_the_original() {
    let binary = Game::C2
        .read_original()
        .expect("the original Chromatron 2 is present");
    let decoded = decode(Game::C2, &binary).expect("the original decodes");
    assert!(
        chromatron::c2::GAME == decoded,
        "embedded data differs from a fresh decode"
    );
}
