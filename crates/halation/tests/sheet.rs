//! The canvas's controls and the level sheet (spec §5.2, §6, §7.5; Review Focus 2 and 5).

mod common;

use common::{SETTLED, cold, cold_with, state};
use halation::{Control, Easel, Lightbox, Output, Route, Scene, Theme};
use lumen::{Floorplan, Hit, InputEvent, board_index};
use spectrograph::Game;

fn phone() -> Easel {
    Easel::new(780, 1480, 2.0).with_touch(true) // 390 × 740: Stacked, a sheet picker
}

fn centre(r: halation::DeviceRect) -> (i32, i32) {
    ((r.left + r.right) / 2, (r.top + r.bottom) / 2)
}

fn press(lightbox: &mut Lightbox, (x, y): (i32, i32), alt: bool) -> Route {
    lightbox.route(InputEvent::MouseDown { x, y, alt })
}

fn release(lightbox: &mut Lightbox, (x, y): (i32, i32), alt: bool) -> Route {
    lightbox.route(InputEvent::MouseUp { x, y, alt })
}

#[test]
fn a_press_and_release_on_a_control_is_that_control() {
    let mut lightbox = Lightbox::new(phone(), Theme::Dark, false);
    let t = lightbox.tableau().clone();
    let pill = centre(t.device(t.card_parts.levels.unwrap()));
    assert!(matches!(press(&mut lightbox, pill, false), Route::Consumed));
    assert!(matches!(
        lightbox.route(InputEvent::MouseDragged {
            x: pill.0 + 3,
            y: pill.1
        }),
        Route::Consumed
    ));
    assert!(matches!(
        release(&mut lightbox, pill, false),
        Route::Control(Control::Levels)
    ));
    // Dragged off before the release: nothing fires.
    press(&mut lightbox, pill, false);
    assert!(matches!(
        release(&mut lightbox, (5, 5), false),
        Route::Consumed
    ));
    // A cancelled pointer forgets its press.
    press(&mut lightbox, pill, false);
    lightbox.forget_press();
    assert!(matches!(
        release(&mut lightbox, pill, false),
        Route::Game(_)
    ));
}

#[test]
fn an_alternate_release_over_a_chip_is_primary() {
    let mut lightbox = Lightbox::new(Easel::new(1280, 800, 1.0), Theme::Dark, false);
    let chip = centre(lightbox.tableau().chip_rect(3));
    assert!(matches!(
        press(&mut lightbox, chip, true),
        Route::Game(InputEvent::MouseDown { alt: true, .. })
    ));
    match release(&mut lightbox, chip, true) {
        Route::Game(InputEvent::MouseUp { alt, .. }) => {
            assert!(!alt, "lumen ignores alternate strip releases")
        }
        other => panic!("{other:?}"),
    }
    let cell = lightbox.tableau().cell_centre(board_index(7, 7));
    match release(&mut lightbox, (cell.x as i32, cell.y as i32), true) {
        Route::Game(InputEvent::MouseUp { alt, .. }) => {
            assert!(alt, "a board release keeps its button")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn opening_the_sheet_ends_nothing_but_hides_the_board_from_hits() {
    let mut lightbox = Lightbox::new(phone(), Theme::Dark, false);
    let t = lightbox.tableau().clone();
    let cell = t.cell_rect(board_index(7, 7));
    assert_eq!(
        lightbox.hit(cell.left, cell.top),
        Hit::Cell(board_index(7, 7))
    );
    assert!(lightbox.command(Control::Levels) && lightbox.sheet_open());
    assert!(!lightbox.has_inline_picker());
    let (x, y) = centre(t.sheet_chip_rect(4));
    assert_eq!(lightbox.hit(x, y), Hit::Strip(4));
    let corner = (
        t.device(t.sheet.panel).left - 4,
        t.device(t.sheet.panel).top - 4,
    );
    assert_eq!(
        lightbox.hit(corner.0, corner.1),
        Hit::Nothing,
        "under the scrim"
    );
    press(&mut lightbox, corner, false);
    assert!(matches!(
        release(&mut lightbox, corner, false),
        Route::Control(Control::CloseSheet)
    ));
    assert!(lightbox.command(Control::CloseSheet) && !lightbox.sheet_open());
    assert!(!lightbox.command(Control::CloseSheet), "already shut");
}

#[test]
fn choosing_an_open_chip_opens_its_level_and_closes_the_sheet() {
    let mut game = state(Game::C2, 0, false);
    let mut lightbox = Lightbox::new(phone(), Theme::Dark, false);
    let mut out = vec![0u32; 780 * 1480];
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    lightbox.command(Control::Levels);
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    let t = lightbox.tableau().clone();
    // C2 opens slots 0–4 on a fresh save; slot 9 is locked.
    for (slot, opens) in [(9, false), (3, true)] {
        let at = centre(t.sheet_chip_rect(slot));
        for event in [
            press(&mut lightbox, at, false),
            release(&mut lightbox, at, false),
        ] {
            if let Route::Game(e) = event {
                game.handle_on(&lightbox, e);
            }
        }
        lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
        assert_eq!(game.slot() == slot, opens, "slot {slot}");
        assert_eq!(
            lightbox.sheet_open(),
            !opens,
            "slot {slot}: a locked chip keeps the sheet"
        );
    }
    assert!(out == cold(phone(), Scene::of(&game)));
}

#[test]
fn growing_into_an_inline_picker_closes_the_sheet() {
    let mut lightbox = Lightbox::new(phone(), Theme::Dark, false);
    lightbox.command(Control::Levels);
    lightbox.resize(Easel::new(2560, 1600, 2.0));
    assert!(lightbox.has_inline_picker() && !lightbox.sheet_open());
    assert!(
        !lightbox.command(Control::Levels),
        "an inline picker has no sheet"
    );
}

/// Spec §7.5: the card's captions hide while the sheet covers it, and come back when it shuts;
/// both changes are reported.
#[test]
fn the_sheet_hides_the_captions_while_open() {
    let game = state(Game::C2, 3, false);
    let mut lightbox = Lightbox::new(phone(), Theme::Dark, false);
    let mut out = vec![0u32; 780 * 1480];
    let first = lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    assert!(
        !first.captions.as_slice().is_empty(),
        "a card with captions"
    );
    assert!(lightbox.command(Control::Levels));
    let open = lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert!(
        open.captions_changed,
        "opening the sheet reports its captions"
    );
    assert!(
        open.captions.as_slice().is_empty(),
        "hidden under the sheet"
    );
    let again = lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert!(!again.captions_changed && again.captions.as_slice().is_empty());
    assert!(lightbox.command(Control::CloseSheet));
    let shut = lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert!(shut.captions_changed, "closing it reports them again");
    assert!(!shut.captions.as_slice().is_empty());
}

/// Spec §6: with the sheet open nothing on the board or tray answers, and a press on the pill
/// position is the scrim's.
#[test]
fn the_sheet_open_covers_the_card_controls() {
    let mut lightbox = Lightbox::new(phone(), Theme::Dark, false);
    let t = lightbox.tableau().clone();
    lightbox.command(Control::Levels);
    for index in [
        board_index(0, 0),
        board_index(14, 14),
        lumen::toolbox_index(0),
    ] {
        let (x, y) = centre(t.cell_rect(index));
        if !within(t.device(t.sheet.panel), x, y) {
            assert_eq!(lightbox.hit(x, y), Hit::Nothing, "cell {index}");
        }
    }
    let pill = centre(t.device(t.card_parts.levels.unwrap()));
    press(&mut lightbox, pill, false);
    let routed = release(&mut lightbox, pill, false);
    assert!(
        !matches!(routed, Route::Control(Control::Levels)),
        "{routed:?}"
    );
    if !within(t.device(t.sheet.panel), pill.0, pill.1) {
        assert!(matches!(routed, Route::Control(Control::CloseSheet)));
    }
}

/// A press on a control that a resize drops swallows its release: lumen acts on a release with
/// no press, so an orphan over a movable piece would turn it.
#[test]
fn a_resize_mid_press_swallows_the_release() {
    // Solved, so its movable pieces sit on the board.
    let mut game = state(Game::C1, 0, true);
    let cell = (0..lumen::BOARD_CELLS)
        .find(|&i| game.board().cells[i].piece.is_movable())
        .expect("a movable piece on the board");
    let mut lightbox = Lightbox::new(
        Easel::new(390, 740, 1.0).with_touch(true),
        Theme::Dark,
        false,
    );
    let t = lightbox.tableau().clone();
    let pill = centre(t.device(t.card_parts.levels.unwrap()));
    assert!(matches!(press(&mut lightbox, pill, false), Route::Consumed));
    lightbox.resize(Easel::new(375, 667, 1.0).with_touch(true));
    let at = lightbox.tableau().cell_centre(cell);
    let at = (at.x as i32, at.y as i32);
    assert_eq!(lightbox.hit(at.0, at.1), Hit::Cell(cell), "over the piece");
    let before = game.board().cells;
    let drag = lightbox.route(InputEvent::MouseDragged { x: at.0, y: at.1 });
    let up = release(&mut lightbox, at, false);
    let consumed = [&drag, &up].map(|r| matches!(r, Route::Consumed));
    for routed in [drag, up] {
        if let Route::Game(e) = routed {
            game.handle_on(&lightbox, e);
        }
    }
    assert!(game.board().cells == before, "the piece did not turn");
    assert_eq!(consumed, [true, true], "the move and release are swallowed");
    // Only that one release is swallowed: the next tap on the piece reaches the game.
    press(&mut lightbox, at, false);
    assert!(matches!(release(&mut lightbox, at, false), Route::Game(_)));
}

fn within(r: halation::DeviceRect, x: i32, y: i32) -> bool {
    (r.left..r.right).contains(&x) && (r.top..r.bottom).contains(&y)
}

#[test]
fn keys_under_the_sheet_update_its_chips_and_keep_it_open() {
    let mut game = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(phone(), Theme::Dark, false);
    let mut out = vec![0u32; 780 * 1480];
    lightbox.paint(&game, 0.0, Output::Xrgb(&mut out));
    lightbox.command(Control::Levels);
    let opened = lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert_eq!(opened.damage.len(), 1, "the scrim covers the canvas");
    assert!(
        opened.captions.as_slice().is_empty() && opened.captions_changed,
        "captions hide"
    );
    game.handle(InputEvent::Key(game.rules().cheat_key));
    game.handle(InputEvent::Key(b'='));
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert!(lightbox.sheet_open());
    assert!(
        out == cold_with(phone(), Scene::of(&game), true),
        "the sheet and its chips follow the level"
    );
    let far = out[10 * 780 + 10];
    lightbox.command(Control::CloseSheet);
    lightbox.paint(&game, SETTLED, Output::Xrgb(&mut out));
    assert!(
        (out[10 * 780 + 10] & 0xff) >= (far & 0xff),
        "the scrim lifts"
    );
    assert!(out == cold(phone(), Scene::of(&game)));
}
