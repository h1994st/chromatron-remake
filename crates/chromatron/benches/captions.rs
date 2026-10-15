//! Spec §14: the macOS captions, cold (every mask set by Core Text, fonts made outside the timing
//! as the stage makes them on resize; best of 5, a fresh typesetter each run): < 4 ms, on each
//! game's level with the longest hint, for
//! - a fresh card at 1280 × 800 pt @2 (Beside: name, hint, credits);
//! - the fullest card at 800 × 600 pt @2 (Beside, the picker a sheet: the level solved earlier,
//!   so "(won)", name, hint, credits and the Levels pill; five lines, the most a card shows);
//! - the compact card at 800 × 700 pt @2 (ruling P2: the compact name widened to the badge's
//!   measured text, badge, pill, hint);
//! - the won card at 800 × 600 pt @2 (the cheat key: the win text, click-next, credits, pill).
//!
//! Each row reports the size the body was set at (15, 13 or 11 pt: one to three Core Text
//! passes) and the warm re-ink every modern frame pays. Then, per game, the stage's level-change
//! first frame at 2560 × 1600 (< 28 ms in one lightbox paint, ruling P4) and its drag frame over
//! the card (p90 < 1 ms). Every row runs in both themes (`[dark]`, `[light]`) at the same caps.
//! Exits non-zero over budget.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use chromatron::caption_typeset::{
    BODY_STEPS, CaptionLines, CaptionTypesetter, Clip, body_fit, body_fonts, lay_out,
};
use chromatron::modern_stage::{ModernStage, StageTimes};
use halation::{CaptionStyle, Easel, Lightbox, Output, Route, Theme};
use lumen::layout::STRIP_SLOTS;
use lumen::{GameState, InputEvent, RuleSet, SaveData, TextRole};

const COLD: Duration = Duration::from_millis(4);
const W: usize = 2560;
const H: usize = 1600;

fn hint_len(state: &GameState<'_>) -> usize {
    lumen::text_boxes(state)
        .as_slice()
        .iter()
        .find(|b| b.role == TextRole::Hint)
        .map_or(0, |b| b.text.len())
}

/// A row's theme prefix.
fn tag(theme: Theme) -> &'static str {
    match theme {
        Theme::Dark => "[dark]",
        Theme::Light => "[light]",
    }
}

/// One gated row: `state`'s captions in `theme` on a `width × height` px canvas at @2 (`px`
/// holds at least that many pixels). True if within budget.
fn captions_row(
    label: &str,
    theme: Theme,
    state: &GameState<'static>,
    width: usize,
    height: usize,
    px: &mut [u32],
) -> bool {
    let px = &mut px[..width * height];
    let mut lightbox = Lightbox::new(Easel::new(width as u32, height as u32, 2.0), theme, false);
    let captions = lightbox
        .paint(state, f64::INFINITY, Output::Xrgb(px))
        .captions;
    let palette = theme.palette();
    let mut lines = CaptionLines::new();
    let mut cold = Duration::MAX;
    let mut warm = Duration::MAX;
    for _ in 0..5 {
        let mut typesetter = CaptionTypesetter::new(2.0);
        let started = Instant::now();
        lay_out(
            captions.as_slice(),
            width as i32,
            2.0,
            |t, s| typesetter.measure(t, s),
            &mut lines,
        );
        typesetter.ink(&lines, palette, px, width, height, Clip::Whole);
        cold = cold.min(started.elapsed());
        let started = Instant::now();
        typesetter.ink(&lines, palette, px, width, height, Clip::Whole);
        warm = warm.min(started.elapsed());
    }
    let body = lines
        .iter()
        .find(|l| l.style == CaptionStyle::Body)
        .map_or_else(
            || "no body".to_owned(),
            |l| {
                let (w, h) = (l.rect.right - l.rect.left, l.rect.bottom - l.rect.top);
                let step = body_fit(&body_fonts(), 2.0, &l.text, w as usize, h as usize).0;
                format!("body at {} pt", BODY_STEPS[step].0)
            },
        );
    let pass = cold < COLD;
    println!(
        "{} {label} {width}×{height} ({} lines, {body}): {cold:?} (budget {COLD:?}) {}; warm re-ink {warm:?} (reported)",
        tag(theme),
        lines.len(),
        if pass { "ok" } else { "OVER" }
    );
    pass
}

/// Routes `event` through the stage and hands what reaches the game to it, as the shell does;
/// whether a piece is held after it.
fn send(stage: &mut ModernStage, state: &mut GameState<'static>, event: InputEvent) -> bool {
    if let Route::Game(ev) = stage.route(event) {
        state.handle_on(stage.lightbox(), ev);
    }
    state.drag().is_some()
}

/// The stage's rows for one game in `theme`, `slot` its level with the longest hint (`px` holds
/// 2560 × 1600 pixels). True if within budget.
///
/// - A level change's first frame through the stage, into `slot` from its neighbour (halation's
///   cold-class paint, the old captions recomposed in that paint, and Core Text setting the
///   longest hint): gated like halation's level change, 28 ms (the native cold-class cap), and
///   it must be one lightbox paint (ruling P4). Best of 5, a fresh stage each run.
/// - A modern frame with a piece held over the card (paint + re-ink): its p90 gated at spec §14's
///   1 ms drag cap (there halation's paint alone), so a re-ink regression shows. The piece is
///   lifted from the tray onto the board (a drag starts only over a cell), then carried across
///   the card; the clock runs at 60 fps and frames are timed once the 90 ms lift has finished,
///   so they are plain drag frames.
fn stage_rows(theme: Theme, state: &mut GameState<'static>, slot: usize, px: &mut [u32]) -> bool {
    const LEVEL: Duration = Duration::from_millis(28);
    const DRAG: Duration = Duration::from_millis(1);
    let game = state.data().game;
    let easel = Easel::new(W as u32, H as u32, 2.0);
    let mut level = Duration::MAX;
    let mut level_paints = 0;
    let mut split = StageTimes::default();
    let other = if slot + 1 < STRIP_SLOTS {
        slot + 1
    } else {
        slot - 1
    };
    for _ in 0..5 {
        let mut stage = ModernStage::new(easel, theme, false);
        state.goto_slot(other);
        stage.paint(state, 0.0, px);
        state.goto_slot(slot);
        let started = Instant::now();
        let times = stage.paint(state, 1000.0, px);
        let took = started.elapsed();
        if took < level {
            level = took;
            split = times;
        }
        level_paints = level_paints.max(times.paints);
    }
    let pass = level < LEVEL && level_paints == 1;
    let tag = tag(theme);
    println!(
        "{tag} {game}: stage level change 2560×1600 (slot {} → {}): {level:?} (paint {:?}, captions {:?}), {level_paints} lightbox paint(s) (budget {LEVEL:?}, 1 paint) {}",
        other + 1,
        slot + 1,
        split.paint,
        split.captions,
        if pass { "ok" } else { "OVER" }
    );

    let mut stage = ModernStage::new(easel, theme, false);
    let mut now = 0.0;
    stage.paint(state, now, px);
    let t = stage.lightbox().tableau().clone();
    let card = t.device(t.card);
    let Some(from) = (lumen::BOARD_CELLS..lumen::BOARD_CELLS + lumen::VISIBLE_TOOLBOX_SLOTS)
        .find(|&i| state.board().cells[i].piece.is_movable())
        .map(|i| t.cell_centre(i))
    else {
        println!(
            "{tag} {game}: stage drag frame: no movable tray piece on slot {} (FAILED)",
            slot + 1
        );
        return false;
    };
    let onto = t.cell_centre(0);
    let (x, y) = (from.x as i32, from.y as i32);
    send(
        &mut stage,
        state,
        InputEvent::MouseDown { x, y, alt: false },
    );
    let (x, y) = (onto.x as i32, onto.y as i32);
    let lifted = send(&mut stage, state, InputEvent::MouseDragged { x, y });
    let mut timed = Vec::with_capacity(37);
    for k in 0..=36 {
        now += 1000.0 / 60.0;
        let x = card.left + (card.right - card.left) * k / 36;
        let y = (card.top + card.bottom) / 2;
        let held = send(&mut stage, state, InputEvent::MouseDragged { x, y });
        let started = Instant::now();
        stage.paint(state, now, px);
        let took = started.elapsed();
        if held && now > 120.0 {
            timed.push(took);
        }
    }
    send(&mut stage, state, InputEvent::MouseUp { x, y, alt: false });
    if !lifted || timed.is_empty() {
        println!("{tag} {game}: stage drag frame: the piece was never held (FAILED)");
        return false;
    }
    timed.sort();
    let p90 = timed[(timed.len() * 9).div_ceil(10) - 1];
    let worst = timed[timed.len() - 1];
    let drag_pass = p90 < DRAG;
    println!(
        "{tag} {game}: stage drag frame over the card 2560×1600: p90 {p90:?}, worst {worst:?} of {} frames (budget {DRAG:?}) {}",
        timed.len(),
        if drag_pass { "ok" } else { "OVER" }
    );
    pass && drag_pass
}

fn main() -> ExitCode {
    let mut ok = true;
    for spec in [
        &chromatron::CHROMATRON_1,
        &chromatron::CHROMATRON_2,
        &chromatron::CHROMATRON_3,
        &chromatron::CHROMATRON_4,
    ] {
        // A fresh save per theme: the won rows mark the level solved.
        for theme in Theme::ALL {
            let game = spec.data.game;
            let mut state = GameState::new(spec.data, SaveData::fresh(RuleSet::of(game)));
            let slot = (0..STRIP_SLOTS)
                .max_by_key(|&s| {
                    state.goto_slot(s);
                    hint_len(&state)
                })
                .expect("50 slots");
            state.goto_slot(slot);
            let mut px = vec![0u32; W * H];
            let at = format!("{game}: captions cold, slot {}", slot + 1);
            ok &= captions_row(&format!("{at}, fresh"), theme, &state, W, H, &mut px);
            state.handle(InputEvent::Key(state.rules().cheat_key));
            if state.is_won() {
                ok &= captions_row(&format!("{at}, won"), theme, &state, 1600, 1200, &mut px);
                // Restarted: not won, but solved before, so the card shows "(won)".
                state.handle(InputEvent::Key(b'r'));
                ok &= captions_row(
                    &format!("{at}, solved earlier"),
                    theme,
                    &state,
                    1600,
                    1200,
                    &mut px,
                );
                ok &= captions_row(
                    &format!("{at}, solved earlier, compact"),
                    theme,
                    &state,
                    1600,
                    1400,
                    &mut px,
                );
            } else {
                ok = false;
                println!(
                    "{} {at}: the cheat key does not win (FAILED: the fuller rows cannot run)",
                    tag(theme)
                );
            }
            ok &= stage_rows(theme, &mut state, slot, &mut px);
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
