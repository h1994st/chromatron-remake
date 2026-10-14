//! Spec §6: `card_min` is sized for a 245-character hint; no game has a longer one.

use halation::tableau::LONGEST_HINT;
use spectrograph::Game;

#[test]
fn no_hint_is_longer_than_the_card_is_sized_for() {
    for game in Game::ALL {
        let data = spectrograph::decode(game, &game.read_original().unwrap()).unwrap();
        let longest = data
            .levels
            .iter()
            .map(|l| l.hint.chars().count())
            .max()
            .unwrap();
        assert!(longest as f32 <= LONGEST_HINT, "{game}: {longest}");
    }
}
