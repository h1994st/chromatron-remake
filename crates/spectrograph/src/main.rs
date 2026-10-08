use std::{env, fs, path::PathBuf, process::ExitCode};

use spectrograph::{Game, Image, cstrings};

const USAGE: &str = "usage: spectrograph <path|dump> <game 1-4>";

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
    let [command, game] = args else {
        return Err(USAGE.into());
    };
    let game = parse_game(game)?;
    let path = game.binary_path(&originals_dir());
    match command.as_str() {
        "path" => println!("{}", path.display()),
        "dump" => {
            let bytes = fs::read(&path).map_err(|e| {
                format!(
                    "{}: {e}; put the original apps in chromatron_mac/ or set CHROMATRON_ORIGINALS",
                    path.display()
                )
            })?;
            let image = Image::load(game, &bytes).map_err(|e| e.to_string())?;
            println!("# {game}\n\n## sections");
            for s in image.sections() {
                println!(
                    "{:<8} {:<20} {:#010x} {:#8x}",
                    s.segment,
                    s.name,
                    s.vmaddr,
                    s.data.len()
                );
            }
            println!("\n## strings");
            for s in cstrings(&image).map_err(|e| e.to_string())? {
                println!("{:#010x}  {:?}", s.vmaddr, s.text);
            }
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn parse_game(arg: &str) -> Result<Game, String> {
    arg.parse::<u8>()
        .ok()
        .and_then(Game::from_number)
        .ok_or_else(|| format!("unknown game {arg:?}; expected 1, 2, 3 or 4"))
}

fn originals_dir() -> PathBuf {
    env::var_os("CHROMATRON_ORIGINALS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../chromatron_mac"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectrograph::Game;

    #[test]
    fn parses_game_numbers() {
        assert_eq!(parse_game("1"), Ok(Game::C1));
        assert_eq!(parse_game("4"), Ok(Game::C4));
    }

    #[test]
    fn rejects_bad_game_arguments() {
        for bad in ["0", "5", "x", ""] {
            let err = parse_game(bad).unwrap_err();
            assert!(err.contains("expected 1, 2, 3 or 4"), "{bad}: {err}");
        }
    }
}
