//! Chromatron 2 v1.4: where its data lives (re/notes/chromatron2/data.md D1–D8, platform.md P1
//! and P3).

use crate::data_map::DataMap;

pub(crate) const MAP: DataMap = DataMap {
    level_table: 0xa934,
    level_name_table: Some(0xaa08),
    hint_table: 0xaadc,
    level_ids: 52,
    max_kind: 15,
    level_order: 0x8110,
    image_table: 0x11ce4,
    num_images: 0x121e4,
    palette: 0x11ee4,
    beam_colors: 0x80d4,
    site_url_xor: 0x80f4,
    window_title: 0x6f88,
    you_win: 0x7024,
    won: 0x7030,
    click_next: 0x7038,
    freeware: None,
    more_levels: None,
    thank_you: Some(0x7068),
};
