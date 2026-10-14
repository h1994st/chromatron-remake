//! `patchwork`: a few rects standing for a region: a paint's damage, a light tile's changed
//! parts, the windows a develop recomputes. Adding past capacity merges the two rects whose hull
//! grows least, so it never allocates and never loses area. (Spec §8 "Damage", ruling M2(g).)

use crate::DeviceRect;
use crate::etch::clip_rect;

#[derive(Clone, Copy, Debug)]
pub struct Patchwork<const N: usize> {
    rects: [DeviceRect; N],
    len: usize,
}

/// Whether `r` holds no pixel.
pub(crate) fn is_empty(r: DeviceRect) -> bool {
    r.left >= r.right || r.top >= r.bottom
}

/// How many px a non-empty `r` holds.
pub(crate) fn area(r: DeviceRect) -> i64 {
    i64::from(r.right - r.left) * i64::from(r.bottom - r.top)
}

fn contains(a: DeviceRect, b: DeviceRect) -> bool {
    a.left <= b.left && a.top <= b.top && a.right >= b.right && a.bottom >= b.bottom
}

/// Whether `a` and `b` share a pixel.
pub(crate) fn meets(a: DeviceRect, b: DeviceRect) -> bool {
    clip_rect(a, b).is_some()
}

/// The smallest rect holding both.
pub fn hull(a: DeviceRect, b: DeviceRect) -> DeviceRect {
    DeviceRect::new(
        a.left.min(b.left),
        a.top.min(b.top),
        a.right.max(b.right),
        a.bottom.max(b.bottom),
    )
}

impl<const N: usize> Default for Patchwork<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Patchwork<N> {
    pub const fn new() -> Self {
        Patchwork {
            rects: [DeviceRect::new(0, 0, 0, 0); N],
            len: 0,
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[DeviceRect] {
        &self.rects[..self.len]
    }

    /// Adds `r`: nothing when it is empty or inside a held rect; it replaces the held rects it
    /// contains; with no room left, the two of the held rects and `r` whose hull grows least merge.
    pub fn add(&mut self, r: DeviceRect) {
        if is_empty(r) || self.as_slice().iter().any(|&h| contains(h, r)) {
            return;
        }
        let mut k = 0;
        while k < self.len {
            if contains(r, self.rects[k]) {
                self.len -= 1;
                self.rects[k] = self.rects[self.len];
            } else {
                k += 1;
            }
        }
        if self.len < N {
            self.rects[self.len] = r;
            self.len += 1;
            return;
        }
        // N + 1 candidates: the held rects and `r` (index N).
        let at = |rects: &[DeviceRect; N], i: usize| if i == N { r } else { rects[i] };
        let mut best = (0, 1, i64::MAX);
        for i in 0..=N {
            for j in i + 1..=N {
                let (a, b) = (at(&self.rects, i), at(&self.rects, j));
                let growth = area(hull(a, b)) - area(a) - area(b);
                if growth < best.2 {
                    best = (i, j, growth);
                }
            }
        }
        let (i, j, _) = best;
        // The merge takes i; when both were held, `r` takes j.
        self.rects[i] = hull(at(&self.rects, i), at(&self.rects, j));
        if j < N {
            self.rects[j] = r;
        }
        // The merge may now contain other held rects: drop them (moving the last into the gap,
        // and following the merge if it was the one moved).
        let mut keep = i;
        let mut k = 0;
        while k < self.len {
            if k != keep && contains(self.rects[keep], self.rects[k]) {
                self.len -= 1;
                if self.len == keep {
                    keep = k;
                }
                self.rects[k] = self.rects[self.len];
            } else {
                k += 1;
            }
        }
    }

    /// Merges every two held rects that overlap into their hull, until none do: each point is
    /// then in exactly one rect (a develop window is recomputed once).
    pub fn coalesce(&mut self) {
        'again: loop {
            for i in 0..self.len {
                for j in i + 1..self.len {
                    if meets(self.rects[i], self.rects[j]) {
                        self.rects[i] = hull(self.rects[i], self.rects[j]);
                        self.len -= 1;
                        self.rects[j] = self.rects[self.len];
                        continue 'again;
                    }
                }
            }
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(l: i32, t: i32, rt: i32, b: i32) -> DeviceRect {
        DeviceRect::new(l, t, rt, b)
    }

    fn covered(p: &Patchwork<4>, x: i32, y: i32) -> bool {
        p.as_slice()
            .iter()
            .any(|q| (q.left..q.right).contains(&x) && (q.top..q.bottom).contains(&y))
    }

    #[test]
    fn empty_and_contained_rects_add_nothing_and_a_container_replaces() {
        let mut p = Patchwork::<4>::new();
        p.add(r(5, 5, 5, 9));
        assert!(p.is_empty(), "an empty rect");
        p.add(r(0, 0, 10, 10));
        p.add(r(2, 2, 4, 4));
        assert_eq!(p.as_slice(), &[r(0, 0, 10, 10)], "inside a held rect");
        p.add(r(20, 0, 22, 2));
        p.add(r(-5, -5, 30, 30));
        assert_eq!(
            p.as_slice(),
            &[r(-5, -5, 30, 30)],
            "a container replaces both"
        );
    }

    #[test]
    fn past_capacity_the_cheapest_pair_merges_and_nothing_is_lost() {
        let mut p = Patchwork::<4>::new();
        let rects = [
            r(0, 0, 2, 2),
            r(100, 0, 102, 2),
            r(0, 100, 2, 102),
            r(100, 100, 102, 102),
            r(3, 0, 5, 2),
        ];
        for q in rects {
            p.add(q);
        }
        assert_eq!(p.as_slice().len(), 4);
        assert!(
            p.as_slice().contains(&r(0, 0, 5, 2)),
            "the two near ones merged: {:?}",
            p.as_slice()
        );
        for q in rects {
            assert!(
                covered(&p, q.left, q.top) && covered(&p, q.right - 1, q.bottom - 1),
                "{q:?} still covered"
            );
        }
    }

    #[test]
    fn coalesce_leaves_no_two_rects_meeting() {
        let mut p = Patchwork::<4>::new();
        for q in [
            r(0, 0, 10, 10),
            r(8, 8, 20, 20),
            r(50, 50, 60, 60),
            r(19, 0, 25, 9),
        ] {
            p.add(q);
        }
        p.coalesce();
        let s = p.as_slice();
        for (i, a) in s.iter().enumerate() {
            for b in &s[i + 1..] {
                assert!(!meets(*a, *b), "{a:?} meets {b:?}");
            }
        }
        for (x, y) in [(0, 0), (19, 19), (55, 55), (24, 1)] {
            assert!(covered(&p, x, y), "({x}, {y})");
        }
    }
}
