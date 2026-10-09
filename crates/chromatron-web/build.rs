//! Decodes all four games from their original binaries and writes them as Rust `static`s into
//! `OUT_DIR`, exactly as the macOS app's build script does, so the wasm embeds every game.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use spectrograph::embed::embed_game;
use spectrograph::{Game, originals_dir};

fn main() -> ExitCode {
    println!("cargo::rerun-if-env-changed=CHROMATRON_ORIGINALS");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    for game in Game::ALL {
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
