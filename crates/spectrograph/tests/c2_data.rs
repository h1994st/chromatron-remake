//! Chromatron 2 decoding, checked against facts in re/notes/chromatron2/data.md.

use std::sync::LazyLock;

use spectrograph::{Game, GameData, Record, decode};

fn decoded(game: Game) -> GameData {
    let binary = game.read_original().unwrap_or_else(|e| panic!("{e}"));
    decode(game, &binary).unwrap()
}

static C2: LazyLock<GameData> = LazyLock::new(|| decoded(Game::C2));

#[test]
fn decodes_all_52_levels_in_id_order() {
    assert_eq!(C2.game, Game::C2);
    assert_eq!(C2.levels.len(), 52);
    for (i, level) in C2.levels.iter().enumerate() {
        assert_eq!(usize::from(level.id), i + 1);
        assert_eq!(level.game, Game::C2, "level {}", level.id);
    }
}

#[test]
fn level_order_reaches_50_of_them() {
    assert_eq!(
        C2.level_order.as_ref(),
        [
            13, 32, 21, 15, 9, 1, 41, 40, 5, 10, 12, 8, 4, 18, 17, 30, 16, 35, 36, 34, 33, 47, 14,
            3, 23, 38, 39, 46, 44, 28, 19, 45, 20, 50, 11, 22, 42, 25, 29, 43, 31, 26, 49, 27, 7,
            48, 51, 2, 37, 6
        ]
    );
    let unreachable: Vec<u8> = (1..=52).filter(|id| !C2.level_order.contains(id)).collect();
    assert_eq!(unreachable, [24, 52]);
}

#[test]
fn slot_1_matches_the_worked_example() {
    let level = C2.slot_level(0).unwrap();
    assert_eq!((level.id, level.records.len()), (13, 16));
    assert_eq!(
        level.records[0],
        Record {
            kind: 10,
            dir: 0,
            colour: 1,
            x: 11,
            y: 1
        }
    );
    assert!(level.records.contains(&Record {
        kind: 2,
        dir: 0,
        colour: 1,
        x: 7,
        y: 14
    }));
    assert_eq!(
        C2.level(1).unwrap().records[0],
        Record {
            kind: 2,
            dir: 4,
            colour: 1,
            x: 7,
            y: 0
        }
    );
}

#[test]
fn record_counts_match_the_notes() {
    for (id, count) in [
        (1, 107),
        (2, 187),
        (6, 80),
        (13, 16),
        (24, 66),
        (46, 116),
        (52, 8),
    ] {
        assert_eq!(C2.level(id).unwrap().records.len(), count, "level {id}");
    }
}

#[test]
fn record_kinds_total_as_in_the_notes() {
    let mut totals = [0usize; 16];
    for level in C2.levels.iter() {
        for r in level.records.iter() {
            totals[usize::from(r.kind)] += 1;
        }
    }
    assert_eq!(
        totals,
        [
            0, 800, 98, 171, 183, 5, 7, 25, 35, 9, 466, 95, 32, 27, 25, 12
        ]
    );
}

#[test]
fn every_level_has_a_name() {
    let name = |id: u8| C2.level(id).unwrap().name.as_ref();
    assert_eq!(
        [name(1), name(2), name(3), name(13), name(37), name(52)],
        [
            "Which One?",
            "Back And Forth",
            "Right Through",
            "Line of Sight",
            "Feedback Eliminator",
            "level052"
        ]
    );
    assert_eq!(name(22), "Limited Access");
    assert_eq!(name(33), "Limited Access");
    assert!(C2.levels.iter().all(|l| !l.name.is_empty()));
    let longest = C2.levels.iter().map(|l| l.name.len()).max();
    assert_eq!(longest, Some("...Standing in the Intersection".len()));
}

#[test]
fn eleven_playable_levels_and_id_52_have_hints() {
    let hinted: Vec<u8> = C2
        .levels
        .iter()
        .filter(|l| !l.hint.is_empty())
        .map(|l| l.id)
        .collect();
    assert_eq!(hinted, [6, 8, 10, 13, 15, 18, 29, 35, 36, 37, 41, 52]);
    assert!(C2.level(35).unwrap().hint.starts_with("The COMPLEMENTOR"));
    assert!(C2.level(41).unwrap().hint.starts_with("The MULTI-FILTER"));
    assert!(C2.level(10).unwrap().hint.starts_with("The twister"));
    assert_eq!(C2.level(52).unwrap().hint, "blah");
}

#[test]
fn only_the_new_pieces_sprites_differ_from_chromatron_1() {
    let c1 = decoded(Game::C1);
    assert_eq!(C2.sprites.len(), 128);
    let changed: Vec<usize> = (0..128)
        .filter(|&i| C2.sprites[i] != c1.sprites[i])
        .collect();
    assert_eq!(changed, [15, 100, 101, 102, 103, 116, 117]);
    assert_eq!(C2.palette, c1.palette);
    assert_eq!(C2.beam_colors, c1.beam_colors);
}

#[test]
fn ui_text_is_decoded() {
    let ui = &C2.ui;
    assert_eq!(ui.window_title, "Chromatron 2 (v1.4)");
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

#[test]
fn chromatron_1_has_no_level_names_or_thank_you() {
    let c1 = decoded(Game::C1);
    assert!(
        c1.levels
            .iter()
            .all(|l| l.name.is_empty() && l.game == Game::C1)
    );
    assert_eq!(c1.ui.thank_you, "");
}
