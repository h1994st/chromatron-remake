use std::{env, fs, path::Path, process::ExitCode};

use spectrograph::{Game, GameData, Image, cstrings, decode, originals_dir, write_rgb_png};

mod dump;

const USAGE: &str = "usage: spectrograph path <game 1-4> | spectrograph dump <game 1-4> [out-dir]";

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
    let (command, game, out_dir) = match args {
        [command, game] => (command, game, None),
        [command, game, out_dir] => (command, game, Some(Path::new(out_dir))),
        _ => return Err(USAGE.into()),
    };
    let game = Game::parse_arg(game)?;
    match command.as_str() {
        "path" if out_dir.is_none() => println!("{}", game.binary_path(&originals_dir()).display()),
        "dump" => {
            let bytes = game.read_original()?;
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
            if game != Game::C1 {
                println!("\n## levels\n(not decoded yet for {game})");
                return Ok(());
            }
            let data = decode(game, &bytes).map_err(|e| e.to_string())?;
            print_levels(&data);
            if let Some(dir) = out_dir {
                write_sprite_sheet(&data, dir)?;
            }
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn print_levels(data: &GameData) {
    println!("\n## levels");
    for (slot, &id) in data.level_order.iter().enumerate() {
        print_level(data, id, &format!("slot {} (id {id})", slot + 1));
    }
    for level in data
        .levels
        .iter()
        .filter(|l| !data.level_order.contains(&l.id))
    {
        print_level(data, level.id, &format!("id {} (unreachable)", level.id));
    }
}

fn print_level(data: &GameData, id: u8, title: &str) {
    let level = data.level(id).expect("level order names a decoded level");
    println!("\n### {title}: {} records", level.records.len());
    if !level.hint.is_empty() {
        println!("hint: {:?}", level.hint);
    }
    print!("{}", dump::level_grid(level));
}

fn write_sprite_sheet(data: &GameData, dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join("sprites.png");
    let (width, height, pixels) = dump::sprite_sheet(data);
    write_rgb_png(&path, width, height, &pixels).map_err(|e| format!("{}: {e}", path.display()))?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_extra_arguments() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(run(&args(&["path", "1", "x"])), Err(USAGE.into()));
        assert_eq!(run(&args(&["dump", "1", "x", "y"])), Err(USAGE.into()));
    }
}
