//! Rendering real Chromatron 4 levels.

mod c4_game;

use c4_game::C4;
use lumen::{
    BOARD_CELLS, Board, FRAME_HEIGHT, FRAME_WIDTH, GameState, InputEvent, Piece, Rect, SaveData,
    encode_solution, render,
};
use spectrograph::SPRITE_SIZE;

#[test]
fn the_game_opens_on_crosswards() {
    let state = GameState::new(&C4, SaveData::default());
    let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    let texts = render(&state, &mut frame);
    assert_eq!(state.level_id(), 49);
    assert_eq!(
        (texts.as_slice()[0].text, texts.as_slice()[0].rect),
        ("Crosswards", Rect::new(50, 385, 300, 420))
    );
}

#[test]
fn every_level_renders_at_the_start_and_solved() {
    let mut state = GameState::new(&C4, SaveData::default());
    let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    for slot in 0..C4.level_order.len() {
        state.goto_slot(slot);
        state.handle(InputEvent::Key(b'r'));
        render(&state, &mut frame);
        let level = C4.slot_level(slot).unwrap();
        let text = encode_solution(&Board::from_level(level), slot).unwrap();
        state.handle(InputEvent::Paste(text));
        let texts = render(&state, &mut frame);
        assert!(state.is_won(), "slot {}", slot + 1);
        assert_eq!(texts.as_slice()[0].text, "You win!", "slot {}", slot + 1);
        assert_eq!(texts.as_slice()[1].text, level.name, "slot {}", slot + 1);
    }
}

/// Every pixel of `sprite` from palette index 16 up is on screen in the cell's interior (beams
/// are drawn under those; the outer two pixels can carry a neighbour's beam end).
fn assert_drawn(frame: &[u32], index: usize, sprite: usize) {
    let (x, y) = (index % 15, index / 15);
    let (left, top) = (48 + 24 * x, 18 + 24 * y);
    let pixels = &C4.sprites[sprite];
    for row in 2..SPRITE_SIZE - 1 {
        for col in 2..SPRITE_SIZE - 1 {
            let p = pixels[row * SPRITE_SIZE + col];
            if p >= 16 {
                assert_eq!(
                    frame[(top + row) * FRAME_WIDTH + left + col],
                    C4.palette[usize::from(p)],
                    "sprite {sprite} at ({x},{y}), pixel ({col},{row})"
                );
            }
        }
    }
}

#[test]
fn the_new_pieces_and_the_retouched_teleporter_are_drawn_with_their_own_sprites() {
    let mut state = GameState::new(&C4, SaveData::default());
    let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    // Solved: slots 4 (logic gate), 9 (starburst), 19 (mobile teleporter), 26 (occupied square),
    // 35 (dual-reflector), 41 (primary logic gate), 2 (quad-bender) and 39 (teleporters).
    let mut seen = Vec::new();
    for slot in [3, 8, 18, 25, 34, 40, 1, 38] {
        state.goto_slot(slot);
        let board = Board::from_level(C4.slot_level(slot).unwrap());
        state.handle(InputEvent::Paste(encode_solution(&board, slot).unwrap()));
        render(&state, &mut frame);
        for index in 0..BOARD_CELLS {
            let cell = state.board().cells[index];
            if cell.piece as u8 >= 12 && cell.piece != Piece::Twister {
                let sprite = state.rules().cell_sprite(cell, 0).unwrap();
                assert_drawn(&frame, index, sprite);
                seen.push(cell.piece);
            }
        }
    }
    for piece in [
        Piece::Teleporter,
        Piece::LogicGate,
        Piece::Starburst,
        Piece::MobileTeleporter,
        Piece::DualReflector,
        Piece::QuadBender,
        Piece::OccupiedSquare,
        Piece::PrimaryLogicGate,
    ] {
        assert!(seen.contains(&piece), "{piece:?}");
    }
}
