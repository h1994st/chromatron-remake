//! The one preference, `UseMouseWheelToChangeLevel` (platform.md P6), in NSUserDefaults under the
//! app's bundle id.

use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2_foundation::{NSBundle, NSString, NSUserDefaults};

/// The original's preference key (platform.md P6).
pub const WHEEL_KEY: &str = "UseMouseWheelToChangeLevel";

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
}

/// A missing (or never written) value means on, as in the original (`load_prefs`, platform.md P6).
fn wheel_setting(stored: Option<bool>) -> bool {
    stored.unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wheel_preference_is_on_unless_stored_off() {
        assert!(wheel_setting(None));
        assert!(wheel_setting(Some(true)));
        assert!(!wheel_setting(Some(false)));
    }
}
