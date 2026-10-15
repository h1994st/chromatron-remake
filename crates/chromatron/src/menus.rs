//! The menu bar: the original nib's menus (platform.md P5), the standard application menu, and
//! the View menu for the modern touches (fullscreen, the look, scale) and File ▸ Levels….

use muda::accelerator::Accelerator;
use muda::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

use crate::viewport::{MAX_SCALE, MIN_SCALE};

/// What a menu item asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    /// File ▸ Reset Level (`rvrt` → key `r`).
    ResetLevel,
    /// File ▸ Next Level (`Next` → key `=`).
    NextLevel,
    /// File ▸ Previous Level (`Prev` → key `-`).
    PreviousLevel,
    /// File ▸ Mouse Wheel Changes Level (`Whee`).
    ToggleWheel,
    /// File ▸ Close: closing the window quits, as in the original.
    Close,
    /// Edit ▸ Copy Level (`copy`).
    CopyLevel,
    /// Edit ▸ Paste Level (`past`).
    PasteLevel,
    /// View ▸ Enter/Exit Full Screen.
    ToggleFullscreen,
    /// View ▸ Scale N×.
    Scale(u32),
    /// View ▸ Classic Look (spec §13).
    ToggleClassicLook,
    /// File ▸ Levels…: the modern level sheet (spec §13).
    Levels,
}

impl MenuCommand {
    /// The muda id of the item that sends this command.
    pub fn id(self) -> String {
        match self {
            MenuCommand::ResetLevel => "reset-level".into(),
            MenuCommand::NextLevel => "next-level".into(),
            MenuCommand::PreviousLevel => "previous-level".into(),
            MenuCommand::ToggleWheel => "wheel-changes-level".into(),
            MenuCommand::Close => "close".into(),
            MenuCommand::CopyLevel => "copy-level".into(),
            MenuCommand::PasteLevel => "paste-level".into(),
            MenuCommand::ToggleFullscreen => "toggle-fullscreen".into(),
            MenuCommand::Scale(n) => format!("scale-{n}"),
            MenuCommand::ToggleClassicLook => "classic-look".into(),
            MenuCommand::Levels => "levels".into(),
        }
    }

    /// The command for a muda id; `None` for ids this app didn't create.
    pub fn from_id(id: &str) -> Option<MenuCommand> {
        Some(match id {
            "reset-level" => MenuCommand::ResetLevel,
            "next-level" => MenuCommand::NextLevel,
            "previous-level" => MenuCommand::PreviousLevel,
            "wheel-changes-level" => MenuCommand::ToggleWheel,
            "close" => MenuCommand::Close,
            "copy-level" => MenuCommand::CopyLevel,
            "paste-level" => MenuCommand::PasteLevel,
            "toggle-fullscreen" => MenuCommand::ToggleFullscreen,
            "classic-look" => MenuCommand::ToggleClassicLook,
            "levels" => MenuCommand::Levels,
            _ => {
                let n: u32 = id.strip_prefix("scale-")?.parse().ok()?;
                if !(MIN_SCALE..=MAX_SCALE).contains(&n) {
                    return None;
                }
                MenuCommand::Scale(n)
            }
        })
    }
}

/// Key equivalents, in muda's syntax ("Cmd" is ⌘).
pub const RESET_KEY: &str = "Cmd+R";
pub const NEXT_KEY: &str = "Cmd+=";
pub const PREVIOUS_KEY: &str = "Cmd+-";
pub const CLOSE_KEY: &str = "Cmd+W";
pub const UNDO_KEY: &str = "Cmd+Z";
pub const REDO_KEY: &str = "Cmd+Shift+Z";
pub const CUT_KEY: &str = "Cmd+X";
pub const COPY_KEY: &str = "Cmd+C";
pub const PASTE_KEY: &str = "Cmd+V";
pub const SELECT_ALL_KEY: &str = "Cmd+A";
pub const FULLSCREEN_KEY: &str = "Ctrl+Cmd+F";
pub const LOOK_KEY: &str = "Alt+Cmd+L";
pub const LEVELS_KEY: &str = "Cmd+L";

fn key(text: &str) -> Option<Accelerator> {
    Some(
        text.parse()
            .unwrap_or_else(|e| panic!("bad accelerator {text:?}: {e}")),
    )
}

fn item(command: MenuCommand, title: &str, shortcut: Option<&str>) -> MenuItem {
    MenuItem::with_id(command.id(), title, true, shortcut.and_then(key))
}

/// A greyed-out item, as the nib's Undo, Redo, Cut, Delete and Select All are.
fn disabled(title: &str, shortcut: Option<&str>) -> MenuItem {
    MenuItem::new(title, false, shortcut.and_then(key))
}

pub const ENTER_FULLSCREEN: &str = "Enter Full Screen";
pub const EXIT_FULLSCREEN: &str = "Exit Full Screen";

/// What the menus show: the window's fullscreen state and classic scale, the look, and whether
/// Levels… can open the modern sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuState {
    pub fullscreen: bool,
    /// An exit from fullscreen is animating, by the menu, the green button or the system
    /// shortcut (winit may already report no fullscreen; AppKit still owns the frame).
    pub leaving: bool,
    pub scale: usize,
    pub classic: bool,
    pub levels: bool,
}

/// The menu state to show, if it differs from what is shown.
pub fn menu_update(shown: Option<MenuState>, now: MenuState) -> Option<MenuState> {
    (shown != Some(now)).then_some(now)
}

/// View ▸ Scale applies to the classic window only, and not in fullscreen or on the way out of
/// it (spec §13).
pub fn scales_enabled(state: &MenuState) -> bool {
    state.classic && !state.fullscreen && !state.leaving
}

/// The installed menu bar, kept alive for the app's lifetime, and the items whose state changes.
pub struct MenuBar {
    _menu: Menu,
    wheel: CheckMenuItem,
    fullscreen: MenuItem,
    classic: CheckMenuItem,
    levels: MenuItem,
    scales: Vec<CheckMenuItem>,
}

impl MenuBar {
    /// Builds the menu bar and makes it the app's main menu. Main thread only, after the event
    /// loop exists. macOS titles the application menu with the bundle name itself.
    /// `classic` is the initial Classic Look check. Levels… starts disabled, as `MenuState.levels`
    /// (modern and a sheet picker) is not known yet; the first `show` sets it.
    pub fn install(wheel_on: bool, classic: bool) -> MenuBar {
        let classic_item = CheckMenuItem::with_id(
            MenuCommand::ToggleClassicLook.id(),
            "Classic Look",
            true,
            classic,
            key(LOOK_KEY),
        );
        let levels = MenuItem::with_id(MenuCommand::Levels.id(), "Levels…", false, key(LEVELS_KEY));
        let wheel = CheckMenuItem::with_id(
            MenuCommand::ToggleWheel.id(),
            "Mouse Wheel Changes Level",
            true,
            wheel_on,
            None,
        );
        let fullscreen = item(
            MenuCommand::ToggleFullscreen,
            ENTER_FULLSCREEN,
            Some(FULLSCREEN_KEY),
        );
        let scales: Vec<CheckMenuItem> = (MIN_SCALE..=MAX_SCALE)
            .map(|n| {
                CheckMenuItem::with_id(
                    MenuCommand::Scale(n).id(),
                    format!("Scale {n}×"),
                    true,
                    false,
                    None,
                )
            })
            .collect();

        let app_menu = Submenu::with_items(
            "Chromatron",
            true,
            &[
                &PredefinedMenuItem::about(None, None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(None),
            ],
        )
        .expect("muda builds the application menu");
        let file_menu = Submenu::with_items(
            "File",
            true,
            &[
                &item(MenuCommand::ResetLevel, "Reset Level", Some(RESET_KEY)),
                &PredefinedMenuItem::separator(),
                &item(MenuCommand::NextLevel, "Next Level", Some(NEXT_KEY)),
                &item(
                    MenuCommand::PreviousLevel,
                    "Previous Level",
                    Some(PREVIOUS_KEY),
                ),
                &levels,
                &PredefinedMenuItem::separator(),
                &wheel,
                &PredefinedMenuItem::separator(),
                &item(MenuCommand::Close, "Close", Some(CLOSE_KEY)),
            ],
        )
        .expect("muda builds the File menu");
        let edit_menu = Submenu::with_items(
            "Edit",
            true,
            &[
                &disabled("Undo", Some(UNDO_KEY)),
                &disabled("Redo", Some(REDO_KEY)),
                &PredefinedMenuItem::separator(),
                &disabled("Cut", Some(CUT_KEY)),
                &item(MenuCommand::CopyLevel, "Copy Level", Some(COPY_KEY)),
                &item(MenuCommand::PasteLevel, "Paste Level", Some(PASTE_KEY)),
                &disabled("Delete", None),
                &disabled("Select All", Some(SELECT_ALL_KEY)),
            ],
        )
        .expect("muda builds the Edit menu");
        let view_menu = Submenu::with_items(
            "View",
            true,
            &[
                &fullscreen,
                &PredefinedMenuItem::separator(),
                &classic_item,
                &PredefinedMenuItem::separator(),
            ],
        )
        .expect("muda builds the View menu");
        for scale in &scales {
            view_menu.append(scale).expect("muda appends a scale item");
        }
        let window_menu = Submenu::with_items(
            "Window",
            true,
            &[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::bring_all_to_front(None),
            ],
        )
        .expect("muda builds the Window menu");

        let menu = Menu::with_items(&[&app_menu, &file_menu, &edit_menu, &view_menu, &window_menu])
            .expect("muda builds the menu bar");
        menu.init_for_nsapp();
        window_menu.set_as_windows_menu_for_nsapp();
        MenuBar {
            _menu: menu,
            wheel,
            fullscreen,
            classic: classic_item,
            levels,
            scales,
        }
    }

    pub fn set_wheel_checked(&self, on: bool) {
        self.wheel.set_checked(on);
    }

    /// Shows the window's and the look's state: the fullscreen item's title, the Classic Look
    /// check, Levels… enabled only for a modern sheet, and a check on the current scale where
    /// scales apply.
    pub fn show(&self, state: &MenuState) {
        self.fullscreen.set_text(if state.fullscreen {
            EXIT_FULLSCREEN
        } else {
            ENTER_FULLSCREEN
        });
        self.classic.set_checked(state.classic);
        self.levels.set_enabled(state.levels);
        let enabled = scales_enabled(state);
        for (item, n) in self.scales.iter().zip(MIN_SCALE..) {
            item.set_checked(enabled && n as usize == state.scale);
            item.set_enabled(enabled);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_round_trips_through_its_id() {
        let mut all = vec![
            MenuCommand::ResetLevel,
            MenuCommand::NextLevel,
            MenuCommand::PreviousLevel,
            MenuCommand::ToggleWheel,
            MenuCommand::Close,
            MenuCommand::CopyLevel,
            MenuCommand::PasteLevel,
            MenuCommand::ToggleFullscreen,
            MenuCommand::ToggleClassicLook,
            MenuCommand::Levels,
        ];
        all.extend((MIN_SCALE..=MAX_SCALE).map(MenuCommand::Scale));
        for command in all {
            assert_eq!(MenuCommand::from_id(&command.id()), Some(command));
        }
    }

    #[test]
    fn foreign_ids_are_ignored() {
        for id in ["", "scale-0", "scale-5", "scale-x", "quit", "42"] {
            assert_eq!(MenuCommand::from_id(id), None, "{id}");
        }
    }

    #[test]
    fn every_shortcut_parses() {
        for text in [
            RESET_KEY,
            NEXT_KEY,
            PREVIOUS_KEY,
            CLOSE_KEY,
            UNDO_KEY,
            REDO_KEY,
            CUT_KEY,
            COPY_KEY,
            PASTE_KEY,
            SELECT_ALL_KEY,
            FULLSCREEN_KEY,
            LOOK_KEY,
            LEVELS_KEY,
        ] {
            assert!(text.parse::<Accelerator>().is_ok(), "{text}");
        }
    }

    #[test]
    fn the_menus_are_updated_only_when_their_state_changed() {
        let state = |fullscreen, scale, classic, levels| MenuState {
            fullscreen,
            leaving: false,
            scale,
            classic,
            levels,
        };
        assert_eq!(
            menu_update(None, state(false, 1, true, false)),
            Some(state(false, 1, true, false))
        );
        assert_eq!(
            menu_update(
                Some(state(true, 2, true, false)),
                state(true, 2, true, false)
            ),
            None
        );
        assert_eq!(
            menu_update(
                Some(state(true, 2, true, false)),
                state(false, 2, true, false)
            ),
            Some(state(false, 2, true, false))
        );
        assert_eq!(
            menu_update(
                Some(state(false, 1, true, false)),
                state(false, 1, false, true)
            ),
            Some(state(false, 1, false, true))
        );
    }

    #[test]
    fn forgetting_the_shown_state_shows_an_unchanged_state_again() {
        // muda flips a check item on click; a command that changes nothing resets `shown`.
        let now = MenuState {
            fullscreen: false,
            leaving: false,
            scale: 2,
            classic: true,
            levels: false,
        };
        assert_eq!(menu_update(Some(now), now), None);
        assert_eq!(menu_update(None, now), Some(now));
        assert!(menu_update(None, now).unwrap().classic);
    }

    #[test]
    fn scales_are_classic_and_windowed_only() {
        let state = |fullscreen, leaving, classic| MenuState {
            fullscreen,
            leaving,
            scale: 2,
            classic,
            levels: false,
        };
        assert!(scales_enabled(&state(false, false, true)));
        assert!(!scales_enabled(&state(true, false, true)));
        assert!(!scales_enabled(&state(false, false, false)));
        assert!(
            !scales_enabled(&state(false, true, true)),
            "not during View ▸ Exit Full Screen's animation"
        );
    }

    #[test]
    fn the_look_and_levels_shortcuts_are_the_specs() {
        assert_eq!((LOOK_KEY, LEVELS_KEY), ("Alt+Cmd+L", "Cmd+L"));
    }
}
