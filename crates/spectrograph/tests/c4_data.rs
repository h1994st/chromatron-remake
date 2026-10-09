//! Chromatron 4 decoding, checked against facts in re/notes/chromatron4/data.md.

use std::sync::LazyLock;

use spectrograph::{Game, GameData, Record, decode};

fn decoded(game: Game) -> GameData {
    let binary = game.read_original().unwrap_or_else(|e| panic!("{e}"));
    decode(game, &binary).unwrap()
}

static C4: LazyLock<GameData> = LazyLock::new(|| decoded(Game::C4));

#[test]
fn decodes_all_54_levels_in_id_order() {
    assert_eq!(C4.game, Game::C4);
    assert_eq!(C4.levels.len(), 54);
    for (i, level) in C4.levels.iter().enumerate() {
        assert_eq!(usize::from(level.id), i + 1);
        assert_eq!(level.game, Game::C4, "level {}", level.id);
    }
}

#[test]
fn level_order_reaches_50_of_them() {
    assert_eq!(
        C4.level_order.as_ref(),
        [
            49, 19, 12, 1, 10, 2, 15, 34, 4, 9, 39, 7, 28, 23, 29, 6, 44, 22, 16, 21, 30, 13, 17,
            18, 24, 51, 40, 33, 8, 37, 32, 42, 20, 45, 36, 53, 43, 47, 54, 26, 27, 14, 48, 25, 50,
            41, 52, 38, 31, 3
        ]
    );
    let unreachable: Vec<u8> = (1..=54).filter(|id| !C4.level_order.contains(id)).collect();
    assert_eq!(unreachable, [5, 11, 35, 46]);
}

#[test]
fn id_1_matches_the_worked_example() {
    let level = C4.level(1).unwrap();
    assert_eq!(level.records.len(), 18);
    assert_eq!(
        level.records[0],
        Record {
            kind: 3,
            dir: 6,
            colour: 1,
            x: 6,
            y: 2
        }
    );
    assert_eq!(
        level.records[3],
        Record {
            kind: 16,
            dir: 4,
            colour: 4,
            x: 7,
            y: 3
        },
        "a logic gate facing S"
    );
    assert_eq!(
        C4.slot_level(0).map(|l| (l.id, l.records.len())),
        Some((49, 148))
    );
}

#[test]
fn record_counts_match_the_notes() {
    for (id, count) in [
        (49, 148),
        (19, 17),
        (12, 7),
        (39, 189),
        (42, 198),
        (3, 39),
        (5, 8),
        (35, 104),
    ] {
        assert_eq!(C4.level(id).unwrap().records.len(), count, "level {id}");
    }
}

#[test]
fn record_kinds_total_as_in_the_notes() {
    let mut totals = [0usize; 23];
    for level in C4.levels.iter() {
        for r in level.records.iter() {
            totals[usize::from(r.kind)] += 1;
        }
    }
    assert_eq!(
        totals,
        [
            0, 1049, 112, 188, 212, 9, 11, 10, 15, 7, 642, 182, 10, 1, 7, 5, 9, 51, 37, 4, 1, 356,
            6
        ]
    );
    assert_eq!(totals.iter().sum::<usize>(), 2924);
}

#[test]
fn every_level_has_its_own_name() {
    let name = |id: u8| C4.level(id).unwrap().name.as_ref();
    assert_eq!(
        [name(1), name(2), name(3), name(49), name(5), name(46)],
        [
            "Down the Hatch",
            "Merge and Merge Again",
            "Easier For a Camel",
            "Crosswards",
            "level005",
            "level046"
        ]
    );
    let mut names: Vec<&str> = C4.levels.iter().map(|l| l.name.as_ref()).collect();
    let longest = names.iter().map(|n| n.len()).max();
    assert_eq!(longest, Some("Choice: It's What's For Dinner".len()));
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 54);
}

#[test]
fn eleven_hints_introduce_pieces_and_drafts_say_blah() {
    let hinted: Vec<u8> = C4
        .levels
        .iter()
        .filter(|l| !l.hint.is_empty())
        .map(|l| l.id)
        .collect();
    assert_eq!(
        hinted,
        [1, 4, 5, 6, 7, 11, 16, 17, 19, 27, 32, 35, 36, 46, 51]
    );
    for (id, start) in [
        (1, "The LOGIC GATE outputs a color"),
        (4, "The STARBURST produces four beams"),
        (16, "The MOBILE TELEPORTER works just like"),
        (27, "The PRIMARY LOGIC GATE is just like"),
        (36, "The DUAL-REFLECTOR is double-sided"),
        (51, "The OCCUPIED SQUARE passes light"),
    ] {
        assert!(C4.level(id).unwrap().hint.starts_with(start), "id {id}");
    }
    for id in [5, 11, 35, 46] {
        assert_eq!(C4.level(id).unwrap().hint, "blah", "id {id}");
    }
}

#[test]
fn only_the_teleporter_changed_among_chromatron_2s_sprites() {
    let c2 = decoded(Game::C2);
    assert_eq!(C4.sprites.len(), 160);
    let changed: Vec<usize> = (0..128)
        .filter(|&i| C4.sprites[i] != c2.sprites[i])
        .collect();
    assert_eq!(changed, [14]);
    let floor: Vec<usize> = (0..160)
        .filter(|&i| C4.sprites[i] == C4.sprites[0])
        .collect();
    assert_eq!(floor, [0, 158, 159]);
    assert_eq!(C4.palette, c2.palette);
    assert_eq!(C4.beam_colors, c2.beam_colors);
}

#[test]
fn chromatron_3_shares_sprites_up_to_143_except_the_teleporter() {
    let c3 = decoded(Game::C3);
    let differ: Vec<usize> = (0..145)
        .filter(|&i| C4.sprites[i] != c3.sprites[i])
        .collect();
    assert_eq!(differ, [14, 144]);
}

#[test]
fn ui_text_is_decoded() {
    let ui = &C4.ui;
    assert_eq!(ui.window_title, "Chromatron 4 (v1.0)");
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
