//! The preferences, in NSUserDefaults under the app's bundle id: the original's
//! `UseMouseWheelToChangeLevel` (platform.md P6), our `LastLevelSlot` (the level to reopen on) and
//! `ClassicLook` (spec §11; modern touches, not part of the original save file).

use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2_foundation::{NSBundle, NSString, NSUserDefaults};

use crate::look_switch::{Look, look_setting};

/// The original's preference key (platform.md P6).
pub const WHEEL_KEY: &str = "UseMouseWheelToChangeLevel";

/// The strip slot (0-based) the player was last on.
pub const LAST_SLOT_KEY: &str = "LastLevelSlot";

/// Whether the player chose the classic look (spec §11); absent means modern.
pub const CLASSIC_LOOK_KEY: &str = "ClassicLook";

pub struct Preferences {
    defaults: Retained<NSUserDefaults>,
}

impl Preferences {
    /// The defaults domain `domain`. Inside the bundle whose id is `domain` that is the standard
    /// defaults; run from `cargo` (no bundle) it is the same domain opened as a suite, so both
    /// share one preference.
    pub fn open(domain: &str) -> Preferences {
        let bundle_id = NSBundle::mainBundle().bundleIdentifier();
        let defaults = if bundle_id.is_some_and(|id| id.to_string() == domain) {
            NSUserDefaults::standardUserDefaults()
        } else {
            NSUserDefaults::initWithSuiteName(
                NSUserDefaults::alloc(),
                Some(&NSString::from_str(domain)),
            )
            .unwrap_or_else(NSUserDefaults::standardUserDefaults)
        };
        Preferences { defaults }
    }

    /// Whether the wheel changes level.
    pub fn wheel_changes_level(&self) -> bool {
        let key = NSString::from_str(WHEEL_KEY);
        let stored = self
            .defaults
            .objectForKey(&key)
            .map(|_| self.defaults.boolForKey(&key));
        wheel_setting(stored)
    }

    pub fn set_wheel_changes_level(&self, on: bool) {
        self.defaults
            .setBool_forKey(on, &NSString::from_str(WHEEL_KEY));
    }

    /// The stored level slot; absent, negative or non-numeric values are `None`.
    pub fn last_level_slot(&self) -> Option<usize> {
        let key = NSString::from_str(LAST_SLOT_KEY);
        self.defaults.objectForKey(&key)?;
        usize::try_from(self.defaults.integerForKey(&key)).ok()
    }

    pub fn set_last_level_slot(&self, slot: usize) {
        let slot = isize::try_from(slot).unwrap_or(0);
        self.defaults
            .setInteger_forKey(slot, &NSString::from_str(LAST_SLOT_KEY));
    }

    /// The stored `ClassicLook`, `None` when never written.
    pub fn classic_look(&self) -> Option<bool> {
        let key = NSString::from_str(CLASSIC_LOOK_KEY);
        self.defaults
            .objectForKey(&key)
            .map(|_| self.defaults.boolForKey(&key))
    }

    pub fn set_classic_look(&self, on: bool) {
        self.defaults
            .setBool_forKey(on, &NSString::from_str(CLASSIC_LOOK_KEY));
    }

    /// The look to open in.
    pub fn look(&self) -> Look {
        look_setting(self.classic_look())
    }
}

/// A missing (or never written) value means on, as in the original (`load_prefs`, platform.md P6).
fn wheel_setting(stored: Option<bool>) -> bool {
    stored.unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_classic_look_key_is_the_specs() {
        assert_eq!(CLASSIC_LOOK_KEY, "ClassicLook");
    }

    #[test]
    fn the_wheel_preference_is_on_unless_stored_off() {
        assert!(wheel_setting(None));
        assert!(wheel_setting(Some(true)));
        assert!(!wheel_setting(Some(false)));
    }
}
