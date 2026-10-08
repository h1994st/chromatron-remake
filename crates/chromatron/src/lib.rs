//! The Chromatron macOS app: window, menus, input, text and saves around `lumen`.

pub mod blit;
pub mod c1;
pub mod damage;
pub mod frame_timing;
pub mod menus;
pub mod pasteboard;
pub mod pointer_keys;
pub mod progress_file;
pub mod snapshot;
pub mod typeset;
pub mod viewport;
pub mod wheel;
pub mod wheel_pref;

mod game_window;

pub use game_window::run;

use spectrograph::GameData;

/// What differs between the games' apps. Chromatron 2–4 each add one, plus a bin and an
/// `EMBEDDED` entry in build.rs.
pub struct AppSpec {
    pub data: &'static GameData,
    /// The bundle id; also the preferences domain.
    pub bundle_id: &'static str,
    /// The folder in `~/Library/Application Support/` that holds the save file.
    pub support_dir: &'static str,
}

pub static CHROMATRON_1: AppSpec = AppSpec {
    data: &c1::GAME,
    bundle_id: "me.shengtuo.chromatron1",
    support_dir: "Chromatron 1",
};
