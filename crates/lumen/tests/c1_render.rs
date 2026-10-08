//! Rendering real Chromatron 1 levels.

mod common;

use common::C1;
use lumen::{
    BACKGROUND, FRAME_HEIGHT, FRAME_WIDTH, GameState, InputEvent, SaveData, encode_solution, render,
};

fn pixel(frame: &[u32], x: usize, y: usize) -> u32 {
    frame[y * FRAME_WIDTH + x]
}

#[test]
fn level_1_starts_with_a_red_beam_across_the_floor() {
    let state = GameState::new(&C1, SaveData::default());
    let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    let texts = render(&state, &mut frame);
    assert_eq!(pixel(&frame, 0, 0), BACKGROUND);
    assert_eq!(pixel(&frame, 48, 18), 0x58442c, "grid line, palette 6");
    assert_eq!(pixel(&frame, 49, 19), 0x5c5c5c, "floor, palette 8");
    assert_eq!(
        pixel(&frame, 125, 198),
        0xff0000,
        "the red beam E of the laser at (2,7)"
    );
    assert!(texts.as_slice()[0].text.starts_with("Drag the REFLECTOR"));
}

#[test]
fn every_level_renders_at_the_start_and_solved() {
    let mut state = GameState::new(&C1, SaveData::default());
    let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
    for slot in 0..C1.level_order.len() {
        state.goto_slot(slot);
        state.handle(InputEvent::Key(b'r'));
        render(&state, &mut frame);
        let level = C1.slot_level(slot).unwrap();
        let text = encode_solution(&lumen::Board::from_level(level), slot).unwrap();
        state.handle(InputEvent::Paste(text));
        let texts = render(&state, &mut frame);
        assert_eq!(
            state.is_won(),
            level.id != common::MISPLACED_LEVEL,
            "slot {}",
            slot + 1
        );
        assert_eq!(
            texts.as_slice()[0].text == "You win!",
            state.is_won(),
            "slot {}",
            slot + 1
        );
    }
}
