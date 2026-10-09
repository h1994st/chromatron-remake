//! Chromatron 3 decoding, checked against facts in re/notes/chromatron3/data.md.

use std::sync::LazyLock;

use spectrograph::{Game, GameData, Record, decode};

fn decoded(game: Game) -> GameData {
    let binary = game.read_original().unwrap_or_else(|e| panic!("{e}"));
    decode(game, &binary).unwrap()
}

static C3: LazyLock<GameData> = LazyLock::new(|| decoded(Game::C3));

#[test]
fn decodes_all_78_levels_in_id_order() {
    assert_eq!(C3.game, Game::C3);
    assert_eq!(C3.levels.len(), 78);
    for (i, level) in C3.levels.iter().enumerate() {
        assert_eq!(usize::from(level.id), i + 1);
        assert_eq!(level.game, Game::C3, "level {}", level.id);
    }
}

#[test]
fn level_order_reaches_50_of_them() {
    assert_eq!(
        C3.level_order.as_ref(),
        [
            11, 19, 59, 76, 75, 22, 58, 73, 31, 49, 67, 34, 68, 74, 27, 50, 55, 60, 66, 30, 71, 44,
            54, 62, 63, 56, 45, 35, 33, 72, 61, 40, 48, 64, 29, 37, 42, 43, 51, 65, 52, 47, 23, 36,
            41, 25, 24, 38, 46, 57
        ]
    );
    let unreachable: Vec<u8> = (1..=78).filter(|id| !C3.level_order.contains(id)).collect();
    assert_eq!(
        unreachable,
        [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13, 14, 15, 16, 17, 18, 20, 21, 26, 28, 32, 39, 53,
            69, 70, 77, 78
        ]
    );
}

#[test]
fn slot_1_and_id_1_match_the_worked_examples() {
    let level = C3.slot_level(0).unwrap();
    assert_eq!((level.id, level.records.len()), (11, 71));
    assert_eq!(
        level.records[0],
        Record {
            kind: 1,
            dir: 0,
            colour: 4,
            x: 0,
            y: 0
        }
    );
    let movable: Vec<(u8, u8, u8, u8)> = level
        .records
        .iter()
        .filter(|r| matches!(r.kind, 3..=9 | 13 | 15 | 16 | 18..=20))
        .map(|r| (r.kind, r.x, r.y, r.dir))
        .collect();
    assert_eq!(movable, [(4, 11, 7, 0), (8, 9, 9, 0), (3, 6, 12, 2)]);
    assert_eq!(
        C3.level(1).unwrap().records[0],
        Record {
            kind: 3,
            dir: 6,
            colour: 1,
            x: 6,
            y: 2
        }
    );
}

#[test]
fn the_dual_reflector_and_quad_bender_records() {
    let dual = Record {
        kind: 19,
        dir: 1,
        colour: 4,
        x: 7,
        y: 7,
    };
    assert!(C3.slot_level(16).unwrap().records.contains(&dual));
    assert!(
        C3.level(44)
            .unwrap()
            .records
            .contains(&Record { kind: 20, ..dual })
    );
}

#[test]
fn record_counts_match_the_notes() {
    for (id, count) in [
        (11, 71),
        (19, 22),
        (71, 193),
        (42, 151),
        (57, 75),
        (1, 18),
        (53, 75),
        (78, 7),
    ] {
        assert_eq!(C3.level(id).unwrap().records.len(), count, "level {id}");
    }
}

#[test]
fn record_kinds_total_as_in_the_notes() {
    let mut totals = [0usize; 21];
    for level in C3.levels.iter() {
        for r in level.records.iter() {
            totals[usize::from(r.kind)] += 1;
        }
    }
    assert_eq!(
        totals,
        [
            0, 2294, 148, 412, 309, 0, 6, 18, 29, 12, 866, 96, 0, 0, 2, 1, 11, 14, 20, 6, 6
        ]
    );
}

#[test]
fn types_16_to_18_appear_only_in_unreachable_levels() {
    for &id in C3.level_order.iter() {
        let kinds = C3.level(id).unwrap().records.iter().map(|r| r.kind);
        assert!(kinds.clone().all(|k| !(16..=18).contains(&k)), "id {id}");
    }
}

#[test]
fn playable_levels_have_real_names_and_drafts_placeholders() {
    let name = |id: u8| C3.level(id).unwrap().name.as_ref();
    assert_eq!(
        [name(11), name(55), name(44), name(57), name(1), name(78)],
        [
            "Easy Does It",
            "Over & Out",
            "Flexible",
            "Bullseye",
            "level001",
            "level078"
        ]
    );
    let mut names: Vec<&str> = C3
        .level_order
        .iter()
        .map(|&id| C3.level(id).unwrap().name.as_ref())
        .collect();
    let longest = names.iter().map(|n| n.len()).max();
    assert_eq!(longest, Some("Blue vs. Green III: Interlock".len()));
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 50, "every playable name is distinct");
}

#[test]
fn three_playable_levels_have_hints() {
    let playable_hints: Vec<u8> = C3
        .level_order
        .iter()
        .copied()
        .filter(|&id| !C3.level(id).unwrap().hint.is_empty())
        .collect();
    assert_eq!(playable_hints, [55, 44, 35]);
    assert_eq!(
        C3.level(55).unwrap().hint,
        "This new piece, the DUAL-REFLECTOR, is a double-sided mirror."
    );
    assert!(C3.level(44).unwrap().hint.starts_with("The QUAD-BENDER"));
    assert_eq!(C3.level(1).unwrap().hint, "foobar");
    assert_eq!(C3.level(78).unwrap().hint, "blah");
}

#[test]
fn sprites_0_to_127_are_chromatron_2s_and_145_on_are_floor_tiles() {
    let c2 = decoded(Game::C2);
    assert_eq!(C3.sprites.len(), 160);
    assert_eq!(C3.sprites[..128], c2.sprites[..]);
    let floor: Vec<usize> = (128..160)
        .filter(|&i| C3.sprites[i] == C3.sprites[0])
        .collect();
    assert_eq!(floor, (145..160).collect::<Vec<_>>());
    assert_eq!(C3.palette, c2.palette);
    assert_eq!(C3.beam_colors, c2.beam_colors);
}

#[test]
fn ui_text_is_decoded() {
    let ui = &C3.ui;
    assert_eq!(ui.window_title, "Chromatron 3 (v1.1)");
    assert_eq!(ui.you_win, "You win!");
    assert_eq!(ui.won, "(won)");
    assert_eq!(
        ui.click_next,
        "Click on a level or press spacebar for next."
    );
    assert_eq!(ui.thank_you, "Thank you for purchasing this game.");
    assert_eq!((ui.freeware.as_ref(), ui.more_levels.as_ref()), ("", ""));
    assert_eq!(ui.site_url, "silverspaceship.com");
}
