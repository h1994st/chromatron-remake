//! Rendering real Chromatron 3 levels.

mod c3_game;

use c3_game::C3;
use lumen::{
    Board, FRAME_HEIGHT, FRAME_WIDTH, GameState, InputEvent, Piece, Rect, SaveData, board_index,
    encode_solution, render,
};
use spectrograph::SPRITE_SIZE;

#[test]
fn the_game_opens_on_easy_does_it_with_25_levels_open() {
    let state = GameState::new(&C3, SaveData::default());
    let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    let texts = render(&state, &mut frame);
    let texts: Vec<(&str, Rect)> = texts.as_slice().iter().map(|t| (t.text, t.rect)).collect();
    assert_eq!(state.level_id(), 11);
    assert_eq!(
        texts,
        [
            ("Easy Does It", Rect::new(50, 385, 300, 420)),
            ("silverspaceship.com", Rect::new(470, 450, 640, 480))
        ],
        "no hint on slot 1"
    );
    let open: Vec<usize> = (0..50).filter(|&s| state.is_unlocked(s)).collect();
    assert_eq!(open, (0..25).collect::<Vec<_>>());
}

#[test]
fn every_level_renders_at_the_start_and_solved() {
    let mut state = GameState::new(&C3, SaveData::default());
    let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    for slot in 0..C3.level_order.len() {
        state.goto_slot(slot);
        state.handle(InputEvent::Key(b'r'));
        render(&state, &mut frame);
        let level = C3.slot_level(slot).unwrap();
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
fn assert_drawn(frame: &[u32], (x, y): (usize, usize), sprite: usize) {
    let (left, top) = (48 + 24 * x, 18 + 24 * y);
    let pixels = &C3.sprites[sprite];
    for row in 2..SPRITE_SIZE - 1 {
        for col in 2..SPRITE_SIZE - 1 {
            let index = pixels[row * SPRITE_SIZE + col];
            if index >= 16 {
                assert_eq!(
                    frame[(top + row) * FRAME_WIDTH + left + col],
                    C3.palette[usize::from(index)],
                    "sprite {sprite} at ({x},{y}), pixel ({col},{row})"
                );
            }
        }
    }
}

#[test]
fn the_dual_reflector_and_quad_bender_are_drawn_with_their_own_sprites() {
    let mut state = GameState::new(&C3, SaveData::default());
    let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    // Slots 17 (dual-reflector) and 22 (quad-bender), solved.
    for (slot, piece, base) in [
        (16, Piece::DualReflector, 136),
        (21, Piece::QuadBender, 140),
    ] {
        state.goto_slot(slot);
        let board = Board::from_level(C3.slot_level(slot).unwrap());
        state.handle(InputEvent::Paste(encode_solution(&board, slot).unwrap()));
        render(&state, &mut frame);
        let cell = state.board().cells[board_index(7, 7)];
        assert_eq!((cell.piece, cell.dir), (piece, 1), "slot {}", slot + 1);
        let sprite = state.rules().cell_sprite(cell, 0).unwrap();
        assert_eq!(sprite, base + 1);
        assert_drawn(&frame, (7, 7), sprite);
    }
}
