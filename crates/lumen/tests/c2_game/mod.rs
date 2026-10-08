//! Shared helpers for tests that need the original Chromatron 2 data.

#![allow(dead_code)]

use std::sync::LazyLock;

use lumen::{BeamField, Board, trace};
use spectrograph::{Game, GameData};

/// Chromatron 2's data, decoded once per test binary from `chromatron_mac/`.
pub static C2: LazyLock<GameData> =
    LazyLock::new(|| spectrograph::decode(Game::C2, &Game::C2.read_original().unwrap()).unwrap());

pub fn traced(board: &Board) -> BeamField {
    let mut field = BeamField::default();
    trace(board, &mut field);
    field
}
