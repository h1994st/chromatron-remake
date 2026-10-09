//! Mouse buttons and keys as browsers report them (`PointerEvent.button`, `KeyboardEvent.key`),
//! turned into what `lumen::gesture` expects.

use lumen::{Button, key_byte, typed_text};

/// `PointerEvent.button`: 0 is the primary (left) button, 2 the secondary (right) one; the rest
/// (middle, back, forward, none) count as the original's other buttons.
pub fn pointer_button(button: i16) -> Button {
    match button {
        0 => Button::Primary,
        2 => Button::Secondary,
        _ => Button::Other,
    }
}

/// The byte `handle_key` gets for a `keydown`, if any. Without Control, `key` is the typed text
/// and `key_byte`'s rules apply. With Control, browsers keep their shortcuts (Ctrl+R reloads on
/// Windows and Linux), so a letter types its control character only when that is the game's
/// cheat key: Chromatron 4's Control-O (0x0F), as `typed_text` decides for the app.
pub fn browser_key_byte(
    key: &str,
    ctrl: bool,
    meta: bool,
    repeat: bool,
    cheat_key: u8,
) -> Option<u8> {
    if !ctrl {
        return key_byte(Some(key), meta, repeat);
    }
    let [letter] = key.as_bytes() else {
        return None;
    };
    if !letter.is_ascii_alphabetic() {
        return None;
    }
    let control = [letter.to_ascii_uppercase() & 0x1f];
    let mac_chars = std::str::from_utf8(&control).ok();
    key_byte(typed_text(None, mac_chars, cheat_key), meta, repeat)
}

#[cfg(test)]
mod tests {
    use super::*;

    const C1_CHEAT: u8 = b'L';
    const C4_CHEAT: u8 = 0x0f;

    #[test]
    fn buttons_are_left_right_and_the_rest() {
        assert_eq!(pointer_button(0), Button::Primary);
        assert_eq!(pointer_button(2), Button::Secondary);
        for other in [1, 3, 4, -1] {
            assert_eq!(pointer_button(other), Button::Other, "{other}");
        }
    }

    #[test]
    fn typed_characters_reach_the_game() {
        assert_eq!(
            browser_key_byte(" ", false, false, false, C1_CHEAT),
            Some(b' ')
        );
        assert_eq!(
            browser_key_byte("=", false, false, false, C1_CHEAT),
            Some(b'=')
        );
        assert_eq!(
            browser_key_byte("L", false, false, false, C1_CHEAT),
            Some(b'L')
        );
        assert_eq!(
            browser_key_byte("r", false, false, false, C1_CHEAT),
            Some(b'r')
        );
    }

    #[test]
    fn named_keys_non_ascii_command_and_repeats_do_not() {
        for key in ["Enter", "Escape", "ArrowLeft", "Shift", "é", "ø"] {
            assert_eq!(
                browser_key_byte(key, false, false, false, C1_CHEAT),
                None,
                "{key}"
            );
        }
        assert_eq!(
            browser_key_byte("r", false, true, false, C1_CHEAT),
            None,
            "⌘R"
        );
        assert_eq!(
            browser_key_byte("=", false, false, true, C1_CHEAT),
            None,
            "repeat"
        );
    }

    #[test]
    fn control_chords_stay_the_browsers_except_the_control_o_cheat() {
        assert_eq!(
            browser_key_byte("r", true, false, false, C1_CHEAT),
            None,
            "Ctrl+R reloads"
        );
        assert_eq!(browser_key_byte("r", true, false, false, C4_CHEAT), None);
        assert_eq!(
            browser_key_byte("o", true, false, false, C1_CHEAT),
            None,
            "not C1's cheat"
        );
        assert_eq!(
            browser_key_byte("o", true, false, false, C4_CHEAT),
            Some(0x0f)
        );
        assert_eq!(
            browser_key_byte("O", true, false, false, C4_CHEAT),
            Some(0x0f),
            "with Shift"
        );
        assert_eq!(
            browser_key_byte("o", true, true, false, C4_CHEAT),
            None,
            "⌘⌃O"
        );
        assert_eq!(
            browser_key_byte("o", true, false, true, C4_CHEAT),
            None,
            "repeat"
        );
        assert_eq!(browser_key_byte("@", true, false, false, C4_CHEAT), None);
        assert_eq!(
            browser_key_byte("Control", true, false, false, C4_CHEAT),
            None
        );
    }
}
