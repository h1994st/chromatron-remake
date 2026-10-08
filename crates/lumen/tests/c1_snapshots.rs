//! Snapshots of every playable level's beams, at the start and solved. They are derived from the
//! originals, so they live only on this machine (git-ignored) as a regression baseline: the first
//! run records them, later runs fail on any change (`cargo insta review` to inspect).

mod common;

use common::{C1, solved_board, traced};
use lumen::{Board, beam_diagram, move_pieces_to_toolbox};

#[test]
fn initial_beam_fields() {
    for (slot, &id) in C1.level_order.iter().enumerate() {
        let mut board = Board::from_level(C1.level(id).unwrap());
        move_pieces_to_toolbox(&mut board);
        insta::assert_snapshot!(
            format!("initial_slot{:02}_id{id:02}", slot + 1),
            beam_diagram(&board, &traced(&board))
        );
    }
}

#[test]
fn solved_beam_fields() {
    for (slot, &id) in C1.level_order.iter().enumerate() {
        let board = solved_board(C1.level(id).unwrap());
        insta::assert_snapshot!(
            format!("solved_slot{:02}_id{id:02}", slot + 1),
            beam_diagram(&board, &traced(&board))
        );
    }
}
