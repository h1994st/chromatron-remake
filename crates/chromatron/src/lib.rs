//! The Chromatron macOS app: window, menus, input, text and saves around `lumen`.

pub mod blit;
pub mod c1;
pub mod c2;
pub mod c3;
pub mod c4;
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

/// What differs between the games' apps, besides what `lumen::RuleSet::of(data.game)` says.
/// Each game adds one, plus a bin and an `EMBEDDED` entry in build.rs.
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

pub static CHROMATRON_2: AppSpec = AppSpec {
    data: &c2::GAME,
    bundle_id: "me.shengtuo.chromatron2",
    support_dir: "Chromatron 2",
};

pub static CHROMATRON_3: AppSpec = AppSpec {
    data: &c3::GAME,
    bundle_id: "me.shengtuo.chromatron3",
    support_dir: "Chromatron 3",
};

pub static CHROMATRON_4: AppSpec = AppSpec {
    data: &c4::GAME,
    bundle_id: "me.shengtuo.chromatron4",
    support_dir: "Chromatron 4",
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress_file::ProgressFile;
    use lumen::RuleSet;
    use spectrograph::Game;

    #[test]
    fn each_app_runs_its_own_game() {
        for (spec, game, title) in [
            (&CHROMATRON_1, Game::C1, "Chromatron 1 (v1.15)"),
            (&CHROMATRON_2, Game::C2, "Chromatron 2 (v1.4)"),
            (&CHROMATRON_3, Game::C3, "Chromatron 3 (v1.1)"),
            (&CHROMATRON_4, Game::C4, "Chromatron 4 (v1.0)"),
        ] {
            assert_eq!(spec.data.game, game);
            assert_eq!(spec.data.ui.window_title, title);
            assert_eq!(
                spec.bundle_id,
                format!("me.shengtuo.chromatron{}", game.number())
            );
            assert_eq!(spec.support_dir, format!("Chromatron {}", game.number()));
        }
    }

    #[test]
    fn chromatron_2_saves_where_the_spec_says() {
        let rules = RuleSet::of(CHROMATRON_2.data.game);
        let file = ProgressFile::for_app(CHROMATRON_2.support_dir, rules)
            .expect("tests run with a home directory");
        assert!(file.path().ends_with(
            "Library/Application Support/Chromatron 2/com.silverspaceship.chromatron2.dat"
        ));
    }

    #[test]
    fn chromatron_3_and_4_save_where_the_spec_says() {
        for (spec, path) in [
            (
                &CHROMATRON_3,
                "Library/Application Support/Chromatron 3/com.silverspaceship.chromatron3.dat",
            ),
            (
                &CHROMATRON_4,
                "Library/Application Support/Chromatron 4/com.silverspaceship.chromatron4.dat",
            ),
        ] {
            let file = ProgressFile::for_app(spec.support_dir, RuleSet::of(spec.data.game))
                .expect("tests run with a home directory");
            assert!(file.path().ends_with(path), "{path}");
        }
    }
}
