//! The save file `com.silverspaceship.chromatron.dat`, byte for byte (platform.md P7). Bytes only:
//! reading and writing the file is the shell's job.

use std::fmt;

use crate::board::{Board, CELLS, Cell};
use crate::piece::Piece;
use crate::progress::PROGRESS_SLOTS;

/// The scrambled progress bitmap at the start of the file.
pub const BITMAP_LEN: usize = 32;
/// One saved board: the level id, then (type, colour, dir) for each of the 285 cells.
pub const RECORD_LEN: usize = 1 + CELLS * 3;
/// Level ids that can have a saved board (`saved_states[128]`).
pub const SAVED_IDS: usize = 128;

/// `save_bit_perm` @ 0x80ac, a permutation of 0..31.
const BIT_PERM: [u8; 32] = [
    0x00, 0x09, 0x0d, 0x13, 0x15, 0x0a, 0x06, 0x18, 0x1e, 0x0e, 0x03, 0x1c, 0x10, 0x0b, 0x19, 0x0c,
    0x17, 0x04, 0x11, 0x1d, 0x12, 0x1a, 0x01, 0x0f, 0x14, 0x1f, 0x07, 0x08, 0x02, 0x16, 0x05, 0x1b,
];

/// What a save file holds: completed slots and a saved board per level id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveData {
    pub completed: [bool; PROGRESS_SLOTS],
    /// `SAVED_IDS` entries, indexed by level id.
    pub boards: Vec<Option<Board>>,
}

impl Default for SaveData {
    fn default() -> SaveData {
        SaveData {
            completed: [false; PROGRESS_SLOTS],
            boards: vec![None; SAVED_IDS],
        }
    }
}

/// A save file the original would crash on or misread. The shell renames it to `.bak`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    /// A slot's check bits are neither 0 nor its check value (the original asserts).
    BadCheck { slot: usize, found: u8 },
    /// A record names a level id past `saved_states[128]`.
    BadLevelId { id: u8 },
    /// A cell's bytes are outside what the game can draw or trace.
    BadCell { id: u8, cell: usize, bytes: [u8; 3] },
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::BadCheck { slot, found } => {
                write!(f, "slot {slot} has check bits {found:#x}")
            }
            SaveError::BadLevelId { id } => {
                write!(f, "saved board for level id {id} (max {})", SAVED_IDS - 1)
            }
            SaveError::BadCell { id, cell, bytes } => {
                write!(f, "level id {id} cell {cell} has bytes {bytes:02x?}")
            }
        }
    }
}

impl std::error::Error for SaveError {}

/// `save_slot_check` @ 0x400c: the 4 check bits stored for a completed slot, 1..=15.
fn check(slot: usize) -> u8 {
    let mut a = 9 * slot as i32;
    a += a << 8;
    a += a >> 4;
    (a % 15 + 1) as u8
}

/// `save_bit_scramble` @ 0x3f00, for n in 0..128.
fn scramble(n: usize) -> usize {
    let p = |i: usize| usize::from(BIT_PERM[i & 31]);
    match n {
        0..32 => p(n),
        32..64 => p(n + 13) + 0x60,
        64..96 => p(n + 20) + 0x20,
        _ => p(n + 5) + 0x40,
    }
}

/// `save_bit_index` @ 0x3fc8: where check bit `k` of `slot` lives, 0..256.
fn bit_index(slot: usize, k: usize) -> usize {
    scramble((slot & 31) + 32 * k) + 128 * (slot >> 5)
}

pub fn encode_bitmap(completed: &[bool; PROGRESS_SLOTS]) -> [u8; BITMAP_LEN] {
    let mut bitmap = [0u8; BITMAP_LEN];
    for slot in (0..PROGRESS_SLOTS).filter(|&s| completed[s]) {
        for k in (0..4).filter(|k| check(slot) >> k & 1 != 0) {
            let bit = bit_index(slot, k);
            bitmap[bit >> 3] |= 1 << (bit & 7);
        }
    }
    bitmap
}

pub fn decode_bitmap(bitmap: &[u8; BITMAP_LEN]) -> Result<[bool; PROGRESS_SLOTS], SaveError> {
    let mut completed = [false; PROGRESS_SLOTS];
    for (slot, done) in completed.iter_mut().enumerate() {
        let found = (0..4).fold(0u8, |v, k| {
            let bit = bit_index(slot, k);
            v | (bitmap[bit >> 3] >> (bit & 7) & 1) << k
        });
        match found {
            0 => {}
            v if v == check(slot) => *done = true,
            v => return Err(SaveError::BadCheck { slot, found: v }),
        }
    }
    Ok(completed)
}

/// The whole file: bitmap, then every saved board in ascending level-id order.
pub fn encode_save(completed: &[bool; PROGRESS_SLOTS], boards: &[Option<Board>]) -> Vec<u8> {
    let mut out = encode_bitmap(completed).to_vec();
    for (id, board) in boards.iter().enumerate().take(SAVED_IDS) {
        if let Some(board) = board {
            out.push(id as u8);
            for cell in &board.cells {
                out.extend([cell.piece as u8, cell.colour, cell.dir]);
            }
        }
    }
    out
}

/// Reads a save file as the original does: a file shorter than the bitmap means a fresh game, and
/// a truncated last record is dropped. Unlike the original, a bad bitmap, level id or cell (kind
/// above 12, colour above 7, dir above 9 on a movable piece or above 7 otherwise) is an error
/// instead of a crash.
pub fn decode_save(bytes: &[u8]) -> Result<SaveData, SaveError> {
    let mut save = SaveData::default();
    let Some((bitmap, mut rest)) = bytes.split_first_chunk::<BITMAP_LEN>() else {
        return Ok(save);
    };
    save.completed = decode_bitmap(bitmap)?;
    while let Some((record, tail)) = rest.split_first_chunk::<RECORD_LEN>() {
        let id = record[0];
        if usize::from(id) >= SAVED_IDS {
            return Err(SaveError::BadLevelId { id });
        }
        let mut board = Board::default();
        for (cell, (index, bytes)) in board
            .cells
            .iter_mut()
            .zip(record[1..].as_chunks::<3>().0.iter().enumerate())
        {
            let bad = || SaveError::BadCell {
                id,
                cell: index,
                bytes: [bytes[0], bytes[1], bytes[2]],
            };
            let piece = Piece::from_byte(bytes[0]).ok_or_else(bad)?;
            // An unchecked paste in the original can leave dir 8 or 9 on a movable piece
            // (platform.md P8); the rules use `dir & 7`, so keep the raw byte.
            let max_dir = if piece.is_movable() { 9 } else { 7 };
            if bytes[1] > 7 || bytes[2] > max_dir {
                return Err(bad());
            }
            *cell = Cell {
                piece,
                colour: bytes[1],
                dir: bytes[2],
            };
        }
        save.boards[usize::from(id)] = Some(board);
        rest = tail;
    }
    Ok(save)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board_index;

    fn completed(slots: impl IntoIterator<Item = usize>) -> [bool; PROGRESS_SLOTS] {
        let mut done = [false; PROGRESS_SLOTS];
        for s in slots {
            done[s] = true;
        }
        done
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn bit_positions_are_a_bijection() {
        let mut seen: Vec<usize> = (0..PROGRESS_SLOTS)
            .flat_map(|s| (0..4).map(move |k| bit_index(s, k)))
            .collect();
        seen.sort();
        assert_eq!(seen, (0..256).collect::<Vec<_>>());
    }

    #[test]
    fn bitmap_examples_from_the_notes() {
        let one = encode_bitmap(&completed([0]));
        assert_eq!(one[0], 0x01);
        assert!(one[1..].iter().all(|&b| b == 0));
        assert_eq!(
            hex(&encode_bitmap(&completed(0..50))),
            "4776c1cf380fb07fe10837ebd1d54b5c590c291081806c80580c033196800602"
        );
    }

    #[test]
    fn bitmaps_round_trip() {
        for done in [
            completed([]),
            completed([0]),
            completed(0..50),
            completed([3, 17, 40, 63]),
        ] {
            assert_eq!(decode_bitmap(&encode_bitmap(&done)), Ok(done));
        }
    }

    #[test]
    fn a_damaged_bitmap_is_an_error() {
        // All bits set: slot 0 reads 0b1111, but its check value is 1.
        assert_eq!(
            decode_bitmap(&[0xff; BITMAP_LEN]),
            Err(SaveError::BadCheck { slot: 0, found: 15 })
        );
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        let mut seed = 7u32;
        for len in [
            32,
            33,
            100,
            BITMAP_LEN + RECORD_LEN,
            BITMAP_LEN + 2 * RECORD_LEN + 5,
        ] {
            for _ in 0..50 {
                let bytes: Vec<u8> = (0..len)
                    .map(|_| {
                        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                        (seed >> 16) as u8
                    })
                    .collect();
                let _ = decode_save(&bytes);
                let mut valid = encode_bitmap(&completed([1, 2])).to_vec();
                valid.extend(&bytes);
                let _ = decode_save(&valid);
            }
        }
    }

    #[test]
    fn short_files_are_a_fresh_game() {
        assert_eq!(decode_save(&[]), Ok(SaveData::default()));
        assert_eq!(decode_save(&[0xff; 31]), Ok(SaveData::default()));
    }

    #[test]
    fn saved_boards_round_trip_in_id_order() {
        let mut save = SaveData {
            completed: completed([0, 1]),
            ..SaveData::default()
        };
        let mut board = Board::default();
        board.cells[board_index(8, 7)] = Cell {
            piece: Piece::Mirror,
            colour: 4,
            dir: 1,
        };
        board.cells[CELLS - 1] = Cell {
            piece: Piece::Empty,
            colour: 5,
            dir: 7,
        };
        save.boards[9] = Some(board.clone());
        save.boards[2] = Some(Board::default());
        let bytes = encode_save(&save.completed, &save.boards);
        assert_eq!(bytes.len(), BITMAP_LEN + 2 * RECORD_LEN);
        assert_eq!(bytes[BITMAP_LEN], 2, "lower id first");
        assert_eq!(bytes[BITMAP_LEN + RECORD_LEN], 9);
        let mirror = BITMAP_LEN + RECORD_LEN + 1 + 3 * board_index(8, 7);
        assert_eq!(bytes[mirror..mirror + 3], [3, 4, 1], "type, colour, dir");
        assert_eq!(decode_save(&bytes), Ok(save));
    }

    #[test]
    fn a_movable_piece_may_have_dir_8_or_9() {
        let mut board = Board::default();
        board.cells[board_index(3, 3)] = Cell {
            piece: Piece::Tangler,
            colour: 4,
            dir: 9,
        };
        let mut boards = vec![None; SAVED_IDS];
        boards[7] = Some(board.clone());
        let save = decode_save(&encode_save(&completed([]), &boards)).unwrap();
        assert_eq!(save.boards[7], Some(board));
        let mut bad = encode_save(&completed([]), &boards);
        bad[BITMAP_LEN + 1 + 3 * board_index(3, 3) + 2] = 10;
        assert!(matches!(
            decode_save(&bad),
            Err(SaveError::BadCell { id: 7, .. })
        ));
    }

    #[test]
    fn a_truncated_record_is_dropped() {
        let mut boards = vec![None; SAVED_IDS];
        boards[1] = Some(Board::default());
        boards[2] = Some(Board::default());
        let bytes = encode_save(&completed([]), &boards);
        let save = decode_save(&bytes[..bytes.len() - 1]).unwrap();
        assert!(save.boards[1].is_some());
        assert!(save.boards[2].is_none());
    }

    #[test]
    fn bad_records_are_errors() {
        let mut boards = vec![None; SAVED_IDS];
        boards[1] = Some(Board::default());
        let good = encode_save(&completed([]), &boards);
        let mut bad_id = good.clone();
        bad_id[BITMAP_LEN] = 200;
        assert_eq!(decode_save(&bad_id), Err(SaveError::BadLevelId { id: 200 }));
        let mut bad_kind = good.clone();
        bad_kind[BITMAP_LEN + 1] = 13;
        assert_eq!(
            decode_save(&bad_kind),
            Err(SaveError::BadCell {
                id: 1,
                cell: 0,
                bytes: [13, 0, 0]
            })
        );
        let mut bad_dir = good;
        bad_dir[BITMAP_LEN + 3] = 8;
        assert_eq!(
            decode_save(&bad_dir),
            Err(SaveError::BadCell {
                id: 1,
                cell: 0,
                bytes: [0, 0, 8]
            })
        );
    }
}
