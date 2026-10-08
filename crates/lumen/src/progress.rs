//! Which strip slots are open (`update_unlocked`: C1 logic.md L6 @ 0x4050, C2 logic.md L6
//! @ 0x4328).

use crate::rule_set::Unlock;

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

/// Chromatron 2: opens every completed slot plus the `n` lowest uncompleted ones, where `n`
/// grows with the number `k` of completed slots: 5, but 4 for `k` = 1 or 2, then +1 above 5, and
/// +2 above each of 19, 29 and 39. Never closes a slot.
pub fn update_unlocked_by_count(
    completed: &[bool; PROGRESS_SLOTS],
    unlocked: &mut [bool; PROGRESS_SLOTS],
) {
    let k = completed.iter().filter(|&&c| c).count();
    let mut n = if k == 1 || k == 2 { 4 } else { 5 };
    n += usize::from(k > 5) + 2 * (usize::from(k > 19) + usize::from(k > 29) + usize::from(k > 39));
    for (done, open) in completed.iter().zip(unlocked.iter_mut()) {
        if *done {
            *open = true;
        } else if n > 0 {
            *open = true;
            n -= 1;
        }
    }
}

impl Unlock {
    /// Recomputes `unlocked` from `completed` with this game's rule.
    pub fn apply(self, completed: &[bool; PROGRESS_SLOTS], unlocked: &mut [bool; PROGRESS_SLOTS]) {
        match self {
            Unlock::HighestCompleted => update_unlocked(completed, unlocked),
            Unlock::CompletedCount => update_unlocked_by_count(completed, unlocked),
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

    fn open_after_in(rule: Unlock, done: &[usize]) -> Vec<usize> {
        let mut completed = [false; PROGRESS_SLOTS];
        for &s in done {
            completed[s] = true;
        }
        let mut unlocked = [false; PROGRESS_SLOTS];
        rule.apply(&completed, &mut unlocked);
        (0..PROGRESS_SLOTS).filter(|&s| unlocked[s]).collect()
    }

    /// Chromatron 2's open slots after completing the first `k` slots in order.
    fn c2_open_after_first(k: usize) -> Vec<usize> {
        open_after_in(Unlock::CompletedCount, &(0..k).collect::<Vec<_>>())
    }

    #[test]
    fn the_rule_for_chromatron_1_is_update_unlocked() {
        assert_eq!(
            open_after_in(Unlock::HighestCompleted, &[9]),
            open_after(&[9])
        );
    }

    #[test]
    fn chromatron_2_opens_five_levels_at_the_start() {
        assert_eq!(c2_open_after_first(0), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn chromatron_2_counts_completed_slots_with_a_dip_after_the_first_win() {
        // logic.md L6 table: k → uncompleted slots open.
        for (k, n) in [
            (1, 4),
            (2, 4),
            (3, 5),
            (5, 5),
            (6, 6),
            (19, 6),
            (20, 8),
            (29, 8),
            (30, 10),
            (39, 10),
            (40, 12),
        ] {
            assert_eq!(
                c2_open_after_first(k),
                (0..k + n).collect::<Vec<_>>(),
                "k={k}"
            );
        }
    }

    #[test]
    fn chromatron_2_opens_the_lowest_uncompleted_slots_wherever_the_wins_are() {
        assert_eq!(
            open_after_in(Unlock::CompletedCount, &[10]),
            [0, 1, 2, 3, 10],
            "one win anywhere: four more open"
        );
        assert_eq!(
            open_after_in(Unlock::CompletedCount, &[0, 2, 4]),
            [0, 1, 2, 3, 4, 5, 6, 7]
        );
    }

    #[test]
    fn chromatron_2_with_40_wins_opens_everything_on_the_strip() {
        assert_eq!(c2_open_after_first(40), (0..52).collect::<Vec<_>>());
    }
}
