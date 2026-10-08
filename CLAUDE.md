# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A faithful native macOS (Apple Silicon) port, in Rust, of Sean Barrett's Chromatron 1–4 laser puzzle games. The originals in `chromatron_mac/` are 32-bit PowerPC Mach-O Carbon apps (C1 v1.15, C2 v1.4, C3 v1.1, C4 v1.0). Everything — game rules, levels, sprites, text — is recovered from our own decompilation of those binaries. The reference look is the **Mac** version (not Win32).

Status: Plan 4 done: Chromatron 2 is decoded, played by its own `RuleSet` and shipped as a second app (`just run 2`, `just bundle` → `dist/Chromatron.app` and `dist/Chromatron 2.app`). Next: Chromatron 3 (diff the RE against C1/C2, extend the `RuleSet`, `chromatron3` bin). The full design spec lives locally at `docs/superpowers/specs/2026-10-07-chromatron-port-design.md` (read it before implementing anything).

## Hard rules

- **No code or data from other ports of Chromatron** (their source, decompiled output, extracted data, scripts). This port is independent. Resolve unclear behavior from our own disassembly.
- **No copyrighted game data in git.** `chromatron_mac/`, the zip, and `re/out/` (decompiled output) are git-ignored. Levels/sprites/strings are decoded from the local originals at build time and embedded in the binary. Chromatron 2–4 were paid shareware.
- **`docs/` is local-only** (git-ignored) — never commit specs or plans.
- **Commands go in the `justfile`**, not shell scripts.
- **Homebrew deps go in `Brewfile` only via `brew bundle add <name>`** (`--cask` for casks) — never edit the Brewfile by hand. Rust comes from rustup, not Homebrew.
- **Avoid generic names** for crates/modules (e.g. not `extract`).
- **Performance is a requirement from day 1**: no allocations in per-frame/trace paths, event-driven redraw, check `just bench` against the budgets in the spec.
- Bundle IDs: `me.shengtuo.chromatron1` … `me.shengtuo.chromatron4`.
- Commits and PRs carry no Claude attribution (configured in `.claude/settings.json`).

## Git conventions

- **Commits:** [Conventional Commits](https://www.conventionalcommits.org/) — `type(scope): subject`, e.g. `feat(lumen): trace beams through splitters`, `chore: add Brewfile`. Scopes are crate or area names (`spectrograph`, `lumen`, `chromatron`, `re`, `just`). Leave the body empty in most cases; if really needed, one or two short sentences.
- **Branches:** [Conventional Branch](https://conventional-branch.github.io/) — `<type>/<short-kebab-description>` with type in `feature`, `bugfix`, `hotfix`, `release`, `chore`, e.g. `feature/c1-level-decoder`.

## Commands (justfile)

| Recipe | Action |
|---|---|
| `just setup` | `brew bundle` (ghidra, just, radare2) |
| `just decompile <game>` | Ghidra headless (`re/ghidra/` scripts) → `re/out/chromatron<game>/` |
| `just dump <game>` | `spectrograph` CLI: print a game's sections, strings and (C1, C2) levels with names and hints as ASCII grids; write `re/out/chromatron<game>/sprites.png` |
| `just run <game>` | `cargo run --release --bin chromatron<game>` (game 1 or 2); `CHROMATRON_PROFILE=1` logs frame timings |
| `just snapshot <game> <png>` | run the app, write its first presented frame (text included) to a PNG and quit; never writes the save file |
| `just shot <game> <level> [solution]` | `lumen shot`: render level 1–50 of game 1 or 2 to `re/out/shots/`; `solution` is `stored` or copied text such as `1-01-hI` |
| `just lint` | `cargo fmt --all --check` + `cargo clippy --workspace --all-targets --features lumen/cli -- -D warnings` |
| `just test` | verifies originals are present, then `cargo test --workspace --features lumen/cli` |
| `just bench` | criterion benches for `trace` and `render` on real levels of games 1 and 2; fails when over the spec's budgets |
| `just bundle` | release builds → `dist/Chromatron.app` and `dist/Chromatron 2.app` (Info.plist, icon and Get Info text from each original), ad-hoc signed |
| `just smoke [game]` | bundle game 1 (default) or 2, launch, check ~0% idle CPU, quit via Apple Event, check the save file was written (touches the real save folder) |
| `cargo run -p spectrograph -- path <game>` | print a game's executable path |

Single test: `cargo test -p <crate> <test_name>`; snapshot review: `cargo insta review`.

## Architecture

Cargo workspace, three crates, strict layering `spectrograph` → `lumen` → `chromatron`:

- **`crates/spectrograph`** — the data types (`GameData`, `Level`, `Record`, `Sprite`, `UiText`, built on `Cow<'static, …>` so a build script can emit them as `static`s) are always compiled; parsing the PPC Mach-O (`object` crate, big-endian) sits behind the `decode` feature; the default is `cli` (implies `decode`, adds PNG output), and library consumers (lumen, the app) depend with `default-features = false`. It decodes levels, hints, palette-indexed 24×24 sprites, the palette and beam colours (as `0x00RRGGBB`) and UI strings via a per-game table of data locations (`DataMap` in `data_map.rs`; `c1.rs`, `c2.rs`). Every `Level` carries its `game` and `name` ("" in C1); `UiText` strings a game lacks are "". Each game pins the SHA-256 of its expected binary; mismatch is a hard error.
- **`crates/lumen`** — pure engine, no platform deps (`spectrograph` with `default-features = false`). `Cell` keeps the raw (type, colour, dir) bytes; `Colour` (3-bit RGB mask) and `Dir` (0–7) are `u8`s. `transfer` (per-piece rules), `trace` (fixed 32-beam lists into a reusable `BeamField`, no allocation; beams carry an entanglement tag), win check, `bsd_qsort` (exact FreeBSD `qsort` for toolbox and clipboard order), unlock rule, save-file and clipboard byte codecs, `GameState` + pure `handle(InputEvent) -> Effects`, and `render` into a 640×480 `&mut [u32]` of `0x00RRGGBB` (softbuffer's format) plus a list of text boxes. Per-game differences live in one `RuleSet` per game (`rule_set.rs`: piece types, splitter variant, unlock rule, cheat key, save file name and bit permutation, saved-board ids, clipboard digit, text-box set); a `Board` carries its `game`, so `trace` and the clipboard codec follow it, while the save codec takes the `RuleSet`. C2's complementor pass runs inside `trace` with stack scratch only. `lumen shot` (feature `cli`, off by default) renders a level to PNG.
- **`crates/chromatron`** — macOS shell: winit 0.30 (`ControlFlow::Wait`) + softbuffer + muda menus (the original nib's, plus app and View menus) + objc2 (NSUserDefaults for `UseMouseWheelToChangeLevel`, NSPasteboard, Core Text with Lucida Grande 13 pt for text boxes, cached per string and rect). `build.rs` decodes the originals (`spectrograph/decode` as a build-dependency only) into `static GAME` data, so the app has no parser. `lumen` renders 640×480; the shell upscales only damaged rects into a device-resolution canvas (integer scale, letterboxed), composites text, and presents. Progress is the original `.dat` format in `~/Library/Application Support/Chromatron N/`. Bins `chromatron1` and `chromatron2`; each later game adds a bin, an `AppSpec` and a row in `build.rs`'s `EMBEDDED`.
- **`re/`** — `ghidra/` (our headless scripts, committed), `notes/chromatronN/{data,logic,platform}.md` plus `*.names.tsv` (our RE findings: function roles, data addresses, formats, quirks, and the names `ApplyNames` applies — committed), `out/` (ignored).

Per-game behavior differs and must come from **that game's own binary**: e.g. C2 changed splitter semantics ("Splitters in the original Chromatron didn't work correctly") and adds complementor/multi-filter/twister; C3 adds dual-reflector/quad-bender; C4 adds logic gates, starburst, mobile teleporter, occupied squares. Games 2–4 are reverse-engineered by diffing against game 1.

Fidelity is verified by spectrograph decode tests, table-driven per-piece rule tests, every original level checked against the tracer and win rule, `insta` ASCII beam-field snapshots per level (derived data, so local only: `crates/lumen/tests/snapshots/` is git-ignored and `.config/insta.yaml` records missing ones), and solved-level fixtures in `tests/solutions/gameN/level-NN.txt` (small codec examples in the original clipboard format, e.g. `2-07-OFHqye`, already in the notes). There is no pixel matching against reference screenshots.
