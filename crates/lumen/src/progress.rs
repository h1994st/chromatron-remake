//! Which strip slots are open (logic.md L6, `update_unlocked` @ 0x4050).

/// Length of the original's `completed[]` and `unlocked[]` arrays. Only slots 0–49 are shown.
pub const PROGRESS_SLOTS: usize = 64;

/// Opens every completed slot plus the `n` lowest uncompleted ones, where `n` grows with the
/// highest completed slot `h`: `1 + (h > 8) + (h > 18) + (h > 25) + 2·(h > 34)`. Never closes a slot.
pub fn update_unlocked(completed: &[bool; PROGRESS_SLOTS], unlocked: &mut [bool; PROGRESS_SLOTS]) {
    let h = completed.iter().rposition(|&c| c).map_or(-1, |s| s as i32);
    let mut n =
        1 + i32::from(h > 8) + i32::from(h > 18) + i32::from(h > 25) + 2 * i32::from(h > 34);
    for (done, open) in completed.iter().zip(unlocked.iter_mut()) {
        if *done {
            *open = true;
        } else if n > 0 {
            *open = true;
            n -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_after(done: &[usize]) -> Vec<usize> {
        let mut completed = [false; PROGRESS_SLOTS];
        for &s in done {
            completed[s] = true;
        }
        let mut unlocked = [false; PROGRESS_SLOTS];
        update_unlocked(&completed, &mut unlocked);
        (0..PROGRESS_SLOTS).filter(|&s| unlocked[s]).collect()
    }

    #[test]
    fn a_fresh_game_opens_slot_0_only() {
        assert_eq!(open_after(&[]), [0]);
    }

    #[test]
    fn the_number_of_open_levels_grows_at_the_thresholds() {
        assert_eq!(open_after(&[0]), [0, 1]);
        assert_eq!(open_after(&[8]), [0, 8]);
        assert_eq!(
            open_after(&[9]),
            [0, 1, 9],
            "slot 10 done: two open (hint at slot 11)"
        );
        assert_eq!(open_after(&[18]), [0, 1, 18]);
        assert_eq!(open_after(&[19]), [0, 1, 2, 19]);
        assert_eq!(open_after(&[25]), [0, 1, 2, 25]);
        assert_eq!(open_after(&[26]), [0, 1, 2, 3, 26]);
        assert_eq!(open_after(&[34]), [0, 1, 2, 3, 34]);
        assert_eq!(open_after(&[35]), [0, 1, 2, 3, 4, 5, 35]);
    }

    #[test]
    fn finishing_all_50_opens_slots_beyond_the_strip() {
        assert_eq!(
            open_after(&(0..50).collect::<Vec<_>>()),
            (0..56).collect::<Vec<_>>()
        );
    }

    #[test]
    fn unlocked_slots_stay_unlocked() {
        let mut unlocked = [false; PROGRESS_SLOTS];
        unlocked[30] = true;
        update_unlocked(&[false; PROGRESS_SLOTS], &mut unlocked);
        assert!(unlocked[0] && unlocked[30]);
    }
}
