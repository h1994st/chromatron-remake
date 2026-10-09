//! What differs between the games' engines: one `RuleSet` per game (spec §4). Everything not
//! listed here is the same code in every game.

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

/// The fixed-point passes `retrace_beams` runs after tracing every laser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Passes {
    /// Chromatron 1 and 2 (C2 logic.md L3): snapshot the inputs and let the complementors emit,
    /// while some complementor gained an input.
    Complementors,
    /// Chromatron 3 and 4 (logic.md L3–L4 of both): snapshot, gate pass, complementors, the
    /// new-input test, gate pass again; while either gate pass emitted or a complementor gained an
    /// input.
    GatesAroundComplementors,
}

/// How many uncompleted strip slots are open (`update_unlocked`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unlock {
    /// Chromatron 1: grows with the highest completed slot.
    HighestCompleted,
    /// Chromatron 2 (logic.md L6): grows with the number of completed slots.
    CompletedCount,
    /// Chromatron 3 and 4 (logic.md L6): always this many, the lowest-numbered uncompleted slots.
    Fixed(usize),
}

/// Which text boxes `paint_window` draws besides the win text, the hint and the site URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextBoxSet {
    /// Chromatron 1: "freeware", and "more levels @" on strip numbers 41, 43, 45, 47, 49, 50.
    Freeware,
    /// Chromatron 2, 3 and 4 (platform.md P3): the level name, and "Thank you for purchasing this
    /// game." when `current_slot % 6 == 4`.
    Purchased,
}

/// `save_bit_perm` @ 0x80ac of Chromatron 2, 3 and 4 (platform.md P7 of each).
const C2_SAVE_BIT_PERM: [u8; 32] = [
    0x14, 0x1f, 0x07, 0x08, 0x1e, 0x0e, 0x03, 0x1c, 0x02, 0x16, 0x05, 0x1b, 0x10, 0x0b, 0x19, 0x0c,
    0x00, 0x09, 0x0d, 0x13, 0x17, 0x04, 0x11, 0x1d, 0x15, 0x0a, 0x06, 0x18, 0x12, 0x1a, 0x01, 0x0f,
];

/// One game's rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleSet {
    pub game: Game,
    /// The highest cell `type` byte the game's code handles (`beam_enter_cell` asserts above it).
    pub max_kind: u8,
    pub splitter: Splitter,
    /// Beams one generation can hold (`push_beam`); further pushes are dropped.
    pub max_beams: usize,
    /// The cell types a teleporter's jump lands on.
    pub teleport_targets: &'static [Piece],
    pub passes: Passes,
    pub unlock: Unlock,
    /// The key that forces a win (`handle_key`).
    pub cheat_key: u8,
    /// The save file's name in the original (platform.md P7).
    pub save_file_name: &'static str,
    /// `save_bit_perm`, a permutation of 0..31.
    pub save_bit_perm: [u8; 32],
    /// The byte written right after the bitmap that marks the current file format (Chromatron 3:
    /// 0xFF), or `None` for a game without one. A file without it gets its completed slots
    /// recomputed from its saved boards (platform.md P7).
    pub save_marker: Option<u8>,
    /// Level ids that can have a saved board (`saved_states[]`).
    pub saved_ids: usize,
    /// The game digit that starts every clipboard solution (platform.md P8).
    pub clipboard_digit: u8,
    pub text_boxes: TextBoxSet,
    /// `cell_image_index` for a mobile teleporter (type 18): Chromatron 3 draws the teleporter's
    /// sprite 14, Chromatron 4 its own 145. `None` in games without the piece.
    pub mobile_teleporter_sprite: Option<usize>,
}

impl RuleSet {
    /// Chromatron 1 v1.15 (re/notes/chromatron1).
    pub const C1: RuleSet = RuleSet {
        game: Game::C1,
        max_kind: 12,
        splitter: Splitter::PassHeadOn,
        max_beams: 32,
        teleport_targets: &[Piece::Teleporter],
        passes: Passes::Complementors,
        unlock: Unlock::HighestCompleted,
        cheat_key: b'L',
        save_file_name: "com.silverspaceship.chromatron.dat",
        save_bit_perm: [
            0x00, 0x09, 0x0d, 0x13, 0x15, 0x0a, 0x06, 0x18, 0x1e, 0x0e, 0x03, 0x1c, 0x10, 0x0b,
            0x19, 0x0c, 0x17, 0x04, 0x11, 0x1d, 0x12, 0x1a, 0x01, 0x0f, 0x14, 0x1f, 0x07, 0x08,
            0x02, 0x16, 0x05, 0x1b,
        ],
        save_marker: None,
        saved_ids: 128,
        clipboard_digit: b'1',
        text_boxes: TextBoxSet::Freeware,
        mobile_teleporter_sprite: None,
    };

    /// Chromatron 2 v1.4 (re/notes/chromatron2).
    pub const C2: RuleSet = RuleSet {
        game: Game::C2,
        max_kind: 15,
        splitter: Splitter::ReflectHeadOn,
        max_beams: 32,
        teleport_targets: &[Piece::Teleporter],
        passes: Passes::Complementors,
        unlock: Unlock::CompletedCount,
        cheat_key: b'z',
        save_file_name: "com.silverspaceship.chromatron2.dat",
        save_bit_perm: C2_SAVE_BIT_PERM,
        save_marker: None,
        saved_ids: 52,
        clipboard_digit: b'2',
        text_boxes: TextBoxSet::Purchased,
        mobile_teleporter_sprite: None,
    };

    /// Chromatron 3 v1.1 (re/notes/chromatron3): Chromatron 2's rules, plus types 16–20, 25 open
    /// slots, the `^` cheat and a save-file format marker.
    pub const C3: RuleSet = RuleSet {
        game: Game::C3,
        max_kind: 20,
        splitter: Splitter::ReflectHeadOn,
        max_beams: 32,
        teleport_targets: &[Piece::Teleporter, Piece::MobileTeleporter],
        passes: Passes::GatesAroundComplementors,
        unlock: Unlock::Fixed(25),
        cheat_key: b'^',
        save_file_name: "com.silverspaceship.chromatron3.dat",
        save_bit_perm: C2_SAVE_BIT_PERM,
        save_marker: Some(0xff),
        saved_ids: 128,
        clipboard_digit: b'3',
        text_boxes: TextBoxSet::Purchased,
        mobile_teleporter_sprite: Some(14),
    };

    /// Chromatron 4 v1.0 (re/notes/chromatron4): Chromatron 3's rules, plus types 21–22, a
    /// 128-beam cap and the Control-O (0x0F) cheat; its save file has no marker.
    pub const C4: RuleSet = RuleSet {
        game: Game::C4,
        max_kind: 22,
        splitter: Splitter::ReflectHeadOn,
        max_beams: 128,
        teleport_targets: &[Piece::Teleporter, Piece::MobileTeleporter],
        passes: Passes::GatesAroundComplementors,
        unlock: Unlock::Fixed(25),
        cheat_key: 0x0f,
        save_file_name: "com.silverspaceship.chromatron4.dat",
        save_bit_perm: C2_SAVE_BIT_PERM,
        save_marker: None,
        saved_ids: 128,
        clipboard_digit: b'4',
        text_boxes: TextBoxSet::Purchased,
        mobile_teleporter_sprite: Some(145),
    };

    /// The rules of `game`.
    pub fn of(game: Game) -> &'static RuleSet {
        match game {
            Game::C1 => &RuleSet::C1,
            Game::C2 => &RuleSet::C2,
            Game::C3 => &RuleSet::C3,
            Game::C4 => &RuleSet::C4,
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
    fn each_game_has_its_own_rules() {
        assert_eq!(RuleSet::of(Game::C1), &RuleSet::C1);
        assert_eq!(RuleSet::of(Game::C2), &RuleSet::C2);
        assert_eq!(RuleSet::of(Game::C3), &RuleSet::C3);
        assert_eq!(RuleSet::of(Game::C4), &RuleSet::C4);
        for game in Game::ALL {
            assert_eq!(RuleSet::of(game).game, game);
        }
    }

    #[test]
    fn chromatron_3_and_4_keep_chromatron_2s_save_table_and_splitter() {
        // platform.md P7 of both games: the same `save_bit_perm` bytes as C2.
        for rules in [RuleSet::C3, RuleSet::C4] {
            assert_eq!(rules.save_bit_perm, RuleSet::C2.save_bit_perm);
            assert_eq!(rules.splitter, Splitter::ReflectHeadOn);
            assert_eq!(rules.unlock, Unlock::Fixed(25));
            assert_eq!(rules.saved_ids, 128);
            assert_eq!(rules.text_boxes, TextBoxSet::Purchased);
        }
        assert_eq!([RuleSet::C3.cheat_key, RuleSet::C4.cheat_key], [b'^', 0x0f]);
        assert_eq!(
            [RuleSet::C3.save_marker, RuleSet::C4.save_marker],
            [Some(0xff), None]
        );
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
    fn piece_types_16_to_20_exist_from_chromatron_3_on_and_21_22_in_chromatron_4() {
        for rules in [RuleSet::C3, RuleSet::C4] {
            assert_eq!(rules.piece(15), Some(Piece::Complementor));
            assert_eq!(rules.piece(16), Some(Piece::LogicGate));
            assert_eq!(rules.piece(17), Some(Piece::Starburst));
            assert_eq!(rules.piece(18), Some(Piece::MobileTeleporter));
            assert_eq!(rules.piece(19), Some(Piece::DualReflector));
            assert_eq!(rules.piece(20), Some(Piece::QuadBender));
        }
        assert_eq!(RuleSet::C3.piece(21), None);
        assert_eq!(RuleSet::C4.piece(21), Some(Piece::OccupiedSquare));
        assert_eq!(RuleSet::C4.piece(22), Some(Piece::PrimaryLogicGate));
        assert_eq!(RuleSet::C4.piece(23), None);
        for kind in 16..=22 {
            assert_eq!(RuleSet::C2.piece(kind), None, "type {kind}");
        }
    }

    #[test]
    fn beams_teleports_and_passes_per_game() {
        assert_eq!(
            [RuleSet::C1, RuleSet::C2, RuleSet::C3, RuleSet::C4].map(|r| r.max_beams),
            [32, 32, 32, 128]
        );
        for rules in [RuleSet::C1, RuleSet::C2] {
            assert_eq!(rules.teleport_targets, [Piece::Teleporter]);
            assert_eq!(rules.passes, Passes::Complementors);
        }
        for rules in [RuleSet::C3, RuleSet::C4] {
            assert_eq!(
                rules.teleport_targets,
                [Piece::Teleporter, Piece::MobileTeleporter]
            );
            assert_eq!(rules.passes, Passes::GatesAroundComplementors);
        }
    }

    #[test]
    fn save_permutations_are_permutations_of_0_to_31() {
        for rules in [RuleSet::C1, RuleSet::C2, RuleSet::C3, RuleSet::C4] {
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
