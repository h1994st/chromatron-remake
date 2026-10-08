//! Chromatron 1 decoding, checked against facts in re/notes/chromatron1/data.md.

use std::sync::LazyLock;

use spectrograph::{DecodeError, Game, GameData, Image, Record, decode, decode_rle};

/// The original executable, from `$CHROMATRON_ORIGINALS` or `chromatron_mac/`.
fn original(game: Game) -> Vec<u8> {
    game.read_original().unwrap_or_else(|e| panic!("{e}"))
}

static C1: LazyLock<GameData> = LazyLock::new(|| decode(Game::C1, &original(Game::C1)).unwrap());

#[test]
fn decodes_all_62_levels_in_id_order() {
    assert_eq!(C1.levels.len(), 62);
    for (i, level) in C1.levels.iter().enumerate() {
        assert_eq!(usize::from(level.id), i + 1);
    }
}

#[test]
fn level_1_matches_the_worked_example() {
    let records = &C1.level(1).unwrap().records;
    assert_eq!(
        records.as_ref(),
        [
            Record {
                kind: 10,
                dir: 6,
                colour: 4,
                x: 8,
                y: 2
            },
            Record {
                kind: 2,
                dir: 2,
                colour: 4,
                x: 2,
                y: 7
            },
            Record {
                kind: 3,
                dir: 1,
                colour: 4,
                x: 8,
                y: 7
            },
        ]
    );
}

#[test]
fn record_counts_match_the_notes() {
    for (id, count) in [
        (1, 3),
        (2, 7),
        (29, 65),
        (49, 143),
        (50, 188),
        (51, 129),
        (62, 27),
        (4, 26),
    ] {
        assert_eq!(C1.level(id).unwrap().records.len(), count, "level {id}");
    }
}

#[test]
fn record_kinds_total_as_in_the_notes() {
    let mut totals = [0usize; 13];
    for level in C1.levels.iter() {
        for r in level.records.iter() {
            totals[usize::from(r.kind)] += 1;
        }
    }
    assert_eq!(
        totals,
        [0, 1049, 108, 221, 269, 9, 15, 45, 46, 14, 557, 171, 140]
    );
}

#[test]
fn every_level_has_a_laser_and_a_pinwheel() {
    for level in C1.levels.iter() {
        assert!(
            level.records.iter().any(|r| r.kind == 2),
            "level {} has no laser",
            level.id
        );
        assert!(
            level.records.iter().any(|r| r.kind == 10),
            "level {} has no pinwheel",
            level.id
        );
    }
}

#[test]
fn level_order_reaches_50_distinct_levels() {
    assert_eq!(C1.level_order.len(), 50);
    assert_eq!(&C1.level_order[..6], &[1, 2, 14, 26, 27, 12]);
    assert_eq!(C1.level_order[49], 4);
    let unreachable: Vec<u8> = (1..=62).filter(|id| !C1.level_order.contains(id)).collect();
    assert_eq!(
        unreachable,
        [15, 18, 31, 33, 40, 47, 50, 51, 53, 59, 60, 61]
    );
}

#[test]
fn seventeen_levels_have_hints() {
    let hinted: Vec<u8> = C1
        .levels
        .iter()
        .filter(|l| !l.hint.is_empty())
        .map(|l| l.id)
        .collect();
    assert_eq!(
        hinted,
        [
            1, 2, 5, 13, 14, 22, 26, 27, 28, 29, 34, 36, 37, 42, 43, 44, 55
        ]
    );
    assert!(C1.level(1).unwrap().hint.starts_with("Drag the REFLECTOR"));
    assert!(
        C1.level(43)
            .unwrap()
            .hint
            .starts_with("If there are any black pinwheel targets")
    );
}

#[test]
fn sprite_0_is_the_floor_tile() {
    assert_eq!(C1.sprites.len(), 128);
    let floor = &C1.sprites[0];
    assert!(
        floor[..24].iter().all(|&p| p == 6),
        "top row is the grid line"
    );
    assert_eq!(floor[24], 6, "left column is the grid line");
    assert!(floor[25..48].iter().all(|&p| p == 8));
}

#[test]
fn digit_glyphs_are_the_top_left_of_sprites_118_to_127() {
    // data.md D7: the 12 × 12 corner holds floor (6, 8) and glyph pixels (≥ 14), and has some
    // glyph pixels.
    for digit in 118..128 {
        let corner: Vec<u8> = C1.sprites[digit]
            .chunks(24)
            .take(12)
            .flat_map(|row| row[..12].to_vec())
            .collect();
        assert!(
            corner.iter().all(|&p| p == 6 || p == 8 || p >= 14),
            "sprite {digit}"
        );
        assert!(
            corner.iter().any(|&p| p >= 14),
            "sprite {digit} has no glyph pixels"
        );
    }
}

#[test]
fn every_image_stream_ends_at_its_zero_padding() {
    let bytes = original(Game::C1);
    let image = Image::load(Game::C1, &bytes).unwrap();
    let pointer =
        |i: u32| u32::from_be_bytes(image.read(0x126dc + 4 * i, 4).unwrap().try_into().unwrap());
    for i in 0..128 {
        let (start, end) = (pointer(i), if i == 127 { 0x126dc } else { pointer(i + 1) });
        let (_, used) = decode_rle(image.tail(start).unwrap()).unwrap();
        let padding = image
            .read(start + used as u32, (end - start) as usize - used)
            .unwrap();
        assert!(
            padding.len() < 4 && padding.iter().all(|&b| b == 0),
            "image {i}"
        );
    }
}

#[test]
fn palette_is_converted_to_rgb() {
    let p = &C1.palette;
    assert_eq!(
        [p[0], p[1], p[2], p[4], p[6], p[8]],
        [0x000000, 0x0000a8, 0x00a800, 0xa80000, 0x58442c, 0x5c5c5c]
    );
    assert!(p.iter().all(|c| c & 0x030303 == 0), "6-bit DAC values × 4");
    assert_eq!(
        [p[16], p[31], p[32], p[252], p[255]],
        [0xfcfcfc, 0x0c0c0c, 0xfc0000, 0x000000, 0xfcfcfc]
    );
}

#[test]
fn beam_colors_are_indexed_by_mask() {
    assert_eq!(
        C1.beam_colors,
        [
            0x000000, 0x0000ff, 0x00c800, 0x00c8ff, 0xff0000, 0xff00ff, 0xffc800, 0xffffff
        ]
    );
}

#[test]
fn ui_text_is_decoded() {
    let ui = &C1.ui;
    assert_eq!(ui.window_title, "Chromatron 1 (v1.15)");
    assert_eq!(ui.you_win, "You win!");
    assert_eq!(ui.won, "(won)");
    assert_eq!(
        ui.click_next,
        "Click on a level or press spacebar for next."
    );
    assert_eq!(ui.freeware, "freeware");
    assert_eq!(ui.more_levels, "more levels @");
    assert_eq!(ui.site_url, "silverspaceship.com");
}

#[test]
fn other_games_are_not_decoded_yet() {
    assert_eq!(
        decode(Game::C2, &original(Game::C2)),
        Err(DecodeError::Unsupported { game: Game::C2 })
    );
}
