//! The page's session, driven natively the way `web/main.js` drives it in the browser.

use std::fs;
use std::path::PathBuf;

use chromatron_web::save_text::{from_save_text, to_save_text};
use chromatron_web::{Flag, LevelState, Session};

/// Each game's cheat key as the browser reports it, and whether Control is held (Chromatron 4's
/// Control-O).
const CHEATS: [(&str, bool); 4] = [("L", false), ("z", false), ("^", false), ("o", true)];

/// The `tests/solutions/game<game>/level-NN.txt` fixtures as (level, text), by level.
fn solutions(game: u8) -> Vec<(u32, String)> {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/solutions/game{game}"));
    let mut found: Vec<(u32, String)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            let level = name
                .strip_prefix("level-")?
                .strip_suffix(".txt")?
                .parse()
                .ok()?;
            Some((level, fs::read_to_string(&path).ok()?.trim().to_owned()))
        })
        .collect();
    found.sort();
    assert!(!found.is_empty(), "no fixtures in {}", dir.display());
    found
}

/// Wins every slot before `slot` with the cheat key and steps on with `=`.
fn cheat_to(session: &mut Session, game: u8, slot: u32) {
    let (cheat, ctrl) = CHEATS[usize::from(game - 1)];
    while session.slot() < slot {
        let from = session.slot();
        assert!(Flag::Consumed.is_in(session.key(cheat, ctrl, false, false)));
        session.key("=", false, false, false);
        assert_eq!(session.slot(), from + 1, "game {game}: = after the cheat");
    }
}

fn started(game: u8) -> Session {
    let mut session = Session::new();
    session.select(game, None, None);
    session
}

#[test]
fn nothing_happens_before_a_game_is_selected() {
    let mut session = Session::new();
    assert_eq!(session.render(), 0);
    assert_eq!(session.key(" ", false, false, false), 0);
    assert_eq!(session.pointer_down(1.0, 1.0, 0, false, false, false), 0);
    assert_eq!(session.pointer_up(1.0, 1.0, 0), 0);
    assert_eq!(session.wheel(0.0, 3.0, false, 0.0), 0);
    assert_eq!(session.command("next"), 0);
    assert_eq!(session.paste("1-01-hI"), 0);
    assert_eq!(session.copy(), None);
    assert_eq!(session.save_text(), None);
    assert_eq!(session.quit(1), None);
    assert_eq!(
        (session.game(), session.slot(), session.won()),
        (0, 0, false)
    );
    assert_eq!(session.title(), "");
    assert_eq!(session.select(0, None, None), 0);
    assert_eq!(session.select(5, None, None), 0);
}

#[test]
fn every_game_opens_on_a_full_frame_with_its_text() {
    for game in 1..=4u8 {
        let mut session = Session::new();
        let flags = session.select(game, None, None);
        assert!(Flag::Redraw.is_in(flags) && !Flag::CorruptSave.is_in(flags));
        assert_eq!(session.game(), game);
        assert_eq!(session.render(), 1);
        assert_eq!(session.dirty_rects(), [[0, 0, 640, 480]]);
        assert_eq!(
            &session.rgba()[..4],
            &[0xa4, 0xa4, 0xa4, 0xff],
            "BACKGROUND at (0, 0)"
        );
        assert!(session.text_changed() && session.text_count() > 0);
        assert_eq!(session.text_rect(0).len(), 4);
        assert!(session.text_rect(session.text_count()).is_empty());
        assert_eq!(session.text(session.text_count()), "");
        assert!(
            session
                .title()
                .starts_with(&format!("Chromatron {game} (v"))
        );
        assert_eq!(session.render(), 0, "nothing changed since");
        assert!(!session.text_changed());
    }
}

#[test]
fn the_win_hint_is_the_games_click_next_text() {
    assert_eq!(Session::new().click_next(), "", "nothing before a select");
    for game in 1..=4u8 {
        let mut session = started(game);
        let click_next = session.click_next();
        assert!(!click_next.is_empty(), "game {game}");
        let (cheat, ctrl) = CHEATS[usize::from(game - 1)];
        session.key(cheat, ctrl, false, false);
        assert!(session.won());
        session.render();
        let shown: Vec<String> = (0..session.text_count()).map(|i| session.text(i)).collect();
        assert!(shown.contains(&click_next), "game {game}: {shown:?}");
    }
}

#[test]
fn the_stored_solutions_win_in_every_game() {
    for game in 1..=4u8 {
        for (level, solution) in solutions(game) {
            let mut session = started(game);
            cheat_to(&mut session, game, level - 1);
            let flags = session.paste(&solution);
            assert!(
                Flag::Redraw.is_in(flags) && Flag::Save.is_in(flags),
                "{solution}"
            );
            assert!(session.won(), "{solution}");
            let copied = session.copy().expect("a won board copies");
            assert!(
                copied.starts_with(&format!("{game}-{level:02}-")),
                "{copied}"
            );
        }
    }
}

#[test]
fn a_save_brings_back_progress_and_the_last_level() {
    let (level, solution) = solutions(2).remove(0);
    let mut session = started(2);
    cheat_to(&mut session, 2, level - 1);
    session.paste(&solution);
    let saved = session.save_text().expect("a started game has a save");
    let mut resumed = Session::new();
    let flags = resumed.select(2, Some(saved), Some(level - 1));
    assert!(!Flag::CorruptSave.is_in(flags));
    assert_eq!(resumed.slot(), level - 1);
    assert!(resumed.won(), "the saved board is restored");
}

#[test]
fn quit_returns_the_save_with_the_current_board() {
    let mut session = started(1);
    assert_eq!(session.quit(2), None, "Chromatron 2 was never started");
    assert_eq!(session.quit(9), None);
    session.paste("1-01-hI");
    let text = session.quit(1).expect("Chromatron 1 is started");
    let bytes = from_save_text(&text).expect("base64");
    assert!(bytes.len() >= 32, "bitmap plus a board record");
    let mut resumed = Session::new();
    resumed.select(1, Some(text), None);
    assert!(resumed.won(), "slot 0's board came back");
}

#[test]
fn a_corrupt_save_starts_fresh_and_says_so() {
    // Not base64; then base64 of an all-ones bitmap, whose check bits are wrong (platform.md P7).
    for bad in ["not base64!".to_owned(), to_save_text(&[0xff; 32])] {
        let mut session = Session::new();
        let flags = session.select(1, Some(bad.clone()), Some(3));
        assert!(Flag::CorruptSave.is_in(flags), "{bad}");
        assert!(Flag::Redraw.is_in(flags));
        assert_eq!(session.slot(), 0);
        assert!(!session.won());
    }
    let mut session = Session::new();
    let flags = session.select(1, Some(String::new()), None);
    assert!(
        !Flag::CorruptSave.is_in(flags),
        "an empty save is a fresh game"
    );
}

#[test]
fn the_last_level_is_reopened_only_when_open() {
    let mut session = Session::new();
    session.select(2, None, Some(3));
    assert_eq!(session.slot(), 3, "a fresh Chromatron 2 opens slots 0-4");
    let mut session = Session::new();
    session.select(1, None, Some(7));
    assert_eq!(session.slot(), 0, "a fresh Chromatron 1 opens slot 0 only");
    let mut session = Session::new();
    session.select(1, None, Some(60));
    assert_eq!(session.slot(), 0);
}

#[test]
fn a_game_already_started_keeps_its_state() {
    let mut session = Session::new();
    session.select(2, None, Some(3));
    session.select(1, None, None);
    let flags = session.select(2, Some("garbage".to_owned()), Some(0));
    assert!(
        !Flag::CorruptSave.is_in(flags),
        "the stored save is not read again"
    );
    assert_eq!((session.game(), session.slot()), (2, 3));
}

#[test]
fn keys_follow_the_browser_rules() {
    let mut session = started(2);
    let flags = session.key(" ", false, false, false);
    assert!(Flag::Consumed.is_in(flags) && Flag::Slot.is_in(flags));
    assert_eq!(session.slot(), 1);
    assert!(Flag::Slot.is_in(session.key("-", false, false, false)));
    assert_eq!(session.slot(), 0);
    assert_eq!(
        session.key("=", false, true, false),
        0,
        "⌘= is the browser's zoom"
    );
    assert_eq!(
        session.key("=", false, false, true),
        0,
        "repeats are ignored"
    );
    assert_eq!(
        session.key("r", true, false, false),
        0,
        "Ctrl+R stays the browser's"
    );
    assert_eq!(
        session.key("o", true, false, false),
        0,
        "Control-O is only Chromatron 4's"
    );
    assert_eq!(session.key("Enter", false, false, false), 0);
    assert_eq!(
        session.key("x", false, false, false),
        0,
        "a key the game ignores stays the browser's"
    );
    assert_eq!(
        session.key("/", false, false, false),
        0,
        "Firefox's Quick Find"
    );

    let mut session = started(4);
    let flags = session.key("o", true, false, false);
    assert!(Flag::Consumed.is_in(flags) && Flag::Save.is_in(flags));
    assert!(session.won());
}

#[test]
fn commands_are_the_menu_keys() {
    let mut session = started(2);
    assert!(Flag::Slot.is_in(session.command("next")));
    assert_eq!(session.slot(), 1);
    assert!(Flag::Slot.is_in(session.command("previous")));
    assert_eq!(session.slot(), 0);
    assert!(Flag::Redraw.is_in(session.command("reset")));
    assert_eq!(session.slot(), 0);
    assert_eq!(session.command("quit"), 0);
}

#[test]
fn scrolling_up_goes_to_the_previous_level_once_per_gesture() {
    let mut session = Session::new();
    session.select(2, None, Some(3));
    // Browser deltas: negative y is scrolling up.
    assert!(Flag::Slot.is_in(session.wheel(0.0, -3.0, false, 0.0)));
    assert_eq!(session.slot(), 2, "a notch up is the previous level");
    assert!(Flag::Slot.is_in(session.wheel(0.0, 3.0, false, 10.0)));
    assert_eq!(session.slot(), 3, "a notch down is the next level");
    // A trackpad flick with momentum: many small pixel deltas, one step.
    let steps = (0..100)
        .filter(|&i| Flag::Slot.is_in(session.wheel(0.0, 6.0, true, 1000.0 + 8.0 * f64::from(i))))
        .count();
    assert_eq!(steps, 1);
    assert_eq!(session.slot(), 4);
    // Chrome's 100 px notches, 50 ms apart, are one gesture too.
    let steps = (0..4)
        .filter(|&i| {
            Flag::Slot.is_in(session.wheel(0.0, -100.0, true, 5000.0 + 50.0 * f64::from(i)))
        })
        .count();
    assert_eq!(steps, 1);
    assert_eq!(session.slot(), 3);
}

#[test]
fn pastes_tolerate_surrounding_whitespace_and_reject_other_levels() {
    let mut session = started(1);
    assert_eq!(session.paste("2-07-OFHqye"), 0, "Chromatron 2's text");
    assert_eq!(session.paste("1-02-hI"), 0, "another level");
    assert!(!session.won());
    assert!(Flag::Redraw.is_in(session.paste("\n1-01-hI\r\n")));
    assert!(session.won());
    let mut session = started(1);
    assert!(
        Flag::Redraw.is_in(session.paste("1-01-hI ")),
        "a trailing space"
    );
    assert!(session.won());
}

#[test]
fn text_is_reported_only_when_it_changes() {
    let mut session = started(1);
    session.render();
    assert!(session.text_changed());
    assert!(Flag::Redraw.is_in(session.command("reset")));
    assert_eq!(session.render(), 1);
    assert!(!session.text_changed(), "same level, same boxes");
    session.paste("1-01-hI");
    session.render();
    assert!(session.text_changed(), "the win text appears");
    session.select(2, None, None);
    session.render();
    assert!(session.text_changed(), "another game");
}

#[test]
fn a_drag_repaints_only_its_clipped_rects() {
    // Chromatron 1's first level starts with its reflector in toolbox slot 0, centred at
    // (460, 20) (`move_pieces_to_toolbox`; layout.rs: toolbox at 448, 8, pitch 26).
    let mut session = started(1);
    session.render();
    session.pointer_down(460.0, 20.0, 0, false, false, false);
    assert!(Flag::Redraw.is_in(session.pointer_move(100.4, 100.6)));
    assert_eq!(session.render(), 2);
    // The old 60 × 60 box around the press, clipped at the top, and the new 25 × 25 box.
    assert_eq!(
        session.dirty_rects(),
        [[430, 0, 490, 50], [88, 88, 113, 113]]
    );
    session.pointer_cancel();
    assert_eq!(session.render(), 1, "ending a drag repaints everything");
}

#[test]
fn a_cancelled_drag_ignores_the_stray_release() {
    let mut session = started(1);
    session.pointer_down(460.0, 20.0, 0, false, false, false);
    session.pointer_move(100.0, 100.0);
    session.pointer_cancel();
    assert_eq!(
        session.pointer_move(200.0, 200.0),
        0,
        "no button is held any more"
    );
    assert_eq!(
        session.pointer_up(200.0, 200.0, 0),
        0,
        "a release without a press is dropped"
    );
    assert_eq!(session.pointer_cancel(), 0);
}

#[test]
fn switching_games_mid_drag_ends_the_drag() {
    let mut session = started(1);
    session.pointer_down(460.0, 20.0, 0, false, false, false);
    session.pointer_move(100.0, 100.0);
    session.select(2, None, None);
    assert_eq!(session.pointer_up(100.0, 100.0, 0), 0);
    session.select(1, None, None);
    assert_eq!(session.render(), 1);
    assert_eq!(session.dirty_rects(), [[0, 0, 640, 480]]);
}

#[test]
fn touch_taps_turn_clockwise_and_touch_drags_still_drop() {
    // The page sends touch presses as button 0, and a touch release as button 2 only while the
    // "Turn clockwise" toggle is on and no piece is being dragged.
    let mut session = started(1);
    let in_toolbox = session.copy();
    assert!(!session.dragging());
    session.pointer_down(460.0, 20.0, 0, false, false, false);
    session.pointer_move(252.0, 198.0);
    assert!(session.dragging());
    session.pointer_up(252.0, 198.0, 0);
    assert!(!session.dragging());
    assert_ne!(
        session.copy(),
        in_toolbox,
        "the reflector dropped on board cell (8, 7)"
    );
    let mut solved = started(1);
    solved.paste("1-01-hI");
    assert!(solved.won());
    // The solution's reflector sits on board cell (8, 7), centred at (252, 198).
    solved.pointer_down(252.0, 198.0, 0, false, false, false);
    solved.pointer_up(252.0, 198.0, 2);
    assert!(!solved.won(), "a clockwise tap turned the reflector away");
    solved.pointer_down(252.0, 198.0, 0, false, false, false);
    solved.pointer_up(252.0, 198.0, 0);
    assert!(solved.won(), "an anticlockwise tap turned it back");
}

#[test]
fn a_forgotten_game_reads_its_save_again() {
    let (level, solution) = solutions(2).remove(0);
    let mut other_tab = started(2);
    cheat_to(&mut other_tab, 2, level - 1);
    other_tab.paste(&solution);
    let newer = other_tab.save_text().expect("started");

    let mut session = Session::new();
    session.select(2, None, None);
    assert!(!session.forget(1), "Chromatron 1 never ran");
    assert!(session.forget(2));
    assert_eq!(
        session.game(),
        0,
        "no game is current until the next select"
    );
    assert_eq!(session.render(), 0);
    session.select(2, Some(newer), Some(level - 1));
    assert_eq!(session.slot(), level - 1);
    assert!(session.won(), "the other tab's progress");
}

#[test]
fn a_strip_tap_needs_a_primary_release() {
    let (x, y) = lumen::layout::strip_origin(1);
    let (x, y) = (f64::from(x + 8), f64::from(y + 8));
    let mut session = started(2);
    assert!(session.on_strip(x, y));
    assert!(!session.on_strip(252.0, 198.0), "a board cell");
    session.pointer_down(x, y, 0, false, false, false);
    session.pointer_up(x, y, 2);
    assert_eq!(session.slot(), 0, "an alternate release does nothing");
    session.pointer_down(x, y, 0, false, false, false);
    session.pointer_up(x, y, 0);
    assert_eq!(session.slot(), 1);
}

#[test]
fn a_forgotten_drag_ignores_the_stray_release() {
    let mut session = started(1);
    session.pointer_down(460.0, 20.0, 0, false, false, false);
    session.pointer_move(100.0, 100.0);
    assert!(session.forget(1));
    assert_eq!(session.pointer_up(100.0, 100.0, 0), 0);
    assert_eq!(session.pointer_move(200.0, 200.0), 0);
    assert_eq!(session.game(), 0);
    assert!(!session.dragging());
    session.select(1, None, None);
    assert_eq!(session.copy().as_deref(), started(1).copy().as_deref());
}

#[test]
fn the_levels_picker_sees_the_strip_states() {
    assert_eq!(
        Session::new().slot_state(0),
        LevelState::Locked,
        "no game yet"
    );
    let mut session = started(2);
    assert_eq!(session.slot_state(0), LevelState::Current);
    assert_eq!(
        session.slot_state(4),
        LevelState::Unlocked,
        "a fresh Chromatron 2 opens slots 0-4"
    );
    assert_eq!(session.slot_state(5), LevelState::Locked);
    assert_eq!(session.slot_state(50), LevelState::Locked, "past the strip");
    assert_eq!(session.slot_state(u32::MAX), LevelState::Locked);
    cheat_to(&mut session, 2, 1);
    assert_eq!(session.slot_state(0), LevelState::Completed);
    assert_eq!(session.slot_state(1), LevelState::Current);
}

#[test]
fn a_level_opens_from_the_picker_only_when_unlocked() {
    assert_eq!(Session::new().open_slot(0), 0, "no game yet");
    let mut session = started(2);
    assert_eq!(session.open_slot(5), 0, "locked");
    assert_eq!(session.open_slot(50), 0, "past the strip");
    assert_eq!(session.slot(), 0);
    let flags = session.open_slot(3);
    assert!(Flag::Redraw.is_in(flags) && Flag::Slot.is_in(flags));
    assert_eq!(session.slot(), 3);
    let flags = session.open_slot(3);
    assert!(
        Flag::Redraw.is_in(flags) && !Flag::Slot.is_in(flags),
        "the current level starts again from its board"
    );
}

#[test]
fn opening_a_level_ends_a_drag() {
    let mut session = started(1);
    session.pointer_down(460.0, 20.0, 0, false, false, false);
    session.pointer_move(100.0, 100.0);
    assert!(session.dragging());
    assert!(Flag::Redraw.is_in(session.open_slot(0)));
    assert!(!session.dragging());
    assert_eq!(
        session.pointer_up(100.0, 100.0, 0),
        0,
        "the stray release is dropped"
    );
}

#[test]
fn the_page_regions_come_from_lumen_layout() {
    let session = Session::new();
    assert_eq!(session.board_rect(), [48, 18, 408, 378]);
    assert_eq!(session.toolbox_rect(), [446, 6, 604, 112]);
    assert_eq!(session.strip_number_rect(0), [10, 410, 30, 422]);
    assert_eq!(session.strip_number_rect(49), [490, 430, 510, 442]);
    assert!(session.strip_number_rect(50).is_empty());
}

#[test]
fn every_strip_number_lies_in_its_rect() {
    for game in 1..=4u8 {
        let mut session = started(game);
        session.render();
        let rects: Vec<Vec<i32>> = (0..50)
            .map(|slot| session.strip_number_rect(slot))
            .collect();
        let inside = |x: i32, y: i32| {
            rects
                .iter()
                .any(|r| (r[0]..r[2]).contains(&x) && (r[1]..r[3]).contains(&y))
        };
        let drawn = |x: i32, y: i32| {
            let i = ((y * 640 + x) * 4) as usize;
            session.rgba()[i..i + 3] != [0xa4; 3]
        };
        for y in 390..480 {
            for x in 0..640 {
                if drawn(x, y) {
                    assert!(inside(x, y), "game {game}: ({x}, {y})");
                }
            }
        }
        for (slot, r) in rects.iter().enumerate() {
            let any = (r[1]..r[3]).any(|y| (r[0]..r[2]).any(|x| drawn(x, y)));
            assert!(any, "game {game}: slot {slot}'s number is drawn");
        }
    }
}

#[test]
fn a_drop_between_the_page_regions_leaves_the_piece() {
    // Between the portrait views the page sends (-100, -100), a point every hit test misses.
    let mut session = started(1);
    let before = session.copy();
    session.pointer_down(460.0, 20.0, 0, false, false, false);
    session.pointer_move(252.0, 198.0);
    assert!(session.dragging());
    session.pointer_move(-100.0, -100.0);
    session.pointer_up(-100.0, -100.0, 0);
    assert!(!session.dragging());
    assert_eq!(
        session.copy(),
        before,
        "the reflector is back in the toolbox"
    );
}
