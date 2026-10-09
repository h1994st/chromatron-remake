//! Snapshots of every playable Chromatron 3 level's beams, at the start and solved. Derived from
//! the originals, so local only (git-ignored), like c1_snapshots.rs.

mod c3_game;

use c3_game::{C3, traced};
use lumen::{Board, beam_diagram, move_pieces_to_toolbox};

#[test]
fn initial_beam_fields() {
    for (slot, &id) in C3.level_order.iter().enumerate() {
        let mut board = Board::from_level(C3.level(id).unwrap());
        move_pieces_to_toolbox(&mut board);
        insta::assert_snapshot!(
            format!("initial_slot{:02}_id{id:02}", slot + 1),
            beam_diagram(&board, &traced(&board))
        );
    }
}

#[test]
fn solved_beam_fields() {
    for (slot, &id) in C3.level_order.iter().enumerate() {
        let board = Board::from_level(C3.level(id).unwrap());
        insta::assert_snapshot!(
            format!("solved_slot{:02}_id{id:02}", slot + 1),
            beam_diagram(&board, &traced(&board))
        );
    }
}
