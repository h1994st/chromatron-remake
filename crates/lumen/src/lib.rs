//! Chromatron engine: board model, beam tracer, rules, input and software renderer. Pure: no
//! platform dependencies and no I/O.

pub mod board;
pub mod bsd_qsort;
pub mod diagram;
#[cfg(test)]
mod fixture;
pub mod game_state;
pub mod layout;
pub mod piece;
mod pointer;
pub mod progress;
pub mod render;
pub mod rule_set;
pub mod save;
pub mod solution;
pub mod toolbox;
pub mod trace;
pub mod transfer;
pub mod win;

pub use board::{
    BOARD_CELLS, Board, CELLS, Cell, SIDE, TOOLBOX_SLOTS, VISIBLE_TOOLBOX_SLOTS, board_index,
    toolbox_index,
};
pub use bsd_qsort::bsd_qsort;
pub use diagram::beam_diagram;
pub use game_state::{Drag, Effects, GameState, InputEvent, Redraw, SlotState};
pub use layout::{FRAME_HEIGHT, FRAME_WIDTH, Hit, Rect, hit_test};
pub use piece::{BLUE, Colour, DX, DY, Dir, GREEN, Piece, RED, WHITE, turn};
pub use progress::{
    PROGRESS_SLOTS, update_unlocked, update_unlocked_by_count, update_unlocked_fixed,
};
pub use render::{BACKGROUND, TextBox, TextBoxes, cell_sprite, render, text_boxes};
pub use rule_set::{Passes, RuleSet, Splitter, TextBoxSet, Unlock};
pub use save::{SaveData, SaveError, decode_save, encode_save};
pub use solution::{apply_solution, encode_solution};
pub use toolbox::move_pieces_to_toolbox;
pub use trace::{BeamField, MAX_BEAMS, MAX_GENERATIONS, trace};
pub use transfer::{Emission, Emissions, doppler_backward, doppler_forward, transfer};
pub use win::is_won;
