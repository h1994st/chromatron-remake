//! `lumen shot`: renders a level of an original game to a PNG for review by eye.

use std::{env, path::Path, process::ExitCode};

use lumen::{
    Board, FRAME_HEIGHT, FRAME_WIDTH, GameState, InputEvent, Redraw, SaveData, encode_solution,
    render,
};
use spectrograph::{Game, GameData, decode, write_rgb_png};

const USAGE: &str =
    "usage: lumen shot <game 1-4> <level 1-50> <out.png> [stored | <copied solution>]";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let [command, game, level, out, solution @ ..] = args else {
        return Err(USAGE.into());
    };
    if command != "shot" || solution.len() > 1 {
        return Err(USAGE.into());
    }
    let game = Game::parse_arg(game)?;
    let slot = parse_level(level)?;
    let data = decode(game, &game.read_original()?).map_err(|e| e.to_string())?;
    let mut state = GameState::new(&data, SaveData::default());
    state.goto_slot(slot);
    if let Some(solution) = solution.first() {
        let text = if solution == "stored" {
            stored_solution(&data, slot)?
        } else {
            solution.clone()
        };
        if state.handle(InputEvent::Paste(text.clone())).redraw == Redraw::None {
            return Err(format!("{text:?} is not a solution for level {}", slot + 1));
        }
    }
    let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    let texts = render(&state, &mut frame);
    write_rgb_png(
        Path::new(out),
        FRAME_WIDTH as u32,
        FRAME_HEIGHT as u32,
        &frame,
    )
    .map_err(|e| format!("{out}: {e}"))?;
    let result = if state.is_won() { "won" } else { "not won" };
    println!(
        "{out}: level {} (id {}), {result}",
        slot + 1,
        state.level_id()
    );
    for text in texts.as_slice() {
        println!("  text {:?} in {:?}", text.text, text.rect);
    }
    Ok(())
}

/// A strip number 1–50, as a 0-based slot.
fn parse_level(arg: &str) -> Result<usize, String> {
    match arg.parse::<usize>() {
        Ok(n @ 1..=50) => Ok(n - 1),
        _ => Err(format!("unknown level {arg:?}; expected 1 to 50")),
    }
}

/// The level's stored placement (data.md D3) as clipboard text.
fn stored_solution(data: &GameData, slot: usize) -> Result<String, String> {
    let level = data.slot_level(slot).ok_or("no such level")?;
    encode_solution(&Board::from_level(level), slot).ok_or_else(|| "too many pieces to copy".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_strip_numbers() {
        assert_eq!(parse_level("1"), Ok(0));
        assert_eq!(parse_level("50"), Ok(49));
        for bad in ["0", "51", "x", ""] {
            assert!(
                parse_level(bad).unwrap_err().contains("expected 1 to 50"),
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_malformed_commands() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(run(&args(&["shot", "1", "5"])), Err(USAGE.into()));
        assert_eq!(run(&args(&["snap", "1", "5", "x.png"])), Err(USAGE.into()));
        assert_eq!(
            run(&args(&["shot", "1", "5", "x.png", "a", "b"])),
            Err(USAGE.into())
        );
        assert_eq!(
            run(&args(&["shot", "5", "5", "x.png"])),
            Err("unknown game \"5\"; expected 1, 2, 3 or 4".into())
        );
    }
}
