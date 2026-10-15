//! The light theme (spec §2 decision 8, phase 5): the palette in use, light pixels at known
//! points, theme switches, and the memory plan (ruling L2).

mod common;

use common::{SETTLED, state};
use halation::{DeviceRect, Easel, Lightbox, Output, Scene, Theme};
use lumen::{BeamField, Board, Cell, Piece, board_index, trace};
use spectrograph::Game;

/// A cold, settled paint of `game`'s `slot` in `theme` at `easel`.
fn paint(
    theme: Theme,
    easel: Easel,
    game: Game,
    slot: usize,
    solved: bool,
) -> (Lightbox, Vec<u32>) {
    let st = state(game, slot, solved);
    let mut lightbox = Lightbox::new(easel, theme, false);
    let mut out = vec![0u32; (easel.width * easel.height) as usize];
    lightbox.paint(&st, SETTLED, Output::Xrgb(&mut out));
    (lightbox, out)
}

/// The XRGB pixel at (`x`, `y`) of a `width`-px-wide output, as [r, g, b].
fn at(out: &[u32], width: i32, x: i32, y: i32) -> [i32; 3] {
    let p = out[(y * width + x) as usize];
    [
        (p >> 16 & 0xff) as i32,
        (p >> 8 & 0xff) as i32,
        (p & 0xff) as i32,
    ]
}

/// Ruling L3 (spec conflict C4): the light theme's light tile is the wash, the board grown by
/// ⌈0.5u⌉ (not the dark tile's 1u), clipped to the canvas.
#[test]
fn a_light_lightbox_paints_into_a_wash_half_a_cell_past_the_board() {
    for easel in [
        Easel::new(1280, 800, 1.0),
        Easel::new(780, 1688, 2.0).with_touch(true),
    ] {
        let (lightbox, out) = paint(Theme::Light, easel, Game::C1, 0, false);
        let t = lightbox.tableau();
        let (b, reach) = (t.board, (0.5 * t.cell as f32).ceil() as i32);
        let canvas = t.canvas();
        let want = DeviceRect::new(
            (b.left - reach).max(canvas.left),
            (b.top - reach).max(canvas.top),
            (b.right + reach).min(canvas.right),
            (b.bottom + reach).min(canvas.bottom),
        );
        assert_eq!(lightbox.darkroom().light_rect(), want, "{easel:?}");
        // A light page: the canvas's mean luminance is high.
        let mean = out.iter().map(|&p| (p >> 8 & 0xff) as u64).sum::<u64>() / out.len() as u64;
        assert!(mean > 180, "{easel:?}: mean green {mean}");
    }
}

/// `a` over `b` at `k`, per channel.
fn lerp(a: [i32; 3], b: [i32; 3], k: f32) -> [i32; 3] {
    [0, 1, 2].map(|c| (a[c] as f32 + (b[c] - a[c]) as f32 * k).round() as i32)
}

fn near(got: [i32; 3], want: [i32; 3], tol: i32, what: &str) {
    assert!(
        (0..3).all(|c| (got[c] - want[c]).abs() <= tol),
        "{what}: {got:?}, want {want:?} ±{tol}"
    );
}

/// The light page with nothing on it at (`x`, `y`) of a 1280 × 800 @1 canvas whose board is
/// `b`: the ground #f4f6fa → #e9ecf3 down the canvas, under white α 0.9 → 0 at 0.55 of the
/// diagonal from the board's centre (spec §7.1 "Page", Light).
fn page(b: halation::DeviceRect, x: i32, y: i32) -> [i32; 3] {
    let focus = (
        (b.left + b.right) as f32 / 2.0,
        (b.top + b.bottom) as f32 / 2.0,
    );
    let diag = (1280f32).hypot(800.0);
    let ground = lerp(
        [0xf4, 0xf6, 0xfa],
        [0xe9, 0xec, 0xf3],
        (y as f32 + 0.5) / 800.0,
    );
    let t = ((x as f32 - focus.0).hypot(y as f32 - focus.1) / (0.55 * diag)).min(1.0);
    lerp(ground, [255, 255, 255], 0.9 * (1.0 - t))
}

fn lum(c: [i32; 3]) -> i32 {
    c[0] + c[1] + c[2]
}

/// Spec §7.1 "Page", Light: the ground and the lift; no grain, no vignette.
#[test]
fn the_light_page_is_a_pale_ground_lifted_towards_the_board() {
    let easel = Easel::new(1280, 800, 1.0);
    let (lightbox, out) = paint(Theme::Light, easel, Game::C1, 0, false);
    let b = lightbox.tableau().board;
    for (x, y) in [(1276, 4), (1276, 796), (640, 4)] {
        near(
            at(&out, 1280, x, y),
            page(b, x, y),
            3,
            &format!("page at ({x}, {y})"),
        );
    }
}

/// Spec §7.2 "Board panel": inside the `pad` margin the radial fill shows; just below the
/// panel its shadow (10 pt down, blur 28 pt) darkens the page well below what the page alone
/// is there, while above the panel the page is untouched; its rim is a 1-px line darker than
/// both sides. (Measured against the page itself, since the ground darkens down the canvas.)
#[test]
fn the_light_board_sits_on_a_panel_with_a_shadow_and_a_rim() {
    let easel = Easel::new(1280, 800, 1.0);
    let (lightbox, out) = paint(Theme::Light, easel, Game::C1, 0, false);
    let t = lightbox.tableau();
    let (b, p) = (t.board, t.board_panel());
    assert!(
        p.left < b.left && p.bottom > b.bottom,
        "the panel holds the board: {p:?}"
    );
    // The middle cell's centre row: no grid line across (they overhang into the margin).
    let mid = (b.top + b.bottom) / 2;
    let x = (p.left + b.left) / 2;
    let r = ((b.left - x) as f32 + 7.5 * t.cell as f32) / (0.75 * 15.0 * t.cell as f32);
    let fill = if r <= 0.6 {
        lerp([0xfb, 0xfc, 0xfe], [0xf1, 0xf3, 0xf8], r / 0.6)
    } else {
        lerp([0xf1, 0xf3, 0xf8], [0xe4, 0xe8, 0xf1], (r - 0.6) / 0.4)
    };
    near(
        at(&out, 1280, x, mid),
        fill,
        4,
        "the panel's fill in its margin",
    );
    let cx = (p.left + p.right) / 2;
    let (bx, by, ay) = (cx, p.bottom + 8, p.top - 8);
    let below = at(&out, 1280, bx, by);
    assert!(
        lum(below) + 20 <= lum(page(b, bx, by)),
        "shadow below: {below:?} vs page {:?}",
        page(b, bx, by)
    );
    near(
        at(&out, 1280, cx, ay),
        page(b, cx, ay),
        4,
        "no shadow above the panel",
    );
    let rim = at(&out, 1280, p.left, mid);
    let (inside, outside) = (
        at(&out, 1280, p.left + 2, mid),
        at(&out, 1280, p.left - 2, mid),
    );
    assert!(
        lum(rim) + 6 < lum(inside) && lum(rim) + 6 < lum(outside),
        "rim {rim:?}"
    );
}

/// Spec §7.5 Win state, light: Next ▶ is a #2f6bff pill with white "NEXT" (ruling L6: flat
/// strokes in `base`, no light tile), with its blue shadow under the pill.
#[test]
fn a_won_light_card_shows_a_blue_next_pill() {
    let easel = Easel::new(1280, 800, 1.0);
    let (lightbox, out) = paint(Theme::Light, easel, Game::C1, 0, true);
    let [l, t, r, b] = lightbox.tableau().next_pill();
    // Left of "NEXT" (which starts 16 pt in), on the pill's mid line.
    let fill = at(&out, 1280, (l + 6.0) as i32, ((t + b) / 2.0) as i32);
    near(fill, [0x2f, 0x6b, 0xff], 6, "the pill");
    let lit = (l as i32 + 16..r as i32 - 30)
        .flat_map(|x| (t as i32 + 8..b as i32 - 8).map(move |y| (x, y)))
        .any(|(x, y)| at(&out, 1280, x, y).iter().all(|&c| c >= 230));
    assert!(lit, "white NEXT strokes on the pill");
    // 3 px under the pill's bottom edge, on the near-white card: the blue shadow (σ 5 pt).
    let under = at(&out, 1280, ((l + r) / 2.0) as i32, b as i32 + 3);
    assert!(under[2] - under[0] >= 15, "the pill's shadow: {under:?}");
}

/// Spec §7.1 "Panel", Light: the card casts a shadow 4 pt down (blur 18 pt), baked into the
/// page (ruling L7): just under its bottom edge the page is darker than the page alone; just
/// above its top it is (nearly) the page.
#[test]
fn the_light_card_casts_its_shadow_on_the_page() {
    let easel = Easel::new(1280, 800, 1.0);
    let (lightbox, out) = paint(Theme::Light, easel, Game::C1, 0, false);
    let (b, c) = (
        lightbox.tableau().board,
        lightbox.tableau().device(lightbox.tableau().card),
    );
    let x = (c.left + c.right) / 2;
    let below = at(&out, 1280, x, c.bottom + 4);
    assert!(
        lum(below) + 12 <= lum(page(b, x, c.bottom + 4)),
        "{below:?}"
    );
    near(
        at(&out, 1280, x, c.top - 4),
        page(b, x, c.top - 4),
        5,
        "above the card",
    );
}

/// A bare board with `pieces` (x, y, piece, colour, dir), traced.
fn lit(pieces: &[(usize, usize, Piece, u8, u8)]) -> (Board, BeamField) {
    let mut board = Board::default();
    for &(x, y, piece, colour, dir) in pieces {
        board.cells[board_index(x, y)] = Cell { piece, colour, dir };
    }
    let mut field = BeamField::default();
    trace(&board, &mut field);
    (board, field)
}

/// A cold light paint of a bare board at 1280 × 800 @1, with the lightbox.
fn bare(pieces: &[(usize, usize, Piece, u8, u8)]) -> (Lightbox, Vec<u32>) {
    let (board, field) = lit(pieces);
    let easel = Easel::new(1280, 800, 1.0);
    let mut lightbox = Lightbox::new(easel, Theme::Light, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint_scene(
        Scene::bare(&board, &field, Game::C1),
        SETTLED,
        Output::Xrgb(&mut out),
    );
    (lightbox, out)
}

/// A cell's centre, device px.
fn centre(lightbox: &Lightbox, x: usize, y: usize) -> (i32, i32) {
    let c = lightbox.tableau().cell_centre(board_index(x, y));
    (c.x as i32, c.y as i32)
}

/// What a horizontal red light line (0.036u, `#f0213a` opaque) shows at (`x`, `y`) of a board of
/// even `u`, whose axis is the px row boundary at `y` (a cell centre): it covers the px by its
/// half width, 0.018u, over the glow under it, which the px below (off the line) shows.
fn line(out: &[u32], u: i32, x: i32, y: i32) -> [i32; 3] {
    assert!(u % 2 == 0, "an even cell puts the axis on a px boundary");
    let k = (0.018 * u as f32).min(1.0);
    lerp(at(out, 1280, x, y + 1), [0xf0, 0x21, 0x3a], k)
}

/// Spec §7.3 Light: a beam is its mix colour on its line, a halo-tinted glow beside it, and
/// nothing a cell away (no bloom leaks across the board).
#[test]
fn a_light_beam_is_its_colour_with_a_soft_glow() {
    let (lightbox, out) = bare(&[(2, 7, Piece::Laser, 4, 2)]);
    let (_, empty) = bare(&[]);
    let u = lightbox.tableau().cell;
    let (x, y) = centre(&lightbox, 7, 7);
    near(at(&out, 1280, x, y), line(&out, u, x, y), 4, "the line");
    let by = y - (0.09 * u as f32) as i32;
    let (beside, bare_there) = (at(&out, 1280, x, by), at(&empty, 1280, x, by));
    assert!(beside[0] >= 200, "the glow keeps red: {beside:?}");
    assert!(
        beside[1] + 15 <= bare_there[1],
        "the glow tints the board there: {beside:?} vs {bare_there:?}"
    );
    assert!(beside[1] > 60, "a glow, not the line: {beside:?}");
    let off = (0.75 * u as f32) as i32;
    let (a, b) = (
        at(&out, 1280, x + 3, y - off),
        at(&empty, 1280, x + 3, y - off),
    );
    assert!(
        (0..3).all(|c| (a[c] - b[c]).abs() <= 2),
        "0.75u away: {a:?} vs {b:?}"
    );
}

/// At an odd cell size a cell's centre is a px centre, so the 0.036u line covers the px on its
/// axis whole: there the beam is its mix colour exactly (spec §7.3 Light).
#[test]
fn a_light_line_on_a_px_centre_is_its_mix() {
    let (board, field) = lit(&[(2, 7, Piece::Laser, 4, 2)]);
    let easel = (780..840)
        .map(|h| Easel::new(1280, h, 1.0))
        .find(|&e| halation::Tableau::arrange(&e).cell % 2 == 1)
        .expect("an odd cell size");
    let mut lightbox = Lightbox::new(easel, Theme::Light, false);
    let w = easel.width as i32;
    let mut out = vec![0u32; (easel.width * easel.height) as usize];
    lightbox.paint_scene(
        Scene::bare(&board, &field, Game::C1),
        SETTLED,
        Output::Xrgb(&mut out),
    );
    let u = lightbox.tableau().cell;
    assert!(
        0.018 * u as f32 >= 0.5,
        "the line covers its axis px whole at u {u}"
    );
    let (x, y) = centre(&lightbox, 7, 7);
    near(
        at(&out, w, x, y),
        [0xf0, 0x21, 0x3a],
        2,
        "the line on its axis",
    );
}

/// Spec §7.3 Light, "mixes are computed and drawn": red crossing green shows yellow at the
/// crossing's centre (the solid mix dot), not the over-blend of the two.
#[test]
fn crossing_beams_show_their_computed_mix() {
    let (lightbox, out) = bare(&[(2, 7, Piece::Laser, 4, 2), (7, 12, Piece::Laser, 2, 0)]);
    let (x, y) = centre(&lightbox, 7, 7);
    near(at(&out, 1280, x, y), [242, 183, 5], 12, "the crossing");
}

/// Spec §7.3 Light: a white beam is white at its core with a blue-grey halo stroke beside it.
#[test]
fn white_beams_have_a_blue_grey_halo() {
    let (lightbox, out) = bare(&[(2, 7, Piece::Laser, 7, 2)]);
    let u = lightbox.tableau().cell as f32;
    let (x, y) = centre(&lightbox, 7, 7);
    assert!(
        at(&out, 1280, x, y).iter().all(|&c| c >= 240),
        "the white core"
    );
    let side = at(&out, 1280, x, y - (0.032 * u).round() as i32);
    assert!(side[2] > side[0] + 30, "the blue-grey halo: {side:?}");
}

/// Spec §7.4 Light: a lit red target is white at its centre with its colour around it.
#[test]
fn a_lit_target_shows_its_mix_under_white() {
    let (lightbox, out) = bare(&[(2, 7, Piece::Laser, 4, 2), (7, 7, Piece::Pinwheel, 4, 0)]);
    let u = lightbox.tableau().cell as f32;
    let (x, y) = centre(&lightbox, 7, 7);
    // The core's radial (white → the mix α 0.9 at 0.35 of 0.23u) at this px's centre, half a
    // px off the target's (integer) centre on each axis.
    let k = 0.5f32.hypot(0.5) / (0.23 * u) / 0.35;
    near(
        at(&out, 1280, x, y),
        lerp([255; 3], [0xf0, 0x21, 0x3a], k),
        8,
        "the white core",
    );
    let ring = at(
        &out,
        1280,
        x + (0.186 * u) as i32,
        y - (0.186 * u) as i32 / 3,
    );
    assert!(ring[0] > 180 && ring[1] < 150, "ring {ring:?}");
}

/// A cold light paint shows known colours at known points: the page, the board panel's fill in
/// its margin, a red beam's line, and a body (a wall) off the beams, untouched by the wash (the
/// same as on a board with no light) yet not the bare board.
#[test]
fn a_cold_light_paint_shows_its_page_panel_beam_and_bodies() {
    let wall = (10, 2, Piece::Wall, 0, 0);
    let (lightbox, out) = bare(&[(2, 7, Piece::Laser, 4, 2), wall]);
    let (_, unlit) = bare(&[wall]);
    let (_, empty) = bare(&[]);
    let t = lightbox.tableau();
    let (b, p) = (t.board, t.board_panel());
    near(at(&out, 1280, 1276, 4), page(b, 1276, 4), 3, "the page");
    let mid = (b.top + b.bottom) / 2;
    let x = (p.left + b.left) / 2;
    let r = ((b.left - x) as f32 + 7.5 * t.cell as f32) / (0.75 * 15.0 * t.cell as f32);
    let fill = lerp([0xf1, 0xf3, 0xf8], [0xe4, 0xe8, 0xf1], (r - 0.6) / 0.4);
    near(at(&out, 1280, x, mid), fill, 4, "the panel's margin");
    let (bx, by) = centre(&lightbox, 7, 7);
    let red = line(&out, t.cell, bx, by);
    near(at(&out, 1280, bx, by), red, 4, "the red line");
    near(red, [0xf0, 0x21, 0x3a], 24, "the red line, nearly its mix");
    let (tx, ty) = centre(&lightbox, 10, 2);
    let body = at(&out, 1280, tx, ty);
    near(body, at(&unlit, 1280, tx, ty), 1, "the body, off the beams");
    let floor = at(&empty, 1280, tx, ty);
    assert!(
        (lum(body) - lum(floor)).abs() >= 30,
        "a body, not the board: {body:?} vs {floor:?}"
    );
}

/// Ruling L2: in the light theme the dark light planes and lettering tiles are given back, so
/// a light lightbox holds less than a dark one; both stay in budget (spec §14).
#[test]
fn a_light_lightbox_holds_less_than_a_dark_one() {
    const MB: usize = 1 << 20;
    for (easel, cap) in [
        (
            Easel::new(780, 1688, 2.0).with_touch(true),
            20 * MB - 780 * 1688 * 4,
        ),
        (Easel::new(2560, 1600, 2.0), 64 * MB),
        (Easel::new(3456, 2234, 2.0), 110 * MB),
    ] {
        let [dark, light] = Theme::ALL.map(|t| Lightbox::new(easel, t, false).buffer_bytes());
        assert!(light < dark, "{easel:?}: light {light} vs dark {dark}");
        assert!(light <= cap && dark <= cap, "{easel:?}: {light} / {dark}");
        println!(
            "{easel:?}: dark {:.1} MiB, light {:.1} MiB",
            dark as f64 / MB as f64,
            light as f64 / MB as f64
        );
    }
}

/// The wash wipes only what it drew, and a reshape to the same size keeps its planes: a cold
/// repaint of another level, and one after a resize away and back (moving the wash, keeping
/// its size), paint what a fresh light lightbox paints.
#[test]
fn a_light_repaint_and_a_resize_round_trip_paint_like_a_fresh_lightbox() {
    let easel = Easel::new(1280, 800, 1.0);
    let (mut lightbox, mut out) = paint(Theme::Light, easel, Game::C1, 41, true);
    let (_, fresh) = paint(Theme::Light, easel, Game::C1, 0, false);
    let st = state(Game::C1, 0, false);
    lightbox.reset();
    lightbox.paint(&st, SETTLED, Output::Xrgb(&mut out));
    assert!(out == fresh, "a cold repaint");
    let (mut lightbox, _) = paint(Theme::Light, easel, Game::C1, 41, true);
    lightbox.resize(Easel::new(1287, 800, 1.0));
    lightbox.resize(easel);
    lightbox.paint(&st, SETTLED, Output::Xrgb(&mut out));
    assert!(out == fresh, "after a resize away and back");
}

/// Ruling L6: the light header ("LEVEL 1") and the brand are ink in `base`: near-black strokes
/// where the dark theme has light.
#[test]
fn the_light_header_and_brand_are_ink() {
    let easel = Easel::new(1280, 800, 1.0);
    let (lightbox, out) = paint(Theme::Light, easel, Game::C1, 0, false);
    let t = lightbox.tableau();
    let darkest = |r: DeviceRect| {
        (r.top..r.bottom)
            .flat_map(|y| (r.left..r.right).map(move |x| (x, y)))
            .map(|(x, y)| at(&out, 1280, x, y).iter().sum::<i32>())
            .min()
            .unwrap()
    };
    let c = t.device(t.card);
    let header = DeviceRect::new(c.left + 12, c.top + 12, c.left + 140, c.top + 40);
    assert!(darkest(header) < 3 * 80, "header ink");
    let brand = t.device(t.brand.expect("1280 × 800 has the brand"));
    assert!(darkest(brand) < 3 * 80, "brand ink");
}

/// A light win: its header is written on in ink (fewer inked px mid-reveal than settled), and
/// the settled frame equals a cold light paint of the won level (the brand's ink kept by the
/// card's and picker's repaints, ruling L6).
#[test]
fn a_light_win_writes_its_header_on_then_settles_to_cold() {
    let easel = Easel::new(1280, 800, 1.0);
    let mut st = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel, Theme::Light, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint(&st, 0.0, Output::Xrgb(&mut out));
    st.handle(lumen::InputEvent::Key(st.rules().cheat_key));
    lightbox.paint(&st, 1000.0, Output::Xrgb(&mut out));
    lightbox.paint(&st, 1000.0 + 250.0, Output::Xrgb(&mut out));
    let c = lightbox.tableau().device(lightbox.tableau().card);
    let header = |o: &[u32]| {
        (c.top + 12..c.top + 40)
            .flat_map(|y| (c.left + 12..c.left + 200).map(move |x| (x, y)))
            .filter(|&(x, y)| at(o, 1280, x, y).iter().sum::<i32>() < 3 * 120)
            .count()
    };
    let mid = header(&out);
    let mut clock = 1250.0;
    while lightbox.next_wake(clock).is_some() {
        clock += 1000.0 / 60.0;
        lightbox.paint(&st, clock, Output::Xrgb(&mut out));
    }
    let mut fresh = Lightbox::new(easel, Theme::Light, false);
    let mut want = vec![0u32; 1280 * 800];
    fresh.paint(&st, SETTLED, Output::Xrgb(&mut want));
    assert!(out == want, "settled equals cold");
    assert!(
        mid < header(&want),
        "mid-reveal {mid} < settled {}",
        header(&want)
    );
}

/// Spec §7.5 Sheet, light: "LEVELS" is ink on the sheet's white panel.
#[test]
fn the_light_sheet_is_titled_in_ink() {
    let easel = Easel::new(780, 1480, 2.0).with_touch(true);
    let st = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel, Theme::Light, false);
    assert!(lightbox.command(halation::Control::Levels));
    let mut out = vec![0u32; 780 * 1480];
    lightbox.paint(&st, SETTLED, Output::Xrgb(&mut out));
    let p = lightbox.tableau().device(lightbox.tableau().sheet.panel);
    let title = DeviceRect::new(p.left + 20, p.top + 16, p.left + 200, p.top + 70);
    let sums: Vec<i32> = (title.top..title.bottom)
        .flat_map(|y| (title.left..title.right).map(move |x| (x, y)))
        .map(|(x, y)| at(&out, 780, x, y).iter().sum::<i32>())
        .collect();
    let darkest = sums.iter().min().unwrap();
    assert!(*darkest < 3 * 80, "LEVELS in ink: {darkest}");
    // Review T8 M4: six letters' strokes (440 px measured), not one dark px or a dark blob, on
    // the white panel (93% of the box measured).
    let inked = sums.iter().filter(|&&s| s < 3 * 120).count();
    let white = sums.iter().filter(|&&s| s >= 3 * 245).count();
    assert!((250..=800).contains(&inked), "LEVELS' strokes: {inked} px");
    assert!(white * 10 >= sums.len() * 8, "the white panel: {white} px");
}

/// Still review finding 1: with the sheet open, the card's ink lettering left of the panel lies
/// under the scrim, and the scrim veils it as it dims the page: the ink's darkest px is lifted
/// toward the dimmed page. (A navy scrim at α 0.35 left near-black ink near-black, so the "L"
/// of "LEVEL 1" read undimmed beside the panel.)
#[test]
fn the_light_scrim_veils_the_cards_ink_outside_the_sheet() {
    let easel = Easel::new(1170, 2532, 3.0).with_touch(true);
    let st = state(Game::C1, 0, false);
    let (plain, open) = (
        fresh(Theme::Light, easel, &st, false),
        fresh(Theme::Light, easel, &st, true),
    );
    let lightbox = Lightbox::new(easel, Theme::Light, false);
    let t = lightbox.tableau();
    let (p, c) = (t.device(t.sheet.panel), t.device(t.card));
    assert!(
        c.left + 30 < p.left,
        "the card's lettering starts left of the panel"
    );
    let sums = |o: &[u32]| {
        (c.top..c.top + 150)
            .flat_map(|y| (c.left..p.left - 2).map(move |x| (x, y)))
            .map(|(x, y)| at(o, 1170, x, y).iter().sum::<i32>())
            .collect::<Vec<_>>()
    };
    let (plain, open) = (sums(&plain), sums(&open));
    let darkest = |s: &[i32]| *s.iter().min().unwrap();
    let brightest = |s: &[i32]| *s.iter().max().unwrap();
    assert!(
        darkest(&plain) < 3 * 50,
        "the ink is there: {}",
        darkest(&plain)
    );
    assert!(
        darkest(&open) >= darkest(&plain) + 3 * 25,
        "the ink under the scrim: {} vs {} without the sheet",
        darkest(&open),
        darkest(&plain)
    );
    assert!(
        brightest(&open) <= brightest(&plain) - 3 * 40,
        "the page under the scrim is dimmed: {} vs {}",
        brightest(&open),
        brightest(&plain)
    );
}

/// Still review finding 2: a lit white target reads lit on the light board: its ring and the
/// halo past its arcs are `halo(WHITE)`, blue-grey (white would vanish on the white disc).
#[test]
fn a_lit_white_target_shows_a_blue_grey_ring_and_halo() {
    let (lightbox, out) = bare(&[(7, 2, Piece::Laser, 7, 4), (7, 7, Piece::Pinwheel, 7, 0)]);
    let u = lightbox.tableau().cell as f32;
    let (x, y) = centre(&lightbox, 7, 7);
    let blue = |[r, _, b]: [i32; 3]| b - r;
    // The ring (r 0.186u, 0.035u wide) and the halo at 0.45u, both east, off the beam.
    let ring = at(&out, 1280, x + (0.186 * u).round() as i32, y);
    assert!(blue(ring) >= 50, "the ring: {ring:?}");
    let (_, unlit) = bare(&[(7, 7, Piece::Pinwheel, 7, 0)]);
    let hx = x + (0.45 * u).round() as i32;
    let (halo, tile) = (at(&out, 1280, hx, y), at(&unlit, 1280, hx, y));
    assert!(
        blue(halo) >= blue(tile) + 6,
        "the halo: {halo:?} over {tile:?}"
    );
}

/// How red a pixel reads: red over the mean of green and blue.
fn redness([r, g, b]: [i32; 3]) -> i32 {
    r - (g + b) / 2
}

/// Ruling L5: a light sweep head is a halo-coloured dot painted over the board: around each
/// head's centre the frame is tinted toward the beam's colour, redder than the settled picture
/// there (where the beam is whole), not lightened to white.
#[test]
fn a_light_sweep_head_is_painted_in_its_halo() {
    let (board, field) = lit(&[(2, 7, Piece::Laser, 4, 2), (7, 7, Piece::Mirror, 0, 3)]);
    let easel = Easel::new(1280, 800, 1.0);
    let mut lightbox = Lightbox::new(easel, Theme::Light, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint_scene(
        Scene::bare(&board, &field, Game::C1),
        0.0,
        Output::Xrgb(&mut out),
    );
    // Turn the mirror: a new strand sweeps north from it.
    let mut turned = board.clone();
    turned.cells[board_index(7, 7)].dir = 1;
    let mut field2 = BeamField::default();
    trace(&turned, &mut field2);
    let scene = Scene::bare(&turned, &field2, Game::C1);
    lightbox.paint_scene(scene, 1000.0, Output::Xrgb(&mut out));
    lightbox.paint_scene(scene, 1060.0, Output::Xrgb(&mut out));
    assert!(lightbox.next_wake(1060.0).is_some(), "still sweeping");
    let (_, settled) = bare(&[(2, 7, Piece::Laser, 4, 2), (7, 7, Piece::Mirror, 0, 1)]);
    // The heads, away from the mirror (where the old strand fades).
    let (t, u) = (lightbox.tableau(), lightbox.tableau().cell as f32);
    let mirror = t.cell_centre(board_index(7, 7));
    let moment = lightbox.choreography().moment(1060.0);
    let view = moment.sweep.expect("a sweep");
    let mut heads = Vec::new();
    halation::beamline::for_each_head(&view, &turned, &field2, |cell, side, at, _| {
        let (c, d) = (t.cell_centre(cell), u / 2.0 * at);
        let p = (
            c.x + f32::from(lumen::DX[side]) * d,
            c.y + f32::from(lumen::DY[side]) * d,
        );
        if (p.0 - mirror.x).hypot(p.1 - mirror.y) > 0.5 * u {
            heads.push(p);
        }
    });
    assert!(!heads.is_empty(), "heads out of the mirror's cell");
    for (hx, hy) in heads {
        let r = (0.15 * u) as i32;
        let (cx, cy) = (hx as i32, hy as i32);
        let tinted = (cy - r..=cy + r)
            .flat_map(|y| (cx - r..=cx + r).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                redness(at(&out, 1280, x, y)) > redness(at(&settled, 1280, x, y)) + 40
            })
            .count();
        assert!(
            tinted >= 8,
            "the head at ({hx:.1}, {hy:.1}) tints {tinted} px toward red"
        );
    }
}

/// Ruling L5 (review finding 6): a white target's burst shows on the light board: its ring is
/// blue-grey (`halo(WHITE)`; `mix(WHITE)` is white, which reads as nothing there), so around
/// the ring's circle the frame is bluer than the settled picture.
#[test]
fn a_white_targets_burst_shows_on_the_light_board() {
    let (dark_b, dark_f) = lit(&[(7, 7, Piece::Pinwheel, 7, 0)]);
    let (lit_b, lit_f) = lit(&[(2, 7, Piece::Laser, 7, 2), (7, 7, Piece::Pinwheel, 7, 0)]);
    let easel = Easel::new(1280, 800, 1.0);
    let mut lightbox = Lightbox::new(easel, Theme::Light, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint_scene(
        Scene::bare(&dark_b, &dark_f, Game::C1),
        0.0,
        Output::Xrgb(&mut out),
    );
    let scene = Scene::bare(&lit_b, &lit_f, Game::C1);
    lightbox.paint_scene(scene, 1000.0, Output::Xrgb(&mut out));
    let burst = lightbox.choreography().bursts()[0];
    assert_eq!(burst.cell, board_index(7, 7));
    // 30 ms into the burst its ring is at ≈ 0.747u, α ≈ 0.23 (as the dark burst test has it).
    lightbox.paint_scene(scene, burst.at + 30.0, Output::Xrgb(&mut out));
    let (_, settled) = bare(&[(2, 7, Piece::Laser, 7, 2), (7, 7, Piece::Pinwheel, 7, 0)]);
    let c = lightbox.tableau().cell_centre(burst.cell);
    let u = lightbox.tableau().cell as f32;
    let bluer = (0..360).step_by(10).filter(|deg| {
        let a = (*deg as f32).to_radians();
        let (x, y) = (
            (c.x + a.cos() * 0.747 * u) as i32,
            (c.y + a.sin() * 0.747 * u) as i32,
        );
        let ([pr, _, pb], [sr, _, sb]) = (at(&out, 1280, x, y), at(&settled, 1280, x, y));
        pb - pr >= 10 && (pb - pr) >= (sb - sr) + 6
    });
    assert!(bluer.count() >= 12, "a blue-grey ring");
}

/// A fresh lightbox's cold paint of `st` in `theme` (with the sheet open when `sheet`).
fn fresh(theme: Theme, easel: Easel, st: &lumen::GameState, sheet: bool) -> Vec<u32> {
    let mut lightbox = Lightbox::new(easel, theme, false);
    if sheet {
        lightbox.command(halation::Control::Levels);
    }
    let mut out = vec![0u32; (easel.width * easel.height) as usize];
    lightbox.paint(st, SETTLED, Output::Xrgb(&mut out));
    out
}

/// Ruling L10: dark → light → dark → light, each paint after a switch equals a fresh lightbox's
/// in that theme, byte for byte (no sprite, tile or lettering of the other theme survives).
#[test]
fn a_theme_round_trip_paints_like_fresh_lightboxes() {
    for easel in [
        Easel::new(1280, 800, 1.0),
        Easel::new(780, 1480, 2.0).with_touch(true),
    ] {
        let st = state(Game::C2, 0, true);
        let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
        let mut out = vec![0u32; (easel.width * easel.height) as usize];
        lightbox.paint(&st, SETTLED, Output::Xrgb(&mut out));
        for theme in [Theme::Light, Theme::Dark, Theme::Light] {
            lightbox.set_theme(theme);
            let damage = lightbox
                .paint(&st, SETTLED, Output::Xrgb(&mut out))
                .damage
                .to_vec();
            let whole = DeviceRect::new(0, 0, easel.width as i32, easel.height as i32);
            assert_eq!(damage, [whole], "cold");
            assert!(
                out == fresh(theme, easel, &st, false),
                "{easel:?} {theme:?}"
            );
        }
    }
}

/// Final review M4: the same theme again is no change: no reshape, no cold repaint, and a
/// running cue runs on.
#[test]
fn the_same_theme_again_changes_nothing() {
    let easel = Easel::new(1280, 800, 1.0);
    for theme in Theme::ALL {
        let mut st = state(Game::C1, 0, false);
        let mut lightbox = Lightbox::new(easel, theme, false);
        let mut out = vec![0u32; (easel.width * easel.height) as usize];
        lightbox.paint(&st, 0.0, Output::Xrgb(&mut out));
        lightbox.set_theme(theme);
        let damage = lightbox.paint(&st, 16.0, Output::Xrgb(&mut out)).damage;
        assert!(
            damage.is_empty(),
            "{theme:?}: nothing to repaint, got {damage:?}"
        );
        st.handle(lumen::InputEvent::Key(st.rules().cheat_key));
        lightbox.paint(&st, 1000.0, Output::Xrgb(&mut out));
        assert!(lightbox.next_wake(1000.0).is_some(), "the win runs");
        lightbox.set_theme(theme);
        lightbox.paint(&st, 1016.0, Output::Xrgb(&mut out));
        assert!(
            lightbox.next_wake(1016.0).is_some(),
            "{theme:?}: the win still runs"
        );
    }
}

/// Spec §11: a theme change mid-cue repaints cold and settled: the frame after it equals a
/// fresh lightbox's, and nothing is left to wake for: mid-sweep, mid-win, mid level fade, and
/// while the sheet fades in.
#[test]
fn a_theme_change_mid_cue_paints_cold_and_idles() {
    let desk = Easel::new(1280, 800, 1.0);
    let phone = Easel::new(780, 1480, 2.0).with_touch(true); // has a sheet picker
    type Setup = fn(&mut lumen::GameState<'static>, &mut Lightbox);
    // A turn of the solved level's first movable board piece (its sweep), as `stills.rs` does.
    let turn: Setup = |st, lb| {
        let i = (0..lumen::BOARD_CELLS)
            .find(|&i| st.board().cells[i].piece.is_movable())
            .unwrap();
        let c = lb.tableau().cell_centre(i);
        let (x, y) = (c.x as i32, c.y as i32);
        st.handle_on(lb, lumen::InputEvent::MouseDown { x, y, alt: true });
        st.handle_on(lb, lumen::InputEvent::MouseUp { x, y, alt: true });
    };
    let cues: [(&str, Easel, bool, Setup); 4] = [
        ("sweep", desk, true, turn),
        ("win", desk, false, |st, _| {
            st.handle(lumen::InputEvent::Key(st.rules().cheat_key));
        }),
        ("level", desk, false, |st, _| {
            st.handle(lumen::InputEvent::Key(st.rules().cheat_key));
            st.handle(lumen::InputEvent::Key(b'='));
        }),
        ("sheet", phone, false, |_, lb| {
            assert!(lb.command(halation::Control::Levels));
        }),
    ];
    for (name, easel, solved, setup) in cues {
        for (from, to) in [(Theme::Dark, Theme::Light), (Theme::Light, Theme::Dark)] {
            let mut st = state(Game::C1, 0, solved);
            let mut lightbox = Lightbox::new(easel, from, false);
            let mut out = vec![0u32; (easel.width * easel.height) as usize];
            lightbox.paint(&st, 0.0, Output::Xrgb(&mut out));
            setup(&mut st, &mut lightbox);
            lightbox.paint(&st, 1000.0, Output::Xrgb(&mut out));
            lightbox.paint(&st, 1050.0, Output::Xrgb(&mut out));
            assert!(lightbox.next_wake(1050.0).is_some(), "{name}: the cue runs");
            lightbox.set_theme(to);
            lightbox.paint(&st, 1066.0, Output::Xrgb(&mut out));
            assert_eq!(lightbox.next_wake(1066.0), None, "{name} {to:?}: no cue");
            let sheet = lightbox.sheet_open();
            assert!(
                out == fresh(to, easel, &st, sheet),
                "{name} {from:?} → {to:?}"
            );
        }
    }
}

/// Ruling L10: a held piece stays held and the open sheet stays open across a theme change.
#[test]
fn a_theme_change_keeps_the_held_piece_and_the_sheet() {
    let phone = Easel::new(780, 1480, 2.0).with_touch(true);
    let st = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(phone, Theme::Dark, false);
    assert!(lightbox.command(halation::Control::Levels));
    let mut out = vec![0u32; 780 * 1480];
    lightbox.paint(&st, SETTLED, Output::Xrgb(&mut out));
    lightbox.set_theme(Theme::Light);
    assert!(lightbox.sheet_open());
    lightbox.paint(&st, SETTLED, Output::Xrgb(&mut out));
    assert!(out == fresh(Theme::Light, phone, &st, true));
    // A held piece: press on the first movable piece, drag over the board.
    let easel = Easel::new(1280, 800, 1.0);
    let mut st = state(Game::C1, 0, false);
    let mut lightbox = Lightbox::new(easel, Theme::Light, false);
    let mut out = vec![0u32; 1280 * 800];
    lightbox.paint(&st, SETTLED, Output::Xrgb(&mut out));
    let from = (0..lumen::BOARD_CELLS + lumen::VISIBLE_TOOLBOX_SLOTS)
        .find(|&i| st.board().cells[i].piece.is_movable())
        .unwrap();
    let (a, b) = (
        lightbox.tableau().cell_centre(from),
        lightbox.tableau().cell_centre(112),
    );
    let (ax, ay) = (a.x as i32, a.y as i32);
    st.handle_on(
        &lightbox,
        lumen::InputEvent::MouseDown {
            x: ax,
            y: ay,
            alt: false,
        },
    );
    let (bx, by) = (b.x as i32, b.y as i32);
    st.handle_on(&lightbox, lumen::InputEvent::MouseDragged { x: bx, y: by });
    lightbox.paint(&st, SETTLED, Output::Xrgb(&mut out));
    lightbox.set_theme(Theme::Dark);
    lightbox.paint(&st, SETTLED, Output::Xrgb(&mut out));
    assert!(st.drag().is_some(), "still held");
    assert!(out == fresh(Theme::Dark, easel, &st, false));
}

/// Ruling L2: a dark → light → dark round trip gives back what it took: the buffers end as a
/// fresh dark lightbox's (within 1/64), and a switch to light holds what a fresh light one does.
#[test]
fn a_theme_round_trip_gives_back_its_buffers() {
    for easel in [
        Easel::new(780, 1688, 2.0).with_touch(true),
        Easel::new(2560, 1600, 2.0),
    ] {
        let [dark, light] = Theme::ALL.map(|t| Lightbox::new(easel, t, false).buffer_bytes());
        let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
        lightbox.set_theme(Theme::Light);
        let switched = lightbox.buffer_bytes();
        assert!(
            switched <= light + light / 64,
            "{easel:?}: light after dark {switched} vs {light}"
        );
        lightbox.set_theme(Theme::Dark);
        let back = lightbox.buffer_bytes();
        assert!(
            back <= dark + dark / 64,
            "{easel:?}: dark again {back} vs {dark}"
        );
    }
}

/// Reported, not gated (the bench gates it from Task 11): a light cold paint against a dark
/// one at the reference size, so Task 6's stop point is visible early.
#[test]
#[ignore]
fn light_cold_paint_time() {
    for theme in Theme::ALL {
        let easel = Easel::new(2560, 1600, 2.0);
        let st = state(Game::C1, 41, true);
        let mut lightbox = Lightbox::new(easel, theme, false);
        let mut out = vec![0u32; 2560 * 1600];
        let mut best = std::time::Duration::MAX;
        for _ in 0..9 {
            lightbox.reset();
            let t = std::time::Instant::now();
            lightbox.paint(&st, SETTLED, Output::Xrgb(&mut out));
            best = best.min(t.elapsed());
        }
        println!("{theme:?}: cold {best:?}");
    }
}
