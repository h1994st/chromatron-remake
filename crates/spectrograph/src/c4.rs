//! Chromatron 4 v1.0: where its data lives (re/notes/chromatron4/data.md D1–D8, platform.md P1
//! and P3).

use crate::data_map::DataMap;

pub(crate) const MAP: DataMap = DataMap {
    level_table: 0xbb6c,
    level_name_table: Some(0xbc48),
    hint_table: 0xbd24,
    level_ids: 54,
    max_kind: 22,
    level_order: 0x8110,
    image_table: 0x14e94,
    num_images: 0x15414,
    palette: 0x15114,
    beam_colors: 0x80d4,
    site_url_xor: 0x80f4,
    window_title: 0x6eb8,
    you_win: 0x6f54,
    won: 0x6f60,
    click_next: 0x6f68,
    freeware: None,
    more_levels: None,
    thank_you: Some(0x6f98),
};
