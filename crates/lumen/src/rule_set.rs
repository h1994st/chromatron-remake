//! What differs between the games' engines: one `RuleSet` per game (spec §4). Everything not
//! listed here is the same code in every game supported so far.

use spectrograph::Game;

use crate::piece::Piece;

/// What a splitter does with a beam that hits it head-on (`r` = 2 or 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Splitter {
    /// Chromatron 1: the beam passes straight through and keeps its entanglement tag.
    PassHeadOn,
    /// Chromatron 2 (logic.md L3): it also reflects straight back, and every splitter output,
    /// at any angle, is disentangled.
    ReflectHeadOn,
}

/// How many uncompleted strip slots are open (`update_unlocked`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unlock {
    /// Chromatron 1: grows with the highest completed slot.
    HighestCompleted,
    /// Chromatron 2 (logic.md L6): grows with the number of completed slots.
    CompletedCount,
}

/// Which text boxes `paint_window` draws besides the win text, the hint and the site URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextBoxSet {
    /// Chromatron 1: "freeware", and "more levels @" on strip numbers 41, 43, 45, 47, 49, 50.
    Freeware,
    /// Chromatron 2 (platform.md P3): the level name, and "Thank you for purchasing this game."
    /// when `current_slot % 6 == 4`.
    Purchased,
}

/// One game's rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleSet {
    pub game: Game,
    /// The highest cell `type` byte the game's code handles (`beam_enter_cell` asserts above it).
    pub max_kind: u8,
    pub splitter: Splitter,
    pub unlock: Unlock,
    /// The key that forces a win (`handle_key`).
    pub cheat_key: u8,
    /// The save file's name in the original (platform.md P7).
    pub save_file_name: &'static str,
    /// `save_bit_perm`, a permutation of 0..31.
    pub save_bit_perm: [u8; 32],
    /// Level ids that can have a saved board (`saved_states[]`).
    pub saved_ids: usize,
    /// The game digit that starts every clipboard solution (platform.md P8).
    pub clipboard_digit: u8,
    pub text_boxes: TextBoxSet,
}

impl RuleSet {
    /// Chromatron 1 v1.15 (re/notes/chromatron1).
    pub const C1: RuleSet = RuleSet {
        game: Game::C1,
        max_kind: 12,
        splitter: Splitter::PassHeadOn,
        unlock: Unlock::HighestCompleted,
        cheat_key: b'L',
        save_file_name: "com.silverspaceship.chromatron.dat",
        save_bit_perm: [
            0x00, 0x09, 0x0d, 0x13, 0x15, 0x0a, 0x06, 0x18, 0x1e, 0x0e, 0x03, 0x1c, 0x10, 0x0b,
            0x19, 0x0c, 0x17, 0x04, 0x11, 0x1d, 0x12, 0x1a, 0x01, 0x0f, 0x14, 0x1f, 0x07, 0x08,
            0x02, 0x16, 0x05, 0x1b,
        ],
        saved_ids: 128,
        clipboard_digit: b'1',
        text_boxes: TextBoxSet::Freeware,
    };

    /// Chromatron 2 v1.4 (re/notes/chromatron2).
    pub const C2: RuleSet = RuleSet {
        game: Game::C2,
        max_kind: 15,
        splitter: Splitter::ReflectHeadOn,
        unlock: Unlock::CompletedCount,
        cheat_key: b'z',
        save_file_name: "com.silverspaceship.chromatron2.dat",
        save_bit_perm: [
            0x14, 0x1f, 0x07, 0x08, 0x1e, 0x0e, 0x03, 0x1c, 0x02, 0x16, 0x05, 0x1b, 0x10, 0x0b,
            0x19, 0x0c, 0x00, 0x09, 0x0d, 0x13, 0x17, 0x04, 0x11, 0x1d, 0x15, 0x0a, 0x06, 0x18,
            0x12, 0x1a, 0x01, 0x0f,
        ],
        saved_ids: 52,
        clipboard_digit: b'2',
        text_boxes: TextBoxSet::Purchased,
    };

    /// The rules of `game`.
    ///
    /// # Panics
    ///
    /// For a game lumen has no rules for yet (Chromatron 3 and 4). `spectrograph` can't decode
    /// those either, so no level or save of theirs reaches lumen.
    pub fn of(game: Game) -> &'static RuleSet {
        match game {
            Game::C1 => &RuleSet::C1,
            Game::C2 => &RuleSet::C2,
            other => panic!("lumen has no rules for {other} yet"),
        }
    }

    /// The piece a cell `type` byte stands for in this game, or `None` if the game has no such
    /// type.
    pub fn piece(&self, byte: u8) -> Option<Piece> {
        Piece::from_byte(byte).filter(|_| byte <= self.max_kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_supported_game_has_its_own_rules() {
        assert_eq!(RuleSet::of(Game::C1), &RuleSet::C1);
        assert_eq!(RuleSet::of(Game::C2), &RuleSet::C2);
        assert_eq!(RuleSet::of(Game::C2).game, Game::C2);
    }

    #[test]
    #[should_panic(expected = "no rules for Chromatron 3")]
    fn later_games_have_no_rules_yet() {
        RuleSet::of(Game::C3);
    }

    #[test]
    fn chromatron_1_has_piece_types_0_to_12() {
        assert_eq!(RuleSet::C1.piece(0), Some(Piece::Empty));
        assert_eq!(RuleSet::C1.piece(12), Some(Piece::Teleporter));
        assert_eq!(RuleSet::C1.piece(13), None);
        assert_eq!(RuleSet::C2.piece(12), Some(Piece::Teleporter));
    }

    #[test]
    fn piece_types_13_to_15_exist_from_chromatron_2_on() {
        assert_eq!(RuleSet::C2.piece(13), Some(Piece::MultiFilter));
        assert_eq!(RuleSet::C2.piece(14), Some(Piece::Twister));
        assert_eq!(RuleSet::C2.piece(15), Some(Piece::Complementor));
        assert_eq!(RuleSet::C2.piece(16), None);
        for kind in 13..=15 {
            assert_eq!(RuleSet::C1.piece(kind), None, "type {kind}");
        }
    }

    #[test]
    fn save_permutations_are_permutations_of_0_to_31() {
        for rules in [RuleSet::C1, RuleSet::C2] {
            let mut sorted = rules.save_bit_perm;
            sorted.sort();
            assert_eq!(sorted, std::array::from_fn(|i| i as u8), "{:?}", rules.game);
        }
    }

    #[test]
    fn chromatron_2s_permutation_regroups_chromatron_1s() {
        // platform.md P7: C1's eight 4-byte groups in the order 6, 2, 7, 3, 0, 4, 1, 5.
        let c1 = RuleSet::C1.save_bit_perm;
        let regrouped: Vec<u8> = [6, 2, 7, 3, 0, 4, 1, 5]
            .iter()
            .flat_map(|&g| c1[4 * g..4 * g + 4].to_vec())
            .collect();
        assert_eq!(regrouped, RuleSet::C2.save_bit_perm);
    }
}
