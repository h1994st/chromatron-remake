//! Spec §15: crossing glints are exactly the cells where classic draws its centre dot, but for
//! targets (no sparkle on targets: a bright centre always means lit), on every original level,
//! with the pieces in the toolbox and at their stored (solution) positions. No target glints.

use halation::beamline::{Beamline, GlintKind};
use lumen::{BOARD_CELLS, BeamField, Board, Piece, move_pieces_to_toolbox, trace};
use spectrograph::Game;

/// Classic's rule, copied from `draw_cell_beams` (lumen/src/render.rs:313–331). If lumen's rule
/// changes, this copy changes with it.
fn classic_dot(field: &BeamField, cell: usize) -> u8 {
    let (inbound, outbound) = (&field.inbound[cell], &field.outbound[cell]);
    let (mut axes, mut dot) = (0u8, 0u8);
    for s in 0..8 {
        if outbound[s] | inbound[(s + 4) & 7] == 0 {
            continue;
        }
        if inbound[s] == outbound[s] {
            axes |= 1 << (s & 3);
            dot |= inbound[s];
        }
    }
    if dot != 0 && axes.count_ones() >= 2 {
        dot
    } else {
        0
    }
}

#[test]
fn crossing_glints_are_classics_centre_dots_on_every_level() {
    let mut line = Beamline::new();
    let mut field = BeamField::default();
    for game in Game::ALL {
        let data = spectrograph::decode(game, &game.read_original().unwrap()).unwrap();
        for level in data.levels.iter() {
            let stored = Board::from_level(level);
            let mut start = stored.clone();
            move_pieces_to_toolbox(&mut start);
            for board in [&start, &stored] {
                trace(board, &mut field);
                line.rebuild(board, &field, None);
                for cell in 0..BOARD_CELLS {
                    if board.cells[cell].piece == Piece::Pinwheel {
                        assert!(
                            line.glints().iter().all(|g| g.cell != cell),
                            "{game} level id {} target {cell} glints",
                            level.id
                        );
                    }
                    let glint = line
                        .glints()
                        .iter()
                        .find(|g| g.cell == cell && g.kind == GlintKind::Crossing)
                        .map(|g| g.mask);
                    // No sparkle on targets: classic's dot there is left out.
                    let dot = if board.cells[cell].piece == Piece::Pinwheel {
                        0
                    } else {
                        classic_dot(&field, cell)
                    };
                    assert_eq!(
                        glint.unwrap_or(0),
                        dot,
                        "{game} level id {} cell {cell}",
                        level.id
                    );
                }
            }
        }
    }
}
