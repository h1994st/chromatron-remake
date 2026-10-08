//! The menu bar: the original nib's menus (platform.md P5), the standard application menu, and
//! the View menu for the modern touches (fullscreen and scale).

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

/// The installed menu bar, kept alive for the app's lifetime, and the items whose state changes.
pub struct MenuBar {
    _menu: Menu,
    wheel: CheckMenuItem,
    fullscreen: MenuItem,
    scales: Vec<CheckMenuItem>,
}

impl MenuBar {
    /// Builds the menu bar and makes it the app's main menu. Main thread only, after the event
    /// loop exists. macOS titles the application menu with the bundle name itself.
    pub fn install(wheel_on: bool) -> MenuBar {
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
            &[&fullscreen, &PredefinedMenuItem::separator()],
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
            scales,
        }
    }

    pub fn set_wheel_checked(&self, on: bool) {
        self.wheel.set_checked(on);
    }

    /// Shows the window's state: the fullscreen item's title, and a check on the current scale.
    /// Scales can't be picked in fullscreen.
    pub fn show_view(&self, fullscreen: bool, scale: usize) {
        self.fullscreen.set_text(if fullscreen {
            EXIT_FULLSCREEN
        } else {
            ENTER_FULLSCREEN
        });
        for (item, n) in self.scales.iter().zip(MIN_SCALE..) {
            item.set_checked(!fullscreen && n as usize == scale);
            item.set_enabled(!fullscreen);
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
        ] {
            assert!(text.parse::<Accelerator>().is_ok(), "{text}");
        }
    }
}
