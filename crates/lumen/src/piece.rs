//! Piece kinds, directions and colour masks (data.md D2, logic.md L1–L2).

/// A direction 0–7 = N, NE, E, SE, S, SW, W, NW: clockwise in 45° steps, y grows downwards.
pub type Dir = u8;
/// A colour mask: 1 blue, 2 green, 4 red; 0 is black (no light), 7 white.
pub type Colour = u8;

pub const BLUE: Colour = 1;
pub const GREEN: Colour = 2;
pub const RED: Colour = 4;
pub const WHITE: Colour = 7;

/// Column step per direction (`dir_dx` @ 0x8044).
pub const DX: [i8; 8] = [0, 1, 1, 1, 0, -1, -1, -1];
/// Row step per direction (`dir_dy` @ 0x8064).
pub const DY: [i8; 8] = [-1, -1, 0, 1, 1, 1, 0, -1];

/// `(dir + steps) & 7`.
pub fn turn(dir: Dir, steps: u8) -> Dir {
    dir.wrapping_add(steps) & 7
}

/// A cell's `type` byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Piece {
    #[default]
    Empty = 0,
    Wall = 1,
    Laser = 2,
    Mirror = 3,
    Bender = 4,
    Filter = 5,
    Prism = 6,
    Doppler = 7,
    Splitter = 8,
    Tangler = 9,
    Pinwheel = 10,
    Conduit = 11,
    Teleporter = 12,
}

impl Piece {
    pub const ALL: [Piece; 13] = [
        Piece::Empty,
        Piece::Wall,
        Piece::Laser,
        Piece::Mirror,
        Piece::Bender,
        Piece::Filter,
        Piece::Prism,
        Piece::Doppler,
        Piece::Splitter,
        Piece::Tangler,
        Piece::Pinwheel,
        Piece::Conduit,
        Piece::Teleporter,
    ];

    pub fn from_byte(byte: u8) -> Option<Piece> {
        Piece::ALL.get(usize::from(byte)).copied()
    }

    /// `is_movable` @ 0x34d8: types 3–9 can be dragged and rotated; everything else is fixed.
    pub fn is_movable(self) -> bool {
        (self as u8).wrapping_sub(3) < 7
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_round_trip_and_reject_unknown_kinds() {
        for piece in Piece::ALL {
            assert_eq!(Piece::from_byte(piece as u8), Some(piece));
        }
        assert_eq!(Piece::from_byte(13), None);
    }

    #[test]
    fn exactly_mirror_to_tangler_are_movable() {
        let movable: Vec<Piece> = Piece::ALL.into_iter().filter(|p| p.is_movable()).collect();
        assert_eq!(
            movable,
            [
                Piece::Mirror,
                Piece::Bender,
                Piece::Filter,
                Piece::Prism,
                Piece::Doppler,
                Piece::Splitter,
                Piece::Tangler
            ]
        );
    }

    #[test]
    fn turning_wraps_around() {
        assert_eq!(turn(7, 1), 0);
        assert_eq!(turn(0, 7), 7);
        assert_eq!(turn(3, 4), 7);
    }
}
