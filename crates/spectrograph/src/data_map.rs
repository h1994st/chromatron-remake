//! Decoding a game's data from the addresses in its `DataMap` (c1.rs, c2.rs). Every game since
//! Chromatron 1 uses the same formats (re/notes/chromatron2/data.md: "same as C1"); only the
//! addresses, the level count, the piece types and the set of UI strings differ.

use std::borrow::Cow;

use crate::{DecodeError, GameData, Image, Level, Record, SPRITE_LEN, Sprite, UiText, cstring_at};

/// Strip slots (2 rows of 25) in every game.
const SLOTS: usize = 50;
/// XOR-chained "silverspaceship.com" plus NUL.
const SITE_URL_LEN: usize = 20;
const BOARD_SIDE: u8 = 15;

/// Where one game keeps its data, as PowerPC virtual addresses.
pub(crate) struct DataMap {
    /// `level_table`: big-endian pointers indexed by level id; id 0 is NULL.
    pub(crate) level_table: u32,
    /// `level_name_table`, parallel to `level_table` (Chromatron 2 on).
    pub(crate) level_name_table: Option<u32>,
    /// `hint_table`, parallel to `level_table`.
    pub(crate) hint_table: u32,
    /// Level ids 1..=`level_ids` are encoded.
    pub(crate) level_ids: u8,
    /// The highest cell `type` the game's code handles.
    pub(crate) max_kind: u8,
    /// `level_order`: u8 level id per strip slot.
    pub(crate) level_order: u32,
    pub(crate) image_table: u32,
    pub(crate) num_images: u32,
    /// 256 × (B, G, R).
    pub(crate) palette: u32,
    /// u32[8] 0x00RRGGBB by colour mask.
    pub(crate) beam_colors: u32,
    pub(crate) site_url_xor: u32,
    /// `__cstring` addresses of the fixed UI strings; `None` where the game has no such string.
    pub(crate) window_title: u32,
    pub(crate) you_win: u32,
    pub(crate) won: u32,
    pub(crate) click_next: u32,
    pub(crate) freeware: Option<u32>,
    pub(crate) more_levels: Option<u32>,
    pub(crate) thank_you: Option<u32>,
}

pub(crate) fn decode(image: &Image, map: &DataMap) -> Result<GameData, DecodeError> {
    Ok(GameData {
        game: image.game(),
        levels: Cow::Owned(levels(image, map)?),
        level_order: Cow::Owned(level_order(image, map)?),
        sprites: Cow::Owned(sprites(image, map)?),
        palette: palette(image, map)?,
        beam_colors: beam_colors(image, map)?,
        ui: ui_text(image, map)?,
    })
}

fn malformed(image: &Image, message: String) -> DecodeError {
    DecodeError::Malformed {
        game: image.game(),
        message,
    }
}

fn bytes<'a>(image: &Image<'a>, vmaddr: u32, len: usize) -> Result<&'a [u8], DecodeError> {
    image.read(vmaddr, len).ok_or_else(|| {
        malformed(
            image,
            format!("{len} bytes at {vmaddr:#x} are outside the binary"),
        )
    })
}

fn be32(image: &Image, vmaddr: u32) -> Result<u32, DecodeError> {
    let b = bytes(image, vmaddr, 4)?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn string(image: &Image, vmaddr: u32) -> Result<Cow<'static, str>, DecodeError> {
    cstring_at(image, vmaddr)
        .map(Cow::Owned)
        .ok_or_else(|| malformed(image, format!("no string at {vmaddr:#x}")))
}

/// The string the pointer at `table[id]` points to.
fn table_string(image: &Image, table: u32, id: u8) -> Result<Cow<'static, str>, DecodeError> {
    string(image, be32(image, table + 4 * u32::from(id))?)
}

fn levels(image: &Image, map: &DataMap) -> Result<Vec<Level>, DecodeError> {
    (1..=map.level_ids)
        .map(|id| {
            let at = be32(image, map.level_table + 4 * u32::from(id))?;
            let name = match map.level_name_table {
                Some(table) => table_string(image, table, id)?,
                None => Cow::Borrowed(""),
            };
            Ok(Level {
                game: image.game(),
                id,
                records: Cow::Owned(records(image, map, id, at)?),
                name,
                hint: table_string(image, map.hint_table, id)?,
            })
        })
        .collect()
}

/// Reads 5-byte records from `at` up to the 0 `kind` byte that ends the list.
fn records(image: &Image, map: &DataMap, id: u8, mut at: u32) -> Result<Vec<Record>, DecodeError> {
    let mut out: Vec<Record> = Vec::new();
    while bytes(image, at, 1)?[0] != 0 {
        let b = bytes(image, at, 5)?;
        let record = Record {
            kind: b[0],
            dir: b[1],
            colour: b[2],
            x: b[3],
            y: b[4],
        };
        let bad = record.kind > map.max_kind || record.dir > 7 || record.colour > 7;
        let outside = record.x >= BOARD_SIDE || record.y >= BOARD_SIDE;
        let shared = out.iter().any(|r| (r.x, r.y) == (record.x, record.y));
        if bad || outside || shared {
            return Err(malformed(
                image,
                format!("level {id} record {}: {record:?}", out.len()),
            ));
        }
        out.push(record);
        at += 5;
    }
    Ok(out)
}

fn level_order(image: &Image, map: &DataMap) -> Result<Vec<u8>, DecodeError> {
    let order = bytes(image, map.level_order, SLOTS)?;
    match order.iter().find(|&&id| id == 0 || id > map.level_ids) {
        Some(id) => Err(malformed(image, format!("level order names level {id}"))),
        None => Ok(order.to_vec()),
    }
}

fn sprites(image: &Image, map: &DataMap) -> Result<Vec<Sprite>, DecodeError> {
    let count = be32(image, map.num_images)?;
    (0..count)
        .map(|i| {
            let at = be32(image, map.image_table + 4 * i)?;
            let src = image.tail(at).unwrap_or(&[]);
            decode_rle(src)
                .map(|(sprite, _)| sprite)
                .ok_or_else(|| malformed(image, format!("image {i} at {at:#x} is truncated")))
        })
        .collect()
}

/// Decodes one PCX-style RLE image (data.md D5). Returns the sprite and the number of source
/// bytes consumed, or `None` if `src` ends first.
pub fn decode_rle(src: &[u8]) -> Option<(Sprite, usize)> {
    let mut out = [0u8; SPRITE_LEN];
    let (mut n, mut i) = (0, 0);
    while n < SPRITE_LEN {
        let b = *src.get(i)?;
        if b <= 0xC0 {
            out[n] = b;
            n += 1;
            i += 1;
        } else {
            let value = *src.get(i + 1)?;
            let run = usize::from(b - 0xC0).min(SPRITE_LEN - n);
            out[n..n + run].fill(value);
            n += run;
            i += 2;
        }
    }
    Some((out, i))
}

fn palette(image: &Image, map: &DataMap) -> Result<[u32; 256], DecodeError> {
    let raw = bytes(image, map.palette, 256 * 3)?;
    let mut out = [0u32; 256];
    for (entry, bgr) in out.iter_mut().zip(raw.as_chunks::<3>().0) {
        *entry = u32::from(bgr[2]) << 16 | u32::from(bgr[1]) << 8 | u32::from(bgr[0]);
    }
    Ok(out)
}

fn beam_colors(image: &Image, map: &DataMap) -> Result<[u32; 8], DecodeError> {
    let mut out = [0u32; 8];
    for (mask, colour) in out.iter_mut().enumerate() {
        *colour = be32(image, map.beam_colors + 4 * mask as u32)?;
    }
    Ok(out)
}

/// `out[i] = in[i] ^ in[i - 1]` with `in[-1] = 0`, up to the first NUL (data.md D8).
fn site_url(image: &Image, map: &DataMap) -> Result<Cow<'static, str>, DecodeError> {
    let raw = bytes(image, map.site_url_xor, SITE_URL_LEN)?;
    let mut text = String::new();
    let mut prev = 0u8;
    for &b in raw {
        let c = b ^ prev;
        if c == 0 {
            return Ok(Cow::Owned(text));
        }
        text.push(char::from(c));
        prev = b;
    }
    Err(malformed(image, "site URL is not terminated".into()))
}

/// The string at `at`, or "" for a string the game doesn't have.
fn optional_string(image: &Image, at: Option<u32>) -> Result<Cow<'static, str>, DecodeError> {
    at.map_or(Ok(Cow::Borrowed("")), |at| string(image, at))
}

fn ui_text(image: &Image, map: &DataMap) -> Result<UiText, DecodeError> {
    Ok(UiText {
        window_title: string(image, map.window_title)?,
        you_win: string(image, map.you_win)?,
        won: string(image, map.won)?,
        click_next: string(image, map.click_next)?,
        freeware: optional_string(image, map.freeware)?,
        more_levels: optional_string(image, map.more_levels)?,
        thank_you: optional_string(image, map.thank_you)?,
        site_url: site_url(image, map)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle_copies_literals_up_to_and_including_c0() {
        let mut src = vec![0x05, 0xC0];
        src.extend([0xC1, 0x00].repeat(SPRITE_LEN - 2));
        let (sprite, used) = decode_rle(&src).unwrap();
        assert_eq!(&sprite[..3], &[0x05, 0xC0, 0x00]);
        assert_eq!(used, src.len());
    }

    #[test]
    fn rle_runs_repeat_the_next_byte() {
        // 25 × 6, 23 × 8, a literal 6: the start of image 0 (data.md D5).
        let mut src = vec![0xD9, 6, 0xD7, 8, 6];
        src.extend([0xFF, 1].repeat(10));
        let (sprite, _) = decode_rle(&src).unwrap();
        assert!(sprite[..25].iter().all(|&p| p == 6));
        assert!(sprite[25..48].iter().all(|&p| p == 8));
        assert_eq!(sprite[48], 6);
        assert_eq!(sprite[49], 1);
    }

    #[test]
    fn rle_stops_a_run_at_the_end_of_the_image() {
        let mut src = [0xFF, 3].repeat(9); // 9 × 63 = 567
        src.extend([0xFF, 7]); // only 9 of these 63 fit
        src.extend([0xAA, 0xBB]); // never read
        let (sprite, used) = decode_rle(&src).unwrap();
        assert_eq!(sprite[SPRITE_LEN - 1], 7);
        assert_eq!(sprite[566], 3);
        assert_eq!(used, 20);
    }

    #[test]
    fn rle_rejects_truncated_input() {
        assert_eq!(decode_rle(&[0xFF, 1, 0xFF]), None);
        assert_eq!(decode_rle(&[]), None);
    }
}
