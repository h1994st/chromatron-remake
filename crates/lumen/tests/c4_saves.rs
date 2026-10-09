//! Chromatron 4's save file on real levels (re/notes/chromatron4/platform.md P7): Chromatron 2's
//! layout, with no marker, and saved boards for ids 0–127.

mod c4_game;

use c4_game::C4;
use lumen::{Board, GameState, InputEvent, RuleSet, SaveData, decode_save, encode_solution};

#[test]
fn a_first_win_writes_the_bitmap_and_the_board_without_a_marker() {
    let mut state = GameState::new(&C4, SaveData::fresh(&RuleSet::C4));
    let text = encode_solution(&Board::from_level(C4.level(49).unwrap()), 0).unwrap();
    state.handle(InputEvent::Paste(text));
    assert!(state.is_won() && state.is_completed(0));
    let bytes = state.save_bytes();
    assert_eq!(bytes[..3], [0x00, 0x00, 0x10], "slot 1 completed");
    assert_eq!(bytes[32], 49, "level id 49 follows the bitmap");
    assert_eq!(bytes.len(), 32 + 856);
    let save = decode_save(&RuleSet::C4, &bytes).unwrap();
    assert!(save.completed[0] && !save.replay_wins);
    let restored = GameState::new(&C4, save);
    assert!(restored.is_won(), "slot 1 comes back solved");
}
