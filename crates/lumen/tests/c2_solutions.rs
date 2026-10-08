//! Chromatron 2 solutions in the original clipboard format, `tests/solutions/game2/level-NN.txt`
//! (NN = strip number). Each must win its level when pasted.

mod c2_game;

use std::{fs, path::PathBuf};

use c2_game::C2;
use lumen::{GameState, InputEvent, SaveData};

#[test]
fn every_solution_fixture_wins() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/solutions/game2");
    let mut checked = 0;
    for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let slot = name
            .strip_prefix("level-")
            .and_then(|n| n.strip_suffix(".txt"))
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| (1..=50).contains(n))
            .map(|n| n - 1)
            .unwrap_or_else(|| panic!("{name}: expected level-NN.txt with NN in 01..50"));
        let text = fs::read_to_string(&path).unwrap();
        let mut state = GameState::new(&C2, SaveData::default());
        state.goto_slot(slot);
        state.handle(InputEvent::Paste(text.trim_end_matches('\n').to_owned()));
        assert!(state.is_won(), "{name} does not win level {}", slot + 1);
        checked += 1;
    }
    assert!(checked > 0, "no fixtures in {}", dir.display());
}
