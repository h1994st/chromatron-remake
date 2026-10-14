//! `CHROMATRON_SNAPSHOT=path.png`: the game as an RGB PNG, for checks without a person looking
//! at the screen. Classic writes the first presented canvas. Modern (`CHROMATRON_LOOK`, the
//! default) paints an offscreen easel (`CHROMATRON_SNAPSHOT_SIZE`, `CHROMATRON_THEME`) settled,
//! with its captions, before any window exists (spec §13, ruling P9), so it is independent of
//! the window and the display. Neither reads or writes the save file or the preferences.

use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

use halation::{DeviceRect, Easel, Theme};
use lumen::layout::STRIP_SLOTS;
use lumen::{GameState, InputEvent, Redraw};
use spectrograph::GameData;

use crate::modern_stage::ModernStage;
use crate::window_shape::theme_for;

/// A modern snapshot's canvas when `CHROMATRON_SNAPSHOT_SIZE` is unset (spec §13).
pub const DEFAULT_SNAPSHOT_SIZE: &str = "1280x800@2";

/// A modern snapshot's easel and theme.
#[derive(Debug, PartialEq)]
pub struct ModernShot {
    pub easel: Easel,
    pub theme: Theme,
}

impl ModernShot {
    /// `size` (`WxH@S` in pt and px per pt; unset or empty: 1280x800@2) and `theme` (`dark`,
    /// the default, or `light`, which paints dark until phase 5 through `theme_for`).
    pub fn parse(size: Option<&str>, theme: Option<&str>) -> Result<ModernShot, String> {
        let size = size
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_SNAPSHOT_SIZE);
        let easel = Easel::parse(size).ok_or_else(|| {
            format!("CHROMATRON_SNAPSHOT_SIZE {size:?} is not WxH@S, such as 1280x800@2")
        })?;
        let theme = match theme.map(str::trim) {
            None | Some("") | Some("dark") => theme_for(Some(winit::window::Theme::Dark)),
            Some("light") => {
                log::warn!("the light theme comes in phase 5; the snapshot is dark");
                theme_for(Some(winit::window::Theme::Light))
            }
            Some(other) => {
                return Err(format!(
                    "CHROMATRON_THEME {other:?}: expected dark or light"
                ));
            }
        };
        Ok(ModernShot { easel, theme })
    }

    /// Reads `CHROMATRON_SNAPSHOT_SIZE` and `CHROMATRON_THEME`.
    pub fn from_env() -> Result<ModernShot, String> {
        let var = |name| std::env::var(name).ok();
        ModernShot::parse(
            var("CHROMATRON_SNAPSHOT_SIZE").as_deref(),
            var("CHROMATRON_THEME").as_deref(),
        )
    }
}

/// A classic snapshot is the window's first frame, so a size would do nothing: an error (P9).
pub fn classic_size_check(size: Option<&str>) -> Result<(), String> {
    match size.map(str::trim) {
        None | Some("") => Ok(()),
        Some(size) => Err(format!(
            "CHROMATRON_SNAPSHOT_SIZE {size:?} is for the modern look; a classic snapshot is the window's first frame"
        )),
    }
}

/// Spec §13: the game at `scene`, painted settled (`now = ∞`) on the shot's offscreen easel
/// with its captions. Also returns where they were inked. No window, display, save file or
/// preference.
pub fn shoot_canvas(
    data: &'static GameData,
    scene: &Scene,
    shot: &ModernShot,
) -> Result<(Vec<u32>, Vec<DeviceRect>), String> {
    let mut state = GameState::new(data, lumen::SaveData::fresh(lumen::RuleSet::of(data.game)));
    scene.apply(&mut state)?;
    let mut stage = ModernStage::new(shot.easel, shot.theme, false);
    let mut canvas = vec![0u32; shot.easel.width as usize * shot.easel.height as usize];
    stage.paint(&state, f64::INFINITY, &mut canvas);
    Ok((canvas, stage.caption_rects().collect()))
}

/// `shoot_canvas`, written to `path` as a PNG.
pub fn shoot_modern(
    data: &'static GameData,
    scene: &Scene,
    shot: &ModernShot,
    path: &Path,
) -> Result<(), String> {
    let (canvas, _) = shoot_canvas(data, scene, shot)?;
    let (width, height) = (shot.easel.width as usize, shot.easel.height as usize);
    write_png(path, &canvas, width, height)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Writes `pixels` (`width × height`, row-major, `0x00RRGGBB`) as an 8-bit RGB PNG.
pub fn write_png(path: &Path, pixels: &[u32], width: usize, height: usize) -> io::Result<()> {
    assert_eq!(
        pixels.len(),
        width * height,
        "pixels must be width × height"
    );
    let mut encoder = png::Encoder::new(
        BufWriter::new(File::create(path)?),
        width as u32,
        height as u32,
    );
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let rgb: Vec<u8> = pixels
        .iter()
        .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
        .collect();
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&rgb))
        .map_err(io::Error::other)
}

/// Where the snapshot's game is put before the first paint.
#[derive(Debug, PartialEq, Eq)]
pub struct Scene {
    /// A 0-based strip slot (`CHROMATRON_SNAPSHOT_LEVEL`, 1–50).
    pub slot: Option<usize>,
    pub solution: Option<Solution>,
}

/// `CHROMATRON_SNAPSHOT_SOLUTION`.
#[derive(Debug, PartialEq, Eq)]
pub enum Solution {
    /// The level's stored placement (data.md D3).
    Stored,
    /// Copied clipboard text such as `3-17-hODwhH`.
    Text(String),
}

impl Scene {
    /// Reads the two variables; unset or empty means "leave as is".
    pub fn from_env() -> Result<Scene, String> {
        let var = |name| std::env::var(name).ok();
        Scene::parse(
            var("CHROMATRON_SNAPSHOT_LEVEL").as_deref(),
            var("CHROMATRON_SNAPSHOT_SOLUTION").as_deref(),
        )
    }

    pub fn parse(level: Option<&str>, solution: Option<&str>) -> Result<Scene, String> {
        let slot = match level.map(str::trim).filter(|s| !s.is_empty()) {
            Some(arg) => Some(parse_level(arg)?),
            None => None,
        };
        Ok(Scene {
            slot,
            solution: solution.and_then(parse_solution),
        })
    }

    /// Goes to the level and pastes the solution; an error for a paste the game rejects.
    pub fn apply(&self, state: &mut GameState<'static>) -> Result<(), String> {
        if let Some(slot) = self.slot {
            state.goto_slot(slot);
        }
        let text = match &self.solution {
            None => return Ok(()),
            Some(Solution::Stored) => lumen::stored_solution(state.data(), state.slot())
                .ok_or("this level has no stored solution to copy")?,
            Some(Solution::Text(text)) => text.clone(),
        };
        if state.handle(InputEvent::Paste(text.clone())).redraw == Redraw::None {
            return Err(format!(
                "{text:?} is not a solution for level {}",
                state.slot() + 1
            ));
        }
        if matches!(self.solution, Some(Solution::Stored)) && !state.is_won() {
            log::warn!(
                "the stored placement of level {} does not win (a quirk of the original data)",
                state.slot() + 1
            );
        }
        Ok(())
    }
}

/// A strip number 1–50, as a 0-based slot.
fn parse_level(arg: &str) -> Result<usize, String> {
    match arg.parse::<usize>() {
        Ok(n @ 1..=STRIP_SLOTS) => Ok(n - 1),
        _ => Err(format!(
            "unknown level {arg:?}; expected 1 to {STRIP_SLOTS}"
        )),
    }
}

/// `stored`, or the copied text as is; empty means no solution.
fn parse_solution(arg: &str) -> Option<Solution> {
    match arg.trim() {
        "" => None,
        "stored" => Some(Solution::Stored),
        text => Some(Solution::Text(text.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_strip_numbers() {
        let slot = |l| Scene::parse(Some(l), None).map(|s| s.slot);
        assert_eq!(slot("1"), Ok(Some(0)));
        assert_eq!(slot(" 50 "), Ok(Some(49)));
        for bad in ["0", "51", "x", "-1"] {
            assert!(slot(bad).unwrap_err().contains("expected 1 to 50"), "{bad}");
        }
        assert_eq!(Scene::parse(None, None).unwrap().slot, None);
        assert_eq!(Scene::parse(Some(""), None).unwrap().slot, None);
    }

    #[test]
    fn solutions_are_stored_or_text() {
        let solution = |s| Scene::parse(None, Some(s)).unwrap().solution;
        assert_eq!(solution(""), None);
        assert_eq!(solution("stored"), Some(Solution::Stored));
        assert_eq!(
            solution("3-17-hODwhH"),
            Some(Solution::Text("3-17-hODwhH".into()))
        );
    }

    #[test]
    fn pixels_round_trip_through_the_png() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        write_png(&path, &[0xa4a4a4, 0x123456, 0x000000, 0xffffff], 2, 2).unwrap();
        let decoder = png::Decoder::new(io::BufReader::new(File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(
            &buf[..12],
            [
                0xa4, 0xa4, 0xa4, 0x12, 0x34, 0x56, 0, 0, 0, 0xff, 0xff, 0xff
            ]
        );
    }

    fn fresh() -> GameState<'static> {
        GameState::new(&crate::c1::GAME, lumen::SaveData::default())
    }

    #[test]
    fn apply_pastes_a_copied_solution() {
        let mut state = fresh();
        let scene = Scene::parse(None, Some("1-01-hI")).unwrap();
        assert_eq!(scene.apply(&mut state), Ok(()));
        assert!(state.is_won());
    }

    #[test]
    fn apply_rejects_text_that_does_not_paste() {
        let mut state = fresh();
        let scene = Scene::parse(Some("5"), Some("garbage")).unwrap();
        assert!(scene.apply(&mut state).unwrap_err().contains("level 5"));
    }

    #[test]
    fn apply_goes_to_the_level_and_pastes_its_stored_placement() {
        let mut state = fresh();
        let scene = Scene::parse(Some("46"), Some("stored")).unwrap();
        assert_eq!(scene.apply(&mut state), Ok(()));
        assert_eq!(state.slot(), 45);
        assert!(state.is_won());
    }

    #[test]
    fn modern_shots_default_to_1280_by_800_at_2_in_the_dark() {
        let shot = ModernShot::parse(None, None).unwrap();
        assert_eq!(
            (shot.easel.width, shot.easel.height, shot.easel.px_per_pt),
            (2560, 1600, 2.0)
        );
        assert_eq!(shot.theme, Theme::Dark);
        assert_eq!(ModernShot::parse(Some(""), Some("")).unwrap(), shot);
        let small = ModernShot::parse(Some("1280x800@1"), Some("dark")).unwrap();
        assert_eq!((small.easel.width, small.easel.height), (1280, 800));
    }

    #[test]
    fn modern_shots_reject_bad_sizes_and_themes() {
        assert!(
            ModernShot::parse(Some("1280x800"), None)
                .unwrap_err()
                .contains("WxH@S")
        );
        assert!(
            ModernShot::parse(None, Some("sepia"))
                .unwrap_err()
                .contains("dark or light")
        );
    }

    #[test]
    fn modern_shots_paint_light_as_dark_until_phase_5() {
        assert_eq!(
            ModernShot::parse(None, Some("light")).unwrap().theme,
            Theme::Dark
        );
    }

    #[test]
    fn a_classic_snapshot_rejects_a_size() {
        assert_eq!(classic_size_check(None), Ok(()));
        assert_eq!(classic_size_check(Some(" ")), Ok(()));
        assert!(
            classic_size_check(Some("1280x800@2"))
                .unwrap_err()
                .contains("modern look")
        );
    }

    #[test]
    fn a_modern_shot_is_byte_stable_and_sized() {
        let dir = tempfile::tempdir().unwrap();
        let shot = ModernShot::parse(Some("800x600@1"), None).unwrap();
        let scene = Scene::parse(Some("1"), Some("1-01-hI")).unwrap();
        let (a, b) = (dir.path().join("a.png"), dir.path().join("b.png"));
        shoot_modern(&crate::c1::GAME, &scene, &shot, &a).unwrap();
        shoot_modern(&crate::c1::GAME, &scene, &shot, &b).unwrap();
        assert_eq!(
            std::fs::read(&a).unwrap(),
            std::fs::read(&b).unwrap(),
            "byte-stable"
        );
        let decoder = png::Decoder::new(io::BufReader::new(File::open(&a).unwrap()));
        let reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (800, 600));
    }

    /// The shot is the settled lightbox plus its captions: it differs from a bare paint only
    /// inside the captions' rects, and there it does.
    #[test]
    fn a_modern_shot_carries_its_captions() {
        let shot = ModernShot::parse(Some("800x600@1"), None).unwrap();
        let scene = Scene::parse(Some("1"), Some("1-01-hI")).unwrap();
        let (canvas, rects) = shoot_canvas(&crate::c1::GAME, &scene, &shot).unwrap();
        let data = &crate::c1::GAME;
        let mut state = GameState::new(data, lumen::SaveData::fresh(lumen::RuleSet::of(data.game)));
        scene.apply(&mut state).unwrap();
        let mut bare = vec![0u32; 800 * 600];
        halation::Lightbox::new(shot.easel, shot.theme, false).paint(
            &state,
            f64::INFINITY,
            halation::Output::Xrgb(&mut bare),
        );
        assert!(!rects.is_empty());
        let mut inked = 0;
        for (i, (a, b)) in canvas.iter().zip(&bare).enumerate() {
            if a != b {
                inked += 1;
                let (x, y) = ((i % 800) as i32, (i / 800) as i32);
                assert!(
                    rects
                        .iter()
                        .any(|r| r.left <= x && x < r.right && r.top <= y && y < r.bottom),
                    "({x}, {y})"
                );
            }
        }
        assert!(inked > 100, "captions were inked");
    }
}
