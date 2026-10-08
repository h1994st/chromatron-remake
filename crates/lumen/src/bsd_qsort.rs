//! An exact port of FreeBSD's `qsort` (Bentley & McIlroy, "Engineering a Sort Function"), which
//! Mac OS X 10.2/10.3's libc shipped. It is not stable, and the original's toolbox order and copied
//! solution text depend on how it orders equal elements (data.md D3, platform.md P8), so only
//! this sort may be used for those two.

use std::cmp::Ordering::{self, Equal, Greater, Less};

/// Sorts `v` exactly as the original's `qsort(v, n, size, cmp)` would.
pub fn bsd_qsort<T>(v: &mut [T], cmp: &mut impl FnMut(&T, &T) -> Ordering) {
    let mut a = 0;
    let mut n = v.len();
    loop {
        if n < 7 {
            insertion_sort(&mut v[a..a + n], cmp);
            return;
        }
        let mut pm = a + n / 2;
        if n > 7 {
            let mut pl = a;
            let mut pn = a + n - 1;
            if n > 40 {
                let d = n / 8;
                pl = med3(v, pl, pl + d, pl + 2 * d, cmp);
                pm = med3(v, pm - d, pm, pm + d, cmp);
                pn = med3(v, pn - 2 * d, pn - d, pn, cmp);
            }
            pm = med3(v, pl, pm, pn, cmp);
        }
        v.swap(a, pm);
        let (mut pa, mut pb) = (a + 1, a + 1);
        let (mut pc, mut pd) = (a + n - 1, a + n - 1);
        let mut swapped = false;
        loop {
            while pb <= pc {
                let r = cmp(&v[pb], &v[a]);
                if r == Greater {
                    break;
                }
                if r == Equal {
                    swapped = true;
                    v.swap(pa, pb);
                    pa += 1;
                }
                pb += 1;
            }
            while pb <= pc {
                let r = cmp(&v[pc], &v[a]);
                if r == Less {
                    break;
                }
                if r == Equal {
                    swapped = true;
                    v.swap(pc, pd);
                    pd -= 1;
                }
                pc -= 1;
            }
            if pb > pc {
                break;
            }
            v.swap(pb, pc);
            swapped = true;
            pb += 1;
            pc -= 1;
        }
        if !swapped {
            insertion_sort(&mut v[a..a + n], cmp);
            return;
        }
        let pn = a + n;
        let r = (pa - a).min(pb - pa);
        vecswap(v, a, pb - r, r);
        let r = (pd - pc).min(pn - pd - 1);
        vecswap(v, pb, pn - r, r);
        let r = pb - pa;
        if r > 1 {
            bsd_qsort(&mut v[a..a + r], cmp);
        }
        let r = pd - pc;
        if r <= 1 {
            return;
        }
        // Iterate rather than recurse on the right part, as the original does.
        a = pn - r;
        n = r;
    }
}

fn insertion_sort<T>(v: &mut [T], cmp: &mut impl FnMut(&T, &T) -> Ordering) {
    for pm in 1..v.len() {
        let mut pl = pm;
        while pl > 0 && cmp(&v[pl - 1], &v[pl]) == Greater {
            v.swap(pl, pl - 1);
            pl -= 1;
        }
    }
}

fn med3<T>(
    v: &[T],
    a: usize,
    b: usize,
    c: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) -> usize {
    if cmp(&v[a], &v[b]) == Less {
        if cmp(&v[b], &v[c]) == Less {
            b
        } else if cmp(&v[a], &v[c]) == Less {
            c
        } else {
            a
        }
    } else if cmp(&v[b], &v[c]) == Greater {
        b
    } else if cmp(&v[a], &v[c]) == Less {
        a
    } else {
        c
    }
}

fn vecswap<T>(v: &mut [T], a: usize, b: usize, n: usize) {
    for i in 0..n {
        v.swap(a + i, b + i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator, so the test needs no extra crate.
    fn lcg(seed: &mut u32) -> u32 {
        *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        *seed >> 16
    }

    #[test]
    fn sorts_like_any_sort_on_keys() {
        let mut seed = 1;
        for len in 0..120 {
            let mut v: Vec<(u32, usize)> = (0..len).map(|i| (lcg(&mut seed) % 6, i)).collect();
            let mut expected: Vec<u32> = v.iter().map(|e| e.0).collect();
            expected.sort();
            bsd_qsort(&mut v, &mut |x, y| x.0.cmp(&y.0));
            assert_eq!(
                v.iter().map(|e| e.0).collect::<Vec<_>>(),
                expected,
                "len {len}"
            );
            let mut ids: Vec<usize> = v.iter().map(|e| e.1).collect();
            ids.sort();
            assert_eq!(
                ids,
                (0..len).collect::<Vec<_>>(),
                "len {len} lost an element"
            );
        }
    }

    #[test]
    fn short_inputs_use_a_stable_insertion_sort() {
        let mut v = [(2, 'a'), (1, 'b'), (2, 'c'), (1, 'd'), (0, 'e'), (2, 'f')];
        bsd_qsort(&mut v, &mut |x, y| x.0.cmp(&y.0));
        assert_eq!(
            v,
            [(0, 'e'), (1, 'b'), (1, 'd'), (2, 'a'), (2, 'c'), (2, 'f')]
        );
    }

    #[test]
    fn longer_inputs_are_not_stable() {
        // n = 7: the middle element 'd' is swapped to the front as the pivot, which moves 'a'
        // behind 'c'; no partition swaps happen, so the insertion sort keeps that order.
        let mut v = [
            (1, 'a'),
            (1, 'b'),
            (1, 'c'),
            (0, 'd'),
            (1, 'e'),
            (1, 'f'),
            (1, 'g'),
        ];
        bsd_qsort(&mut v, &mut |x, y| x.0.cmp(&y.0));
        let order: String = v.iter().map(|e| e.1).collect();
        assert_eq!(order, "dbcaefg");
    }
}
