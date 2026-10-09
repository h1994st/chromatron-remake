//! `CHROMATRON_SNAPSHOT=path.png`: the first presented canvas as an RGB PNG, for checks without a
//! person looking at the screen.

use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

use lumen::layout::STRIP_SLOTS;
use lumen::{GameState, InputEvent, Redraw};

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
}
