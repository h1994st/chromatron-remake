//! Image snapshots of the modern look (spec §15), derived from the originals, so local only
//! (git-ignored): the first run records them (`INSTA_UPDATE=always`), later runs fail on any
//! change (`cargo insta review`). The dark theme, settled: 1280 × 800 pt, the first levels in the
//! touch arrangements, and the won first level and the level sheet in portrait.

mod common;

use common::{png_bytes, state};
use halation::{Control, Easel, Lightbox, Output, Scene, Theme, specimen};
use lumen::{GameState, InputEvent, Piece};
use spectrograph::Game;

/// One canvas of the reference 1280 × 800 pt at `scale`: @2 for the pieces sheet and each game's
/// first, densest and solved levels; @1 for the per-kind levels, which keeps the local snapshots
/// to tens of MB, not hundreds.
fn still(scene: Scene, scale: f32) -> Vec<u8> {
    let easel = Easel::new((1280.0 * scale) as u32, (800.0 * scale) as u32, scale);
    let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
    let mut px = vec![0u32; (easel.width * easel.height) as usize];
    lightbox.paint_scene(scene, 0.0, Output::Xrgb(&mut px));
    png_bytes(easel.width, easel.height, &px)
}

#[test]
fn pieces_sheet() {
    for (k, page) in specimen::pages().iter().enumerate() {
        let scene = Scene::bare(&page.board, &page.field, page.game);
        insta::assert_binary_snapshot!(&format!("pieces_{}.png", k + 1), still(scene, 2.0));
    }
}

/// The first level, the densest level, a solved level, and the first level with each piece kind.
fn levels(game: Game) {
    let data = common::data(game);
    let slots = data.level_order.len().min(50);
    let pieces = |slot: usize| data.slot_level(slot).unwrap().records.len();
    let densest = (0..slots).max_by_key(|&s| pieces(s)).unwrap();
    let mut picks = vec![
        ("first".to_owned(), 0, false, 2.0),
        ("densest".to_owned(), densest, false, 2.0),
        ("solved".to_owned(), 0, true, 2.0),
    ];
    let rules = lumen::RuleSet::of(game);
    for kind in Piece::ALL {
        let first = (0..slots).find(|&s| {
            data.slot_level(s)
                .unwrap()
                .records
                .iter()
                .any(|r| rules.piece(r.kind) == Some(kind))
        });
        if let Some(slot) = first.filter(|&s| s != 0 && s != densest) {
            picks.push((format!("{kind:?}").to_lowercase(), slot, false, 1.0));
        }
    }
    for (name, slot, solved, scale) in picks {
        let game_state = state(game, slot, solved);
        let shot = still(Scene::of(&game_state), scale);
        insta::assert_binary_snapshot!(
            &format!("c{}_{name}_slot{:02}.png", game.number(), slot + 1),
            shot
        );
    }
}

/// The first level of each game in the touch layouts' arrangements: Stacked (390 × 740 @2) and
/// compact Beside (700 × 370 @2).
#[test]
fn touch_arrangements() {
    for game in Game::ALL {
        let state = state(game, 0, false);
        for (w, h) in [(390.0, 740.0), (700.0, 370.0)] {
            let easel = Easel::new((w * 2.0) as u32, (h * 2.0) as u32, 2.0).with_touch(true);
            let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
            let mut px = vec![0u32; (easel.width * easel.height) as usize];
            lightbox.paint(&state, 0.0, Output::Xrgb(&mut px));
            insta::assert_binary_snapshot!(
                &format!("c{}_first_{w}x{h}@2.png", game.number()),
                png_bytes(easel.width, easel.height, &px)
            );
        }
    }
}

/// A short landscape (spec §6 "Revision: short landscape"): an iPhone 12–14's canvas with
/// Safari's bars (741 × 321 @3), C1's first level, and won (Next ▶ on the compact card).
#[test]
fn short_landscape() {
    let easel = Easel::new(2223, 963, 3.0).with_touch(true);
    for won in [false, true] {
        let state = state(Game::C1, 0, won);
        let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
        let mut px = vec![0u32; (easel.width * easel.height) as usize];
        lightbox.paint(&state, 0.0, Output::Xrgb(&mut px));
        let name = if won { "won" } else { "first" };
        insta::assert_binary_snapshot!(
            &format!("c1_{name}_741x321@3.png"),
            png_bytes(easel.width, easel.height, &px)
        );
    }
}

/// Each game's first level won with its stored solution in the portrait touch arrangement
/// (390 × 740 @2), then the level sheet open there on the second level, so its chips show every
/// state: solved (the first), current, open and locked. The won first level at 1280 × 800 @2 is
/// `levels`' `solved` still.
#[test]
fn won_and_sheet() {
    let easel = Easel::new(780, 1480, 2.0).with_touch(true);
    for game in Game::ALL {
        for sheet in [false, true] {
            // Each picture from its own state, so the order of the two never matters.
            let mut state = state(game, 0, true);
            let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
            if sheet {
                state.goto_slot(1);
                assert!(lightbox.command(Control::Levels), "portrait has a sheet");
            }
            let mut px = vec![0u32; (easel.width * easel.height) as usize];
            lightbox.paint(&state, 0.0, Output::Xrgb(&mut px));
            let name = if sheet { "sheet" } else { "won" };
            insta::assert_binary_snapshot!(
                &format!("c{}_{name}_390x740@2.png", game.number()),
                png_bytes(easel.width, easel.height, &px)
            );
        }
    }
}

/// Mid-animation frames (spec §15), at fixed times after their cue, at 1280 × 800 @1 (the sheet
/// on a 390 × 740 @2 phone): a turn of the solved level's first piece, off the solution, at 50
/// and 150 ms (the piece turning, its strand fading, no new strand); a cheat-key win's packets
/// and reveal at 120 and 500 ms (no target lit, so no burst or pulse); a level's cross-fade at
/// 90 ms and its sweep at 400 ms; the sheet rising at 100 ms. A sweep's heads and a win's pulse
/// are `motion_frames_won_by_a_turn`'s.
#[test]
fn motion_frames() {
    let desk = Easel::new(1280, 800, 1.0);
    let phone = Easel::new(780, 1480, 2.0).with_touch(true);
    let shoot = |easel: Easel,
                 setup: &dyn Fn(&mut GameState<'static>, &mut Lightbox),
                 ats: &[f64],
                 name: &str| {
        for &at in ats {
            let mut st = state(Game::C1, 0, name == "turn");
            let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
            let mut px = vec![0u32; (easel.width * easel.height) as usize];
            lightbox.paint(&st, 0.0, Output::Xrgb(&mut px));
            setup(&mut st, &mut lightbox);
            lightbox.paint(&st, 1000.0, Output::Xrgb(&mut px));
            lightbox.paint(&st, 1000.0 + at, Output::Xrgb(&mut px));
            insta::assert_binary_snapshot!(
                &format!("c1_{name}_{at}ms.png"),
                png_bytes(easel.width, easel.height, &px)
            );
        }
    };
    let turn = |st: &mut GameState<'static>, lb: &mut Lightbox| {
        let i = (0..lumen::BOARD_CELLS)
            .find(|&i| st.board().cells[i].piece.is_movable())
            .unwrap();
        let c = lb.tableau().cell_centre(i);
        let (x, y) = (c.x as i32, c.y as i32);
        st.handle_on(lb, lumen::InputEvent::MouseDown { x, y, alt: true });
        st.handle_on(lb, lumen::InputEvent::MouseUp { x, y, alt: true });
    };
    let win = |st: &mut GameState<'static>, _: &mut Lightbox| {
        st.handle(lumen::InputEvent::Key(st.rules().cheat_key));
    };
    let level = |st: &mut GameState<'static>, _: &mut Lightbox| {
        st.handle(lumen::InputEvent::Key(st.rules().cheat_key));
        st.handle(lumen::InputEvent::Key(b'='));
    };
    let sheet =
        |_: &mut GameState<'static>, lb: &mut Lightbox| assert!(lb.command(Control::Levels));
    shoot(desk, &turn, &[50.0, 150.0], "turn");
    shoot(desk, &win, &[120.0, 500.0], "win");
    shoot(desk, &level, &[90.0, 400.0], "level");
    shoot(phone, &sheet, &[100.0], "sheet");
}

/// C1's first level solved, a piece turned off the solution (settled), then turned back: it
/// wins by lighting its target. At 120 ms the sweep's heads run out from the piece; at 630 ms the
/// win's packet has reached the target, which pulses near its height (its burst's sparks still
/// fly).
#[test]
fn motion_frames_won_by_a_turn() {
    let easel = Easel::new(1280, 800, 1.0);
    for at in [120.0, 630.0] {
        let mut st = state(Game::C1, 0, true);
        let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
        let mut px = vec![0u32; (easel.width * easel.height) as usize];
        lightbox.paint(&st, 0.0, Output::Xrgb(&mut px));
        // The first piece whose clockwise turn unwins (one that keeps the win is turned back).
        let (x, y) = (0..lumen::BOARD_CELLS)
            .find_map(|i| {
                if !st.board().cells[i].piece.is_movable() {
                    return None;
                }
                let c = lightbox.tableau().cell_centre(i);
                let (x, y) = (c.x as i32, c.y as i32);
                for alt in [true, false] {
                    st.handle_on(&lightbox, InputEvent::MouseDown { x, y, alt });
                    st.handle_on(&lightbox, InputEvent::MouseUp { x, y, alt });
                    if !st.is_won() {
                        return Some((x, y));
                    }
                }
                None
            })
            .expect("a turn that unwins");
        lightbox.paint(&st, f64::INFINITY, Output::Xrgb(&mut px));
        st.handle_on(&lightbox, InputEvent::MouseDown { x, y, alt: false });
        st.handle_on(&lightbox, InputEvent::MouseUp { x, y, alt: false });
        assert!(st.is_won(), "turned back, it wins");
        lightbox.paint(&st, 1000.0, Output::Xrgb(&mut px));
        lightbox.paint(&st, 1000.0 + at, Output::Xrgb(&mut px));
        let c = lightbox.choreography();
        let m = c.moment(1000.0 + at);
        if at < 300.0 {
            let sweep = m.sweep.expect("mid-sweep");
            let mut heads = 0;
            halation::beamline::for_each_head(&sweep, st.board(), st.field(), |_, _, _, _| {
                heads += 1
            });
            assert!(heads > 0, "heads show at {at} ms");
        } else {
            let since = m.win.expect("the win runs").since_ms;
            let pulsing = (0..lumen::BOARD_CELLS).any(|i| {
                let cell = st.board().cells[i];
                let arrive = f32::from(c.reach().cells[i]) * 1000.0 / 60.0;
                cell.piece == Piece::Pinwheel
                    && cell.colour != 0
                    && (0.25..0.75).contains(&((since - arrive) / 400.0))
            });
            assert!(pulsing, "a target pulses near its height at {at} ms");
        }
        insta::assert_binary_snapshot!(
            &format!("c1_won_by_a_turn_{at}ms.png"),
            png_bytes(easel.width, easel.height, &px)
        );
    }
}

#[test]
fn c1_levels() {
    levels(Game::C1)
}
#[test]
fn c2_levels() {
    levels(Game::C2)
}
#[test]
fn c3_levels() {
    levels(Game::C3)
}
#[test]
fn c4_levels() {
    levels(Game::C4)
}
