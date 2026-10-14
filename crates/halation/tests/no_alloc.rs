//! Spec §14: `paint`, `route`, `command`, `next_wake`, `set_reduced_motion` and `handle_on` never
//! allocate (a rotation's patched light and its cues over frames included: the piece turning as
//! vectors, the sweep's partials, its heads and fading strands drawn over the output; a
//! pasted solution's targets bursting; a held piece rising and a dropped or cancelled one
//! landing, with a mouse and on touch; a level change's cross-fade and sweep), nor do the
//! tableau's hit tests, the
//! darkroom's paints, composes and a same-size reshape under them, its light frames (mid-sweep
//! too), the light's develop by patches, `Playbill::build`/`changes`, `Reach::compute`, or
//! `Choreography::observe`/`moment`.
//! One test per binary, counting this thread's allocations only.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use halation::beamline::SweepView;
use halation::captions::MAX_CAPTIONS;
use halation::choreography::{Choreography, Observed};
use halation::darkroom::{Darkroom, Pose};
use halation::playbill::Playbill;
use halation::{Control, DeviceRect, Easel, Lightbox, Output, Reach, Route, Scene, Tableau, Theme};
use lumen::{
    BOARD_CELLS, BeamField, Floorplan, InputEvent, Piece, VISIBLE_TOOLBOX_SLOTS, board_index,
};
use spectrograph::Game;

struct Counting;

thread_local! { static COUNT: Cell<usize> = const { Cell::new(0) }; }

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocations(f: impl FnOnce()) -> usize {
    let before = COUNT.with(Cell::get);
    f();
    COUNT.with(Cell::get) - before
}

#[test]
fn painting_and_input_never_allocate() {
    for game in Game::ALL {
        let mut state = common::state(game, 0, false);
        let easel = Easel::new(1280, 800, 1.0);
        let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
        let mut out = vec![0u32; 1280 * 800];
        let mut bytes = vec![0u8; 1280 * 800 * 4];
        let cheat = state.rules().cheat_key;
        let tableau = lightbox.tableau().clone();
        let pages = halation::specimen::pages();
        // Won (the pasted `String` allocates, so before counting): the first paint shows the won
        // card and Next ▶, and the drag below changes the board under it.
        let solution = lumen::stored_solution(state.data(), 0).expect("a stored solution");
        state.handle(InputEvent::Paste(solution));
        assert!(state.is_won(), "{game}: won");
        // The piece the drag lifts (solved, it stands on the board: a dashed source cell), and
        // two empty board cells it is held over (drop targets), dropped on the second.
        let a = (0..BOARD_CELLS + VISIBLE_TOOLBOX_SLOTS)
            .find(|&i| state.board().cells[i].piece.is_movable())
            .map(|i| tableau.cell_centre(i))
            .expect("a movable piece");
        let mut empty = (0..BOARD_CELLS)
            .filter(|&i| state.board().cells[i].piece == Piece::Empty)
            .map(|i| tableau.cell_centre(i));
        let held = [empty.next().unwrap(), empty.nth(3).unwrap()];
        let b = held[1];
        // The first movable board piece, turned in place (its light framed by patches).
        let turn = (0..BOARD_CELLS)
            .find(|&i| state.board().cells[i].piece.is_movable())
            .expect("a movable board piece");
        let t = tableau.cell_centre(turn);
        let t = (t.x as i32, t.y as i32);
        let mut turned = 0;
        let (mut woke, mut slept) = (false, false);
        let mut swept = 0;
        let (mut headed, mut faded) = (false, false);
        let mut over_targets = 0;
        let mut won_with_next = false;
        let mut captioned = 0;
        // A phone with a sheet picker: its Levels pill, routed, opens the sheet.
        let phone = Easel::new(390, 740, 1.0).with_touch(true);
        let mut pocket = Lightbox::new(phone, Theme::Dark, false);
        let mut small = vec![0u32; 390 * 740];
        let mut small_bytes = vec![0u8; 390 * 740 * 4];
        let pocket_tableau = pocket.tableau().clone();
        let pill = pocket_tableau.device(pocket_tableau.card_parts.levels.expect("a sheet picker"));
        let pill = ((pill.left + pill.right) / 2, (pill.top + pill.bottom) / 2);
        let chip = pocket_tableau.sheet_chip_rect(4);
        let chip = ((chip.left + chip.right) / 2, (chip.top + chip.bottom) / 2);
        let mut routed = None;
        let n = allocations(|| {
            // Reading the captions (lumen's text, borrowed) must not allocate either.
            let painted = lightbox.paint(&state, 0.0, Output::Xrgb(&mut out));
            assert!(painted.captions.as_slice().len() <= MAX_CAPTIONS);
            assert!(
                painted.captions_changed,
                "a cold paint reports its captions"
            );
            captioned += painted
                .captions
                .as_slice()
                .iter()
                .map(|c| c.text.len())
                .sum::<usize>();
            // A rotation, painted to both outputs: the board's light redrawn where it changed.
            for k in 0..2 {
                let before = state.board().cells[turn];
                state.handle_on(
                    &lightbox,
                    InputEvent::MouseDown {
                        x: t.0,
                        y: t.1,
                        alt: true,
                    },
                );
                state.handle_on(
                    &lightbox,
                    InputEvent::MouseUp {
                        x: t.0,
                        y: t.1,
                        alt: true,
                    },
                );
                turned += usize::from(state.board().cells[turn] != before);
                if k == 0 {
                    lightbox.paint(&state, 4.0, Output::Xrgb(&mut out));
                } else {
                    lightbox.paint(&state, 8.0, Output::Rgba(&mut bytes));
                }
            }
            // Two more turns' cues (each turning on, at most a round, until the beams change),
            // painted over frames until they settle: their sweeps' frames (the patched light,
            // the partials, the heads and the fading strands over the output, the develop
            // windows), on both outputs; reduced motion on and off again.
            let (x, y) = t;
            let mut start = 1000.0;
            for _ in 0..2 {
                let before = state.field().clone();
                for _ in 0..8 {
                    for event in [
                        InputEvent::MouseDown { x, y, alt: true },
                        InputEvent::MouseUp { x, y, alt: true },
                    ] {
                        state.handle_on(&lightbox, event);
                    }
                    if *state.field() != before {
                        break;
                    }
                }
                lightbox.paint(&state, start, Output::Xrgb(&mut out));
                woke |= lightbox.next_wake(start).is_some();
                let mut now = start;
                while now < start + 700.0 {
                    now += halation::FRAME_MS;
                    if swept % 2 == 0 {
                        lightbox.paint(&state, now, Output::Xrgb(&mut out));
                    } else {
                        lightbox.paint(&state, now, Output::Rgba(&mut bytes));
                    }
                    swept += 1;
                    let m = lightbox.choreography().moment(now);
                    headed |= m.sweep.is_some();
                    faded |= m.fade > 0.0;
                }
                start += 1000.0;
            }
            for now in [3000.0, 5000.0] {
                lightbox.paint(&state, now, Output::Xrgb(&mut out));
            }
            slept = lightbox.next_wake(5000.0).is_none();
            lightbox.set_reduced_motion(true);
            let _ = lightbox.next_wake(5016.0);
            lightbox.set_reduced_motion(false);
            state.handle_on(
                &lightbox,
                InputEvent::MouseDown {
                    x: a.x as i32,
                    y: a.y as i32,
                    alt: false,
                },
            );
            // Held over two empty cells in turn, each a drop target (both outputs), dropped on
            // the second.
            for (k, e) in held.iter().enumerate() {
                state.handle_on(
                    &lightbox,
                    InputEvent::MouseDragged {
                        x: e.x as i32,
                        y: e.y as i32,
                    },
                );
                over_targets += usize::from(state.drag().is_some());
                if k == 0 {
                    lightbox.paint(&state, 16.0, Output::Xrgb(&mut out));
                } else {
                    lightbox.paint(&state, 24.0, Output::Rgba(&mut bytes));
                }
            }
            state.handle_on(
                &lightbox,
                InputEvent::MouseUp {
                    x: b.x as i32,
                    y: b.y as i32,
                    alt: false,
                },
            );
            lightbox.paint(&state, 32.0, Output::Xrgb(&mut out));
            state.handle_on(&lightbox, InputEvent::Key(b'r'));
            state.handle_on(&lightbox, InputEvent::Key(b'='));
            lightbox.paint(&state, 48.0, Output::Xrgb(&mut out));
            lightbox.invalidate(DeviceRect::new(10, 10, 60, 40));
            lightbox.paint(&state, 1.0e9, Output::Xrgb(&mut out));
            let _ = lightbox.next_wake(64.0);
            lightbox.reset();
            lightbox.paint(&state, 80.0, Output::Xrgb(&mut out));
            // The web's output, and a run of levels so most kinds' art and the sprite pools' refills run.
            lightbox.reset();
            lightbox.paint(&state, 96.0, Output::Rgba(&mut bytes));
            for step in 0..12 {
                state.handle_on(&lightbox, InputEvent::Key(cheat));
                state.handle_on(&lightbox, InputEvent::Key(b'='));
                lightbox.paint(&state, 112.0 + f64::from(step), Output::Rgba(&mut bytes));
            }
            // A win on an incremental paint: the card letters YOU WIN! and Next ▶ again.
            state.handle_on(&lightbox, InputEvent::Key(cheat));
            let painted = lightbox.paint(&state, 130.0, Output::Rgba(&mut bytes));
            assert!(painted.captions.as_slice().len() <= MAX_CAPTIONS);
            won_with_next = Scene::of(&state).next_open();
            // The pieces sheet: page 1 alone overflows the board sprite pool, so the refill runs.
            for page in &pages {
                lightbox.paint_scene(
                    Scene::bare(&page.board, &page.field, page.game),
                    200.0,
                    Output::Rgba(&mut bytes),
                );
            }
            // The canvas's controls and the sheet: hits, a routed press and release on the
            // Levels pill, then the sheet open over a few paints (both outputs) and closed again.
            let _ = (
                pocket_tableau.hit(chip.0, chip.1),
                pocket_tableau.hit_sheet(chip.0, chip.1),
                pocket_tableau.control_at(pill.0, pill.1, false, true),
                pocket_tableau.control_at(0, 0, true, false),
            );
            pocket.paint(&state, 300.0, Output::Xrgb(&mut small));
            let _ = pocket.route(InputEvent::MouseDown {
                x: pill.0,
                y: pill.1,
                alt: false,
            });
            if let Route::Control(control) = pocket.route(InputEvent::MouseUp {
                x: pill.0,
                y: pill.1,
                alt: false,
            }) {
                routed = Some(control);
            }
            pocket.command(Control::Levels);
            let _ = (pocket.hit(chip.0, chip.1), pocket.sheet_open());
            pocket.paint(&state, 316.0, Output::Xrgb(&mut small));
            state.handle_on(&pocket, InputEvent::Key(b'='));
            pocket.paint(&state, 332.0, Output::Xrgb(&mut small));
            // The sheet drawn onto the web's output too: whole, then over a level change.
            pocket.reset();
            pocket.paint(&state, 336.0, Output::Rgba(&mut small_bytes));
            state.handle_on(&pocket, InputEvent::Key(b'='));
            pocket.paint(&state, 340.0, Output::Rgba(&mut small_bytes));
            pocket.command(Control::CloseSheet);
            pocket.paint(&state, 348.0, Output::Rgba(&mut small_bytes));
        });
        assert_eq!(turned, 2, "{game}: the board piece turned twice");
        assert!(
            woke && slept,
            "{game}: the turns' cues ran ({woke}), then settled ({slept})"
        );
        assert!(swept >= 80, "{game}: 2 × 700 ms of sweep frames ({swept})");
        assert!(
            headed && faded,
            "{game}: the turns swept new light in and faded old light out"
        );
        assert_eq!(routed, Some(Control::Levels), "{game}: the pill was routed");
        assert_eq!(
            over_targets, 2,
            "{game}: a piece was held over two drop targets"
        );
        assert_eq!(
            n, 0,
            "{game}: allocations in paint, route, command, handle_on, next_wake or \
             set_reduced_motion"
        );
        assert!(captioned > 0, "{game}: the won card has captions");
        assert!(won_with_next, "{game}: a won card with Next ▶ was painted");

        // A pasted solution (the paste's `String` allocates, so before counting): its frames,
        // at the paste and every frame until idle, on both outputs: its sweep bursts its
        // targets, then its win runs (packets, pulses, the header written on).
        let mut fresh = common::state(game, 0, false);
        let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
        lightbox.paint(&fresh, 0.0, Output::Xrgb(&mut out));
        let solution = lumen::stored_solution(fresh.data(), 0).expect("a stored solution");
        fresh.handle(InputEvent::Paste(solution));
        let (mut burst, mut frames) = (0, 0);
        let (mut packets, mut revealed, mut idle) = (false, false, false);
        let n = allocations(|| {
            lightbox.paint(&fresh, 1000.0, Output::Rgba(&mut bytes));
            burst = lightbox.choreography().bursts().len();
            let mut now = 1000.0;
            // At most 400 frames: a lightbox that never sleeps fails below, not hangs.
            while lightbox.next_wake(now).is_some() && frames < 400 {
                now += halation::FRAME_MS;
                if frames % 2 == 0 {
                    lightbox.paint(&fresh, now, Output::Xrgb(&mut out));
                } else {
                    lightbox.paint(&fresh, now, Output::Rgba(&mut bytes));
                }
                frames += 1;
                if let Some(w) = lightbox.choreography().moment(now).win {
                    packets |= w.packets && w.packet > 1.0;
                    revealed |= w.reveal > 0.0 && w.reveal < 1.0;
                }
            }
            idle = lightbox.next_wake(now).is_none();
        });
        assert!(idle, "{game}: the paste's cues settle within 400 frames");
        assert!(burst > 0, "{game}: the paste lit targets");
        assert!(
            frames >= 90,
            "{game}: its sweep's and win's frames ({frames})"
        );
        assert!(
            packets && revealed,
            "{game}: the win ran its packets and wrote its header on"
        );
        assert_eq!(n, 0, "{game}: allocations in a pasted solution's frames");

        // A cheat win (no beam moves: the packets' reach is worked out in `observe`) and one
        // under reduced motion (the header fades): every frame until idle, on both outputs.
        for reduced in [false, true] {
            let mut cheated = common::state(game, 0, false);
            let mut lightbox = Lightbox::new(easel, Theme::Dark, reduced);
            lightbox.paint(&cheated, 0.0, Output::Xrgb(&mut out));
            let (mut frames, mut idle) = (0, false);
            let n = allocations(|| {
                cheated.handle_on(&lightbox, InputEvent::Key(cheat));
                lightbox.paint(&cheated, 1000.0, Output::Rgba(&mut bytes));
                let mut now = 1000.0;
                while lightbox.next_wake(now).is_some() && frames < 400 {
                    now += halation::FRAME_MS;
                    if frames % 2 == 0 {
                        lightbox.paint(&cheated, now, Output::Xrgb(&mut out));
                    } else {
                        lightbox.paint(&cheated, now, Output::Rgba(&mut bytes));
                    }
                    frames += 1;
                }
                idle = lightbox.next_wake(now).is_none();
            });
            assert!(
                idle,
                "{game} reduced {reduced}: the win settles within 400 frames"
            );
            let least = if reduced { 8 } else { 80 };
            assert!(
                frames >= least,
                "{game} reduced {reduced}: the win's frames ({frames})"
            );
            assert_eq!(
                n, 0,
                "{game} reduced {reduced}: allocations in a win's frames"
            );
        }

        // A level change (the cheat key, then `=`): the board cross-fades from the frame shown
        // (kept from either output), then its beams sweep out from the lasers; under reduced
        // motion a 150 ms fade alone. Every frame until idle, on both outputs.
        for reduced in [false, true] {
            let mut moving = common::state(game, 0, false);
            let mut lightbox = Lightbox::new(easel, Theme::Dark, reduced);
            lightbox.paint(&moving, 0.0, Output::Xrgb(&mut out));
            let (mut frames, mut faded, mut swept, mut idle) = (0, 0, false, false);
            let n = allocations(|| {
                moving.handle_on(&lightbox, InputEvent::Key(cheat));
                moving.handle_on(&lightbox, InputEvent::Key(b'='));
                if reduced {
                    lightbox.paint(&moving, 1000.0, Output::Xrgb(&mut out));
                } else {
                    lightbox.paint(&moving, 1000.0, Output::Rgba(&mut bytes));
                }
                let mut now = 1000.0;
                while lightbox.next_wake(now).is_some() && frames < 400 {
                    now += halation::FRAME_MS;
                    if frames % 2 == 0 {
                        lightbox.paint(&moving, now, Output::Xrgb(&mut out));
                    } else {
                        lightbox.paint(&moving, now, Output::Rgba(&mut bytes));
                    }
                    frames += 1;
                    let m = lightbox.choreography().moment(now);
                    faded += usize::from(m.level.is_some());
                    swept |= m.sweep.is_some();
                }
                idle = lightbox.next_wake(now).is_none();
            });
            assert_eq!(moving.slot(), 1, "{game}: on to level 2");
            assert!(
                idle,
                "{game} reduced {reduced}: the level change settles within 400 frames"
            );
            assert!(
                faded >= 8 && swept == !reduced,
                "{game} reduced {reduced}: {faded} fade frames, swept {swept}"
            );
            assert_eq!(
                n, 0,
                "{game} reduced {reduced}: allocations in a level change's frames"
            );
        }

        // The level sheet on a phone, opened then closed: every frame of both cues (the panel
        // fading and rising in, then fading and sinking out) until idle, on both outputs; under
        // reduced motion a 150 ms fade each way.
        for reduced in [false, true] {
            let phone = Easel::new(780, 1480, 2.0).with_touch(true);
            let size = (phone.width * phone.height) as usize;
            let (mut px, mut rgba) = (vec![0u32; size], vec![0u8; size * 4]);
            let shown = common::state(game, 0, false);
            let mut lightbox = Lightbox::new(phone, Theme::Dark, reduced);
            lightbox.paint(&shown, 0.0, Output::Xrgb(&mut px));
            let (mut frames, mut posed, mut idle) = (0, 0, true);
            let n = allocations(|| {
                let mut now = 1000.0;
                for control in [Control::Levels, Control::CloseSheet] {
                    lightbox.command(control);
                    lightbox.paint(&shown, now, Output::Rgba(&mut rgba));
                    let mut ran = 0;
                    while lightbox.next_wake(now).is_some() && ran < 400 {
                        ran += 1;
                        now += halation::FRAME_MS;
                        if frames % 2 == 0 {
                            lightbox.paint(&shown, now, Output::Xrgb(&mut px));
                        } else {
                            lightbox.paint(&shown, now, Output::Rgba(&mut rgba));
                        }
                        frames += 1;
                        posed += usize::from(lightbox.choreography().moment(now).sheet.is_some());
                    }
                    idle &= lightbox.next_wake(now).is_none();
                    now += 1000.0;
                }
            });
            assert!(
                idle,
                "{game} reduced {reduced}: the sheet's cues settle within 400 frames"
            );
            let least = if reduced { 16 } else { 22 };
            assert!(
                posed >= least,
                "{game} reduced {reduced}: {posed} sheet frames mid-fade"
            );
            assert_eq!(
                n, 0,
                "{game} reduced {reduced}: allocations in the sheet's frames"
            );
        }

        // A tray piece lifted and held (its rise), dropped on an empty board cell (its land),
        // then picked up again and cancelled (a land back to its source): every frame, with a
        // mouse and on a touch phone, on both outputs.
        for easel in [easel, Easel::new(780, 1480, 2.0).with_touch(true)] {
            let size = (easel.width * easel.height) as usize;
            let (mut px, mut rgba) = (vec![0u32; size], vec![0u8; size * 4]);
            let mut held = common::state(game, 0, false);
            let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
            lightbox.paint(&held, 0.0, Output::Xrgb(&mut px));
            let t = lightbox.tableau().clone();
            let at = |i: usize| {
                let c = t.cell_centre(i);
                (c.x as i32, c.y as i32)
            };
            let from = (BOARD_CELLS..BOARD_CELLS + VISIBLE_TOOLBOX_SLOTS)
                .find(|&i| held.board().cells[i].piece.is_movable())
                .map(at)
                .expect("a tray piece");
            let to = (0..BOARD_CELLS)
                .find(|&i| held.board().cells[i].piece == Piece::Empty)
                .map(at)
                .expect("an empty cell");
            let (mut frames, mut rose, mut landed) = (0, 0, 0);
            let mut asleep = true;
            let n = allocations(|| {
                let mut now = 1000.0;
                // Every frame until the cues settle, alternating the outputs.
                let mut run = |lightbox: &mut Lightbox,
                               held: &lumen::GameState,
                               now: &mut f64,
                               px: &mut [u32],
                               rgba: &mut [u8]| {
                    // At most 400 frames: a lightbox that never sleeps fails below, not hangs.
                    let mut ran = 0;
                    while lightbox.next_wake(*now).is_some() && ran < 400 {
                        ran += 1;
                        *now += halation::FRAME_MS;
                        if frames % 2 == 0 {
                            lightbox.paint(held, *now, Output::Xrgb(px));
                        } else {
                            lightbox.paint(held, *now, Output::Rgba(rgba));
                        }
                        frames += 1;
                    }
                    asleep &= lightbox.next_wake(*now).is_none();
                    *now += 1000.0;
                };
                for (press, cancel) in [(from, false), (to, true)] {
                    let (x, y) = press;
                    held.handle_on(&lightbox, InputEvent::MouseDown { x, y, alt: false });
                    let (x, y) = if cancel { from } else { (to.0 + 3, to.1 + 2) };
                    held.handle_on(&lightbox, InputEvent::MouseDragged { x, y });
                    lightbox.paint(&held, now, Output::Xrgb(&mut px));
                    rose += usize::from(lightbox.choreography().moment(now).lift < 1.0);
                    run(&mut lightbox, &held, &mut now, &mut px, &mut rgba);
                    let event = if cancel {
                        InputEvent::MouseDown {
                            x: 1,
                            y: 1,
                            alt: false,
                        }
                    } else {
                        InputEvent::MouseUp { x, y, alt: false }
                    };
                    held.handle_on(&lightbox, event);
                    lightbox.paint(&held, now, Output::Rgba(&mut rgba));
                    landed += usize::from(lightbox.choreography().moment(now).land.is_some());
                    run(&mut lightbox, &held, &mut now, &mut px, &mut rgba);
                }
            });
            assert!(
                asleep,
                "{game} touch {}: each lift and land settles within 400 frames",
                easel.touch
            );
            assert_eq!(
                (rose, landed),
                (2, 2),
                "{game} touch {}: two lifts and two lands",
                easel.touch
            );
            assert!(
                frames >= 20,
                "{game}: the lifts' and lands' frames ({frames})"
            );
            assert_eq!(
                n, 0,
                "{game} touch {}: allocations in a lift's or a land's frames",
                easel.touch
            );
        }

        // The darkroom on its own: every paint, both outputs, and a same-size reshape.
        let tableau = Tableau::arrange(&easel);
        let palette = Theme::Dark.palette();
        let mut room = Darkroom::new(&tableau, palette);
        let scene = Scene::of(&state);
        let canvas = tableau.canvas();
        let mut stale = [false; lumen::BOARD_CELLS];
        (stale[0], stale[17], stale[224]) = (true, true, true);
        // The light developed by patches: one cell (a corner), then 24 spread over the board
        // (past `LIGHT_PATCHES`, so the windows merge).
        let corner = [tableau.cell_rect(0)];
        let spread: [DeviceRect; 24] = std::array::from_fn(|k| tableau.cell_rect(k * 9 + k % 7));
        let mut patched = 0;
        // A board edit: a white laser set down mid-board, the beams traced again.
        let mut edited_board = state.board().clone();
        edited_board.cells[board_index(7, 7)] = lumen::Cell {
            piece: Piece::Laser,
            colour: 7,
            dir: 2,
        };
        let mut edited_field = BeamField::default();
        lumen::trace(&edited_board, &mut edited_field);
        let edited = Scene {
            board: &edited_board,
            field: &edited_field,
            ..scene
        };
        let mut framed = 0;
        let edit_reach = Reach::compute(&edited_board, &edited_field);
        let n = allocations(|| {
            room.paint_all(&tableau, palette, &scene);
            room.compose(canvas, &mut Output::Xrgb(&mut out));
            room.paint_board(&tableau, palette, &scene);
            room.compose(room.board_damage(), &mut Output::Xrgb(&mut out));
            room.paint_cells(&tableau, palette, &scene, &stale);
            room.compose(room.board_damage(), &mut Output::Xrgb(&mut out));
            room.paint_tray(&tableau, palette, &scene);
            room.compose(room.tray_damage(), &mut Output::Rgba(&mut bytes));
            room.paint_picker(&tableau, palette, &scene);
            room.compose(room.picker_damage(), &mut Output::Xrgb(&mut out));
            room.paint_card(&tableau, palette, &scene);
            room.compose(room.card_damage(), &mut Output::Rgba(&mut bytes));
            room.reshape(&tableau, palette);
            room.paint_all(&tableau, palette, &scene);
            room.compose(canvas, &mut Output::Rgba(&mut bytes));
            for dirty in [&corner[..], &spread[..]] {
                let device = room.develop_light(Some(dirty));
                patched += device.as_slice().len();
                for &rect in device.as_slice() {
                    room.compose(rect, &mut Output::Xrgb(&mut out));
                }
            }
            room.develop_light(None);
            // The board's light framed: whole, then (after a board edit) by patches.
            room.light_frame(&tableau, palette, &scene, None, true);
            framed += room
                .light_frame(&tableau, palette, &edited, None, false)
                .len();
            // The edit swept in from the board before it: whole at its first frame, then a
            // frame further on by patches.
            let sweep = |front| SweepView {
                old: scene.field,
                reach: &edit_reach,
                front,
            };
            room.paint_all_at(&tableau, palette, &edited, Some(&sweep(2.0)));
            framed += room
                .light_frame(&tableau, palette, &edited, Some(&sweep(4.5)), false)
                .len();
        });
        assert_eq!(
            n, 0,
            "{game}: allocations in the darkroom's paints, composes, reshape, light_frame or \
             develop_patches"
        );
        assert!(patched > 0, "{game}: patched develops reported damage");
        assert!(framed > 0, "{game}: the edit's light frame reported damage");

        // Two frames' playbills, built and compared.
        let (mut bill, last) = (Playbill::new(), Playbill::new());
        let mut changes = 0;
        let n = allocations(|| {
            bill.build(&scene, None, &Pose::SETTLED);
            bill.changes(&last, |_| changes += 1);
        });
        assert_eq!(n, 0, "{game}: allocations in Playbill::build or changes");
        assert!(changes > 0, "{game}: a lit board against an empty one");

        // The beams' reach (spec §9.2), on the board the run above left.
        let mut reach = Reach::default();
        let n = allocations(|| reach = Reach::compute(state.board(), state.field()));
        assert_eq!(n, 0, "{game}: allocations in Reach::compute");
        assert!(reach.cells.iter().any(|&r| r > 0), "{game}: light reached");

        // The choreography on its own: two traced boards observed (an empty one, then the edit:
        // a sweep, its reach and bursts), sampled, woken, and observed again in reduced motion.
        let mut choreography = Choreography::new();
        let (empty, dark) = (lumen::Board::new(game), BeamField::default());
        let seen = Observed {
            board: [&empty, &edited_board],
            field: [&dark, &edited_field],
            drag: [None, None],
            slot: [0, 0],
            won: [false, true],
            sheet: [false, true],
        };
        let mut sampled = None;
        let n = allocations(|| {
            choreography.observe(&seen, 1000.0);
            sampled = Some(choreography.moment(1100.0).sweep.is_some());
            let _ = choreography.next_wake(1100.0);
            choreography.set_reduced(true);
            choreography.observe(&seen, 1200.0);
            let _ = choreography.moment(1250.0);
            choreography.set_reduced(false);
        });
        assert_eq!(
            n, 0,
            "{game}: allocations in Choreography::observe or moment"
        );
        assert_eq!(sampled, Some(true), "{game}: the edit swept");
    }
}
