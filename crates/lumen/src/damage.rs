//! The frame rects waiting to be repainted, merged across events between two presents.

use crate::game_state::Redraw;
use crate::layout::Rect;

/// More rects than this between two presents become one full repaint.
pub const MAX_DAMAGE_RECTS: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Damage {
    #[default]
    None,
    Full,
    Rects {
        rects: [Rect; MAX_DAMAGE_RECTS],
        len: usize,
    },
}

impl Damage {
    /// Adds what one `Effects::redraw` asks for.
    pub fn add(&mut self, redraw: Redraw) {
        match redraw {
            Redraw::None => {}
            Redraw::Full => *self = Damage::Full,
            Redraw::Rects(new) => {
                for rect in new {
                    self.add_rect(rect);
                }
            }
        }
    }

    fn add_rect(&mut self, rect: Rect) {
        match self {
            Damage::Full => {}
            Damage::None => {
                let mut rects = [Rect::default(); MAX_DAMAGE_RECTS];
                rects[0] = rect;
                *self = Damage::Rects { rects, len: 1 };
            }
            Damage::Rects { rects, len } => {
                if *len == MAX_DAMAGE_RECTS {
                    *self = Damage::Full;
                } else {
                    rects[*len] = rect;
                    *len += 1;
                }
            }
        }
    }

    /// Returns the pending damage and leaves `None` behind.
    pub fn take(&mut self) -> Damage {
        std::mem::take(self)
    }

    pub fn is_none(&self) -> bool {
        *self == Damage::None
    }

    /// The rects of a partial repaint (empty for `None` and `Full`).
    pub fn rects(&self) -> &[Rect] {
        match self {
            Damage::Rects { rects, len } => &rects[..*len],
            _ => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Rect = Rect::new(0, 0, 60, 60);
    const B: Rect = Rect::new(10, 10, 35, 35);

    #[test]
    fn rects_accumulate_until_taken() {
        let mut damage = Damage::default();
        assert!(damage.is_none());
        damage.add(Redraw::None);
        assert!(damage.is_none());
        damage.add(Redraw::Rects([A, B]));
        damage.add(Redraw::Rects([B, A]));
        assert_eq!(damage.rects(), [A, B, B, A]);
        let taken = damage.take();
        assert_eq!(taken.rects().len(), 4);
        assert!(damage.is_none());
    }

    #[test]
    fn full_absorbs_everything() {
        let mut damage = Damage::default();
        damage.add(Redraw::Rects([A, B]));
        damage.add(Redraw::Full);
        damage.add(Redraw::Rects([A, B]));
        assert_eq!(damage, Damage::Full);
        assert!(damage.rects().is_empty());
    }

    #[test]
    fn too_many_rects_become_full() {
        let mut damage = Damage::default();
        for _ in 0..MAX_DAMAGE_RECTS / 2 {
            damage.add(Redraw::Rects([A, B]));
        }
        assert_eq!(damage.rects().len(), MAX_DAMAGE_RECTS);
        damage.add(Redraw::Rects([A, B]));
        assert_eq!(damage, Damage::Full);
    }
}
