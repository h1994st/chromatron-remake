//! Text and image views of decoded data for `spectrograph dump`.

use spectrograph::{GameData, Level, SPRITE_SIZE, colour_symbol, kind_symbol};

const SIDE: usize = 15;
/// Sprites per row in the sprite sheet, and the gap around each one.
const SHEET_COLUMNS: usize = 16;
const SHEET_GAP: usize = 2;
const SHEET_BACKGROUND: u32 = 0xa4a4a4;

/// A 15 × 15 grid, three characters per cell: kind, colour, direction (blank where unused).
/// Kinds: `.` empty `#` wall `*` laser `M` mirror `B` bender `F` filter `P` prism `D` doppler
/// `S` splitter `Q` tangler `o` pinwheel `=` conduit `T` teleporter. Colours: k b g c r m y w.
pub fn level_grid(level: &Level) -> String {
    let mut cells = [[*b".  "; SIDE]; SIDE];
    for r in level.records.iter() {
        let (colour, dir) = match r.kind {
            2 | 5 => (true, true),
            10 => (true, false),
            3 | 4 | 6..=9 | 11 => (false, true),
            _ => (false, false),
        };
        cells[usize::from(r.y)][usize::from(r.x)] = [
            kind_symbol(r.kind) as u8,
            if colour {
                colour_symbol(r.colour) as u8
            } else {
                b' '
            },
            if dir { b'0' + r.dir } else { b' ' },
        ];
    }
    let mut out = String::from("   ");
    for x in 0..SIDE {
        out.push_str(&format!("{x:<4}"));
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    for (y, row) in cells.iter().enumerate() {
        let line: Vec<&str> = row
            .iter()
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        out.push_str(&format!("{y:>2} {}\n", line.join(" ").trim_end()));
    }
    out
}

/// All sprites in a 16-wide grid, each surrounded by a 2 px background gap.
/// Returns (width, height, 0x00RRGGBB pixels).
pub fn sprite_sheet(data: &GameData) -> (u32, u32, Vec<u32>) {
    let rows = data.sprites.len().div_ceil(SHEET_COLUMNS);
    let pitch = SPRITE_SIZE + SHEET_GAP;
    let (width, height) = (SHEET_COLUMNS * pitch + SHEET_GAP, rows * pitch + SHEET_GAP);
    let mut pixels = vec![SHEET_BACKGROUND; width * height];
    for (i, sprite) in data.sprites.iter().enumerate() {
        let (left, top) = (
            SHEET_GAP + (i % SHEET_COLUMNS) * pitch,
            SHEET_GAP + (i / SHEET_COLUMNS) * pitch,
        );
        for (row, line) in sprite.as_chunks::<SPRITE_SIZE>().0.iter().enumerate() {
            for (col, &index) in line.iter().enumerate() {
                pixels[(top + row) * width + left + col] = data.palette[usize::from(index)];
            }
        }
    }
    (width as u32, height as u32, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectrograph::{Game, Record, SPRITE_LEN, UiText};
    use std::borrow::Cow;

    fn level(records: &[Record]) -> Level {
        Level {
            id: 1,
            records: Cow::Owned(records.to_vec()),
            hint: Cow::Borrowed(""),
        }
    }

    #[test]
    fn grid_shows_kind_colour_and_direction() {
        let grid = level_grid(&level(&[
            Record {
                kind: 2,
                dir: 2,
                colour: 4,
                x: 0,
                y: 0,
            },
            Record {
                kind: 10,
                dir: 6,
                colour: 7,
                x: 1,
                y: 0,
            },
            Record {
                kind: 3,
                dir: 1,
                colour: 4,
                x: 2,
                y: 0,
            },
            Record {
                kind: 1,
                dir: 3,
                colour: 5,
                x: 14,
                y: 14,
            },
        ]));
        let lines: Vec<&str> = grid.lines().collect();
        assert_eq!(lines.len(), 16);
        assert_eq!(
            lines[0],
            "   0   1   2   3   4   5   6   7   8   9   10  11  12  13  14"
        );
        assert!(
            lines[1].starts_with(" 0 *r2 ow  M 1 .   .  "),
            "{:?}",
            lines[1]
        );
        assert!(lines[15].ends_with(" .   #"), "{:?}", lines[15]);
    }

    #[test]
    fn sheet_places_sprites_in_rows_of_16() {
        let mut sprites = vec![[0u8; SPRITE_LEN]; 17];
        sprites[16][0] = 1;
        let mut palette = [0u32; 256];
        palette[1] = 0x123456;
        let empty = Cow::Borrowed("");
        let data = GameData {
            game: Game::C1,
            levels: Cow::Owned(vec![]),
            level_order: Cow::Owned(vec![]),
            sprites: Cow::Owned(sprites),
            palette,
            beam_colors: [0; 8],
            ui: UiText {
                window_title: empty.clone(),
                you_win: empty.clone(),
                won: empty.clone(),
                click_next: empty.clone(),
                freeware: empty.clone(),
                more_levels: empty.clone(),
                site_url: empty,
            },
        };
        let (width, height, pixels) = sprite_sheet(&data);
        assert_eq!((width, height), (418, 54));
        assert_eq!(pixels[0], SHEET_BACKGROUND);
        assert_eq!(
            pixels[28 * 418 + 2],
            0x123456,
            "sprite 16 starts the second row"
        );
        assert_eq!(pixels[2 * 418 + 2], 0, "sprite 0 pixel 0 is palette 0");
    }
}
