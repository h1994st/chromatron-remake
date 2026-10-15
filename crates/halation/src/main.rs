//! `halation shot`: a level in the modern look as a PNG, for review by eye; `halation pieces`:
//! the synthetic pieces sheet. Review renders go under `dist/` (`just shot-modern`).

use std::{env, path::Path, process::ExitCode};

use halation::{Control, Easel, Lightbox, Output, Scene, Theme, specimen};
use lumen::layout::STRIP_SLOTS;
use lumen::{
    BOARD_CELLS, GameState, InputEvent, Redraw, SaveData, VISIBLE_TOOLBOX_SLOTS, stored_solution,
};
use spectrograph::{Game, decode, write_rgb_png};

const USAGE: &str = "usage: halation shot <game 1-4> <level 1-50> <out.png> [stored | <copied solution>] [--easel WxH@S] [--touch] [--theme dark|light] [--sheet] [--hold <from>,<to>] [--cue turn:<cell>|move:<from>,<to>|lift:<from>,<to>|win|level|sheet [--at ms]]\n       halation pieces <out-prefix> [--easel WxH@S] [--touch] [--theme dark|light]";
const DEFAULT_EASEL: &str = "1280x800@2";
/// The board's cells and the visible tray slots: what `--hold` and `--cue` can name.
const VISIBLE_CELLS: usize = BOARD_CELLS + VISIBLE_TOOLBOX_SLOTS;
/// A paint at this `now` is settled: no cue runs.
const SETTLED: f64 = f64::INFINITY;

/// An input made after the shot's settled first paint, so `--at` shows the cue it starts.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cue {
    /// Alt-click (turn clockwise) the piece at a cell.
    Turn(usize),
    /// Press at one cell, drag to another (painted held) and release.
    Move(usize, usize),
    /// Press at one cell and drag to another (held).
    Lift(usize, usize),
    /// The game's cheat key: a win without moving a piece.
    Win,
    /// The cheat key, then `=`: the next level.
    Level,
    /// Open the level sheet.
    Sheet,
}

/// When the cue's input is painted (any time works: cues are relative to it).
const CUE_AT: f64 = 1000.0;

fn parse_cue(arg: &str) -> Option<Cue> {
    let cell = |s: &str| s.parse().ok().filter(|&i| i < VISIBLE_CELLS);
    let pair = |s: &str| {
        s.split_once(',')
            .and_then(|(a, b)| Some((cell(a)?, cell(b)?)))
    };
    match arg.split_once(':') {
        Some(("turn", c)) => cell(c).map(Cue::Turn),
        Some(("move", p)) => pair(p).map(|(a, b)| Cue::Move(a, b)),
        Some(("lift", p)) => pair(p).map(|(a, b)| Cue::Lift(a, b)),
        None if arg == "win" => Some(Cue::Win),
        None if arg == "level" => Some(Cue::Level),
        None if arg == "sheet" => Some(Cue::Sheet),
        _ => None,
    }
}

/// What the flags ask for: the easel (touch or not), the theme, whether the level sheet is open,
/// a piece held in the air (pressed at one cell index's centre and dragged to another's), and a
/// cue with the time after it (ms) to paint at.
struct Options {
    easel: Easel,
    theme: Theme,
    sheet: bool,
    hold: Option<(usize, usize)>,
    cue: Option<Cue>,
    at: Option<f64>,
}

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

/// The positional arguments and the flags.
fn options(args: &[String]) -> Result<(Vec<&str>, Options), String> {
    let mut positional = Vec::new();
    let mut options = Options {
        easel: Easel::parse(DEFAULT_EASEL).unwrap(),
        theme: Theme::Dark,
        sheet: false,
        hold: None,
        cue: None,
        at: None,
    };
    let mut touch = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--easel" => {
                options.easel = rest
                    .next()
                    .and_then(|v| Easel::parse(v))
                    .ok_or("--easel takes WxH@S, such as 1280x800@2")?;
            }
            "--at" => {
                options.at = Some(
                    rest.next()
                        .and_then(|v| v.parse().ok())
                        .ok_or("--at takes milliseconds")?,
                );
            }
            "--cue" => {
                options.cue = Some(rest.next().and_then(|v| parse_cue(v)).ok_or(
                    "--cue takes turn:<cell>, move:<from>,<to>, lift:<from>,<to>, win, level or sheet",
                )?);
            }
            "--theme" => {
                options.theme = match rest.next().map(String::as_str) {
                    Some("dark") => Theme::Dark,
                    Some("light") => Theme::Light,
                    _ => return Err("--theme takes dark or light".into()),
                };
            }
            "--sheet" => options.sheet = true,
            "--touch" => touch = true,
            "--hold" => {
                let cell = |s: &str| s.parse().ok().filter(|&i| i < VISIBLE_CELLS);
                options.hold = Some(
                    rest.next()
                        .and_then(|v| v.split_once(','))
                        .and_then(|(a, b)| Some((cell(a)?, cell(b)?)))
                        .ok_or("--hold takes two cell indices, such as 225,112")?,
                );
            }
            other => positional.push(other),
        }
    }
    options.easel = options.easel.with_touch(touch);
    if options.at.is_some() && options.cue.is_none() {
        return Err("--at is a time after a --cue".into());
    }
    Ok((positional, options))
}

fn run(args: &[String]) -> Result<(), String> {
    let (positional, options) = options(args)?;
    match positional.as_slice() {
        ["shot", game, level, out, rest @ ..] if rest.len() <= 1 => {
            shot(game, level, out, rest.first().copied(), &options)
        }
        ["pieces", prefix] if !options.sheet && options.hold.is_none() && options.cue.is_none() => {
            pieces(prefix, options.easel, options.theme)
        }
        _ => Err(USAGE.into()),
    }
}

/// A cold, settled paint of `scene` in `theme` into `out`.
fn paint(easel: Easel, theme: Theme, scene: Scene, out: &str) -> Result<(), String> {
    let mut lightbox = Lightbox::new(easel, theme, false);
    let mut px = vec![0u32; (easel.width * easel.height) as usize];
    lightbox.paint_scene(scene, SETTLED, Output::Xrgb(&mut px));
    write_rgb_png(Path::new(out), easel.width, easel.height, &px).map_err(|e| format!("{out}: {e}"))
}

fn shot(
    game: &str,
    level: &str,
    out: &str,
    solution: Option<&str>,
    &Options {
        easel,
        theme,
        sheet,
        hold,
        cue,
        at,
    }: &Options,
) -> Result<(), String> {
    let game = Game::parse_arg(game)?;
    let slot = match level.parse::<usize>() {
        Ok(n @ 1..=STRIP_SLOTS) => n - 1,
        _ => {
            return Err(format!(
                "unknown level {level:?}; expected 1 to {STRIP_SLOTS}"
            ));
        }
    };
    let data = decode(game, &game.read_original()?).map_err(|e| e.to_string())?;
    let mut state = GameState::new(&data, SaveData::default());
    state.goto_slot(slot);
    if let Some(solution) = solution {
        let text = if solution == "stored" {
            stored_solution(&data, slot).ok_or("no stored solution for that level")?
        } else {
            solution.to_owned()
        };
        if state.handle(InputEvent::Paste(text.clone())).redraw == Redraw::None {
            return Err(format!("{text:?} is not a solution for level {}", slot + 1));
        }
    }
    let mut lightbox = Lightbox::new(easel, theme, false);
    if let Some((from, to)) = hold {
        lift(&mut state, &lightbox, from, to).map_err(|e| format!("--hold {from},{to}: {e}"))?;
    }
    if sheet && !lightbox.command(Control::Levels) {
        return Err("--sheet: this easel has an inline picker, so no sheet".into());
    }
    let mut px = vec![0u32; (easel.width * easel.height) as usize];
    // Settled first; a cue's paints then run from `CUE_AT`.
    lightbox.paint(&state, SETTLED, Output::Xrgb(&mut px));
    if let Some(cue) = cue {
        play(&mut state, &mut lightbox, cue, &mut px)?;
        lightbox.paint(&state, CUE_AT, Output::Xrgb(&mut px));
        lightbox.paint(
            &state,
            at.map_or(SETTLED, |ms| CUE_AT + ms),
            Output::Xrgb(&mut px),
        );
    }
    write_rgb_png(Path::new(out), easel.width, easel.height, &px)
        .map_err(|e| format!("{out}: {e}"))?;
    // The level shown: a `level` cue moves on from the one asked for.
    println!(
        "{out}: {game} level {} {}x{}, {}",
        state.slot() + 1,
        easel.width,
        easel.height,
        if state.is_won() { "won" } else { "not won" }
    );
    Ok(())
}

/// Cell `cell`'s centre on `plan`, as a pointer position.
fn centre(plan: &Lightbox, cell: usize) -> (i32, i32) {
    let c = plan.tableau().cell_centre(cell);
    (c.x as i32, c.y as i32)
}

/// Presses at cell `from`'s centre and drags to cell `to`'s, through the lightbox's floorplan,
/// leaving the piece in the air.
fn lift(state: &mut GameState, plan: &Lightbox, from: usize, to: usize) -> Result<(), String> {
    let ((ax, ay), (bx, by)) = (centre(plan, from), centre(plan, to));
    state.handle_on(
        plan,
        InputEvent::MouseDown {
            x: ax,
            y: ay,
            alt: false,
        },
    );
    state.handle_on(plan, InputEvent::MouseDragged { x: bx, y: by });
    state
        .drag()
        .map(|_| ())
        .ok_or_else(|| "no movable piece lifted".into())
}

/// Makes `cue`'s input through `lightbox`, as a player would; fails if nothing moved (the board,
/// the held piece, the level, the win and the sheet all as they were). A move's held piece is
/// painted (settled, into `px`) before its release, as a shell paints a drag, so it lands.
fn play(
    state: &mut GameState,
    lightbox: &mut Lightbox,
    cue: Cue,
    px: &mut [u32],
) -> Result<(), String> {
    let before = (
        state.board().clone(),
        state.drag(),
        state.slot(),
        state.is_won(),
        lightbox.sheet_open(),
    );
    let cheat = InputEvent::Key(state.rules().cheat_key);
    match cue {
        Cue::Turn(cell) => {
            let (x, y) = centre(lightbox, cell);
            state.handle_on(lightbox, InputEvent::MouseDown { x, y, alt: true });
            state.handle_on(lightbox, InputEvent::MouseUp { x, y, alt: true });
        }
        Cue::Move(from, to) | Cue::Lift(from, to) => {
            lift(state, lightbox, from, to)?;
            if let Cue::Move(..) = cue {
                lightbox.paint(state, SETTLED, Output::Xrgb(px));
                let (x, y) = centre(lightbox, to);
                state.handle_on(lightbox, InputEvent::MouseUp { x, y, alt: false });
            }
        }
        Cue::Win => {
            state.handle_on(lightbox, cheat);
        }
        Cue::Level => {
            state.handle_on(lightbox, cheat);
            state.handle_on(lightbox, InputEvent::Key(b'='));
            if state.slot() == before.2 {
                return Err("--cue level: the next level is locked".into());
            }
        }
        Cue::Sheet => {
            lightbox.command(Control::Levels);
        }
    }
    let after = (
        state.board().clone(),
        state.drag(),
        state.slot(),
        state.is_won(),
        lightbox.sheet_open(),
    );
    if after == before {
        return Err(format!("--cue {cue:?}: nothing moved"));
    }
    Ok(())
}

fn pieces(prefix: &str, easel: Easel, theme: Theme) -> Result<(), String> {
    for (k, page) in specimen::pages().iter().enumerate() {
        let out = format!("{prefix}-{}.png", k + 1);
        let scene = Scene::bare(&page.board, &page.field, page.game);
        paint(easel, theme, scene, &out)?;
        println!("{out}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malformed_commands() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(run(&args(&["shot", "1", "5"])), Err(USAGE.into()));
        assert_eq!(run(&args(&["snap", "1", "5", "x.png"])), Err(USAGE.into()));
        assert_eq!(
            run(&args(&["pieces", "x", "--sheet"])),
            Err(USAGE.into()),
            "the pieces sheet has no level sheet"
        );
        assert!(
            run(&args(&["shot", "1", "5", "x.png", "--easel", "big"]))
                .unwrap_err()
                .contains("--easel")
        );
        assert!(
            run(&args(&["shot", "1", "5", "x.png", "--hold", "225"]))
                .unwrap_err()
                .contains("--hold")
        );
        assert!(
            run(&args(&["shot", "1", "5", "x.png", "--hold", "1,249"]))
                .unwrap_err()
                .contains("--hold"),
            "past the visible tray"
        );
        assert_eq!(
            run(&args(&["pieces", "x", "--hold", "225,112"])),
            Err(USAGE.into())
        );
        assert!(
            run(&args(&["shot", "1", "51", "x.png"]))
                .unwrap_err()
                .contains("expected 1 to 50")
        );
        for bad in ["spin:3", "move:1", "turn:250", "lift:1,x"] {
            assert!(
                run(&args(&["shot", "1", "5", "x.png", "--cue", bad]))
                    .unwrap_err()
                    .contains("--cue takes"),
                "{bad}"
            );
        }
        assert!(
            run(&args(&["shot", "1", "5", "x.png", "--at", "100"]))
                .unwrap_err()
                .contains("--at"),
            "a time with no cue"
        );
        assert_eq!(
            run(&args(&["pieces", "x", "--cue", "win"])),
            Err(USAGE.into())
        );
        for bad in [&["--theme", "sepia"][..], &["--theme"]] {
            let mut list = vec!["shot", "1", "5", "x.png"];
            list.extend_from_slice(bad);
            assert!(
                run(&args(&list)).unwrap_err().contains("--theme"),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn reads_the_theme() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let theme = |list: &[&str]| options(&args(list)).map(|(_, o)| o.theme);
        assert_eq!(theme(&["shot"]), Ok(Theme::Dark), "dark by default");
        assert_eq!(theme(&["shot", "--theme", "light"]), Ok(Theme::Light));
        assert_eq!(theme(&["shot", "--theme", "dark"]), Ok(Theme::Dark));
    }

    #[test]
    fn reads_every_cue() {
        assert_eq!(parse_cue("turn:112"), Some(Cue::Turn(112)));
        assert_eq!(parse_cue("move:225,112"), Some(Cue::Move(225, 112)));
        // Ruling F6: a held drag is a cue (lumen changes no board while a piece is held).
        assert_eq!(parse_cue("lift:225,112"), Some(Cue::Lift(225, 112)));
        assert_eq!(parse_cue("win"), Some(Cue::Win));
        assert_eq!(parse_cue("level"), Some(Cue::Level));
        assert_eq!(parse_cue("sheet"), Some(Cue::Sheet));
        assert_eq!(parse_cue("turn"), None);
    }

    #[test]
    fn a_cue_must_move_something() {
        let data = decode(Game::C1, &Game::C1.read_original().unwrap()).unwrap();
        let mut state = GameState::new(&data, SaveData::default());
        let easel = Easel::parse(DEFAULT_EASEL).unwrap();
        let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
        let px = &mut vec![0u32; (easel.width * easel.height) as usize];
        // Ruling F6: a piece lifted from the tray and held is a cue, though no board changed.
        assert_eq!(
            play(&mut state, &mut lightbox, Cue::Lift(225, 112), px),
            Ok(())
        );
        assert!(state.drag().is_some());
        // A move's held piece is painted before its release, so its paint lands it.
        let mut state = GameState::new(&data, SaveData::default());
        let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
        assert_eq!(
            play(&mut state, &mut lightbox, Cue::Move(225, 112), px),
            Ok(())
        );
        lightbox.paint(&state, CUE_AT, Output::Xrgb(px));
        let land = lightbox.choreography().moment(CUE_AT).land;
        assert_eq!(land.map(|l| l.cell), Some(112), "it lands");
        let mut state = GameState::new(&data, SaveData::default());
        assert!(
            play(&mut state, &mut lightbox, Cue::Turn(0), px)
                .unwrap_err()
                .contains("nothing moved"),
            "an empty cell turns nothing"
        );
        assert_eq!(play(&mut state, &mut lightbox, Cue::Win, px), Ok(()));
        assert!(state.is_won());
        assert_eq!(play(&mut state, &mut lightbox, Cue::Level, px), Ok(()));
        assert_eq!(state.slot(), 1, "on to level 2");
        // Level 7 of a fresh save: level 8 is locked, so the cue would only win.
        let mut state = GameState::new(&data, SaveData::default());
        state.goto_slot(6);
        assert!(
            play(&mut state, &mut lightbox, Cue::Level, px)
                .unwrap_err()
                .contains("locked")
        );
    }
}
