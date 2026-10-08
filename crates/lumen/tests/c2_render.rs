//! Rendering real Chromatron 2 levels.

mod c2_game;

use c2_game::C2;
use lumen::{
    Board, FRAME_HEIGHT, FRAME_WIDTH, GameState, InputEvent, Piece, Rect, SaveData, board_index,
    cell_sprite, encode_solution, render,
};
use spectrograph::SPRITE_SIZE;

#[test]
fn the_game_opens_on_line_of_sight_with_its_name_and_hint() {
    let state = GameState::new(&C2, SaveData::default());
    let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    let texts = render(&state, &mut frame);
    let texts = texts.as_slice();
    assert_eq!(state.level_id(), 13);
    assert_eq!(
        (texts[0].text, texts[0].rect),
        ("Line of Sight", Rect::new(50, 385, 300, 420))
    );
    assert!(texts[1].text.starts_with("An easy warm up puzzle"));
}

#[test]
fn every_level_renders_at_the_start_and_solved() {
    let mut state = GameState::new(&C2, SaveData::default());
    let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    for slot in 0..C2.level_order.len() {
        state.goto_slot(slot);
        state.handle(InputEvent::Key(b'r'));
        render(&state, &mut frame);
        let level = C2.slot_level(slot).unwrap();
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
    let pixels = &C2.sprites[sprite];
    for row in 2..SPRITE_SIZE - 1 {
        for col in 2..SPRITE_SIZE - 1 {
            let index = pixels[row * SPRITE_SIZE + col];
            if index >= 16 {
                assert_eq!(
                    frame[(top + row) * FRAME_WIDTH + left + col],
                    C2.palette[usize::from(index)],
                    "sprite {sprite} at ({x},{y}), pixel ({col},{row})"
                );
            }
        }
    }
}

#[test]
fn the_new_pieces_are_drawn_with_their_own_sprites() {
    let mut state = GameState::new(&C2, SaveData::default());
    let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    // Slots 7 (multi-filter), 10 (twisters) and 18 (complementors), solved.
    for slot in [6, 9, 17] {
        state.goto_slot(slot);
        let board = Board::from_level(C2.slot_level(slot).unwrap());
        state.handle(InputEvent::Paste(encode_solution(&board, slot).unwrap()));
        render(&state, &mut frame);
        let mut seen = 0;
        for y in 0..15 {
            for x in 0..15 {
                let cell = state.board().cells[board_index(x, y)];
                if matches!(
                    cell.piece,
                    Piece::MultiFilter | Piece::Twister | Piece::Complementor
                ) {
                    assert_drawn(&frame, (x, y), cell_sprite(cell, 0).unwrap());
                    seen += 1;
                }
            }
        }
        assert!(seen > 0, "slot {}", slot + 1);
    }
}
