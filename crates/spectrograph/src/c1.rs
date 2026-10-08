//! Chromatron 1 v1.15: data locations and decoders (re/notes/chromatron1/data.md D1–D8).

use std::borrow::Cow;

use crate::{DecodeError, GameData, Image, Level, Record, SPRITE_LEN, Sprite, UiText, cstring_at};

/// `level_table` and `hint_table`: 63 big-endian pointers each, indexed by level id; id 0 is NULL.
const LEVEL_TABLE: u32 = 0xb604;
const HINT_TABLE: u32 = 0xb700;
const LEVEL_IDS: u8 = 62;
/// `level_order`: u8 level id per strip slot.
const LEVEL_ORDER: u32 = 0x8110;
const SLOTS: usize = 50;
const IMAGE_TABLE: u32 = 0x126dc;
const NUM_IMAGES: u32 = 0x12bdc;
/// 256 × (B, G, R).
const PALETTE: u32 = 0x128dc;
/// u32[8] 0x00RRGGBB by colour mask.
const BEAM_COLORS: u32 = 0x80d4;
/// XOR-chained "silverspaceship.com" plus NUL.
const SITE_URL_XOR: u32 = 0x80f4;
const SITE_URL_LEN: usize = 20;
/// `__cstring` addresses of the fixed UI strings (re/out/chromatron1/strings.tsv).
const WINDOW_TITLE: u32 = 0x6c40;
const YOU_WIN: u32 = 0x6ce0;
const WON: u32 = 0x6cec;
const CLICK_NEXT: u32 = 0x6cf4;
const FREEWARE: u32 = 0x6d24;
const MORE_LEVELS: u32 = 0x6d30;

const BOARD_SIDE: u8 = 15;
const MAX_KIND: u8 = 12;

pub(crate) fn decode(image: &Image) -> Result<GameData, DecodeError> {
    Ok(GameData {
        game: image.game(),
        levels: Cow::Owned(levels(image)?),
        level_order: Cow::Owned(level_order(image)?),
        sprites: Cow::Owned(sprites(image)?),
        palette: palette(image)?,
        beam_colors: beam_colors(image)?,
        ui: ui_text(image)?,
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

fn levels(image: &Image) -> Result<Vec<Level>, DecodeError> {
    (1..=LEVEL_IDS)
        .map(|id| {
            let records = records(image, id, be32(image, LEVEL_TABLE + 4 * u32::from(id))?)?;
            let hint = string(image, be32(image, HINT_TABLE + 4 * u32::from(id))?)?;
            Ok(Level {
                id,
                records: Cow::Owned(records),
                hint,
            })
        })
        .collect()
}

/// Reads 5-byte records from `at` up to the 0 `kind` byte that ends the list.
fn records(image: &Image, id: u8, mut at: u32) -> Result<Vec<Record>, DecodeError> {
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
        let bad = record.kind > MAX_KIND || record.dir > 7 || record.colour > 7;
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

fn level_order(image: &Image) -> Result<Vec<u8>, DecodeError> {
    let order = bytes(image, LEVEL_ORDER, SLOTS)?;
    match order.iter().find(|&&id| id == 0 || id > LEVEL_IDS) {
        Some(id) => Err(malformed(image, format!("level order names level {id}"))),
        None => Ok(order.to_vec()),
    }
}

fn sprites(image: &Image) -> Result<Vec<Sprite>, DecodeError> {
    let count = be32(image, NUM_IMAGES)?;
    (0..count)
        .map(|i| {
            let at = be32(image, IMAGE_TABLE + 4 * i)?;
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

fn palette(image: &Image) -> Result<[u32; 256], DecodeError> {
    let raw = bytes(image, PALETTE, 256 * 3)?;
    let mut out = [0u32; 256];
    for (entry, bgr) in out.iter_mut().zip(raw.as_chunks::<3>().0) {
        *entry = u32::from(bgr[2]) << 16 | u32::from(bgr[1]) << 8 | u32::from(bgr[0]);
    }
    Ok(out)
}

fn beam_colors(image: &Image) -> Result<[u32; 8], DecodeError> {
    let mut out = [0u32; 8];
    for (mask, colour) in out.iter_mut().enumerate() {
        *colour = be32(image, BEAM_COLORS + 4 * mask as u32)?;
    }
    Ok(out)
}

/// `out[i] = in[i] ^ in[i - 1]` with `in[-1] = 0`, up to the first NUL (data.md D8).
fn site_url(image: &Image) -> Result<Cow<'static, str>, DecodeError> {
    let raw = bytes(image, SITE_URL_XOR, SITE_URL_LEN)?;
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

fn ui_text(image: &Image) -> Result<UiText, DecodeError> {
    Ok(UiText {
        window_title: string(image, WINDOW_TITLE)?,
        you_win: string(image, YOU_WIN)?,
        won: string(image, WON)?,
        click_next: string(image, CLICK_NEXT)?,
        freeware: string(image, FREEWARE)?,
        more_levels: string(image, MORE_LEVELS)?,
        site_url: site_url(image)?,
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
