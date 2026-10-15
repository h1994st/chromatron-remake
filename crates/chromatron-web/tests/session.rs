//! The page's session, driven natively the way `web/main.js` drives it in the browser.

use std::fs;
use std::path::PathBuf;

use chromatron_web::save_text::{from_save_text, to_save_text};
use chromatron_web::{Flag, LevelState, Look, Session, render_scale, stage_height};
use halation::{Easel, Tableau};
use lumen::{BOARD_CELLS, board_index, toolbox_index};

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
    assert_eq!(session.render_at(0.0), 0);
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
        assert_eq!(session.render_at(0.0), 1);
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
        assert_eq!(session.render_at(0.0), 0, "nothing changed since");
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
        session.render_at(0.0);
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
    session.render_at(0.0);
    assert!(session.text_changed());
    assert!(Flag::Redraw.is_in(session.command("reset")));
    assert_eq!(session.render_at(0.0), 1);
    assert!(!session.text_changed(), "same level, same boxes");
    session.paste("1-01-hI");
    session.render_at(0.0);
    assert!(session.text_changed(), "the win text appears");
    session.select(2, None, None);
    session.render_at(0.0);
    assert!(session.text_changed(), "another game");
}

#[test]
fn a_drag_repaints_only_its_clipped_rects() {
    // Chromatron 1's first level starts with its reflector in toolbox slot 0, centred at
    // (460, 20) (`move_pieces_to_toolbox`; layout.rs: toolbox at 448, 8, pitch 26).
    let mut session = started(1);
    session.render_at(0.0);
    session.pointer_down(460.0, 20.0, 0, false, false, false);
    assert!(Flag::Redraw.is_in(session.pointer_move(100.4, 100.6)));
    assert_eq!(session.render_at(0.0), 2);
    // The old 60 × 60 box around the press, clipped at the top, and the new 25 × 25 box.
    assert_eq!(
        session.dirty_rects(),
        [[430, 0, 490, 50], [88, 88, 113, 113]]
    );
    session.pointer_cancel();
    assert_eq!(
        session.render_at(0.0),
        1,
        "ending a drag repaints everything"
    );
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
    assert_eq!(session.render_at(0.0), 1);
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
    assert_eq!(session.render_at(0.0), 0);
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
        session.render_at(0.0);
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

const W: u32 = 1600;
const H: u32 = 1200;

fn modern(game: u8) -> Session {
    let mut session = started(game);
    assert!(Flag::Redraw.is_in(session.set_look(Look::Modern)));
    assert!(Flag::Redraw.is_in(session.resize(W, H, 2.0, vec![0.0; 4], false)));
    session
}

fn tableau() -> Tableau {
    Tableau::arrange(&Easel::new(W, H, 2.0))
}

#[test]
fn the_modern_look_paints_its_canvas_whole_then_only_changes() {
    let mut session = modern(1);
    assert_eq!((session.canvas_width(), session.canvas_height()), (W, H));
    assert_eq!(session.render_at(0.0), 1);
    assert_eq!(session.dirty_rects(), [[0, 0, W as i32, H as i32]]);
    assert_eq!(session.rgba().len(), (W * H * 4) as usize);
    assert_eq!(session.render_at(16.0), 0, "nothing changed");
    assert_eq!(session.next_wake(16.0), -1.0);
}

#[test]
fn modern_pointers_drag_pieces_through_the_lightbox() {
    let mut session = modern(1);
    session.render_at(0.0);
    let t = tableau();
    let before = session.copy();
    let (a, b) = (
        t.cell_centre(toolbox_index(0)),
        t.cell_centre(board_index(0, 0)),
    );
    session.pointer_down(f64::from(a.x), f64::from(a.y), 0, false, false, false);
    assert!(Flag::Redraw.is_in(session.pointer_move(f64::from(b.x), f64::from(b.y))));
    assert!(session.dragging());
    let painted = session.render_at(16.0);
    assert!(
        (1..=halation::MAX_DAMAGE).contains(&painted)
            && session.dirty_rects()[0] != [0, 0, W as i32, H as i32]
    );
    session.pointer_up(f64::from(b.x), f64::from(b.y), 0);
    assert_ne!(
        session.copy(),
        before,
        "C1 level 1's corner is empty: the piece moved onto the board"
    );
    assert!(!session.on_strip(f64::from(b.x), f64::from(b.y)));
}

#[test]
fn modern_pointers_outside_the_canvas_hit_nothing() {
    let mut session = modern(1);
    let before = session.copy();
    for (x, y) in [
        (-50.0, -50.0),
        (1e12, 1e12),
        (f64::NAN, f64::NAN),
        (f64::INFINITY, -1.0),
    ] {
        session.pointer_down(x, y, 0, false, false, false);
        session.pointer_move(x, y);
        session.pointer_up(x, y, 0);
        assert!(!session.dragging());
    }
    assert_eq!(session.copy(), before);
}

#[test]
fn resize_and_look_switch_end_a_drag() {
    for switch in [true, false] {
        let mut session = modern(1);
        let t = tableau();
        let (a, b) = (
            t.cell_centre(toolbox_index(0)),
            t.cell_centre(BOARD_CELLS - 1),
        );
        session.pointer_down(f64::from(a.x), f64::from(a.y), 0, false, false, false);
        session.pointer_move(f64::from(b.x), f64::from(b.y));
        assert!(session.dragging());
        if switch {
            session.set_look(Look::Classic);
        } else {
            session.resize(W / 2, H / 2, 1.0, vec![0.0; 4], false);
        }
        assert!(!session.dragging(), "switch {switch}");
        assert!(session.render_at(32.0) >= 1);
    }
}

#[test]
fn looks_round_trip_and_repaint_whole() {
    let mut session = modern(2);
    session.render_at(0.0);
    assert_eq!(session.set_look(Look::Modern), 0, "already modern");
    session.set_look(Look::Classic);
    assert_eq!(session.look(), Look::Classic);
    assert_eq!(session.render_at(16.0), 1);
    assert_eq!(
        session.dirty_rects(),
        [[0, 0, 640, 480]],
        "classic repaints its frame"
    );
    assert!(session.text_changed(), "and its text");
    session.set_look(Look::Modern);
    assert_eq!(session.render_at(32.0), 1);
    assert_eq!(
        session.dirty_rects(),
        [[0, 0, W as i32, H as i32]],
        "modern repaints cold"
    );
    session.select(3, None, None);
    assert_eq!(session.render_at(48.0), 1, "another game paints cold");
}

#[test]
fn degenerate_resizes_are_harmless() {
    let mut session = started(1);
    session.set_look(Look::Modern);
    assert_eq!(
        session.render_at(0.0),
        0,
        "no canvas before the first resize"
    );
    for (w, h, scale) in [
        (0, 0, f64::NAN),
        (1, 1, 0.0),
        (300, 200, -2.0),
        (5, 3000, 3.0),
    ] {
        session.resize(w, h, scale, vec![f64::NAN, -4.0], false);
        assert!(session.canvas_width() >= 1 && session.canvas_height() >= 1);
        session.render_at(0.0);
        session.pointer_down(1.0, 1.0, 0, false, false, false);
        session.pointer_up(1.0, 1.0, 0);
    }
}

#[test]
fn the_render_scale_follows_the_board() {
    assert_eq!(render_scale(1400.0, 955.0, 2.0, false), 1.75);
    assert_eq!(render_scale(1400.0, 955.0, f64::NAN, false), 1.0);
}

fn centre(rect: &[i32]) -> (f64, f64) {
    (
        f64::from(rect[0] + rect[2]) / 2.0,
        f64::from(rect[1] + rect[3]) / 2.0,
    )
}

fn tap(session: &mut Session, (x, y): (f64, f64), button: i16) -> u32 {
    session.pointer_down(x, y, button, false, false, false) | session.pointer_up(x, y, button)
}

#[test]
fn the_levels_pill_opens_the_sheet_and_says_so() {
    let mut session = modern(2);
    session.render_at(0.0);
    assert!(!session.has_inline_picker());
    let t = tableau();
    let pill = t.device(t.card_parts.levels.unwrap());
    let flags = tap(
        &mut session,
        centre(&[pill.left, pill.top, pill.right, pill.bottom]),
        0,
    );
    assert!(Flag::SheetOpened.is_in(flags) && Flag::Redraw.is_in(flags));
    assert!(session.sheet_open());
    session.render_at(16.0);
    assert!(
        session.captions_changed() && session.caption_count() == 0,
        "captions hide under the sheet"
    );
    let flags = session.key("Escape", false, false, false);
    assert!(
        Flag::Consumed.is_in(flags) && !session.sheet_open(),
        "Escape closes it first"
    );
    assert_eq!(
        session.key("Escape", false, false, false),
        0,
        "with the sheet closed, Escape stays the browser's"
    );
}

/// Review M1: Escape closing the sheet ends a press held on a chip, so its release over a board
/// piece turns nothing.
#[test]
fn escape_ends_a_press_held_under_the_sheet() {
    let mut session = modern(1);
    let t = tableau();
    let (a, b) = (
        t.cell_centre(toolbox_index(0)),
        t.cell_centre(board_index(0, 0)),
    );
    let (b_x, b_y) = (f64::from(b.x), f64::from(b.y));
    session.pointer_down(f64::from(a.x), f64::from(a.y), 0, false, false, false);
    session.pointer_move(b_x, b_y);
    session.pointer_up(b_x, b_y, 0);
    let placed = session.copy();
    assert!(Flag::SheetOpened.is_in(session.command("levels")));
    let chip = centre(&session.chip_rect(0));
    session.pointer_down(chip.0, chip.1, 0, false, false, false);
    assert!(Flag::Consumed.is_in(session.key("Escape", false, false, false)));
    assert!(!session.sheet_open());
    assert_eq!(
        session.pointer_up(b_x, b_y, 0),
        0,
        "the release reaches nothing"
    );
    assert_eq!(session.copy(), placed, "the piece did not turn");
}

/// The classic look has no sheet and no captions: switching to it closes the sheet and clears the
/// captions, and its `levels` command is the page's own dialog.
#[test]
fn the_classic_look_has_no_sheet_or_captions() {
    let mut session = modern(2);
    session.render_at(0.0);
    assert!(session.caption_count() > 0);
    assert!(Flag::SheetOpened.is_in(session.command("levels")));
    session.set_look(Look::Classic);
    assert!(!session.sheet_open(), "set_look closed the sheet");
    assert_eq!(session.command("levels"), 0, "classic: the page's dialog");
    assert!(!session.sheet_open());
    session.render_at(16.0);
    assert!(!session.captions_changed());
    assert_eq!(session.caption_count(), 0);
    assert_eq!(session.caption_text(0), "");
    assert!(session.caption_rect(0).is_empty());
    session.set_look(Look::Modern);
    session.render_at(32.0);
    assert!(
        session.captions_changed() && session.caption_count() > 0,
        "back to modern: a cold paint brings them back"
    );
    assert!(!session.sheet_open(), "the sheet stays closed");
}

#[test]
fn an_alternate_tap_on_a_chip_opens_its_level() {
    let mut session = modern(2);
    session.render_at(0.0);
    assert!(Flag::SheetOpened.is_in(session.command("levels")));
    let chip = session.chip_rect(3);
    let flags = tap(&mut session, centre(&chip), 2);
    assert!(Flag::Slot.is_in(flags) && session.slot() == 3);
    session.render_at(16.0);
    assert!(!session.sheet_open(), "choosing a level closed the sheet");
}

#[test]
fn opening_levels_ends_a_drag_and_reports_sheet_opened() {
    let mut session = modern(1);
    let t = tableau();
    let (a, b) = (
        t.cell_centre(toolbox_index(0)),
        t.cell_centre(BOARD_CELLS - 1),
    );
    session.pointer_down(f64::from(a.x), f64::from(a.y), 0, false, false, false);
    session.pointer_move(f64::from(b.x), f64::from(b.y));
    assert!(session.dragging());
    let flags = session.command("levels");
    assert!(Flag::SheetOpened.is_in(flags) && !session.dragging());
    assert_eq!(
        session.command("levels") & Flag::SheetOpened as u32,
        0,
        "a second press closes it"
    );
    assert!(!session.sheet_open());
}

#[test]
fn next_on_a_won_card_goes_to_the_next_level() {
    let mut session = modern(1);
    let (cheat, ctrl) = CHEATS[0];
    session.key(cheat, ctrl, false, false);
    assert!(session.won(), "the cheat key wins the level");
    assert!(session.next_open(), "level 2 opened: Next ▶ shows");
    session.render_at(0.0);
    let t = tableau();
    let next = t.device(t.card_parts.next);
    let flags = tap(
        &mut session,
        centre(&[next.left, next.top, next.right, next.bottom]),
        0,
    );
    assert!(Flag::Slot.is_in(flags) && session.slot() == 1);
    assert!(!session.next_open());
}

#[test]
fn captions_cross_to_the_page() {
    let mut session = modern(2);
    session.render_at(0.0);
    assert!(session.captions_changed() && session.caption_count() >= 3);
    let hint = (0..session.caption_count())
        .find(|&i| session.caption_role(i) == 3)
        .expect("a hint caption");
    assert!(!session.caption_text(hint).is_empty());
    assert_eq!(session.caption_style(hint), 2, "Body");
    let rect = session.caption_rect(hint);
    assert!(rect.len() == 4 && rect[2] > rect[0] && rect[3] > rect[1]);
    assert_eq!(session.caption_alpha(hint), 1.0);
    session.render_at(16.0);
    assert!(!session.captions_changed());
    assert_eq!(session.caption_text(99), "");
    assert!(session.caption_rect(99).is_empty());
}

#[test]
fn the_page_and_the_session_agree_on_touch() {
    assert_eq!(render_scale(390.0, 740.0, 3.0, true), 2.0);
    assert_eq!(render_scale(1400.0, 955.0, 2.0, false), 1.75);
    assert!(
        (stage_height(390.0, 500.0, true) - 596.0).abs() <= 0.6,
        "spec §6's 596 (595.5 exactly)"
    );
    let mut session = started(1);
    session.set_look(Look::Modern);
    session.resize(780, 1480, 2.0, vec![0.0; 4], true);
    assert!(
        !session.has_inline_picker(),
        "390 × 740 pt touch: Stacked, a sheet picker"
    );
    let tray = session.cell_rect(toolbox_index(0));
    assert!(tray[3] - tray[1] >= 88, "touch slots ≥ 44 pt at @2");
}

/// Ruling 18 (review I2): `resize` builds a new lightbox, but the sheet stays open when the new
/// layout still has a sheet picker, closes when it gets an inline one, and survives a game switch.
#[test]
fn the_sheet_survives_a_resize_and_a_game_switch() {
    let mut session = started(2);
    session.set_look(Look::Modern);
    session.resize(780, 1480, 2.0, vec![0.0; 4], true);
    session.command("levels");
    assert!(session.sheet_open());
    session.resize(1480, 780, 2.0, vec![0.0; 4], true); // 740 × 390 pt: compact Beside, a sheet
    assert!(
        session.sheet_open() && !session.has_inline_picker(),
        "rotated, still open"
    );
    session.select(3, None, None);
    assert!(session.sheet_open(), "another game keeps the sheet");
    session.resize(2560, 1600, 2.0, vec![0.0; 4], false);
    assert!(
        session.has_inline_picker() && !session.sheet_open(),
        "an inline picker replaces it"
    );
}

/// A chip chosen in the sheet closes it at the next paint, even when a resize rebuilds the
/// lightbox in between.
#[test]
fn a_chip_chosen_before_a_resize_still_closes_the_sheet() {
    let mut session = modern(2);
    session.render_at(0.0);
    session.command("levels");
    let chip = session.chip_rect(3);
    assert!(Flag::Slot.is_in(tap(&mut session, centre(&chip), 0)));
    session.resize(W, H, 2.0, vec![0.0; 4], false);
    session.render_at(16.0);
    assert!(!session.sheet_open());
}

/// Ruling 19: the card and the margins are inert (the page pans the page from there); cells,
/// tray slots, chips and controls are not.
#[test]
fn inert_points_are_the_card_and_margins() {
    let mut session = modern(1);
    session.render_at(0.0);
    let t = tableau();
    let card = t.device(t.card_parts.body);
    assert!(session.inert(f64::from(card.left + 4), f64::from(card.top + 4)));
    assert!(session.inert(1.0, 1.0), "the top-left margin");
    let cell = session.cell_rect(0);
    assert!(!session.inert(f64::from(cell[0] + 2), f64::from(cell[1] + 2)));
    let pill = t.device(t.card_parts.levels.unwrap());
    assert!(
        !session.inert(f64::from(pill.left + 4), f64::from(pill.top + 4)),
        "a control"
    );
    session.command("levels");
    assert!(
        !session.inert(1.0, 1.0),
        "under the sheet the scrim is a control"
    );
    assert!(!started(1).inert(1.0, 1.0), "classic: nothing is inert");
}

/// Moves toolbox slot 0's piece onto board cell (0, 0) (C1 level 1's corner is empty) and
/// returns that cell's centre: a real movable piece on the board.
fn place_corner_piece(session: &mut Session) -> (f64, f64) {
    let (from, to) = (
        centre(&session.cell_rect(toolbox_index(0))),
        centre(&session.cell_rect(board_index(0, 0))),
    );
    let before = session.copy();
    session.pointer_down(from.0, from.1, 0, false, false, false);
    session.pointer_move(to.0, to.1);
    session.pointer_up(to.0, to.1, 0);
    assert_ne!(session.copy(), before, "the piece moved onto the board");
    session.render_at(0.0);
    to
}

/// Review focus 5 (Task 10a): a press on a control dragged onto a piece and released there is
/// nothing: no control fires and the piece neither moves nor turns.
#[test]
fn a_press_on_a_control_released_over_a_piece_does_nothing() {
    let mut session = modern(1);
    session.render_at(0.0);
    let piece = place_corner_piece(&mut session);
    let before = session.copy();
    let t = tableau();
    let pill = t.device(t.card_parts.levels.unwrap());
    let (x, y) = centre(&[pill.left, pill.top, pill.right, pill.bottom]);
    assert_eq!(session.pointer_down(x, y, 0, false, false, false), 0);
    assert_eq!(session.pointer_move(piece.0, piece.1), 0);
    assert_eq!(session.pointer_up(piece.0, piece.1, 0), 0);
    assert!(!session.sheet_open() && !session.dragging());
    assert_eq!(session.copy(), before, "the piece did not turn");
}

/// A press held on a control across a `resize` (which builds a new lightbox, dropping the old
/// one's press) is swallowed: its release over a piece does not reach the game.
#[test]
fn a_resize_mid_press_swallows_the_release() {
    let mut session = modern(1);
    session.render_at(0.0);
    place_corner_piece(&mut session);
    let before = session.copy();
    let t = tableau();
    let pill = t.device(t.card_parts.levels.unwrap());
    let (x, y) = centre(&[pill.left, pill.top, pill.right, pill.bottom]);
    session.pointer_down(x, y, 0, false, false, false);
    session.resize(W + 200, H, 2.0, vec![0.0; 4], false);
    session.render_at(16.0);
    let piece = centre(&session.cell_rect(board_index(0, 0)));
    session.pointer_move(piece.0, piece.1);
    assert_eq!(session.pointer_up(piece.0, piece.1, 0), 0);
    assert!(!session.sheet_open());
    assert_eq!(session.copy(), before, "the piece did not turn");
    // The next press is the game's again.
    session.pointer_down(piece.0, piece.1, 0, false, false, false);
    session.pointer_up(piece.0, piece.1, 0);
    assert_ne!(session.copy(), before, "a tap turns it");
}

/// `command("levels")` mid-drag ends the drag (lumen's cancel) before the sheet opens, so the
/// physical release that follows, over an open chip, neither opens that level nor closes the sheet.
#[test]
fn a_release_after_levels_ended_a_drag_chooses_no_level() {
    let mut session = modern(2);
    session.render_at(0.0);
    let before = session.copy();
    let t = tableau();
    let from = t.cell_centre(toolbox_index(0));
    let chip = t.sheet_chip_rect(3);
    let over = centre(&[chip.left, chip.top, chip.right, chip.bottom]);
    session.pointer_down(f64::from(from.x), f64::from(from.y), 0, false, false, false);
    session.pointer_move(over.0, over.1);
    assert!(session.dragging());
    assert!(Flag::SheetOpened.is_in(session.command("levels")));
    assert_eq!(session.pointer_up(over.0, over.1, 0), 0);
    session.render_at(16.0);
    assert!(session.sheet_open(), "the sheet stays open");
    assert_eq!(session.slot(), 0);
    assert_eq!(session.copy(), before, "the piece went back");
}

/// C1's cheat key, through the page's key path: the level is won without moving a piece.
fn cheat_win(session: &mut Session) {
    session.key("L", false, false, false);
    assert!(session.won());
}

#[test]
fn a_win_wakes_the_modern_page_until_it_settles() {
    let mut session = modern(1);
    session.render_at(0.0);
    cheat_win(&mut session);
    session.render_at(1000.0);
    assert!(session.next_wake(1000.0) >= 0.0, "the win animates");
    session.render_at(1000.0 + halation::SETTLE_MS);
    assert_eq!(
        session.next_wake(1000.0 + halation::SETTLE_MS),
        -1.0,
        "then idles"
    );
}

#[test]
fn set_reduced_motion_mid_cue_redraws_and_idles() {
    let mut session = modern(1);
    session.render_at(0.0);
    cheat_win(&mut session);
    session.render_at(1000.0);
    assert_eq!(
        session.set_reduced_motion(true) & Flag::Redraw as u32,
        Flag::Redraw as u32
    );
    assert_eq!(session.set_reduced_motion(true), 0, "no change, no redraw");
    assert!(session.render_at(1016.0) > 0, "the settled frame");
    assert_eq!(session.next_wake(1016.0), -1.0);
}

#[test]
fn reduced_motion_survives_a_resize_and_keeps_moves_still() {
    let mut session = modern(1);
    session.set_reduced_motion(true);
    session.resize(W + 1, H, 2.0, vec![0.0; 4], false);
    session.render_at(0.0);
    session.key("r", false, false, false);
    cheat_win(&mut session);
    session.render_at(1000.0);
    assert!(
        session.next_wake(1000.0) >= 0.0,
        "a reduced win still fades its header"
    );
    session.render_at(1200.0);
    assert_eq!(session.next_wake(1200.0), -1.0, "within 150 ms");
}

#[test]
fn a_resize_mid_cue_paints_cold_and_idles() {
    let mut session = modern(1);
    session.render_at(0.0);
    cheat_win(&mut session);
    session.render_at(1000.0);
    session.resize(W - 2, H, 2.0, vec![0.0; 4], false);
    assert_eq!(session.render_at(1016.0), 1, "a cold paint, whole");
    assert_eq!(session.next_wake(1016.0), -1.0);
}

#[test]
fn the_classic_look_never_wakes() {
    let mut session = modern(1);
    session.set_look(Look::Classic);
    cheat_win(&mut session);
    session.render_at(1000.0);
    assert_eq!(session.next_wake(1000.0), -1.0);
}

/// Releases toolbox slot 0's piece over board cell (0, 0) (C1 level 1's corner is empty), painting
/// each step from `t` on, so the release starts a landing cue at `t + 32`.
fn drop_corner_piece(session: &mut Session, t: f64) {
    let (from, to) = (
        centre(&session.cell_rect(toolbox_index(0))),
        centre(&session.cell_rect(board_index(0, 0))),
    );
    session.pointer_down(from.0, from.1, 0, false, false, false);
    session.render_at(t);
    session.pointer_move(to.0, to.1);
    session.render_at(t + 16.0);
    session.pointer_up(to.0, to.1, 0);
    session.render_at(t + 32.0);
}

/// Review focus (Task 12): a resize in the middle of a cue (a landing piece, a level cross-fade,
/// a win, the sheet fading in) paints the new size cold, idles, and shows exactly what a session
/// that never painted mid-cue shows at that size: nothing of the cue (the landing piece, the
/// level fade's old frame, the half-faded panel) is left as a ghost.
#[test]
fn a_resize_mid_cue_settles_to_the_cold_picture() {
    type Steps = fn(&mut Session, f64);
    let cues: [(&str, Steps); 4] = [
        ("landing", drop_corner_piece),
        ("level fade", |s, t| {
            cheat_win(s);
            s.render_at(t);
            s.key("=", false, false, false);
            s.render_at(t + 16.0);
        }),
        ("win", |s, t| {
            cheat_win(s);
            s.render_at(t);
        }),
        ("sheet", |s, t| {
            s.command("levels");
            s.render_at(t);
        }),
    ];
    for (name, steps) in cues {
        for (w, h) in [(W - 2, H), (W / 2, H / 2), (W + 400, H + 300)] {
            let mut session = modern(1);
            session.render_at(0.0);
            steps(&mut session, 1000.0);
            let at = session.next_wake(1000.0 + 48.0);
            assert!(at >= 0.0, "{name}: mid-cue");
            session.resize(w, h, 2.0, vec![0.0; 4], false);
            assert_eq!(session.render_at(at), 1, "{name} {w}×{h}: cold, whole");
            assert!(session.captions_changed(), "{name}: captions set anew");
            assert_eq!(session.next_wake(at), -1.0, "{name} {w}×{h}: idle");
            let mut fresh = modern(1);
            // The same input with settled paints: a non-finite `now` starts no cue (halation's
            // `Lightbox::paint_with` and `Choreography::observe`, ruling M7), so nothing ran mid-cue.
            steps(&mut fresh, f64::NAN);
            fresh.resize(w, h, 2.0, vec![0.0; 4], false);
            fresh.render_at(0.0);
            assert_eq!(session.sheet_open(), fresh.sheet_open(), "{name}");
            assert!(
                session.rgba() == fresh.rgba(),
                "{name} {w}×{h}: the cold picture"
            );
        }
    }
}

#[test]
fn set_theme_repaints_the_modern_canvas_cold_in_the_new_theme() {
    let mut session = modern(1);
    session.render_at(0.0);
    let dark = session.rgba().to_vec();
    assert_eq!(session.set_theme(true), 0, "already dark");
    assert!(Flag::Redraw.is_in(session.set_theme(false)));
    assert_eq!(session.render_at(16.0), 1, "a cold paint, whole");
    assert_ne!(session.rgba(), &dark[..], "light pixels");
    assert_eq!(session.next_wake(16.0), -1.0);
    assert!(Flag::Redraw.is_in(session.set_theme(true)));
    session.render_at(32.0);
    assert_eq!(session.rgba(), &dark[..], "dark again, byte for byte");
}

#[test]
fn set_theme_mid_cue_redraws_and_idles() {
    let mut session = modern(1);
    session.render_at(0.0);
    cheat_win(&mut session);
    session.render_at(1000.0);
    assert!(Flag::Redraw.is_in(session.set_theme(false)));
    assert_eq!(session.render_at(1016.0), 1);
    assert_eq!(session.next_wake(1016.0), -1.0);
}

#[test]
fn set_theme_in_classic_is_kept_for_modern() {
    let mut session = started(1);
    assert_eq!(session.set_theme(false), 0, "classic does not repaint");
    session.set_look(Look::Modern);
    session.resize(W, H, 2.0, vec![0.0; 4], false);
    session.render_at(0.0);
    let light = session.rgba().to_vec();
    let mut fresh = modern(1);
    fresh.set_theme(false);
    fresh.render_at(0.0);
    assert_eq!(
        fresh.rgba(),
        &light[..],
        "the lightbox took the stored theme"
    );
}

/// Review T12 M1: with a lightbox already built (modern, dark), a theme change in classic
/// reaches it, and the switch back to modern paints it in that theme: a fresh light lightbox's
/// picture.
#[test]
fn set_theme_in_classic_reaches_an_existing_lightbox() {
    let mut session = modern(1);
    session.render_at(0.0);
    session.set_look(Look::Classic);
    session.render_at(16.0);
    assert_eq!(session.set_theme(false), 0, "classic does not repaint");
    session.set_look(Look::Modern);
    session.render_at(32.0);
    let light = session.rgba().to_vec();
    let mut fresh = modern(1);
    fresh.set_theme(false);
    fresh.render_at(0.0);
    assert_eq!(fresh.rgba(), &light[..], "the lightbox repainted in light");
}

#[test]
fn a_resize_keeps_the_theme() {
    let mut session = modern(1);
    session.set_theme(false);
    session.render_at(0.0);
    let light = session.rgba().to_vec();
    session.resize(W, H, 2.0, vec![0.0; 4], false);
    session.render_at(16.0);
    assert_eq!(session.rgba(), &light[..]);
}

/// Ruling L2: a theme change swaps the lightbox's planes in place, so a piece held mid-drag
/// stays held and drops where it is released.
#[test]
fn a_held_piece_survives_a_theme_change() {
    let mut session = modern(1);
    session.render_at(0.0);
    let (from, to) = (
        centre(&session.cell_rect(toolbox_index(0))),
        centre(&session.cell_rect(board_index(0, 0))),
    );
    let before = session.copy();
    session.pointer_down(from.0, from.1, 0, false, false, false);
    session.pointer_move(to.0, to.1);
    assert!(session.dragging());
    assert!(Flag::Redraw.is_in(session.set_theme(false)));
    assert!(session.dragging(), "still held");
    session.render_at(16.0);
    session.pointer_up(to.0, to.1, 0);
    assert_ne!(session.copy(), before, "dropped on the board");
}
