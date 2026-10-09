//! Chromatron 3's save file on real levels (re/notes/chromatron3/platform.md P7).

mod c3_game;

use c3_game::C3;
use lumen::{
    Board, GameState, InputEvent, PROGRESS_SLOTS, RuleSet, SaveData, decode_save, encode_save,
    move_pieces_to_toolbox,
};

#[test]
fn a_first_win_writes_the_bitmap_the_marker_and_the_board() {
    let mut state = GameState::new(&C3, SaveData::fresh(&RuleSet::C3));
    state.handle(InputEvent::Paste("3-01-oCcmcO".into()));
    assert!(state.is_won() && state.is_completed(0));
    let bytes = state.save_bytes();
    assert_eq!(bytes[..3], [0x00, 0x00, 0x10], "slot 1 completed");
    assert_eq!(bytes[32..34], [0xff, 11], "the marker, then level id 11");
    assert_eq!(bytes.len(), 33 + 856);
}

#[test]
fn a_save_without_the_marker_counts_only_its_solved_boards() {
    // Id 55 (slot 17) saved solved, id 11 (slot 1) saved unsolved, and a bitmap that claims
    // slot 4: only slot 17 ends up completed.
    let mut boards = vec![None; 128];
    boards[55] = Some(Board::from_level(C3.level(55).unwrap()));
    let mut unsolved = Board::from_level(C3.level(11).unwrap());
    move_pieces_to_toolbox(&mut unsolved);
    boards[11] = Some(unsolved);
    let mut claimed = [false; PROGRESS_SLOTS];
    claimed[3] = true;
    let mut bytes = encode_save(&RuleSet::C3, &claimed, &boards);
    assert_eq!(bytes.remove(32), 0xff);
    let save = decode_save(&RuleSet::C3, &bytes).unwrap();
    let state = GameState::new(&C3, save);
    let completed: Vec<usize> = (0..50).filter(|&s| state.is_completed(s)).collect();
    assert_eq!(completed, [16]);
    let open: Vec<usize> = (0..50).filter(|&s| state.is_unlocked(s)).collect();
    assert_eq!(
        open,
        (0..26).collect::<Vec<_>>(),
        "25 uncompleted plus slot 17"
    );
}
