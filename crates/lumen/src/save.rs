//! The save file (`com.silverspaceship.chromatron.dat`, `…chromatron2.dat`), byte for byte
//! (platform.md P7 of each game). Bytes only: reading and writing the file is the shell's job.

use std::fmt;

use crate::board::{Board, CELLS, Cell};
use crate::progress::PROGRESS_SLOTS;
use crate::rule_set::RuleSet;

/// The scrambled progress bitmap at the start of the file.
pub const BITMAP_LEN: usize = 32;
/// One saved board: the level id, then (type, colour, dir) for each of the 285 cells.
pub const RECORD_LEN: usize = 1 + CELLS * 3;

/// What a save file holds: completed slots and a saved board per level id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveData {
    pub completed: [bool; PROGRESS_SLOTS],
    /// Indexed by level id; `RuleSet::saved_ids` entries from `decode_save`. `GameState::new`
    /// brings any other length to the game's.
    pub boards: Vec<Option<Board>>,
}

/// A fresh game, with room for Chromatron 1's 128 saved boards.
impl Default for SaveData {
    fn default() -> SaveData {
        SaveData::fresh(&RuleSet::C1)
    }
}

impl SaveData {
    /// A fresh game of `rules`' game.
    pub fn fresh(rules: &RuleSet) -> SaveData {
        SaveData {
            completed: [false; PROGRESS_SLOTS],
            boards: vec![None; rules.saved_ids],
        }
    }
}

/// A save file the original would crash on or misread. The shell renames it to `.bak`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    /// A slot's check bits are neither 0 nor its check value (the original asserts).
    BadCheck { slot: usize, found: u8 },
    /// A record names a level id past the game's `saved_states[]`.
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
                write!(
                    f,
                    "saved board for level id {id}, past the game's saved states"
                )
            }
            SaveError::BadCell { id, cell, bytes } => {
                write!(f, "level id {id} cell {cell} has bytes {bytes:02x?}")
            }
        }
    }
}

impl std::error::Error for SaveError {}

/// `save_slot_check` (C1 @ 0x400c, C2 @ 0x42e4): the 4 check bits stored for a completed slot,
/// 1..=15.
fn check(slot: usize) -> u8 {
    let mut a = 9 * slot as i32;
    a += a << 8;
    a += a >> 4;
    (a % 15 + 1) as u8
}

/// `save_bit_scramble` (C1 @ 0x3f00, C2 @ 0x41d8), for n in 0..128, with the game's
/// `save_bit_perm`.
fn scramble(perm: &[u8; 32], n: usize) -> usize {
    let p = |i: usize| usize::from(perm[i & 31]);
    match n {
        0..32 => p(n),
        32..64 => p(n + 13) + 0x60,
        64..96 => p(n + 20) + 0x20,
        _ => p(n + 5) + 0x40,
    }
}

/// `save_bit_index` (C1 @ 0x3fc8, C2 @ 0x42a0): where check bit `k` of `slot` lives, 0..256.
fn bit_index(perm: &[u8; 32], slot: usize, k: usize) -> usize {
    scramble(perm, (slot & 31) + 32 * k) + 128 * (slot >> 5)
}

pub fn encode_bitmap(rules: &RuleSet, completed: &[bool; PROGRESS_SLOTS]) -> [u8; BITMAP_LEN] {
    let mut bitmap = [0u8; BITMAP_LEN];
    for slot in (0..PROGRESS_SLOTS).filter(|&s| completed[s]) {
        for k in (0..4).filter(|k| check(slot) >> k & 1 != 0) {
            let bit = bit_index(&rules.save_bit_perm, slot, k);
            bitmap[bit >> 3] |= 1 << (bit & 7);
        }
    }
    bitmap
}

pub fn decode_bitmap(
    rules: &RuleSet,
    bitmap: &[u8; BITMAP_LEN],
) -> Result<[bool; PROGRESS_SLOTS], SaveError> {
    let mut completed = [false; PROGRESS_SLOTS];
    for (slot, done) in completed.iter_mut().enumerate() {
        let found = (0..4).fold(0u8, |v, k| {
            let bit = bit_index(&rules.save_bit_perm, slot, k);
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

/// The whole file: bitmap, then every saved board with an id below `rules.saved_ids`, in
/// ascending level-id order.
pub fn encode_save(
    rules: &RuleSet,
    completed: &[bool; PROGRESS_SLOTS],
    boards: &[Option<Board>],
) -> Vec<u8> {
    let mut out = encode_bitmap(rules, completed).to_vec();
    for (id, board) in boards.iter().enumerate().take(rules.saved_ids) {
        if let Some(board) = board {
            out.push(id as u8);
            for cell in &board.cells {
                out.extend([cell.piece as u8, cell.colour, cell.dir]);
            }
        }
    }
    out
}

/// Reads a save file of `rules`' game as the original does: a file shorter than the bitmap means
/// a fresh game, and a truncated last record is dropped. Unlike the original, a bad bitmap, a
/// level id with no saved state, or a bad cell (a type the game doesn't have, colour above 7, dir
/// above 9 on a movable piece or above 7 otherwise) is an error instead of a crash.
pub fn decode_save(rules: &RuleSet, bytes: &[u8]) -> Result<SaveData, SaveError> {
    let mut save = SaveData::fresh(rules);
    let Some((bitmap, mut rest)) = bytes.split_first_chunk::<BITMAP_LEN>() else {
        return Ok(save);
    };
    save.completed = decode_bitmap(rules, bitmap)?;
    while let Some((record, tail)) = rest.split_first_chunk::<RECORD_LEN>() {
        let id = record[0];
        if usize::from(id) >= rules.saved_ids {
            return Err(SaveError::BadLevelId { id });
        }
        let mut board = Board::new(rules.game);
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
            let piece = rules.piece(bytes[0]).ok_or_else(bad)?;
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
    use crate::piece::Piece;
    use spectrograph::Game;

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
            .flat_map(|s| (0..4).map(move |k| bit_index(&RuleSet::C1.save_bit_perm, s, k)))
            .collect();
        seen.sort();
        assert_eq!(seen, (0..256).collect::<Vec<_>>());
    }

    #[test]
    fn bitmap_examples_from_the_notes() {
        let one = encode_bitmap(&RuleSet::C1, &completed([0]));
        assert_eq!(one[0], 0x01);
        assert!(one[1..].iter().all(|&b| b == 0));
        assert_eq!(
            hex(&encode_bitmap(&RuleSet::C1, &completed(0..50))),
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
            assert_eq!(
                decode_bitmap(&RuleSet::C1, &encode_bitmap(&RuleSet::C1, &done)),
                Ok(done)
            );
        }
    }

    #[test]
    fn a_damaged_bitmap_is_an_error() {
        // All bits set: slot 0 reads 0b1111, but its check value is 1.
        assert_eq!(
            decode_bitmap(&RuleSet::C1, &[0xff; BITMAP_LEN]),
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
                let _ = decode_save(&RuleSet::C1, &bytes);
                let mut valid = encode_bitmap(&RuleSet::C1, &completed([1, 2])).to_vec();
                valid.extend(&bytes);
                let _ = decode_save(&RuleSet::C1, &valid);
            }
        }
    }

    #[test]
    fn short_files_are_a_fresh_game() {
        assert_eq!(decode_save(&RuleSet::C1, &[]), Ok(SaveData::default()));
        assert_eq!(
            decode_save(&RuleSet::C1, &[0xff; 31]),
            Ok(SaveData::default())
        );
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
        let bytes = encode_save(&RuleSet::C1, &save.completed, &save.boards);
        assert_eq!(bytes.len(), BITMAP_LEN + 2 * RECORD_LEN);
        assert_eq!(bytes[BITMAP_LEN], 2, "lower id first");
        assert_eq!(bytes[BITMAP_LEN + RECORD_LEN], 9);
        let mirror = BITMAP_LEN + RECORD_LEN + 1 + 3 * board_index(8, 7);
        assert_eq!(bytes[mirror..mirror + 3], [3, 4, 1], "type, colour, dir");
        assert_eq!(decode_save(&RuleSet::C1, &bytes), Ok(save));
    }

    #[test]
    fn a_movable_piece_may_have_dir_8_or_9() {
        let mut board = Board::default();
        board.cells[board_index(3, 3)] = Cell {
            piece: Piece::Tangler,
            colour: 4,
            dir: 9,
        };
        let mut boards = vec![None; RuleSet::C1.saved_ids];
        boards[7] = Some(board.clone());
        let save = decode_save(
            &RuleSet::C1,
            &encode_save(&RuleSet::C1, &completed([]), &boards),
        )
        .unwrap();
        assert_eq!(save.boards[7], Some(board));
        let mut bad = encode_save(&RuleSet::C1, &completed([]), &boards);
        bad[BITMAP_LEN + 1 + 3 * board_index(3, 3) + 2] = 10;
        assert!(matches!(
            decode_save(&RuleSet::C1, &bad),
            Err(SaveError::BadCell { id: 7, .. })
        ));
    }

    #[test]
    fn a_truncated_record_is_dropped() {
        let mut boards = vec![None; RuleSet::C1.saved_ids];
        boards[1] = Some(Board::default());
        boards[2] = Some(Board::default());
        let bytes = encode_save(&RuleSet::C1, &completed([]), &boards);
        let save = decode_save(&RuleSet::C1, &bytes[..bytes.len() - 1]).unwrap();
        assert!(save.boards[1].is_some());
        assert!(save.boards[2].is_none());
    }

    #[test]
    fn bad_records_are_errors() {
        let mut boards = vec![None; RuleSet::C1.saved_ids];
        boards[1] = Some(Board::default());
        let good = encode_save(&RuleSet::C1, &completed([]), &boards);
        let mut bad_id = good.clone();
        bad_id[BITMAP_LEN] = 200;
        assert_eq!(
            decode_save(&RuleSet::C1, &bad_id),
            Err(SaveError::BadLevelId { id: 200 })
        );
        let mut bad_kind = good.clone();
        bad_kind[BITMAP_LEN + 1] = 13;
        assert_eq!(
            decode_save(&RuleSet::C1, &bad_kind),
            Err(SaveError::BadCell {
                id: 1,
                cell: 0,
                bytes: [13, 0, 0]
            })
        );
        let mut bad_dir = good;
        bad_dir[BITMAP_LEN + 3] = 8;
        assert_eq!(
            decode_save(&RuleSet::C1, &bad_dir),
            Err(SaveError::BadCell {
                id: 1,
                cell: 0,
                bytes: [0, 0, 8]
            })
        );
    }

    #[test]
    fn chromatron_2_bitmap_examples_from_the_notes() {
        // re/notes/chromatron2/platform.md P7.
        let one = encode_bitmap(&RuleSet::C2, &completed([0]));
        assert_eq!(one[..3], [0x00, 0x00, 0x10]);
        assert!(one[3..].iter().all(|&b| b == 0));
        assert_eq!(
            hex(&encode_bitmap(&RuleSet::C2, &completed(0..50))),
            "9dd4579637ca28db4eacb9525cf3512d284b114840059064286a091840228622"
        );
    }

    #[test]
    fn chromatron_2_bit_positions_are_a_bijection_and_bitmaps_round_trip() {
        let mut seen: Vec<usize> = (0..PROGRESS_SLOTS)
            .flat_map(|s| (0..4).map(move |k| bit_index(&RuleSet::C2.save_bit_perm, s, k)))
            .collect();
        seen.sort();
        assert_eq!(seen, (0..256).collect::<Vec<_>>());
        let done = completed([3, 17, 40, 63]);
        assert_eq!(
            decode_bitmap(&RuleSet::C2, &encode_bitmap(&RuleSet::C2, &done)),
            Ok(done)
        );
    }

    #[test]
    fn a_bitmap_is_only_readable_by_its_own_game() {
        // platform.md P7: the other game's table usually trips the check.
        let c1 = encode_bitmap(&RuleSet::C1, &completed(0..50));
        assert!(matches!(
            decode_bitmap(&RuleSet::C2, &c1),
            Err(SaveError::BadCheck { .. })
        ));
    }

    #[test]
    fn chromatron_2_saves_ids_0_to_51_only() {
        let mut boards = vec![None; 128];
        boards[51] = Some(Board::new(Game::C2));
        boards[52] = Some(Board::new(Game::C2));
        let bytes = encode_save(&RuleSet::C2, &completed([]), &boards);
        assert_eq!(
            bytes.len(),
            BITMAP_LEN + RECORD_LEN,
            "id 52 has no saved state"
        );
        let save = decode_save(&RuleSet::C2, &bytes).unwrap();
        assert_eq!(save.boards.len(), 52);
        assert_eq!(save.boards[51], Some(Board::new(Game::C2)));
        let mut bad = bytes;
        bad[BITMAP_LEN] = 52;
        assert_eq!(
            decode_save(&RuleSet::C2, &bad),
            Err(SaveError::BadLevelId { id: 52 })
        );
    }

    #[test]
    fn decoded_boards_belong_to_the_saves_game() {
        let mut boards = vec![None; 52];
        let mut board = Board::new(Game::C2);
        board.cells[board_index(1, 1)] = Cell {
            piece: Piece::Splitter,
            colour: 4,
            dir: 3,
        };
        boards[9] = Some(board.clone());
        let bytes = encode_save(&RuleSet::C2, &completed([1]), &boards);
        let save = decode_save(&RuleSet::C2, &bytes).unwrap();
        assert!(save.completed[1]);
        assert_eq!(save.boards[9].as_ref().map(|b| b.game), Some(Game::C2));
        assert_eq!(save.boards[9], Some(board));
    }

    #[test]
    fn chromatron_2_saves_hold_its_new_pieces() {
        let mut board = Board::new(Game::C2);
        for (x, piece, dir) in [
            (0, Piece::MultiFilter, 9),
            (1, Piece::Twister, 0),
            (2, Piece::Complementor, 1),
        ] {
            board.cells[board_index(x, 0)] = Cell {
                piece,
                colour: 4,
                dir,
            };
        }
        let mut boards = vec![None; 52];
        boards[4] = Some(board.clone());
        let bytes = encode_save(&RuleSet::C2, &completed([]), &boards);
        assert_eq!(bytes[BITMAP_LEN + 1..BITMAP_LEN + 7], [13, 4, 9, 14, 4, 0]);
        assert_eq!(
            decode_save(&RuleSet::C2, &bytes).unwrap().boards[4],
            Some(board)
        );
        let mut bad_dir = bytes;
        bad_dir[BITMAP_LEN + 6] = 8;
        assert!(
            matches!(
                decode_save(&RuleSet::C2, &bad_dir),
                Err(SaveError::BadCell { id: 4, cell: 1, .. })
            ),
            "the twister is fixed, so dir 8 is corrupt"
        );
    }
}
