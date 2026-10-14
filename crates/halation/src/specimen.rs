//! `specimen`: the synthetic pieces sheet (spec §15): every piece kind, colour and direction,
//! and the lit and spoilt target states, on two 15 × 15 boards. Targets and gates get a
//! hand-made beam field (a beam entering from the south), so their lights show.

use lumen::{BeamField, Board, Cell, Piece, board_index};
use spectrograph::Game;

pub struct Specimen {
    pub game: Game,
    pub board: Board,
    pub field: BeamField,
}

/// Fills a page row-major from cell 0.
struct Page {
    specimen: Specimen,
    next: usize,
}

impl Page {
    fn new(game: Game) -> Page {
        Page {
            specimen: Specimen {
                game,
                board: Board::new(game),
                field: BeamField::default(),
            },
            next: 0,
        }
    }

    fn put(&mut self, piece: Piece, colour: u8, dir: u8) -> usize {
        let i = self.next;
        assert!(i < board_index(0, 13), "rows 13–14 are kept for walls");
        self.specimen.board.cells[i] = Cell { piece, colour, dir };
        self.next += 1;
        i
    }

    /// A beam of `mask` entering cell `i` travelling north (from its south side).
    fn feed(&mut self, i: usize, mask: u8) {
        self.specimen.field.inbound[i][0] = mask;
    }
}

pub fn pages() -> [Specimen; 2] {
    let mut one = Page::new(Game::C1);
    for colour in [4, 2, 1, 7] {
        for dir in 0..8 {
            one.put(Piece::Laser, colour, dir);
        }
    }
    for piece in [
        Piece::Mirror,
        Piece::Bender,
        Piece::Prism,
        Piece::Doppler,
        Piece::Splitter,
        Piece::Tangler,
        Piece::Conduit,
    ] {
        for dir in 0..8 {
            one.put(piece, 0, dir);
        }
    }
    for colour in [4, 2, 1] {
        for dir in 0..8 {
            one.put(Piece::Filter, colour, dir);
        }
    }
    one.put(Piece::Teleporter, 0, 0);
    for colour in 0..8u8 {
        // The colour plus its lowest missing bit (spoilt), or red alone for white (a partial).
        let other = if colour == 7 {
            4
        } else {
            colour | (!colour & 7 & (!colour & 7).wrapping_neg())
        };
        // A black target is lit by nothing, so its second state is white (spoilt), not a repeat.
        let lit = if colour == 0 { Some(7) } else { Some(colour) };
        for received in [None, lit, Some(other)] {
            let i = one.put(Piece::Pinwheel, colour, 0);
            if let Some(mask) = received.filter(|&m| m != 0) {
                one.feed(i, mask);
            }
        }
    }
    walls(&mut one.specimen.board);
    let mut two = Page::new(Game::C4);
    for piece in [
        Piece::MultiFilter,
        Piece::Complementor,
        Piece::LogicGate,
        Piece::DualReflector,
        Piece::QuadBender,
        Piece::PrimaryLogicGate,
    ] {
        for dir in 0..8 {
            two.put(piece, 0, dir);
        }
    }
    for dir in 0..8 {
        let i = two.put(Piece::LogicGate, 0, dir);
        two.specimen.field.outbound[i][usize::from(dir)] = 7;
    }
    for piece in [
        Piece::Twister,
        Piece::Starburst,
        Piece::MobileTeleporter,
        Piece::OccupiedSquare,
        Piece::Teleporter,
    ] {
        two.put(piece, 0, 0);
    }
    walls(&mut two.specimen.board);
    [one.specimen, two.specimen]
}

/// An L of three, a 2 × 2 block and a lone wall in rows 13–14: every joint the slabs have.
fn walls(board: &mut Board) {
    for (x, y) in [
        (0, 13),
        (1, 13),
        (2, 13),
        (0, 14),
        (4, 13),
        (5, 13),
        (4, 14),
        (5, 14),
        (7, 13),
    ] {
        board.cells[board_index(x, y)] = Cell {
            piece: Piece::Wall,
            colour: 0,
            dir: 0,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen::BOARD_CELLS;

    #[test]
    fn the_sheet_shows_every_kind_and_target_state() {
        let pages = pages();
        let kinds: Vec<Piece> = pages
            .iter()
            .flat_map(|p| p.board.cells[..BOARD_CELLS].iter().map(|c| c.piece))
            .collect();
        for piece in Piece::ALL.into_iter().filter(|&p| p != Piece::Empty) {
            assert!(kinds.contains(&piece), "{piece:?} is missing");
        }
        let lasers = pages[0]
            .board
            .cells
            .iter()
            .filter(|c| c.piece == Piece::Laser)
            .count();
        assert_eq!(lasers, 32, "4 colours × 8 directions");
        let targets: Vec<usize> = (0..BOARD_CELLS)
            .filter(|&i| pages[0].board.cells[i].piece == Piece::Pinwheel)
            .collect();
        assert_eq!(targets.len(), 24, "8 colours × unlit, lit, spoilt");
        assert!(targets.iter().any(|&i| pages[0].field.received(i)
            == pages[0].board.cells[i].colour
            && pages[0].board.cells[i].colour != 0));
        assert_eq!(
            pages[1].game,
            Game::C4,
            "mobile teleporters draw C4's brackets"
        );
        assert!(
            (0..BOARD_CELLS).any(
                |i| matches!(pages[1].board.cells[i].piece, Piece::LogicGate)
                    && pages[1].field.outbound[i][usize::from(pages[1].board.cells[i].dir & 7)]
                        != 0
            ),
            "an emitting gate"
        );
    }
}
