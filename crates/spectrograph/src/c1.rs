//! Chromatron 1 v1.15: where its data lives (re/notes/chromatron1/data.md D1–D8).

use crate::data_map::DataMap;

pub(crate) const MAP: DataMap = DataMap {
    level_table: 0xb604,
    level_name_table: None,
    hint_table: 0xb700,
    level_ids: 62,
    max_kind: 12,
    level_order: 0x8110,
    image_table: 0x126dc,
    num_images: 0x12bdc,
    palette: 0x128dc,
    beam_colors: 0x80d4,
    site_url_xor: 0x80f4,
    window_title: 0x6c40,
    you_win: 0x6ce0,
    won: 0x6cec,
    click_next: 0x6cf4,
    freeware: Some(0x6d24),
    more_levels: Some(0x6d30),
    thank_you: None,
};
