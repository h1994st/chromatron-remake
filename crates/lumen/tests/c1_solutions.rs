//! Our own solutions in the original clipboard format, `tests/solutions/game1/level-NN.txt`
//! (NN = strip number). Each must win its level when pasted.

mod common;

use std::{fs, path::PathBuf};

use common::C1;
use lumen::{GameState, InputEvent, SaveData};

#[test]
fn every_solution_fixture_wins() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/solutions/game1");
    let mut checked = 0;
    for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let number = name
            .strip_prefix("level-")
            .and_then(|n| n.strip_suffix(".txt"));
        let slot = number
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| (1..=50).contains(n))
            .map(|n| n - 1);
        let slot =
            slot.unwrap_or_else(|| panic!("{name}: expected level-NN.txt with NN in 01..50"));
        let text = fs::read_to_string(&path).unwrap();
        let mut state = GameState::new(&C1, SaveData::default());
        state.goto_slot(slot);
        state.handle(InputEvent::Paste(text.trim_end_matches('\n').to_owned()));
        assert!(state.is_won(), "{name} does not win level {}", slot + 1);
        checked += 1;
    }
    assert!(checked > 0, "no fixtures in {}", dir.display());
}
