//! `just bench`: trace and render on real Chromatron 1 and 2 levels, checked against the spec's
//! §6 budgets (Apple Silicon, release build). Exits non-zero if a budget is exceeded.

use std::{
    hint::black_box,
    process::ExitCode,
    time::{Duration, Instant},
};

use criterion::Criterion;
use lumen::{
    BeamField, Board, FRAME_HEIGHT, FRAME_WIDTH, GameState, InputEvent, SaveData, encode_solution,
    render, trace,
};
use spectrograph::{Game, GameData};

const TRACE_BUDGET: Duration = Duration::from_micros(50);
const RENDER_BUDGET: Duration = Duration::from_millis(1);

/// Median time per call over 21 batches of 200 calls.
fn per_call(mut f: impl FnMut()) -> Duration {
    let mut batches: Vec<Duration> = (0..21)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..200 {
                f();
            }
            start.elapsed() / 200
        })
        .collect();
    batches.sort();
    batches[batches.len() / 2]
}

/// The playable level whose solved board takes longest to trace: (slot, board).
fn slowest_level(data: &GameData) -> (usize, Board) {
    let mut field = BeamField::default();
    (0..data.level_order.len())
        .map(|slot| (slot, Board::from_level(data.slot_level(slot).unwrap())))
        .max_by_key(|(_, board)| per_call(|| trace(black_box(board), &mut field)))
        .unwrap()
}

fn check(name: &str, took: Duration, budget: Duration) -> bool {
    let ok = took < budget;
    println!(
        "{name}: {took:?} (budget {budget:?}) {}",
        if ok { "ok" } else { "OVER BUDGET" }
    );
    ok
}

/// Benchmarks one game's slowest level and checks it against the budgets.
fn bench_game(criterion: &mut Criterion, game: Game) -> bool {
    let data = spectrograph::decode(game, &game.read_original().unwrap()).unwrap();
    let (slot, board) = slowest_level(&data);
    let mut state = GameState::new(&data, SaveData::default());
    state.goto_slot(slot);
    state.handle(InputEvent::Paste(encode_solution(&board, slot).unwrap()));
    let mut field = BeamField::default();
    let mut frame = vec![0u32; FRAME_WIDTH * FRAME_HEIGHT];

    let trace_name = format!("{game}: trace level {} solved", slot + 1);
    let render_name = format!("{game}: render");
    criterion.bench_function(&trace_name, |b| {
        b.iter(|| trace(black_box(&board), &mut field))
    });
    criterion.bench_function(&render_name, |b| {
        b.iter(|| render(black_box(&state), &mut frame))
    });

    let trace_ok = check(
        &trace_name,
        per_call(|| trace(black_box(&board), &mut field)),
        TRACE_BUDGET,
    );
    let render_ok = check(
        &render_name,
        per_call(|| _ = render(black_box(&state), &mut frame)),
        RENDER_BUDGET,
    );
    trace_ok && render_ok
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default().configure_from_args();
    let mut ok = true;
    for game in [Game::C1, Game::C2] {
        ok &= bench_game(&mut criterion, game);
    }
    criterion.final_summary();
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
