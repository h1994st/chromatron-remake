//! Chromatron 3 v1.1: where its data lives (re/notes/chromatron3/data.md D1–D8, platform.md P1
//! and P3).

use crate::data_map::DataMap;

pub(crate) const MAP: DataMap = DataMap {
    level_table: 0xd590,
    level_name_table: Some(0xd6cc),
    hint_table: 0xd808,
    level_ids: 78,
    max_kind: 20,
    level_order: 0x8110,
    image_table: 0x15e60,
    num_images: 0x163e0,
    palette: 0x160e0,
    beam_colors: 0x80d4,
    site_url_xor: 0x80f4,
    window_title: 0x7974,
    you_win: 0x7a10,
    won: 0x7a1c,
    click_next: 0x7a24,
    freeware: None,
    more_levels: None,
    thank_you: Some(0x7a54),
};
