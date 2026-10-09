//! Shared helpers for tests that need the original Chromatron 3 data.

#![allow(dead_code)]

use std::sync::LazyLock;

use lumen::{BeamField, Board, trace};
use spectrograph::{Game, GameData};

/// Chromatron 3's data, decoded once per test binary from `chromatron_mac/`.
pub static C3: LazyLock<GameData> =
    LazyLock::new(|| spectrograph::decode(Game::C3, &Game::C3.read_original().unwrap()).unwrap());

pub fn traced(board: &Board) -> BeamField {
    let mut field = BeamField::default();
    trace(board, &mut field);
    field
}
