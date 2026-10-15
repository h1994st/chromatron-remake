//! The "dashed beam, while rotating the right item" bug (C1 level 27, modern look): a half
//! segment that new light crosses both ways (into its cell, and back out of it) was drawn by a
//! sweep only from the outbound strand's start, so while the front was past the inbound light
//! but short of the outbound, every such half was dark and the other half of its segment lit:
//! evenly spaced dashes, one per cell, until the outbound front arrived. Through the lightbox,
//! in both themes. Needs the originals.

mod common;

use common::{Clock, state};
use halation::beamline::{half_colour, strand_start};
use halation::{Easel, Lightbox, Output, Theme};
use lumen::{BOARD_CELLS, BeamField, DX, DY, GameState, InputEvent, board_index};
use spectrograph::Game;

fn click(game: &mut GameState, lightbox: &Lightbox, cell: usize) {
    let c = lightbox.tableau().cell_centre(cell);
    let (x, y) = (c.x as i32, c.y as i32);
    game.handle_on(lightbox, InputEvent::MouseDown { x, y, alt: false });
    game.handle_on(lightbox, InputEvent::MouseUp { x, y, alt: false });
}

fn carry(game: &mut GameState, lightbox: &Lightbox, from: usize, to: usize) {
    let (a, b) = (
        lightbox.tableau().cell_centre(from),
        lightbox.tableau().cell_centre(to),
    );
    let (ax, ay, bx, by) = (a.x as i32, a.y as i32, b.x as i32, b.y as i32);
    game.handle_on(
        lightbox,
        InputEvent::MouseDown {
            x: ax,
            y: ay,
            alt: false,
        },
    );
    game.handle_on(lightbox, InputEvent::MouseDragged { x: bx, y: by });
    game.handle_on(
        lightbox,
        InputEvent::MouseUp {
            x: bx,
            y: by,
            alt: false,
        },
    );
}

/// A fresh bit lit both ways on a half: (cell, side, bit, inbound start, outbound start).
type TwoWay = (usize, usize, u8, f32, f32);

/// The fresh halves of `new` lit both ways whose inbound light starts (reach units) before the
/// outbound light, which is the only one a sweep draws.
fn two_way_halves(old: &BeamField, new: &BeamField, lightbox: &Lightbox) -> Vec<TwoWay> {
    let reach = lightbox.choreography().reach();
    let mut found = Vec::new();
    for c in 0..BOARD_CELLS {
        for s in 0..8 {
            let fresh = half_colour(new, c, s) & !half_colour(old, c, s);
            let both = new.outbound[c][s] & new.inbound[c][(s + 4) & 7] & fresh;
            for bit in [1u8, 2, 4].into_iter().filter(|b| both & b != 0) {
                let (out_start, _) = strand_start(new, reach, c, s, bit);
                // The inbound light's start: `strand_start` with the outbound bit taken away.
                let mut inbound_only = new.clone();
                inbound_only.outbound[c][s] &= !bit;
                let (in_start, inward) = strand_start(&inbound_only, reach, c, s, bit);
                assert!(inward);
                if in_start + 0.5 < out_start {
                    found.push((c, s, bit, in_start, out_start));
                }
            }
        }
    }
    found
}

/// The device pixel at the middle of half (`cell`, `side`), along the beam.
fn half_middle(lightbox: &Lightbox, cell: usize, side: usize) -> (usize, usize) {
    let t = lightbox.tableau();
    let c = t.cell_centre(cell);
    let (x, y) = (
        (cell % lumen::SIDE) as i32 + i32::from(DX[side]),
        (cell / lumen::SIDE) as i32 + i32::from(DY[side]),
    );
    let n = t.cell_centre(board_index(x as usize, y as usize));
    let m = (c.x + (n.x - c.x) * 0.25, c.y + (n.y - c.y) * 0.25);
    (m.0 as usize, m.1 as usize)
}

/// The largest difference of one channel between two `0x00RRGGBB` pixels.
fn distance(a: u32, b: u32) -> u32 {
    (0..3)
        .map(|k| (a >> (8 * k) & 0xff).abs_diff(b >> (8 * k) & 0xff))
        .max()
        .unwrap()
}

/// Most a channel may differ between a segment's two halves while both are lit (glow from the
/// pieces at either end differs a little).
const ALIKE: u32 = 40;

#[test]
fn a_half_lit_both_ways_shows_light_from_its_first_strand() {
    for (theme, scale) in [
        (Theme::Dark, 1.0),
        (Theme::Dark, 2.0),
        (Theme::Light, 1.0),
        (Theme::Light, 2.0),
    ] {
        let easel = Easel::new((1280.0 * scale) as u32, (800.0 * scale) as u32, scale);
        // Level 27 as in the report: its stored solution, with the two dopplers moved to the
        // middle row of the left and right columns.
        let mut st = state(Game::C1, 26, true);
        let mut lightbox = Lightbox::new(easel, theme, false);
        let mut out = vec![0u32; (easel.width * easel.height) as usize];
        let mut clock = Clock { now: 0.0 };
        lightbox.paint(&st, clock.now, Output::Xrgb(&mut out));
        let right = board_index(14, 6);
        carry(&mut st, &lightbox, board_index(0, 8), board_index(0, 6));
        carry(&mut st, &lightbox, board_index(3, 14), right);
        lightbox.paint(&st, clock.later(), Output::Xrgb(&mut out));
        clock.later();
        let (mut checked, mut dashed, mut worst) = (0, Vec::new(), 0);
        let at = |lightbox: &Lightbox, cell: usize, side: usize| {
            let (x, y) = half_middle(lightbox, cell, side);
            y * easel.width as usize + x
        };
        // Turns the right doppler round, each turn's cues run out at 60 fps.
        for k in 0..8 {
            let old = st.field().clone();
            click(&mut st, &lightbox, right);
            lightbox.paint(&st, clock.tick(), Output::Xrgb(&mut out));
            let halves = two_way_halves(&old, st.field(), &lightbox);
            while lightbox.next_wake(clock.now).is_some() {
                let now = clock.tick();
                lightbox.paint(&st, now, Output::Xrgb(&mut out));
                let Some(front) = lightbox.choreography().moment(now).sweep.map(|v| v.front) else {
                    continue;
                };
                // The front has passed the whole half coming in, not yet the light going out:
                // the half is lit, and looks like the other half of its segment (lit since the
                // light left the neighbour), not a gap between two dashes.
                for &(c, s, bit, in_start, out_start) in &halves {
                    if front >= in_start + 0.5 && front < out_start {
                        let n = board_index(
                            (c % lumen::SIDE).wrapping_add_signed(DX[s].into()),
                            (c / lumen::SIDE).wrapping_add_signed(DY[s].into()),
                        );
                        let (here, there) =
                            (out[at(&lightbox, c, s)], out[at(&lightbox, n, (s + 4) & 7)]);
                        checked += 1;
                        worst = worst.max(distance(here, there));
                        if distance(here, there) > ALIKE {
                            let (x, y) = (c % lumen::SIDE, c / lumen::SIDE);
                            dashed.push((k, now, x, y, s, bit, here, there));
                        }
                    }
                }
            }
            clock.later();
        }
        eprintln!("{theme:?} x{scale}: {checked} halves checked, worst difference {worst}");
        assert!(
            checked > 0,
            "{theme:?} x{scale}: no half lit both ways was checked"
        );
        assert!(
            dashed.is_empty(),
            "{theme:?} x{scale}: {} of {checked} halves lit both ways differ from their \
             segment's other half mid-sweep (turn, ms, x, y, side, bit, pixel, other half's): \
             {:x?}",
            dashed.len(),
            &dashed[..dashed.len().min(6)]
        );
    }
}
