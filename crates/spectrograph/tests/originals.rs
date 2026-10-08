use spectrograph::{DecodeError, Game, Image};
use std::{fs, path::PathBuf};

fn original(game: Game) -> Vec<u8> {
    let originals = std::env::var_os("CHROMATRON_ORIGINALS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../chromatron_mac"));
    let path = game.binary_path(&originals);
    fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} — put the original apps in chromatron_mac/",
            path.display()
        )
    })
}

/// (game, section, vmaddr, size) measured from the originals with radare2.
const SECTIONS: [(Game, &str, u32, usize); 12] = [
    (Game::C1, "__text", 0x22c0, 0x484c),
    (Game::C1, "__cstring", 0x6b78, 0xc18),
    (Game::C1, "__data", 0x8000, 0xabe0),
    (Game::C2, "__text", 0x2190, 0x4cc4),
    (Game::C2, "__cstring", 0x6ec0, 0x8cc),
    (Game::C2, "__data", 0x8000, 0xa1e8),
    (Game::C3, "__text", 0x1ff0, 0x503c),
    (Game::C3, "__cstring", 0x78ac, 0x700),
    (Game::C3, "__data", 0x8000, 0xe3e4),
    (Game::C4, "__text", 0x1d70, 0x5014),
    (Game::C4, "__cstring", 0x6df0, 0x99c),
    (Game::C4, "__data", 0x8000, 0xd418),
];

#[test]
fn loads_every_original_with_expected_sections() {
    for (game, name, vmaddr, size) in SECTIONS {
        let bytes = original(game);
        let image = Image::load(game, &bytes).unwrap();
        let segment = if name == "__data" { "__DATA" } else { "__TEXT" };
        let section = image.section(segment, name).unwrap();
        assert_eq!(
            (section.vmaddr, section.data.len()),
            (vmaddr, size),
            "{game} {name}"
        );
    }
}

#[test]
fn reads_by_virtual_address() {
    let bytes = original(Game::C1);
    let image = Image::load(Game::C1, &bytes).unwrap();
    // "You win!" lives at file offset 0x5ce0 = vmaddr 0x6ce0 in C1.
    assert_eq!(image.read(0x6ce0, 8), Some(&b"You win!"[..]));
    assert_eq!(image.read(0x0000_0010, 4), None);
    assert_eq!(image.read(0x6ce0, usize::MAX), None);
}

#[test]
fn rejects_modified_binary() {
    let mut bytes = original(Game::C1);
    bytes[0x5ce0] ^= 0xff;
    let err = Image::load(Game::C1, &bytes).unwrap_err();
    assert!(matches!(
        err,
        DecodeError::HashMismatch { game: Game::C1, .. }
    ));
    let message = err.to_string();
    assert!(message.contains("Chromatron 1 v1.15"), "{message}");
    assert!(message.contains(Game::C1.sha256()), "{message}");
}

#[test]
fn rejects_wrong_game() {
    let bytes = original(Game::C2);
    assert!(matches!(
        Image::load(Game::C1, &bytes),
        Err(DecodeError::HashMismatch { game: Game::C1, .. })
    ));
}

#[test]
fn reports_missing_section() {
    let bytes = original(Game::C1);
    let image = Image::load(Game::C1, &bytes).unwrap();
    assert_eq!(
        image.section("__TEXT", "__nope").unwrap_err(),
        DecodeError::MissingSection {
            game: Game::C1,
            segment: "__TEXT",
            section: "__nope"
        }
    );
}

use spectrograph::cstrings;

fn string_at(game: Game, vmaddr: u32) -> String {
    let bytes = original(game);
    let image = Image::load(game, &bytes).unwrap();
    cstrings(&image)
        .unwrap()
        .into_iter()
        .find(|s| s.vmaddr == vmaddr)
        .unwrap_or_else(|| panic!("{game}: no string starts at {vmaddr:#x}"))
        .text
}

#[test]
fn finds_known_strings_at_known_addresses() {
    assert_eq!(string_at(Game::C1, 0x6ce0), "You win!");
    assert!(string_at(Game::C1, 0x6d40).starts_with("Drag the REFLECTOR"));
    assert_eq!(string_at(Game::C2, 0x7024), "You win!");
    assert!(string_at(Game::C2, 0x7618).starts_with("The COMPLEMENTOR"));
    assert!(string_at(Game::C3, 0x7e74).starts_with("There and Back Again"));
    assert!(string_at(Game::C4, 0x7114).starts_with("Wheels Within Wheels"));
}
