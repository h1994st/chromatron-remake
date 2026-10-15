//! `just bench`: the modern look's paint paths on the slowest real levels of each game (the
//! densest and the most wall-heavy of the first 50, solved where a stored solution exists) at
//! 2560 × 1600 (1280 × 800 pt @2) and on a phone (780 × 1480, touch), checked against the spec's
//! §14 budgets as the user revised them (Apple Silicon, release build), with the animation frames
//! of three sweeps, of a pasted solution's sweep and bursts, of a cheat-key win's packets, pulses
//! and header, of level changes' cross-fades and sweeps, and of the level sheet fading in and out
//! (at 1600 × 1200 @2, which has a sheet) (the p90, on the desktop and the phone) against the
//! user's motion budgets, the level changes' first frames (onto the level
//! fresh, and back onto it solved) against the cold ones; a drop onto a tray slot, a rotation
//! (its cue's first frame), a win, opening the level sheet and the board light's develop (whole,
//! and by patches after one cell changed) are reported without a budget. Every other row paints
//! settled. Every gated row runs in both themes at the same cap, and the light/dark ratio of each
//! is printed (over 1.15 is recorded for phase 6, not failed). Exits non-zero if a budget is
//! exceeded or a row that must run cannot.

use std::{
    hint::black_box,
    process::ExitCode,
    time::{Duration, Instant},
};

use criterion::Criterion;
use halation::darkroom::Darkroom;
use halation::{Control, Easel, Lightbox, Output, Scene, Tableau, Theme};
use lumen::{
    BOARD_CELLS, Board, GameState, InputEvent, Piece, SaveData, VISIBLE_TOOLBOX_SLOTS, board_index,
    stored_solution, toolbox_index,
};
use spectrograph::{Game, GameData};

const ARRANGE_BUDGET: Duration = Duration::from_micros(20);
/// Spec §14: `Reach::compute` on the largest level.
const REACH_BUDGET: Duration = Duration::from_micros(20);
/// The resize and level-change rows' budget. 28 ms, not the spec's 14: the user set 25 ms after
/// the bit-identical optimisation reached about 19–24 ms (Task 21), then raised it to 28 ms
/// (2026-10-12) once the honest resize rows measured 22–24.4 ms, leaving speed work to polish.
const COLD_BUDGET: Duration = Duration::from_millis(28);
/// Spec §14 said 5 ms; the user's R-U3 decision (2026-10-12) budgets 8 ms.
const PHONE_COLD_BUDGET: Duration = Duration::from_millis(8);
const DRAG_BUDGET: Duration = Duration::from_millis(1);
/// The user's motion budget (2026-10-13): an animation frame (p90) at 2560 × 1600. Kept by the
/// phase-4 Task 13 tightening, which only lowers a cap whose worst row is ≤ 0.6 × it: the worst
/// over five runs was 6.57 ms (C4's sheet frames at 1600 × 1200 @2; sweeps 6.03), over 6.
const ANIM_BUDGET: Duration = Duration::from_millis(10);
/// The user's motion budget (2026-10-13) on a phone (780 × 1480 @2). Kept by Task 13 likewise:
/// the worst over five runs was 5.43 ms (C4's sheet frames), over 4.8.
const PHONE_ANIM_BUDGET: Duration = Duration::from_millis(8);
/// Level changes timed per level row (the first frames' median, every animation frame's p90).
const LEVEL_REPEATS: usize = 5;
/// A paint at this `now` is settled: no cue starts and every running one ends.
const SETTLED: f64 = f64::INFINITY;

const WIDTH: usize = 2560;
const HEIGHT: usize = 1600;
/// A phone in portrait: 390 × 740 pt @2, the Stacked arrangement.
const PHONE_WIDTH: usize = 780;
const PHONE_HEIGHT: usize = 1480;

/// Canvas-sized outputs for `width` and `width + 1` px across, as `cold` alternates them.
fn outputs(width: usize, height: usize) -> [Vec<u32>; 2] {
    [vec![0; width * height], vec![0; (width + 1) * height]]
}

/// A truly cold paint, as after a resize: each call alternates the easel between `width` and
/// `width + 1` px (as `lightbox_budgets.mjs` alternates widths), so every call pays the reshape a
/// user meets in a live resize (re-arranged, every buffer refitted, backdrop re-baked), which a
/// same-size `resize` would partly skip. What a px wider leaves unchanged is kept, as it is for
/// the user: the sprites when the cell holds, and lettering whose tile keeps its size
/// (`cell_change` times the resizes that keep none of the sprites).
fn cold(
    width: usize,
    height: usize,
    touch: bool,
) -> impl FnMut(&mut Lightbox, &GameState, &mut [Vec<u32>; 2]) {
    let mut k = 0;
    move |lightbox, state, outs| {
        k = (k + 1) % 2;
        let easel = Easel::new((width + k) as u32, height as u32, 2.0).with_touch(touch);
        lightbox.resize(easel);
        lightbox.paint(state, SETTLED, Output::Xrgb(&mut outs[k]));
    }
}

/// The median of `samples` cold paints at `easel` after a resize that changes the board's cell
/// and the tray's piece size, as a first paint, a resize across a cell size, a scale change or a
/// game switch meets it: each sample first resizes to `away` and paints there (untimed), then
/// times the resize back to `easel` and its paint, which rasterise every board and tray sprite
/// and the chrome's again.
fn cell_change(
    lightbox: &mut Lightbox,
    state: &GameState,
    easel: Easel,
    away: Easel,
    samples: usize,
) -> Duration {
    let (here, there) = (Tableau::arrange(&easel), Tableau::arrange(&away));
    assert!(
        here.cell != there.cell && here.tray_piece_px() != there.tray_piece_px(),
        "{away:?} changes the cell and the tray's pieces"
    );
    let mut away_out = vec![0; (away.width * away.height) as usize];
    let mut out = vec![0; (easel.width * easel.height) as usize];
    let mut times: Vec<Duration> = (0..samples)
        .map(|_| {
            lightbox.resize(away);
            lightbox.paint(state, SETTLED, Output::Xrgb(&mut away_out));
            let start = Instant::now();
            lightbox.resize(easel);
            lightbox.paint(state, SETTLED, Output::Xrgb(&mut out));
            start.elapsed()
        })
        .collect();
    times.sort();
    times[times.len() / 2]
}

/// Median time per call over `batches` batches of `calls` calls. One-time work in the first
/// batch (sprites rasterised after a reshape, lettering re-lettered) lands in that batch only, so
/// the median leaves it out: rows that must pay it every time (`cell_change`) time single calls.
fn per_call(batches: usize, calls: u32, mut f: impl FnMut()) -> Duration {
    let mut times: Vec<Duration> = (0..batches)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..calls {
                f();
            }
            start.elapsed() / calls
        })
        .collect();
    times.sort();
    times[times.len() / 2]
}

/// A light row slower than this times its dark twin is recorded for phase 6 (spec revision
/// "phase-5 light theme"), not failed.
const RATIO_NOTE: f64 = 1.15;
/// Dark rows faster than this give no meaningful light/dark ratio.
const RATIO_FLOOR: Duration = Duration::from_micros(1);

fn tag(theme: Theme) -> &'static str {
    match theme {
        Theme::Dark => "[dark]",
        Theme::Light => "[light]",
    }
}

/// Every gated row of both themes: each checked at its cap (a light row at its dark twin's), and
/// kept to report each light row's ratio to its dark twin.
#[derive(Default)]
struct Ledger {
    rows: Vec<(Theme, String, Duration)>,
}

impl Ledger {
    fn check(&mut self, theme: Theme, name: &str, took: Duration, budget: Duration) -> bool {
        self.rows.push((theme, name.to_owned(), took));
        check(&format!("{} {name}", tag(theme)), took, budget)
    }

    /// `check_sweep`'s rows (a p90 of frames), kept the same way.
    fn check_sweep(
        &mut self,
        theme: Theme,
        name: &str,
        frames: Option<Vec<Duration>>,
        budget: Duration,
    ) -> bool {
        let frames = frames.map(|mut f| {
            f.sort();
            f
        });
        // Sorted here, not copied (review T11 M4): the p90 is read off it, and `check_sweep`'s
        // `p90` finds it sorted.
        if let Some(p) = frames.as_ref().and_then(|f| f.get(f.len() * 9 / 10)) {
            self.rows.push((theme, name.to_owned(), *p));
        }
        check_sweep(&format!("{} {name}", tag(theme)), frames, budget)
    }

    fn ratios(&self) {
        let mut noted = Vec::new();
        for (theme, name, light) in &self.rows {
            if *theme != Theme::Light {
                continue;
            }
            let dark = self
                .rows
                .iter()
                .find(|(t, n, _)| *t == Theme::Dark && n == name);
            let Some((_, _, dark)) = dark else { continue };
            let ratio = light.as_secs_f64() / dark.as_secs_f64().max(1e-12);
            // Rows under RATIO_FLOOR (the arranges, tens of ns) are timer noise: shown, not
            // noted (review T11 M2).
            if *dark < RATIO_FLOOR {
                println!("light/dark {name}: {ratio:.2} (under {RATIO_FLOOR:?}: noise)");
                continue;
            }
            println!("light/dark {name}: {ratio:.2}");
            if ratio > RATIO_NOTE {
                noted.push(format!("{name}: {ratio:.2}"));
            }
        }
        if noted.is_empty() {
            println!("light/dark: every row within {RATIO_NOTE}");
        } else {
            println!("light/dark over {RATIO_NOTE} (recorded for phase 6, not failed):");
            for n in noted {
                println!("  {n}");
            }
        }
    }
}

fn check(name: &str, took: Duration, budget: Duration) -> bool {
    let ok = took < budget;
    println!(
        "{name}: {took:?} (budget {budget:?}) {}",
        if ok { "ok" } else { "OVER BUDGET" }
    );
    ok
}

/// The 90th percentile and the worst of `frames`; `None` for none (ruling F12).
fn p90(mut frames: Vec<Duration>) -> Option<(Duration, Duration)> {
    frames.sort();
    Some((*frames.get(frames.len() * 9 / 10)?, *frames.last()?))
}

/// Every animation frame (16.7 ms apart, after each turn's first) of `turns` turns of pieces
/// whose turn changes the beams, painted on `lightbox`; `clock` moves on 10 s between turns.
/// `None` when no turn changes the beams.
fn sweep_frames(
    lightbox: &mut Lightbox,
    state: &mut GameState,
    out: &mut [u32],
    clock: &mut f64,
    turns: usize,
) -> Option<Vec<Duration>> {
    let mut frames = Vec::new();
    let tableau = lightbox.tableau().clone();
    let cells: Vec<usize> = (0..BOARD_CELLS)
        .filter(|&i| state.board().cells[i].piece.is_movable())
        .collect();
    let mut swept = 0;
    for &cell in cells.iter().cycle().take(cells.len() * 4) {
        if swept == turns {
            break;
        }
        let before = state.field().clone();
        let c = tableau.cell_centre(cell);
        let (x, y) = (c.x as i32, c.y as i32);
        state.handle_on(lightbox, InputEvent::MouseDown { x, y, alt: true });
        state.handle_on(lightbox, InputEvent::MouseUp { x, y, alt: true });
        *clock += 10_000.0;
        lightbox.paint(state, *clock, Output::Xrgb(out));
        if *state.field() == before {
            continue;
        }
        swept += 1;
        frames.extend(cue_frames(lightbox, state, out, clock));
    }
    (!frames.is_empty()).then_some(frames)
}

/// A level change onto `slot` (the cheat key and `=` from a fresh state at the slot before it,
/// opened as a player would, winning each level on the way; at slot 0, onto slot 1): its first
/// frame (the old board kept, the new one painted whole) and every frame of its cross-fade and
/// sweep after it. `None` if the level cannot be reached.
fn level_frames(
    lightbox: &mut Lightbox,
    data: &GameData,
    slot: usize,
    out: &mut [u32],
    clock: &mut f64,
) -> Option<(Duration, Vec<Duration>)> {
    let mut state = GameState::new(data, SaveData::default());
    while state.slot() + 1 < slot {
        let before = state.slot();
        state.handle(InputEvent::Key(state.rules().cheat_key));
        state.handle(InputEvent::Key(b'='));
        if state.slot() == before {
            return None;
        }
    }
    *clock += 10_000.0;
    lightbox.paint(&state, *clock, Output::Xrgb(out));
    let before = state.slot();
    state.handle(InputEvent::Key(state.rules().cheat_key));
    state.handle(InputEvent::Key(b'='));
    if state.slot() == before {
        return None;
    }
    *clock += 10_000.0;
    let start = Instant::now();
    lightbox.paint(&state, *clock, Output::Xrgb(out));
    let first = start.elapsed();
    Some((first, cue_frames(lightbox, &state, out, clock)))
}

/// The median first frame of `LEVEL_REPEATS` level changes onto `slot`, and all their animation
/// frames.
fn level_changes(
    lightbox: &mut Lightbox,
    data: &GameData,
    slot: usize,
    out: &mut [u32],
    clock: &mut f64,
) -> Option<(Duration, Vec<Duration>)> {
    let mut firsts = Vec::new();
    let mut frames = Vec::new();
    for _ in 0..LEVEL_REPEATS {
        let (first, more) = level_frames(lightbox, data, slot, out, clock)?;
        firsts.push(first);
        frames.extend(more);
    }
    firsts.sort();
    Some((firsts[firsts.len() / 2], frames))
}

/// Every animation frame (16.7 ms apart, after its first) of the level's stored solution pasted
/// onto it restarted: its sweep and every target it lights bursting. `None` without a stored
/// solution or with one that lights nothing.
fn paste_frames(
    lightbox: &mut Lightbox,
    state: &mut GameState,
    out: &mut [u32],
    clock: &mut f64,
) -> Option<Vec<Duration>> {
    let text = stored_solution(state.data(), state.slot())?;
    state.handle(InputEvent::Key(b'r'));
    *clock += 10_000.0;
    lightbox.paint(state, *clock, Output::Xrgb(out));
    state.handle(InputEvent::Paste(text));
    *clock += 10_000.0;
    lightbox.paint(state, *clock, Output::Xrgb(out));
    lightbox.choreography().bursts().first()?;
    Some(cue_frames(lightbox, state, out, clock))
}

/// Times every frame (16.7 ms apart) until the lightbox sleeps; a cue still running after 400
/// frames panics rather than hanging the bench.
fn cue_frames(
    lightbox: &mut Lightbox,
    state: &GameState,
    out: &mut [u32],
    clock: &mut f64,
) -> Vec<Duration> {
    let mut frames = Vec::new();
    while lightbox.next_wake(*clock).is_some() && frames.len() < 400 {
        *clock += halation::FRAME_MS;
        let start = Instant::now();
        lightbox.paint(state, *clock, Output::Xrgb(out));
        frames.push(start.elapsed());
    }
    assert!(
        lightbox.next_wake(*clock).is_none(),
        "a cue that never settles"
    );
    frames
}

/// Every animation frame (16.7 ms apart, after its first) of a cheat-key win of the level
/// restarted: its packets, its targets' pulses and its header written on. `None` if the cheat
/// key does not win.
fn win_frames(
    lightbox: &mut Lightbox,
    state: &mut GameState,
    out: &mut [u32],
    clock: &mut f64,
) -> Option<Vec<Duration>> {
    state.handle(InputEvent::Key(b'r'));
    *clock += 10_000.0;
    lightbox.paint(state, *clock, Output::Xrgb(out));
    state.handle(InputEvent::Key(state.rules().cheat_key));
    if !state.is_won() {
        return None;
    }
    *clock += 10_000.0;
    lightbox.paint(state, *clock, Output::Xrgb(out));
    Some(cue_frames(lightbox, state, out, clock))
}

/// Checks the animation row `name` against `budget`, or reports that nothing animated (no turn
/// changed the beams, or no paste lit a target; not gated). A cue that must animate and ran no
/// frame (`Some` of none: a level change, the sheet) fails the gate.
fn check_sweep(name: &str, frames: Option<Vec<Duration>>, budget: Duration) -> bool {
    let Some(frames) = frames else {
        println!("{name}: nothing to animate");
        return true;
    };
    let n = frames.len();
    match p90(frames) {
        Some((p90, worst)) => check(
            &format!("{name} (p90 of {n}; worst {worst:?})"),
            p90,
            budget,
        ),
        None => {
            println!("{name}: no animation frame (FAILED: this cue must animate)");
            false
        }
    }
}

/// The slot, of the first 50, with the highest `score` (the first of equals).
fn pick(data: &GameData, score: impl Fn(&Board) -> usize) -> usize {
    (0..data.level_order.len().min(50))
        .max_by_key(|&s| {
            let board = Board::from_level(data.slot_level(s).unwrap());
            (score(&board), usize::MAX - s)
        })
        .unwrap()
}

/// `slot`, solved where a stored solution exists.
fn solved(data: &GameData, slot: usize) -> GameState<'_> {
    let mut state = GameState::new(data, SaveData::default());
    state.goto_slot(slot);
    if let Some(text) = stored_solution(data, slot) {
        state.handle(InputEvent::Paste(text));
    }
    state
}

/// A press at `from`'s centre, a drag to `to`'s and a release there.
fn carry(lightbox: &Lightbox, state: &mut GameState, from: usize, to: usize) {
    let tableau = lightbox.tableau();
    let (a, b) = (tableau.cell_centre(from), tableau.cell_centre(to));
    let (a, b) = ((a.x as i32, a.y as i32), (b.x as i32, b.y as i32));
    for event in [
        InputEvent::MouseDown {
            x: a.0,
            y: a.1,
            alt: false,
        },
        InputEvent::MouseDragged { x: b.0, y: b.1 },
        InputEvent::MouseUp {
            x: b.0,
            y: b.1,
            alt: false,
        },
    ] {
        state.handle_on(lightbox, event);
    }
}

/// The median paint after a board piece is dropped onto an empty tray slot (the board, its light
/// and the tray repainted), reported without a budget; the piece goes back after each drop, and
/// a level with no piece on its board borrows one from the tray first. `None` if nothing moves.
fn tray_drop(lightbox: &mut Lightbox, state: &mut GameState, out: &mut [u32]) -> Option<Duration> {
    let cells = |state: &GameState| state.board().cells;
    let tray = (0..VISIBLE_TOOLBOX_SLOTS).map(toolbox_index);
    let empty_slot = tray
        .clone()
        .find(|&i| cells(state)[i].piece == Piece::Empty)?;
    let mut borrowed = None;
    let source = match (0..BOARD_CELLS).find(|&i| cells(state)[i].piece.is_movable()) {
        Some(cell) => cell,
        None => {
            let slot = tray.clone().find(|&i| cells(state)[i].piece.is_movable())?;
            let cell = (0..BOARD_CELLS).find(|&i| cells(state)[i].piece == Piece::Empty)?;
            carry(lightbox, state, slot, cell);
            borrowed = Some((cell, slot));
            cell
        }
    };
    lightbox.paint(state, SETTLED, Output::Xrgb(out));
    let mut times = Vec::new();
    for _ in 0..21 {
        carry(lightbox, state, source, empty_slot);
        if cells(state)[empty_slot].piece == Piece::Empty {
            return None;
        }
        let start = Instant::now();
        lightbox.paint(state, SETTLED, Output::Xrgb(out));
        times.push(start.elapsed());
        carry(lightbox, state, empty_slot, source);
        lightbox.paint(state, SETTLED, Output::Xrgb(out));
    }
    if let Some((cell, slot)) = borrowed {
        carry(lightbox, state, cell, slot);
        lightbox.paint(state, SETTLED, Output::Xrgb(out));
    }
    times.sort();
    Some(times[times.len() / 2])
}

/// A rotation of the first movable board piece, painted as its cue's first frame (`clock` moves
/// on 10 s, past the last one's cues): reported. The web's edit row gates it.
fn rotation(
    lightbox: &mut Lightbox,
    state: &mut GameState,
    out: &mut [u32],
    clock: &mut f64,
) -> Option<Duration> {
    let tableau = lightbox.tableau().clone();
    let cell = (0..BOARD_CELLS).find(|&i| state.board().cells[i].piece.is_movable())?;
    let c = tableau.cell_centre(cell);
    let (x, y) = (c.x as i32, c.y as i32);
    state.handle_on(lightbox, InputEvent::MouseDown { x, y, alt: true });
    state.handle_on(lightbox, InputEvent::MouseUp { x, y, alt: true });
    *clock += 10_000.0;
    let start = Instant::now();
    lightbox.paint(state, *clock, Output::Xrgb(out));
    Some(start.elapsed())
}

/// The median paint after a win (the cheat key, from the level restarted; it wins without moving
/// a piece): the card (its header and Next ▶) and, the first time, the picker. Reported without
/// a budget; `None` if the cheat key does not win.
fn win(lightbox: &mut Lightbox, state: &mut GameState, out: &mut [u32]) -> Option<Duration> {
    let cheat = state.rules().cheat_key;
    let mut times = Vec::new();
    for _ in 0..21 {
        state.handle(InputEvent::Key(b'r'));
        lightbox.paint(state, SETTLED, Output::Xrgb(out));
        state.handle(InputEvent::Key(cheat));
        if !state.is_won() {
            return None;
        }
        let start = Instant::now();
        lightbox.paint(state, SETTLED, Output::Xrgb(out));
        times.push(start.elapsed());
    }
    times.sort();
    Some(times[times.len() / 2])
}

/// The median paint that opens the level sheet at 1600 × 1200 @2 (800 × 600 pt: 1280 × 800 has
/// an inline picker, this a sheet): the whole canvas recomposed with the sheet drawn over it,
/// reported without a budget (the web's time is gated with `Session::command`); and its opening
/// and closing animation frames (`sheet_frames`).
fn sheet_open(state: &GameState, clock: &mut f64, theme: Theme) -> (Duration, Vec<Duration>) {
    let easel = Easel::new(1600, 1200, 2.0);
    let mut lightbox = Lightbox::new(easel, theme, false);
    let mut out = vec![0u32; 1600 * 1200];
    lightbox.paint(state, SETTLED, Output::Xrgb(&mut out));
    let mut times: Vec<Duration> = (0..21)
        .map(|_| {
            lightbox.command(Control::CloseSheet);
            lightbox.paint(state, SETTLED, Output::Xrgb(&mut out));
            let start = Instant::now();
            assert!(lightbox.command(Control::Levels), "a sheet picker");
            lightbox.paint(state, SETTLED, Output::Xrgb(&mut out));
            start.elapsed()
        })
        .collect();
    times.sort();
    lightbox.command(Control::CloseSheet);
    lightbox.paint(state, SETTLED, Output::Xrgb(&mut out));
    let frames = sheet_frames(&mut lightbox, state, &mut out, clock);
    (times[times.len() / 2], frames)
}

/// Every animation frame (16.7 ms apart, after each cue's first) of the level sheet opened and
/// closed `LEVEL_REPEATS` times on `lightbox` (shut, with a sheet picker): its panel fading and
/// rising in, then fading and sinking out.
fn sheet_frames(
    lightbox: &mut Lightbox,
    state: &GameState,
    out: &mut [u32],
    clock: &mut f64,
) -> Vec<Duration> {
    let mut frames = Vec::new();
    for _ in 0..LEVEL_REPEATS {
        for control in [Control::Levels, Control::CloseSheet] {
            assert!(lightbox.command(control), "{control:?}: a sheet picker");
            *clock += 10_000.0;
            lightbox.paint(state, *clock, Output::Xrgb(out));
            frames.extend(cue_frames(lightbox, state, out, clock));
        }
    }
    frames
}

/// The board light's develop at `easel`, per call: whole, and by patches after one cell's light
/// changed (the board's centre cell; `develop_patches` adds its glow and bloom windows). Reported
/// without a budget.
fn light_develops(state: &GameState, easel: Easel, theme: Theme) -> (Duration, Duration) {
    let tableau = Tableau::arrange(&easel);
    let palette = theme.palette();
    let mut room = Darkroom::new(&tableau, palette);
    room.paint_all(&tableau, palette, &Scene::of(state));
    let cell = [tableau.cell_rect(board_index(7, 7))];
    let whole = per_call(21, 10, || {
        black_box(room.develop_light(None));
    });
    let patched = per_call(21, 10, || {
        black_box(room.develop_light(Some(&cell)));
    });
    (whole, patched)
}

fn bench_level(
    criterion: &mut Criterion,
    ledger: &mut Ledger,
    theme: Theme,
    label: &str,
    mut state: GameState,
) -> bool {
    let easel = Easel::new(WIDTH as u32, HEIGHT as u32, 2.0);
    let mut lightbox = Lightbox::new(easel, theme, false);
    let t = tag(theme);
    let mut outs = outputs(WIDTH, HEIGHT);
    let mut phone_outs = outputs(PHONE_WIDTH, PHONE_HEIGHT);
    let arrange_name = format!("{label}: arrange 1280×800@2");
    let cold_name = format!("{label}: cold paint 2560×1600 (resize)");
    let level_name = format!("{label}: level change 2560×1600");
    let phone_name = format!("{label}: cold paint 780×1480 (phone)");
    let first_name = format!("{label}: cold paint 2560×1600 (cell-changing resize)");
    let phone_first_name = format!("{label}: cold paint 780×1480 (phone, cell-changing resize)");
    let drag_name = format!("{label}: drag frame 2560×1600");
    criterion.bench_function(&format!("{t} {arrange_name}"), |b| {
        b.iter(|| Tableau::arrange(black_box(&easel)))
    });
    let mut desk = cold(WIDTH, HEIGHT, false);
    criterion.bench_function(&format!("{t} {cold_name}"), |b| {
        b.iter(|| desk(&mut lightbox, &state, &mut outs))
    });
    // Arrange takes tens of ns: 100k calls make each batch about 1 ms or more.
    let arrange = per_call(21, 100_000, || {
        black_box(Tableau::arrange(black_box(&easel)));
    });
    let reach_name = format!("{label}: Reach::compute");
    let reach = per_call(21, 10_000, || {
        black_box(halation::Reach::compute(
            black_box(state.board()),
            state.field(),
        ));
    });
    let cold_time = per_call(11, 6, || desk(&mut lightbox, &state, &mut outs));
    // A level change: every layer repainted at an unchanged size (no reshape).
    lightbox.resize(easel);
    let mut out = vec![0u32; WIDTH * HEIGHT];
    let level = |lightbox: &mut Lightbox, state: &GameState, out: &mut Vec<u32>| {
        lightbox.reset();
        lightbox.paint(state, SETTLED, Output::Xrgb(out));
    };
    criterion.bench_function(&format!("{t} {level_name}"), |b| {
        b.iter(|| level(&mut lightbox, &state, &mut out))
    });
    let level_time = per_call(11, 6, || level(&mut lightbox, &state, &mut out));
    let mut phone = cold(PHONE_WIDTH, PHONE_HEIGHT, true);
    criterion.bench_function(&format!("{t} {phone_name}"), |b| {
        b.iter(|| phone(&mut lightbox, &state, &mut phone_outs))
    });
    let phone_time = per_call(11, 6, || phone(&mut lightbox, &state, &mut phone_outs));
    // Cell-changing resizes: 2560 × 1538 has cell 92 and tray pieces 86 px (96 and 108 at
    // 2560 × 1600); the phone at 792 px has cell 48 and pieces 84 px (47 and 82 at 780).
    let first_time = cell_change(
        &mut lightbox,
        &state,
        easel,
        Easel::new(WIDTH as u32, 1538, 2.0),
        33,
    );
    let phone = Easel::new(PHONE_WIDTH as u32, PHONE_HEIGHT as u32, 2.0).with_touch(true);
    let phone_away = Easel::new(792, PHONE_HEIGHT as u32, 2.0).with_touch(true);
    let phone_first_time = cell_change(&mut lightbox, &state, phone, phone_away, 33);
    lightbox.resize(easel);
    lightbox.paint(&state, SETTLED, Output::Xrgb(&mut out));
    match tray_drop(&mut lightbox, &mut state, &mut out) {
        Some(took) => println!("{t} {label}: tray drop 2560×1600: {took:?} (reported, no budget)"),
        None => println!("{t} {label}: tray drop 2560×1600: no piece to drop"),
    }
    let mut clock = 0.0;
    let mut turns: Vec<Duration> = (0..21)
        .map_while(|_| rotation(&mut lightbox, &mut state, &mut out, &mut clock))
        .collect();
    turns.sort();
    match turns.get(turns.len() / 2) {
        Some(took) => println!("{t} {label}: rotation 2560×1600: {took:?} (reported)"),
        None => println!("{t} {label}: rotation 2560×1600: no board piece to turn"),
    }
    // A drag: pick up the first movable piece, then wiggle between two points over the board.
    let tableau = lightbox.tableau().clone();
    let source = (0..BOARD_CELLS + VISIBLE_TOOLBOX_SLOTS)
        .find(|&i| state.board().cells[i].piece.is_movable())
        .expect("a movable piece");
    let from = tableau.cell_centre(source);
    let (a, b) = (tableau.cell_centre(0), tableau.cell_centre(BOARD_CELLS - 1));
    state.handle_on(
        &lightbox,
        InputEvent::MouseDown {
            x: from.x as i32,
            y: from.y as i32,
            alt: false,
        },
    );
    state.handle_on(
        &lightbox,
        InputEvent::MouseDragged {
            x: a.x as i32,
            y: a.y as i32,
        },
    );
    lightbox.paint(&state, SETTLED, Output::Xrgb(&mut out));
    let mut flip = false;
    let mut drag = |lightbox: &mut Lightbox, state: &mut GameState, out: &mut Vec<u32>| {
        flip = !flip;
        let p = if flip { b } else { a };
        state.handle_on(
            lightbox,
            InputEvent::MouseDragged {
                x: p.x as i32,
                y: p.y as i32,
            },
        );
        lightbox.paint(state, SETTLED, Output::Xrgb(out));
    };
    criterion.bench_function(&format!("{t} {drag_name}"), |bench| {
        bench.iter(|| drag(&mut lightbox, &mut state, &mut out))
    });
    let drag_time = per_call(21, 50, || drag(&mut lightbox, &mut state, &mut out));
    state.handle_on(
        &lightbox,
        InputEvent::MouseUp {
            x: a.x as i32,
            y: a.y as i32,
            alt: false,
        },
    );
    // Animation frames: three sweeps after turns that change the beams, on the desktop and on
    // the phone (before the win row, which restarts the level).
    let anim_name = format!("{label}: animation frame 2560×1600 (sweep)");
    let anim = sweep_frames(&mut lightbox, &mut state, &mut out, &mut clock, 3);
    // The phone's frames on the same lightbox resized (as a rotation would), then back. (A
    // second lightbox here, under fat LTO, made the cold rows above 1–2 ms slower.)
    let phone_out = &mut phone_outs[0];
    lightbox.resize(phone);
    lightbox.paint(&state, SETTLED, Output::Xrgb(phone_out));
    let phone_anim_name = format!("{label}: animation frame 780×1480 (sweep)");
    let phone_anim = sweep_frames(&mut lightbox, &mut state, phone_out, &mut clock, 3);
    let phone_burst_name = format!("{label}: animation frame 780×1480 (paste: bursts)");
    let phone_burst = paste_frames(&mut lightbox, &mut state, phone_out, &mut clock);
    let phone_win_name = format!("{label}: animation frame 780×1480 (win)");
    let phone_win = win_frames(&mut lightbox, &mut state, phone_out, &mut clock);
    let phone_level_name = format!("{label}: animation frame 780×1480 (level, p90)");
    let phone_level_first_name = format!("{label}: level change, first frame 780×1480");
    let data = state.data();
    let phone_level = level_changes(&mut lightbox, data, state.slot(), phone_out, &mut clock);
    let phone_solved_name = format!("{label}: level change onto it solved, first frame 780×1480");
    let phone_solved = solved_level_first(&mut lightbox, &mut state, phone_out, &mut clock);
    let phone_sheet_name = format!("{label}: animation frame 780×1480 (sheet, p90)");
    let phone_sheet = sheet_frames(&mut lightbox, &state, phone_out, &mut clock);
    lightbox.resize(easel);
    lightbox.paint(&state, SETTLED, Output::Xrgb(&mut out));
    let burst_name = format!("{label}: animation frame 2560×1600 (paste: bursts)");
    let burst = paste_frames(&mut lightbox, &mut state, &mut out, &mut clock);
    let win_name = format!("{label}: animation frame 2560×1600 (win)");
    let win_anim = win_frames(&mut lightbox, &mut state, &mut out, &mut clock);
    let level_anim_name = format!("{label}: animation frame 2560×1600 (level, p90)");
    let level_first_name = format!("{label}: level change, first frame 2560×1600");
    let level = level_changes(&mut lightbox, data, state.slot(), &mut out, &mut clock);
    let solved_name = format!("{label}: level change onto it solved, first frame 2560×1600");
    let solved = solved_level_first(&mut lightbox, &mut state, &mut out, &mut clock);
    match win(&mut lightbox, &mut state, &mut out) {
        Some(took) => println!("{t} {label}: win 2560×1600: {took:?} (reported, no budget)"),
        None => println!("{t} {label}: win 2560×1600: the cheat key does not win"),
    }
    let (sheet_first, sheet) = sheet_open(&state, &mut clock, theme);
    println!("{t} {label}: sheet open frame 1600×1200@2: {sheet_first:?} (reported, no budget)");
    let sheet_name = format!("{label}: animation frame 1600×1200@2 (sheet, p90)");
    for (size, easel) in [("2560×1600", easel), ("780×1480 (phone)", phone)] {
        let (whole, patched) = light_develops(&state, easel, theme);
        println!(
            "{t} {label}: light develop {size}: whole {whole:?}, one cell by patches {patched:?} \
             (reported, no budget)"
        );
    }
    ledger.check(theme, &arrange_name, arrange, ARRANGE_BUDGET)
        & ledger.check(theme, &reach_name, reach, REACH_BUDGET)
        & ledger.check(theme, &cold_name, cold_time, COLD_BUDGET)
        & ledger.check(theme, &level_name, level_time, COLD_BUDGET)
        & ledger.check(theme, &phone_name, phone_time, PHONE_COLD_BUDGET)
        & ledger.check(theme, &first_name, first_time, COLD_BUDGET)
        & ledger.check(
            theme,
            &phone_first_name,
            phone_first_time,
            PHONE_COLD_BUDGET,
        )
        & ledger.check(theme, &drag_name, drag_time, DRAG_BUDGET)
        & ledger.check_sweep(theme, &anim_name, anim, ANIM_BUDGET)
        & ledger.check_sweep(theme, &phone_anim_name, phone_anim, PHONE_ANIM_BUDGET)
        & ledger.check_sweep(theme, &burst_name, burst, ANIM_BUDGET)
        & ledger.check_sweep(theme, &phone_burst_name, phone_burst, PHONE_ANIM_BUDGET)
        & ledger.check_sweep(theme, &win_name, win_anim, ANIM_BUDGET)
        & ledger.check_sweep(theme, &phone_win_name, phone_win, PHONE_ANIM_BUDGET)
        & ledger.check_sweep(theme, &sheet_name, Some(sheet), ANIM_BUDGET)
        & ledger.check_sweep(
            theme,
            &phone_sheet_name,
            Some(phone_sheet),
            PHONE_ANIM_BUDGET,
        )
        & check_level(
            ledger,
            theme,
            (&level_first_name, &level_anim_name),
            level,
            (COLD_BUDGET, ANIM_BUDGET),
        )
        & check_level(
            ledger,
            theme,
            (&phone_level_first_name, &phone_level_name),
            phone_level,
            (PHONE_COLD_BUDGET, PHONE_ANIM_BUDGET),
        )
        & check_ran(ledger, theme, &solved_name, solved, COLD_BUDGET)
        & check_ran(
            ledger,
            theme,
            &phone_solved_name,
            phone_solved,
            PHONE_COLD_BUDGET,
        )
}

/// Checks a level change's first frame and its animation frames against `budgets`; a level the
/// walk cannot reach fails the gate (a regression in the cheat key, unlocking or `=`).
fn check_level(
    ledger: &mut Ledger,
    theme: Theme,
    (first_name, anim_name): (&str, &str),
    level: Option<(Duration, Vec<Duration>)>,
    (first_budget, anim_budget): (Duration, Duration),
) -> bool {
    match level {
        Some((first, frames)) => {
            ledger.check(theme, first_name, first, first_budget)
                & ledger.check_sweep(theme, anim_name, Some(frames), anim_budget)
        }
        None => {
            println!(
                "{} {first_name}: the level cannot be reached (FAILED: the walk should reach it)",
                tag(theme)
            );
            false
        }
    }
}

/// Checks a first frame row that must have run (`None` means its setup failed: a regression).
fn check_ran(
    ledger: &mut Ledger,
    theme: Theme,
    name: &str,
    took: Option<Duration>,
    budget: Duration,
) -> bool {
    match took {
        Some(took) => ledger.check(theme, name, took, budget),
        None => {
            println!(
                "{} {name}: did not run (FAILED: the level did not open solved)",
                tag(theme)
            );
            false
        }
    }
}

/// The median first frame of `LEVEL_REPEATS` level changes back onto `state`'s level, solved:
/// from the level beside it (`-`, or `=` at slot 0), `=` (or `-`) and its stored solution pasted
/// before the paint, so it opens with every piece in place and its beams lit, the heaviest
/// level change a player meets. `None` if the level beside it does not open or the solution
/// does not win.
fn solved_level_first(
    lightbox: &mut Lightbox,
    state: &mut GameState,
    out: &mut [u32],
    clock: &mut f64,
) -> Option<Duration> {
    let slot = state.slot();
    let text = stored_solution(state.data(), slot)?;
    let (away, back) = if slot > 0 { (b'-', b'=') } else { (b'=', b'-') };
    let mut times = Vec::new();
    for _ in 0..LEVEL_REPEATS {
        state.handle(InputEvent::Key(away));
        if state.slot() == slot {
            return None;
        }
        *clock += 10_000.0;
        lightbox.paint(state, *clock, Output::Xrgb(out));
        state.handle(InputEvent::Key(back));
        state.handle(InputEvent::Paste(text.clone()));
        if state.slot() != slot || !state.is_won() {
            return None;
        }
        *clock += 10_000.0;
        let start = Instant::now();
        lightbox.paint(state, *clock, Output::Xrgb(out));
        times.push(start.elapsed());
    }
    times.sort();
    Some(times[times.len() / 2])
}

fn bench_game(criterion: &mut Criterion, ledger: &mut Ledger, game: Game, theme: Theme) -> bool {
    let data = spectrograph::decode(game, &game.read_original().unwrap()).unwrap();
    let densest = pick(&data, |b| {
        b.cells[..BOARD_CELLS]
            .iter()
            .filter(|c| c.piece != Piece::Empty)
            .count()
    });
    let walliest = pick(&data, |b| {
        b.cells[..BOARD_CELLS]
            .iter()
            .filter(|c| c.piece == Piece::Wall)
            .count()
    });
    let mut ok = bench_level(
        criterion,
        ledger,
        theme,
        &format!("{game}, densest (slot {densest})"),
        solved(&data, densest),
    );
    if walliest != densest {
        ok &= bench_level(
            criterion,
            ledger,
            theme,
            &format!("{game}, most walls (slot {walliest})"),
            solved(&data, walliest),
        );
    }
    ok
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .configure_from_args()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    let mut ledger = Ledger::default();
    let mut ok = true;
    for game in Game::ALL {
        for theme in Theme::ALL {
            ok &= bench_game(&mut criterion, &mut ledger, game, theme);
        }
    }
    criterion.final_summary();
    ledger.ratios();
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
