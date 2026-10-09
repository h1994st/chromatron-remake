//! Shared helpers for tests that need the original Chromatron 4 data.

#![allow(dead_code)]

use std::sync::LazyLock;

use lumen::{BeamField, Board, trace};
use spectrograph::{Game, GameData};

/// Chromatron 4's data, decoded once per test binary from `chromatron_mac/`.
pub static C4: LazyLock<GameData> =
    LazyLock::new(|| spectrograph::decode(Game::C4, &Game::C4.read_original().unwrap()).unwrap());

pub fn traced(board: &Board) -> BeamField {
    let mut field = BeamField::default();
    trace(board, &mut field);
    field
}
