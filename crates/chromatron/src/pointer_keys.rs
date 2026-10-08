//! Mouse buttons and typed keys as the original read them (platform.md P4, logic.md L7).

/// A mouse button as the original's handler distinguishes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// Button 1 (left).
    Primary,
    /// Button 2 (right).
    Secondary,
    /// Middle and extra buttons. The original treats them as primary and never checks modifiers.
    Other,
}

/// Modifier keys held when a button goes down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub shift: bool,
    pub option: bool,
    pub control: bool,
}

/// `mouse_alt_button`: whether a gesture is the alternate (rotate clockwise) kind. Latched on a
/// primary press from Shift, Option or Control (Ctrl-click is a right click), and reused for the
/// matching release.
#[derive(Debug, Default)]
pub struct ButtonLatch {
    alt: bool,
}

impl ButtonLatch {
    /// `alt` for a button going down.
    pub fn press(&mut self, button: Button, held: Held) -> bool {
        match button {
            Button::Primary => {
                self.alt = held.shift || held.option || held.control;
                self.alt
            }
            Button::Secondary => true,
            Button::Other => false,
        }
    }

    /// `alt` for a button going up.
    pub fn release(&self, button: Button) -> bool {
        match button {
            Button::Primary => self.alt,
            Button::Secondary => true,
            Button::Other => false,
        }
    }
}

/// The byte `handle_key` gets for a key press, if any: the typed character when it is one ASCII
/// byte, with Command up (Command keys belong to the menus) and not a key repeat (the original
/// doesn't register repeats).
pub fn key_byte(text: Option<&str>, command: bool, repeat: bool) -> Option<u8> {
    if command || repeat {
        return None;
    }
    match text?.as_bytes() {
        [byte] if byte.is_ascii() => Some(*byte),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Held = Held {
        shift: false,
        option: false,
        control: false,
    };

    #[test]
    fn a_plain_left_click_is_primary_and_a_right_click_is_alternate() {
        let mut latch = ButtonLatch::default();
        assert!(!latch.press(Button::Primary, NONE));
        assert!(!latch.release(Button::Primary));
        assert!(latch.press(Button::Secondary, NONE));
        assert!(latch.release(Button::Secondary));
    }

    #[test]
    fn shift_option_or_control_click_is_alternate_through_the_release() {
        for held in [
            Held {
                shift: true,
                ..NONE
            },
            Held {
                option: true,
                ..NONE
            },
            Held {
                control: true,
                ..NONE
            },
        ] {
            let mut latch = ButtonLatch::default();
            assert!(latch.press(Button::Primary, held), "{held:?}");
            // The modifier may be let go before the release; the latch keeps the gesture's kind.
            assert!(latch.release(Button::Primary), "{held:?}");
        }
    }

    #[test]
    fn other_buttons_are_primary_whatever_is_held() {
        let mut latch = ButtonLatch::default();
        let held = Held {
            shift: true,
            ..NONE
        };
        assert!(!latch.press(Button::Other, held));
        assert!(!latch.release(Button::Other));
    }

    #[test]
    fn a_right_press_does_not_change_the_primary_latch() {
        let mut latch = ButtonLatch::default();
        latch.press(
            Button::Primary,
            Held {
                control: true,
                ..NONE
            },
        );
        latch.press(Button::Secondary, NONE);
        assert!(latch.release(Button::Primary));
    }

    #[test]
    fn keys_are_single_ascii_characters_without_command_or_repeat() {
        assert_eq!(key_byte(Some(" "), false, false), Some(b' '));
        assert_eq!(key_byte(Some("L"), false, false), Some(b'L'));
        assert_eq!(key_byte(Some("+"), false, false), Some(b'+'));
        assert_eq!(key_byte(Some("\u{1b}"), false, false), Some(0x1b));
        assert_eq!(key_byte(Some("r"), true, false), None);
        assert_eq!(key_byte(Some("r"), false, true), None);
        assert_eq!(key_byte(Some("é"), false, false), None);
        assert_eq!(key_byte(Some("ab"), false, false), None);
        assert_eq!(key_byte(None, false, false), None);
    }
}
