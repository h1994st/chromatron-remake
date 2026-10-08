//! Software renderer into a 640 × 480 `0x00RRGGBB` frame (platform.md P2–P3, data.md D5–D7), plus
//! the text boxes the shell draws on top.

use spectrograph::{GameData, SPRITE_SIZE};

use crate::board::{BOARD_CELLS, Board, Cell, VISIBLE_TOOLBOX_SLOTS, toolbox_index};
use crate::game_state::{GameState, SlotState};
use crate::layout::{
    FRAME_HEIGHT, FRAME_WIDTH, FREEWARE_RECT, HINT_RECT, LAST_SLOT, MORE_LEVELS_RECT, Rect,
    SITE_URL_RECT, STRIP_SLOTS, WON_RECT, YOU_WIN_RECT, cell_origin, strip_origin,
};
use crate::piece::{Colour, DX, DY, Piece};
use crate::trace::BeamField;

/// The cleared frame, RGB a4a4a4 (`memset(fb, 0xa4, …)`).
pub const BACKGROUND: u32 = 0xa4a4a4;
/// `min_index` values: draw every pixel; only pixels ≥ 16 (cover beams); the drag sprite and digits.
const OPAQUE: u8 = 0;
const OVER_BEAMS: u8 = 0x10;
const ON_TOP: u8 = 0x0e;
const SPRITE: i32 = SPRITE_SIZE as i32;
const HALF: i32 = SPRITE / 2;
/// Digits '0'–'9' are the top-left 12 × 12 of sprites 118–127.
const DIGIT_SPRITE: usize = 118;
const DIGIT_SIZE: i32 = 12;
/// A beam half-segment runs from the cell centre 13 px towards a side, both ends drawn.
const BEAM_REACH: i32 = 13;
pub const MAX_TEXT_BOXES: usize = 5;

/// `cell_image_index` @ 0x45d4: the sprite for a cell, or `None` for colours the original has no
/// sprite for. `received` (what enters the cell) only matters for pinwheels, which light up when
/// satisfied.
pub fn cell_sprite(cell: Cell, received: Colour) -> Option<usize> {
    let d = usize::from(cell.dir);
    let axis = d & 3;
    Some(match cell.piece {
        Piece::Empty => 0,
        Piece::Wall => 1,
        Piece::Laser => match cell.colour {
            4 => 56 + d,
            2 => 72 + d,
            1 => 88 + d,
            7 => 104 + d,
            _ => return None,
        },
        Piece::Mirror => 32 + d,
        Piece::Bender => 48 + d,
        Piece::Filter => match cell.colour {
            1 => 8 + axis,
            4 => 96 + axis,
            2 => 112 + axis,
            _ => return None,
        },
        Piece::Prism => 24 + d,
        Piece::Doppler => 64 + d,
        Piece::Splitter => 40 + axis,
        Piece::Tangler => 80 + d,
        Piece::Pinwheel => {
            let off = [12, 6, 4, 18, 2, 20, 16, 22]
                .get(usize::from(cell.colour))
                .copied()?;
            off + usize::from(cell.colour == received)
        }
        Piece::Conduit => 44 + axis,
        Piece::Teleporter => 14,
    })
}

/// One string for the shell to draw with Lucida Grande 13 pt, black, left-aligned and wrapped to
/// `rect`'s width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextBox<'a> {
    pub text: &'a str,
    pub rect: Rect,
}

/// The frame's text boxes in draw order (fixed capacity, no allocation).
#[derive(Clone, Copy, Debug, Default)]
pub struct TextBoxes<'a> {
    boxes: [TextBox<'a>; MAX_TEXT_BOXES],
    len: usize,
}

impl<'a> TextBoxes<'a> {
    pub fn as_slice(&self) -> &[TextBox<'a>] {
        &self.boxes[..self.len]
    }

    /// Empty strings draw nothing, so they are left out.
    fn push(&mut self, text: &'a str, rect: Rect) {
        if !text.is_empty() {
            self.boxes[self.len] = TextBox { text, rect };
            self.len += 1;
        }
    }
}

/// `paint_window`'s `draw_text_box` calls (platform.md P3), minus the unreachable `mode_text`.
pub fn text_boxes<'a>(state: &GameState<'a>) -> TextBoxes<'a> {
    let ui = &state.data().ui;
    let slot = state.slot();
    let mut boxes = TextBoxes::default();
    if state.is_won() {
        boxes.push(ui.you_win.as_ref(), YOU_WIN_RECT);
    } else if state.is_completed(slot) {
        boxes.push(ui.won.as_ref(), WON_RECT);
    }
    boxes.push(
        if state.is_won() {
            ui.click_next.as_ref()
        } else {
            state.hint()
        },
        HINT_RECT,
    );
    boxes.push(ui.freeware.as_ref(), FREEWARE_RECT);
    if slot > 39 && (slot.is_multiple_of(2) || slot == LAST_SLOT) {
        boxes.push(ui.more_levels.as_ref(), MORE_LEVELS_RECT);
    }
    boxes.push(ui.site_url.as_ref(), SITE_URL_RECT);
    boxes
}

/// Draws the whole scene into `frame` (640 × 480, row-major, `0x00RRGGBB`) and returns the text
/// boxes to draw over it. Allocation-free; uses the beams from the last trace.
pub fn render<'a>(state: &GameState<'a>, frame: &mut [u32]) -> TextBoxes<'a> {
    assert_eq!(
        frame.len(),
        FRAME_WIDTH * FRAME_HEIGHT,
        "frame must be 640 × 480"
    );
    let data = state.data();
    let scene = Scene {
        data,
        board: state.board(),
        field: state.field(),
        hidden: state.drag().map(|d| d.source),
    };
    frame.fill(BACKGROUND);
    for index in 0..BOARD_CELLS {
        scene.draw_cell(frame, index, OPAQUE);
    }
    for index in 0..BOARD_CELLS {
        let (left, top) = cell_origin(index);
        let (inbound, outbound) = (&scene.field.inbound[index], &scene.field.outbound[index]);
        if draw_cell_beams(
            frame,
            &data.beam_colors,
            left + HALF,
            top + HALF,
            inbound,
            outbound,
        ) {
            scene.draw_cell(frame, index, OVER_BEAMS);
        }
    }
    for slot in 0..VISIBLE_TOOLBOX_SLOTS {
        scene.draw_cell(frame, toolbox_index(slot), OPAQUE);
    }
    for slot in 0..STRIP_SLOTS {
        draw_strip_number(frame, data, slot, state.slot_state(slot));
    }
    if let Some(drag) = state.drag()
        && let Some(sprite) = cell_sprite(drag.piece, 0)
    {
        let src = &data.sprites[sprite];
        blit(
            frame,
            &data.palette,
            drag.x - HALF,
            drag.y - HALF,
            src,
            SPRITE,
            SPRITE,
            ON_TOP,
            0,
        );
    }
    text_boxes(state)
}

struct Scene<'s> {
    data: &'s GameData,
    board: &'s Board,
    field: &'s BeamField,
    /// The cell a piece is being dragged out of; it is drawn empty.
    hidden: Option<usize>,
}

impl Scene<'_> {
    /// `draw_slot_cell`: one board cell or toolbox slot.
    fn draw_cell(&self, frame: &mut [u32], index: usize, min: u8) {
        let cell = if self.hidden == Some(index) {
            Cell::default()
        } else {
            self.board.cells[index]
        };
        let received = if index < BOARD_CELLS {
            self.field.received(index)
        } else {
            0
        };
        if let Some(sprite) = cell_sprite(cell, received) {
            let (left, top) = cell_origin(index);
            blit(
                frame,
                &self.data.palette,
                left,
                top,
                &self.data.sprites[sprite],
                SPRITE,
                SPRITE,
                min,
                0,
            );
        }
    }
}

/// `blit_indexed` @ 0x47e0 with a 24-pixel source stride. Pixels below `min` are transparent.
/// Clipping on the left or top shrinks the image without advancing the source, as the original does.
#[allow(clippy::too_many_arguments)]
fn blit(
    frame: &mut [u32],
    palette: &[u32; 256],
    mut left: i32,
    mut top: i32,
    src: &[u8],
    mut width: i32,
    mut height: i32,
    min: u8,
    palette_offset: usize,
) {
    if left < 0 {
        width += left;
        left = 0;
    }
    if top < 0 {
        height += top;
        top = 0;
    }
    width = width.min(FRAME_WIDTH as i32 - left);
    height = height.min(FRAME_HEIGHT as i32 - top);
    if width <= 0 || height <= 0 {
        return;
    }
    for row in 0..height as usize {
        let src_row = &src[row * SPRITE_SIZE..][..width as usize];
        let start = (top as usize + row) * FRAME_WIDTH + left as usize;
        for (dst, &p) in frame[start..start + src_row.len()].iter_mut().zip(src_row) {
            if p >= min {
                *dst = palette[usize::from(p) + palette_offset];
            }
        }
    }
}

/// `draw_cell_beams` @ 0x61ec: for each side `s`, a 14-pixel line from the centre towards `s` in
/// `beam_colors[out[s] | in[s + 4]]`; then a centre dot where straight-through beams
/// (`in[s] == out[s]`) cross on two or more axes. Returns whether anything was drawn.
fn draw_cell_beams(
    frame: &mut [u32],
    beam_colors: &[u32; 8],
    cx: i32,
    cy: i32,
    inbound: &[Colour; 8],
    outbound: &[Colour; 8],
) -> bool {
    let (mut drew, mut axes, mut dot) = (false, 0u8, 0u8);
    for s in 0..8 {
        let colour = outbound[s] | inbound[(s + 4) & 7];
        if colour == 0 {
            continue;
        }
        drew = true;
        let (dx, dy) = (i32::from(DX[s]), i32::from(DY[s]));
        for k in 0..=BEAM_REACH {
            frame[((cy + k * dy) as usize) * FRAME_WIDTH + (cx + k * dx) as usize] =
                beam_colors[usize::from(colour)];
        }
        if inbound[s] == outbound[s] {
            axes |= 1 << (s & 3);
            dot |= inbound[s];
        }
    }
    if dot != 0 && axes.count_ones() >= 2 {
        frame[cy as usize * FRAME_WIDTH + cx as usize] = beam_colors[usize::from(dot)];
    }
    drew
}

/// The strip number for `slot` (1–50, a leading 0 becomes a 4 px space), tinted by its state.
fn draw_strip_number(frame: &mut [u32], data: &GameData, slot: usize, state: SlotState) {
    let offset = match state {
        SlotState::Current => 0x00,
        SlotState::Completed => 0x48,
        SlotState::Unlocked => 0x30,
        SlotState::Locked => 0x10,
    };
    let n = slot + 1;
    let (mut x, y) = strip_origin(slot);
    for digit in [Some(n / 10).filter(|&d| d != 0), Some(n % 10)] {
        match digit {
            Some(d) => {
                let src = &data.sprites[DIGIT_SPRITE + d];
                blit(
                    frame,
                    &data.palette,
                    x,
                    y,
                    src,
                    DIGIT_SIZE,
                    DIGIT_SIZE,
                    ON_TOP,
                    offset,
                );
                x += 8;
            }
            None => x += 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::tiny_game;
    use crate::game_state::InputEvent;
    use crate::layout::{HINT_RECT, strip_origin};
    use crate::piece::{BLUE, GREEN, RED};
    use crate::save::SaveData;

    fn pixel(frame: &[u32], x: i32, y: i32) -> u32 {
        frame[y as usize * FRAME_WIDTH + x as usize]
    }

    fn rendered(state: &GameState) -> Vec<u32> {
        let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
        render(state, &mut frame);
        frame
    }

    /// The fixture's palette: entry `i` is `0x10000 + i`.
    fn pal(i: usize) -> u32 {
        0x10000 + i as u32
    }

    #[test]
    fn sprite_indices_follow_cell_image_index() {
        let cell = |piece, colour, dir| Cell { piece, colour, dir };
        assert_eq!(cell_sprite(cell(Piece::Empty, 4, 3), 0), Some(0));
        assert_eq!(cell_sprite(cell(Piece::Wall, 0, 0), 0), Some(1));
        assert_eq!(cell_sprite(cell(Piece::Laser, RED, 2), 0), Some(58));
        assert_eq!(cell_sprite(cell(Piece::Laser, GREEN, 7), 0), Some(79));
        assert_eq!(cell_sprite(cell(Piece::Laser, BLUE, 0), 0), Some(88));
        assert_eq!(cell_sprite(cell(Piece::Laser, 7, 1), 0), Some(105));
        assert_eq!(cell_sprite(cell(Piece::Laser, 3, 1), 0), None);
        assert_eq!(cell_sprite(cell(Piece::Mirror, 4, 5), 0), Some(37));
        assert_eq!(cell_sprite(cell(Piece::Bender, 4, 7), 0), Some(55));
        assert_eq!(cell_sprite(cell(Piece::Filter, BLUE, 6), 0), Some(10));
        assert_eq!(cell_sprite(cell(Piece::Filter, RED, 1), 0), Some(97));
        assert_eq!(cell_sprite(cell(Piece::Filter, GREEN, 3), 0), Some(115));
        assert_eq!(cell_sprite(cell(Piece::Prism, 0, 2), 0), Some(26));
        assert_eq!(cell_sprite(cell(Piece::Doppler, 0, 2), 0), Some(66));
        assert_eq!(cell_sprite(cell(Piece::Splitter, 0, 5), 0), Some(41));
        assert_eq!(cell_sprite(cell(Piece::Tangler, 0, 4), 0), Some(84));
        assert_eq!(cell_sprite(cell(Piece::Conduit, 0, 6), 0), Some(46));
        assert_eq!(cell_sprite(cell(Piece::Teleporter, 0, 0), 0), Some(14));
    }

    #[test]
    fn pinwheels_light_up_when_satisfied() {
        let wheel = |colour| Cell {
            piece: Piece::Pinwheel,
            colour,
            dir: 0,
        };
        let off: Vec<_> = (0..8)
            .map(|c| cell_sprite(wheel(c), 0xff).unwrap())
            .collect();
        assert_eq!(off, [12, 6, 4, 18, 2, 20, 16, 22]);
        assert_eq!(cell_sprite(wheel(RED), RED), Some(3));
        assert_eq!(
            cell_sprite(wheel(0), 0),
            Some(13),
            "black is lit while dark"
        );
        assert_eq!(cell_sprite(wheel(RED), RED | BLUE), Some(2));
    }

    #[test]
    fn board_toolbox_and_background() {
        let data = tiny_game();
        let state = GameState::new(&data, SaveData::default());
        let frame = rendered(&state);
        assert_eq!(pixel(&frame, 0, 0), BACKGROUND);
        assert_eq!(
            pixel(&frame, 440, 50),
            BACKGROUND,
            "between board and toolbox"
        );
        assert_eq!(pixel(&frame, 48, 18), pal(0), "floor tile");
        assert_eq!(
            pixel(&frame, 96 + 3, 186 + 20),
            pal(58),
            "red laser facing E"
        );
        assert_eq!(
            pixel(&frame, 448, 8),
            pal(32),
            "mirror dir 0 in toolbox slot 0"
        );
        assert_eq!(pixel(&frame, 472, 8), BACKGROUND, "2 px gap between slots");
        assert_eq!(
            pixel(&frame, 474, 8),
            pal(0),
            "toolbox slot 1 is an empty tile"
        );
        assert_eq!(
            pixel(&frame, 578 + 23, 86 + 23),
            pal(0),
            "toolbox slot 23 is drawn"
        );
    }

    #[test]
    fn beams_draw_half_segments_under_sprites_of_16_and_up() {
        let data = tiny_game();
        let state = GameState::new(&data, SaveData::default());
        let frame = rendered(&state);
        let red = data.beam_colors[usize::from(RED)];
        // Cell (3,7) is empty floor: centre (132, 198), beam W and E of it.
        assert_eq!(pixel(&frame, 125, 198), red);
        assert_eq!(pixel(&frame, 145, 198), red);
        assert_eq!(pixel(&frame, 132, 197), pal(0), "1 px wide");
        // The laser's own segment is covered by its sprite (58 ≥ 16) except where cell (3,7)'s
        // W segment reaches 1 px back into it.
        assert_eq!(pixel(&frame, 118, 198), pal(58));
        assert_eq!(pixel(&frame, 119, 198), red);
    }

    #[test]
    fn a_lit_pinwheel_and_the_win_texts() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        state.handle(InputEvent::Paste("1-01-hI".into()));
        let frame = rendered(&state);
        assert_eq!(pixel(&frame, 240, 66), pal(3), "red pinwheel at (8,2), lit");
        let texts: Vec<&str> = text_boxes(&state)
            .as_slice()
            .iter()
            .map(|t| t.text)
            .collect();
        assert_eq!(
            texts,
            ["You win!", "next please", "freeware", "example.com"]
        );
        assert_eq!(text_boxes(&state).as_slice()[0].rect, YOU_WIN_RECT);
    }

    #[test]
    fn a_completed_level_that_is_no_longer_solved_says_won() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        state.handle(InputEvent::Paste("1-01-hI".into()));
        state.handle(InputEvent::Key(b'r'));
        let boxes = text_boxes(&state);
        assert_eq!(
            boxes.as_slice()[0],
            TextBox {
                text: "(won)",
                rect: WON_RECT
            }
        );
        assert_eq!(
            boxes.as_slice()[1],
            TextBox {
                text: "one",
                rect: HINT_RECT
            }
        );
    }

    #[test]
    fn more_levels_shows_on_slots_41_43_45_47_49_and_50() {
        let data = tiny_game();
        let save = SaveData {
            completed: [true; 64],
            ..SaveData::default()
        };
        let mut state = GameState::new(&data, save);
        let mut shown = vec![];
        for slot in 0..STRIP_SLOTS {
            state.goto_slot(slot);
            state.handle(InputEvent::Key(b'r'));
            if text_boxes(&state)
                .as_slice()
                .iter()
                .any(|t| t.text == "more levels @")
            {
                shown.push(slot + 1);
            }
        }
        assert_eq!(shown, [41, 43, 45, 47, 49, 50]);
    }

    #[test]
    fn strip_numbers_are_tinted_by_state() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        state.handle(InputEvent::Key(b'L'));
        state.handle(InputEvent::Key(b' '));
        let frame = rendered(&state);
        let (x0, y0) = strip_origin(0);
        assert_eq!(
            pixel(&frame, x0, y0),
            BACKGROUND,
            "a one-digit number starts with a 4 px space"
        );
        assert_eq!(
            pixel(&frame, x0 + 4, y0),
            pal(119 + 0x48),
            "slot 1 completed: '1' tinted blue"
        );
        let (x1, y1) = strip_origin(1);
        assert_eq!(pixel(&frame, x1 + 4, y1), pal(120), "slot 2 current: grey");
        let (x2, y2) = strip_origin(2);
        assert_eq!(
            pixel(&frame, x2 + 4, y2),
            pal(121 + 0x10),
            "slot 3 locked: red"
        );
        assert_eq!(
            pixel(&frame, x2 + 4 + 11, y2 + 11),
            pal(121 + 0x10),
            "12 × 12 glyph"
        );
        assert_eq!(pixel(&frame, x2 + 4 + 12, y2), BACKGROUND);
        let (x9, y9) = strip_origin(9);
        assert_eq!(pixel(&frame, x9, y9), pal(119 + 0x10), "'10': the 1");
        assert_eq!(
            pixel(&frame, x9 + 8, y9),
            pal(118 + 0x10),
            "'10': the 0, 8 px on"
        );
    }

    #[test]
    fn a_dragged_piece_leaves_an_empty_cell_and_follows_the_pointer() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        state.handle(InputEvent::MouseDown {
            x: 460,
            y: 20,
            alt: false,
        });
        state.handle(InputEvent::MouseDragged { x: 200, y: 300 });
        let frame = rendered(&state);
        assert_eq!(
            pixel(&frame, 448, 8),
            pal(0),
            "the source slot is drawn empty"
        );
        assert_eq!(
            pixel(&frame, 188, 288),
            pal(32),
            "the sprite is centred on the pointer"
        );
        assert_eq!(pixel(&frame, 211, 311), pal(32));
        assert_eq!(pixel(&frame, 212, 300), pal(0));
    }

    #[test]
    fn clipping_at_the_left_edge_does_not_advance_the_source() {
        let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
        let mut palette = [0u32; 256];
        for (i, p) in palette.iter_mut().enumerate() {
            *p = i as u32;
        }
        let src: Vec<u8> = (0..24 * 24).map(|i| (i % 24) as u8 + 100).collect();
        blit(&mut frame, &palette, -5, -3, &src, 24, 24, 0, 0);
        assert_eq!(frame[0], 100, "column 0 of the source lands on x = 0");
        assert_eq!(frame[18], 118);
        assert_eq!(frame[19], 0, "only 19 columns are drawn");
        assert_eq!(frame[20 * FRAME_WIDTH], 100, "21 rows are drawn");
        assert_eq!(frame[21 * FRAME_WIDTH], 0);
        blit(&mut frame, &palette, 630, 470, &src, 24, 24, 0, 0);
        assert_eq!(
            frame[479 * FRAME_WIDTH + 639],
            109,
            "right and bottom clip normally"
        );
    }

    #[test]
    fn a_drag_sprite_off_the_frame_is_clipped_away() {
        let data = tiny_game();
        let mut state = GameState::new(&data, SaveData::default());
        state.handle(InputEvent::MouseDown {
            x: 460,
            y: 20,
            alt: false,
        });
        state.handle(InputEvent::MouseDragged { x: 200, y: 300 });
        for (x, y) in [
            (700, 470),
            (664, 479),
            (-40, 100),
            (300, -40),
            (-500, 900),
            (9999, 9999),
        ] {
            state.handle(InputEvent::MouseDragged { x, y });
            let frame = rendered(&state);
            assert_eq!(pixel(&frame, 639, 479), BACKGROUND, "({x}, {y})");
        }
        state.handle(InputEvent::MouseDragged { x: 645, y: 100 });
        assert_eq!(
            pixel(&rendered(&state), 639, 100),
            pal(32),
            "partly visible on the right"
        );
    }

    #[test]
    fn crossing_beams_get_a_centre_dot_in_their_combined_colour() {
        let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
        let colors = [0, 1, 2, 3, 4, 5, 6, 7];
        // Red passes straight through E→E, blue straight through S→S.
        let mut inbound = [0; 8];
        let mut outbound = [0; 8];
        inbound[2] = RED;
        outbound[2] = RED;
        inbound[4] = BLUE;
        outbound[4] = BLUE;
        assert!(draw_cell_beams(
            &mut frame, &colors, 100, 100, &inbound, &outbound
        ));
        assert_eq!(frame[100 * FRAME_WIDTH + 100], u32::from(RED | BLUE));
        assert_eq!(
            frame[100 * FRAME_WIDTH + 87],
            u32::from(RED),
            "W half: in[E]"
        );
        assert_eq!(
            frame[100 * FRAME_WIDTH + 113],
            u32::from(RED),
            "E half: out[E]"
        );
        assert_eq!(
            frame[87 * FRAME_WIDTH + 100],
            u32::from(BLUE),
            "N half: in[S]"
        );
        assert_eq!(frame[86 * FRAME_WIDTH + 100], 0, "14 pixels per half");
    }

    #[test]
    fn a_single_straight_beam_or_a_turn_gets_no_dot() {
        let mut frame = vec![0; FRAME_WIDTH * FRAME_HEIGHT];
        let colors = [0, 1, 2, 3, 4, 5, 6, 7];
        let mut inbound = [0; 8];
        let mut outbound = [0; 8];
        inbound[2] = RED;
        outbound[0] = RED; // a mirror turns E into N
        draw_cell_beams(&mut frame, &colors, 100, 100, &inbound, &outbound);
        // Side N (s=0): in[0] = 0 ≠ out[0]; side W (s=6): in[6] = out[6] = 0, counted with no colour.
        assert_eq!(
            frame[100 * FRAME_WIDTH + 100],
            u32::from(RED),
            "last segment drawn, no dot"
        );
        assert!(!draw_cell_beams(
            &mut frame, &colors, 200, 200, &[0; 8], &[0; 8]
        ));
    }

    #[test]
    fn rendering_does_not_depend_on_the_previous_frame() {
        let data = tiny_game();
        let state = GameState::new(&data, SaveData::default());
        let mut frame = vec![0x123456; FRAME_WIDTH * FRAME_HEIGHT];
        render(&state, &mut frame);
        assert_eq!(frame, rendered(&state));
    }

    #[test]
    #[should_panic(expected = "640 × 480")]
    fn the_frame_must_be_full_size() {
        let data = tiny_game();
        let state = GameState::new(&data, SaveData::default());
        render(&state, &mut [0; 10]);
    }
}
