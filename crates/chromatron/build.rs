//! Decodes each game's data from its original binary and writes it as Rust `static`s into
//! `OUT_DIR`, so the app embeds the data and never parses anything at runtime.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use spectrograph::embed::embed_game;
use spectrograph::{Game, originals_dir};

/// The games this crate embeds; each one's file is `spectrograph::embed::data_file_name`.
const EMBEDDED: [Game; 4] = Game::ALL;

fn main() -> ExitCode {
    println!("cargo::rerun-if-env-changed=CHROMATRON_ORIGINALS");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    for game in EMBEDDED {
        println!(
            "cargo::rerun-if-changed={}",
            game.binary_path(&originals_dir()).display()
        );
        if let Err(message) = embed_game(game, &out_dir) {
            println!("cargo::error=cannot embed the game data: {message}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
