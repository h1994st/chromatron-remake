# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A faithful native macOS (Apple Silicon) port, in Rust, of Sean Barrett's Chromatron 1–4 laser puzzle games. The originals in `chromatron_mac/` are 32-bit PowerPC Mach-O Carbon apps (C1 v1.15, C2 v1.4, C3 v1.1, C4 v1.0). Everything — game rules, levels, sprites, text — is recovered from our own decompilation of those binaries. The reference look is the **Mac** version (not Win32).

Status: design approved stage; the full design spec lives locally at `docs/superpowers/specs/2026-10-07-chromatron-port-design.md` (read it before implementing anything). The workspace below is the planned layout.

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
| `just dump <game>` | `spectrograph` CLI: levels as ASCII, strings, sprite-sheet PNG → `re/out/` |
| `just run <game>` | run app for game 1–4 |
| `just shot <game> <level> [solution]` | render a level to PNG for visual review |
| `just test` | verifies originals are present, then `cargo test --workspace` |
| `just bench` | criterion benches vs. perf budgets |
| `just bundle` | release build → `dist/Chromatron*.app`, ad-hoc signed |

Single test: `cargo test -p <crate> <test_name>`; snapshot review: `cargo insta review`.

## Architecture

Cargo workspace, three crates, strict layering `spectrograph` → `lumen` → `chromatron`:

- **`crates/spectrograph`** — parses the PPC Mach-O (`object` crate, big-endian) and decodes `GameData` (levels, sprites → RGBA, MacRoman strings, palette) via a per-game table of data locations. Each game pins the SHA-256 of its expected binary; mismatch is a hard error.
- **`crates/lumen`** — pure engine, no platform deps. Board/pieces model (`Color` = 3-bit RGB mask, 8 `Dir`s, beams carry an optional entanglement id), `trace` (beam propagation into reusable scratch → `BeamField`), win check, per-game `RuleSet`, `GameState` + pure `handle(InputEvent) -> Effects`, and `render` into an RGBA framebuffer at native resolution plus a list of text boxes.
- **`crates/chromatron`** — macOS shell: winit + softbuffer + muda menus + objc2 (NSUserDefaults, NSPasteboard, Core Text with Lucida Grande for text boxes). Four bin targets `chromatron1..4` select a `Game`. Its `build.rs` runs `spectrograph` on the originals and generates `static` data, so startup does no decoding.
- **`re/`** — `ghidra/` (our headless scripts, committed), `notes/chromatronN.md` (our RE findings: function roles, data addresses, formats, quirks — committed), `out/` (ignored).

Per-game behavior differs and must come from **that game's own binary**: e.g. C2 changed splitter semantics ("Splitters in the original Chromatron didn't work correctly") and adds complementor/multi-filter/twister; C3 adds dual-reflector/quad-bender; C4 adds logic gates, starburst, mobile teleporter, occupied squares. Games 2–4 are reverse-engineered by diffing against game 1.

Fidelity is verified by spectrograph decode tests, table-driven per-piece rule tests per `RuleSet`, `insta` ASCII beam-field snapshots per level, and solved-level fixtures in `tests/solutions/gameN/` (our own solutions, original clipboard format). There is no pixel matching against reference screenshots.
