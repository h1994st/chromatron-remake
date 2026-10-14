# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A faithful, independent Rust port of Sean Barrett's Chromatron 1–4 laser puzzle games:
- four native macOS (Apple Silicon) apps;
- one browser page, live at https://shengtuo.me/chromatron-remake/.

The originals in `chromatron_mac/` are PowerPC Carbon apps. Rules, levels, sprites and text all come from our own decompilation of them. The reference look is the Mac version, not Win32.

Design specs and implementation plans live locally in `docs/superpowers/`. Read the relevant spec before changing behaviour.

## Hard rules

- **Independent work.** Take no code or data from other Chromatron ports. Resolve unclear behaviour from our own disassembly (`re/notes/`).
- **No game data in git.**
  - The originals, decompiled output (`re/out/`), `insta` snapshots and every build output (`dist/`, `target/`, the `.wasm`) stay out of git. Game data is decoded from the local originals at build time.
  - The only game-derived files allowed in git are the four README screenshots in `assets/screenshots/`. `assets/promo/` and the web page's `og.png` and touch icon are our own artwork: the stills are rendered by `trailer/`, and `blog-concept.svg` is hand-made.
- **`docs/` is local-only.** Never commit it.
- **Tooling.**
  - Commands go in the `justfile`; there are no shell scripts.
  - Homebrew deps are added only via `brew bundle add <name>`.
  - Rust comes from rustup.
- **Naming.** Avoid generic names for crates and modules (e.g. not `extract`, `utils`).
- **Performance from day 1.**
  - No allocation in per-frame or trace paths.
  - Redraw is event-driven.
  - `just bench` must stay within the spec's budgets.
- **Per-game behaviour** comes from that game's own binary. Games 2–4 were reverse-engineered by diffing against game 1. The differences live in `lumen`'s `RuleSet`.
- **Identifiers.** Bundle IDs are `me.shengtuo.chromatron1` … `4`. Commits and PRs carry no Claude attribution.

## Git

- **Commits:** [Conventional Commits](https://www.conventionalcommits.org/), `type(scope): subject`. Scopes are crate or area names: `spectrograph`, `lumen`, `halation`, `chromatron`, `chromatron-web`, `web`, `trailer`, `re`, `just`. The body is empty unless really needed.
- **Branches:** [Conventional Branch](https://conventional-branch.github.io/), `<type>/<kebab-description>`. The type is `feature`, `bugfix`, `hotfix`, `release` or `chore`.
- **Releases:**
  - Finished branches are squash-merged into `main`.
  - A release bumps the workspace version and is tagged `v*`.
  - Pushing a `v*` tag deploys the web page.

## Commands

`just` lists every recipe. The common ones:

| Recipe | Purpose |
|---|---|
| `just init` | first-time setup: Homebrew deps and the original games |
| `just lint` / `just test` | rustfmt + clippy / the whole test suite (needs the originals) |
| `just run <game>` | run a macOS app (game 1–4) |
| `just bundle` | build the four `.app`s into `dist/` |
| `just bench` | engine benchmarks against the performance budgets |
| `just web` / `just web-serve` | build the browser page into `dist/web/` / serve it locally |
| `just web-check` | Node smoke test of the built page (CI runs it) |
| `just web-shots` | headless-Chrome screenshots and checks of the page's layouts |
| `just trailer` / `just trailer-score` | the two promo videos with their soundtrack into `dist/trailer/` / re-score them, picture copied |
| `just trailer-sheet <dir>` / `just trailer-test` | keyframe contact sheets for review / test the trailer's modules |
| `just social-images` | re-render the brand stills |
| `just dump <game>` / `just decompile <game>` | inspect a game's decoded data / Ghidra decompile |

To run a single test, use `cargo test -p <crate> <name>`. Review snapshots with `cargo insta review`.

## Architecture

The Cargo workspace has a strict layering: `spectrograph` → `lumen` → `halation` → shells.

- **`spectrograph`** decodes each original binary (its SHA-256 is pinned) into data types. It also emits that data as Rust `static`s, so the shells' build scripts can embed the games.
- **`lumen`** is the pure, platform-free engine:
  - beam tracing, piece rules and the win check;
  - the save and clipboard codecs in the original formats;
  - `GameState::handle(InputEvent) -> Effects`, the input rules shared by both shells;
  - rendering into a 640×480 frame plus text boxes (the classic look).
- **`halation`** is the modern look: a dependency-free CPU rasteriser (`etch`), additive light with glow and bloom, its own layout (`tableau`) and piece art, behind one façade (`Lightbox`) that both shells call. It never allocates while painting; `just bench` and `web-check` hold it to its time, size and memory budgets.
- **`chromatron`** is the macOS shell, built on winit, softbuffer, muda menus and objc2/Core Text. There is one binary per game. Progress is saved in the original `.dat` format.
- **`chromatron-web`** is the wasm shell, a `wasm-bindgen` `Session` that drives the four games for the page.
- **`web/`** is the static page: plain HTML, CSS and JS, with no bundler.
  - It has a desktop layout and a touch layout (portrait/landscape) for phones.
  - It saves progress in `localStorage` and sends GA4 analytics only on the live site.
  - `.github/workflows/pages.yml` deploys it.
- **`trailer/`** is the promo renderer (plain ES modules drawn in headless Chrome, with `score.mjs` synthesising the soundtrack in Node) and the single source of the brand art: invented boards only, never game data.
- **`re/`** holds our Ghidra scripts and reverse-engineering notes (`notes/chromatronN/*.md`). `re/out/` is git-ignored.

Fidelity is checked by decode tests, per-piece rule tests, every original level run through the tracer and win rule, and local `insta` beam-field snapshots. There are also solved-level fixtures in `tests/solutions/`.
